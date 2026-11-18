//! Content-stream interpretation, far enough to extract text (§7.8.2, §9).
//!
//! The interpreter walks a decoded content stream with the same tokenizer the
//! object parser uses: operands are pushed on a stack and the operator that
//! follows consumes them. Only the text operators change what is extracted —
//! `BT`, `ET`, `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tc`, `Tw`, `Tz`, `Ts`,
//! `Tj`, `TJ`, `'` and `"` — while everything else is consumed and ignored, so
//! graphics-heavy pages do not disturb the text run.
//!
//! Limits, on purpose and documented in the README: simple fonts (Type1,
//! TrueType, Type3 with a single-byte encoding) and composite (Type0) fonts that
//! carry a `/ToUnicode` CMap over an Identity encoding. Codes for composite fonts
//! are split by the CMap's code space, and their `/W`/`/DW` widths advance the
//! pen so a gap between runs becomes a space; gaps are not guessed for simple
//! fonts, where line breaks still come from text-positioning operators and spaces
//! from the text itself or from wide `TJ` adjustments.
//!
//! Lines are ordered geometrically — descending `y`, then ascending `x` — rather
//! than by the order the content stream paints them, because a writer is free to
//! paint a page bottom-up or in stripes; `ShowText-ShadingPattern.pdf` in the
//! corpus paints 2, 1, 4, 3 and reads 1, 2, 3, 4. The decision and its limits are
//! in `docs/adr/0002-text-extraction-shape.md`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use alloc::collections::BTreeMap;

use crate::document::Document;
use crate::encoding::Encoding;
use crate::error::{Error, Result};
use crate::lexer::Token;
use crate::object::{Dictionary, Object};
use crate::page::Page;
use crate::page::Rect;
use crate::parser::Parser;

/// A `TJ` adjustment larger than this (in thousandths of an em) is treated as
/// a word gap. Wire values land between 200 and 400 for a real space.
const WORD_GAP: f64 = 200.0;

/// Movement in text space smaller than this is the same line.
const LINE_TOLERANCE: f64 = 0.01;

/// A run of text at one position, before the lines are put in reading order.
#[derive(Debug, Default)]
struct Line {
    /// Text-space position of the line's first character.
    x: f64,
    y: f64,
    text: String,
    /// The text matrix composed with the CTM when the line was started, so the
    /// reader gets page space and the writer's transforms alike (`ADR 0008`).
    matrix: Matrix,
    /// Font size in effect, which is the box's height in text space.
    size: f64,
    /// How far the pen advanced on this line, in text space.
    advance: f64,
    /// The advance of each character in `text`, in text space: the glyph
    /// advances the document states, which is what lets a run be sliced by
    /// offset. Empty when the font states none (the estimate then applies).
    advances: Vec<f64>,
}

