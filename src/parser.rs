//! Parser for PDF objects, built on [`crate::lexer`].
//!
//! [`parse_object`] turns a byte slice into a direct [`Object`];
//! [`parse_indirect_object`] reads the `n g obj … endobj` wrapper and the
//! `stream` … `endstream` payload. The cross-reference machinery that finds
//! those offsets is layered on top in Q3.

use alloc::vec::Vec;

use alloc::collections::VecDeque;

use crate::error::{Error, Result};
use crate::lexer::{Lexer, Token};
use crate::object::{Dictionary, Object, ObjectId, Stream};

/// Maximum container nesting. Hostile input must not recurse without bound,
/// which M10 pins with an error rather than a stack overflow.
pub const MAX_DEPTH: usize = 128;

/// Resolves a `/Length` that is an indirect object.
///
/// The parser cannot follow references on its own — that needs the object
/// table — so [`crate::Document`] supplies this when it parses a stream whose
/// `/Length` is `n g R`. Without a resolver the payload is delimited by
/// scanning for `endstream` instead.
pub trait LengthResolver: core::fmt::Debug {
    /// The byte length of the object `id` points at.
    fn resolve_length(&self, id: ObjectId) -> Result<usize>;
}

/// Parse exactly one object from `input`.
///
/// Only whitespace and comments may follow the object; anything else is an
/// error, so a caller that expects one value cannot silently drop the rest.
pub fn parse_object(input: &[u8]) -> Result<Object> {
    let mut parser = Parser::new(input);
    let object = parser.parse_object()?;
    if matches!(parser.peek_token()?, Token::Eof) {
        Ok(object)
    } else {
        Err(Error::Malformed {
            what: "trailing data after object",
            offset: Some(parser.position()),
        })
    }
}

/// An object together with the identity it was stored under.
#[derive(Debug, Clone, PartialEq)]
pub struct IndirectObject {
    /// The object number and generation from its `n g obj` header.
    pub id: ObjectId,
    /// The value, which may be a [`crate::object::Stream`].
    pub object: Object,
}

/// Parse exactly one indirect object from `input`.
///
/// The whole slice must be consumed: `n g obj … endobj`, optionally with a
/// `stream` payload whose `/Length` must agree with the bytes found.
pub fn parse_indirect_object(input: &[u8]) -> Result<IndirectObject> {
    let mut parser = Parser::new(input);
    let indirect = parser.parse_indirect_object()?;
    if matches!(parser.peek_token()?, Token::Eof) {
        Ok(indirect)
    } else {
        Err(Error::Malformed {
            what: "trailing data after endobj",
            offset: Some(parser.position()),
        })
    }
}

/// A token cursor that assembles [`Object`]s.
///
/// Two tokens of lookahead are buffered, which is what `n g R` reference
/// detection and the dictionary/array terminators need.
#[derive(Debug, Clone)]
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    buffered: VecDeque<Token>,
    depth: usize,
    resolver: Option<&'a dyn LengthResolver>,
}

