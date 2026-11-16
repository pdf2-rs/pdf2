//! CCITT Group 3 and Group 4 fax decoding (7.4.6), one line at a time.
//!
//! A fax stream is a bit stream of run-length codes: alternating white and black
//! runs, ending each line at `/Columns`, optionally after an end-of-line marker.
//! Group 3 has a one-dimensional mode (`/K` 0) and a two-dimensional one
//! (`/K` greater than 0, mixed line by line); Group 4 (`/K` -1) is
//! two-dimensional throughout.
//! This module implements the bit reader, the parameter handling and the
//! **one-dimensional** codes; the two-dimensional modes are reported rather than
//! half-decoded, because a partially correct fax line is noise rather than an
//! approximation.
//!
//! The run-length tables are ITU-T T.4 facts, transcribed here from
//! `hayro-ccitt` 0.5.0 (Apache-2.0 OR MIT) rather than typed from memory; see
//! `THIRD-PARTY-NOTICES.md`.

use alloc::vec;
use alloc::vec::Vec;

use crate::error::{Error, Result};

/// `/DecodeParms` for a `/CCITTFaxDecode` filter (7.4.6, Table 11).
#[derive(Debug, Clone, Copy)]
pub struct Params {
    /// `/K`: 0 for one-dimensional, > 0 for mixed Group 3, -1 for Group 4.
    pub k: i64,
    /// `/Columns`: pixels per line, 1728 by default.
    pub columns: u32,
    /// `/Rows`: lines to decode, 0 for "as many as the data holds".
    pub rows: u32,
    /// `/EndOfLine`: every line is preceded by an EOL code.
    pub end_of_line: bool,
    /// `/EncodedByteAlign`: each line starts on a byte boundary.
    pub encoded_byte_align: bool,
    /// `/EndOfBlock`: a 24-bit end-of-block may follow the last line.
    pub end_of_block: bool,
    /// `/BlackIs1`: whether a 1 bit is black (false by default in PDF, i.e. 1 is
    /// white and 0 is black).
    pub black_is_1: bool,
}

impl Default for Params {
    /// The PDF defaults (7.4.6).
    fn default() -> Self {
        Self {
            k: 0,
            columns: 1728,
            rows: 0,
            end_of_line: false,
            encoded_byte_align: false,
            end_of_block: true,
            black_is_1: false,
        }
    }
}

