//! The transports of the sources: the MCP client on the standard input and output of the command
//! of a source, with the pages of a stream, and the files of a directory.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::time::{Duration, Instant};

use galaxy_scene::{Code, Framing, Json, LoadRefusal, ObjectId, document};

use crate::sources::{Source, Sources, Transport};

/// The MCP protocol version of the client.
pub const PROTOCOL: &str = "2025-06-18";
/// The tool of a source on MCP.
pub const TOOL: &str = "galaxy_relations";
/// Largest number of bytes of a page of the MCP transport.
pub const PAGE: usize = 4_194_304;
/// Largest number of bytes of a stream with all its pages joined, and of a `.jsonl` file.
pub const STREAM: usize = 268_435_456;
/// Largest number of characters of the cursor of a `next` line.
pub const CURSOR: usize = 1024;
/// The kinds of the record lines of a stream.
const CLASSES: [&str; 3] = ["node", "link", "flow"];
/// Largest number of bytes of 1 JSON-RPC line of a source.
pub const MESSAGE: usize = 8 * PAGE;
/// Longest wait for 1 answer of a source.
pub const WAIT: Duration = Duration::from_secs(60);
/// Largest number of requests of a source that the client answers during 1 request.
pub const ASKED: usize = 64;
/// Largest number of lines of a source in the queue of the reader thread.
const QUEUE: usize = 2;
/// Longest wait for the command of a source to end after its standard input closes.
const END: Duration = Duration::from_secs(1);
/// Largest number of characters of the text of a source in a refusal.
const TEXT: usize = 1024;
/// The valid text of a name.
const NAME_VALID: &str = "1 to 64 ASCII letters, digits, -, _ and .";

/// The bytes of a document or a stream with all its pages joined in order, their framing, and
/// the credentials of the source.
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub frames: Frames,
    pub credentials: Vec<String>,
}

/// The framing of fetched bytes: a `.json` file, a `.jsonl` file, or the pages of the MCP
/// transport with the byte offset of the end of each page.
pub enum Frames {
    Document,
    Stream,
    Pages(Vec<usize>),
}

impl Fetched {
    /// The framing of the bytes for the reader.
    pub fn framing(&self) -> Framing<'_> {
        match &self.frames {
            Frames::Document => Framing::Document,
            Frames::Stream => Framing::Stream,
            Frames::Pages(ends) => Framing::Pages(ends),
        }
    }
}

/// A refusal of the load: `at` is `load`.
pub fn refusal(member: &str, code: Code, valid: impl Into<String>) -> LoadRefusal {
    LoadRefusal {
        at: "load".to_owned(),
        member: member.to_owned(),
        code,
        valid: valid.into(),
        value: None,
    }
}

/// The answer of the source `source` of `sources` to `query`. An http source is refused with
/// the code source. The credentials of the source go only to its command. A text of the source
/// that holds one of them is withheld, and a document that holds one of them in its bytes or in
/// a decoded JSON string is refused.
pub fn fetch(sources: &Sources, source: &Source, query: &str) -> Result<Fetched, LoadRefusal> {
    if let Transport::Http { .. } = source.transport {
        return Err(refusal("transport", Code::Source, "mcp, file"));
    }
    let mut credentials = Vec::new();
    for (member, reference) in source.transport.references() {
        match reference.value() {
            Some(value) => credentials.push((member, value)),
            None => {
                return Err(refusal(
                    &member,
                    Code::Credential,
                    "env:<name> or file:<path>",
                ));
            }
        }
    }
    let secrets: Vec<String> = credentials.iter().map(|(_, value)| value.clone()).collect();
    let fetched = match &source.transport {
        Transport::Http { .. } => Err(refusal("transport", Code::Source, "mcp, file")),
        Transport::File { path } => file(path, query),
        Transport::Mcp { command, .. } => {
            let env: Vec<(String, String)> = credentials
                .iter()
                .map(|(member, value)| {
                    (member.trim_start_matches("env.").to_owned(), value.clone())
                })
                .collect();
            Mcp::start(command, &env, sources, secrets.clone())
                .and_then(|mut client| {
                    let pages = client.pages(query);
                    client.end();
                    pages
                })
                .map(|(bytes, ends)| Fetched {
                    bytes,
                    frames: Frames::Pages(ends),
                    credentials: Vec::new(),
                })
        }
    };
    match fetched {
        Err(mut refusal) => {
            if holds(refusal.valid.as_bytes(), &secrets) {
                refusal.valid = WITHHELD.to_owned();
            }
            Err(refusal)
        }
        Ok(fetched)
            if holds(&fetched.bytes, &secrets) || decoded_holds(&fetched.bytes, &secrets) =>
        {
            Err(no_credential())
        }
        Ok(fetched) => Ok(Fetched {
            credentials: secrets,
            ..fetched
        }),
    }
}

