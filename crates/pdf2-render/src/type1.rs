//! Type 1 fonts (`/FontFile`, `/Subtype /Type1`): the program, `eexec`, and the
//! charstring interpreter.
//!
//! `ttf-parser` reads sfnt and CFF programs; a Type 1 program is neither. It is a
//! PostScript program whose glyph procedures are **charstrings** — RPN bytecode
//! with PostScript operators — wrapped in an encrypted `eexec` section whose key
//! is fixed by the format (`R = 55665` for the section, `4330` for the
//! charstrings when `/lenIV` is non-negative).
//!
//! What is read: `/Subrs` and `/CharStrings` from the decrypted section, both the
//! binary-token form real files use (`… N RD <bytes> ND`) and the hex form, in
//! PFA (ASCII) and PFB (binary segment) containers. What is interpreted: the
//! drawing operators — `hsbw`, `sbw`, `rmoveto`, `hmoveto`, `vmoveto`,
//! `rlineto`, `hlineto`, `vlineto`, `rrcurveto`, `closepath`, `callsubr`,
//! `callgsubr`, `return`, `endchar` and `div` — plus hints (`hstem`, `vstem`,
//! `hstemhm`, `vstemhm`, `hintmask`) which are popped and ignored, because hints
//! are a rasteriser's business and the outlines do not need them.
//!
//! What is refused by name: `seac` (accent composition, which needs the font's
//! own `StandardEncoding` and a second glyph) and `callothersubr` (the flex
//! mechanism, which is a PostScript interpreter's job). A charstring containing
//! either yields no glyph rather than a wrong one.

use std::collections::BTreeMap;

use crate::geom::Point;
use crate::Glyph;

/// The `eexec` section key (`t1disasm`, Adobe Type 1 Font Format §7.2).
const EEXEC_R: u16 = 55665;
/// The charstring key, used when `/lenIV` is not `-1`.
const CHARSTRING_R: u16 = 4330;
/// How deep a charstring may call into `/Subrs` before giving up.
const MAX_SUBR_DEPTH: usize = 10;

/// A parsed Type 1 font program.
#[derive(Debug, Clone, Default)]
pub struct Program {
    /// Glyph name → charstring, already decrypted.
    charstrings: BTreeMap<Vec<u8>, Vec<u8>>,
    /// `/Subrs`, indexed by the operand of `callsubr`.
    subrs: Vec<Vec<u8>>,
    /// Length of the random prefix `/lenIV` strips, or `None` for `-1`.
    len_iv: Option<usize>,
}

impl Program {
    /// Read a `/FontFile` stream: PFA (ASCII) or PFB (binary segments).
    pub fn parse(data: &[u8]) -> Option<Self> {
        let program = if data.starts_with(&[0x80, 0x01]) {
            pfb_to_pfa(data)?
        } else {
            data.to_vec()
        };
        let start = find(&program, b"eexec")? + b"eexec".len();
        let section = &program[start..];
        let decrypted = eexec_decrypt(section);
        let mut parsed = Self::default();
        parsed.read_section(&decrypted);
        Some(parsed)
    }

    /// The outline for a glyph name, in em units.
    pub fn glyph(&self, name: &[u8]) -> Option<Glyph> {
        let charstring = self.charstrings.get(name)?;
        let mut interpreter = Interpreter {
            program: self,
            stack: Vec::new(),
            contours: Vec::new(),
            current: Vec::new(),
            x: 0.0,
            y: 0.0,
            advance: 0.0,
        };
        interpreter.run(charstring, 0)?;
        interpreter.finish();
        Some(Glyph {
            contours: interpreter.contours,
            advance: interpreter.advance / 1000.0,
        })
    }

    /// Whether any glyph at all was read, which is what decides if the program is
    /// usable rather than merely present.
    pub fn is_usable(&self) -> bool {
        !self.charstrings.is_empty()
    }

    /// Glyph names in the program, which is what a `/Differences` encoding has to
    /// match.
    pub fn names(&self) -> impl Iterator<Item = &[u8]> {
        self.charstrings.keys().map(Vec::as_slice)
    }