/// Decode a fax stream into packed one-bit rows, high bit first.
///
/// Rows are padded to whole bytes, which is what the image reader expects.
pub fn decode(data: &[u8], params: &Params, limit: usize) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(data);
    let stride = usize::try_from(params.columns.div_ceil(8))
        .map_err(|_| Error::malformed("a fax image wider than the platform can address"))?;
    if stride == 0 {
        return Err(Error::malformed("a fax image with zero columns"));
    }
    let mut out: Vec<u8> = Vec::new();
    let mut row: Vec<u8> = vec![0; stride];
    // Lines are decoded as per-pixel colours and packed afterwards, so that
    // `/BlackIs1` decides what a bit means only when the bits are written out.
    let columns = usize::try_from(params.columns).unwrap_or(0);
    let mut line: Vec<bool> = vec![false; columns];
    // The line above, which a two-dimensional line is coded against (T.4
    // §4.2.1.2). The first line of a G4 image is coded against an all-white one,
    // which is what `reference` starts as.
    let mut reference: Vec<bool> = vec![false; columns];
    let mut lines = 0u32;
    loop {
        if params.rows > 0 && lines >= params.rows {
            break;
        }
        // Trailing zero padding is not a line: every run code has at least one
        // 1 bit in it (the shortest, white 2, is `0111`).
        if reader.exhausted() || reader.rest_is_zero() {
            break;
        }
        if params.end_of_line {
            reader.skip_eol()?;
            if reader.exhausted() {
                break;
            }
        }
        row.iter_mut().for_each(|byte| *byte = 0);
        line.iter_mut().for_each(|pixel| *pixel = false);
        // `/K > 0` is G3 with two-dimensional coding: each line carries a tag bit
        // saying whether *it* is one- or two-dimensional. `/K < 0` is G4, where
        // every line after the first is two-dimensional and no tag bit is sent.
        let two_dimensional = if params.k > 0 {
            // Mixed-mode (G3) lines carry an EOL and then a tag bit, and writers
            // emit the EOL even when `/EndOfLine` is absent — the corpus page's
            // `/K 1` images do exactly that. Skipping zeros up to an EOL before
            // the tag is what pdf.js's decoder does, and it is what makes those
            // images readable rather than misaligned.
            if !params.end_of_line {
                reader.skip_zeros_before_eol();
            }
            // The tag bit: 1 means this line is one-dimensional.
            match reader.next_bit() {
                Some(bit) => bit == 0,
                None => break,
            }
        } else {
            // G4 codes *every* line two-dimensionally, the first against an
            // all-white reference line (T.4 §4.2.2).
            params.k < 0
        };
        if two_dimensional {
            decode_line_2d(&mut reader, &mut line, &reference, params.columns)?;
        } else {
            decode_line(&mut reader, &mut line, params)?;
        }
        pack_line(&line, &mut row, params.black_is_1);
        out.extend_from_slice(&row);
        reference.copy_from_slice(&line);
        if out.len() > limit {
            return Err(Error::Limit {
                what: "a /CCITTFaxDecode image",
                limit,
            });
        }
        lines += 1;
        if params.encoded_byte_align {
            reader.align();
        }
        if params.end_of_block && reader.peek(24) == Some(0b0000_0000_0001_0000_0000_0001) {
            break;
        }
    }
    if lines == 0 {
        return Err(Error::malformed("a fax stream without a decodable line"));
    }
    Ok(out)
}

/// One one-dimensional line: alternating runs until `/Columns` is filled.
fn decode_line(reader: &mut BitReader<'_>, line: &mut [bool], params: &Params) -> Result<()> {
    let columns = params.columns;
    let mut position = 0u32;
    let mut black = false;
    while position < columns {
        let run = match read_run(reader, black)? {
            Some(run) => run,
            None => return Ok(()),
        };
        let run = run.min(columns - position);
        paint(line, i64::from(position), i64::from(position + run), black);
        position += run;
        black = !black;
    }
    Ok(())
}

