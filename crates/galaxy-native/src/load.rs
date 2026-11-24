//! The tool `load_view`: the document of a source, or the kept document of a log entry, read and
//! checked, and the bytes of each read document kept in the directory of the kept documents.

use std::path::Path;

use galaxy_scene::wire::{self, WireRefusal};
use galaxy_scene::{Change, Code, Framing, Json, Load, LoadRefusal, sha256};

use crate::fetch::{self, refusal};
use crate::sources::{DOCUMENTS, Sources};

/// A refused load: the lines of the refusals, and the lines for the standard error with the
/// place and the refused value of each refusal that has one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Refused {
    pub lines: Vec<String>,
    pub stderr: Vec<String>,
}

impl Refused {
    fn new(refusals: Vec<LoadRefusal>) -> Self {
        Self {
            lines: refusals.iter().map(ToString::to_string).collect(),
            stderr: refusals
                .iter()
                .filter_map(|refusal| {
                    let value = refusal.value.as_ref()?;
                    Some(format!("{} {}: {value}", refusal.at, refusal.member))
                })
                .collect(),
        }
    }
}

impl From<LoadRefusal> for Refused {
    fn from(refusal: LoadRefusal) -> Self {
        Self::new(vec![refusal])
    }
}

/// Where the arguments of `load_view` come from: a call of the tool, or a line of an operation
/// file that replays a log entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Call,
    Replay,
}

/// The load of the arguments of `load_view`. A call takes `source` and `query` and reads the
/// answer of the source, whose bytes stay in the directory of the kept documents. A replay with
/// `sha256` reads the kept document of that hash of a source of the configuration, with no
/// request to the source.
pub fn read(sources: Option<&Sources>, arguments: &Json, origin: Origin) -> Result<Load, Refused> {
    let (source, query, hash) = match wire::decode("load_view", arguments) {
        Ok(Change::LoadView {
            source,
            query,
            sha256,
        }) => (source, query, sha256),
        Ok(_) => unreachable!("load_view decodes to LoadView"),
        Err(WireRefusal::Value {
            field: "source", ..
        }) => return Err(no_source(sources).into()),
        Err(WireRefusal::Missing { field }) => {
            return Err(refusal(field, Code::Missing, "source, query").into());
        }
        Err(WireRefusal::Type { field, expected }) => {
            return Err(refusal(field, Code::Type, expected).into());
        }
        Err(WireRefusal::Value {
            field,
            value,
            valid,
        }) => {
            return Err(Refused::new(vec![LoadRefusal {
                value: Some(value),
                ..refusal(field, Code::Value, valid)
            }]));
        }
        Err(other) => return Err(refusal("arguments", Code::Value, other.to_string()).into()),
    };
    if hash.is_some() && origin == Origin::Call {
        return Err(refusal("sha256", Code::Value, "source and query").into());
    }
    let Some((sources, configured)) =
        sources.and_then(|sources| Some((sources, sources.get(&source)?)))
    else {
        return Err(no_source(sources).into());
    };
    if let Some(hash) = hash {
        let bytes = kept(&sources.documents(), &hash)?;
        return Load::read(&source, &query, &bytes, Framing::Kept).map_err(Refused::new);
    }
    let fetched = fetch::fetch(sources, configured, &query)?;
    let read = Load::read(&source, &query, &fetched.bytes, fetched.framing());
    if holds_credential(&read, &fetched.credentials) {
        return Err(fetch::no_credential().into());
    }
    let load = read.map_err(Refused::new)?;
    keep(&sources.documents(), load.hash(), &fetched.bytes)?;
    Ok(load)
}

/// The refusal of a source that is not a source of `sources`, with the names of the sources.
fn no_source(sources: Option<&Sources>) -> LoadRefusal {
    let valid = match sources.map(Sources::names) {
        Some(names) if !names.is_empty() => names,
        _ => "a source of --sources <file>".to_owned(),
    };
    refusal("source", Code::NoSource, valid)
}

/// `true` when a text that a read load shows holds 1 of `credentials`: the answer, the names,
/// the kinds, the parents, the paths, the outcomes and the fields of the relations, or the text
/// and the refused value of each refusal.
fn holds_credential(read: &Result<Load, Vec<LoadRefusal>>, credentials: &[String]) -> bool {
    if credentials.is_empty() {
        return false;
    }
    let mut text = String::new();
    match read {
        Ok(load) => {
            text.push_str(&load.answer().to_json().to_string());
            let relations = load.relations();
            for node in relations.nodes() {
                text.push_str(&format!("\n{} {}", node.id, node.kind));
                if let Some(parent) = &node.parent {
                    text.push_str(&format!(" {parent}"));
                }
                for (name, value) in &node.fields {
                    text.push_str(&format!(" {name} {value}"));
                }
            }
            for flow in relations.flows() {
                text.push_str(&format!("\n{}", flow.id));
                for node in &flow.path {
                    text.push_str(&format!(" {node}"));
                }
                if let Some(reason) = flow
                    .outcome
                    .as_ref()
                    .and_then(|outcome| outcome.reason.as_ref())
                {
                    text.push_str(&format!(" {reason}"));
                }
                for (name, value) in &flow.fields {
                    text.push_str(&format!(" {name} {value}"));
                }
            }
        }
        Err(refusals) => {
            for refusal in refusals {
                text.push_str(&format!(
                    "\n{refusal} {}",
                    refusal.value.as_deref().unwrap_or("")
                ));
            }
        }
    }
    fetch::holds(text.as_bytes(), credentials)
}

/// The bytes of the kept document `hash` in `dir`, checked against the hash.
fn kept(dir: &Path, hash: &str) -> Result<Vec<u8>, LoadRefusal> {
    let name = format!("{DOCUMENTS}/{hash}");
    let bytes = std::fs::read(dir.join(hash))
        .map_err(|_| refusal("sha256", Code::Source, format!("the kept file {name}")))?;
    if sha256::hex(&bytes) != hash {
        return Err(refusal(
            "sha256",
            Code::Source,
            format!("the kept file {name} with this SHA-256"),
        ));
    }
    Ok(bytes)
}

/// Writes `bytes` to the file `hash` in `dir`, unless the file holds them.
fn keep(dir: &Path, hash: &str, bytes: &[u8]) -> Result<(), LoadRefusal> {
    let path = dir.join(hash);
    if std::fs::read(&path).is_ok_and(|kept| sha256::hex(&kept) == hash) {
        return Ok(());
    }
    let part = dir.join(format!("{hash}.part"));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&part, bytes))
        .and_then(|()| std::fs::rename(&part, &path))
        .map_err(|error| {
            refusal(
                DOCUMENTS,
                Code::Source,
                format!("a writable directory {DOCUMENTS}: {error}"),
            )
        })
}
