//! Fonts for text rendering: a provider trait, a built-in base-14 fallback and
//! TrueType outlines through `ttf-parser`.

use pdf2::Encoding;

use crate::geom::Point;

/// One glyph's outline in em units: y grows upwards, the baseline is y = 0 and
/// one em is 1.0.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glyph {
    /// Closed contours of the outline.
    pub contours: Vec<Vec<Point>>,
    /// Advance width in em units.
    pub advance: f64,
}

/// A resolved font ready to place glyphs.
#[derive(Debug, Clone)]
pub enum Font {
    /// The built-in fallback: a 5×7 bitmap alphabet, used when the document
    /// carries no usable font program (all of the base-14 cases).
    Builtin {
        /// Code → character table.
        encoding: Encoding,
    },
    /// An embedded TrueType program.
    TrueType {
        /// Code → character table.
        encoding: Encoding,
        /// The font program, still compressed in the PDF sense is not our
        /// business: this is the decoded `/FontFile2`.
        data: Vec<u8>,
    },
}

impl Font {
    /// The built-in fallback with an encoding.
    pub fn builtin(encoding: Encoding) -> Self {
        Self::Builtin { encoding }
    }

    /// An embedded TrueType program, rejected when it cannot be parsed.
    pub fn true_type(encoding: Encoding, data: Vec<u8>) -> Option<Self> {
        let face = ttf_parser::Face::parse(&data, 0).ok()?;
        if face.units_per_em() == 0 {
            return None;
        }
        Some(Self::TrueType { encoding, data })
    }

    /// The outline and advance for a byte code, or `None` when there is no
    /// glyph for it.
    pub fn glyph(&self, code: u8) -> Option<Glyph> {
        match self {
            Self::Builtin { encoding } => builtin_glyph(encoding.character(code)?),
            Self::TrueType { encoding, data } => {
                let character = encoding.character(code)?;
                let face = ttf_parser::Face::parse(data, 0).ok()?;
                let glyph_id = face.glyph_index(character)?;
                let units = f64::from(face.units_per_em());
                let mut builder = OutlineBuilder::new(units);
                face.outline_glyph(glyph_id, &mut builder)?;
                let advance = f64::from(face.glyph_hor_advance(glyph_id).unwrap_or(0)) / units;
                Some(Glyph {
                    contours: builder.contours,
                    advance,
                })
            }
        }
    }

    /// The advance of a code in em units, falling back to the built-in metric.
    pub fn advance(&self, code: u8) -> f64 {
        self.glyph(code)
            .map_or(BUILTIN_ADVANCE, |glyph| glyph.advance)
    }
}

const BUILTIN_ADVANCE: f64 = 6.0 / 9.0;
const BUILTIN_CELL: f64 = 1.0 / 9.0;
const BUILTIN_WIDTH: u8 = 5;
const BUILTIN_HEIGHT: u8 = 7;

/// A 5×7 bitmap alphabet. Lowercase letters use the uppercase shapes; anything
/// without a pattern is drawn as a notdef box.
fn builtin_glyph(character: char) -> Option<Glyph> {
    let code = u8::try_from(u32::from(character)).ok()?;
    let rows = if character == ' ' {
        [0_u8; BUILTIN_HEIGHT as usize]
    } else if let Some(rows) = glyph_rows(code) {
        rows
    } else if character == '\u{2022}' {
        [0, 0, 0b00100, 0b01110, 0b00100, 0, 0]
    } else if let Some(upper) = upper(character) {
        glyph_rows(upper as u8).unwrap_or(notdef_rows())
    } else {
        notdef_rows()
    };
    Some(Glyph {
        contours: rows_to_contours(&rows),
        advance: BUILTIN_ADVANCE,
    })
}

fn upper(character: char) -> Option<char> {
    character
        .to_uppercase()
        .next()
        .filter(|upper| *upper != character)
}

fn notdef_rows() -> [u8; BUILTIN_HEIGHT as usize] {
    [
        0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
    ]
}