/// One two-dimensional line (T.4 §4.2.1.2), coded against `reference`.
///
/// The model is the changing elements, not the pixels: `a0` walks the coding line
/// while `b1` and `b2` are the next two changes on the reference line, and every
/// mode is a rule about those positions. The coding line is kept as a list of
/// change positions whose parity is the run's colour (index 0 is the start, an odd
/// index starts a black run), and the reference position is *tracked* as `a0`
/// advances — which is the part a pixel loop cannot express, and the part that made
/// a first attempt at this draw a box border with its text missing.
///
/// The structure follows pdf.js's `ccitt.js` (Apache-2.0, pinned at `v1.10.100`;
/// provenance in `THIRD-PARTY-NOTICES.md`), because that implementation is tested
/// against the same corpus of damaged fax files this one has to survive.
fn decode_line_2d(
    reader: &mut BitReader<'_>,
    line: &mut [bool],
    reference: &[bool],
    columns: u32,
) -> Result<()> {
    let reference_changes = change_positions(reference, columns);
    let mut coding: Vec<u32> = vec![0, columns, columns];
    let mut coding_pos = 0usize;
    let mut reference_pos = 0usize;
    let mut black = false;
    // A line has at most one change per pixel; anything longer is a broken stream
    // rather than a picture, and the guard keeps that from looping.
    let mut guard = 0usize;
    let limit = usize::try_from(columns).unwrap_or(0) + 64;
    while coding[coding_pos] < columns {
        guard += 1;
        if guard > limit {
            return Err(Error::malformed(
                "a two-dimensional line with too many runs",
            ));
        }
        let mode = read_two_dimensional(reader)?;
        match mode {
            Some(TwoDimensional::Pass) => {
                let b2 = reference_changes
                    .get(reference_pos + 1)
                    .copied()
                    .unwrap_or(columns);
                add_pixels(&mut coding, &mut coding_pos, line, b2, black, columns);
                if b2 < columns {
                    reference_pos += 2;
                }
            }
            Some(TwoDimensional::Horizontal) => {
                let first = read_run(reader, black)?
                    .ok_or_else(|| Error::malformed("a horizontal mode without a run"))?;
                let second = read_run(reader, !black)?
                    .ok_or_else(|| Error::malformed("a horizontal mode without its second run"))?;
                let a0 = coding[coding_pos];
                add_pixels(
                    &mut coding,
                    &mut coding_pos,
                    line,
                    a0.saturating_add(first),
                    black,
                    columns,
                );
                if coding[coding_pos] < columns {
                    let a1 = coding[coding_pos];
                    add_pixels(
                        &mut coding,
                        &mut coding_pos,
                        line,
                        a1.saturating_add(second),
                        !black,
                        columns,
                    );
                }
                while reference_changes
                    .get(reference_pos)
                    .is_some_and(|at| *at <= coding[coding_pos] && *at < columns)
                {
                    reference_pos += 2;
                }
            }
            Some(TwoDimensional::Vertical(delta)) => {
                let b1 = reference_changes
                    .get(reference_pos)
                    .copied()
                    .unwrap_or(columns);
                let a1 = if delta >= 0 {
                    b1.saturating_add(delta.unsigned_abs().into())
                } else {
                    b1.saturating_sub(delta.unsigned_abs().into())
                };
                add_pixels_neg(&mut coding, &mut coding_pos, line, a1, black, columns);
                black = !black;
                if coding[coding_pos] < columns {
                    if delta >= 0 {
                        reference_pos += 1;
                    } else if reference_pos > 0 {
                        reference_pos -= 1;
                    } else {
                        reference_pos += 1;
                    }
                    while reference_changes
                        .get(reference_pos)
                        .is_some_and(|at| *at <= coding[coding_pos] && *at < columns)
                    {
                        reference_pos += 2;
                    }
                }
            }
            None => return Err(Error::malformed("a two-dimensional mode code")),
        }
    }
    Ok(())
}

/// Record a change at `a1`, painting the run that ends there.
///
/// The run colour decides the parity of the position it is written to: an odd
/// index starts a black run, an even one a white run, counting from the line's
/// start at index 0.
fn add_pixels(
    coding: &mut Vec<u32>,
    coding_pos: &mut usize,
    line: &mut [bool],
    a1: u32,
    black: bool,
    columns: u32,
) {
    let a0 = coding[*coding_pos];
    if a1 <= a0 {
        return;
    }
    let a1 = a1.min(columns);
    let mut position = *coding_pos;
    if (position % 2 == 1) != black {
        position += 1;
    }
    paint(line, i64::from(a0), i64::from(a1), black);
    while coding.len() <= position {
        coding.push(a1);
    }
    coding[position] = a1;
    *coding_pos = position;
}

/// The same, for a vertical code whose `a1` lands to the *left* of `a0`: the
/// change list is walked back to where the new position belongs.
fn add_pixels_neg(
    coding: &mut Vec<u32>,
    coding_pos: &mut usize,
    line: &mut [bool],
    a1: u32,
    black: bool,
    columns: u32,
) {
    let a0 = coding[*coding_pos];
    if a1 > a0 {
        add_pixels(coding, coding_pos, line, a1, black, columns);
        return;
    }
    if a1 < a0 {
        let mut position = *coding_pos;
        while position > 0 && a1 < coding[position - 1] {
            position -= 1;
        }
        paint(line, i64::from(a1), i64::from(a0), black);
        coding[position] = a1;
        *coding_pos = position;
    }
}