    /// Read `/Subrs`, `/CharStrings` and `/lenIV` out of the decrypted section.
    fn read_section(&mut self, section: &[u8]) {
        let mut tokens = Tokens::new(section);
        while let Some(token) = tokens.next_token() {
            match token.as_slice() {
                b"/lenIV" => {
                    if let Some(value) = tokens.next_token().and_then(|token| parse_int(&token)) {
                        self.len_iv = usize::try_from(value).ok();
                    }
                }
                b"/Subrs" => {
                    let _count = tokens.next_token().and_then(|t| parse_int(&t));
                    // `array` or `readonly array` — the entries follow.
                    while let Some(token) = tokens.next_token() {
                        if token == b"dup" {
                            let Some(index) = tokens.next_token().and_then(|t| parse_int(&t))
                            else {
                                break;
                            };
                            let Some(entry) = tokens.binary_after_length() else {
                                break;
                            };
                            if let (Ok(slot), Some(data)) =
                                (usize::try_from(index), self.decrypt_charstring(entry))
                            {
                                if self.subrs.len() <= slot {
                                    self.subrs.resize(slot + 1, Vec::new());
                                }
                                self.subrs[slot] = data;
                            }
                        } else if token == b"readonly" || token == b"/CharStrings" {
                            if token == b"/CharStrings" {
                                tokens.push_back(token);
                            }
                            break;
                        }
                    }
                }
                b"/CharStrings" => {
                    let _count = tokens.next_token().and_then(|t| parse_int(&t));
                    // `dict dup begin`, then `<name> <length> RD <bytes> ND`.
                    while let Some(token) = tokens.next_token() {
                        if token == b"end" || token == b"readonly" {
                            break;
                        }
                        // The delimiters that close an entry are tokens too.
                        if matches!(
                            token.as_slice(),
                            b"dup" | b"begin" | b"dict" | b"ND" | b"NP" | b"|-" | b"-|"
                        ) {
                            continue;
                        }
                        // A glyph name is a name object: `/A`, which is one token.
                        let name = token
                            .strip_prefix(b"/")
                            .map(<[u8]>::to_vec)
                            .unwrap_or(token);
                        let Some(entry) = tokens.binary_after_length() else {
                            break;
                        };
                        if let Some(charstring) = self.decrypt_charstring(entry) {
                            self.charstrings.insert(name, charstring);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Undo `/lenIV`: a charstring is encrypted with `R = 4330` and the first
    /// `lenIV` bytes are random padding.
    fn decrypt_charstring(&self, entry: Vec<u8>) -> Option<Vec<u8>> {
        match self.len_iv {
            // `/lenIV -1` means the charstrings are not encrypted at all.
            None => Some(entry),
            Some(0) => Some(entry),
            Some(skip) => {
                let decrypted = crypt(&entry, CHARSTRING_R);
                (decrypted.len() >= skip).then(|| decrypted[skip..].to_vec())
            }
        }
    }
}

/// A glyph's paint: the current point, the contours so far and the advance.
struct Interpreter<'a> {
    program: &'a Program,
    stack: Vec<f64>,
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
    x: f64,
    y: f64,
    advance: f64,
}

impl Interpreter<'_> {
    fn run(&mut self, charstring: &[u8], depth: usize) -> Option<()> {
        if depth > MAX_SUBR_DEPTH {
            return None;
        }
        let mut index = 0;
        while index < charstring.len() {
            let byte = charstring[index];
            index += 1;
            match byte {
                // Numbers: the four encodings §6.2 describes.
                32..=246 => self.stack.push(f64::from(byte) - 139.0),
                247..=250 => {
                    let next = f64::from(*charstring.get(index)?);
                    index += 1;
                    self.stack
                        .push((f64::from(byte) - 247.0) * 256.0 + next + 108.0);
                }
                251..=254 => {
                    let next = f64::from(*charstring.get(index)?);
                    index += 1;
                    self.stack
                        .push(-(f64::from(byte) - 251.0) * 256.0 - next - 108.0);
                }
                255 => {
                    let bytes: [u8; 4] = charstring.get(index..index + 4)?.try_into().ok()?;
                    index += 4;
                    self.stack.push(f64::from(i32::from_be_bytes(bytes)));
                }
                _ => {
                    if !self.operator(byte, charstring, &mut index, depth)? {
                        break;
                    }
                }
            }
        }
        Some(())
    }

    /// Apply one operator; `false` ends the charstring (`return`/`endchar`).
    fn operator(
        &mut self,
        operator: u8,
        charstring: &[u8],
        index: &mut usize,
        depth: usize,
    ) -> Option<bool> {
        match operator {
            // Hints: popped and ignored. The outlines do not depend on them.
            1 | 3 | 18 | 23 => self.stack.clear(),
            19 | 20 => {
                // `hintmask`/`cntrmask`: a hint count, then one mask bit per hint.
                let hints = self.stack.iter().sum::<f64>().max(0.0) as usize;
                let bits = hints.div_ceil(8);
                *index += bits;
                self.stack.clear();
            }
            4 => {
                // vmoveto
                self.y += self.pop()?;
                self.move_to();
            }
            21 => {
                // rmoveto
                let dy = self.pop()?;
                let dx = self.pop()?;
                self.x += dx;
                self.y += dy;
                self.move_to();
            }
            22 => {
                // hmoveto
                self.x += self.pop()?;
                self.move_to();
            }
            5 => {
                // rlineto
                while self.stack.len() >= 2 {
                    let dy = self.pop()?;
                    let dx = self.pop()?;
                    self.line_by(dx, dy);
                }
            }
            6 | 7 => {
                // hlineto / vlineto: alternating, starting with `operator`'s axis.
                let horizontal = operator == 6;
                let mut values = core::mem::take(&mut self.stack);
                values.reverse();
                for (step, value) in values.into_iter().enumerate() {
                    if (step % 2 == 0) == horizontal {
                        self.line_by(value, 0.0);
                    } else {
                        self.line_by(0.0, value);
                    }
                }
            }
            8 => {
                // rrcurveto
                while self.stack.len() >= 6 {
                    let (c1x, c1y, c2x, c2y, dx, dy) = (
                        self.pop()?,
                        self.pop()?,
                        self.pop()?,
                        self.pop()?,
                        self.pop()?,
                        self.pop()?,
                    );
                    self.curve_by(c1x, c1y, c2x, c2y, dx, dy);
                }
            }
            9 => self.finish_contour(),
            10 | 29 => {
                // callsubr / callgsubr
                let number = self.pop()?;
                let index = usize::try_from(number as i64).ok()?;
                let subr = self.program.subrs.get(index)?;
                let subr = subr.clone();
                self.run(&subr, depth + 1)?;
            }
            11 => return Some(false), // return
            13 => {
                // hsbw: sidebearing x, sidebearing y, and the width.
                let sbx = self.pop()?;
                let _sby = self.pop()?;
                self.advance = sbx;
                self.x = sbx;
                self.move_to();
            }
            14 => return Some(false), // endchar
            12 => {
                let second = *charstring.get(*index)?;
                *index += 1;
                match second {
                    6 => return None,  // seac: refused, see the module docs
                    16 => return None, // callothersubr: refused
                    12 => {
                        // div
                        let divisor = self.pop()?;
                        let value = self.pop()?;
                        if divisor == 0.0 {
                            return None;
                        }
                        self.stack.push(value / divisor);
                    }
                    7 => {
                        // sbw
                        let sbx = self.pop()?;
                        let _sby = self.pop()?;
                        let wx = self.pop()?;
                        self.advance = wx;
                        self.x = sbx;
                        self.move_to();
                    }
                    34 | 36 | 37 => {
                        // hflex / hflex1 / flex1: six or seven points of a curve.
                        let mut values = core::mem::take(&mut self.stack);
                        values.reverse();
                        let mut remaining = values.as_slice();
                        while remaining.len() >= 6 {
                            let (c1x, c1y, c2x, c2y, dx, dy) = (
                                remaining[0],
                                remaining[1],
                                remaining[2],
                                remaining[3],
                                remaining[4],
                                remaining[5],
                            );
                            self.curve_by(c1x, c1y, c2x, c2y, dx, dy);
                            remaining = &remaining[6..];
                        }
                    }
                    35 => {
                        // flex: two curves, then the flex depth to ignore.
                        let mut values = core::mem::take(&mut self.stack);
                        values.reverse();
                        if values.len() >= 13 {
                            for pair in values[..12].chunks_exact(6) {
                                self.curve_by(pair[0], pair[1], pair[2], pair[3], pair[4], pair[5]);
                            }
                        }
                    }
                    _ => self.stack.clear(),
                }
            }
            _ => self.stack.clear(),
        }
        Some(true)
    }

    fn pop(&mut self) -> Option<f64> {
        self.stack.pop()
    }

    /// Glyph space is 1/1000 em for the `FontMatrix [0.001 …]` every Type 1 font
    /// in practice declares.
    fn point(&self) -> Point {
        Point::new(self.x / 1000.0, self.y / 1000.0)
    }

    fn move_to(&mut self) {
        self.finish_contour();
        self.current.push(self.point());
    }

    fn line_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        if self.current.is_empty() {
            self.move_to();
        } else {
            self.current.push(self.point());
        }
    }

    fn curve_by(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, dx: f64, dy: f64) {
        let start = self.point();
        let c1 = Point::new(start.x + c1x / 1000.0, start.y + c1y / 1000.0);
        let c2 = Point::new(c1.x + c2x / 1000.0, c1.y + c2y / 1000.0);
        let end = Point::new(c2.x + dx / 1000.0, c2.y + dy / 1000.0);
        if self.current.is_empty() {
            self.move_to();
        }
        for step in 1..=12 {
            let t = f64::from(step) / 12.0;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * u * start.x
                    + 3.0 * u * u * t * c1.x
                    + 3.0 * u * t * t * c2.x
                    + t * t * t * end.x,
                u * u * u * start.y
                    + 3.0 * u * u * t * c1.y
                    + 3.0 * u * t * t * c2.y
                    + t * t * t * end.y,
            ));
        }
        self.x = end.x * 1000.0;
        self.y = end.y * 1000.0;
    }

    fn finish_contour(&mut self) {
        if self.current.len() > 1 {
            self.contours.push(core::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn finish(&mut self) {
        self.finish_contour();
    }
}

/// A token reader over the decrypted section, with binary-token support.
struct Tokens<'a> {
    data: &'a [u8],
    at: usize,
    pushed: Option<Vec<u8>>,
}

impl<'a> Tokens<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            pushed: None,
        }
    }

