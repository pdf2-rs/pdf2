//! `/ToUnicode` CMaps (`9.10.3`), enough to map character codes to text.
//!
//! A `/ToUnicode` CMap is a PostScript-ish program whose meaningful parts are the
//! three delimited sections — `begincodespacerange`, `beginbfchar` and
//! `beginbfrange` — and this module reads exactly those, in a byte-level lexer
//! rather than a whitespace tokenizer, because the traps are in the bytes:
//! `<0001><0070>` is two codes with no separator between them, hex strings may
//! contain whitespace, and `%` comments run to the end of the line.
//!
//! The parser is bounded from its first commit: at most [`MAX_MAPPINGS`] entries
//! and [`MAX_CODESPACE_RANGES`] code space ranges, both checked before anything is
//! expanded, so a hostile `bfrange` cannot demand millions of entries. Anything
//! it cannot read is an [`Error::Malformed`], never a panic and never a hang.
//!
//! Predefined CMaps — `/90ms-RKSJ-H` and friends — are data, not code: the one
//! the corpus needs is generated into `src/data/japan1_90ms_rksj.bin` by
//! `cargo xtask cmaps` from Adobe's CMap resources (BSD-3-Clause, notice in
//! `vendor/adobe-cmaps/LICENSE.md`), and its size is recorded against the wasm
//! budget in `docs/PERFORMANCE.md`. Adding another is a data and one-line change,
//! which is the point of the generated table.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};

/// The most entries a CMap may map, across `bfchar` and `bfrange`.
pub(crate) const MAX_MAPPINGS: u32 = 1 << 20;

/// The most `codespacerange` pairs a CMap may declare.
pub(crate) const MAX_CODESPACE_RANGES: u32 = 1 << 16;

/// A code's widest allowed width in bytes: `1..=4` per `9.10.3`.
const MAX_CODE_BYTES: usize = 4;

/// Character codes mapped to the text they stand for.
#[derive(Debug, Clone)]
pub(crate) struct ToUnicode {
    /// Mappings, keyed by code width and code value (big-endian).
    mappings: BTreeMap<(u8, u32), String>,
    /// Code widths the code space allows, ascending, without duplicates.
    code_lengths: Vec<u8>,
}

/// A predefined CMap's code → text table, generated at build time.
///
/// The file layout is `P2CV`, one byte per allowed code width, a big-endian `u16`
/// count, then that many varint entries of `(code delta, zig-zag text delta)` —
/// scanning stops when the sorted codes pass the one asked for, so a lookup
/// allocates nothing and reads a few thousand bytes at worst.
#[derive(Debug, Clone)]
pub(crate) struct Predefined {
    /// The generated table, as it ships.
    data: &'static [u8],
    /// Allowed code widths, ascending.
    lengths: &'static [u8],
}

const JAPAN1_90MS_RKSJ: &[u8] = include_bytes!("data/japan1_90ms_rksj.bin");
const JAPAN1_UNIJIS_UCS2: &[u8] = include_bytes!("data/japan1_unijis_ucs2.bin");

/// Whether a table maps codes to text, or only lists the codes it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// `P2CV`: (code delta, zig-zag text delta) per code.
    Codes,
    /// `P2VR`: (start delta, run length) per run, and a code's text is the code —
    /// which is what a *Unicode* CMap means, so 9 772 identity entries are stored
    /// as ranges instead.
    Ranges,
}

impl Predefined {
    /// The predefined CMap an `/Encoding` name asks for, when it is shipped.
    ///
    /// Names are compared **case-insensitively**: `/90MS-RKSJ-H` is the spelling
    /// many writers emit, and the exact-match comparison this replaced treated it
    /// as unknown, so a CJK page lost its text to a capital letter.
    pub(crate) fn by_name(name: &[u8]) -> Option<Self> {
        let lowered = name.to_ascii_lowercase();
        let data = match lowered.as_slice() {
            b"90ms-rksj-h" => JAPAN1_90MS_RKSJ,
            b"unijis-ucs2-h" => JAPAN1_UNIJIS_UCS2,
            _ => return None,
        };
        let lengths = &data[4..6];
        Some(Self { data, lengths })
    }

