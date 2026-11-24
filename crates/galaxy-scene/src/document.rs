//! The reader of the relations format: a document, or a stream text with all its pages joined in
//! order, into the relations of a world and the answer of the load.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use crate::id::ObjectId;
use crate::json::Json;
use crate::sha256;
use crate::world::{Outcome, OutcomeKind, RelationRefusal, Relations};

/// The versions that the reader reads.
pub const VERSIONS: &[&str] = &["0.1"];
/// Largest number of refusals of a refused load.
pub const REFUSALS: usize = 20;
/// Largest number of bytes of a document as 1 JSON object.
pub const DOCUMENT_BYTES: usize = 67_108_864;
/// Largest number of bytes of the JSON text of 1 record and of 1 line of a stream.
pub const RECORD_BYTES: usize = 1_048_576;
/// Largest number of nodes, links and flows of a document.
pub const RECORDS: [usize; 3] = [1_000_000, 1_000_000, 100_000];
/// Largest number of kinds of the nodes of a document.
pub const KINDS: usize = 256;
/// Largest number of members of `fields`.
pub const FIELDS: usize = 64;
/// Largest number of items of `partial`.
pub const PARTIAL: usize = 64;
/// Smallest and largest number of nodes of a path.
pub const PATH: [usize; 2] = [2, 1024];

/// The valid text of a name.
const NAME_VALID: &str = "1 to 64 ASCII letters, digits, -, _ and .";
/// The valid text of a kind.
const KIND_VALID: &str = "1 to 64 ASCII letters, digits, -, _ and ., not root, not default";
/// The valid text of a time.
const TIME_VALID: &str = "RFC 3339 time in UTC with Z";
/// The largest integer that a number holds.
const INTEGER: f64 = 9_007_199_254_740_991.0;
/// The units of a magnitude.
const UNITS: &[&str] = &["bit/s", "packet/s", "byte", "packet", "flow"];
/// The count units of a magnitude.
const COUNT_UNITS: &[&str] = &["byte", "packet", "flow"];
/// The byte order mark of UTF-8.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
/// The values of `missing` of a field.
const MISSING: &[&str] = &["not_available", "not_known"];
/// The valid text of the kind of a line.
const LINES: &str = "header, node, link, flow, next, abort, end";
/// The valid text of a last line with no LF.
const LF_VALID: &str = "LF at the end of the line";
/// The valid text of a `next` line that is not the last line of a page.
const NEXT_VALID: &str = "next as the last line of a page";
/// The valid text of a page with a page after it that does not end in a line break.
const BREAK_VALID: &str = "a page that ends in a line break";
/// The valid text of an `abort` line with no valid `reason`.
const NO_ABORT: &str = "no abort line";
/// Largest number of items of `ignored` of the answer.
pub const IGNORED: usize = 64;
/// The kinds of the lines of a stream.
const LINE_KINDS: [&str; 7] = ["header", "node", "link", "flow", "next", "abort", "end"];
/// The classes of the records.
const CLASSES: [&str; 3] = ["node", "link", "flow"];
/// The arrays of a document of each class of `CLASSES`.
const CLASS_ARRAYS: [&str; 3] = ["nodes", "links", "flows"];

/// The members of the document that the engine does not show, in the order of the table of the
/// engine operations.
pub const NOT_SHOWN: &[&str] = &[
    "node.fields.missing",
    "node.label",
    "node.handle",
    "link",
    "flow.fields.missing",
    "flow.hops",
    "flow.magnitude",
    "flow.group",
    "flow.reply_to",
    "flow.handle",
];

/// The cause of a refused load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Code {
    Json,
    Format,
    Version,
    Missing,
    Type,
    Value,
    Size,
    Exists,
    NoNode,
    NoFlow,
    ShortPath,
    Repeat,
    Unit,
    Window,
    Header,
    Line,
    NoEnd,
    Abort,
    Count,
    NoSource,
    Source,
    Credential,
}

impl Code {
    /// The codes in the order of their index.
    pub const ALL: [Code; 22] = [
        Code::Json,
        Code::Format,
        Code::Version,
        Code::Missing,
        Code::Type,
        Code::Value,
        Code::Size,
        Code::Exists,
        Code::NoNode,
        Code::NoFlow,
        Code::ShortPath,
        Code::Repeat,
        Code::Unit,
        Code::Window,
        Code::Header,
        Code::Line,
        Code::NoEnd,
        Code::Abort,
        Code::Count,
        Code::NoSource,
        Code::Source,
        Code::Credential,
    ];

    /// The names of the codes in the order of `ALL`.
    pub const NAMES: &'static [&'static str] = &[
        "json",
        "format",
        "version",
        "missing",
        "type",
        "value",
        "size",
        "exists",
        "no_node",
        "no_flow",
        "short_path",
        "repeat",
        "unit",
        "window",
        "header",
        "line",
        "no_end",
        "abort",
        "count",
        "no_source",
        "source",
        "credential",
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES[self as usize]
    }
}

/// A refusal of a load: the place, the member, the cause and the valid values. `value` holds the
/// refused value as JSON text for the standard error, and is no part of the text of the refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadRefusal {
    pub at: String,
    pub member: String,
    pub code: Code,
    pub valid: String,
    pub value: Option<String>,
}

/// `<at> <member>: <code>, valid: <valid>`.
impl fmt::Display for LoadRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {}: {}, valid: {}",
            self.at,
            self.member,
            self.code.name(),
            self.valid
        )
    }
}

/// The member `source` of a document, without `query`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceData {
    pub name: String,
    pub version: Option<String>,
    pub snapshot: String,
    pub time: Option<String>,
    /// `from` and `to`.
    pub window: Option<[String; 2]>,
}

/// A kind of the nodes of a document: the count of its nodes and the names of their fields in
/// name order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindCount {
    pub kind: String,
    pub nodes: usize,
    pub fields: Vec<String>,
}

/// The answer of a load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    pub source: SourceData,
    pub nodes: usize,
    pub links: usize,
    pub flows: usize,
    /// The kinds in name order.
    pub kinds: Vec<KindCount>,
    /// The unit of the magnitudes, for a document with a magnitude.
    pub unit: Option<String>,
    /// `part` and `reason` of each item of `partial`.
    pub partial: Vec<[String; 2]>,
    /// The members and the lines that the version does not define, in name order.
    pub ignored: Vec<String>,
    /// The members of `NOT_SHOWN` that the document holds, in the order of `NOT_SHOWN`.
    pub not_shown: Vec<String>,
}