impl<'a> Parser<'a> {
    /// A parser over `input`.
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            lexer: Lexer::new(input),
            buffered: VecDeque::new(),
            depth: 0,
            resolver: None,
        }
    }

    /// A parser that can resolve an indirect `/Length`.
    pub fn with_resolver(input: &'a [u8], resolver: &'a dyn LengthResolver) -> Self {
        Self {
            lexer: Lexer::new(input),
            buffered: VecDeque::new(),
            depth: 0,
            resolver: Some(resolver),
        }
    }

    /// Offset of the next token in the input.
    pub fn position(&self) -> usize {
        self.lexer.position()
    }

    /// The whole byte slice this parser reads.
    pub fn content(&self) -> &'a [u8] {
        self.lexer.input()
    }

    /// Offset just past the last token consumed.
    pub fn raw_position(&self) -> usize {
        self.lexer.raw_position()
    }

    /// Jump the cursor to `offset`, dropping any lookahead.
    ///
    /// Content-stream interpretation uses this to skip inline image data,
    /// which cannot be tokenized.
    pub fn seek(&mut self, offset: usize) {
        self.buffered.clear();
        self.lexer.seek(offset);
    }

    /// Skip trivia and return the next token without consuming it.
    pub fn peek_token(&mut self) -> Result<&Token> {
        self.fill(1)?;
        match self.buffered.front() {
            Some(token) => Ok(token),
            None => Err(Error::malformed("parser buffer")),
        }
    }

    /// Consume and return the next token.
    pub fn next_token(&mut self) -> Result<Token> {
        self.fill(1)?;
        self.buffered
            .pop_front()
            .ok_or_else(|| Error::malformed("parser buffer"))
    }

    /// Consume the next token, which must be `expected`.
    pub fn expect(&mut self, expected: &Token) -> Result<()> {
        let token = self.next_token()?;
        if &token == expected {
            Ok(())
        } else {
            Err(Error::Malformed {
                what: "expected token",
                offset: Some(self.lexer.raw_position()),
            })
        }
    }

    /// Parse `n g obj … endobj`, including a `stream` payload.
    ///
    /// Streams are read by byte offset, not as tokens: their payload is opaque
    /// and may contain anything, so `/Length` decides where the data ends and
    /// `endstream` is checked afterwards to catch a wrong length.
    pub fn parse_indirect_object(&mut self) -> Result<IndirectObject> {
        let offset = self.position();
        let number = match self.next_token()? {
            Token::Integer(number) => number,
            _ => {
                return Err(Error::Malformed {
                    what: "object number",
                    offset: Some(offset),
                })
            }
        };
        let generation = match self.next_token()? {
            Token::Integer(generation) => generation,
            _ => {
                return Err(Error::Malformed {
                    what: "object generation",
                    offset: Some(offset),
                })
            }
        };
        if number < 1 {
            return Err(Error::Malformed {
                what: "object number must be positive",
                offset: Some(offset),
            });
        }
        let number = u32::try_from(number).map_err(|_| Error::Malformed {
            what: "object number",
            offset: Some(offset),
        })?;
        let generation = u16::try_from(generation).map_err(|_| Error::Malformed {
            what: "object generation",
            offset: Some(offset),
        })?;
        self.expect_keyword(b"obj")?;
        let object = self.parse_value_or_stream()?;
        self.expect_keyword(b"endobj")?;
        Ok(IndirectObject {
            id: ObjectId::new(number, generation),
            object,
        })
    }

    fn expect_keyword(&mut self, keyword: &[u8]) -> Result<()> {
        match self.next_token()? {
            Token::Keyword(found) if found == keyword => Ok(()),
            _ => Err(Error::Malformed {
                what: "expected keyword",
                offset: Some(self.lexer.raw_position()),
            }),
        }
    }

    /// A direct object, followed by stream data when the value is a dictionary
    /// and the `stream` keyword comes next.
    fn parse_value_or_stream(&mut self) -> Result<Object> {
        let object = self.parse_object()?;
        let Object::Dictionary(dict) = object else {
            return Ok(object);
        };
        if !matches!(self.peek_token()?, Token::Keyword(keyword) if keyword == b"stream") {
            return Ok(Object::Dictionary(dict));
        }
        self.next_token()?; // `stream`
        self.parse_stream(dict)
    }

    fn parse_stream(&mut self, dict: Dictionary) -> Result<Object> {
        let input = self.lexer.input();
        let start = stream_data_start(self.lexer.raw_position(), input)?;
        let (data_end, scanned) = match stream_length(&dict)? {
            StreamLength::Direct(length) => (checked_end(start, length)?, None),
            StreamLength::Indirect(id) => match self.resolver {
                Some(resolver) => (checked_end(start, resolver.resolve_length(id)?)?, None),
                // Standalone parsing has no object table, so the payload is
                // delimited by its `endstream` marker instead.
                None => {
                    let (data_end, after) = scan_endstream(input, start)?;
                    (data_end, Some(after))
                }
            },
        };
        if data_end > input.len() {
            return Err(Error::unexpected_eof("stream data"));
        }
        let after = match scanned {
            Some(after) => after,
            None => endstream_after(input, data_end)?,
        };
        let data = input[start..data_end].to_vec();
        self.lexer.seek(after);
        Ok(Object::Stream(Stream::new(dict, data)))
    }

    /// Parse one direct object.
    pub fn parse_object(&mut self) -> Result<Object> {
        if self.depth >= MAX_DEPTH {
            return Err(Error::malformed("nesting depth"));
        }
        self.depth += 1;
        let object = self.parse_object_inner();
        self.depth -= 1;
        object
    }

    fn fill(&mut self, count: usize) -> Result<()> {
        while self.buffered.len() < count {
            let token = self.lexer.next_token()?;
            let eof = token == Token::Eof;
            self.buffered.push_back(token);
            if eof {
                // Keep returning EOF instead of reading past the end forever.
                while self.buffered.len() < count {
                    self.buffered.push_back(Token::Eof);
                }
            }
        }
        Ok(())
    }

    fn peek_at(&mut self, index: usize) -> Result<&Token> {
        self.fill(index + 1)?;
        match self.buffered.get(index) {
            Some(token) => Ok(token),
            None => Err(Error::malformed("parser buffer")),
        }
    }

    fn parse_object_inner(&mut self) -> Result<Object> {
        match self.next_token()? {
            Token::Integer(value) => self.parse_integer_or_reference(value),
            Token::Real(value) => Ok(Object::Real(value)),
            Token::String(string) => Ok(Object::String(string)),
            Token::Name(name) => Ok(Object::Name(name)),
            Token::ArrayStart => self.parse_array(),
            Token::DictionaryStart => self.parse_dictionary(),
            Token::Keyword(keyword) => match keyword.as_slice() {
                b"true" => Ok(Object::Boolean(true)),
                b"false" => Ok(Object::Boolean(false)),
                b"null" => Ok(Object::Null),
                _ => Err(Error::Malformed {
                    what: "keyword",
                    offset: Some(self.lexer.raw_position()),
                }),
            },
            Token::ArrayEnd | Token::DictionaryEnd => Err(Error::Malformed {
                what: "unbalanced delimiter",
                offset: Some(self.lexer.raw_position()),
            }),
            Token::Eof => Err(Error::unexpected_eof("object")),
        }
    }

    /// `12` alone is an integer, `12 0 R` is a reference.
    fn parse_integer_or_reference(&mut self, value: i64) -> Result<Object> {
        let is_reference = matches!(self.peek_at(0)?, Token::Integer(_))
            && matches!(self.peek_at(1)?, Token::Keyword(keyword) if keyword == b"R");
        if !is_reference {
            return Ok(Object::Integer(value));
        }
        let generation = match self.next_token()? {
            Token::Integer(generation) => generation,
            _ => return Err(Error::malformed("reference generation")),
        };
        self.expect(&Token::Keyword(b"R".to_vec()))?;
        let number = u32::try_from(value).map_err(|_| Error::Malformed {
            what: "reference object number",
            offset: None,
        })?;
        let generation = u16::try_from(generation).map_err(|_| Error::Malformed {
            what: "reference generation number",
            offset: None,
        })?;
        Ok(Object::Reference(ObjectId::new(number, generation)))
    }

    fn parse_array(&mut self) -> Result<Object> {
        let mut items = Vec::new();
        loop {
            match self.peek_at(0)? {
                Token::ArrayEnd => {
                    self.next_token()?;
                    return Ok(Object::Array(items));
                }
                Token::Eof => return Err(Error::unexpected_eof("array")),
                _ => items.push(self.parse_object()?),
            }
        }
    }

    fn parse_dictionary(&mut self) -> Result<Object> {
        let mut dict = Dictionary::new();
        loop {
            match self.peek_at(0)? {
                Token::DictionaryEnd => {
                    self.next_token()?;
                    return Ok(Object::Dictionary(dict));
                }
                Token::Name(_) => {
                    let name = match self.next_token()? {
                        Token::Name(name) => name,
                        _ => return Err(Error::malformed("dictionary key")),
                    };
                    let value = self.parse_object()?;
                    dict.insert(name, value);
                }
                Token::Eof => return Err(Error::unexpected_eof("dictionary")),
                _ => {
                    return Err(Error::Malformed {
                        what: "dictionary key",
                        offset: Some(self.lexer.position()),
                    })
                }
            }
        }
    }
}