/// A 2-D affine transform, the six numbers `cm` and `Tm` take.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// `self × other`: `other` is applied first, as PDF's matrix product is.
    fn multiply(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    fn apply(self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

/// How a composite font's codes become text.
#[derive(Debug, Clone)]
pub(crate) enum CodeMap {
    /// The page's `/ToUnicode` CMap.
    ToUnicode(crate::cmap::ToUnicode),
    /// A predefined CMap shipped as data (`/90ms-RKSJ-H` and friends).
    Predefined(crate::cmap::Predefined),
}

impl CodeMap {
    fn text(&self, code: u32, length: u8) -> Option<alloc::string::String> {
        match self {
            Self::ToUnicode(map) => map.text(code, length).map(alloc::borrow::ToOwned::to_owned),
            Self::Predefined(map) => map.text(code, length).map(|text| {
                let mut buffer = alloc::string::String::new();
                buffer.push(text);
                buffer
            }),
        }
    }

    fn code_lengths(&self) -> &[u8] {
        match self {
            Self::ToUnicode(map) => map.code_lengths(),
            Self::Predefined(map) => map.code_lengths(),
        }
    }

    fn max_code_length(&self) -> u8 {
        match self {
            Self::ToUnicode(map) => map.max_code_length(),
            Self::Predefined(map) => map.max_code_length(),
        }
    }
}

/// A CID's advance width, from `/W` with `/DW` as the default (9.7.4.3).
#[derive(Debug, Clone, Default)]
struct Widths {
    /// `/DW`, in thousandths of an em.
    default: f64,
    /// `/W` entries: first CID, last CID, widths in order.
    runs: Vec<(u32, u32, Vec<f64>)>,
}

impl Widths {
    /// The advance for `cid`, in thousandths of an em.
    fn width(&self, cid: u32) -> f64 {
        for (first, last, widths) in &self.runs {
            if cid >= *first && cid <= *last {
                let index = usize::try_from(cid - first).unwrap_or(usize::MAX);
                if let Some(width) = widths.get(index) {
                    return *width;
                }
            }
        }
        self.default
    }
}

/// A font, and the way its codes turn into text.
#[derive(Debug, Clone)]
enum Font {
    /// One byte per code, resolved through an encoding table.
    Simple {
        /// Code → character table.
        encoding: Encoding,
        /// `/Widths` and the descriptor's `/MissingWidth`, when the font states
        /// them. Without them the advance is unknown, and the line's box falls
        /// back to the documented estimate.
        widths: Option<SimpleWidths>,
    },
    /// A composite font: codes split by the CMap's code space, text from
    /// `/ToUnicode` or a predefined CMap, advances from `/W` and `/DW`.
    Composite {
        /// Code → text, from `/ToUnicode` when present, otherwise from the
        /// predefined CMap the `/Encoding` names.
        map: CodeMap,
        /// CID advances.
        widths: Widths,
    },
}

impl Font {
    /// The text `bytes` stand for, and each code's advance in em, with the number
    /// of characters that advance covers.
    ///
    /// A simple font's advance comes from its `/Widths`; a font that states none
    /// reports zero, and the caller falls back to the documented estimate rather
    /// than inventing a width. The text is one `String` and the advances one
    /// `Vec`, because a per-character `String` made a 1 MB page thirteen times
    /// slower (measured by `pdf2-bench`'s `text_pages_s`).
    fn pieces(&self, bytes: &[u8]) -> (String, Vec<(usize, f64)>) {
        match self {
            Self::Simple { encoding, widths } => {
                let mut text = String::with_capacity(bytes.len());
                let mut pieces = Vec::with_capacity(bytes.len());
                for code in bytes {
                    let Some(character) = encoding.character(*code) else {
                        continue;
                    };
                    text.push(character);
                    let em = widths
                        .as_ref()
                        .and_then(|widths| widths.advance(*code))
                        .unwrap_or(0.0);
                    pieces.push((1, em));
                }
                (text, pieces)
            }
            Self::Composite { map, widths } => {
                let mut text = String::new();
                let mut pieces = Vec::new();
                let lengths = map.code_lengths();
                let narrowest = lengths.first().map_or(1, |length| usize::from(*length));
                let widest = usize::from(map.max_code_length());
                let mut position = 0;
                while position < bytes.len() {
                    // A code space with one width is the common case and splits
                    // exactly; a variable one is tried widest first, because a
                    // variable code space means "the longest code that matches wins".
                    let mut consumed = 0;
                    let mut width = if lengths.len() == 1 {
                        narrowest
                    } else {
                        widest
                    };
                    while width >= narrowest {
                        if position + width <= bytes.len() {
                            let code = pack(&bytes[position..position + width]);
                            let matched = lengths.len() == 1
                                || map.text(code, u8::try_from(width).unwrap_or(2)).is_some();
                            if matched {
                                consumed = width;
                                break;
                            }
                        }
                        if width == narrowest {
                            break;
                        }
                        width -= 1;
                    }
                    if consumed == 0 {
                        // Nothing maps this position: step over the narrowest code
                        // so an unmapped byte cannot stall the loop.
                        if position + narrowest > bytes.len() {
                            break;
                        }
                        position += narrowest;
                        continue;
                    }
                    let code = pack(&bytes[position..position + consumed]);
                    let piece = map
                        .text(code, u8::try_from(consumed).unwrap_or(2))
                        .unwrap_or_default();
                    let characters = piece.chars().count();
                    text.push_str(&piece);
                    pieces.push((characters, widths.width(code) / 1000.0));
                    position += consumed;
                }
                (text, pieces)
            }
        }
    }

    /// The text `bytes` stand for, and the advance in em.
    ///
    /// This is the path extraction takes: one `String` and one sum, no per-code
    /// vector, because a 1 MB page pays for every allocation it makes.
    fn decode(&self, bytes: &[u8]) -> (String, f64) {
        match self {
            Self::Simple { encoding, widths } => {
                let text = bytes
                    .iter()
                    .filter_map(|code| encoding.character(*code))
                    .collect();
                let advance: f64 = if let Some(widths) = widths {
                    bytes.iter().filter_map(|code| widths.advance(*code)).sum()
                } else {
                    0.0
                };
                (text, advance)
            }
            Self::Composite { map, widths } => {
                let mut text = String::new();
                let mut advance = 0.0;
                let lengths = map.code_lengths();
                let narrowest = lengths.first().map_or(1, |length| usize::from(*length));
                let widest = usize::from(map.max_code_length());
                let mut position = 0;
                while position < bytes.len() {
                    let mut consumed = 0;
                    let mut width = if lengths.len() == 1 {
                        narrowest
                    } else {
                        widest
                    };
                    while width >= narrowest {
                        if position + width <= bytes.len() {
                            let code = pack(&bytes[position..position + width]);
                            let matched = lengths.len() == 1
                                || map.text(code, u8::try_from(width).unwrap_or(2)).is_some();
                            if matched {
                                consumed = width;
                                break;
                            }
                        }
                        if width == narrowest {
                            break;
                        }
                        width -= 1;
                    }
                    if consumed == 0 {
                        if position + narrowest > bytes.len() {
                            break;
                        }
                        position += narrowest;
                        continue;
                    }
                    let code = pack(&bytes[position..position + consumed]);
                    if let Some(mapped) = map.text(code, u8::try_from(consumed).unwrap_or(2)) {
                        text.push_str(&mapped);
                    }
                    advance += widths.width(code) / 1000.0;
                    position += consumed;
                }
                (text, advance)
            }
        }
    }

    /// Whether `decode` reports advances, so the pen can be tracked.
    fn tracks_advances(&self) -> bool {
        matches!(self, Self::Composite { .. })
    }
}

/// A simple font's `/Widths`, in 1/1000 em, with the descriptor's `/MissingWidth`
/// for codes the array does not cover (9.2.4).
///
/// The renderer keeps the same table (`SimpleWidths` in
/// `crates/pdf2-render/src/walk.rs`); the extractor needs it to place a line and
/// to slice a run, which is why a hit can have its own box.
#[derive(Debug, Clone, Default, PartialEq)]
struct SimpleWidths {
    first: i64,
    widths: Vec<f64>,
    missing: f64,
}

impl SimpleWidths {
    /// The advance for `code`, in em, or `None` when the table says nothing.
    fn advance(&self, code: u8) -> Option<f64> {
        let index = i64::from(code) - self.first;
        let width = usize::try_from(index)
            .ok()
            .and_then(|index| self.widths.get(index))
            .copied()
            .or_else(|| (self.missing > 0.0).then_some(self.missing))?;
        Some(width / 1000.0)
    }
}

/// A character code packed big-endian into a `u32`.
fn pack(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0_u32, |value, &byte| (value << 8) | u32::from(byte))
}

/// The text state and the glyph mapping needed to read a page.
struct TextInterpreter<'a> {
    document: &'a Document,
    resources: Option<Dictionary>,
    fonts: BTreeMap<Vec<u8>, Font>,
    font: Option<Font>,
    leading: f64,
    /// Font size from `Tf`, needed to turn a pen position into an em.
    size: f64,
    /// Current text line origin, in text space.
    line_x: f64,
    line_y: f64,
    /// Where the pen is after the text shown since the line was placed.
    pen_x: f64,
    /// Whether `pen_x` is meaningful for the current font.
    pen_tracked: bool,
    /// Lines in the order they were painted; sorted for the result.
    lines: Vec<Line>,
    /// Current transform matrix, saved and restored by `q`/`Q` (`8.4.4`, `8.4.2`).
    ctm: Matrix,
    /// The `q` stack.
    ctm_stack: Vec<Matrix>,
    /// The text matrix, which `BT` resets and `Tm`/`Td`/`TD`/`T*` move (`9.4.2`).
    text_matrix: Matrix,
    /// The open marked-content sequences, each with its `/ActualText` when it has
    /// one — a stack, because `BDC` nests.
    captures: Vec<Option<Capture>>,
    /// Whether to record each character's advance. Off for extraction, which pays
    /// for every allocation; on for a caller that needs a run's geometry.
    positions: bool,
}