    fn kind(&self) -> Kind {
        if self.data.starts_with(b"P2VR") {
            Kind::Ranges
        } else {
            Kind::Codes
        }
    }

    /// The text a code stands for.
    pub(crate) fn text(&self, code: u32, length: u8) -> Option<char> {
        if !self.lengths.contains(&length) {
            return None;
        }
        if self.kind() == Kind::Ranges {
            // A code this CMap covers stands for the character with that value.
            return self
                .covers(code)
                .then(|| core::char::from_u32(code))
                .flatten();
        }
        let code = u16::try_from(code).ok()?;
        // A one-byte code space holds codes below 0x100 and a two-byte one holds
        // codes above it, so the width is part of the key.
        if (length == 1) != (code < 0x100) {
            return None;
        }
        let mut position = 8;
        let mut current_code = 0_u32;
        let mut current_text = 0_u32;
        loop {
            let (code_delta, after) = read_varint(self.data, position)?;
            position = after;
            let (zigzag, after) = read_varint(self.data, position)?;
            position = after;
            current_code = current_code.checked_add(code_delta)?;
            if current_code > u32::from(code) {
                return None;
            }
            let delta = i64::from(zigzag >> 1) ^ -i64::from(zigzag & 1);
            current_text = u32::try_from(i64::from(current_text) + delta).ok()?;
            if current_code == u32::from(code) {
                return core::char::from_u32(current_text);
            }
        }
    }

    /// Whether a range table lists `code`.
    fn covers(&self, code: u32) -> bool {
        let Some(count) = self.data.get(6..8) else {
            return false;
        };
        let count = usize::from(u16::from_be_bytes([count[0], count[1]]));
        let mut position = 8;
        let mut start = 0_u32;
        for _ in 0..count {
            let Some((delta, after)) = read_varint(self.data, position) else {
                return false;
            };
            position = after;
            start = start.saturating_add(delta);
            let Some((length, after)) = read_varint(self.data, position) else {
                return false;
            };
            position = after;
            if code < start {
                return false;
            }
            if code < start.saturating_add(length) {
                return true;
            }
        }
        false
    }

    /// The code widths the CMap allows, ascending.
    pub(crate) fn code_lengths(&self) -> &[u8] {
        self.lengths
    }

    /// The widest code the CMap allows.
    pub(crate) fn max_code_length(&self) -> u8 {
        self.lengths.last().copied().unwrap_or(2)
    }
}

/// Read one little-endian base-128 varint, with the position after it.
fn read_varint(data: &[u8], mut position: usize) -> Option<(u32, usize)> {
    let mut value = 0_u32;
    let mut shift = 0;
    loop {
        let byte = *data.get(position)?;
        position += 1;
        value |= u32::from(byte & 0x7F).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some((value, position));
        }
        shift += 7;
        if shift >= 32 {
            return None;
        }
    }
}

impl ToUnicode {
    /// Parse a decoded `/ToUnicode` stream.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] when the program is unterminated, an operand is the
    /// wrong kind for its section, a code is wider than four bytes, a range's
    /// bounds are reversed or its array destination does not match its domain,
    /// or a bound in this module is exceeded.
    pub(crate) fn parse(data: &[u8]) -> Result<Self> {
        let mut mappings = BTreeMap::new();
        let mut lengths: Vec<u8> = Vec::new();
        let mut ranges: u32 = 0;
        let mut lexer = Lexer::new(data);
        while let Some(token) = lexer.next_token()? {
            match token {
                Token::Keyword(keyword) if keyword == b"begincodespacerange" => {
                    ranges = read_codespace(&mut lexer, ranges, &mut lengths)?;
                }
                Token::Keyword(keyword) if keyword == b"beginbfchar" => {
                    read_bfchar(&mut lexer, &mut mappings)?;
                }
                Token::Keyword(keyword) if keyword == b"beginbfrange" => {
                    read_bfrange(&mut lexer, &mut mappings)?;
                }
                _ => {}
            }
        }
        if lengths.is_empty() {
            return Err(Error::malformed("a /ToUnicode CMap without a codespace"));
        }
        lengths.sort_unstable();
        lengths.dedup();
        Ok(Self {
            mappings,
            code_lengths: lengths,
        })
    }

