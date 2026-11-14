//! The content-stream walker: graphics state, path construction, painting
//! operators, clipping and dash handling.
//!
//! The walker reuses [`pdf2::Parser`] (the same tokenizer the object parser
//! uses) and maps operators to draw calls. Operators it does not implement —
//! text and images, until the next round — are counted and reported, never
//! fatal: a page with text still paints its paths.

use std::collections::BTreeMap;
use std::fmt;

use pdf2::lexer::{is_delimiter, is_whitespace};
use pdf2::{Document, Object, Page, Parser, Token};

use crate::canvas::{Canvas, ClipMask, Color};
use crate::geom::{Matrix, Point};
use crate::path::{dash_polyline, stroke_outline, Cap, Dash, Join, Path};
use crate::raster::{clip_mask, fill_polygons};

/// What a page render should look like.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Device pixels per PDF point: `1.0` is 72 dpi.
    pub scale: f64,
    /// Canvas background.
    pub background: Color,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            background: Color::WHITE,
        }
    }
}

/// What happened while rendering, beyond the pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    /// Operators that were recognised but not drawn, with how often each
    /// appeared. Text and image operators land here until they are implemented.
    pub unsupported: BTreeMap<String, usize>,
}

impl Report {
    /// Total unsupported operators.
    pub fn unsupported_total(&self) -> usize {
        self.unsupported.values().sum()
    }

    fn note(&mut self, operator: &[u8]) {
        let name = String::from_utf8_lossy(operator).into_owned();
        *self.unsupported.entry(name).or_insert(0) += 1;
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.unsupported.is_empty() {
            return write!(formatter, "all operators supported");
        }
        let mut first = true;
        for (name, count) in &self.unsupported {
            if !first {
                write!(formatter, ", ")?;
            }
            first = false;
            write!(formatter, "{name}×{count}")?;
        }
        Ok(())
    }
}

/// A rendered page.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    /// The raster.
    pub canvas: Canvas,
    /// What was skipped.
    pub report: Report,
}

/// A document, page number (zero based) and rasterisation scale in one call.
pub fn render_page_number(
    document: &Document,
    page_index: usize,
    options: &RenderOptions,
) -> crate::Result<RenderedPage> {
    let page = document.page(page_index)?;
    render_page(document, &page, options)
}

/// Rasterise one page.
pub fn render_page(
    document: &Document,
    page: &Page,
    options: &RenderOptions,
) -> crate::Result<RenderedPage> {
    let scale = if options.scale.is_finite() && options.scale > 0.0 {
        options.scale
    } else {
        1.0
    };
    let media = page
        .media_box
        .unwrap_or(pdf2::Rect::new(0.0, 0.0, 612.0, 792.0));
    let width = ((media.width() * scale).round().max(1.0)) as u32;
    let height = ((media.height() * scale).round().max(1.0)) as u32;
    let mut canvas = Canvas::new(width, height, options.background);
    let mut report = Report::default();

    // PDF user space to device space: shift the media box origin to (0, 0) and
    // flip the y axis, which grows downwards in the raster.
    let base = Matrix::new(scale, 0.0, 0.0, -scale, -media.x0 * scale, media.y1 * scale);

    let content = document.page_content(page)?;
    let mut walker = Walker {
        canvas: &mut canvas,
        report: &mut report,
        state: State::new(base),
        stack: Vec::new(),
        path: Path::new(),
        pending_clip: None,
        mask: ClipMask::all(width, height),
    };
    walker.run(&content)?;
    Ok(RenderedPage { canvas, report })
}

/// The part of the graphics state that painting depends on.
#[derive(Debug, Clone)]
struct State {
    ctm: Matrix,
    line_width: f64,
    cap: Cap,
    join: Join,
    dash: Dash,
    fill: Color,
    stroke: Color,
}

impl State {
    fn new(ctm: Matrix) -> Self {
        Self {
            ctm,
            line_width: 1.0,
            cap: Cap::Butt,
            join: Join::Miter,
            dash: Dash::solid(),
            fill: Color::BLACK,
            stroke: Color::BLACK,
        }
    }
}

struct Walker<'a> {
    canvas: &'a mut Canvas,
    report: &'a mut Report,
    state: State,
    stack: Vec<(State, ClipMask)>,
    path: Path,
    pending_clip: Option<bool>,
    mask: ClipMask,
}