/// The valid value of a refusal in place of a text of the source that holds a credential.
const WITHHELD: &str = "a text of the source with no credential";

/// The refusal of a document that holds a credential of its source.
pub fn no_credential() -> LoadRefusal {
    refusal("document", Code::Source, "a document with no credential")
}

/// `true` when `bytes` holds 1 of `secrets`.
pub fn holds(bytes: &[u8], secrets: &[String]) -> bool {
    secrets.iter().any(|secret| {
        let secret = secret.as_bytes();
        !secret.is_empty() && bytes.windows(secret.len()).any(|window| window == secret)
    })
}

/// `true` when a JSON string of `bytes`, a member name or a value, holds 1 of `secrets` after
/// its escapes are decoded. A string runs from a quotation mark outside a string to the next
/// quotation mark that no backslash escapes. A string with no end runs to the end of `bytes`.
fn decoded_holds(bytes: &[u8], secrets: &[String]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut raw = String::from('"');
        let mut escaped = false;
        for c in chars.by_ref() {
            raw.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                break;
            }
        }
        if !raw.ends_with('"') || escaped || raw.len() == 1 {
            raw.push('"');
        }
        let decoded = match Json::parse(&raw) {
            Ok(Json::String(value)) => value,
            _ => decode_lossy(&raw),
        };
        if holds(decoded.as_bytes(), secrets) {
            return true;
        }
    }
    false
}

/// The text of the JSON string `raw` with each valid escape decoded and each other byte kept.
fn decode_lossy(raw: &str) -> String {
    let inner = raw.trim_start_matches('"').trim_end_matches('"');
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    let mut units: Vec<u16> = Vec::new();
    let flush = |units: &mut Vec<u16>, out: &mut String| {
        out.extend(char::decode_utf16(units.drain(..)).map(|c| c.unwrap_or('\u{fffd}')));
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            flush(&mut units, &mut out);
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                match u16::from_str_radix(&hex, 16) {
                    Ok(unit) => units.push(unit),
                    Err(_) => {
                        flush(&mut units, &mut out);
                        out.push_str(&hex);
                    }
                }
            }
            Some(other) => {
                flush(&mut units, &mut out);
                out.push(match other {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    other => other,
                });
            }
            None => {}
        }
    }
    flush(&mut units, &mut out);
    out
}