    /// The text `code` stands for, when the CMap maps it.
    ///
    /// `length` is the code's width in bytes, as the code space dictates.
    pub(crate) fn text(&self, code: u32, length: u8) -> Option<&str> {
        self.mappings.get(&(length, code)).map(String::as_str)
    }

    /// The code widths the CMap's code space allows, ascending.
    pub(crate) fn code_lengths(&self) -> &[u8] {
        &self.code_lengths
    }

    /// The widest code the code space allows, at most four bytes.
    pub(crate) fn max_code_length(&self) -> u8 {
        self.code_lengths.last().copied().unwrap_or(1)
    }

    /// How many codes the CMap maps, for tests and diagnostics.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.mappings.len()
    }
}

fn read_codespace(lexer: &mut Lexer<'_>, mut ranges: u32, lengths: &mut Vec<u8>) -> Result<u32> {
    loop {
        match lexer.next_token()? {
            None => return Err(Error::malformed("codespacerange without its end")),
            Some(Token::Keyword(keyword)) if keyword == b"endcodespacerange" => return Ok(ranges),
            Some(Token::Hex(low)) => {
                let Some(Token::Hex(high)) = lexer.next_token()? else {
                    return Err(Error::malformed("codespacerange needs a second bound"));
                };
                if low.len() != high.len() || low.is_empty() || low.len() > MAX_CODE_BYTES {
                    return Err(Error::malformed(
                        "codespacerange bounds need one width of 1 to 4 bytes",
                    ));
                }
                ranges = ranges
                    .checked_add(1)
                    .filter(|ranges| *ranges <= MAX_CODESPACE_RANGES)
                    .ok_or_else(|| Error::malformed("too many codespace ranges"))?;
                lengths.push(u8::try_from(low.len()).map_err(|_| {
                    Error::malformed("codespacerange bounds need one width of 1 to 4 bytes")
                })?);
            }
            Some(_) => return Err(Error::malformed("codespacerange needs a bound")),
        }
    }
}

fn read_bfchar(lexer: &mut Lexer<'_>, mappings: &mut BTreeMap<(u8, u32), String>) -> Result<()> {
    loop {
        match lexer.next_token()? {
            None => return Err(Error::malformed("bfchar without its end")),
            Some(Token::Keyword(keyword)) if keyword == b"endbfchar" => return Ok(()),
            Some(Token::Hex(source)) => {
                let Some(Token::Hex(destination)) = lexer.next_token()? else {
                    return Err(Error::malformed("bfchar needs a destination"));
                };
                if source.is_empty() || source.len() > MAX_CODE_BYTES {
                    return Err(Error::malformed("bfchar source must be 1 to 4 bytes"));
                }
                let width = u8::try_from(source.len())
                    .map_err(|_| Error::malformed("bfchar source must be 1 to 4 bytes"))?;
                insert(mappings, width, pack(&source), decode(&destination))?;
            }
            Some(_) => return Err(Error::malformed("bfchar needs a source")),
        }
    }
}

fn read_bfrange(lexer: &mut Lexer<'_>, mappings: &mut BTreeMap<(u8, u32), String>) -> Result<()> {
    loop {
        match lexer.next_token()? {
            None => return Err(Error::malformed("bfrange without its end")),
            Some(Token::Keyword(keyword)) if keyword == b"endbfrange" => return Ok(()),
            Some(Token::Hex(low)) => read_one_range(lexer, mappings, &low)?,
            Some(_) => return Err(Error::malformed("bfrange needs a lower bound")),
        }
    }
}

