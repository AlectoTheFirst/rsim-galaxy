//! The configuration of the sources of the viewport: 1 JSON object with the member `sources`,
//! an object with a source for each name. A relative `path`, a relative `file:` reference and
//! the command of a source resolve in the directory of the configuration file.

use std::path::{Path, PathBuf};

use galaxy_scene::{Code, Json, LoadRefusal, ObjectId};

/// The directory of the kept documents next to the configuration file.
pub const DOCUMENTS: &str = "galaxy-documents";

/// The valid text of a name.
const NAME_VALID: &str = "1 to 64 ASCII letters, digits, -, _ and .";
/// The valid text of a credential reference.
const REFERENCE_VALID: &str = "env:<name> or file:<path>";
/// The valid text of a transport.
const TRANSPORTS: &str = "mcp, http, file";
/// Largest number of characters of a description.
const DESCRIPTION: usize = 1024;

/// A credential reference: an environment variable of the viewport, or a file whose content
/// with no line break at its end is the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    Env(String),
    File(PathBuf),
}

impl Reference {
    /// The reference of the text `env:<name>` or `file:<path>`, with a relative path in `dir`.
    fn parse(text: &str, dir: &Path) -> Option<Self> {
        if let Some(name) = text.strip_prefix("env:") {
            return is_variable(name).then(|| Self::Env(name.to_owned()));
        }
        let path = text.strip_prefix("file:")?;
        (!path.is_empty()).then(|| Self::File(dir.join(path)))
    }

    /// The value of the reference, or `None` for an absent variable, an unreadable file or an
    /// empty value.
    pub fn value(&self) -> Option<String> {
        let value = match self {
            Self::Env(name) => std::env::var(name).ok()?,
            Self::File(path) => {
                let mut text = std::fs::read_to_string(path).ok()?;
                if text.ends_with('\n') {
                    text.pop();
                    if text.ends_with('\r') {
                        text.pop();
                    }
                }
                text
            }
        };
        (!value.is_empty()).then_some(value)
    }
}

/// The transport of a source with its members.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    /// The command with its arguments, and the environment variables of the command with their
    /// credential references.
    Mcp {
        command: Vec<String>,
        env: Vec<(String, Reference)>,
    },
    Http {
        url: String,
        credential: Option<Reference>,
    },
    /// The directory of the files.
    File { path: PathBuf },
}

impl Transport {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Mcp { .. } => "mcp",
            Self::Http { .. } => "http",
            Self::File { .. } => "file",
        }
    }

    /// The credential references of the source with their members.
    pub fn references(&self) -> Vec<(String, &Reference)> {
        match self {
            Self::Mcp { env, .. } => env
                .iter()
                .map(|(name, reference)| (format!("env.{name}"), reference))
                .collect(),
            Self::Http {
                credential: Some(reference),
                ..
            } => vec![("credential".to_owned(), reference)],
            _ => Vec::new(),
        }
    }
}

/// A source: its name, its description and its transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub description: String,
    pub transport: Transport,
}

/// The sources of a configuration file in the order of the file, with the directory of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sources {
    dir: PathBuf,
    sources: Vec<Source>,
}

impl Sources {
    /// The sources of the configuration file `path`. A refused file gives its refusals in the
    /// order of the file, each `<at> <member>: <code>, valid: <valid>`.
    pub fn read(path: &Path) -> Result<Self, Vec<LoadRefusal>> {
        let refusal = |member: &str, code: Code, valid: &str| LoadRefusal {
            at: "configuration".to_owned(),
            member: member.to_owned(),
            code,
            valid: valid.to_owned(),
            value: None,
        };
        let text = std::fs::read_to_string(path).map_err(|error| {
            vec![refusal(
                "file",
                Code::Source,
                &format!("a readable file: {error}"),
            )]
        })?;
        let dir = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let value = Json::parse(&text)
            .map_err(|_| vec![refusal("text", Code::Json, "JSON of RFC 8259 in UTF-8")])?;
        Self::from_json(&value, dir)
    }