/// What a stream dictionary says about the payload length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamLength {
    /// A direct integer, usable immediately.
    Direct(usize),
    /// An indirect object that only [`LengthResolver`] can turn into a number.
    Indirect(ObjectId),
}

fn stream_length(dict: &Dictionary) -> Result<StreamLength> {
    match dict.get(b"Length") {
        Some(Object::Integer(length)) => usize::try_from(*length)
            .map(StreamLength::Direct)
            .map_err(|_| Error::Malformed {
                what: "stream length must not be negative",
                offset: None,
            }),
        Some(Object::Reference(id)) => Ok(StreamLength::Indirect(*id)),
        Some(_) => Err(Error::Malformed {
            what: "stream length must be an integer",
            offset: None,
        }),
        None => Err(Error::Malformed {
            what: "stream without /Length",
            offset: None,
        }),
    }
}

/// The offset just past the end-of-line that follows the `stream` keyword.
fn stream_data_start(after_keyword: usize, input: &[u8]) -> Result<usize> {
    let mut start = after_keyword;
    match input.get(start) {
        Some(b'\r') => {
            start += 1;
            if input.get(start) == Some(&b'\n') {
                start += 1;
            }
        }
        Some(b'\n') => start += 1,
        _ => {
            return Err(Error::Malformed {
                what: "stream keyword without an end-of-line",
                offset: Some(start),
            })
        }
    }
    Ok(start)
}