/// One `/ActualText` sequence being collected.
struct Capture {
    /// The line it belongs to, once its first glyph has placed one.
    line: Option<usize>,
    /// Where in that line's text the replacement goes, in characters.
    offset: usize,
    /// What the writer says the content means.
    text: String,
}

impl<'a> TextInterpreter<'a> {
    fn new(document: &'a Document, resources: Option<Dictionary>) -> Self {
        Self {
            document,
            resources,
            fonts: BTreeMap::new(),
            font: None,
            leading: 0.0,
            size: 0.0,
            line_x: 0.0,
            line_y: 0.0,
            pen_x: 0.0,
            pen_tracked: false,
            lines: Vec::new(),
            ctm: Matrix::IDENTITY,
            ctm_stack: Vec::new(),
            text_matrix: Matrix::IDENTITY,
            captures: Vec::new(),
            positions: false,
        }
    }

    /// Record each character's advance as well as the line's total.
    fn with_positions(mut self) -> Self {
        self.positions = true;
        self
    }

    fn run(mut self, content: &[u8]) -> Result<Vec<Line>> {
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
        Ok(self.ordered_lines())
    }

    /// The painted lines, ordered for a reader rather than for the painter.
    ///
    /// Descending `y` is top to bottom in default user space; `x` breaks ties so
    /// that a line's fragments stay left to right. A page with several columns
    /// sorts left-to-right within a band — an approximation this crate states in
    /// the ADR rather than guesses at with a layout engine.
    fn ordered_lines(mut self) -> Vec<Line> {
        self.lines.retain(|line| !line.text.is_empty());
        self.lines.sort_by(|left, right| {
            right
                .y
                .partial_cmp(&left.y)
                .unwrap_or(core::cmp::Ordering::Equal)
                .then(
                    left.x
                        .partial_cmp(&right.x)
                        .unwrap_or(core::cmp::Ordering::Equal),
                )
        });
        self.lines
    }

    /// The line at the current text position, created on first use.
    fn line(&mut self) -> &mut Line {
        if let Some(index) = self
            .lines
            .iter()
            .position(|line| difference(line.y, self.line_y) <= LINE_TOLERANCE)
        {
            return &mut self.lines[index];
        }
        self.lines.push(Line {
            x: self.line_x,
            y: self.line_y,
            text: String::new(),
            matrix: self.text_matrix.multiply(self.ctm),
            size: self.size,
            advance: 0.0,
            advances: Vec::new(),
        });
        #[allow(clippy::unwrap_used)]
        self.lines.last_mut().unwrap()
    }

    /// `BDC` whose properties carry `/ActualText`: what the writer says the
    /// content means, in place of what the glyph codes would give (`14.9.4`).
    ///
    /// The replacement is remembered rather than substituted immediately, because
    /// the text it replaces is only known once the sequence closes — and a
    /// ligature glyph is exactly one code whose text is two characters.
    fn enter_marked(&mut self, properties: Option<&Object>) -> Result<()> {
        let mut capture = None;
        if let Some(properties) = properties {
            let resolved = self
                .document
                .resolve(Some(properties))?
                .unwrap_or(Object::Null);
            if let Object::Dictionary(dict) = resolved
                && let Some(Object::String(text)) = dict.get(b"ActualText")
            {
                capture = Some(Capture {
                    line: None,
                    offset: 0,
                    text: String::from_utf8_lossy(text.as_bytes()).into_owned(),
                });
            }
        }
        self.captures.push(capture);
        Ok(())
    }