impl Walker<'_> {
    fn run(&mut self, content: &[u8]) -> crate::Result<()> {
        let mut parser = Parser::new(content);
        let mut operands: Vec<Object> = Vec::new();
        loop {
            match parser.peek_token()? {
                Token::Eof => break,
                Token::Keyword(_) => {
                    let operator = match parser.next_token()? {
                        Token::Keyword(operator) => operator,
                        _ => return Err(crate::Error::malformed("content-stream operator")),
                    };
                    self.execute(&operator, &mut operands, &mut parser)?;
                    operands.clear();
                }
                _ => {
                    operands.push(parser.parse_object()?);
                }
            }
        }
        Ok(())
    }

    fn execute(
        &mut self,
        operator: &[u8],
        operands: &mut [Object],
        parser: &mut Parser<'_>,
    ) -> crate::Result<()> {
        match operator {
            b"q" => {
                self.stack.push((self.state.clone(), self.mask.clone()));
            }
            b"Q" => {
                if let Some((state, mask)) = self.stack.pop() {
                    self.state = state;
                    self.mask = mask;
                }
            }
            b"cm" => {
                if let Some(matrix) = matrix_operand(operands) {
                    self.state.ctm = matrix.multiply(self.state.ctm);
                }
            }
            b"w" => self.state.line_width = number(operands, 0),
            b"J" => {
                self.state.cap = match integer(operands, 0) {
                    1 => Cap::Round,
                    2 => Cap::Square,
                    _ => Cap::Butt,
                }
            }
            b"j" => {
                self.state.join = match integer(operands, 0) {
                    1 => Join::Round,
                    2 => Join::Bevel,
                    _ => Join::Miter,
                }
            }
            b"M" => {}
            b"d" => self.state.dash = dash_operand(operands),
            b"m" => {
                if let Some(point) = point_operand(operands, 0) {
                    self.path.move_to(point);
                }
            }
            b"l" => {
                if let Some(point) = point_operand(operands, 0) {
                    self.path.line_to(point);
                }
            }
            b"c" => {
                if let (Some(c1), Some(c2), Some(to)) = (
                    point_operand(operands, 0),
                    point_operand(operands, 1),
                    point_operand(operands, 2),
                ) {
                    self.path.curve_to(c1, c2, to);
                }
            }
            b"v" | b"y" => {
                // `v` and `y` are shorthand curves; draw them as a straight
                // line to the end point, which keeps the path closed.
                if let Some(to) = point_operand(operands, 1) {
                    self.path.line_to(to);
                }
            }
            b"h" => self.path.close(),
            b"re" => {
                let x = number(operands, 0);
                let y = number(operands, 1);
                let width = number(operands, 2);
                let height = number(operands, 3);
                self.path.add_rect(x, y, width, height);
            }
            b"S" => self.paint(Paint::Stroke, false)?,
            b"s" => {
                self.path.close();
                self.paint(Paint::Stroke, false)?;
            }
            b"f" | b"F" => self.paint(Paint::Fill, false)?,
            b"f*" => self.paint(Paint::Fill, true)?,
            b"B" => self.paint(Paint::Both, false)?,
            b"B*" => self.paint(Paint::Both, true)?,
            b"b" => {
                self.path.close();
                self.paint(Paint::Both, false)?;
            }
            b"b*" => {
                self.path.close();
                self.paint(Paint::Both, true)?;
            }
            b"W" => self.pending_clip = Some(false),
            b"W*" => self.pending_clip = Some(true),
            b"n" => {
                self.apply_pending_clip();
                self.path = Path::new();
            }
            b"g" => {
                self.state.fill = Color::rgb(
                    number(operands, 0),
                    number(operands, 0),
                    number(operands, 0),
                )
            }
            b"G" => {
                self.state.stroke = Color::rgb(
                    number(operands, 0),
                    number(operands, 0),
                    number(operands, 0),
                )
            }
            b"rg" => {
                self.state.fill = Color::rgb(
                    number(operands, 0),
                    number(operands, 1),
                    number(operands, 2),
                );
            }
            b"RG" => {
                self.state.stroke = Color::rgb(
                    number(operands, 0),
                    number(operands, 1),
                    number(operands, 2),
                );
            }
            b"k" => {
                self.state.fill = Color::from_cmyk(
                    number(operands, 0),
                    number(operands, 1),
                    number(operands, 2),
                    number(operands, 3),
                );
            }
            b"K" => {
                self.state.stroke = Color::from_cmyk(
                    number(operands, 0),
                    number(operands, 1),
                    number(operands, 2),
                    number(operands, 3),
                );
            }
            b"BI" => {
                skip_inline_image(parser)?;
                self.report.note(operator);
            }
            _ => self.report.note(operator),
        }
        Ok(())
    }

    fn paint(&mut self, what: Paint, even_odd: bool) -> crate::Result<()> {
        self.apply_pending_clip();
        let ctm = self.state.ctm;
        let scale = determinant_scale(ctm);
        let polylines = self.path.flatten(ctm);
        let painted = match what {
            Paint::Fill | Paint::Both => true,
            Paint::Stroke => false,
        };
        if painted && !polylines.is_empty() {
            fill_polygons(
                self.canvas,
                &self.mask,
                &polylines,
                self.state.fill,
                even_odd,
                1.0,
            );
        }
        if matches!(what, Paint::Stroke | Paint::Both) {
            let mut stroked: Vec<Vec<Point>> = Vec::new();
            for polyline in &polylines {
                stroked.extend(dash_polyline(polyline, &self.state.dash, scale));
            }
            let outline = stroke_outline(&stroked, self.state.line_width * scale, self.state.cap);
            if !outline.is_empty() {
                fill_polygons(
                    self.canvas,
                    &self.mask,
                    &outline,
                    self.state.stroke,
                    false,
                    1.0,
                );
            }
        }
        self.path = Path::new();
        Ok(())
    }

    fn apply_pending_clip(&mut self) {
        let Some(even_odd) = self.pending_clip.take() else {
            return;
        };
        let polylines = self.path.flatten(self.state.ctm);
        let mask = clip_mask(
            self.canvas.width(),
            self.canvas.height(),
            &polylines,
            even_odd,
        );
        self.mask.intersect(&mask);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Fill,
    Stroke,
    Both,
}