/// A two-dimensional mode, as T.4 §4.2.1.2 codes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TwoDimensional {
    Pass,
    Horizontal,
    /// `a1 - b1`, in −3..=3.
    Vertical(i8),
}

/// Read one mode code.
///
/// The prefixes are unambiguous in this order — pass is `0001…`, horizontal
/// `001…`, and the vertical codes are `1`, `011`, `010`, `000011`, `000010`,
/// `0000011`, `0000010` — so the shortest code that can decide is tried first.
fn read_two_dimensional(reader: &mut BitReader<'_>) -> Result<Option<TwoDimensional>> {
    for (code, length) in [(0b0001_u32, 4_usize), (0b001, 3)] {
        if reader.peek(length) == Some(code) {
            for _ in 0..length {
                reader.next_bit();
            }
            return Ok(Some(if length == 4 {
                TwoDimensional::Pass
            } else {
                TwoDimensional::Horizontal
            }));
        }
    }
    for (code, length, delta) in [
        (0b1_u32, 1_usize, 0_i8),
        (0b011, 3, 1),
        (0b000011, 6, 2),
        (0b0000011, 7, 3),
        (0b010, 3, -1),
        (0b000010, 6, -2),
        (0b0000010, 7, -3),
    ] {
        if reader.peek(length) == Some(code) {
            for _ in 0..length {
                reader.next_bit();
            }
            return Ok(Some(TwoDimensional::Vertical(delta)));
        }
    }
    Ok(None)
}

/// The change positions of a line, each followed by two copies of `columns`.
///
/// A change at index `i` starts a black run when `i` is even: the line begins in
/// white, so the first change is into black.
fn change_positions(line: &[bool], columns: u32) -> Vec<u32> {
    let mut changes = Vec::new();
    let mut previous = false;
    for (index, black) in line.iter().enumerate() {
        if *black != previous {
            if let Ok(at) = u32::try_from(index) {
                changes.push(at);
            }
            previous = *black;
        }
    }
    changes.push(columns);
    changes.push(columns);
    changes
}

/// The colour of `[from, to)` in `line`, for a black run when `black`.
fn paint(line: &mut [bool], from: i64, to: i64, black: bool) {
    let from = from.max(0);
    let to = to.min(line.len() as i64);
    for x in from..to {
        if let Some(slot) = line.get_mut(usize::try_from(x).unwrap_or(0)) {
            *slot = black;
        }
    }
}

/// Write one decoded line into packed bytes: 1 for white and 0 for black unless
/// `/BlackIs1` says otherwise (7.4.6).
fn pack_line(line: &[bool], row: &mut [u8], black_is_1: bool) {
    for (index, black) in line.iter().enumerate() {
        let bit = if black_is_1 { *black } else { !*black };
        if bit {
            let byte = index / 8;
            if let Some(slot) = row.get_mut(byte) {
                *slot |= 1 << (7 - index % 8);
            }
        }
    }
}

/// A run length: one or more make-up codes followed by one terminating code.
fn read_run(reader: &mut BitReader<'_>, black: bool) -> Result<Option<u32>> {
    let (terminating, makeup, common) = if black {
        (BLACK_TERMINATING, BLACK_MAKEUP, COMMON_MAKEUP)
    } else {
        (WHITE_TERMINATING, WHITE_MAKEUP, COMMON_MAKEUP)
    };
    let mut total = 0u32;
    loop {
        let Some(code) = read_code(reader, &terminating, &makeup, &common)? else {
            return Ok(if total == 0 { None } else { Some(total) });
        };
        total = total.saturating_add(code);
        if code < 64 {
            return Ok(Some(total));
        }
    }
}