/// The file `<query>.json`, a document, or else `<query>.jsonl`, a stream, of the directory
/// `dir`. A document above `document::DOCUMENT_BYTES` and a stream above `STREAM` are refused
/// before the read, and the read stops after 1 byte above the limit.
fn file(dir: &Path, query: &str) -> Result<Fetched, LoadRefusal> {
    if ObjectId::new(query).is_none() {
        return Err(refusal("query", Code::Value, NAME_VALID));
    }
    for extension in ["json", "jsonl"] {
        let path = dir.join(format!("{query}.{extension}"));
        if path.is_file() {
            let length = std::fs::metadata(&path)
                .map_err(|error| refusal("query", Code::Source, error.to_string()))?
                .len();
            let most = if extension == "json" {
                document::DOCUMENT_BYTES
            } else {
                STREAM
            };
            let large = || LoadRefusal {
                at: "document".to_owned(),
                ..refusal("text", Code::Size, format!("{most} bytes"))
            };
            if length > most as u64 {
                return Err(large());
            }
            let mut bytes = Vec::new();
            std::fs::File::open(&path)
                .and_then(|file| file.take(most as u64 + 1).read_to_end(&mut bytes))
                .map_err(|error| refusal("query", Code::Source, error.to_string()))?;
            if bytes.len() > most {
                return Err(large());
            }
            return Ok(Fetched {
                bytes,
                frames: if extension == "json" {
                    Frames::Document
                } else {
                    Frames::Stream
                },
                credentials: Vec::new(),
            });
        }
    }
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|error| refusal("path", Code::Source, error.to_string()))?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let stem = name
                .strip_suffix(".jsonl")
                .or_else(|| name.strip_suffix(".json"))?;
            ObjectId::new(stem).map(|_| stem.to_owned())
        })
        .collect();
    names.sort_unstable();
    names.dedup();
    let valid = if names.is_empty() {
        "the name of a .json or .jsonl file of the source".to_owned()
    } else {
        names.join(", ")
    };
    Err(refusal("query", Code::Source, valid))
}

/// A line of the standard output of a command, or the cause of its end.
type Line = Result<Vec<u8>, String>;

/// The MCP client of a running command.
struct Mcp {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<Line>,
    id: u64,
    secrets: Vec<String>,
}

