//! A minimal JSON reader and writer, enough for the baseline file.
//!
//! The benchmark has no dependencies on purpose, so
//! `docs/performance-baseline.json` is read by this small recursive-descent
//! parser instead of serde.

/// A parsed JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object, with its keys in document order.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value stored under `key`, if this is an object that has it.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The value as a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// The value as an array.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// Parse one JSON document.
pub fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(format!("trailing data at byte {}", parser.position));
    }
    Ok(value)
}

/// A JSON string literal, quotes included.
pub fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A JSON number; non-finite values have no JSON form and become `0`.
pub fn number(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_owned()
    }
}

const MAX_DEPTH: usize = 32;

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err("nesting is too deep".to_owned());
        }
        self.skip_whitespace();
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            Some(byte) => Err(format!("unexpected byte {byte:#04X}")),
            None => Err("unexpected end of input".to_owned()),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            entries.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(format!("expected `,` or `}}` at byte {}", self.position)),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected `,` or `]` at byte {}", self.position)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.next() else {
                return Err("unterminated string".to_owned());
            };
            match byte {
                b'"' => {
                    return String::from_utf8(bytes)
                        .map_err(|_| "string is not valid UTF-8".to_owned())
                }
                b'\\' => {
                    let Some(escaped) = self.next() else {
                        return Err("unterminated escape".to_owned());
                    };
                    match escaped {
                        b'"' => bytes.push(b'"'),
                        b'\\' => bytes.push(b'\\'),
                        b'/' => bytes.push(b'/'),
                        b'b' => bytes.push(0x08),
                        b'f' => bytes.push(0x0c),
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'u' => {
                            let code = self.hex4()?;
                            let character = char::from_u32(u32::from(code))
                                .ok_or_else(|| format!("invalid escape \\u{code:04x}"))?;
                            let mut buffer = [0_u8; 4];
                            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                        }
                        other => return Err(format!("invalid escape `\\{}`", other as char)),
                    }
                }
                byte if byte < 0x20 => {
                    return Err(format!("control byte {byte:#04X} in a string"));
                }
                byte => bytes.push(byte),
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, String> {
        let mut value = 0_u16;
        for _ in 0..4 {
            let Some(byte) = self.next() else {
                return Err("short \\u escape".to_owned());
            };
            let digit = (byte as char)
                .to_digit(16)
                .ok_or_else(|| format!("bad hex digit {}", byte as char))?;
            value = value * 16 + digit as u16;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        if self.peek() == Some(b'.') {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| "number is not UTF-8".to_owned())?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("invalid number `{text}`"))
    }

    fn literal(&mut self, expected: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.position..].starts_with(expected.as_bytes()) {
            self.position += expected.len();
            Ok(value)
        } else {
            Err(format!("expected `{expected}` at byte {}", self.position))
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        match self.next() {
            Some(byte) if byte == expected => Ok(()),
            Some(byte) => Err(format!(
                "expected `{}` but found `{}` at byte {}",
                expected as char,
                byte as char,
                self.position - 1
            )),
            None => Err(format!("expected `{}`, input ended", expected as char)),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }

    fn skip_whitespace(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.position += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{number, parse, string, Json};

    #[test]
    fn scalars_round_trip() {
        assert_eq!(parse("null"), Ok(Json::Null));
        assert_eq!(parse("true"), Ok(Json::Bool(true)));
        assert_eq!(parse(" 1.5 "), Ok(Json::Number(1.5)));
        assert_eq!(parse("-2e3"), Ok(Json::Number(-2000.0)));
        assert_eq!(parse("\"hi\""), Ok(Json::String("hi".to_owned())));
    }

    #[test]
    fn strings_escape_and_unescape() {
        assert_eq!(string("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
        assert_eq!(
            parse(&string("tab\tand cafe\u{e9}")),
            Ok(Json::String("tab\tand cafe\u{e9}".to_owned()))
        );
        assert_eq!(parse("\"\\u00e9\""), Ok(Json::String("\u{e9}".to_owned())));
    }

    #[test]
    fn objects_and_arrays_nest() {
        let value = parse("{\"a\": [1, 2, {\"b\": true}], \"c\": null}").expect("parsed");
        let items = value.get("a").and_then(Json::as_array).expect("array");
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].as_f64(), Some(2.0));
        assert_eq!(items[2].get("b"), Some(&Json::Bool(true)));
        assert_eq!(value.get("c"), Some(&Json::Null));
        assert_eq!(value.get("missing"), None);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        for text in [
            "",
            "{",
            "{\"a\" 1}",
            "[1,]",
            "\"unterminated",
            "tru",
            "1 2",
            "{\"a\": 1}}",
        ] {
            assert!(parse(text).is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn deeply_nested_input_is_rejected() {
        let text = "[".repeat(64) + &"]".repeat(64);
        assert!(parse(&text).is_err());
    }

    #[test]
    fn numbers_format_for_json() {
        assert_eq!(number(1.0), "1");
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(f64::NAN), "0");
    }
}
