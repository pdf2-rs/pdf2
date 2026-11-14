//! Content-stream interpretation, far enough to extract text (§7.8.2, §9).
//!
//! The interpreter walks a decoded content stream with the same tokenizer the
//! object parser uses: operands are pushed on a stack and the operator that
//! follows consumes them. Only the text operators change what is extracted —
//! `BT`, `ET`, `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tc`, `Tw`, `Tz`, `Ts`,
//! `Tj`, `TJ`, `'` and `"` — while everything else is consumed and ignored, so
//! graphics-heavy pages do not disturb the text run.
//!
//! Limits, on purpose and documented in the README: simple fonts only (Type1,
//! TrueType, Type3 with a single-byte encoding); composite (Type0) fonts,
//! `/ToUnicode` CMaps and `/Widths`-based gap detection are not implemented, so
//! line breaks come from text-positioning operators and spaces from the text
//! itself or from large `TJ` adjustments.

use std::collections::HashMap;

use crate::document::Document;
use crate::error::{Error, Result};
use crate::lexer::Token;
use crate::object::{Dictionary, Object};
use crate::page::Page;
use crate::parser::Parser;

/// A `TJ` adjustment larger than this (in thousandths of an em) is treated as
/// a word gap. Wire values land between 200 and 400 for a real space.
const WORD_GAP: f64 = 200.0;

/// Movement in text space smaller than this is the same line.
const LINE_TOLERANCE: f64 = 0.01;

/// A simple font: one byte per code, resolved to characters.
#[derive(Debug, Clone)]
struct Font {
    /// Code → character, index by byte value.
    encoding: Vec<Option<char>>,
}

impl Font {
    fn character(&self, code: u8) -> Option<char> {
        self.encoding.get(usize::from(code)).copied().flatten()
    }
}

/// The text state and the glyph mapping needed to read a page.
struct TextInterpreter<'a> {
    document: &'a Document,
    resources: Option<Dictionary>,
    fonts: HashMap<Vec<u8>, Font>,
    font: Option<Font>,
    leading: f64,
    /// Current text line origin, in text space.
    line_x: f64,
    line_y: f64,
    /// Y of the last emitted text, for line-break detection.
    last_y: Option<f64>,
    output: String,
}

impl<'a> TextInterpreter<'a> {
    fn new(document: &'a Document, resources: Option<Dictionary>) -> Self {
        Self {
            document,
            resources,
            fonts: HashMap::new(),
            font: None,
            leading: 0.0,
            line_x: 0.0,
            line_y: 0.0,
            last_y: None,
            output: String::new(),
        }
    }

    fn run(mut self, content: &[u8]) -> Result<String> {
        let mut parser = Parser::new(content);
        let mut operands: Vec<Object> = Vec::new();
        loop {
            match parser.peek_token()? {
                Token::Eof => break,
                Token::Keyword(_) => {
                    let keyword = match parser.next_token()? {
                        Token::Keyword(keyword) => keyword,
                        _ => return Err(Error::malformed("content-stream operator")),
                    };
                    self.execute(&keyword, &mut operands, &mut parser)?;
                    operands.clear();
                }
                _ => {
                    let object = parser.parse_object()?;
                    operands.push(object);
                }
            }
        }
        Ok(self.output)
    }