fn read_one_range(
    lexer: &mut Lexer<'_>,
    mappings: &mut BTreeMap<(u8, u32), String>,
    low: &[u8],
) -> Result<()> {
    let Some(Token::Hex(high)) = lexer.next_token()? else {
        return Err(Error::malformed("bfrange needs an upper bound"));
    };
    if low.len() != high.len() || low.is_empty() || low.len() > MAX_CODE_BYTES {
        return Err(Error::malformed(
            "bfrange bounds need one width of 1 to 4 bytes",
        ));
    }
    let width = u8::try_from(low.len())
        .map_err(|_| Error::malformed("bfrange bounds need one width of 1 to 4 bytes"))?;
    let first = pack(low);
    let last = pack(&high);
    if first > last {
        return Err(Error::malformed("bfrange bounds are reversed"));
    }
    let count = last - first + 1;
    if count > MAX_MAPPINGS {
        return Err(Error::malformed("bfrange asks for too many mappings"));
    }
    match lexer.next_token()? {
        Some(Token::Hex(destination)) => {
            let mut value = destination;
            for code in first..=last {
                insert(mappings, width, code, decode(&value))?;
                // The destination's last byte is incremented per `9.10.3`; the
                // carry keeps a two-byte destination sane past 0xFF, which the
                // standard leaves undefined but writers still rely on.
                if !increment_be(&mut value) {
                    break;
                }
            }
        }
        Some(Token::ArrayOpen) => {
            let mut index = first;
            loop {
                match lexer.next_token()? {
                    None => return Err(Error::malformed("bfrange array is not closed")),
                    Some(Token::ArrayClose) => break,
                    Some(Token::Hex(destination)) => {
                        if index > last {
                            return Err(Error::malformed(
                                "bfrange array has more entries than its domain",
                            ));
                        }
                        insert(mappings, width, index, decode(&destination))?;
                        index += 1;
                    }
                    Some(_) => return Err(Error::malformed("bfrange array needs strings")),
                }
            }
            if index != last + 1 {
                return Err(Error::malformed(
                    "bfrange array has fewer entries than its domain",
                ));
            }
        }
        _ => return Err(Error::malformed("bfrange needs a destination")),
    }
    Ok(())
}

fn insert(
    mappings: &mut BTreeMap<(u8, u32), String>,
    width: u8,
    code: u32,
    text: String,
) -> Result<()> {
    if mappings.len() >= MAX_MAPPINGS as usize && !mappings.contains_key(&(width, code)) {
        return Err(Error::malformed("too many mappings"));
    }
    mappings.insert((width, code), text);
    Ok(())
}

/// Turn a destination string into text: UTF-16BE when even, bytes as Latin-1 when
/// odd (some writers emit one byte where the standard asks for two).
fn decode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() / 2);
    if bytes.len() % 2 == 1 {
        for &byte in bytes {
            text.push(char::from(byte));
        }
        return text;
    }
    for pair in bytes.as_chunks::<2>().0 {
        let unit = u16::from_be_bytes(*pair);
        text.push(core::char::from_u32(u32::from(unit)).unwrap_or('\u{FFFD}'));
    }
    text
}

fn pack(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |value, &byte| (value << 8) | u32::from(byte))
}

/// Increment a big-endian byte string in place; `false` when it wrapped.
fn increment_be(bytes: &mut [u8]) -> bool {
    for byte in bytes.iter_mut().rev() {
        let (next, carry) = byte.overflowing_add(1);
        *byte = next;
        if !carry {
            return true;
        }
    }
    false
}

enum Token {
    Hex(Vec<u8>),
    Keyword(Vec<u8>),
    ArrayOpen,
    ArrayClose,
}

/// A byte-level lexer for the parts of a CMap this module reads.
struct Lexer<'a> {
    data: &'a [u8],
    position: usize,
}