    /// The sources of the configuration `value`, with relative paths in `dir`.
    pub fn from_json(value: &Json, dir: PathBuf) -> Result<Self, Vec<LoadRefusal>> {
        let mut checks = Checks {
            at: "configuration".to_owned(),
            refusals: Vec::new(),
        };
        let Some(members) = value.as_object() else {
            checks.refuse("text", Code::Type, "object");
            return Err(checks.refusals);
        };
        checks.members(members, &["sources"]);
        let sources = match value.get("sources") {
            None => {
                checks.refuse("sources", Code::Missing, "sources");
                &[][..]
            }
            Some(Json::Object(sources)) => sources,
            Some(_) => {
                checks.refuse("sources", Code::Type, "object");
                &[][..]
            }
        };
        let mut read = Vec::new();
        for (name, value) in sources {
            checks.at = "configuration".to_owned();
            if ObjectId::new(name).is_none() {
                checks.refuse("sources", Code::Value, NAME_VALID);
                continue;
            }
            checks.at = format!("sources.{name}");
            if let Some(source) = checks.source(name, value, &dir) {
                read.push(source);
            }
        }
        if checks.refusals.is_empty() {
            Ok(Self { dir, sources: read })
        } else {
            Err(checks.refusals)
        }
    }

    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    pub fn get(&self, name: &str) -> Option<&Source> {
        self.sources.iter().find(|source| source.name == name)
    }

    /// The names of the sources, joined with `, `.
    pub fn names(&self) -> String {
        let names: Vec<&str> = self
            .sources
            .iter()
            .map(|source| source.name.as_str())
            .collect();
        names.join(", ")
    }

    /// The directory of the configuration file.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory of the kept documents.
    pub fn documents(&self) -> PathBuf {
        self.dir.join(DOCUMENTS)
    }

    /// The names of the environment variables of the `env:` references of all sources.
    pub fn variables(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .sources
            .iter()
            .flat_map(|source| source.transport.references())
            .filter_map(|(_, reference)| match reference {
                Reference::Env(name) => Some(name.as_str()),
                Reference::File(_) => None,
            })
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The answer of the tool `sources`: the name and the description of each source.
    pub fn to_json(&self) -> Json {
        Json::object([(
            "sources",
            Json::Array(
                self.sources
                    .iter()
                    .map(|source| {
                        Json::object([
                            ("name", Json::string(&source.name)),
                            ("description", Json::string(&source.description)),
                        ])
                    })
                    .collect(),
            ),
        )])
    }
}

struct Checks {
    at: String,
    refusals: Vec<LoadRefusal>,
}

impl Checks {
    fn refuse(&mut self, member: &str, code: Code, valid: &str) {
        self.refusals.push(LoadRefusal {
            at: self.at.clone(),
            member: member.to_owned(),
            code,
            valid: valid.to_owned(),
            value: None,
        });
    }

    /// Refuses each member outside `valid`.
    fn members(&mut self, members: &[(String, Json)], valid: &[&str]) {
        for (name, _) in members {
            if !valid.contains(&name.as_str()) {
                self.refuse(name, Code::Value, &valid.join(", "));
            }
        }
    }