impl Answer {
    pub fn to_json(&self) -> Json {
        let strings = |items: &[String]| Json::Array(items.iter().map(Json::string).collect());
        let mut source = vec![("name".to_owned(), Json::string(&self.source.name))];
        if let Some(version) = &self.source.version {
            source.push(("version".to_owned(), Json::string(version)));
        }
        source.push(("snapshot".to_owned(), Json::string(&self.source.snapshot)));
        if let Some(time) = &self.source.time {
            source.push(("time".to_owned(), Json::string(time)));
        }
        if let Some([from, to]) = &self.source.window {
            source.push((
                "window".to_owned(),
                Json::object([("from", Json::string(from)), ("to", Json::string(to))]),
            ));
        }
        let mut members = vec![
            ("source".to_owned(), Json::Object(source)),
            ("nodes".to_owned(), Json::Number(self.nodes as f64)),
            ("links".to_owned(), Json::Number(self.links as f64)),
            ("flows".to_owned(), Json::Number(self.flows as f64)),
            (
                "kinds".to_owned(),
                Json::Array(
                    self.kinds
                        .iter()
                        .map(|kind| {
                            Json::object([
                                ("kind", Json::string(&kind.kind)),
                                ("nodes", Json::Number(kind.nodes as f64)),
                                ("fields", strings(&kind.fields)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ];
        if let Some(unit) = &self.unit {
            members.push(("unit".to_owned(), Json::string(unit)));
        }
        members.extend([
            (
                "partial".to_owned(),
                Json::Array(
                    self.partial
                        .iter()
                        .map(|[part, reason]| {
                            Json::object([
                                ("part", Json::string(part)),
                                ("reason", Json::string(reason)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("ignored".to_owned(), strings(&self.ignored)),
            ("not_shown".to_owned(), strings(&self.not_shown)),
        ]);
        Json::Object(members)
    }
}

/// A read document of a source: the name of the source, the query, the SHA-256 hash of the bytes
/// in lowercase hex, the relations of the records and the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Load {
    source: String,
    query: String,
    hash: String,
    relations: Arc<Relations>,
    answer: Arc<Answer>,
}

impl Load {
    /// Reads `bytes`, the answer of the source `source` to `query`, with `framing`. A refused text
    /// gives its first `REFUSALS` refusals in the order of the text.
    pub fn read(
        source: &str,
        query: &str,
        bytes: &[u8],
        framing: Framing,
    ) -> Result<Self, Vec<LoadRefusal>> {
        let (relations, answer) = read(bytes, framing)?;
        Ok(Self {
            source: source.to_owned(),
            query: query.to_owned(),
            hash: sha256::hex(bytes),
            relations: Arc::new(relations),
            answer: Arc::new(answer),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The SHA-256 hash of the bytes in 64 lowercase hex digits.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    pub fn answer(&self) -> &Answer {
        &self.answer
    }
}

/// The framing of a text: 1 text that is a document or a stream by its content, the document of
/// a `.json` file, the stream of a `.jsonl` file, the pages of the MCP transport with the byte
/// offset of the end of each page in the joined text, or the pages that the `next` lines end in
/// a kept text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Framing<'a> {
    #[default]
    Text,
    Document,
    Stream,
    Pages(&'a [usize]),
    Kept,
}

/// The relations and the answer of a document or a stream text with `framing`. A refused text
/// gives its first `REFUSALS` refusals in the order of the text.
pub fn read(bytes: &[u8], framing: Framing) -> Result<(Relations, Answer), Vec<LoadRefusal>> {
    let mut reader = Reader {
        framing,
        ..Reader::default()
    };
    let line = |bytes: &[u8]| {
        let bytes = bytes.strip_prefix(BOM).unwrap_or(bytes);
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| Json::parse(text).ok())
    };
    let is_line = |value: &Json| {
        value
            .as_object()
            .is_some_and(|members| members.len() == 1 && members[0].0 != "format")
    };
    let end = bytes
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(bytes.len());
    let first = line(&bytes[..end]);
    let stream = match framing {
        Framing::Document => false,
        Framing::Stream => true,
        _ => first
            .as_ref()
            .is_some_and(|value| value.get("header").is_some() || is_line(value)),
    };
    match &first {
        _ if stream => reader.stream(bytes),
        _ if bytes.len() > DOCUMENT_BYTES => reader.refuse(
            "document",
            "text",
            Code::Size,
            format!("{DOCUMENT_BYTES} bytes"),
            None,
        ),
        _ => match std::str::from_utf8(bytes) {
            Err(error) => reader.refuse(
                "document",
                &format!("byte {}", error.valid_up_to()),
                Code::Json,
                "JSON of RFC 8259 in UTF-8".to_owned(),
                None,
            ),
            Ok(text) => match Json::parse(text) {
                Ok(value) => reader.document(&value),
                Err(_)
                    if framing != Framing::Document
                        && bytes.ends_with(b"\n")
                        && bytes
                            .split(|&byte| byte == b'\n')
                            .any(|text| line(text).is_some_and(|value| is_line(&value))) =>
                {
                    reader.stream(bytes)
                }
                Err(error) => reader.refuse(
                    "document",
                    &format!("byte {}", error.offset),
                    Code::Json,
                    "JSON of RFC 8259 in UTF-8".to_owned(),
                    None,
                ),
            },
        },
    }
    reader.finish()
}

/// The class of an `id`. A flow with `true` has a `reply_to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Node,
    Link,
    Flow(bool),
}

#[derive(Default)]
struct Reader<'a> {
    framing: Framing<'a>,
    refusals: Vec<LoadRefusal>,
    relations: Relations,
    ids: BTreeMap<String, Class>,
    kinds: BTreeMap<String, (usize, BTreeSet<String>)>,
    source: SourceData,
    /// The place of `source`, and the count of the refusals up to the end of `source`.
    source_at: String,
    source_end: usize,
    window: bool,
    window_refused: bool,
    unit: Option<String>,
    partial: Vec<[String; 2]>,
    ignored: BTreeSet<String>,
    not_shown: [bool; 10],
    /// The records of each class of `CLASSES`.
    records: [usize; 3],
}

/// `prefix.name`, or `name` for an empty prefix.
fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}.{name}")
    }
}

impl Reader<'_> {
    fn full(&self) -> bool {
        self.refusals.len() >= REFUSALS
    }

    fn refuse(&mut self, at: &str, member: &str, code: Code, valid: String, value: Option<&Json>) {
        if self.full() {
            return;
        }
        self.refusals.push(LoadRefusal {
            at: at.to_owned(),
            member: member.to_owned(),
            code,
            valid,
            value: value.map(Json::to_string),
        });
    }

    fn not_shown(&mut self, name: &str) {
        if let Some(index) = NOT_SHOWN.iter().position(|shown| *shown == name) {
            self.not_shown[index] = true;
        }
    }

    /// The relations and the answer, or the first `REFUSALS` refusals with the refusal of a count
    /// unit with no window at the place of `source` after its refusals.
    fn finish(mut self) -> Result<(Relations, Answer), Vec<LoadRefusal>> {
        if self.window_refused {
            let refusal = LoadRefusal {
                at: self.source_at.clone(),
                member: "source.window".to_owned(),
                code: Code::Window,
                valid: "from and to, from not after to".to_owned(),
                value: None,
            };
            let place = self.source_end.min(self.refusals.len());
            self.refusals.insert(place, refusal);
            self.refusals.truncate(REFUSALS);
        }
        if !self.refusals.is_empty() {
            return Err(self.refusals);
        }
        let answer = Answer {
            source: self.source,
            nodes: self.records[0],
            links: self.records[1],
            flows: self.records[2],
            kinds: self
                .kinds
                .into_iter()
                .map(|(kind, (nodes, fields))| KindCount {
                    kind,
                    nodes,
                    fields: fields.into_iter().collect(),
                })
                .collect(),
            unit: self.unit,
            partial: self.partial,
            ignored: self.ignored.into_iter().collect(),
            not_shown: NOT_SHOWN
                .iter()
                .zip(self.not_shown)
                .filter(|(_, shown)| *shown)
                .map(|(name, _)| (*name).to_owned())
                .collect(),
        };
        Ok((self.relations, answer))
    }

    /// Refuses a value that is not an object; `true` for an object.
    fn object(&mut self, at: &str, member: &str, value: &Json) -> bool {
        if value.as_object().is_none() {
            self.refuse(at, member, Code::Type, "object".to_owned(), Some(value));
            return false;
        }
        true
    }

    /// Refuses each missing member of `required` and adds each member outside `known` to the
    /// ignored members under `ignore`. `true` with each required member.
    fn members(
        &mut self,
        at: &str,
        prefix: &str,
        ignore: &str,
        value: &Json,
        known: &[&str],
        required: &[&str],
    ) -> bool {
        let mut complete = true;
        for name in required {
            if value.get(name).is_none() {
                self.refuse(
                    at,
                    &join(prefix, name),
                    Code::Missing,
                    required.join(", "),
                    None,
                );
                complete = false;
            }
        }
        for (name, _) in value.as_object().unwrap_or_default() {
            if !known.contains(&name.as_str()) && ObjectId::new(name).is_some() {
                self.ignore(join(ignore, name));
            }
        }
        complete
    }

    /// Adds `name` to the ignored members and lines, and keeps the first `IGNORED` in name
    /// order.
    fn ignore(&mut self, name: String) {
        if self.ignored.len() >= IGNORED && self.ignored.last().is_some_and(|last| name >= *last) {
            return;
        }
        self.ignored.insert(name);
        if self.ignored.len() > IGNORED {
            self.ignored.pop_last();
        }
    }

    fn string<'a>(&mut self, at: &str, member: &str, value: &'a Json) -> Option<&'a str> {
        match value {
            Json::String(text) => Some(text),
            _ => {
                self.refuse(at, member, Code::Type, "string".to_owned(), Some(value));
                None
            }
        }
    }

    /// A string by the name rule.
    fn name<'a>(&mut self, at: &str, member: &str, value: &'a Json) -> Option<&'a str> {
        let text = self.string(at, member, value)?;
        if ObjectId::new(text).is_none() {
            self.refuse(at, member, Code::Value, NAME_VALID.to_owned(), Some(value));
            return None;
        }
        Some(text)
    }

    /// A string by the name rule other than root and default.
    fn kind<'a>(&mut self, at: &str, member: &str, value: &'a Json) -> Option<&'a str> {
        let text = self.string(at, member, value)?;
        if ObjectId::new(text).is_none() || text == "root" || text == "default" {
            self.refuse(at, member, Code::Value, KIND_VALID.to_owned(), Some(value));
            return None;
        }
        Some(text)
    }

    /// A string of `fewest` to `most` characters.
    fn text<'a>(
        &mut self,
        at: &str,
        member: &str,
        value: &'a Json,
        fewest: usize,
        most: usize,
    ) -> Option<&'a str> {
        let text = self.string(at, member, value)?;
        let count = text.chars().count();
        if !(fewest..=most).contains(&count) {
            self.refuse(
                at,
                member,
                Code::Size,
                format!("{fewest} to {most} characters"),
                Some(value),
            );
            return None;
        }
        Some(text)
    }

    /// A time of RFC 3339 in UTC with `Z`, as its second and the nanoseconds of its fraction.
    fn time(&mut self, at: &str, member: &str, value: &Json) -> Option<(String, u32)> {
        let text = self.string(at, member, value)?;
        let parsed = parse_time(text);
        if parsed.is_none() {
            self.refuse(at, member, Code::Value, TIME_VALID.to_owned(), Some(value));
        }
        parsed
    }

    /// A number. An integer is from -(2^53 - 1) to 2^53 - 1.
    fn number(&mut self, at: &str, member: &str, value: &Json) -> Option<f64> {
        let Json::Number(number) = value else {
            self.refuse(at, member, Code::Type, "number".to_owned(), Some(value));
            return None;
        };
        if number.fract() == 0.0 && number.abs() > INTEGER {
            self.refuse(
                at,
                member,
                Code::Value,
                "an integer from -9007199254740991 to 9007199254740991, or a string".to_owned(),
                Some(value),
            );
            return None;
        }
        Some(*number)
    }

    /// An integer of 0 to `most`.
    fn count(&mut self, at: &str, member: &str, value: &Json, most: usize) -> Option<usize> {
        let number = self.number(at, member, value)?;
        if number.fract() != 0.0 || number < 0.0 {
            self.refuse(
                at,
                member,
                Code::Value,
                "an integer of 0 or more".to_owned(),
                Some(value),
            );
            return None;
        }
        if number > most as f64 {
            self.refuse(
                at,
                member,
                Code::Size,
                format!("{most} at most"),
                Some(value),
            );
            return None;
        }
        Some(number as usize)
    }

    /// The `id` of a record: a name of no record before it.
    fn id(&mut self, at: &str, value: &Json) -> Option<String> {
        let id = self.name(at, "id", value.get("id")?)?;
        if self.ids.contains_key(id) {
            self.refuse(
                at,
                "id",
                Code::Exists,
                "an id of no record before".to_owned(),
                value.get("id"),
            );
            return None;
        }
        Some(id.to_owned())
    }

    /// The `id` of a node before the record.
    fn node_id(&mut self, at: &str, member: &str, value: &Json) -> Option<ObjectId> {
        let id = self.name(at, member, value)?;
        if self.ids.get(id) != Some(&Class::Node) {
            self.refuse(
                at,
                member,
                Code::NoNode,
                "the id of a node before the record".to_owned(),
                Some(value),
            );
            return None;
        }
        ObjectId::new(id)
    }

    /// The fields in the engine form, the names of all fields, and `true` for a missing value.
    fn fields(
        &mut self,
        at: &str,
        member: &str,
        ignore: &str,
        value: &Json,
    ) -> (Vec<(String, String)>, Vec<String>, bool) {
        let mut fields = Vec::new();
        let mut names = Vec::new();
        let mut missing = false;
        if !self.object(at, member, value) {
            return (fields, names, missing);
        }
        let members = value.as_object().unwrap_or_default();
        if members.len() > FIELDS {
            self.refuse(
                at,
                member,
                Code::Size,
                format!("{FIELDS} members"),
                Some(value),
            );
            return (fields, names, missing);
        }
        for (name, item) in members {
            if ObjectId::new(name).is_none() {
                self.refuse(
                    at,
                    member,
                    Code::Value,
                    format!("member names of {NAME_VALID}"),
                    Some(&Json::string(name)),
                );
                continue;
            }
            let place = format!("{member}.{name}");
            match item {
                Json::String(_) => {
                    if let Some(text) = self.text(at, &place, item, 1, 1024) {
                        fields.push((name.clone(), text.to_owned()));
                    }
                }
                Json::Number(_) => {
                    if self.number(at, &place, item).is_some() {
                        fields.push((name.clone(), item.to_string()));
                    }
                }
                Json::Bool(_) => fields.push((name.clone(), item.to_string())),
                Json::Object(_) => {
                    missing = true;
                    let ignore = format!("{ignore}.missing");
                    if self.members(
                        at,
                        &place,
                        &ignore,
                        item,
                        &["missing", "reason"],
                        &["missing"],
                    ) {
                        let kind = item.get("missing").unwrap_or(&Json::Null);
                        let place = format!("{place}.missing");
                        if let Some(text) = self.string(at, &place, kind)
                            && !MISSING.contains(&text)
                        {
                            self.refuse(at, &place, Code::Value, MISSING.join(", "), Some(kind));
                        }
                    }
                    if let Some(reason) = item.get("reason") {
                        self.text(at, &format!("{place}.reason"), reason, 1, 1024);
                    }
                }
                _ => self.refuse(
                    at,
                    &place,
                    Code::Type,
                    "string, number, boolean or object".to_owned(),
                    Some(item),
                ),
            }
            names.push(name.clone());
        }
        (fields, names, missing)
    }

    /// `partial`, at most `PARTIAL` items of `part` and `reason`.
    fn partial(&mut self, at: &str, member: &str, value: &Json) {
        let Some(items) = value.as_array() else {
            self.refuse(at, member, Code::Type, "array".to_owned(), Some(value));
            return;
        };
        if items.len() > PARTIAL {
            self.refuse(
                at,
                member,
                Code::Size,
                format!("{PARTIAL} items"),
                Some(&Json::Number(items.len() as f64)),
            );
            return;
        }
        for (index, item) in items.iter().enumerate() {
            let place = format!("{member}[{index}]");
            if !self.object(at, &place, item) {
                continue;
            }
            self.members(
                at,
                &place,
                "partial",
                item,
                &["part", "reason"],
                &["part", "reason"],
            );
            let part = item
                .get("part")
                .and_then(|part| self.text(at, &format!("{place}.part"), part, 1, 1024));
            let reason = item
                .get("reason")
                .and_then(|reason| self.text(at, &format!("{place}.reason"), reason, 1, 1024));
            if let (Some(part), Some(reason)) = (part, reason) {
                self.partial.push([part.to_owned(), reason.to_owned()]);
            }
        }
    }

    /// `format` and `version`; `false` for a text in another format or another version.
    fn format(&mut self, at: &str, value: &Json) -> bool {
        let before = self.refusals.len();
        if let Some(format) = value.get("format")
            && let Some(text) = self.string(at, "format", format)
            && text != "galaxy-relations"
        {
            self.refuse(
                at,
                "format",
                Code::Format,
                "galaxy-relations".to_owned(),
                Some(format),
            );
        }
        if let Some(version) = value.get("version")
            && let Some(text) = self.string(at, "version", version)
            && !VERSIONS.contains(&text)
        {
            self.refuse(
                at,
                "version",
                Code::Version,
                VERSIONS.join(", "),
                Some(version),
            );
        }
        self.refusals.len() == before
    }

    fn source(&mut self, at: &str, value: &Json) {
        if !self.object(at, "source", value) {
            return;
        }
        self.members(
            at,
            "source",
            "source",
            value,
            &["name", "version", "snapshot", "time", "window", "query"],
            &["name", "snapshot", "query"],
        );
        if let Some(name) = value.get("name")
            && let Some(name) = self.name(at, "source.name", name)
        {
            self.source.name = name.to_owned();
        }
        if let Some(version) = value.get("version")
            && let Some(version) = self.text(at, "source.version", version, 1, 64)
        {
            self.source.version = Some(version.to_owned());
        }
        if let Some(snapshot) = value.get("snapshot")
            && let Some(snapshot) = self.text(at, "source.snapshot", snapshot, 1, 256)
        {
            self.source.snapshot = snapshot.to_owned();
        }
        if let Some(time) = value.get("time")
            && self.time(at, "source.time", time).is_some()
        {
            self.source.time = time.as_str().map(str::to_owned);
        }
        if let Some(window) = value.get("window")
            && self.object(at, "source.window", window)
        {
            self.window = true;
            self.members(
                at,
                "source.window",
                "source.window",
                window,
                &["from", "to"],
                &["from", "to"],
            );
            let from = window.get("from");
            let to = window.get("to");
            let from_time = from.and_then(|from| self.time(at, "source.window.from", from));
            let to_time = to.and_then(|to| self.time(at, "source.window.to", to));
            if let (Some(first), Some(last)) = (&from_time, &to_time) {
                if first > last {
                    self.refuse(
                        at,
                        "source.window",
                        Code::Window,
                        "from and to, from not after to".to_owned(),
                        Some(window),
                    );
                } else if let (Some(from), Some(to)) =
                    (from.and_then(Json::as_str), to.and_then(Json::as_str))
                {
                    self.source.window = Some([from.to_owned(), to.to_owned()]);
                }
            }
        }
        if let Some(query) = value.get("query") {
            self.text(at, "source.query", query, 0, 8192);
        }
    }

    /// Reads a document: its members, `format` and `version`, then `source`, `partial` and the
    /// record arrays in the order of the text, with the arrays in the order nodes, links and
    /// flows.
    fn document(&mut self, value: &Json) {
        let at = "document";
        if !self.object(at, "document", value) {
            return;
        }
        self.members(
            at,
            "",
            "",
            value,
            &[
                "format", "version", "source", "nodes", "links", "flows", "partial",
            ],
            &["format", "version", "source", "nodes"],
        );
        if !self.format(at, value) {
            return;
        }
        self.source_at = at.to_owned();
        self.source_end = self.refusals.len();
        self.window = value
            .get("source")
            .and_then(|source| source.get("window"))
            .is_some_and(|window| window.as_object().is_some());
        let mut order: Vec<&str> = value
            .as_object()
            .unwrap_or_default()
            .iter()
            .map(|(name, _)| name.as_str())
            .filter(|name| CLASS_ARRAYS.contains(name) || ["source", "partial"].contains(name))
            .collect();
        let slots: Vec<usize> = (0..order.len())
            .filter(|&place| CLASS_ARRAYS.contains(&order[place]))
            .collect();
        let mut arrays: Vec<&str> = slots.iter().map(|&place| order[place]).collect();
        arrays.sort_by_key(|name| CLASS_ARRAYS.iter().position(|array| array == name));
        for (&place, name) in slots.iter().zip(arrays) {
            order[place] = name;
        }
        for name in order {
            if self.full() {
                break;
            }
            let Some(array) = value.get(name) else {
                continue;
            };
            if name == "source" {
                self.source(at, array);
                self.source_end = self.refusals.len();
                continue;
            }
            let Some(class) = CLASS_ARRAYS.iter().position(|array| *array == name) else {
                self.partial(at, "partial", array);
                continue;
            };
            let Some(items) = array.as_array() else {
                self.refuse(at, name, Code::Type, "array".to_owned(), Some(array));
                continue;
            };
            if items.len() > RECORDS[class] {
                self.refuse(
                    at,
                    name,
                    Code::Size,
                    format!("{} items", RECORDS[class]),
                    Some(&Json::Number(items.len() as f64)),
                );
                continue;
            }
            for (index, item) in items.iter().enumerate() {
                if self.full() {
                    break;
                }
                let place = format!("{name}[{index}]");
                if json_length(item) > RECORD_BYTES {
                    self.refuse(
                        &place,
                        CLASSES[class],
                        Code::Size,
                        format!("{RECORD_BYTES} bytes of JSON text"),
                        None,
                    );
                    continue;
                }
                self.record(class, &place, item);
            }
        }
        if self.full()
            && let Some(flows) = value.get("flows").and_then(Json::as_array)
        {
            for flow in flows {
                if self.unit_after_stop(flow) {
                    break;
                }
            }
        }
    }

    fn record(&mut self, class: usize, at: &str, value: &Json) {
        self.records[class] += 1;
        match class {
            0 => self.node(at, value),
            1 => self.link(at, value),
            _ => self.flow(at, value),
        }
    }

    fn node(&mut self, at: &str, value: &Json) {
        if !self.object(at, "node", value) {
            return;
        }
        self.members(
            at,
            "",
            "node",
            value,
            &["id", "kind", "in", "label", "handle", "fields"],
            &["id", "kind"],
        );
        let id = self.id(at, value);
        let kind = value
            .get("kind")
            .and_then(|kind| self.kind(at, "kind", kind));
        let parent = value
            .get("in")
            .and_then(|parent| self.node_id(at, "in", parent));
        if let Some(label) = value.get("label") {
            self.text(at, "label", label, 1, 256);
            self.not_shown("node.label");
        }
        if let Some(handle) = value.get("handle") {
            self.text(at, "handle", handle, 1, 256);
            self.not_shown("node.handle");
        }
        let (fields, names, missing) = match value.get("fields") {
            Some(fields) => self.fields(at, "fields", "node.fields", fields),
            None => (Vec::new(), Vec::new(), false),
        };
        if missing {
            self.not_shown("node.fields.missing");
        }
        if let Some(kind) = kind {
            if !self.kinds.contains_key(kind) && self.kinds.len() >= KINDS {
                self.refuse(
                    at,
                    "kind",
                    Code::Size,
                    format!("{KINDS} kinds"),
                    value.get("kind"),
                );
            } else {
                let (count, known) = self.kinds.entry(kind.to_owned()).or_default();
                *count += 1;
                known.extend(names);
            }
        }
        if let Some(id) = &id {
            self.ids.insert(id.clone(), Class::Node);
        }
        if let (Some(id), Some(kind), true) = (id, kind, self.refusals.is_empty())
            && let Some(id) = ObjectId::new(&id)
        {
            let added = self.relations.add_with_fields(id, kind, parent, fields);
            self.relation(at, added);
        }
    }

    fn link(&mut self, at: &str, value: &Json) {
        if !self.object(at, "link", value) {
            return;
        }
        self.not_shown("link");
        self.members(
            at,
            "",
            "link",
            value,
            &["id", "ends", "kind", "handle", "fields"],
            &["id", "ends"],
        );
        let id = self.id(at, value);
        if let Some(ends) = value.get("ends") {
            match ends.as_array() {
                None => self.refuse(at, "ends", Code::Type, "array".to_owned(), Some(ends)),
                Some(items) if items.len() != 2 => self.refuse(
                    at,
                    "ends",
                    Code::Size,
                    "2 items".to_owned(),
                    Some(&Json::Number(items.len() as f64)),
                ),
                Some(items) => {
                    let first = self.node_id(at, "ends[0]", &items[0]);
                    let second = self.node_id(at, "ends[1]", &items[1]);
                    if first.is_some() && first == second {
                        self.refuse(
                            at,
                            "ends[1]",
                            Code::Repeat,
                            "another node".to_owned(),
                            Some(&items[1]),
                        );
                    }
                }
            }
        }
        if let Some(kind) = value.get("kind") {
            self.kind(at, "kind", kind);
        }
        if let Some(handle) = value.get("handle") {
            self.text(at, "handle", handle, 1, 256);
        }
        if let Some(fields) = value.get("fields") {
            self.fields(at, "fields", "link.fields", fields);
        }
        if let Some(id) = id {
            self.ids.insert(id, Class::Link);
        }
    }

    fn flow(&mut self, at: &str, value: &Json) {
        if !self.object(at, "flow", value) {
            return;
        }
        self.members(
            at,
            "",
            "flow",
            value,
            &[
                "id",
                "path",
                "hops",
                "outcome",
                "magnitude",
                "group",
                "reply_to",
                "handle",
                "fields",
            ],
            &["id", "path"],
        );
        let id = self.id(at, value);
        let path = value.get("path").and_then(|path| self.path(at, path));
        if let Some(hops) = value.get("hops") {
            self.not_shown("flow.hops");
            self.hops(at, hops, value.get("path").and_then(Json::as_array));
        }
        let outcome = value
            .get("outcome")
            .and_then(|outcome| self.outcome(at, outcome));
        if let Some(magnitude) = value.get("magnitude") {
            self.not_shown("flow.magnitude");
            self.magnitude(at, magnitude);
        }
        if let Some(group) = value.get("group") {
            self.not_shown("flow.group");
            self.name(at, "group", group);
        }
        let reply = value.get("reply_to").is_some();
        if let Some(reply_to) = value.get("reply_to") {
            self.not_shown("flow.reply_to");
            if let Some(name) = self.name(at, "reply_to", reply_to)
                && self.ids.get(name) != Some(&Class::Flow(false))
            {
                self.refuse(
                    at,
                    "reply_to",
                    Code::NoFlow,
                    "the id of a flow before the record with no reply_to".to_owned(),
                    Some(reply_to),
                );
            }
        }
        if let Some(handle) = value.get("handle") {
            self.not_shown("flow.handle");
            self.text(at, "handle", handle, 1, 256);
        }
        let (fields, _, missing) = match value.get("fields") {
            Some(fields) => self.fields(at, "fields", "flow.fields", fields),
            None => (Vec::new(), Vec::new(), false),
        };
        if missing {
            self.not_shown("flow.fields.missing");
        }
        if let Some(id) = &id {
            self.ids.insert(id.clone(), Class::Flow(reply));
        }
        if let (Some(id), Some(path), true) = (id, path, self.refusals.is_empty())
            && let Some(id) = ObjectId::new(&id)
        {
            let added = self.relations.add_flow_with_data(id, path, outcome, fields);
            self.relation(at, added);
        }
    }

    /// Adds the refusal of the relations in `result` with the code of its check.
    fn relation(&mut self, at: &str, result: Result<(), RelationRefusal>) {
        let Err(refusal) = result else {
            return;
        };
        let (member, code, valid) = match refusal {
            RelationRefusal::Exists { .. } => ("id", Code::Exists, "an id of no record before"),
            RelationRefusal::NoParent { .. } => {
                ("in", Code::NoNode, "the id of a node before the record")
            }
            RelationRefusal::NoNode { .. } => {
                ("path", Code::NoNode, "the id of a node before the record")
            }
            RelationRefusal::ShortPath { .. } => ("path", Code::ShortPath, "2 or more"),
            RelationRefusal::Repeat { .. } => ("path", Code::Repeat, "another node"),
        };
        self.refuse(at, member, code, valid.to_owned(), None);
    }

    fn path(&mut self, at: &str, value: &Json) -> Option<Vec<ObjectId>> {
        let Some(items) = value.as_array() else {
            self.refuse(at, "path", Code::Type, "array".to_owned(), Some(value));
            return None;
        };
        if items.len() < PATH[0] {
            self.refuse(
                at,
                "path",
                Code::ShortPath,
                "2 or more".to_owned(),
                Some(&Json::Number(items.len() as f64)),
            );
            return None;
        }
        if items.len() > PATH[1] {
            self.refuse(
                at,
                "path",
                Code::Size,
                format!("{} nodes", PATH[1]),
                Some(&Json::Number(items.len() as f64)),
            );
            return None;
        }
        let mut path = Vec::with_capacity(items.len());
        let mut complete = true;
        for (index, item) in items.iter().enumerate() {
            let member = format!("path[{index}]");
            match self.node_id(at, &member, item) {
                Some(node) => {
                    if index > 0 && items[index - 1].as_str() == Some(node.as_str()) {
                        self.refuse(
                            at,
                            &member,
                            Code::Repeat,
                            "another node".to_owned(),
                            Some(item),
                        );
                        complete = false;
                    }
                    path.push(node);
                }
                None => complete = false,
            }
        }
        complete.then_some(path)
    }

    fn hops(&mut self, at: &str, value: &Json, path: Option<&[Json]>) {
        let Some(items) = value.as_array() else {
            self.refuse(at, "hops", Code::Type, "array".to_owned(), Some(value));
            return;
        };
        if let Some(path) = path
            && items.len() != path.len()
        {
            self.refuse(
                at,
                "hops",
                Code::Size,
                "1 item for each node of path".to_owned(),
                Some(&Json::Number(items.len() as f64)),
            );
        }
        for (index, item) in items.iter().enumerate() {
            let place = format!("hops[{index}]");
            if !self.object(at, &place, item) {
                continue;
            }
            self.members(at, &place, "flow.hops", item, &["fields"], &[]);
            if let Some(fields) = item.get("fields") {
                self.fields(at, &format!("{place}.fields"), "flow.hops.fields", fields);
            }
        }
    }

    fn outcome(&mut self, at: &str, value: &Json) -> Option<Outcome> {
        if !self.object(at, "outcome", value) {
            return None;
        }
        self.members(
            at,
            "outcome",
            "flow.outcome",
            value,
            &["kind", "reason"],
            &["kind"],
        );
        let kind = value.get("kind").and_then(|kind| {
            let text = self.string(at, "outcome.kind", kind)?;
            let parsed = OutcomeKind::parse(text);
            if parsed.is_none() {
                self.refuse(
                    at,
                    "outcome.kind",
                    Code::Value,
                    OutcomeKind::NAMES.join(", "),
                    Some(kind),
                );
            }
            parsed
        });
        let reason = match value.get("reason") {
            None => None,
            Some(reason) => Some(self.text(at, "outcome.reason", reason, 1, 1024)?.to_owned()),
        };
        Some(Outcome {
            kind: kind?,
            reason,
        })
    }

    fn magnitude(&mut self, at: &str, value: &Json) {
        if !self.object(at, "magnitude", value) {
            return;
        }
        self.members(
            at,
            "magnitude",
            "flow.magnitude",
            value,
            &["value", "unit"],
            &["value", "unit"],
        );
        if let Some(number) = value.get("value")
            && let Some(amount) = self.number(at, "magnitude.value", number)
            && amount < 0.0
        {
            self.refuse(
                at,
                "magnitude.value",
                Code::Value,
                "0 or more".to_owned(),
                Some(number),
            );
        }
        let Some(unit) = value.get("unit") else {
            return;
        };
        let Some(text) = self.string(at, "magnitude.unit", unit) else {
            return;
        };
        if !UNITS.contains(&text) {
            self.refuse(
                at,
                "magnitude.unit",
                Code::Value,
                UNITS.join(", "),
                Some(unit),
            );
            return;
        }
        match &self.unit {
            None => self.unit = Some(text.to_owned()),
            Some(first) if first != text => {
                let first = first.clone();
                self.refuse(at, "magnitude.unit", Code::Unit, first, Some(unit));
                return;
            }
            Some(_) => {}
        }
        if COUNT_UNITS.contains(&text) {
            self.no_window();
        }
    }

    /// Refuses a count unit with no window. `finish` puts the refusal at the place of `source`
    /// after its refusals.
    fn no_window(&mut self) {
        if self.window {
            return;
        }
        self.window_refused = true;
    }

    /// Takes the unit of the load from the `magnitude` of `flow` after the check stopped, for the
    /// refusal of a count unit with no window; `true` when the unit of the load is known.
    fn unit_after_stop(&mut self, flow: &Json) -> bool {
        if self.unit.is_some() || self.window {
            return true;
        }
        let Some(text) = flow
            .get("magnitude")
            .and_then(|magnitude| magnitude.get("unit"))
            .and_then(Json::as_str)
            .filter(|text| UNITS.contains(text))
        else {
            return false;
        };
        self.unit = Some(text.to_owned());
        if COUNT_UNITS.contains(&text) {
            self.no_window();
        }
        true
    }

    fn stream(&mut self, bytes: &[u8]) {
        let mut lines: Vec<&[u8]> = bytes.split(|&byte| byte == b'\n').collect();
        if lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        let mut ends = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in &lines {
            offset += line.len() + 1;
            ends.push(offset);
        }
        let page_ends: &[usize] = match self.framing {
            Framing::Pages(page_ends) => page_ends,
            _ => &[],
        };
        let open: BTreeSet<usize> = page_ends
            .iter()
            .filter(|&&end| end < bytes.len() && end > 0 && bytes[end - 1] != b'\n')
            .map(|&end| ends.partition_point(|&line_end| line_end <= end))
            .collect();
        let unended = !bytes.ends_with(b"\n");
        let mut last = "text".to_owned();
        let mut ended = false;
        for (index, line) in lines.iter().enumerate() {
            if self.full() {
                for line in &lines[index..] {
                    let flow = std::str::from_utf8(line)
                        .ok()
                        .and_then(|text| Json::parse(text).ok());
                    if let Some(flow) = flow.as_ref().and_then(|value| value.get("flow"))
                        && self.unit_after_stop(flow)
                    {
                        break;
                    }
                }
                return;
            }
            let at = format!("line {}", index + 1);
            if open.contains(&index) {
                self.refuse(&at, "text", Code::Line, BREAK_VALID.to_owned(), None);
            }
            if unended && index + 1 == lines.len() {
                self.refuse(&at, "text", Code::Line, LF_VALID.to_owned(), None);
            }
            let parsed = if line.len() > RECORD_BYTES {
                self.refuse(
                    &at,
                    "text",
                    Code::Size,
                    format!("{RECORD_BYTES} bytes"),
                    None,
                );
                None
            } else {
                match std::str::from_utf8(line) {
                    Err(error) => {
                        self.refuse(
                            &at,
                            &format!("byte {}", error.valid_up_to()),
                            Code::Json,
                            "JSON of RFC 8259 in UTF-8".to_owned(),
                            None,
                        );
                        None
                    }
                    Ok(text) => match Json::parse(text) {
                        Err(error) => {
                            self.refuse(
                                &at,
                                &format!("byte {}", error.offset),
                                Code::Json,
                                "JSON of RFC 8259 in UTF-8".to_owned(),
                                None,
                            );
                            None
                        }
                        Ok(value) => Some(value),
                    },
                }
            };
            let Some(value) = parsed else {
                if index == 0 {
                    return;
                }
                last = "text".to_owned();
                continue;
            };
            let members = match value.as_object() {
                Some(members) if members.len() == 1 => members,
                _ => {
                    self.refuse(&at, "text", Code::Line, LINES.to_owned(), None);
                    if index == 0 {
                        return;
                    }
                    last = "text".to_owned();
                    continue;
                }
            };
            let (kind, inner) = (&members[0].0, &members[0].1);
            let (member, refused) = line_member(kind);
            if ended {
                self.refuse(
                    &at,
                    member,
                    Code::NoEnd,
                    "end as the last line".to_owned(),
                    refused.as_ref(),
                );
                return;
            }
            if index == 0 && kind != "header" {
                self.refuse(
                    &at,
                    member,
                    Code::Header,
                    "header".to_owned(),
                    refused.as_ref(),
                );
                return;
            }
            last = member.to_owned();
            match kind.as_str() {
                "header" if index == 0 => {
                    if !self.object(&at, "header", inner) {
                        return;
                    }
                    self.members(
                        &at,
                        "",
                        "",
                        inner,
                        &["format", "version", "source"],
                        &["format", "version", "source"],
                    );
                    if !self.format(&at, inner) {
                        return;
                    }
                    self.source_at = at.clone();
                    if let Some(source) = inner.get("source") {
                        self.source(&at, source);
                    }
                    self.source_end = self.refusals.len();
                }
                "node" | "link" | "flow" => {
                    let class = CLASSES.iter().position(|name| name == kind).unwrap_or(0);
                    if self.records[class] >= RECORDS[class] {
                        self.refuse(
                            &at,
                            kind,
                            Code::Size,
                            format!("{} {kind}s", RECORDS[class]),
                            None,
                        );
                        self.records[class] += 1;
                        continue;
                    }
                    self.record(class, &at, inner);
                }
                "next"
                    if self.framing == Framing::Kept
                        || page_ends.contains(&(ends[index].min(bytes.len()))) =>
                {
                    if self.object(&at, "next", inner) {
                        self.members(&at, "next", "next", inner, &["cursor"], &["cursor"]);
                        if let Some(cursor) = inner.get("cursor") {
                            self.text(&at, "next.cursor", cursor, 1, 1024);
                        }
                    }
                }
                "abort" => {
                    ended = true;
                    let mut valid = NO_ABORT.to_owned();
                    if self.object(&at, "abort", inner) {
                        self.members(&at, "abort", "abort", inner, &["reason"], &["reason"]);
                        if let Some(reason) = inner.get("reason")
                            && let Some(reason) = self.text(&at, "abort.reason", reason, 1, 1024)
                        {
                            valid = reason.to_owned();
                        }
                    }
                    self.refuse(&at, "abort", Code::Abort, valid, None);
                }
                "end" => {
                    ended = true;
                    self.end(&at, inner);
                }
                "header" => self.refuse(&at, kind, Code::Line, LINES.to_owned(), None),
                "next" => self.refuse(&at, kind, Code::Line, NEXT_VALID.to_owned(), None),
                other => {
                    if ObjectId::new(other).is_some() {
                        self.ignore(format!("line {other}"));
                    }
                }
            }
        }
        if !ended {
            let at = format!("line {}", lines.len());
            self.refuse(
                &at,
                &last,
                Code::NoEnd,
                "end as the last line".to_owned(),
                None,
            );
        }
    }

    fn end(&mut self, at: &str, value: &Json) {
        if !self.object(at, "end", value) {
            return;
        }
        self.members(
            at,
            "end",
            "end",
            value,
            &["nodes", "links", "flows", "partial"],
            &["nodes", "links", "flows"],
        );
        let mut counts = [None; 3];
        for (class, name) in ["nodes", "links", "flows"].into_iter().enumerate() {
            if let Some(count) = value.get(name) {
                counts[class] = self.count(at, &format!("end.{name}"), count, RECORDS[class]);
            }
        }
        if let [Some(nodes), Some(links), Some(flows)] = counts
            && [nodes, links, flows] != self.records
        {
            let [nodes, links, flows] = self.records;
            self.refuse(
                at,
                "end",
                Code::Count,
                format!("nodes {nodes}, links {links}, flows {flows}"),
                Some(value),
            );
        }
        if let Some(partial) = value.get("partial") {
            self.partial(at, "end.partial", partial);
        }
    }
}

/// The member of a refusal of a line of the kind `kind`: the kind for a kind of `LINE_KINDS`,
/// else `text` with the kind as the refused value.
fn line_member(kind: &str) -> (&str, Option<Json>) {
    if LINE_KINDS.contains(&kind) {
        (kind, None)
    } else {
        ("text", Some(Json::string(kind)))
    }
}

/// The second of a time of RFC 3339 in UTC with `Z` in the form `YYYY-MM-DDTHH:MM:SS`, and the
/// nanoseconds of its fraction.
fn parse_time(text: &str) -> Option<(String, u32)> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 || bytes.last() != Some(&b'Z') {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = bytes.get(range)?;
        if !part.iter().all(u8::is_ascii_digit) {
            return None;
        }
        part.iter().try_fold(0u32, |value, digit| {
            Some(value * 10 + u32::from(digit - b'0'))
        })
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators.iter().any(|&(index, byte)| bytes[index] != byte) {
        return None;
    }
    let year = digits(0..4)?;
    let month = digits(5..7)?;
    let day = digits(8..10)?;
    let hour = digits(11..13)?;
    let minute = digits(14..16)?;
    let second = digits(17..19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let fraction = &bytes[19..bytes.len() - 1];
    let nanos = match fraction {
        [] => 0,
        [b'.', rest @ ..] if (1..=9).contains(&rest.len()) => {
            let value = digits(20..20 + rest.len())?;
            value * 10u32.pow(9 - rest.len() as u32)
        }
        _ => return None,
    };
    Some((text[..19].to_owned(), nanos))
}

/// The number of bytes of the compact JSON text of `value`.
fn json_length(value: &Json) -> usize {
    match value {
        Json::Null => 4,
        Json::Bool(true) => 4,
        Json::Bool(false) => 5,
        Json::Number(_) => value.to_string().len(),
        Json::String(text) => string_length(text),
        Json::Array(items) => {
            2 + items.iter().map(json_length).sum::<usize>() + items.len().saturating_sub(1)
        }
        Json::Object(members) => {
            2 + members
                .iter()
                .map(|(name, item)| string_length(name) + 1 + json_length(item))
                .sum::<usize>()
                + members.len().saturating_sub(1)
        }
    }
}

fn string_length(text: &str) -> usize {
    2 + text
        .chars()
        .map(|character| match character {
            '"' | '\\' | '\n' | '\r' | '\t' => 2,
            control if u32::from(control) < 0x20 => 6,
            other => other.len_utf8(),
        })
        .sum::<usize>()
}