impl Lexer<'_> {
    fn new(data: &[u8]) -> Lexer<'_> {
        Lexer { data, position: 0 }
    }

    fn next_token(&mut self) -> Result<Option<Token>> {
        loop {
            let Some(&byte) = self.data.get(self.position) else {
                return Ok(None);
            };
            match byte {
                b'%' => {
                    while self
                        .data
                        .get(self.position)
                        .is_some_and(|&byte| byte != b'\n')
                    {
                        self.position += 1;
                    }
                }
                b'<' => {
                    // `<<` opens a PostScript dictionary; only a single `<`
                    // starts a hex string this module needs.
                    if self.data.get(self.position + 1) == Some(&b'<') {
                        self.position += 2;
                        continue;
                    }
                    self.position += 1;
                    let mut digits: Vec<u8> = Vec::new();
                    loop {
                        let Some(&byte) = self.data.get(self.position) else {
                            return Err(Error::malformed("unterminated hex string"));
                        };
                        self.position += 1;
                        match byte {
                            b'>' => break,
                            b'0'..=b'9' => digits.push(byte - b'0'),
                            b'a'..=b'f' => digits.push(byte - b'a' + 10),
                            b'A'..=b'F' => digits.push(byte - b'A' + 10),
                            byte if is_space(byte) => {}
                            _ => return Err(Error::malformed("a hex string with a stray byte")),
                        }
                    }
                    if !digits.len().is_multiple_of(2) {
                        return Err(Error::malformed("a hex string with an odd digit count"));
                    }
                    let mut bytes = Vec::with_capacity(digits.len() / 2);
                    for pair in digits.as_chunks::<2>().0 {
                        bytes.push((pair[0] << 4) | pair[1]);
                    }
                    return Ok(Some(Token::Hex(bytes)));
                }
                b'[' => {
                    self.position += 1;
                    return Ok(Some(Token::ArrayOpen));
                }
                b']' => {
                    self.position += 1;
                    return Ok(Some(Token::ArrayClose));
                }
                b'{' | b'}' | b')' | b'(' | b'>' => {
                    // Braces, literal strings and stray closers carry nothing this
                    // module reads; `<<` and `>>` are consumed a byte at a time.
                    self.position += 1;
                }
                byte if is_space(byte) => self.position += 1,
                _ => {
                    let start = self.position;
                    while self
                        .data
                        .get(self.position)
                        .is_some_and(|&byte| !is_space(byte) && !is_special(byte))
                    {
                        self.position += 1;
                    }
                    if start == self.position {
                        return Err(Error::malformed("stray byte in a /ToUnicode CMap"));
                    }
                    return Ok(Some(Token::Keyword(
                        self.data[start..self.position].to_vec(),
                    )));
                }
            }
        }
    }
}

fn is_space(byte: u8) -> bool {
    crate::lexer::is_whitespace(byte)
}

fn is_special(byte: u8) -> bool {
    matches!(
        byte,
        b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'%' | b'(' | b')'
    )
}

#[cfg(test)]
mod tests {
    use super::{MAX_CODESPACE_RANGES, MAX_MAPPINGS, ToUnicode};
    use crate::document::Document;
    use crate::object::Object;
    use alloc::format;
    use alloc::string::String;

    const SIMPLE: &str = "\
/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
<< /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
2 beginbfchar
<0003> <0041>
<0004> <0044>
endbfchar
1 beginbfrange
<0010> <0012> [<0061> <0062> <0063>]
endbfrange
1 beginbfrange
<0020> <0022> <0078>
endbfrange
endcmap
";

    fn parse(text: &str) -> ToUnicode {
        ToUnicode::parse(text.as_bytes()).expect("parse")
    }

    #[test]
    fn bfchar_bfrange_and_arrays_map_known_codes() {
        let cmap = parse(SIMPLE);
        assert_eq!(cmap.code_lengths(), &[2]);
        assert_eq!(cmap.max_code_length(), 2);
        assert_eq!(cmap.text(0x0003, 2), Some("A"));
        assert_eq!(cmap.text(0x0004, 2), Some("D"));
        // The array destination is taken entry by entry.
        assert_eq!(cmap.text(0x0010, 2), Some("a"));
        assert_eq!(cmap.text(0x0011, 2), Some("b"));
        assert_eq!(cmap.text(0x0012, 2), Some("c"));
        // A single destination increments its last byte.
        assert_eq!(cmap.text(0x0020, 2), Some("x"));
        assert_eq!(cmap.text(0x0021, 2), Some("y"));
        assert_eq!(cmap.text(0x0022, 2), Some("z"));
        assert_eq!(cmap.text(0x0023, 2), None);
        assert_eq!(cmap.text(0x0003, 1), None, "the width is part of the key");
        assert_eq!(cmap.len(), 8);
    }

