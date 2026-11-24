//! A source of the relations format for the tests of the viewport: it serves the tool
//! `galaxy_relations` on MCP over JSON-RPC lines on its standard input and output, with the
//! documents of a directory.
//!
//! `galaxy-fixture <dir> [--page <bytes>] [--credential <variable> <sha256>] [--absent <variable>]
//! [--echo <variable>] [--pad <count>] [--frame open|early|ask|endless|cursor]`
//!
//! The query is the name of a file of `<dir>`: `<query>.json` is a document, `<query>.jsonl` a
//! stream. A stream goes in pages of whole lines of at most `--page` bytes, each with a `next` line
//! but the last. A call is refused unless the environment variable of `--credential` holds a value
//! with the SHA-256 hash `<sha256>`, and unless the variable of `--absent` is absent. The refusal
//! of an unknown query holds `--pad` letters x and then the value of the variable of `--echo`. With
//! `--frame open`, a page with a page after it has no line break at its end. With `--frame early`,
//! the `next` line of a page comes 2 times: before and after the last record of the page. With
//! `--frame ask`, the fixture sends the requests `ping` and `roots/list` before each answer of
//! `galaxy_relations`, and refuses the call unless the answers are an empty result and the error
//! -32601. With `--frame endless`, each page holds 100,000 node lines and a `next` line with a
//! fresh cursor. With `--frame cursor`, the first page holds the header and a `next` line with a
//! cursor of 1,025 characters.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use galaxy_scene::{Json, ObjectId, sha256};

const PAGE: usize = 4_194_304;

struct Options {
    dir: PathBuf,
    page: usize,
    credential: Option<(String, String)>,
    absent: Option<String>,
    echo: Option<String>,
    pad: usize,
    frame: Option<String>,
}

fn options() -> Result<Options, String> {
    let mut words = std::env::args().skip(1);
    let dir = words.next().ok_or("dir: missing value")?;
    let mut options = Options {
        dir: PathBuf::from(dir),
        page: PAGE,
        credential: None,
        absent: None,
        echo: None,
        pad: 0,
        frame: None,
    };
    while let Some(word) = words.next() {
        match word.as_str() {
            "--page" => {
                options.page = words
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--page: expected a number")?;
            }
            "--credential" => {
                let variable = words.next().ok_or("--credential: missing variable")?;
                let hash = words.next().ok_or("--credential: missing sha256")?;
                options.credential = Some((variable, hash));
            }
            "--absent" => options.absent = Some(words.next().ok_or("--absent: missing value")?),
            "--echo" => options.echo = Some(words.next().ok_or("--echo: missing value")?),
            "--pad" => {
                options.pad = words
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--pad: expected a number")?;
            }
            "--frame" => options.frame = Some(words.next().ok_or("--frame: missing value")?),
            other => {
                return Err(format!(
                    "no option {other}, valid: --page, --credential, --absent, --echo, --pad, \
                     --frame"
                ));
            }
        }
    }
    Ok(options)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = options()?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    let mut input = stdin.lock().lines();
    while let Some(line) = input.next() {
        let Ok(message) = Json::parse(&line?) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let params = message.get("params").cloned().unwrap_or(Json::Null);
        let result = match message.get("method").and_then(Json::as_str) {
            Some("initialize") => Json::object([
                ("protocolVersion", Json::string("2025-06-18")),
                (
                    "capabilities",
                    Json::object([("tools", Json::Object(Vec::new()))]),
                ),
                (
                    "serverInfo",
                    Json::object([("name", Json::string("galaxy-fixture"))]),
                ),
            ]),
            Some("tools/list") => Json::object([(
                "tools",
                Json::Array(vec![Json::object([
                    ("name", Json::string("galaxy_relations")),
                    (
                        "inputSchema",
                        Json::object([("type", Json::string("object"))]),
                    ),
                ])]),
            )]),
            Some("tools/call") => {
                let arguments = params.get("arguments").cloned().unwrap_or(Json::Null);
                let asked = if options.frame.as_deref() == Some("ask") {
                    ask(&mut stdout, &mut input)?
                } else {
                    Ok(())
                };
                let answer = match params.get("name").and_then(Json::as_str) {
                    Some("galaxy_relations") => {
                        asked.and_then(|()| relations(&options, &arguments))
                    }
                    _ => Err("name: galaxy_relations".to_owned()),
                };
                let (text, error) = match answer {
                    Ok(text) => (text, false),
                    Err(text) => (text, true),
                };
                Json::object([
                    (
                        "content",
                        Json::Array(vec![Json::object([
                            ("type", Json::string("text")),
                            ("text", Json::string(text)),
                        ])]),
                    ),
                    ("isError", Json::Bool(error)),
                ])
            }
            _ => {
                let error = Json::object([
                    ("code", Json::Number(-32601.0)),
                    ("message", Json::string("no method")),
                ]);
                writeln!(
                    stdout,
                    "{}",
                    Json::object([
                        ("jsonrpc", Json::string("2.0")),
                        ("id", id),
                        ("error", error)
                    ])
                )?;
                stdout.flush()?;
                continue;
            }
        };
        writeln!(
            stdout,
            "{}",
            Json::object([
                ("jsonrpc", Json::string("2.0")),
                ("id", id),
                ("result", result)
            ])
        )?;
        stdout.flush()?;
    }
    Ok(())
}