    /// `EMC`: put the sequence's text where the codes' own text would have gone.
    fn leave_marked(&mut self) {
        let Some(capture) = self.captures.pop().flatten() else {
            return;
        };
        let Some(index) = capture.line else {
            return;
        };
        let Some(line) = self.lines.get_mut(index) else {
            return;
        };
        let at = char_to_byte(&line.text, capture.offset);
        line.text.insert_str(at, &capture.text);
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
                self.size = number(operands.get(1));
                self.font = Some(self.load_font(&name)?);
                self.pen_tracked = self.font.as_ref().is_some_and(Font::tracks_advances);
                self.pen_x = self.line_x;
            }
            b"Td" | b"TD" => {
                let tx = number(operands.first());
                let ty = number(operands.get(1));
                let previous_y = self.line_y;
                self.line_x += tx;
                self.line_y += ty;
                self.text_matrix = Matrix {
                    e: self.text_matrix.e + tx * self.text_matrix.a + ty * self.text_matrix.c,
                    f: self.text_matrix.f + tx * self.text_matrix.b + ty * self.text_matrix.d,
                    ..self.text_matrix
                };
                // A move along the line leaves the pen where the text stopped,
                // which is what lets a gap become a space; a move down starts a
                // new line, so the pen starts there.
                if difference(previous_y, self.line_y) > LINE_TOLERANCE {
                    self.pen_x = self.line_x;
                }
                if operator == b"TD" {
                    self.leading = -ty;
                }
            }
            b"T*" => {
                self.line_y -= self.leading;
                self.pen_x = self.line_x;
                self.text_matrix = Matrix {
                    e: self.text_matrix.e - self.leading * self.text_matrix.c,
                    f: self.text_matrix.f - self.leading * self.text_matrix.d,
                    ..self.text_matrix
                };
            }
            b"TL" => {
                self.leading = number(operands.first());
            }
            b"q" => self.ctm_stack.push(self.ctm),
            b"Q" => {
                if let Some(saved) = self.ctm_stack.pop() {
                    self.ctm = saved;
                }
            }
            b"cm" => {
                let matrix = Matrix {
                    a: number(operands.first()),
                    b: number(operands.get(1)),
                    c: number(operands.get(2)),
                    d: number(operands.get(3)),
                    e: number(operands.get(4)),
                    f: number(operands.get(5)),
                };
                // `8.4.4`: the new matrix is concatenated to the current one, so it
                // is applied to the coordinates *before* it.
                self.ctm = self.ctm.multiply(matrix);
            }
            b"Tm" => {
                // The translation is what places the line; the linear part is what
                // sizes it, and `spans()` needs both (`ADR 0008`).
                self.text_matrix = Matrix {
                    a: number(operands.first()),
                    b: number(operands.get(1)),
                    c: number(operands.get(2)),
                    d: number(operands.get(3)),
                    e: number(operands.get(4)),
                    f: number(operands.get(5)),
                };
                let previous_y = self.line_y;
                self.line_x = number(operands.get(4));
                self.line_y = number(operands.get(5));
                if difference(previous_y, self.line_y) > LINE_TOLERANCE {
                    self.pen_x = self.line_x;
                }
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
                self.pen_x = self.line_x;
                if let Some(Object::String(string)) = operands.first() {
                    self.show(string.as_bytes());
                }
            }
            b"\"" => {
                // Set word and character spacing, then show on the next line.
                self.line_y -= self.leading;
                self.pen_x = self.line_x;
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
            b"BDC" | b"BMC" => {
                let properties = if operator == b"BDC" {
                    operands.get(1)
                } else {
                    None
                };
                self.enter_marked(properties)?;
            }
            b"EMC" => self.leave_marked(),
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
        if -adjustment < WORD_GAP {
            return;
        }
        let space = (-adjustment / 1000.0 * self.size).max(0.0);
        let line = self.line();
        if !line.text.is_empty() && !line.text.ends_with(' ') {
            line.text.push(' ');
            line.advances.push(space);
        }
        if self.pen_tracked {
            // A positive adjustment moves the pen left, so the sign is inverted.
            self.pen_x -= adjustment / 1000.0 * self.size;
        }
    }