    #[test]
    fn hex_strings_need_no_separator_and_may_hold_whitespace() {
        let cmap = parse(
            "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
             2 beginbfchar <0001><0070> <00 02> <0041> endbfchar",
        );
        assert_eq!(cmap.text(0x0001, 2), Some("p"));
        assert_eq!(cmap.text(0x0002, 2), Some("A"));
    }

    #[test]
    fn comments_dictionaries_and_odd_widths_are_read() {
        let cmap = parse(
            "% a comment <DEAD> that maps nothing\n\
             1 begincodespacerange <00> <FF> endcodespacerange\n\
             1 beginbfchar <41> <41> endbfchar",
        );
        assert_eq!(cmap.code_lengths(), &[1]);
        assert_eq!(cmap.text(0x41, 1), Some("A"), "a one-byte destination");
    }

    #[test]
    fn a_range_longer_than_a_byte_carries_instead_of_repeating() {
        // 0x0041 + 0x101 would wrap a single byte; the carry is what writers mean.
        let cmap = parse(
            "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
             1 beginbfrange <0000> <0101> <0041> endbfrange",
        );
        assert_eq!(cmap.text(0x0100, 2), Some("\u{141}"));
    }

    #[test]
    fn a_four_byte_code_is_allowed_and_five_is_not() {
        let four = parse(
            "1 begincodespacerange <00000000> <FFFFFFFF> endcodespacerange\n\
             1 beginbfchar <00000041> <0041> endbfchar",
        );
        assert_eq!(four.max_code_length(), 4);
        assert_eq!(four.text(0x41, 4), Some("A"));

        assert!(
            ToUnicode::parse(b"1 begincodespacerange <0000000000> <FFFFFFFFFF> endcodespacerange")
                .is_err(),
            "a five-byte code is out of range"
        );
    }

    #[test]
    fn malformed_cmaps_are_errors_not_panics() {
        let cases: &[&[u8]] = &[
            b"1 begincodespacerange <0000> <FFFF>",
            b"1 begincodespacerange <0000> <FFFFFF> endcodespacerange",
            b"1 begincodespacerange <> <> endcodespacerange",
            b"1 beginbfchar <0001> <0041>",
            b"1 beginbfchar <0001>",
            b"1 beginbfchar <0001> <00> endbfchar",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0012> <0010> <0041> endbfrange",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0010> <0012> [<0061>] endbfrange",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0010> <0011> [<0061> <0062> <0063>] endbfrange",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0010> <0012> <0041",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange 1 beginbfrange <0000> <00FFFFFF> <0041> endbfrange",
            b"no codespace at all",
            b"1 begincodespacerange <0000> <FFF> endcodespacerange",
            b"1 beginbfchar <0G01> <0041> endbfchar",
        ];
        for case in cases {
            assert!(
                ToUnicode::parse(case).is_err(),
                "should not parse: {}",
                String::from_utf8_lossy(case)
            );
        }
    }

    #[test]
    fn the_bounds_refuse_growth_before_expanding_it() {
        let allowed = parse(
            "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
             1 beginbfrange <0000> <FFFF> <00000041> endbfrange",
        );
        assert_eq!(allowed.len(), 65_536);

        let over = format!(
            "1 begincodespacerange <0000> <7FFFFF> endcodespacerange\n\
             1 beginbfrange <00000000> <{:X}> <0041> endbfrange",
            MAX_MAPPINGS + 1
        );
        assert!(
            ToUnicode::parse(over.as_bytes()).is_err(),
            "more than {MAX_MAPPINGS} mappings is refused"
        );

        let mut ranges = String::new();
        for _ in 0..=MAX_CODESPACE_RANGES {
            ranges.push_str("<0000> <FFFF> ");
        }
        let text = format!(
            "1 begincodespacerange {} endcodespacerange",
            ranges.trim_end()
        );
        assert!(
            ToUnicode::parse(text.as_bytes()).is_err(),
            "more than {MAX_CODESPACE_RANGES} code space ranges is refused"
        );
    }