    /// The string member `name` of `value`, with 1 to `limit` characters.
    fn string<'a>(&mut self, value: &'a Json, name: &str, limit: usize) -> Option<&'a str> {
        match value.get(name) {
            None => {
                self.refuse(name, Code::Missing, name);
                None
            }
            Some(Json::String(text)) if !text.is_empty() && text.chars().count() <= limit => {
                Some(text)
            }
            Some(Json::String(_)) => {
                self.refuse(name, Code::Size, &format!("1 to {limit} characters"));
                None
            }
            Some(_) => {
                self.refuse(name, Code::Type, "string");
                None
            }
        }
    }

    fn reference(&mut self, member: &str, value: &Json, dir: &Path) -> Option<Reference> {
        let Some(text) = value.as_str() else {
            self.refuse(member, Code::Type, "string");
            return None;
        };
        let reference = Reference::parse(text, dir);
        if reference.is_none() {
            self.refuse(member, Code::Value, REFERENCE_VALID);
        }
        reference
    }

    fn source(&mut self, name: &str, value: &Json, dir: &Path) -> Option<Source> {
        let Some(members) = value.as_object() else {
            self.refuse("source", Code::Type, "object");
            return None;
        };
        let start = self.refusals.len();
        let description = self.string(value, "description", DESCRIPTION);
        let transport = match value.get("transport").map(Json::as_str) {
            None => {
                self.refuse("transport", Code::Missing, "transport");
                None
            }
            Some(None) => {
                self.refuse("transport", Code::Type, "string");
                None
            }
            Some(Some("mcp")) => {
                self.members(members, &["transport", "description", "command", "env"]);
                self.mcp(value, dir)
            }
            Some(Some("http")) => {
                self.members(members, &["transport", "description", "url", "credential"]);
                self.http(value, dir)
            }
            Some(Some("file")) => {
                self.members(members, &["transport", "description", "path"]);
                self.string(value, "path", usize::MAX)
                    .map(|path| Transport::File {
                        path: dir.join(path),
                    })
            }
            Some(Some(_)) => {
                self.refuse("transport", Code::Value, TRANSPORTS);
                None
            }
        };
        if self.refusals.len() > start {
            return None;
        }
        Some(Source {
            name: name.to_owned(),
            description: description?.to_owned(),
            transport: transport?,
        })
    }

    fn mcp(&mut self, value: &Json, dir: &Path) -> Option<Transport> {
        let command = match value.get("command") {
            None => {
                self.refuse("command", Code::Missing, "command");
                None
            }
            Some(Json::Array(items)) if !items.is_empty() => {
                let mut command = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    match item.as_str() {
                        Some(text) if !text.is_empty() => command.push(text.to_owned()),
                        Some(_) => self.refuse(
                            &format!("command[{index}]"),
                            Code::Size,
                            "1 or more characters",
                        ),
                        None => self.refuse(&format!("command[{index}]"), Code::Type, "string"),
                    }
                }
                (command.len() == items.len()).then_some(command)
            }
            Some(Json::Array(_)) => {
                self.refuse("command", Code::Size, "1 or more strings");
                None
            }
            Some(_) => {
                self.refuse("command", Code::Type, "array of strings");
                None
            }
        };
        let mut env = Vec::new();
        match value.get("env") {
            None => {}
            Some(Json::Object(members)) => {
                for (name, value) in members {
                    let member = format!("env.{name}");
                    if !is_variable(name) {
                        self.refuse(
                            &member,
                            Code::Value,
                            "ASCII letters, digits and _, not a digit first",
                        );
                        continue;
                    }
                    if let Some(reference) = self.reference(&member, value, dir) {
                        env.push((name.clone(), reference));
                    }
                }
            }
            Some(_) => self.refuse("env", Code::Type, "object"),
        }
        Some(Transport::Mcp {
            command: command?,
            env,
        })
    }

    fn http(&mut self, value: &Json, dir: &Path) -> Option<Transport> {
        let url = self.string(value, "url", 8192);
        if let Some(url) = url
            && !is_url(url)
        {
            self.refuse(
                "url",
                Code::Value,
                "https://<host>/<path>, http:// for a loopback address",
            );
        }
        let credential = match value.get("credential") {
            None => None,
            Some(value) => Some(self.reference("credential", value, dir)?),
        };
        Some(Transport::Http {
            url: url?.to_owned(),
            credential,
        })
    }
}

/// `true` for an environment variable name: ASCII letters, digits and `_`, not a digit first.
fn is_variable(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `true` for an `https` URL with a host, and for an `http` URL to a loopback address.
fn is_url(url: &str) -> bool {
    let (rest, loopback) = if let Some(rest) = url.strip_prefix("https://") {
        (rest, false)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (rest, true)
    } else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if !loopback {
        return true;
    }
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    match host.parse::<std::net::IpAddr>() {
        Ok(address) => address.is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}