/// The document or the page of the stream for the arguments of `galaxy_relations`.
fn relations(options: &Options, arguments: &Json) -> Result<String, String> {
    if let Some((variable, hash)) = &options.credential {
        let held = std::env::var(variable).ok();
        if held.is_none_or(|value| &sha256::hex(value.as_bytes()) != hash) {
            return Err("credential: not valid".to_owned());
        }
    }
    if let Some(variable) = &options.absent
        && std::env::var_os(variable).is_some()
    {
        return Err(format!("{variable}: present"));
    }
    let versions = arguments.get("versions").and_then(Json::as_array);
    if !versions.is_some_and(|versions| versions.contains(&Json::string("0.1"))) {
        return Err("versions: 0.1".to_owned());
    }
    let query = arguments.get("query").and_then(Json::as_str).unwrap_or("");
    match options.frame.as_deref() {
        Some("endless") => return Ok(endless(arguments)),
        Some("cursor") => {
            return Ok(format!(
                "{{\"header\":{{\"format\":\"galaxy-relations\",\"version\":\"0.1\",\"source\":{{\"name\":\"fixture\",\"snapshot\":\"s\",\"query\":\"q\"}}}}}}\n{{\"next\":{{\"cursor\":\"{}\"}}}}\n",
                "c".repeat(1025)
            ));
        }
        _ => {}
    }
    if ObjectId::new(query).is_none() {
        return Err("query: the name of a document".to_owned());
    }
    let document = options.dir.join(format!("{query}.json"));
    if document.is_file() {
        return read(&document);
    }
    let echo = options
        .echo
        .as_ref()
        .and_then(|variable| std::env::var(variable).ok())
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    let pad = match options.pad {
        0 => String::new(),
        count => format!(" {}", "x".repeat(count)),
    };
    let text = read(&options.dir.join(format!("{query}.jsonl")))
        .map_err(|_| format!("query: no document {query}{pad}{echo}"))?;
    let start = match arguments.get("cursor") {
        None => 0,
        Some(cursor) => cursor
            .as_str()
            .and_then(|cursor| cursor.strip_prefix("line-"))
            .and_then(|line| line.parse().ok())
            .ok_or("cursor: line-<number>")?,
    };
    let page = page(&text, start, options.page)?;
    let Some(next) = page
        .trim_end()
        .rsplit('\n')
        .next()
        .filter(|line| line.starts_with("{\"next\""))
    else {
        return Ok(page);
    };
    Ok(match options.frame.as_deref() {
        Some("open") => page.trim_end_matches('\n').to_owned(),
        Some("early") => {
            let mut lines: Vec<&str> = page.trim_end().split('\n').collect();
            lines.insert(lines.len().saturating_sub(2), next);
            format!("{}\n", lines.join("\n"))
        }
        _ => page,
    })
}

/// Sends the request `ping` and the request `roots/list` to the client, and reads their
/// answers: an empty result and the error -32601.
fn ask(
    stdout: &mut impl Write,
    input: &mut impl Iterator<Item = std::io::Result<String>>,
) -> std::io::Result<Result<(), String>> {
    for (id, method, member) in [
        ("ask-1", "ping", "result"),
        ("ask-2", "roots/list", "error"),
    ] {
        writeln!(
            stdout,
            "{}",
            Json::object([
                ("jsonrpc", Json::string("2.0")),
                ("id", Json::string(id)),
                ("method", Json::string(method)),
            ])
        )?;
        stdout.flush()?;
        let answer = input.next().transpose()?.unwrap_or_default();
        let answer = Json::parse(&answer).unwrap_or(Json::Null);
        let valid = answer.get("id") == Some(&Json::string(id))
            && match member {
                "result" => answer.get("result") == Some(&Json::Object(Vec::new())),
                _ => {
                    answer.get("error").and_then(|error| error.get("code"))
                        == Some(&Json::Number(-32601.0))
                }
            };
        if !valid {
            return Ok(Err(format!("{method}: no {member} in the answer")));
        }
    }
    Ok(Ok(()))
}

/// A page of 100,000 node lines with a fresh `next` line, after the header on the first page.
fn endless(arguments: &Json) -> String {
    let page: usize = arguments
        .get("cursor")
        .and_then(Json::as_str)
        .and_then(|cursor| cursor.strip_prefix("page-"))
        .and_then(|page| page.parse().ok())
        .unwrap_or(0);
    let mut text = String::new();
    if page == 0 {
        text.push_str("{\"header\":{\"format\":\"galaxy-relations\",\"version\":\"0.1\",\"source\":{\"name\":\"fixture\",\"snapshot\":\"s\",\"query\":\"q\"}}}\n");
    }
    for index in 0..100_000 {
        text.push_str(&format!(
            "{{\"node\":{{\"id\":\"n{page}-{index}\",\"kind\":\"site\"}}}}\n"
        ));
    }
    text.push_str(&format!(
        "{{\"next\":{{\"cursor\":\"page-{}\"}}}}\n",
        page + 1
    ));
    text
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| error.to_string())
}

/// The page of `text` from the line `start`: whole lines of at most `limit` bytes with the `next`
/// line, or the rest of the lines.
fn page(text: &str, start: usize, limit: usize) -> Result<String, String> {
    let lines: Vec<&str> = text.lines().collect();
    if start >= lines.len() {
        return Err("cursor: a line of the stream".to_owned());
    }
    let mut page = String::new();
    for (index, line) in lines.iter().enumerate().skip(start) {
        let next = format!("{{\"next\":{{\"cursor\":\"line-{index}\"}}}}\n");
        let room = if index + 1 == lines.len() {
            0
        } else {
            next.len()
        };
        if !page.is_empty() && page.len() + line.len() + 1 + room > limit {
            page.push_str(&next);
            return Ok(page);
        }
        page.push_str(line);
        page.push('\n');
    }
    Ok(page)
}