    fn execute(
        &mut self,
        operator: &[u8],
        operands: &mut [Object],
        parser: &mut Parser<'_>,
    ) -> Result<()> {
        match operator {
            b"BT" => {
                self.line_x = 0.0;
                self.line_y = 0.0;
            }
            b"ET" => {}
            b"Tf" => {
                let name = match operands.first() {
                    Some(Object::Name(name)) => name.clone(),
                    _ => return Err(Error::malformed("/Tf needs a font name")),
                };
                // The size is not needed: line breaks come from positioning
                // operators, not from advance widths.
                self.font = Some(self.load_font(&name)?);
            }
            b"Td" | b"TD" => {
                let tx = number(operands.first());
                let ty = number(operands.get(1));
                self.line_x += tx;
                self.line_y += ty;
                if operator == b"TD" {
                    self.leading = -ty;
                }
            }
            b"T*" => {
                self.line_y -= self.leading;
            }
            b"TL" => {
                self.leading = number(operands.first());
            }
            b"Tm" => {
                // Only the translation part matters for text extraction.
                self.line_x = number(operands.get(4));
                self.line_y = number(operands.get(5));
            }
            // Character spacing, word spacing, horizontal scaling and rise
            // do not change which characters a text run produces.
            b"Tc" | b"Tw" | b"Tz" | b"Ts" => {}
            b"Tj" => {
                if let Some(Object::String(string)) = operands.first() {
                    self.show(string.as_bytes());
                }
            }
            b"'" => {
                self.line_y -= self.leading;
                if let Some(Object::String(string)) = operands.first() {
                    self.show(string.as_bytes());
                }
            }
            b"\"" => {
                // Set word and character spacing, then show on the next line.
                self.line_y -= self.leading;
                if let Some(Object::String(string)) = operands.get(2) {
                    self.show(string.as_bytes());
                }
            }
            b"TJ" => {
                if let Some(Object::Array(items)) = operands.first() {
                    for item in items {
                        match item {
                            Object::String(string) => self.show(string.as_bytes()),
                            Object::Integer(adjustment) => self.adjust(*adjustment as f64),
                            Object::Real(adjustment) => self.adjust(*adjustment),
                            _ => return Err(Error::malformed("unsupported /TJ array entry")),
                        }
                    }
                }
            }
            b"BI" => {
                // Inline images are opaque data; text cannot hide inside them,
                // so skip to the `EI` that closes them.
                skip_inline_image(parser)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn adjust(&mut self, adjustment: f64) {
        // Positive values move the pen left, so a wide negative adjustment is
        // how writers usually encode a space inside a `TJ` array.
        if -adjustment >= WORD_GAP && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        if self
            .last_y
            .is_none_or(|y| (y - self.line_y).abs() > LINE_TOLERANCE)
        {
            if !self.output.is_empty() && !self.output.ends_with('\n') {
                self.output.push('\n');
            }
            self.last_y = Some(self.line_y);
        }
        let Some(font) = &self.font else {
            return;
        };
        for &code in bytes {
            if let Some(character) = font.character(code) {
                self.output.push(character);
            }
        }
    }

    fn load_font(&mut self, name: &[u8]) -> Result<Font> {
        if let Some(font) = self.fonts.get(name) {
            return Ok(font.clone());
        }
        let font = self.read_font(name)?;
        self.fonts.insert(name.to_vec(), font.clone());
        Ok(font)
    }

    fn read_font(&self, name: &[u8]) -> Result<Font> {
        let font = match self
            .resources
            .as_ref()
            .and_then(|resources| resources.get(b"Font"))
        {
            Some(fonts) => {
                let fonts = self.document.resolve(Some(fonts))?.unwrap_or(Object::Null);
                match fonts {
                    Object::Dictionary(fonts) => fonts.get(name).cloned(),
                    _ => None,
                }
            }
            None => None,
        };
        let Some(font) = font else {
            return Err(Error::Malformed {
                what: "content stream selects a font the page does not declare",
                offset: None,
            });
        };
        let font = self.document.resolve(Some(&font))?.unwrap_or(Object::Null);
        let Object::Dictionary(dict) = font else {
            return Err(Error::Malformed {
                what: "font resource is not a dictionary",
                offset: None,
            });
        };
        if let Some(Object::Name(subtype)) = dict.get(b"Subtype") {
            if subtype == b"Type0" {
                return Err(Error::Malformed {
                    what: "composite (Type0) fonts are not supported yet",
                    offset: None,
                });
            }
        }
        Ok(Font {
            encoding: self.encoding_of(&dict)?,
        })
    }

    fn encoding_of(&self, dict: &Dictionary) -> Result<Vec<Option<char>>> {
        let mut encoding = standard_encoding();
        let Some(encoding_object) = dict.get(b"Encoding") else {
            return Ok(encoding);
        };
        let encoding_object = self
            .document
            .resolve(Some(encoding_object))?
            .unwrap_or(Object::Null);
        match encoding_object {
            Object::Name(name) => encoding = base_encoding(&name),
            Object::Dictionary(differences) => {
                if let Some(Object::Name(base)) = differences.get(b"BaseEncoding") {
                    encoding = base_encoding(base);
                }
                let Some(differences) = differences.get(b"Differences") else {
                    return Ok(encoding);
                };
                let differences = self
                    .document
                    .resolve(Some(differences))?
                    .unwrap_or(Object::Null);
                let Object::Array(items) = differences else {
                    return Err(Error::Malformed {
                        what: "/Differences must be an array",
                        offset: None,
                    });
                };
                let mut code = 0_usize;
                for item in &items {
                    match item {
                        Object::Integer(value) => {
                            code = usize::try_from(*value).map_err(|_| Error::Malformed {
                                what: "/Differences code must be a non-negative integer",
                                offset: None,
                            })?;
                        }
                        Object::Name(glyph) => {
                            if code < 256 {
                                encoding[code] = glyph_to_char(glyph);
                            }
                            code += 1;
                        }
                        _ => {
                            return Err(Error::Malformed {
                                what: "/Differences entries must be integers or names",
                                offset: None,
                            })
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(encoding)
    }
}

/// Skip inline image data: everything up to the `EI` operator.
fn skip_inline_image(parser: &mut Parser<'_>) -> Result<()> {
    let search = parser.raw_position();
    let found = parser
        .content()
        .get(search..)
        .and_then(|rest| find_operator(rest, b"EI"))
        .ok_or(Error::malformed("inline image without a closing EI"))?;
    parser.seek(search + found + 2);
    Ok(())
}

/// Find `needle` as a whole operator: preceded by whitespace or a delimiter.
fn find_operator(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if &haystack[index..index + needle.len()] == needle {
            let before_ok = index == 0
                || crate::lexer::is_whitespace(haystack[index - 1])
                || crate::lexer::is_delimiter(haystack[index - 1]);
            let after = index + needle.len();
            let after_ok = after >= haystack.len()
                || crate::lexer::is_whitespace(haystack[after])
                || crate::lexer::is_delimiter(haystack[after]);
            if before_ok && after_ok {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

fn number(operand: Option<&Object>) -> f64 {
    match operand {
        Some(Object::Integer(value)) => *value as f64,
        Some(Object::Real(value)) => *value,
        _ => 0.0,
    }
}

impl Document {
    /// Extract the text of a page's content streams.
    pub fn extract_text(&self, page: &Page) -> Result<String> {
        let content = self.page_content(page)?;
        self.extract_text_from(&content, page.resources.as_ref())
    }

    /// Extract text from a decoded content stream and its resource dictionary.
    pub fn extract_text_from(
        &self,
        content: &[u8],
        resources: Option<&Dictionary>,
    ) -> Result<String> {
        TextInterpreter::new(self, resources.cloned()).run(content)
    }
}

/// The ASCII part of `StandardEncoding` (§D.2), which is what almost every
/// simple font uses for its codes 32–126.
fn standard_encoding() -> Vec<Option<char>> {
    let mut encoding = vec![None; 256];
    for code in 32_u8..=126 {
        encoding[usize::from(code)] = Some(char::from(code));
    }
    encoding[0x27] = Some('\u{2019}'); // quoteright
    encoding[0x60] = Some('\u{2018}'); // quoteleft
    encoding
}

/// `WinAnsiEncoding` is Latin-1 with the `0x80..=0x9F` range replaced by the
/// Windows-1252 punctuation block (§D.2).
fn win_ansi_encoding() -> Vec<Option<char>> {
    let mut encoding = vec![None; 256];
    for code in 0_u8..=255 {
        encoding[usize::from(code)] = match code {
            0x80 => Some('\u{20AC}'),
            0x82 => Some('\u{201A}'),
            0x83 => Some('\u{0192}'),
            0x84 => Some('\u{201E}'),
            0x85 => Some('\u{2026}'),
            0x86 => Some('\u{2020}'),
            0x87 => Some('\u{2021}'),
            0x88 => Some('\u{02C6}'),
            0x89 => Some('\u{2030}'),
            0x8A => Some('\u{0160}'),
            0x8B => Some('\u{2039}'),
            0x8C => Some('\u{0152}'),
            0x8E => Some('\u{017D}'),
            0x91 => Some('\u{2018}'),
            0x92 => Some('\u{2019}'),
            0x93 => Some('\u{201C}'),
            0x94 => Some('\u{201D}'),
            0x95 => Some('\u{2022}'),
            0x96 => Some('\u{2013}'),
            0x97 => Some('\u{2014}'),
            0x98 => Some('\u{02DC}'),
            0x99 => Some('\u{2122}'),
            0x9A => Some('\u{0161}'),
            0x9B => Some('\u{203A}'),
            0x9C => Some('\u{0153}'),
            0x9E => Some('\u{017E}'),
            0x9F => Some('\u{0178}'),
            0x81 | 0x8D | 0x8F | 0x90 | 0x9D => None,
            _ => Some(char::from(code)),
        };
    }
    encoding
}

fn base_encoding(name: &[u8]) -> Vec<Option<char>> {
    match name {
        b"WinAnsiEncoding" => win_ansi_encoding(),
        // MacRomanEncoding shares the ASCII range with StandardEncoding; the
        // upper half is not implemented, which the README states.
        b"StandardEncoding" | b"MacRomanEncoding" | b"PDFDocEncoding" => standard_encoding(),
        _ => standard_encoding(),
    }
}

/// Glyph names to characters, for `/Differences` arrays.
///
/// Covers the Latin punctuation and accented letters that real documents use;
/// anything else is skipped rather than guessed.
fn glyph_to_char(name: &[u8]) -> Option<char> {
    if name.len() == 1 {
        return Some(char::from(name[0]));
    }
    let name = std::str::from_utf8(name).ok()?;
    let named = match name {
        "space" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" => '\'',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" => '-',
        "period" => '.',
        "slash" => '/',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "backslash" => '\\',
        "bracketright" => ']',
        "asciicircum" => '^',
        "underscore" => '_',
        "grave" => '`',
        "braceleft" => '{',
        "bar" => '|',
        "braceright" => '}',
        "asciitilde" => '~',
        "bullet" => '\u{2022}',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "quoteleft" => '\u{2018}',
        "quoteright" => '\u{2019}',
        "quotedblleft" => '\u{201C}',
        "quotedblright" => '\u{201D}',
        "ellipsis" => '\u{2026}',
        "dagger" => '\u{2020}',
        "daggerdbl" => '\u{2021}',
        "section" => '\u{00A7}',
        "paragraph" => '\u{00B6}',
        "degree" => '\u{00B0}',
        "plusminus" => '\u{00B1}',
        "multiply" => '\u{00D7}',
        "divide" => '\u{00F7}',
        "copyright" => '\u{00A9}',
        "registered" => '\u{00AE}',
        "trademark" => '\u{2122}',
        "eacute" => '\u{00E9}',
        "egrave" => '\u{00E8}',
        "agrave" => '\u{00E0}',
        "ccedilla" => '\u{00E7}',
        "udieresis" => '\u{00FC}',
        "odieresis" => '\u{00F6}',
        "adieresis" => '\u{00E4}',
        "germandbls" => '\u{00DF}',
        "fi" => '\u{FB01}',
        "fl" => '\u{FB02}',
        "zero" => '0',
        "one" => '1',
        "two" => '2',
        "three" => '3',
        "four" => '4',
        "five" => '5',
        "six" => '6',
        "seven" => '7',
        "eight" => '8',
        "nine" => '9',
        _ => {
            // `uniXXXX` and `uXXXX` names carry their code point directly.
            let hex = name
                .strip_prefix("uni")
                .or_else(|| name.strip_prefix('u'))?;
            return char::from_u32(u32::from_str_radix(hex, 16).ok()?);
        }
    };
    Some(named)
}

#[cfg(test)]
mod tests {
    use super::{find_operator, glyph_to_char, standard_encoding, win_ansi_encoding};

    #[test]
    fn win_ansi_uses_latin1_outside_the_punctuation_block() {
        let encoding = win_ansi_encoding();
        assert_eq!(encoding[usize::from(b'A')], Some('A'));
        assert_eq!(encoding[0xE9], Some('\u{00E9}'), "e-acute");
        assert_eq!(encoding[0x80], Some('\u{20AC}'), "euro");
        assert_eq!(encoding[0x95], Some('\u{2022}'), "bullet");
        assert_eq!(encoding[0x81], None, "undefined in cp1252");
    }

    #[test]
    fn standard_encoding_is_ascii_with_the_two_quote_names_swapped() {
        let encoding = standard_encoding();
        assert_eq!(encoding[usize::from(b' ')], Some(' '));
        assert_eq!(encoding[usize::from(b'~')], Some('~'));
        assert_eq!(encoding[0x27], Some('\u{2019}'));
        assert_eq!(encoding[0x60], Some('\u{2018}'));
        assert_eq!(encoding[0x00], None);
    }

    #[test]
    fn glyph_names_resolve_to_characters() {
        assert_eq!(glyph_to_char(b"A"), Some('A'));
        assert_eq!(glyph_to_char(b"7"), Some('7'));
        assert_eq!(glyph_to_char(b"bullet"), Some('\u{2022}'));
        assert_eq!(glyph_to_char(b"eacute"), Some('\u{00E9}'));
        assert_eq!(glyph_to_char(b"uni20AC"), Some('\u{20AC}'));
        assert_eq!(glyph_to_char(b"u0041"), Some('A'));
        assert_eq!(glyph_to_char(b"nosuchglyph"), None);
    }

    #[test]
    fn inline_image_terminator_must_be_a_whole_operator() {
        assert_eq!(find_operator(b"abc EI more", b"EI"), Some(4));
        assert_eq!(find_operator(b"EI", b"EI"), Some(0));
        assert_eq!(
            find_operator(b"tailEIx", b"EI"),
            None,
            "EI inside a word is not the operator"
        );
        assert_eq!(find_operator(b"no terminator", b"EI"), None);
    }
}