impl Mcp {
    /// Starts `command` in the directory of `sources` and initializes the session. A program
    /// name with no directory is the file of that name in the directory of `sources`, or the
    /// program of that name on `PATH` when the directory has no such file. A relative program
    /// path with a directory resolves in the directory of `sources`. The command gets the
    /// variables of `env`. The variables that the `env:` references of the other
    /// sources name are removed from its environment. A text of the command that holds 1 of
    /// `secrets` is withheld.
    fn start(
        command: &[String],
        env: &[(String, String)],
        sources: &Sources,
        secrets: Vec<String>,
    ) -> Result<Self, LoadRefusal> {
        let local = std::path::absolute(sources.dir().join(&command[0]))
            .map_err(|error| refusal("command", Code::Source, error.to_string()))?;
        let bare = Path::new(&command[0]).components().count() == 1;
        let mut process = if bare && !local.is_file() {
            Command::new(&command[0])
        } else {
            Command::new(local)
        };
        process
            .args(&command[1..])
            .current_dir(sources.dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in sources.variables() {
            process.env_remove(name);
        }
        process.envs(env.iter().map(|(name, value)| (name, value)));
        let mut child = process
            .spawn()
            .map_err(|error| refusal("command", Code::Source, error.to_string()))?;
        let input = child.stdin.take();
        let output = child.stdout.take().expect("piped standard output");
        let (send, lines) = sync_channel(QUEUE);
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                let read = (&mut output)
                    .take(MESSAGE as u64 + 1)
                    .read_until(b'\n', &mut line);
                let line = match read {
                    Ok(0) => Err("no answer".to_owned()),
                    Ok(_) if line.len() > MESSAGE => {
                        Err(format!("a message of {MESSAGE} bytes or fewer"))
                    }
                    Ok(_) => Ok(line),
                    Err(error) => Err(error.to_string()),
                };
                let end = line.is_err();
                if send.send(line).is_err() || end {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            lines,
            id: 0,
            secrets,
        };
        let result = client.request(
            "initialize",
            Json::object([
                ("protocolVersion", Json::string(PROTOCOL)),
                ("capabilities", Json::Object(Vec::new())),
                (
                    "clientInfo",
                    Json::object([
                        ("name", Json::string("galaxy")),
                        ("version", Json::string(env!("CARGO_PKG_VERSION"))),
                    ]),
                ),
            ]),
        );
        if let Err(refusal) = result {
            client.end();
            return Err(refusal);
        }
        if let Err(refusal) = client.send(&Json::object([
            ("jsonrpc", Json::string("2.0")),
            ("method", Json::string("notifications/initialized")),
        ])) {
            client.end();
            return Err(refusal);
        }
        Ok(client)
    }

    fn send(&mut self, message: &Json) -> Result<(), LoadRefusal> {
        let input = self
            .input
            .as_mut()
            .ok_or_else(|| refusal("command", Code::Source, "an open standard input"))?;
        writeln!(input, "{message}")
            .and_then(|()| input.flush())
            .map_err(|error| refusal("command", Code::Source, error.to_string()))
    }

    /// Sends the request `method` with `params` and gives the `result` of its answer. The answer
    /// comes in `WAIT` or the request is refused. The request is refused at request `ASKED` + 1 of
    /// the source before its answer.
    fn request(&mut self, method: &str, params: Json) -> Result<Json, LoadRefusal> {
        self.id += 1;
        let id = Json::Number(self.id as f64);
        self.send(&Json::object([
            ("jsonrpc", Json::string("2.0")),
            ("id", id.clone()),
            ("method", Json::string(method)),
            ("params", params),
        ]))?;
        let deadline = Instant::now() + WAIT;
        let late = || {
            refusal(
                method,
                Code::Source,
                format!("an answer in {} s", WAIT.as_secs()),
            )
        };
        let mut asked = 0;
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(late());
            }
            let line = match self.lines.recv_timeout(deadline - now) {
                Ok(Ok(line)) => line,
                Ok(Err(cause)) => return Err(refusal(method, Code::Source, cause)),
                Err(RecvTimeoutError::Timeout) => return Err(late()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(refusal(method, Code::Source, "no answer"));
                }
            };
            let Some(message) = std::str::from_utf8(&line)
                .ok()
                .and_then(|text| Json::parse(text).ok())
            else {
                continue;
            };
            if let Some(kind) = message.get("method") {
                if let Some(request) = message.get("id") {
                    asked += 1;
                    if asked > ASKED {
                        return Err(refusal(
                            method,
                            Code::Source,
                            format!("{ASKED} requests of the source or fewer"),
                        ));
                    }
                    self.answer(request, kind)?;
                }
                continue;
            }
            if message.get("id") != Some(&id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let text = error.get("message").and_then(Json::as_str).unwrap_or("");
                return Err(refusal(method, Code::Source, self.text(text)));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| refusal(method, Code::Source, "a result"));
        }
    }

    /// Answers the request `id` of the source for `method`: `ping` with an empty result, and each
    /// other method with the JSON-RPC error -32601.
    fn answer(&mut self, id: &Json, method: &Json) -> Result<(), LoadRefusal> {
        let outcome = if method.as_str() == Some("ping") {
            ("result", Json::Object(Vec::new()))
        } else {
            (
                "error",
                Json::object([
                    ("code", Json::Number(-32601.0)),
                    ("message", Json::string("Method not found")),
                ]),
            )
        };
        self.send(&Json::object([
            ("jsonrpc", Json::string("2.0")),
            ("id", id.clone()),
            outcome,
        ]))
    }

    /// The text of the source for a refusal: its first `TEXT` characters, or `WITHHELD` for a
    /// text that holds a credential.
    fn text(&self, text: &str) -> String {
        let text = text.trim();
        if holds(text.as_bytes(), &self.secrets) {
            return WITHHELD.to_owned();
        }
        if text.is_empty() {
            return "a text of the source".to_owned();
        }
        text.chars().take(TEXT).collect()
    }

    /// The pages of the answer to `query`, joined in order as sent, with the byte offset of the
    /// end of each page. A page after a page whose last line is `next` is requested with its
    /// cursor. The pages stop with a refusal of the code size when the lines of a class pass
    /// its limit, at a page that brings the joined bytes above `STREAM` before the page is
    /// joined, and at a cursor outside 1 to `CURSOR` characters.
    fn pages(&mut self, query: &str) -> Result<(Vec<u8>, Vec<usize>), LoadRefusal> {
        let versions = Json::Array(
            document::VERSIONS
                .iter()
                .map(|v| Json::string(*v))
                .collect(),
        );
        let mut bytes = Vec::new();
        let mut ends = Vec::new();
        let mut cursors = BTreeSet::new();
        let mut last: Option<String> = None;
        let mut number = 0;
        let mut lines = 0;
        let mut records = [0_usize; 3];
        loop {
            let mut arguments = vec![
                ("query".to_owned(), Json::string(query)),
                ("versions".to_owned(), versions.clone()),
            ];
            number += 1;
            if let Some(cursor) = &last {
                arguments.push(("cursor".to_owned(), Json::string(cursor)));
            }
            let result = self.request(
                "tools/call",
                Json::object([
                    ("name", Json::string(TOOL)),
                    ("arguments", Json::Object(arguments)),
                ]),
            )?;
            let content = result
                .get("content")
                .and_then(Json::as_array)
                .and_then(<[Json]>::first);
            let text = content
                .filter(|item| item.get("type").and_then(Json::as_str) == Some("text"))
                .and_then(|item| item.get("text"))
                .and_then(Json::as_str);
            if result.get("isError") == Some(&Json::Bool(true)) {
                return Err(refusal(TOOL, Code::Source, self.text(text.unwrap_or(""))));
            }
            let Some(page) = text else {
                return Err(refusal(
                    TOOL,
                    Code::Source,
                    "a first content item of the type text",
                ));
            };
            if page.len() > PAGE {
                return Err(refusal(
                    &format!("page {number}"),
                    Code::Size,
                    format!("{PAGE} bytes"),
                ));
            }
            for line in page.lines() {
                lines += 1;
                let class = Json::parse(line).ok().and_then(|value| {
                    let members = value.as_object()?;
                    let [(kind, _)] = members else {
                        return None;
                    };
                    CLASSES.iter().position(|class| class == kind)
                });
                if let Some(class) = class {
                    records[class] += 1;
                    if records[class] > document::RECORDS[class] {
                        return Err(refusal(
                            &format!("page {number}"),
                            Code::Size,
                            format!("{} {}s", document::RECORDS[class], CLASSES[class]),
                        ));
                    }
                }
            }
            if bytes.len() + page.len() > STREAM {
                return Err(refusal(
                    &format!("page {number}"),
                    Code::Size,
                    format!("{STREAM} bytes"),
                ));
            }
            bytes.extend_from_slice(page.as_bytes());
            ends.push(bytes.len());
            let Some(cursor) = next(page) else {
                return Ok((bytes, ends));
            };
            let length = cursor.chars().count();
            if !(1..=CURSOR).contains(&length) {
                return Err(refusal(
                    &format!("line {lines}"),
                    Code::Size,
                    format!("next.cursor of 1 to {CURSOR} characters"),
                ));
            }
            if !cursors.insert(cursor.clone()) {
                return Err(refusal(
                    &format!("page {number}"),
                    Code::Source,
                    "a cursor of no earlier page",
                ));
            }
            last = Some(cursor);
        }
    }

    /// Closes the standard input of the command, and stops the command when it does not end.
    fn end(&mut self) {
        self.input = None;
        let start = Instant::now();
        while start.elapsed() < END {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The `cursor` of the `next` line that ends `page`.
fn next(page: &str) -> Option<String> {
    let last = page.trim_end().rsplit('\n').next()?;
    let line = Json::parse(last).ok()?;
    let members = line.as_object()?;
    if members.len() != 1 {
        return None;
    }
    line.get("next")?.get("cursor")?.as_str().map(str::to_owned)
}