    fn show(&mut self, bytes: &[u8]) {
        // Collect first so the borrow of `font` ends before `line()` is called.
        let Some(font) = &self.font else {
            return;
        };
        let pieces = if self.positions {
            Some(font.pieces(bytes))
        } else {
            None
        };
        let (run, advance) = match &pieces {
            Some((text, pieces)) => (text.clone(), pieces.iter().map(|(_, em)| em).sum()),
            None => font.decode(bytes),
        };
        // A new run placed well to the right of where the pen stopped is a gap a
        // reader sees as a space; only fonts whose widths are known are trusted
        // with it, and a quarter of an em is the threshold a word space clears.
        let gap =
            self.pen_tracked && self.size > 0.0 && self.line_x > self.pen_x + 0.25 * self.size;
        // `/ActualText` replaces what the codes say: the glyphtext still places the
        // line and moves the pen, but the characters are dropped here and the
        // writer's own string is inserted when `EMC` arrives.
        let inside_has_line = self
            .captures
            .iter()
            .rev()
            .find_map(|capture| capture.as_ref())
            .is_some_and(|capture| capture.line.is_some());
        let capturing = self
            .captures
            .iter()
            .rev()
            .find_map(|capture| capture.as_ref())
            .is_some();
        if capturing {
            self.line();
            let index = self.lines.len() - 1;
            let offset = self.lines[index].text.len();
            if !inside_has_line
                && let Some(slot) = self
                    .captures
                    .iter_mut()
                    .rev()
                    .find_map(|capture| capture.as_mut())
            {
                slot.line = Some(index);
                slot.offset = offset;
            }
            if self.pen_tracked {
                self.pen_x = self.line_x + advance * self.size;
            }
            return;
        }
        let gap_width = (self.line_x - self.pen_x).max(0.0);
        let line = self.line();
        if gap && !line.text.is_empty() && !line.text.ends_with(' ') {
            line.text.push(' ');
            line.advances.push(gap_width);
        }
        line.text.push_str(&run);
        // One entry per character: a composite code can stand for several, and the
        // code's advance is shared between them.
        if let Some((_, pieces)) = &pieces
            && advance > 0.0
            && let Some(line) = self.lines.last_mut()
        {
            for (characters, em) in pieces {
                let each = em * self.size / (*characters).max(1) as f64;
                line.advances
                    .extend(core::iter::repeat_n(each, *characters));
            }
        }
        let tracked = self.pen_tracked;
        let width = advance * self.size;
        if tracked {
            self.pen_x = self.line_x + width;
            let start = self.line_x;
            if let Some(line) = self.lines.last_mut() {
                line.advance = self.pen_x - start;
            }
        } else if width > 0.0
            && let Some(line) = self.lines.last_mut()
        {
            // A simple font that states widths knows its own advance even when the
            // pen is not tracked for space purposes.
            line.advance = width;
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
        // A let-chain (`1.88`, edition 2024): the two conditions are one test.
        if let Some(Object::Name(subtype)) = dict.get(b"Subtype")
            && subtype == b"Type0"
        {
            return self.composite_font(&dict);
        }
        Ok(Font::Simple {
            encoding: self.encoding_of(&dict)?,
            widths: self.simple_widths(&dict)?,
        })
    }

    /// A simple font's `/Widths` and its descriptor's `/MissingWidth` (9.2.4),
    /// when the font states them.
    fn simple_widths(&self, dict: &Dictionary) -> Result<Option<SimpleWidths>> {
        let Some(entries) = dict.get(b"Widths") else {
            return Ok(None);
        };
        let entries = self
            .document
            .resolve(Some(entries))?
            .unwrap_or(Object::Null);
        let Object::Array(entries) = entries else {
            return Ok(None);
        };
        let first = match dict.get(b"FirstChar") {
            Some(Object::Integer(first)) => *first,
            _ => 0,
        };
        let mut widths = Vec::with_capacity(entries.len());
        for entry in &entries {
            let value = self.document.resolve(Some(entry))?.unwrap_or(Object::Null);
            widths.push(value.as_real().unwrap_or(0.0));
        }
        let missing = dict
            .get(b"FontDescriptor")
            .map(|descriptor| self.document.resolve(Some(descriptor)))
            .transpose()?
            .flatten()
            .and_then(|descriptor| match descriptor {
                Object::Dictionary(descriptor) => descriptor.get(b"MissingWidth").cloned(),
                _ => None,
            })
            .and_then(|value| value.as_real())
            .unwrap_or(0.0);
        Ok(Some(SimpleWidths {
            first,
            widths,
            missing,
        }))
    }

    /// A Type0 font: an Identity encoding, a `/ToUnicode` map, and its widths.
    ///
    /// Only Identity encodings are read. Any other `/Encoding` names a CMap
    /// resource — `/90ms-RKSJ-H` and friends — whose bytes this crate does not
    /// ship yet, and an embedded CMap stream is the same problem from the inside;
    /// guessing a code space would produce confidently wrong text, so those are
    /// refused by name instead.
    fn composite_font(&self, dict: &Dictionary) -> Result<Font> {
        match dict.get(b"Encoding") {
            Some(Object::Name(name)) if name == b"Identity-H" => {}
            Some(Object::Name(name)) if name == b"Identity-V" => {
                return Err(Error::Unsupported {
                    what: "vertical composite (Type0) fonts",
                });
            }
            Some(Object::Name(name)) => {
                // A predefined CMap is data: when we ship it, the encoding is
                // readable; when we do not, the name says which one is missing.
                if let Some(predefined) = crate::cmap::Predefined::by_name(name) {
                    return Ok(Font::Composite {
                        map: CodeMap::Predefined(predefined),
                        widths: self.widths_of(dict)?,
                    });
                }
                return Err(Error::Unsupported {
                    what: "CMap-encoded composite (Type0) fonts",
                });
            }
            Some(Object::Reference(_)) | Some(Object::Stream(_)) => {
                return Err(Error::Unsupported {
                    what: "embedded CMap-encoded composite (Type0) fonts",
                });
            }
            _ => {
                return Err(Error::Unsupported {
                    what: "composite (Type0) fonts without an /Encoding",
                });
            }
        }
        let Some(to_unicode) = dict.get(b"ToUnicode") else {
            return Err(Error::Unsupported {
                what: "composite (Type0) fonts without a /ToUnicode map",
            });
        };
        let Object::Reference(id) = to_unicode else {
            return Err(Error::Unsupported {
                what: "a direct /ToUnicode stream",
            });
        };
        let map = crate::cmap::ToUnicode::parse(&self.document.stream_data(*id)?)?;
        Ok(Font::Composite {
            map: CodeMap::ToUnicode(map),
            widths: self.widths_of(dict)?,
        })
    }

    /// `/W` and `/DW` from the first `/DescendantFonts` entry (9.7.4.3).
    fn widths_of(&self, dict: &Dictionary) -> Result<Widths> {
        let mut widths = Widths {
            default: 1000.0,
            runs: Vec::new(),
        };
        let Some(descendants) = dict.get(b"DescendantFonts") else {
            return Ok(widths);
        };
        let descendants = self
            .document
            .resolve(Some(descendants))?
            .unwrap_or(Object::Null);
        let Object::Array(descendants) = descendants else {
            return Err(Error::Malformed {
                what: "/DescendantFonts must be an array",
                offset: None,
            });
        };
        let Some(first) = descendants.first() else {
            return Ok(widths);
        };
        let first = self.document.resolve(Some(first))?.unwrap_or(Object::Null);
        let Object::Dictionary(descendant) = first else {
            return Ok(widths);
        };
        if let Some(default) = descendant.get(b"DW") {
            widths.default = number(Some(default));
        }
        let Some(w_array) = descendant.get(b"W") else {
            return Ok(widths);
        };
        let w_array = self
            .document
            .resolve(Some(w_array))?
            .unwrap_or(Object::Null);
        let Object::Array(items) = w_array else {
            return Err(Error::Malformed {
                what: "/W must be an array",
                offset: None,
            });
        };
        let mut index = 0;
        while index < items.len() {
            let Some(Object::Integer(first)) = items.get(index) else {
                return Err(Error::Malformed {
                    what: "/W entries start with a CID",
                    offset: None,
                });
            };
            let first = u32::try_from(*first).map_err(|_| Error::Malformed {
                what: "/W CIDs must not be negative",
                offset: None,
            })?;
            match items.get(index + 1) {
                Some(Object::Array(list)) => {
                    let run: Vec<f64> = list.iter().map(|item| number(Some(item))).collect();
                    let last = first.saturating_add(
                        u32::try_from(run.len().saturating_sub(1)).unwrap_or(u32::MAX),
                    );
                    widths.runs.push((first, last, run));
                    index += 2;
                }
                Some(Object::Integer(last)) => {
                    let width = number(items.get(index + 2));
                    let last = u32::try_from(*last).map_err(|_| Error::Malformed {
                        what: "/W CIDs must not be negative",
                        offset: None,
                    })?;
                    widths.runs.push((first, last, alloc::vec![width]));
                    index += 3;
                }
                _ => {
                    return Err(Error::Malformed {
                        what: "/W entry is neither an array nor a range",
                        offset: None,
                    });
                }
            }
        }
        Ok(widths)
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
        if let Object::Dictionary(differences) = &encoding_object
            && let Some(differences) = differences.get(b"Differences")
        {
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

/// One line of text with the rectangle it occupies, in page space.
///
/// A *line* box, not a glyph box ([ADR 0008](../docs/adr/0008-positioned-text.md)):
/// selection highlights and search hits are unions of these, and a caller that
/// needs per-character rectangles can split `text` by its advances.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSpan {
    /// The line's text, exactly as [`Document::extract_text`] reports it.
    pub text: String,
    /// Where the line sits on the page, after the CTM and the text matrix.
    pub bounds: Rect,
}

/// One match of a needle: the characters it names, where the line sits, and the
/// rectangle of the match itself.
///
/// [`SearchHit`] carries the line's box, because a hit was a line in the first
/// model; this carries the rectangle of the matched characters, derived from the
/// glyph advances the document states. It is a separate type rather than new fields
/// on `SearchHit` so that no caller's struct literal breaks: the change is additive,
/// which is what keeps it a minor release.
///
/// The rectangle is the *document's* view of the characters. Where a reader
/// substitutes a font — `pdf2-render` paints its built-in alphabet for a base-14
/// font — the painted advance differs (0.5 em stated against 6/9 em painted on
/// `tests/fixtures/text.pdf`), so a rendered highlight still has to follow the
/// glyphs the renderer placed.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    /// The matched text, as the page carries it.
    pub text: String,
    /// The line's box, for a caller that wants the whole line.
    pub line: Rect,
    /// The rectangle of the matched characters.
    pub bounds: Rect,
    /// Where the match starts in the line's text, in characters.
    pub start: usize,
    /// Where it ends, exclusive, in characters.
    pub end: usize,
}

/// One page's matches: a needle found on a page, with where each one sits.
#[derive(Debug, Clone, PartialEq)]
pub struct PageMatches {
    /// The page, counting from zero as [`Document::page`] does.
    pub page: usize,
    /// The matches on that page, left to right and top to bottom.
    pub matches: Vec<SearchMatch>,
}

/// One page's search hits: a needle found on a page, with where it was found.
#[derive(Debug, Clone, PartialEq)]
pub struct PageHits {
    /// The page, counting from zero as [`Document::page`] does.
    pub page: usize,
    /// The hits on that page, left to right and top to bottom.
    pub hits: Vec<SearchHit>,
}

/// One occurrence of a search needle.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The matched text, as the page carries it — the needle's own case is not
    /// substituted, so a caller can highlight what is really there.
    pub text: String,
    /// The line's box on the page. Per-character rectangles need per-glyph
    /// advances, which extraction does not collect yet; a line box is honest about
    /// that and is what a find box highlights.
    pub bounds: Rect,
}

impl Line {
    /// The line's box in page space, or `None` when it cannot be placed.
    /// The rectangle the characters `[start, end)` occupy, in the page space the
    /// line's matrix puts them in.
    ///
    /// The advances are the ones the document states (round 1); a font that states
    /// none falls back to the half-em estimate, spread evenly, so the rectangle is
    /// always inside the line's own box.
    fn bounds_of(&self, start: usize, end: usize) -> Rect {
        let characters = self.text.chars().count();
        let end = end.min(characters);
        let start = start.min(end);
        let total = if self.advance > 0.0 {
            self.advance
        } else {
            self.size * 0.5 * characters as f64
        };
        let x_at = |index: usize| -> f64 {
            if self.advances.len() == characters && characters > 0 {
                self.advances[..index].iter().sum()
            } else if characters > 0 {
                total * index as f64 / characters as f64
            } else {
                0.0
            }
        };
        let matrix = self.matrix;
        let left = x_at(start);
        let right = x_at(end);
        let (x0, y0) = matrix.apply(left, 0.0);
        let (x1, y1) = matrix.apply(right, 0.0);
        let (x2, y2) = matrix.apply(left, self.size);
        let (x3, y3) = matrix.apply(right, self.size);
        Rect::new(
            x0.min(x1).min(x2).min(x3),
            y0.min(y1).min(y2).min(y3),
            x0.max(x1).max(x2).max(x3),
            y0.max(y1).max(y2).max(y3),
        )
    }