    fn push_back(&mut self, token: Vec<u8>) {
        self.pushed = Some(token);
    }

    fn next_token(&mut self) -> Option<Vec<u8>> {
        if let Some(token) = self.pushed.take() {
            return Some(token);
        }
        while self.at < self.data.len() && self.data[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        if self.at >= self.data.len() {
            return None;
        }
        let start = self.at;
        while self.at < self.data.len() && !self.data[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        Some(self.data[start..self.at].to_vec())
    }

    /// After a length token and `RD`/`-|`, the bytes of that length follow.
    fn binary_after_length(&mut self) -> Option<Vec<u8>> {
        let token = self.next_token()?;
        let length = parse_int(&token)?;
        let marker = self.next_token()?;
        if marker != b"RD" && marker != b"-|" {
            return None;
        }
        // **Exactly one** whitespace byte separates `RD` from the data: a
        // charstring's first byte is often whitespace-valued (it is a number
        // operand), so skipping more would shift the whole glyph.
        if self.at < self.data.len() && self.data[self.at].is_ascii_whitespace() {
            self.at += 1;
        }
        let length = usize::try_from(length).ok()?;
        let bytes = self.data.get(self.at..self.at + length)?.to_vec();
        self.at += length;
        // The delimiter that follows (`ND`, `NP`, `|-`) is left in the stream: it
        // is a token like any other and the caller ignores it.
        Some(bytes)
    }
}

/// `eexec`: a fixed-key cipher, with the first four decrypted bytes discarded.
fn eexec_decrypt(section: &[u8]) -> Vec<u8> {
    // The section is hex in a PFA and binary in a PFB. Hex is recognised by the
    // first non-space bytes all being hex digits.
    let mut start = 0;
    while start < section.len() && section[start].is_ascii_whitespace() {
        start += 1;
    }
    let hex = section[start..]
        .iter()
        .take(16)
        .all(|byte| byte.is_ascii_hexdigit());
    let bytes = if hex {
        let mut out = Vec::with_capacity(section.len() / 2);
        let mut high: Option<u8> = None;
        for &byte in &section[start..] {
            if !byte.is_ascii_hexdigit() {
                break;
            }
            let value = (byte as char).to_digit(16).unwrap_or(0) as u8;
            match high.take() {
                Some(first) => out.push((first << 4) | value),
                None => high = Some(value),
            }
        }
        out
    } else {
        // Binary: up to the `cleartomark` trailer when it is ASCII-visible, or the
        // whole rest of the stream.
        match find(&section[start..], b"cleartomark") {
            Some(at) => section[start..start + at].to_vec(),
            None => section[start..].to_vec(),
        }
    };
    let decrypted = crypt(&bytes, EEXEC_R);
    if decrypted.len() <= 4 {
        return Vec::new();
    }
    decrypted[4..].to_vec()
}

/// The Type 1 cipher: a fixed-key stream cipher over the bytes.
fn crypt(data: &[u8], mut key: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &byte in data {
        let plain = byte ^ (key >> 8) as u8;
        out.push(plain);
        // The key steps on the **plaintext** byte, which is why the same routine
        // both encrypts and decrypts: on the way in the plaintext is what you
        // have, on the way out it is what you just recovered (t1utils, §7.2).
        key = (u16::from(plain).wrapping_add(key))
            .wrapping_mul(52845)
            .wrapping_add(22719);
    }
    out
}

/// Assemble a PFB container's segments into one program: a PFB is `0x80 type
/// <length:u32 little-endian> <data>` repeated, and only the ASCII and binary
/// segments carry the program.
fn pfb_to_pfa(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    while at + 6 <= data.len() {
        if data[at] != 0x80 {
            return None;
        }
        let kind = data[at + 1];
        let length = u32::from_le_bytes(data.get(at + 2..at + 6)?.try_into().ok()?) as usize;
        let body = data.get(at + 6..at + 6 + length)?;
        if kind == 1 || kind == 2 {
            out.extend_from_slice(body);
        }
        at += 6 + length;
        if kind == 3 {
            break;
        }
    }
    Some(out)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_int(token: &[u8]) -> Option<i64> {
    core::str::from_utf8(token).ok()?.parse::<i64>().ok()
}

#[cfg(test)]
mod tests {
    use super::{find, Program};

    fn fixture() -> Program {
        let data = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/type1.pdf"
        ))
        .expect("fixture");
        let start = find(&data, b"/FontFile 7 0 R").expect("font file reference");
        let _ = start;
        // The program is the object the descriptor points at; the stream body is
        // found by its `%!PS` header.
        let at = find(&data, b"%!PS-AdobeFont-1.0").expect("program header");
        let end = find(&data[at..], b"cleartomark").expect("trailer") + at;
        Program::parse(&data[at..end]).expect("parse")
    }

    #[test]
    fn the_fixture_font_reads_its_glyphs() {
        let program = fixture();
        assert!(program.is_usable());
        let names: Vec<String> = program
            .names()
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect();
        assert_eq!(names, [".notdef", "A", "B", "C"]);
        for (name, points) in [("A", 4), ("B", 5), ("C", 3)] {
            let glyph = program
                .glyph(name.as_bytes())
                .unwrap_or_else(|| panic!("{name} parses"));
            assert_eq!(glyph.contours.len(), 1, "{name} has one contour");
            assert!(
                glyph.contours[0].len() >= points,
                "{name} has at least {points} points, has {}",
                glyph.contours[0].len()
            );
        }
    }
}
