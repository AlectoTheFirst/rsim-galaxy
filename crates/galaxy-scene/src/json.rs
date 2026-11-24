use std::collections::BTreeSet;
use std::fmt;

/// A JSON value. An object keeps the order of its members.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Cause of a refused JSON text: the byte offset and the cause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    pub offset: usize,
    pub cause: &'static str,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "json at byte {}: {}", self.offset, self.cause)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    /// The value of a JSON text with no other text than white space around it, with each member
    /// name 1 time in an object.
    pub fn parse(text: &str) -> Result<Json, JsonError> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            offset: 0,
        };
        let value = parser.value(0)?;
        parser.space();
        if parser.offset != parser.bytes.len() {
            return Err(parser.error("text after the value"));
        }
        Ok(value)
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(number) => Some(*number),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(members) => Some(members),
            _ => None,
        }
    }

    /// An object of the members in order.
    pub fn object<const N: usize>(members: [(&str, Json); N]) -> Json {
        Json::Object(
            members
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }
}

/// The compact JSON text of the value. A number with no fraction below 2^53 has no decimal
/// point. A number that is not finite is `null`.
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(value) => write!(f, "{value}"),
            Json::Number(number) => {
                if !number.is_finite() {
                    f.write_str("null")
                } else if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
                    write!(f, "{}", *number as i64)
                } else {
                    write!(f, "{number}")
                }
            }
            Json::String(text) => write_string(f, text),
            Json::Array(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Json::Object(members) => {
                f.write_str("{")?;
                for (index, (name, value)) in members.iter().enumerate() {
                    if index > 0 {
                        f.write_str(",")?;
                    }
                    write_string(f, name)?;
                    write!(f, ":{value}")?;
                }
                f.write_str("}")
            }
        }
    }
}

fn write_string(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    for character in text.chars() {
        match character {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            control if u32::from(control) < 0x20 => write!(f, "\\u{:04x}", u32::from(control))?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

/// Depth of nested arrays and objects that the parser accepts.
const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    fn error(&self, cause: &'static str) -> JsonError {
        JsonError {
            offset: self.offset,
            cause,
        }
    }

    fn space(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.offset) {
            self.offset += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.offset..].starts_with(word.as_bytes()) {
            self.offset += word.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting deeper than 128"));
        }
        self.space();
        match self.bytes.get(self.offset) {
            None => Err(self.error("end of text")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => self.string().map(Json::String),
            Some(b'[') => {
                self.offset += 1;
                let mut items = Vec::new();
                self.space();
                if self.bytes.get(self.offset) == Some(&b']') {
                    self.offset += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.bytes.get(self.offset) {
                        Some(b',') => self.offset += 1,
                        Some(b']') => {
                            self.offset += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(self.error("expected , or ]")),
                    }
                }
            }
            Some(b'{') => {
                self.offset += 1;
                let mut members = Vec::new();
                let mut names = BTreeSet::new();
                self.space();
                if self.bytes.get(self.offset) == Some(&b'}') {
                    self.offset += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.space();
                    if self.bytes.get(self.offset) != Some(&b'"') {
                        return Err(self.error("expected a member name"));
                    }
                    let start = self.offset;
                    let name = self.string()?;
                    if !names.insert(name.clone()) {
                        return Err(JsonError {
                            offset: start,
                            cause: "duplicate member name",
                        });
                    }
                    self.space();
                    if self.bytes.get(self.offset) != Some(&b':') {
                        return Err(self.error("expected :"));
                    }
                    self.offset += 1;
                    members.push((name, self.value(depth + 1)?));
                    self.space();
                    match self.bytes.get(self.offset) {
                        Some(b',') => self.offset += 1,
                        Some(b'}') => {
                            self.offset += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(self.error("expected , or }")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    /// The digits from the offset; `false` for no digit.
    fn digits(&mut self) -> bool {
        let start = self.offset;
        while let Some(b'0'..=b'9') = self.bytes.get(self.offset) {
            self.offset += 1;
        }
        self.offset > start
    }

    /// A number of the grammar of RFC 8259 with a finite value as a double.
    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.offset;
        let invalid = JsonError {
            offset: start,
            cause: "invalid number",
        };
        if self.bytes.get(self.offset) == Some(&b'-') {
            self.offset += 1;
        }
        match self.bytes.get(self.offset) {
            Some(b'0') => self.offset += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(invalid),
        }
        if self.bytes.get(self.offset) == Some(&b'.') {
            self.offset += 1;
            if !self.digits() {
                return Err(invalid);
            }
        }
        if let Some(b'e' | b'E') = self.bytes.get(self.offset) {
            self.offset += 1;
            if let Some(b'+' | b'-') = self.bytes.get(self.offset) {
                self.offset += 1;
            }
            if !self.digits() {
                return Err(invalid);
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.offset]).map_err(|_| invalid)?;
        match text.parse::<f64>() {
            Ok(number) if number.is_finite() => Ok(Json::Number(number)),
            _ => Err(JsonError {
                offset: start,
                cause: "number out of the range of a double",
            }),
        }
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .bytes
            .get(self.offset..self.offset + 4)
            .ok_or_else(|| self.error("short \\u escape"))?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return Err(self.error("invalid \\u escape"));
        }
        let text = std::str::from_utf8(digits).map_err(|_| self.error("invalid \\u escape"))?;
        let value = u32::from_str_radix(text, 16).map_err(|_| self.error("invalid \\u escape"))?;
        self.offset += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.offset += 1;
        let mut text = Vec::new();
        loop {
            let Some(&byte) = self.bytes.get(self.offset) else {
                return Err(self.error("unterminated string"));
            };
            self.offset += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let Some(&escape) = self.bytes.get(self.offset) else {
                        return Err(self.error("unterminated string"));
                    };
                    self.offset += 1;
                    let character = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let high = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&high) {
                                if !self.bytes[self.offset..].starts_with(b"\\u") {
                                    return Err(self.error("lone surrogate"));
                                }
                                self.offset += 2;
                                let low = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&low) {
                                    return Err(self.error("lone surrogate"));
                                }
                                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
                            } else {
                                high
                            };
                            char::from_u32(code).ok_or_else(|| self.error("lone surrogate"))?
                        }
                        _ => return Err(self.error("invalid escape")),
                    };
                    let mut buffer = [0; 4];
                    text.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                }
                control if control < 0x20 => {
                    return Err(self.error("control character in string"));
                }
                other => text.push(other),
            }
        }
        String::from_utf8(text).map_err(|_| self.error("invalid UTF-8"))
    }
}