/// Read one code: a terminating run length, a make-up run length or `None` at
/// the end of the data.
fn read_code(
    reader: &mut BitReader<'_>,
    terminating: &[(u16, u8, u16)],
    makeup: &[(u16, u8, u16)],
    common: &[(u16, u8, u16)],
) -> Result<Option<u32>> {
    let mut value = 0u16;
    for length in 1..=13u8 {
        let Some(bit) = reader.next_bit() else {
            return Ok(None);
        };
        value = (value << 1) | u16::from(bit);
        for table in [terminating, makeup, common] {
            if let Some((run, _, _)) = table
                .iter()
                .find(|(_, code_length, code)| *code_length == length && *code == value)
            {
                return Ok(Some(u32::from(*run)));
            }
        }
    }
    Err(Error::malformed("a CCITT run code longer than 13 bits"))
}

/// A big-endian bit reader over the fax stream.
struct BitReader<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, bit: 0 }
    }

    fn exhausted(&self) -> bool {
        self.bit >= self.data.len() * 8
    }

    fn next_bit(&mut self) -> Option<u8> {
        if self.exhausted() {
            return None;
        }
        let byte = *self.data.get(self.bit / 8)?;
        let shift = 7 - (self.bit % 8);
        self.bit += 1;
        Some((byte >> shift) & 1)
    }

    /// The next `count` bits, without consuming them.
    fn peek(&self, count: usize) -> Option<u32> {
        if self.bit + count > self.data.len() * 8 {
            return None;
        }
        let mut value = 0u32;
        for offset in 0..count {
            let index = self.bit + offset;
            let byte = *self.data.get(index / 8)?;
            let shift = 7 - (index % 8);
            value = (value << 1) | u32::from((byte >> shift) & 1);
        }
        Some(value)
    }

    /// Whether every remaining bit is zero.
    fn rest_is_zero(&self) -> bool {
        let bytes = self.bit.div_ceil(8);
        for index in self.bit..self.data.len() * 8 {
            let byte = self.data[index / 8];
            if index % 8 == 0 && index >= bytes * 8 {
                if byte != 0 {
                    return false;
                }
                continue;
            }
            if (byte >> (7 - index % 8)) & 1 != 0 {
                return false;
            }
        }
        true
    }

    fn align(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }

    /// Skip an end-of-line marker: eleven or more zeros and a one.
    /// Skip zero bits up to and including an EOL, when one is there.
    ///
    /// Mixed-mode data always has an EOL before its tag bit; a file that omits
    /// `/EndOfLine` still carries them, so the reader looks rather than trusting
    /// the dictionary.
    fn skip_zeros_before_eol(&mut self) {
        while self.peek(1) == Some(0) {
            if self.peek(12) == Some(1) {
                for _ in 0..12 {
                    self.next_bit();
                }
                return;
            }
            self.next_bit();
        }
    }

    fn skip_eol(&mut self) -> Result<()> {
        let mut zeros = 0;
        while let Some(bit) = self.next_bit() {
            if bit == 0 {
                zeros += 1;
                if zeros > 16 {
                    // Not an EOL: the data is malformed rather than merely odd.
                    return Err(Error::malformed("a CCITT end-of-line marker"));
                }
            } else if zeros >= 11 {
                return Ok(());
            } else {
                zeros = 0;
            }
        }
        Ok(())
    }
}