fn rows_to_contours(rows: &[u8; BUILTIN_HEIGHT as usize]) -> Vec<Vec<Point>> {
    let mut contours = Vec::new();
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..BUILTIN_WIDTH {
            let mask = 1_u8 << (BUILTIN_WIDTH - 1 - column);
            if bits & mask == 0 {
                continue;
            }
            let x0 = f64::from(column) * BUILTIN_CELL;
            let x1 = x0 + BUILTIN_CELL;
            let top = f64::from(BUILTIN_HEIGHT - 1 - row as u8) * BUILTIN_CELL;
            let bottom = top + BUILTIN_CELL;
            contours.push(vec![
                Point::new(x0, bottom),
                Point::new(x1, bottom),
                Point::new(x1, top),
                Point::new(x0, top),
            ]);
        }
    }
    contours
}

fn glyph_rows(code: u8) -> Option<[u8; 7]> {
    let rows = match code {
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        b'6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        b'.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        b',' => [0, 0, 0, 0, 0b01100, 0b01100, 0b01000],
        b'!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        b'?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        b'-' => [0, 0, 0, 0b11111, 0, 0, 0],
        b':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        b';' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0b01000],
        b'\'' => [0b00100, 0b00100, 0b01000, 0, 0, 0, 0],
        b'"' => [0b01010, 0b01010, 0, 0, 0, 0, 0],
        b'(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        b')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        b'/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        b'+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        b'=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        b'*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        b'#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        b'%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        b'&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        _ => return None,
    };
    Some(rows)
}

/// Collects `ttf-parser` outlines into em-space contours.
struct OutlineBuilder {
    units: f64,
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
}

impl OutlineBuilder {
    fn new(units: f64) -> Self {
        Self {
            units,
            contours: Vec::new(),
            current: Vec::new(),
        }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(f64::from(x) / self.units, f64::from(y) / self.units)
    }

    fn finish_contour(&mut self) {
        if self.current.len() > 1 {
            self.contours.push(std::mem::take(&mut self.current));
        }
    }
}

impl ttf_parser::OutlineBuilder for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        self.current.push(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(self.point(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(&start) = self.current.last() else {
            return;
        };
        let control = self.point(x1, y1);
        let end = self.point(x, y);
        for step in 1..=8 {
            let t = f64::from(step) / 8.0;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * start.x + 2.0 * u * t * control.x + t * t * end.x,
                u * u * start.y + 2.0 * u * t * control.y + t * t * end.y,
            ));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let Some(&start) = self.current.last() else {
            return;
        };
        let c1 = self.point(x1, y1);
        let c2 = self.point(x2, y2);
        let end = self.point(x, y);
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
    }

    fn close(&mut self) {
        self.finish_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::Font;
    use pdf2::Encoding;

    #[test]
    fn the_builtin_font_draws_letters_as_ink() {
        let font = Font::builtin(Encoding::standard());
        let glyph = font.glyph(b'H').expect("H");
        assert!(!glyph.contours.is_empty());
        assert!((glyph.advance - 0.6667).abs() < 1e-3);
        // Every contour is a rectangle inside the glyph box.
        for contour in &glyph.contours {
            assert_eq!(contour.len(), 4);
            for point in contour {
                assert!(point.x >= 0.0 && point.x <= 6.0 / 9.0 + 1e-9);
                assert!(point.y >= 0.0 && point.y <= 7.0 / 9.0 + 1e-9);
            }
        }
    }

    #[test]
    fn the_builtin_font_is_blank_for_space() {
        let font = Font::builtin(Encoding::standard());
        let glyph = font.glyph(b' ').expect("space");
        assert!(glyph.contours.is_empty());
        assert!(glyph.advance > 0.0);
    }

    #[test]
    fn a_broken_font_program_is_rejected() {
        assert!(Font::true_type(Encoding::standard(), vec![0, 1, 2, 3]).is_none());
        assert!(Font::true_type(Encoding::standard(), Vec::new()).is_none());
    }

    #[test]
    fn unknown_characters_become_the_notdef_box() {
        let font = Font::builtin(Encoding::standard());
        // 0xA4 is undefined in StandardEncoding, so there is no glyph at all.
        assert!(font.glyph(0xA4).is_none());
        // A mapped but unpatterned character gets the notdef box, not a panic.
        let glyph = font.glyph(b'@').expect("@");
        assert_eq!(
            glyph.contours.len(),
            20,
            "the notdef box is a hollow 5×7 frame"
        );
    }
}
