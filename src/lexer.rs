//! Tokenizer for PDF object syntax (ISO 32000-2 §7.2 lexical conventions).
//!
//! The lexer turns a byte slice into [`Token`]s. It is deliberately dumb: it
//! knows nothing about indirect objects, streams or the object graph, so the
//! same tokens serve [`crate::parser`], the content-stream reader and the
//! cross-reference reader.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::object::PdfString;

/// Whitespace: NUL, HT, LF, FF, CR and SP (§7.2.2).
pub fn is_whitespace(byte: u8) -> bool {
    matches!(byte, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

/// Delimiters: `( ) < > [ ] { } / %` (§7.2.2).
pub fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Regular characters: everything that is neither whitespace nor a delimiter.
pub fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

pub(crate) fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// One lexical unit of a PDF file.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// An integer, e.g. `-17`.
    Integer(i64),
    /// A real number, e.g. `2.375`.
    Real(f64),
    /// A string with the syntax it was written with.
    String(PdfString),
    /// A name with its `#xx` escapes decoded, without the leading `/`.
    Name(Vec<u8>),
    /// `[`.
    ArrayStart,
    /// `]`.
    ArrayEnd,
    /// `<<`.
    DictionaryStart,
    /// `>>`.
    DictionaryEnd,
    /// A run of regular characters that is not a number: `obj`, `R`, `true`,
    /// `stream`, `endstream`, `xref`, `trailer`, …
    Keyword(Vec<u8>),
    /// End of input.
    Eof,
}

impl Token {
    /// The keyword text, if this token is a keyword.
    pub fn keyword(&self) -> Option<&[u8]> {
        match self {
            Self::Keyword(text) => Some(text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Peeked {
    token: Token,
    start: usize,
    end: usize,
}

/// A cursor over PDF tokens.
///
/// Whitespace and `%` comments are skipped between tokens. One token of
/// lookahead is buffered by [`Lexer::peek`].
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    input: &'a [u8],
    pos: usize,
    peeked: Option<Peeked>,
}

impl<'a> Lexer<'a> {
    /// Wrap a byte slice.
    pub fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            pos: 0,
            peeked: None,
        }
    }

    /// The whole input this lexer reads from.
    pub fn input(&self) -> &'a [u8] {
        self.input
    }

    /// Offset of the next token, or of the current cursor when the input is
    /// exhausted. Whitespace and comments before it are not counted.
    pub fn position(&self) -> usize {
        match &self.peeked {
            Some(peeked) => peeked.start,
            None => self.pos,
        }
    }

    /// Move the cursor to `offset`, dropping any buffered token.
    pub fn seek(&mut self, offset: usize) {
        self.pos = offset.min(self.input.len());
        self.peeked = None;
    }

    /// Offset just past the last token returned, i.e. where the token after
    /// [`Lexer::next_token`] would start before trivia is skipped.
    pub fn raw_position(&self) -> usize {
        match &self.peeked {
            Some(peeked) => peeked.end,
            None => self.pos,
        }
    }

    /// What the next token will be, without consuming it.
    pub fn peek(&mut self) -> Result<&Token> {
        self.fill()?;
        match &self.peeked {
            Some(peeked) => Ok(&peeked.token),
            None => Err(Error::malformed("lexer buffer")),
        }
    }

    /// Consume and return the next token.
    pub fn next_token(&mut self) -> Result<Token> {
        if let Some(peeked) = self.peeked.take() {
            self.pos = peeked.end;
            return Ok(peeked.token);
        }
        self.read_token()
    }

    fn fill(&mut self) -> Result<()> {
        if self.peeked.is_none() {
            let start = self.skip_trivia();
            let token = self.read_token()?;
            self.peeked = Some(Peeked {
                token,
                start,
                end: self.pos,
            });
        }
        Ok(())
    }

    /// Skip whitespace and comments, returning the offset of the next token.
    fn skip_trivia(&mut self) -> usize {
        loop {
            while self.input.get(self.pos).is_some_and(|b| is_whitespace(*b)) {
                self.pos += 1;
            }
            if self.input.get(self.pos) != Some(&b'%') {
                return self.pos;
            }
            while self
                .input
                .get(self.pos)
                .is_some_and(|b| *b != b'\n' && *b != b'\r')
            {
                self.pos += 1;
            }
        }
    }

    fn read_token(&mut self) -> Result<Token> {
        self.skip_trivia();
        let Some(&byte) = self.input.get(self.pos) else {
            return Ok(Token::Eof);
        };
        match byte {
            b'[' => {
                self.pos += 1;
                Ok(Token::ArrayStart)
            }
            b']' => {
                self.pos += 1;
                Ok(Token::ArrayEnd)
            }
            b'<' if self.input.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                Ok(Token::DictionaryStart)
            }
            b'<' => self.hex_string(),
            b'>' if self.input.get(self.pos + 1) == Some(&b'>') => {
                self.pos += 2;
                Ok(Token::DictionaryEnd)
            }
            b'>' => Err(Error::UnexpectedByte {
                byte,
                offset: self.pos,
                what: "token",
            }),
            b'(' => self.literal_string(),
            b')' | b'{' | b'}' => Err(Error::UnexpectedByte {
                byte,
                offset: self.pos,
                what: "token",
            }),
            b'/' => self.name(),
            b'%' => Err(Error::UnexpectedByte {
                byte,
                offset: self.pos,
                what: "token",
            }),
            _ => self.number_or_keyword(),
        }
    }

    fn hex_string(&mut self) -> Result<Token> {
        self.pos += 1; // `<`
        let mut bytes = Vec::new();
        let mut high: Option<u8> = None;
        loop {
            let Some(&byte) = self.input.get(self.pos) else {
                return Err(Error::unexpected_eof("hex string"));
            };
            self.pos += 1;
            if byte == b'>' {
                break;
            }
            if is_whitespace(byte) {
                continue;
            }
            let value = hex_value(byte).ok_or(Error::UnexpectedByte {
                byte,
                offset: self.pos - 1,
                what: "hex string",
            })?;
            match high.take() {
                Some(high) => bytes.push((high << 4) | value),
                None => high = Some(value),
            }
        }
        if let Some(high) = high {
            // An odd number of digits is padded with a trailing zero (§7.3.4.3).
            bytes.push(high << 4);
        }
        Ok(Token::String(PdfString::hex(bytes)))
    }

    fn literal_string(&mut self) -> Result<Token> {
        self.pos += 1; // `(`
        let mut bytes = Vec::new();
        let mut depth = 1_usize;
        loop {
            let Some(&byte) = self.input.get(self.pos) else {
                return Err(Error::unexpected_eof("literal string"));
            };
            self.pos += 1;
            match byte {
                b'(' => {
                    depth += 1;
                    bytes.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    bytes.push(b')');
                }
                b'\\' => self.string_escape(&mut bytes)?,
                b'\r' => {
                    // An end-of-line marker inside a string is a single LF.
                    if self.input.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    bytes.push(b'\n');
                }
                b'\n' => bytes.push(b'\n'),
                _ => bytes.push(byte),
            }
        }
        Ok(Token::String(PdfString::literal(bytes)))
    }

    fn string_escape(&mut self, out: &mut Vec<u8>) -> Result<()> {
        let Some(&byte) = self.input.get(self.pos) else {
            return Err(Error::unexpected_eof("string escape"));
        };
        self.pos += 1;
        match byte {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'(' | b')' | b'\\' => out.push(byte),
            b'\r' => {
                // Line continuation: backslash and EOL contribute nothing.
                if self.input.get(self.pos) == Some(&b'\n') {
                    self.pos += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                let mut value = u16::from(byte - b'0');
                let mut digits = 1;
                while digits < 3 {
                    match self.input.get(self.pos) {
                        Some(&digit @ b'0'..=b'7') => {
                            value = value * 8 + u16::from(digit - b'0');
                            self.pos += 1;
                            digits += 1;
                        }
                        _ => break,
                    }
                }
                // Values above 255 cannot be stored; PDF writers stay below it.
                out.push(value as u8);
            }
            // An unknown escape stands for the character itself (§7.3.4.2).
            other => out.push(other),
        }
        Ok(())
    }

    fn name(&mut self) -> Result<Token> {
        self.pos += 1; // `/`
        let mut bytes = Vec::new();
        while let Some(&byte) = self.input.get(self.pos) {
            if !is_regular(byte) {
                break;
            }
            self.pos += 1;
            if byte != b'#' {
                bytes.push(byte);
                continue;
            }
            let high = match self.input.get(self.pos) {
                Some(&digit) => digit,
                None => return Err(Error::unexpected_eof("name escape")),
            };
            let low = match self.input.get(self.pos + 1) {
                Some(&digit) => digit,
                None => return Err(Error::unexpected_eof("name escape")),
            };
            match (hex_value(high), hex_value(low)) {
                (Some(high), Some(low)) => {
                    bytes.push((high << 4) | low);
                    self.pos += 2;
                }
                _ => {
                    return Err(Error::Malformed {
                        what: "name escape",
                        offset: Some(self.pos - 1),
                    })
                }
            }
        }
        Ok(Token::Name(bytes))
    }

    fn number_or_keyword(&mut self) -> Result<Token> {
        let start = self.pos;
        while self.input.get(self.pos).is_some_and(|b| is_regular(*b)) {
            self.pos += 1;
        }
        let text = &self.input[start..self.pos];
        let starts_number = text
            .first()
            .is_some_and(|b| b.is_ascii_digit() || matches!(*b, b'+' | b'-' | b'.'));
        if !starts_number {
            return Ok(Token::Keyword(text.to_vec()));
        }
        parse_number(text)
    }
}

/// Classify a run of regular characters starting with a digit, sign or dot.
///
/// Lenient where readers are lenient (`3.` and `.5` parse as reals) and strict
/// where laxness would hide corruption (`1.2.3` is an error).
fn parse_number(text: &[u8]) -> Result<Token> {
    let invalid = || Error::InvalidNumber {
        text: String::from_utf8_lossy(text).into_owned(),
    };
    let text_str = core::str::from_utf8(text).map_err(|_| invalid())?;
    let has_dot = text.contains(&b'.');
    let mut dots = 0;
    for (index, byte) in text.iter().enumerate() {
        match byte {
            b'+' | b'-' if index == 0 => {}
            b'.' => {
                dots += 1;
                if dots > 1 {
                    return Err(invalid());
                }
            }
            b'0'..=b'9' => {}
            _ => return Err(invalid()),
        }
    }
    if has_dot {
        text_str
            .parse::<f64>()
            .map(Token::Real)
            .map_err(|_| invalid())
    } else {
        text_str
            .parse::<i64>()
            .map(Token::Integer)
            .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::{Lexer, Token};
    use crate::object::PdfString;

    fn tokens(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.as_bytes());
        let mut out = Vec::new();
        loop {
            let token = lexer.next_token().expect("token");
            if token == Token::Eof {
                return out;
            }
            out.push(token);
        }
    }

    #[test]
    fn whitespace_and_comments_are_skipped() {
        assert_eq!(
            tokens("  1\t% a comment\r\n2\n% trailing comment without newline"),
            vec![Token::Integer(1), Token::Integer(2)]
        );
    }

    #[test]
    fn numbers_split_into_integers_and_reals() {
        assert_eq!(
            tokens("0 -17 +42 2.375 -0.5 .5 3."),
            vec![
                Token::Integer(0),
                Token::Integer(-17),
                Token::Integer(42),
                Token::Real(2.375),
                Token::Real(-0.5),
                Token::Real(0.5),
                Token::Real(3.0),
            ]
        );
    }

    #[test]
    fn invalid_numbers_are_errors_not_keywords() {
        for text in ["1.2.3", "1-2", "--1", "+", ".", "9223372036854775808"] {
            let mut lexer = Lexer::new(text.as_bytes());
            let error = lexer.next_token().expect_err(text);
            assert!(
                matches!(error, crate::Error::InvalidNumber { .. }),
                "{text} gave {error}"
            );
        }
    }

    #[test]
    fn names_decode_hash_escapes() {
        assert_eq!(tokens("/Type"), vec![Token::Name(b"Type".to_vec())]);
        assert_eq!(tokens("/A#20Name"), vec![Token::Name(b"A Name".to_vec())]);
        assert_eq!(tokens("/#23hash"), vec![Token::Name(b"#hash".to_vec())]);
        assert_eq!(
            tokens("/Name#C3#A9"),
            vec![Token::Name(b"Name\xc3\xa9".to_vec())]
        );
        assert_eq!(tokens("/"), vec![Token::Name(Vec::new())]);
        // A name ends at a delimiter.
        assert_eq!(
            tokens("/Type/Page"),
            vec![Token::Name(b"Type".to_vec()), Token::Name(b"Page".to_vec())]
        );
    }

    #[test]
    fn malformed_name_escapes_are_errors() {
        for text in ["/Bad#zz", "/Bad#2"] {
            let mut lexer = Lexer::new(text.as_bytes());
            assert!(lexer.next_token().is_err(), "{text} must not lex");
        }
    }

    #[test]
    fn literal_strings_handle_nesting_and_escapes() {
        assert_eq!(
            tokens("(Hello, World!)"),
            vec![Token::String(PdfString::literal("Hello, World!"))]
        );
        assert_eq!(
            tokens("(a(b)c)"),
            vec![Token::String(PdfString::literal("a(b)c"))]
        );
        assert_eq!(
            tokens("(a\\(b\\)c)"),
            vec![Token::String(PdfString::literal("a(b)c"))]
        );
        assert_eq!(
            tokens("(back\\\\slash)"),
            vec![Token::String(PdfString::literal("back\\slash"))]
        );
        assert_eq!(
            tokens("(\\n\\r\\t\\b\\f)"),
            vec![Token::String(PdfString::literal(
                b"\n\r\t\x08\x0c".to_vec()
            ))]
        );
        assert_eq!(
            tokens("(\\101\\102)"),
            vec![Token::String(PdfString::literal("AB"))]
        );
        assert_eq!(
            tokens("(\\53)"),
            vec![Token::String(PdfString::literal("+"))]
        );
        assert_eq!(tokens("()"), vec![Token::String(PdfString::literal(""))]);
    }

    #[test]
    fn literal_strings_normalise_end_of_line_markers() {
        assert_eq!(
            tokens("(a\r\nb)"),
            vec![Token::String(PdfString::literal("a\nb"))]
        );
        assert_eq!(
            tokens("(a\rb)"),
            vec![Token::String(PdfString::literal("a\nb"))]
        );
        assert_eq!(
            tokens("(a\\\r\nb)"),
            vec![Token::String(PdfString::literal("ab"))],
            "backslash before an EOL continues the line"
        );
    }

    #[test]
    fn hex_strings_ignore_whitespace_and_pad_odd_digits() {
        assert_eq!(
            tokens("<48656C6C6F>"),
            vec![Token::String(PdfString::hex("Hello"))]
        );
        assert_eq!(
            tokens("<48 65 6c 6C 6f>"),
            vec![Token::String(PdfString::hex("Hello"))]
        );
        assert_eq!(tokens("<>"), vec![Token::String(PdfString::hex(""))]);
        assert_eq!(
            tokens("<414>"),
            vec![Token::String(PdfString::hex(vec![0x41, 0x40]))]
        );
    }

    #[test]
    fn unterminated_strings_are_eof_errors() {
        for text in ["(unterminated", "<48656C6C6F", "(trailing backslash\\"] {
            let mut lexer = Lexer::new(text.as_bytes());
            assert!(lexer.next_token().is_err(), "{text} must not lex");
        }
        let mut lexer = Lexer::new(b"<4G>");
        assert!(matches!(
            lexer.next_token(),
            Err(crate::Error::UnexpectedByte { .. })
        ));
    }

    #[test]
    fn delimiters_are_their_own_tokens() {
        assert_eq!(
            tokens("<< /A 1 >> [ ]"),
            vec![
                Token::DictionaryStart,
                Token::Name(b"A".to_vec()),
                Token::Integer(1),
                Token::DictionaryEnd,
                Token::ArrayStart,
                Token::ArrayEnd,
            ]
        );
        assert_eq!(
            tokens("<>"),
            vec![Token::String(PdfString::hex(""))],
            "a single < starts a hex string"
        );
        for text in [">", ")", "{", "}"] {
            let mut lexer = Lexer::new(text.as_bytes());
            assert!(lexer.next_token().is_err(), "{text} must not lex");
        }
    }

    #[test]
    fn keywords_are_regular_character_runs() {
        assert_eq!(
            tokens("true false null obj endobj stream R xref trailer"),
            vec![
                Token::Keyword(b"true".to_vec()),
                Token::Keyword(b"false".to_vec()),
                Token::Keyword(b"null".to_vec()),
                Token::Keyword(b"obj".to_vec()),
                Token::Keyword(b"endobj".to_vec()),
                Token::Keyword(b"stream".to_vec()),
                Token::Keyword(b"R".to_vec()),
                Token::Keyword(b"xref".to_vec()),
                Token::Keyword(b"trailer".to_vec()),
            ]
        );
    }

    #[test]
    fn peek_does_not_consume_and_position_reports_the_token_start() {
        let mut lexer = Lexer::new(b"  12 0 R");
        assert_eq!(lexer.position(), 0);
        assert_eq!(lexer.peek().expect("peek"), &Token::Integer(12));
        assert_eq!(lexer.position(), 2, "trivia is skipped by peek");
        assert_eq!(lexer.next_token().expect("token"), Token::Integer(12));
        assert_eq!(lexer.position(), 4, "just past the consumed token");
        assert_eq!(lexer.next_token().expect("token"), Token::Integer(0));
        assert_eq!(
            lexer.next_token().expect("token"),
            Token::Keyword(b"R".to_vec())
        );
        assert_eq!(lexer.next_token().expect("token"), Token::Eof);
        assert_eq!(lexer.next_token().expect("token"), Token::Eof);
    }

    #[test]
    fn seek_repositions_the_cursor() {
        let mut lexer = Lexer::new(b"12 0 R");
        assert_eq!(lexer.next_token().expect("token"), Token::Integer(12));
        lexer.seek(0);
        assert_eq!(lexer.next_token().expect("token"), Token::Integer(12));
        lexer.seek(1000);
        assert_eq!(lexer.next_token().expect("token"), Token::Eof);
    }
}