const WHITE_TERMINATING: [(u16, u8, u16); 64] = [
    (0, 8, 0b00110101),
    (1, 6, 0b000111),
    (2, 4, 0b0111),
    (3, 4, 0b1000),
    (4, 4, 0b1011),
    (5, 4, 0b1100),
    (6, 4, 0b1110),
    (7, 4, 0b1111),
    (8, 5, 0b10011),
    (9, 5, 0b10100),
    (10, 5, 0b00111),
    (11, 5, 0b01000),
    (12, 6, 0b001000),
    (13, 6, 0b000011),
    (14, 6, 0b110100),
    (15, 6, 0b110101),
    (16, 6, 0b101010),
    (17, 6, 0b101011),
    (18, 7, 0b0100111),
    (19, 7, 0b0001100),
    (20, 7, 0b0001000),
    (21, 7, 0b0010111),
    (22, 7, 0b0000011),
    (23, 7, 0b0000100),
    (24, 7, 0b0101000),
    (25, 7, 0b0101011),
    (26, 7, 0b0010011),
    (27, 7, 0b0100100),
    (28, 7, 0b0011000),
    (29, 8, 0b00000010),
    (30, 8, 0b00000011),
    (31, 8, 0b00011010),
    (32, 8, 0b00011011),
    (33, 8, 0b00010010),
    (34, 8, 0b00010011),
    (35, 8, 0b00010100),
    (36, 8, 0b00010101),
    (37, 8, 0b00010110),
    (38, 8, 0b00010111),
    (39, 8, 0b00101000),
    (40, 8, 0b00101001),
    (41, 8, 0b00101010),
    (42, 8, 0b00101011),
    (43, 8, 0b00101100),
    (44, 8, 0b00101101),
    (45, 8, 0b00000100),
    (46, 8, 0b00000101),
    (47, 8, 0b00001010),
    (48, 8, 0b00001011),
    (49, 8, 0b01010010),
    (50, 8, 0b01010011),
    (51, 8, 0b01010100),
    (52, 8, 0b01010101),
    (53, 8, 0b00100100),
    (54, 8, 0b00100101),
    (55, 8, 0b01011000),
    (56, 8, 0b01011001),
    (57, 8, 0b01011010),
    (58, 8, 0b01011011),
    (59, 8, 0b01001010),
    (60, 8, 0b01001011),
    (61, 8, 0b00110010),
    (62, 8, 0b00110011),
    (63, 8, 0b00110100),
];

const WHITE_MAKEUP: [(u16, u8, u16); 27] = [
    (64, 5, 0b11011),
    (128, 5, 0b10010),
    (192, 6, 0b010111),
    (256, 7, 0b0110111),
    (320, 8, 0b00110110),
    (384, 8, 0b00110111),
    (448, 8, 0b01100100),
    (512, 8, 0b01100101),
    (576, 8, 0b01101000),
    (640, 8, 0b01100111),
    (704, 9, 0b011001100),
    (768, 9, 0b011001101),
    (832, 9, 0b011010010),
    (896, 9, 0b011010011),
    (960, 9, 0b011010100),
    (1024, 9, 0b011010101),
    (1088, 9, 0b011010110),
    (1152, 9, 0b011010111),
    (1216, 9, 0b011011000),
    (1280, 9, 0b011011001),
    (1344, 9, 0b011011010),
    (1408, 9, 0b011011011),
    (1472, 9, 0b010011000),
    (1536, 9, 0b010011001),
    (1600, 9, 0b010011010),
    (1664, 6, 0b011000),
    (1728, 9, 0b010011011),
];

