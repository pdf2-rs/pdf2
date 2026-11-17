//! Simple-font encodings: code → character mapping (§9.6, §D.2).
//!
//! Text extraction and rendering both need the same mapping, so it lives here
//! instead of inside the content-stream interpreter: `StandardEncoding`,
//! `WinAnsiEncoding`, a font's `/Encoding` name or dictionary with
//! `/BaseEncoding` and `/Differences`, and glyph-name lookup for the
//! differences array.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Error, Result};
use crate::object::Object;

/// A simple font's code → character table, one entry per byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoding {
    table: Vec<Option<char>>,
}

impl Default for Encoding {
    fn default() -> Self {
        Self::standard()
    }
}

impl Encoding {
    fn from_table(table: Vec<Option<char>>) -> Self {
        Self { table }
    }

    /// `StandardEncoding`: ASCII with the two quote glyphs in their Adobe
    /// positions (§D.2).
    pub fn standard() -> Self {
        Self::from_table(standard_encoding())
    }

    /// `WinAnsiEncoding`: Latin-1 with the Windows-1252 punctuation block.
    pub fn win_ansi() -> Self {
        Self::from_table(win_ansi_encoding())
    }

    /// A base encoding by its `/Encoding` name; unknown names fall back to
    /// `StandardEncoding`.
    pub fn from_name(name: &[u8]) -> Self {
        Self::from_table(base_encoding(name))
    }

    /// Build the encoding of a font dictionary's `/Encoding` entry.
    ///
    /// `None` means the font has no `/Encoding` and gets `StandardEncoding`.
    /// Indirect values must be resolved by the caller; `/Differences` arrays
    /// must be resolved too and can be applied with
    /// [`Encoding::apply_differences`].
    pub fn from_object(object: Option<&Object>) -> Result<Self> {
        match object {
            None | Some(Object::Null) => Ok(Self::standard()),
            Some(Object::Name(name)) => Ok(Self::from_name(name)),
            Some(Object::Dictionary(dict)) => {
                let mut encoding = match dict.get(b"BaseEncoding") {
                    Some(Object::Name(base)) => Self::from_name(base),
                    _ => Self::standard(),
                };
                if let Some(Object::Array(items)) = dict.get(b"Differences") {
                    encoding.apply_differences(items)?;
                }
                Ok(encoding)
            }
            Some(_) => Err(Error::Malformed {
                what: "/Encoding must be a name or a dictionary",
                offset: None,
            }),
        }
    }

    /// Apply a `/Differences` array: code numbers followed by glyph names.
    pub fn apply_differences(&mut self, items: &[Object]) -> Result<()> {
        let mut code = 0_usize;
        for item in items {
            match item {
                Object::Integer(value) => {
                    code = usize::try_from(*value).map_err(|_| Error::Malformed {
                        what: "/Differences code must be a non-negative integer",
                        offset: None,
                    })?;
                }
                Object::Name(glyph) => {
                    if code < self.table.len() {
                        self.table[code] = glyph_name_character(glyph);
                    }
                    code += 1;
                }
                _ => {
                    return Err(Error::Malformed {
                        what: "/Differences entries must be integers or names",
                        offset: None,
                    });
                }
            }
        }
        Ok(())
    }

    /// The character for `code`, when the encoding maps it.
    pub fn character(&self, code: u8) -> Option<char> {
        self.table.get(usize::from(code)).copied().flatten()
    }

    /// How many codes the encoding maps, for tests and diagnostics.
    pub fn len(&self) -> usize {
        self.table
            .iter()
            .filter(|character| character.is_some())
            .count()
    }

    /// Whether the encoding maps no codes at all.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The character a glyph name stands for, for `/Differences` arrays.
///
/// Covers Latin punctuation and accented letters; unknown names are skipped
/// rather than guessed.
pub fn glyph_name_character(name: &[u8]) -> Option<char> {
    glyph_name_character_lookup(name)
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
fn glyph_name_character_lookup(name: &[u8]) -> Option<char> {
    if name.len() == 1 {
        return Some(char::from(name[0]));
    }
    let name = core::str::from_utf8(name).ok()?;
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
    use super::{Encoding, glyph_name_character};
    use crate::object::Object;

    #[test]
    fn win_ansi_uses_latin1_outside_the_punctuation_block() {
        let encoding = Encoding::win_ansi();
        assert_eq!(encoding.character(b'A'), Some('A'));
        assert_eq!(encoding.character(0xE9), Some('\u{00E9}'), "e-acute");
        assert_eq!(encoding.character(0x80), Some('\u{20AC}'), "euro");
        assert_eq!(encoding.character(0x95), Some('\u{2022}'), "bullet");
        assert_eq!(encoding.character(0x81), None, "undefined in cp1252");
    }

    #[test]
    fn standard_encoding_is_ascii_with_the_two_quote_names_swapped() {
        let encoding = Encoding::standard();
        assert_eq!(encoding.character(b' '), Some(' '));
        assert_eq!(encoding.character(b'~'), Some('~'));
        assert_eq!(encoding.character(0x27), Some('\u{2019}'));
        assert_eq!(encoding.character(0x60), Some('\u{2018}'));
        assert_eq!(encoding.character(0x00), None);
    }

    #[test]
    fn glyph_names_resolve_to_characters() {
        assert_eq!(glyph_name_character(b"A"), Some('A'));
        assert_eq!(glyph_name_character(b"7"), Some('7'));
        assert_eq!(glyph_name_character(b"bullet"), Some('\u{2022}'));
        assert_eq!(glyph_name_character(b"eacute"), Some('\u{00E9}'));
        assert_eq!(glyph_name_character(b"uni20AC"), Some('\u{20AC}'));
        assert_eq!(glyph_name_character(b"u0041"), Some('A'));
        assert_eq!(glyph_name_character(b"nosuchglyph"), None);
    }

    #[test]
    fn a_font_without_encoding_gets_standard() {
        assert_eq!(
            Encoding::from_object(None).expect("default"),
            Encoding::standard()
        );
        assert_eq!(
            Encoding::from_object(Some(&Object::Name(b"WinAnsiEncoding".to_vec()))).expect("name"),
            Encoding::win_ansi()
        );
    }

    #[test]
    fn differences_replace_codes_and_advance() {
        let mut dict = crate::object::Dictionary::new();
        dict.insert("BaseEncoding", Object::name("WinAnsiEncoding"));
        dict.insert(
            "Differences",
            Object::array([
                Object::Integer(65),
                Object::name("bullet"),
                Object::name("eacute"),
            ]),
        );
        let encoding = Encoding::from_object(Some(&Object::Dictionary(dict))).expect("encoding");
        assert_eq!(encoding.character(65), Some('\u{2022}'));
        assert_eq!(encoding.character(66), Some('\u{00E9}'));
        assert_eq!(
            encoding.character(67),
            Some('C'),
            "unchanged by differences"
        );
        assert!(encoding.len() > 200);
    }

    #[test]
    fn a_bad_encoding_value_is_an_error() {
        assert!(Encoding::from_object(Some(&Object::Integer(1))).is_err());
    }
}