fn determinant_scale(ctm: Matrix) -> f64 {
    let determinant = ctm.a * ctm.d - ctm.b * ctm.c;
    determinant.abs().sqrt()
}

fn number(operands: &[Object], index: usize) -> f64 {
    operands.get(index).and_then(Object::as_real).unwrap_or(0.0)
}

fn integer(operands: &[Object], index: usize) -> i64 {
    operands
        .get(index)
        .and_then(Object::as_integer)
        .unwrap_or(0)
}

fn matrix_operand(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    Some(Matrix::new(
        number(operands, 0),
        number(operands, 1),
        number(operands, 2),
        number(operands, 3),
        number(operands, 4),
        number(operands, 5),
    ))
}

fn point_operand(operands: &[Object], start: usize) -> Option<Point> {
    if operands.len() < start + 2 {
        return None;
    }
    Some(Point::new(
        number(operands, start),
        number(operands, start + 1),
    ))
}

fn dash_operand(operands: &[Object]) -> Dash {
    let pattern = match operands.first() {
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(Object::as_real)
            .collect::<Vec<f64>>(),
        _ => Vec::new(),
    };
    let phase = number(operands, 1);
    Dash { pattern, phase }
}

/// Skip inline image data (`BI` … `EI`), which cannot be tokenised.
fn skip_inline_image(parser: &mut Parser<'_>) -> crate::Result<()> {
    let start = parser.raw_position();
    let content = parser.content();
    let found = content
        .get(start..)
        .and_then(|rest| find_operator(rest, b"EI"))
        .ok_or_else(|| crate::Error::malformed("inline image without a closing EI"))?;
    parser.seek(start + found + 2);
    Ok(())
}

fn find_operator(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if &haystack[index..index + needle.len()] == needle {
            let delimiter = |byte: u8| is_whitespace(byte) || is_delimiter(byte);
            let before_ok = index == 0 || delimiter(haystack[index - 1]);
            let after = index + needle.len();
            let after_ok = after >= haystack.len() || delimiter(haystack[after]);
            if before_ok && after_ok {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{dash_operand, Report};
    use pdf2::Object;

    #[test]
    fn reports_collect_operator_counts_in_order() {
        let mut report = Report::default();
        report.note(b"Tj");
        report.note(b"Tj");
        report.note(b"Do");
        assert_eq!(report.unsupported_total(), 3);
        assert_eq!(report.unsupported.get("Tj"), Some(&2));
        assert_eq!(report.to_string(), "Do×1, Tj×2");
    }

    #[test]
    fn dash_operands_default_to_solid() {
        assert!(dash_operand(&[]).is_solid());
        let dash = dash_operand(&[
            Object::array([Object::Integer(4), Object::Integer(2)]),
            Object::Integer(1),
        ]);
        assert_eq!(dash.pattern, vec![4.0, 2.0]);
        assert_eq!(dash.phase, 1.0);
    }
}