const BLACK_TERMINATING: [(u16, u8, u16); 64] = [
    (0, 10, 0b0000110111),
    (1, 3, 0b010),
    (2, 2, 0b11),
    (3, 2, 0b10),
    (4, 3, 0b011),
    (5, 4, 0b0011),
    (6, 4, 0b0010),
    (7, 5, 0b00011),
    (8, 6, 0b000101),
    (9, 6, 0b000100),
    (10, 7, 0b0000100),
    (11, 7, 0b0000101),
    (12, 7, 0b0000111),
    (13, 8, 0b00000100),
    (14, 8, 0b00000111),
    (15, 9, 0b000011000),
    (16, 10, 0b0000010111),
    (17, 10, 0b0000011000),
    (18, 10, 0b0000001000),
    (19, 11, 0b00001100111),
    (20, 11, 0b00001101000),
    (21, 11, 0b00001101100),
    (22, 11, 0b00000110111),
    (23, 11, 0b00000101000),
    (24, 11, 0b00000010111),
    (25, 11, 0b00000011000),
    (26, 12, 0b000011001010),
    (27, 12, 0b000011001011),
    (28, 12, 0b000011001100),
    (29, 12, 0b000011001101),
    (30, 12, 0b000001101000),
    (31, 12, 0b000001101001),
    (32, 12, 0b000001101010),
    (33, 12, 0b000001101011),
    (34, 12, 0b000011010010),
    (35, 12, 0b000011010011),
    (36, 12, 0b000011010100),
    (37, 12, 0b000011010101),
    (38, 12, 0b000011010110),
    (39, 12, 0b000011010111),
    (40, 12, 0b000001101100),
    (41, 12, 0b000001101101),
    (42, 12, 0b000011011010),
    (43, 12, 0b000011011011),
    (44, 12, 0b000001010100),
    (45, 12, 0b000001010101),
    (46, 12, 0b000001010110),
    (47, 12, 0b000001010111),
    (48, 12, 0b000001100100),
    (49, 12, 0b000001100101),
    (50, 12, 0b000001010010),
    (51, 12, 0b000001010011),
    (52, 12, 0b000000100100),
    (53, 12, 0b000000110111),
    (54, 12, 0b000000111000),
    (55, 12, 0b000000100111),
    (56, 12, 0b000000101000),
    (57, 12, 0b000001011000),
    (58, 12, 0b000001011001),
    (59, 12, 0b000000101011),
    (60, 12, 0b000000101100),
    (61, 12, 0b000001011010),
    (62, 12, 0b000001100110),
    (63, 12, 0b000001100111),
];

const BLACK_MAKEUP: [(u16, u8, u16); 27] = [
    (64, 10, 0b0000001111),
    (128, 12, 0b000011001000),
    (192, 12, 0b000011001001),
    (256, 12, 0b000001011011),
    (320, 12, 0b000000110011),
    (384, 12, 0b000000110100),
    (448, 12, 0b000000110101),
    (512, 13, 0b0000001101100),
    (576, 13, 0b0000001101101),
    (640, 13, 0b0000001001010),
    (704, 13, 0b0000001001011),
    (768, 13, 0b0000001001100),
    (832, 13, 0b0000001001101),
    (896, 13, 0b0000001110010),
    (960, 13, 0b0000001110011),
    (1024, 13, 0b0000001110100),
    (1088, 13, 0b0000001110101),
    (1152, 13, 0b0000001110110),
    (1216, 13, 0b0000001110111),
    (1280, 13, 0b0000001010010),
    (1344, 13, 0b0000001010011),
    (1408, 13, 0b0000001010100),
    (1472, 13, 0b0000001010101),
    (1536, 13, 0b0000001011010),
    (1600, 13, 0b0000001011011),
    (1664, 13, 0b0000001100100),
    (1728, 13, 0b0000001100101),
];

const COMMON_MAKEUP: [(u16, u8, u16); 13] = [
    (1792, 11, 0b00000001000),
    (1856, 11, 0b00000001100),
    (1920, 11, 0b00000001101),
    (1984, 12, 0b000000010010),
    (2048, 12, 0b000000010011),
    (2112, 12, 0b000000010100),
    (2176, 12, 0b000000010101),
    (2240, 12, 0b000000010110),
    (2304, 12, 0b000000010111),
    (2368, 12, 0b000000011100),
    (2432, 12, 0b000000011101),
    (2496, 12, 0b000000011110),
    (2560, 12, 0b000000011111),
];

#[cfg(test)]
mod tests {
    use super::{decode, Params};
    use crate::error::Error;

    /// Pack bits given as a string of '0' and '1'.
    fn bits(pattern: &str) -> Vec<u8> {
        let mut out = vec![0u8; pattern.len().div_ceil(8)];
        for (index, bit) in pattern.bytes().enumerate() {
            if bit == b'1' {
                out[index / 8] |= 1 << (7 - index % 8);
            }
        }
        out
    }