fn checked_end(start: usize, length: usize) -> Result<usize> {
    start.checked_add(length).ok_or(Error::Malformed {
        what: "stream length overflow",
        offset: Some(start),
    })
}

const ENDSTREAM: &[u8] = b"endstream";

/// Verify that `end` is followed (after an optional end-of-line) by
/// `endstream`, and return the offset just past that keyword.
fn endstream_after(input: &[u8], end: usize) -> Result<usize> {
    let mut after = end;
    match input.get(after) {
        Some(b'\r') => {
            after += 1;
            if input.get(after) == Some(&b'\n') {
                after += 1;
            }
        }
        Some(b'\n') => after += 1,
        _ => {}
    }
    if input
        .get(after..)
        .is_some_and(|rest| rest.starts_with(ENDSTREAM))
    {
        Ok(after + ENDSTREAM.len())
    } else {
        // A wrong length shows up here: the bytes after the payload must reach
        // `endstream`, otherwise the dictionary and the data disagree.
        Err(Error::Malformed {
            what: "stream length does not end at endstream",
            offset: Some(end),
        })
    }
}

/// Delimit the payload by its `endstream` marker, for streams whose `/Length`
/// cannot be resolved. Returns `(data_end, offset after endstream)`.
fn scan_endstream(input: &[u8], start: usize) -> Result<(usize, usize)> {
    let found = input
        .get(start..)
        .and_then(|rest| {
            rest.windows(ENDSTREAM.len())
                .position(|window| window == ENDSTREAM)
        })
        .map(|position| start + position)
        .ok_or(Error::unexpected_eof("endstream"))?;
    let mut data_end = found;
    if data_end > start && input[data_end - 1] == b'\n' {
        data_end -= 1;
    }
    if data_end > start && input[data_end - 1] == b'\r' {
        data_end -= 1;
    }
    Ok((data_end, found + ENDSTREAM.len()))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_indirect_object, parse_object, IndirectObject, LengthResolver, Parser, MAX_DEPTH,
    };
    use crate::error::Error;
    use crate::object::{Dictionary, Object, ObjectId};

    fn parsed(input: &str) -> Object {
        parse_object(input.as_bytes()).unwrap_or_else(|error| panic!("{input}: {error}"))
    }

    #[test]
    fn scalars_and_keywords_parse() {
        assert_eq!(parsed("null"), Object::Null);
        assert_eq!(parsed("true"), Object::Boolean(true));
        assert_eq!(parsed("false"), Object::Boolean(false));
        assert_eq!(parsed("-17"), Object::Integer(-17));
        assert_eq!(parsed("2.375"), Object::Real(2.375));
        assert_eq!(parsed("/Page"), Object::name("Page"));
        assert_eq!(parsed("(text)"), Object::string("text"));
        assert_eq!(parsed("  \n 7 \r\n"), Object::Integer(7));
    }

    #[test]
    fn references_need_both_numbers_and_r() {
        assert_eq!(parsed("12 0 R"), Object::reference(ObjectId::new(12, 0)));
        assert_eq!(
            parsed("7 65535 R"),
            Object::reference(ObjectId::new(7, 65535))
        );
        assert_eq!(
            parsed("[1 0 R 2]"),
            Object::array([Object::reference(ObjectId::new(1, 0)), Object::Integer(2)])
        );
        assert_eq!(
            parsed("[1 2 3]"),
            Object::array([Object::Integer(1), Object::Integer(2), Object::Integer(3)]),
            "runs of integers are not mistaken for references"
        );
        // Two integers without `R` stay two integers; the second one is
        // trailing data at top level, so it must be rejected.
        assert!(parse_object(b"12 0").is_err());
    }

    #[test]
    fn arrays_and_dictionaries_nest() {
        assert_eq!(parsed("[]"), Object::array(Vec::new()));
        assert_eq!(
            parsed("[[1] [2 3]]"),
            Object::array([
                Object::array([Object::Integer(1)]),
                Object::array([Object::Integer(2), Object::Integer(3)])
            ])
        );
        assert_eq!(parsed("<<>>"), Object::Dictionary(Dictionary::new()));
        let page = parsed("<< /Type /Page /Count 3 /Kids [1 0 R] >>");
        let dict = page.as_dict().expect("dictionary");
        assert_eq!(dict.get(b"Type"), Some(&Object::name("Page")));
        assert_eq!(dict.get(b"Count"), Some(&Object::Integer(3)));
        assert_eq!(
            dict.get(b"Kids"),
            Some(&Object::array([Object::reference(ObjectId::new(1, 0))]))
        );
    }

    #[test]
    fn dictionary_keys_keep_their_order() {
        let page = parsed("<< /Zed 1 /Alpha 2 /Mid 3 >>");
        let dict = page.as_dict().expect("dictionary");
        let names: Vec<&[u8]> = dict.iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec![&b"Zed"[..], &b"Alpha"[..], &b"Mid"[..]]);
    }

    #[test]
    fn duplicate_dictionary_keys_keep_the_first_position() {
        let dict = parsed("<< /A 1 /B 2 /A 3 >>");
        let dict = dict.as_dict().expect("dictionary");
        assert_eq!(dict.len(), 2);
        assert_eq!(dict.get(b"A"), Some(&Object::Integer(3)));
    }

    #[test]
    fn malformed_input_returns_an_error() {
        let cases = [
            "",
            "(",
            "[1 2",
            "<< /A >>",
            "<< 1 2 >>",
            "]",
            ">>",
            "<< /A 1 >",
            "/Bad#zz",
            "1.2.3",
            "obj",
            "12 R",
        ];
        for case in cases {
            let error = parse_object(case.as_bytes()).expect_err(case);
            assert!(
                !error.to_string().is_empty(),
                "{case} produced an error without a message"
            );
        }
    }

    #[test]
    fn trailing_tokens_are_rejected() {
        assert!(matches!(parse_object(b"1 2"), Err(Error::Malformed { .. })));
        assert!(matches!(
            parse_object(b"<< >> << >>"),
            Err(Error::Malformed { .. })
        ));
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        let depth = MAX_DEPTH + 10;
        let mut input = "[".repeat(depth);
        input.push_str(&"]".repeat(depth));
        assert!(matches!(
            parse_object(input.as_bytes()),
            Err(Error::Malformed { .. })
        ));
        // Just below the limit still parses.
        let depth = MAX_DEPTH - 1;
        let mut input = "[".repeat(depth);
        input.push_str(&"]".repeat(depth));
        assert!(parse_object(input.as_bytes()).is_ok());
    }

    #[test]
    fn truncation_never_panics() {
        let document = b"<< /Type /Page /Kids [1 0 R 2 0 R] /Count 2 >>";
        for end in 0..document.len() {
            // Either an error or a complete object; never a panic.
            let _ = parse_object(&document[..end]);
        }
    }

    fn indirect(input: &str) -> IndirectObject {
        parse_indirect_object(input.as_bytes()).unwrap_or_else(|error| panic!("{input}: {error}"))
    }

    #[test]
    fn indirect_objects_carry_their_identity() {
        let object = indirect("1 0 obj\n<< /Type /Catalog >>\nendobj\n");
        assert_eq!(object.id, ObjectId::new(1, 0));
        assert_eq!(
            object.object,
            Object::Dictionary({
                let mut dict = Dictionary::new();
                dict.insert("Type", Object::name("Catalog"));
                dict
            })
        );
        assert_eq!(
            indirect("2147483647 65535 obj 42 endobj").id,
            ObjectId::new(2147483647, 65535)
        );
        assert_eq!(indirect("7 0 obj 42 endobj").object, Object::Integer(42));
    }

    #[test]
    fn streams_take_their_bytes_from_length() {
        let object = indirect("3 0 obj\n<< /Length 5 >>\nstream\nhello\nendstream\nendobj\n");
        let stream = object.object.as_stream().expect("stream");
        assert_eq!(stream.data(), b"hello");
        assert_eq!(stream.dict().get(b"Length"), Some(&Object::Integer(5)));
    }

    #[test]
    fn stream_data_is_opaque_and_may_contain_endstream() {
        let input = "4 0 obj\n<< /Length 9 >>\nstream\nendstream\nendstream\nendobj\n";
        let object = indirect(input);
        let stream = object.object.as_stream().expect("stream");
        assert_eq!(stream.data(), b"endstream");
    }

    #[test]
    fn stream_payload_keeps_binary_bytes() {
        let mut input = Vec::new();
        input.extend_from_slice(b"5 0 obj\n<< /Length 4 >>\nstream\r\n");
        input.extend_from_slice(&[0x00, 0xff, b')', b'>']);
        input.extend_from_slice(b"\r\nendstream\r\nendobj\r\n");
        let object = parse_indirect_object(&input).expect("stream");
        let stream = object.object.as_stream().expect("stream");
        assert_eq!(stream.data(), &[0x00, 0xff, b')', b'>']);
    }

    #[test]
    fn a_wrong_length_is_an_error() {
        // Too short: three of the five bytes, so `endstream` is not next.
        let short = b"3 0 obj\n<< /Length 3 >>\nstream\nhello\nendstream\nendobj\n";
        let error = parse_indirect_object(short).expect_err("short length");
        assert!(error.to_string().contains("endstream"), "{error}");

        // Too long: the length runs past the available bytes.
        let long = b"3 0 obj\n<< /Length 500 >>\nstream\nhello\nendstream\nendobj\n";
        assert!(matches!(
            parse_indirect_object(long),
            Err(Error::UnexpectedEof { .. })
        ));
    }

    #[test]
    fn stream_length_must_be_a_number_or_a_reference() {
        let missing = b"3 0 obj\n<< /Type /XObject >>\nstream\nhello\nendstream\nendobj\n";
        assert!(parse_indirect_object(missing).is_err());

        let name = b"3 0 obj\n<< /Length /Five >>\nstream\nhello\nendstream\nendobj\n";
        assert!(parse_indirect_object(name).is_err());

        let negative = b"3 0 obj\n<< /Length -1 >>\nstream\nhello\nendstream\nendobj\n";
        assert!(parse_indirect_object(negative).is_err());
    }

    #[test]
    fn an_indirect_length_is_scanned_without_a_document() {
        let input = b"3 0 obj\n<< /Length 9 0 R >>\nstream\nhello\nendstream\nendobj\n";
        let object = parse_indirect_object(input).expect("scanned stream");
        let stream = object.object.as_stream().expect("stream");
        assert_eq!(stream.data(), b"hello");
    }

    #[derive(Debug)]
    struct FixedLength(usize);

    impl LengthResolver for FixedLength {
        fn resolve_length(&self, _id: ObjectId) -> crate::Result<usize> {
            Ok(self.0)
        }
    }

    #[test]
    fn an_indirect_length_can_be_resolved_through_the_object_table() {
        let input = b"3 0 obj\n<< /Length 9 0 R >>\nstream\nhello\nendstream\nendobj\n";
        let resolver = FixedLength(5);
        let mut parser = Parser::with_resolver(input, &resolver);
        let object = parser.parse_indirect_object().expect("resolved stream");
        assert_eq!(object.object.as_stream().expect("stream").data(), b"hello");

        // A length that disagrees with the payload is still an error.
        let wrong = FixedLength(3);
        let mut parser = Parser::with_resolver(input, &wrong);
        assert!(parser.parse_indirect_object().is_err());
    }

    #[test]
    fn a_stream_keyword_needs_an_end_of_line() {
        let input = b"3 0 obj\n<< /Length 5 >>\nstreamhello\nendstream\nendobj\n";
        assert!(matches!(
            parse_indirect_object(input),
            Err(Error::Malformed { .. })
        ));
    }

    #[test]
    fn indirect_object_wrappers_are_enforced() {
        for input in [
            &b"obj 42 endobj"[..],
            b"1 0 obj 42",
            b"1 0 obj 42 endobj 43",
            b"1 obj 42 endobj",
            b"1 0 42 endobj",
            b"0 0 obj 42 endobj",
            b"-1 0 obj 42 endobj",
            b"4294967296 0 obj 42 endobj",
            b"1 65536 obj 42 endobj",
        ] {
            assert!(
                parse_indirect_object(input).is_err(),
                "{} must not parse",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn truncated_indirect_objects_never_panic() {
        let document =
            b"6 0 obj\n<< /Type /XObject /Length 5 >>\nstream\nhello\nendstream\nendobj\n";
        for end in 0..document.len() {
            let _ = parse_indirect_object(&document[..end]);
        }
        assert!(parse_indirect_object(document).is_ok());
    }
}