    #[test]
    fn the_predefined_cmap_reads_shift_jis() {
        // Both spellings, because a writer's capital letters must not cost a page
        // its text: `90MS-RKSJ-H` is legal and common.
        let map = super::Predefined::by_name(b"90ms-RKSJ-H").expect("shipped");
        let upper = super::Predefined::by_name(b"90MS-RKSJ-H").expect("case-insensitive");
        assert_eq!(upper.text(0x93FA, 2), map.text(0x93FA, 2));
        assert_eq!(map.code_lengths(), &[1, 2]);
        assert_eq!(map.max_code_length(), 2);
        // `Hello` in single-byte codes, then the fixture's own text.
        assert_eq!(map.text(0x48, 1), Some('H'));
        assert_eq!(map.text(0x93FA, 2), Some('\u{65E5}'), "日, not the radical");
        assert_eq!(map.text(0x967B, 2), Some('\u{672C}'));
        assert_eq!(map.text(0x8CEA, 2), Some('\u{8A9E}'));
        assert_eq!(map.text(0x8365, 2), Some('\u{30C6}'));
        assert_eq!(map.text(0xFFFF, 2), None, "unmapped code");
        assert_eq!(map.text(0x93FA, 1), None, "the width is part of the key");
        assert!(super::Predefined::by_name(b"90pv-RKSJ-H").is_none());

        // The second family: a Unicode CMap, where a code stands for itself.
        let unicode = super::Predefined::by_name(b"UniJIS-UCS2-H").expect("shipped");
        assert_eq!(
            unicode.text(0x65E5, 2),
            Some('日'),
            "the code is the character"
        );
        // `UniJIS-UCS2-H` maps the Japanese CIDs, not the proportional Latin set
        // `90ms-RKSJ-H` reaches at 0x20–0x7D, so ASCII is *not* covered here and the
        // table says so rather than inventing it.
        assert_eq!(unicode.text(0x0041, 2), None);
        assert_eq!(
            unicode.text(0x65E5, 1),
            None,
            "UCS-2 codes are two bytes wide"
        );
        assert!(
            unicode.text(0xE000, 2).is_none(),
            "a code the CMap does not cover is not invented"
        );
    }

    #[test]
    fn the_fixtures_cmap_parses_through_the_document() {
        let path = format!(
            "{}/tests/fixtures/tounicode.pdf",
            env!("CARGO_MANIFEST_DIR")
        );
        let document = Document::open_path(&path).expect("open fixture");
        let page = document.page(0).expect("page");
        let resources = page.resources.expect("resources");
        let fonts = document
            .resolve(resources.get(b"Font"))
            .expect("resolve fonts")
            .expect("fonts");
        let Object::Dictionary(fonts) = fonts else {
            panic!("fonts is not a dictionary");
        };
        let font = fonts.get(b"F1").expect("F1");
        let font = document
            .resolve(Some(font))
            .expect("resolve font")
            .expect("font");
        let Object::Dictionary(font) = font else {
            panic!("font is not a dictionary");
        };
        let Object::Reference(id) = font.get(b"ToUnicode").expect("/ToUnicode") else {
            panic!("/ToUnicode is not a reference");
        };
        let data = document.stream_data(*id).expect("cmap stream");
        let cmap = ToUnicode::parse(&data).expect("parse the fixture's CMap");
        assert_eq!(cmap.code_lengths(), &[2]);
        assert_eq!(cmap.text(0x0003, 2), Some("A"));
        assert_eq!(cmap.text(0x0004, 2), Some("D"));
        assert_eq!(cmap.text(0x0010, 2), Some("a"));
        assert_eq!(cmap.text(0x0011, 2), Some("b"));
        assert_eq!(cmap.text(0x0012, 2), Some("c"));
        assert_eq!(cmap.text(0x0020, 2), Some("x"));
        assert_eq!(cmap.text(0x0022, 2), Some("z"));
    }
}