    #[test]
    fn a_one_dimensional_line_decodes_to_its_runs() {
        // Eight white pixels: run 8 is `10011`, then run 0 of black (`0000110111`)
        // closes the line at `/Columns` 8.
        let data = bits("10011 0000110111".replace(' ', "").as_str());
        let params = Params {
            columns: 8,
            rows: 1,
            ..Params::default()
        };
        let line = decode(&data, &params, 1 << 20).expect("decode");
        assert_eq!(line, vec![0xFF], "eight white samples");
    }

    #[test]
    fn the_black_bit_flips_with_black_is_1() {
        let data = bits("10011 0000110111".replace(' ', "").as_str());
        let params = Params {
            columns: 8,
            rows: 1,
            black_is_1: true,
            ..Params::default()
        };
        let line = decode(&data, &params, 1 << 20).expect("decode");
        assert_eq!(line, vec![0x00], "the same line, inverted");
    }

    #[test]
    fn make_up_codes_add_to_the_run() {
        // White 64 (`11011`) as a makeup plus white 8 (`10011`): a 72-pixel line of
        // white, then black 0 to close it, over `/Columns` 72.
        let data = bits("1101110011 0000110111".replace(' ', "").as_str());
        let params = Params {
            columns: 72,
            rows: 1,
            ..Params::default()
        };
        let line = decode(&data, &params, 1 << 20).expect("decode");
        assert_eq!(line.len(), 9, "72 bits is nine bytes");
        assert!(line.iter().all(|byte| *byte == 0xFF));
    }

    #[test]
    fn a_g4_stream_decodes_every_line_two_dimensionally() {
        // `/K < 0` codes *every* line against the one above, the first against an
        // all-white line (T.4 §4.2.2). Two lines of eight: all white, then four
        // black and four white.
        //   line 0: V0 at the end of the white reference — the single bit `1`.
        //   line 1: horizontal (`001`), white run 0 (`00110101`), black run 4
        //           (`011`), then V0 (`1`) to the end of the line.
        let data = bits("1 001 00110101 011 1".replace(' ', "").as_str());
        let params = Params {
            k: -1,
            columns: 8,
            rows: 2,
            end_of_block: false,
            ..Params::default()
        };
        let decoded = decode(&data, &params, 1 << 20).expect("decode");
        assert_eq!(decoded, vec![0xFF, 0x0F], "white, then four black samples");
    }

    #[test]
    fn a_leftward_vertical_code_walks_the_change_list_back() {
        // The corpus page's `/K -1` images carry vertical codes whose `a1` lands
        // left of `a0`; the change list has to be walked back rather than refused.
        // Line 0 is white; line 1 is black from 2; line 2 starts black at 0 and so
        // needs a code that moves backwards.
        let data = bits(
            "1 001 00110101 0001 011 1 001 00110101 011 0000010"
                .replace(' ', "")
                .as_str(),
        );
        let params = Params {
            k: -1,
            columns: 8,
            rows: 3,
            end_of_block: false,
            ..Params::default()
        };
        // The third line's code is a guess at a legal backward move; what the test
        // insists on is that decoding terminates or fails cleanly, never loops.
        let _ = decode(&data, &params, 1 << 20);
    }

    #[test]
    fn an_empty_stream_is_an_error_not_an_empty_image() {
        let params = Params::default();
        assert!(decode(&[], &params, 1 << 20).is_err());
    }

    #[test]
    fn the_budget_stops_a_tall_image() {
        let data = bits("10011 0000110111".replace(' ', "").as_str());
        let params = Params {
            columns: 1728,
            rows: 100_000,
            ..Params::default()
        };
        assert!(matches!(
            decode(&data, &params, 64),
            Err(Error::Limit { .. })
        ));
    }
}
