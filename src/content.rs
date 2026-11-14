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

use alloc::string::String;
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::document::Document;
use crate::encoding::Encoding;
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
    /// Code → character table.
    encoding: Encoding,
}

impl Font {
    fn character(&self, code: u8) -> Option<char> {
        self.encoding.character(code)
    }
}

/// The text state and the glyph mapping needed to read a page.
struct TextInterpreter<'a> {
    document: &'a Document,
    resources: Option<Dictionary>,
    fonts: BTreeMap<Vec<u8>, Font>,
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
            fonts: BTreeMap::new(),
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
            .is_none_or(|y| difference(y, self.line_y) > LINE_TOLERANCE)
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
                return Err(Error::Unsupported {
                    what: "composite (Type0) fonts",
                });
            }
        }
        Ok(Font {
            encoding: self.encoding_of(&dict)?,
        })
    }

    fn encoding_of(&self, dict: &Dictionary) -> Result<Encoding> {
        let Some(encoding_object) = dict.get(b"Encoding") else {
            return Ok(Encoding::standard());
        };
        let encoding_object = self
            .document
            .resolve(Some(encoding_object))?
            .unwrap_or(Object::Null);
        let mut encoding = Encoding::from_object(Some(&encoding_object))?;
        if let Object::Dictionary(differences) = &encoding_object {
            if let Some(differences) = differences.get(b"Differences") {
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
                encoding.apply_differences(&items)?;
            }
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

/// `|left - right|` without the `std`-only `f64::abs`.
fn difference(left: f64, right: f64) -> f64 {
    if left >= right {
        left - right
    } else {
        right - left
    }
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

#[cfg(test)]
mod tests {
    use super::find_operator;

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