    fn bounds(&self) -> Rect {
        let matrix = self.matrix;
        let (x0, y0) = matrix.apply(0.0, 0.0);
        // A font that reports no advances leaves the pen where it started; half an
        // em per character is the estimate the built-in alphabet uses, and it is
        // stated here rather than hidden in a caller.
        let advance = if self.advance > 0.0 {
            self.advance
        } else {
            self.size * 0.5 * self.text.chars().count() as f64
        };
        let (x1, y1) = matrix.apply(advance, 0.0);
        let (x2, y2) = matrix.apply(0.0, self.size);
        let (x3, y3) = matrix.apply(advance, self.size);
        Rect::new(
            x0.min(x1).min(x2).min(x3),
            y0.min(y1).min(y2).min(y3),
            x0.max(x1).max(x2).max(x3),
            y0.max(y1).max(y2).max(y3),
        )
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
        let lines = TextInterpreter::new(self, resources.cloned()).run(content)?;
        Ok(lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// The page's lines with their rectangles, in the same order `extract_text`
    /// reports the text ([ADR 0008](../docs/adr/0008-positioned-text.md)).
    pub fn spans(&self, page: &Page) -> Result<Vec<TextSpan>> {
        let content = self.page_content(page)?;
        self.spans_from(&content, page.resources.as_ref())
    }

    /// Every occurrence of `needle` in the document's pages, case-insensitively.
    ///
    /// The search runs over the **spans**, so a phrase is found inside a line and
    /// never across a line break: a reader searching "first line" wants the words,
    /// and a reader searching for text that only matches if two lines are glued
    /// together wants a different tool. An empty needle matches nothing rather than
    /// everything.
    pub fn search(&self, needle: &str) -> Result<Vec<PageHits>> {
        let folded = needle.to_lowercase();
        let mut found = Vec::new();
        if folded.is_empty() {
            return Ok(found);
        }
        for (index, page) in self.pages()?.iter().enumerate() {
            let content = self.page_content(page)?;
            let mut hits = Vec::new();
            // A hit needs its own rectangle, which is cut from the characters'
            // advances -- so this path asks the interpreter to keep them.
            for span in self.spans_with_positions(&content, page.resources.as_ref())? {
                let haystack = span.text.to_lowercase();
                let mut from = 0;
                while let Some(at) = haystack[from..].find(&folded) {
                    let start = from + at;
                    let end = start + folded.len();
                    // The matched slice of the *original* text, which keeps its case.
                    let first = char_boundary(&span.text, start);
                    let last = char_boundary(&span.text, end);
                    let text = span
                        .text
                        .get(first..last)
                        .unwrap_or(span.text.as_str())
                        .to_string();
                    hits.push(SearchHit {
                        text,
                        bounds: span.bounds,
                    });
                    from = end;
                }
            }
            if !hits.is_empty() {
                found.push(PageHits { page: index, hits });
            }
        }
        Ok(found)
    }

    /// The spans of a decoded content stream, with each character's advance kept
    /// for the line -- what a hit's own rectangle is cut from.
    pub(crate) fn spans_with_positions(
        &self,
        content: &[u8],
        resources: Option<&Dictionary>,
    ) -> Result<Vec<TextSpan>> {
        let lines = TextInterpreter::new(self, resources.cloned())
            .with_positions()
            .run(content)?;
        Ok(lines
            .into_iter()
            .map(|line| TextSpan {
                bounds: line.bounds(),
                text: line.text,
            })
            .collect())
    }

    /// Every match of `needle`, with the rectangle of the matched characters.
    ///
    /// The match is found on the lowercased line, as [`Document::search`] finds it,
    /// but the offsets and the rectangle describe the *original* text: folding can
    /// change length (`İ` lowercases to two code points), so the folded string is
    /// built with a map back to the characters it came from.
    pub fn search_matches(&self, needle: &str) -> Result<Vec<PageMatches>> {
        let folded_needle = needle.to_lowercase();
        let mut found = Vec::new();
        if folded_needle.is_empty() {
            return Ok(found);
        }
        for (index, page) in self.pages()?.iter().enumerate() {
            let content = self.page_content(page)?;
            let lines = TextInterpreter::new(self, page.resources.clone())
                .with_positions()
                .run(&content)?;
            let mut matches = Vec::new();
            for line in &lines {
                let (folded, origin) = fold_with_origin(&line.text);
                let mut from = 0;
                while let Some(at) = folded[from..].find(&folded_needle) {
                    let start = from + at;
                    let end = start + folded_needle.len();
                    let first = origin[start];
                    let last = origin[end - 1] + 1;
                    matches.push(SearchMatch {
                        text: line.text.chars().skip(first).take(last - first).collect(),
                        line: line.bounds(),
                        bounds: line.bounds_of(first, last),
                        start: first,
                        end: last,
                    });
                    from = end;
                }
            }
            if !matches.is_empty() {
                found.push(PageMatches {
                    page: index,
                    matches,
                });
            }
        }
        Ok(found)
    }

    /// Spans of a decoded content stream and its resource dictionary.
    pub fn spans_from(
        &self,
        content: &[u8],
        resources: Option<&Dictionary>,
    ) -> Result<Vec<TextSpan>> {
        let lines = TextInterpreter::new(self, resources.cloned()).run(content)?;
        Ok(lines
            .into_iter()
            .map(|line| TextSpan {
                bounds: line.bounds(),
                text: line.text,
            })
            .collect())
    }
}

/// The lowercased text and, for each of its *bytes*, the character index it came
/// from in the original.
///
/// `to_lowercase` can change length (`İ` is two code points), so a byte found in
/// the folded string cannot be used as an offset into the original without a map.
fn fold_with_origin(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (index, character) in text.chars().enumerate() {
        for lowered in character.to_lowercase() {
            let before = folded.len();
            folded.push(lowered);
            origin.extend(core::iter::repeat_n(index, folded.len() - before));
        }
    }
    (folded, origin)
}

/// The byte offset of a character position in `text`.
fn char_to_byte(text: &str, characters: usize) -> usize {
    text.char_indices()
        .nth(characters)
        .map_or(text.len(), |(at, _)| at)
}

/// The nearest character boundary at or below a byte offset.
///
/// `str::to_lowercase` can change a string's length (`İ` becomes two bytes), so a
/// match offset in the folded text is *not* necessarily a boundary in the original;
/// the slice is widened to one rather than panicking on it.
fn char_boundary(text: &str, offset: usize) -> usize {
    let mut at = offset.min(text.len());
    while at > 0 && !text.is_char_boundary(at) {
        at -= 1;
    }
    at
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

#[cfg(test)]
mod width_tests {
    use super::{Font, SimpleWidths};
    use crate::encoding::Encoding;

    fn widths(first: i64, values: &[f64], missing: f64) -> SimpleWidths {
        SimpleWidths {
            first,
            widths: values.to_vec(),
            missing,
        }
    }

    #[test]
    fn a_code_takes_its_width_from_the_table_and_the_rest_from_missing_width() {
        let table = widths(65, &[500.0, 1000.0], 250.0);
        assert_eq!(table.advance(b'A'), Some(0.5), "the first entry is code 65");
        assert_eq!(table.advance(b'B'), Some(1.0));
        assert_eq!(
            table.advance(b'C'),
            Some(0.25),
            "past the array: /MissingWidth"
        );
        assert_eq!(table.advance(b'@'), Some(0.25), "before the array too");
        let without = widths(65, &[500.0], 0.0);
        assert_eq!(without.advance(b'Z'), None, "no width, no guess");
    }

    #[test]
    fn a_simple_font_reports_one_advance_per_code_it_states() {
        let font = Font::Simple {
            encoding: Encoding::win_ansi(),
            widths: Some(widths(65, &[500.0, 600.0], 0.0)),
        };
        let (text, pieces) = font.pieces(b"AB");
        assert_eq!(text, "AB");
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].0, 1, "one character per code");
        assert!((pieces[0].1 - 0.5).abs() < 1e-9);
        assert!((pieces[1].1 - 0.6).abs() < 1e-9);
        let (text, advance) = font.decode(b"AB");
        assert_eq!(text, "AB");
        assert!(
            (advance - 1.1).abs() < 1e-9,
            "the total is the entries' sum"
        );
    }

    /// A one-page PDF around `(object number, body)` pairs.
    fn document(objects: &[(u32, &str)]) -> crate::Document {
        let mut out = b"%PDF-2.0\n".to_vec();
        let mut offsets = Vec::new();
        for (number, body) in objects {
            offsets.push((*number, out.len()));
            out.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n");
        for (number, offset) in &offsets {
            out.extend_from_slice(format!("{number} 1\n{offset:010} 00000 n \n").as_bytes());
        }
        let size = objects.iter().map(|(number, _)| *number).max().unwrap_or(0) + 1;
        out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
        out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        crate::Document::open(out).expect("the test document opens")
    }

    #[test]
    fn a_run_keeps_one_advance_per_character_when_asked() {
        // Two codes of 500 and 1000 units at 10 pt: the line's characters advance
        // by 5 and 10 units, which is what a hit's rectangle is cut from.
        let content = "BT /F1 10 Tf 10 10 Td (AB) Tj ET";
        let document = document(&[
            (1, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (
                3,
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
                 /Resources << /Font << /F1 5 0 R >> >> >>",
            ),
            (
                4,
                &format!(
                    "<< /Length {} >>\nstream\n{content}\nendstream",
                    content.len()
                ),
            ),
            (
                5,
                "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding /WinAnsiEncoding \
                 /FirstChar 65 /Widths [500 1000] >>",
            ),
        ]);
        let page = document.page(0).expect("page");
        let lines = super::TextInterpreter::new(&document, page.resources.clone())
            .with_positions()
            .run("BT /F1 10 Tf 10 10 Td (AB) Tj ET".as_bytes())
            .expect("the content runs");
        let line = lines.first().expect("one line");
        assert_eq!(line.text, "AB");
        assert_eq!(line.advances.len(), 2, "one advance per character");
        assert!((line.advances[0] - 5.0).abs() < 1e-9, "0.5 em of 10 pt");
        assert!((line.advances[1] - 10.0).abs() < 1e-9, "1 em of 10 pt");
        assert!((line.advance - 15.0).abs() < 1e-9, "and the line's total");
    }

    #[test]
    fn offsets_survive_a_character_whose_lowercase_is_longer() {
        // `İ` lowercases to two code points, so a byte offset in the folded string is
        // not one in the original: the map is what keeps a match on the right
        // characters. No fixture can carry such a character, so this is the
        // function's own test.
        let (folded, origin) = super::fold_with_origin("İstanbul");
        assert!(folded.starts_with("i\u{307}stanbul"), "folded: {folded}");
        let at = folded.find("stan").expect("the needle folds in");
        let first = origin[at];
        let last = origin[at + 3];
        assert_eq!(
            (first, last),
            (1, 4),
            "the characters 'stan' in the original"
        );
        let taken: String = "İstanbul"
            .chars()
            .skip(first)
            .take(last + 1 - first)
            .collect();
        assert_eq!(
            taken, "stan",
            "and the offsets name exactly those characters"
        );
    }

    #[test]
    fn a_simple_font_without_widths_still_reads_its_text_and_no_advance() {
        let font = Font::Simple {
            encoding: Encoding::win_ansi(),
            widths: None,
        };
        let (text, advance) = font.decode(b"Hi");
        assert_eq!(text, "Hi");
        assert_eq!(advance, 0.0, "the caller's estimate applies");
    }
}
