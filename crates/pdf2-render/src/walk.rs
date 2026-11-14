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
use pdf2::{Dictionary, Document, Encoding, Object, Page, Parser, Token};

use crate::canvas::{Canvas, ClipMask, Color};
use crate::font::Font;
use crate::geom::{Matrix, Point};
use crate::image::{resolve, Image};
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

/// `/Widths` of a simple font, in 1/1000 em, with the descriptor's
/// `/MissingWidth` for codes the array does not cover (9.2.4).
#[derive(Debug, Clone, Default, PartialEq)]
struct SimpleWidths {
    first: i64,
    widths: Vec<f64>,
    missing: f64,
}

impl SimpleWidths {
    fn advance(&self, code: u8) -> f64 {
        let code = i64::from(code);
        let index = code - self.first;
        if index >= 0 {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            if let Some(width) = self.widths.get(index as usize) {
                return width / 1000.0;
            }
        }
        self.missing / 1000.0
    }
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
    // `/Rotate` turns the page clockwise for display, so a quarter turn swaps
    // the canvas dimensions.
    let rotate = page.rotate.rem_euclid(360);
    let landscape = rotate == 90 || rotate == 270;
    let (width_points, height_points) = if landscape {
        (media.height(), media.width())
    } else {
        (media.width(), media.height())
    };
    let width = ((width_points * scale).round().max(1.0)) as u32;
    let height = ((height_points * scale).round().max(1.0)) as u32;
    let mut canvas = Canvas::new(width, height, options.background);
    let mut report = Report::default();

    // PDF user space to device space. Every case shifts the media box origin to
    // (0, 0); the y axis grows downwards in the raster, and `/Rotate` composes
    // a clockwise quarter turn (or two) on top of that flip.
    let base = match rotate {
        90 => Matrix::new(0.0, scale, scale, 0.0, -media.y0 * scale, -media.x0 * scale),
        180 => Matrix::new(-scale, 0.0, 0.0, scale, media.x1 * scale, -media.y0 * scale),
        270 => Matrix::new(0.0, scale, -scale, 0.0, media.y1 * scale, -media.x0 * scale),
        _ => Matrix::new(scale, 0.0, 0.0, -scale, -media.x0 * scale, media.y1 * scale),
    };

    let content = document.page_content(page)?;
    let mut walker = Walker {
        canvas: &mut canvas,
        report: &mut report,
        document,
        resources: page.resources.clone(),
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
    font: Option<Font>,
    font_size: f64,
    miter_limit: f64,
    widths: Option<SimpleWidths>,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
    text_matrix: Matrix,
    line_matrix: Matrix,
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
            font: None,
            font_size: 0.0,
            miter_limit: 10.0,
            widths: None,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            text_matrix: Matrix::IDENTITY,
            line_matrix: Matrix::IDENTITY,
        }
    }
}

struct Walker<'a> {
    canvas: &'a mut Canvas,
    report: &'a mut Report,
    document: &'a Document,
    resources: Option<Dictionary>,
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
            b"M" => self.state.miter_limit = number(operands, 0).max(1.0),
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
            b"BT" => {
                self.state.text_matrix = Matrix::IDENTITY;
                self.state.line_matrix = Matrix::IDENTITY;
            }
            b"ET" => {}
            b"Tf" => {
                let name = match operands.first() {
                    Some(Object::Name(name)) => name.clone(),
                    _ => return Err(crate::Error::malformed("/Tf needs a font name")),
                };
                self.state.font_size = number(operands, 1);
                let (font, widths) = self.load_font(&name);
                self.state.font = font;
                self.state.widths = widths;
            }
            b"Td" | b"TD" => {
                let tx = number(operands, 0);
                let ty = number(operands, 1);
                self.state.line_matrix =
                    Matrix::new(1.0, 0.0, 0.0, 1.0, tx, ty).multiply(self.state.line_matrix);
                self.state.text_matrix = self.state.line_matrix;
                if operator == b"TD" {
                    self.state.leading = -ty;
                }
            }
            b"T*" => {
                self.state.line_matrix = Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, -self.state.leading)
                    .multiply(self.state.line_matrix);
                self.state.text_matrix = self.state.line_matrix;
            }
            b"Tm" => {
                if let Some(matrix) = matrix_operand(operands) {
                    self.state.line_matrix = matrix;
                    self.state.text_matrix = matrix;
                }
            }
            b"TL" => self.state.leading = number(operands, 0),
            b"Tc" => self.state.char_spacing = number(operands, 0),
            b"Tw" => self.state.word_spacing = number(operands, 0),
            b"Tz" => self.state.horizontal_scale = number(operands, 0) / 100.0,
            b"Ts" => self.state.rise = number(operands, 0),
            b"Tj" => {
                if let Some(Object::String(string)) = operands.first() {
                    let bytes = string.as_bytes().to_vec();
                    self.show_text(&bytes)?;
                }
            }
            b"'" => {
                self.state.line_matrix = Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, -self.state.leading)
                    .multiply(self.state.line_matrix);
                self.state.text_matrix = self.state.line_matrix;
                if let Some(Object::String(string)) = operands.first() {
                    let bytes = string.as_bytes().to_vec();
                    self.show_text(&bytes)?;
                }
            }
            b"\"" => {
                self.state.word_spacing = number(operands, 0);
                self.state.char_spacing = number(operands, 1);
                self.state.line_matrix = Matrix::new(1.0, 0.0, 0.0, 1.0, 0.0, -self.state.leading)
                    .multiply(self.state.line_matrix);
                self.state.text_matrix = self.state.line_matrix;
                if let Some(Object::String(string)) = operands.get(2) {
                    let bytes = string.as_bytes().to_vec();
                    self.show_text(&bytes)?;
                }
            }
            b"TJ" => {
                if let Some(Object::Array(items)) = operands.first() {
                    for item in items {
                        match item {
                            Object::String(string) => {
                                let bytes = string.as_bytes().to_vec();
                                self.show_text(&bytes)?;
                            }
                            Object::Integer(adjustment) => {
                                self.adjust_text(*adjustment as f64);
                            }
                            Object::Real(adjustment) => self.adjust_text(*adjustment),
                            _ => {}
                        }
                    }
                }
            }
            b"Do" => {
                if let Some(Object::Name(name)) = operands.first() {
                    let name = name.clone();
                    self.draw_image(&name)?;
                }
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
            let outline = stroke_outline(
                &stroked,
                self.state.line_width * scale,
                self.state.cap,
                self.state.join,
                self.state.miter_limit,
            );
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

    fn adjust_text(&mut self, adjustment: f64) {
        let tx = -adjustment / 1000.0 * self.state.font_size * self.state.horizontal_scale;
        self.state.text_matrix =
            Matrix::new(1.0, 0.0, 0.0, 1.0, tx, 0.0).multiply(self.state.text_matrix);
    }

    fn show_text(&mut self, bytes: &[u8]) -> crate::Result<()> {
        let Some(font) = self.state.font.clone() else {
            return Ok(());
        };
        for &code in bytes {
            if let Some(glyph) = font.glyph(code) {
                if !glyph.contours.is_empty() {
                    let scale = Matrix::new(
                        self.state.font_size * self.state.horizontal_scale,
                        0.0,
                        0.0,
                        self.state.font_size,
                        0.0,
                        self.state.rise,
                    );
                    let transform = scale
                        .multiply(self.state.text_matrix)
                        .multiply(self.state.ctm);
                    let polygons: Vec<Vec<Point>> = glyph
                        .contours
                        .iter()
                        .map(|contour| {
                            contour
                                .iter()
                                .map(|point| transform.apply(*point))
                                .collect()
                        })
                        .collect();
                    fill_polygons(
                        self.canvas,
                        &self.mask,
                        &polygons,
                        self.state.fill,
                        false,
                        1.0,
                    );
                }
            }
            let em = self
                .state
                .widths
                .as_ref()
                .map_or_else(|| font.advance(code), |widths| widths.advance(code));
            let mut advance = em * self.state.font_size + self.state.char_spacing;
            if code == b' ' {
                advance += self.state.word_spacing;
            }
            advance *= self.state.horizontal_scale;
            self.state.text_matrix =
                Matrix::new(1.0, 0.0, 0.0, 1.0, advance, 0.0).multiply(self.state.text_matrix);
        }
        Ok(())
    }

    fn draw_image(&mut self, name: &[u8]) -> crate::Result<()> {
        let Some(resources) = self.resources.clone() else {
            self.report.note(b"Do");
            return Ok(());
        };
        let xobjects = match resolve(self.document, resources.get(b"XObject")) {
            Ok(Object::Dictionary(xobjects)) => xobjects,
            _ => {
                self.report.note(b"Do");
                return Ok(());
            }
        };
        let entry = match xobjects.get(name) {
            Some(entry) => entry.clone(),
            None => {
                self.report.note(b"Do");
                return Ok(());
            }
        };
        let object = resolve(self.document, Some(&entry))?;
        let Object::Stream(stream) = object else {
            self.report.note(b"Do");
            return Ok(());
        };
        if !matches!(stream.dict().get(b"Subtype"), Some(Object::Name(name)) if name == b"Image") {
            self.report.note(b"Do");
            return Ok(());
        }
        match Image::from_stream(self.document, &stream) {
            Ok(image) => {
                image.paint(self.canvas, &self.mask, self.state.ctm);
            }
            // An image we cannot decode is reported, not fatal: the rest of the
            // page still renders.
            Err(_) => self.report.note(b"Do"),
        }
        Ok(())
    }

    fn load_font(&mut self, name: &[u8]) -> (Option<Font>, Option<SimpleWidths>) {
        let Some((font, widths, unsupported)) = self.font_entry(name) else {
            return (None, None);
        };
        if let Some(kind) = unsupported {
            self.report.note(kind.as_bytes());
        }
        (font, widths)
    }

    /// The font dictionary behind a resource name, its widths and, when the
    /// family is not one this renderer draws, the name to report.
    fn font_entry(
        &mut self,
        name: &[u8],
    ) -> Option<(Option<Font>, Option<SimpleWidths>, Option<&'static str>)> {
        let resources = self.resources.clone()?;
        let fonts = match resolve(self.document, resources.get(b"Font")) {
            Ok(Object::Dictionary(fonts)) => fonts,
            _ => return None,
        };
        let entry = fonts.get(name)?.clone();
        let font_object = resolve(self.document, Some(&entry)).ok()?;
        let Object::Dictionary(font) = font_object else {
            return None;
        };
        let subtype = match font.get(b"Subtype") {
            Some(Object::Name(name)) => name.clone(),
            _ => Vec::new(),
        };
        // Type 0 (CID), Type 3 and embedded Type 1/CFF programs are not drawn
        // yet; report them by name instead of painting fallback glyphs as if
        // they were the document's font.
        match subtype.as_slice() {
            b"Type0" => return Some((None, None, Some("font:Type0"))),
            b"Type3" => return Some((None, None, Some("font:Type3"))),
            _ => {}
        }
        let encoding = self.encoding_for(&font);
        let widths = self.widths_for(&font);
        if let Some(descriptor) = font.get(b"FontDescriptor") {
            let descriptor = resolve(self.document, Some(descriptor)).ok()?;
            if let Object::Dictionary(descriptor) = descriptor {
                for (key, kind) in [
                    (&b"FontFile3"[..], "font:FontFile3"),
                    (&b"FontFile"[..], "font:FontFile"),
                ] {
                    if descriptor.get(key).is_some() {
                        return Some((Some(Font::builtin(encoding)), widths, Some(kind)));
                    }
                }
                if let Some(font_file) = descriptor.get(b"FontFile2") {
                    let data = match font_file {
                        Object::Reference(id) => self.document.stream_data(*id).ok(),
                        _ => None,
                    };
                    if let Some(data) = data {
                        if let Some(font) = Font::true_type(encoding.clone(), data) {
                            return Some((Some(font), widths, None));
                        }
                    }
                }
            }
        }
        Some((Some(Font::builtin(encoding)), widths, None))
    }

    /// `/Widths` plus the descriptor's `/MissingWidth`, when the font states
    /// them. A base-14 font usually does not, and then the glyph's own advance
    /// is the best available answer.
    fn widths_for(&self, font: &Dictionary) -> Option<SimpleWidths> {
        let first = font.get(b"FirstChar").and_then(Object::as_integer)?;
        let Object::Array(items) = resolve(self.document, font.get(b"Widths")).ok()? else {
            return None;
        };
        let widths: Vec<f64> = items
            .iter()
            .map(|item| {
                resolve(self.document, Some(item))
                    .ok()
                    .and_then(|item| item.as_real())
            })
            .map(|width| width.unwrap_or(0.0))
            .collect();
        let missing = font
            .get(b"FontDescriptor")
            .and_then(|descriptor| resolve(self.document, Some(descriptor)).ok())
            .and_then(|descriptor| match descriptor {
                Object::Dictionary(descriptor) => {
                    descriptor.get(b"MissingWidth").and_then(Object::as_real)
                }
                _ => None,
            })
            .unwrap_or(0.0);
        Some(SimpleWidths {
            first,
            widths,
            missing,
        })
    }

    fn encoding_for(&self, font: &Dictionary) -> Encoding {
        let Some(encoding_object) = font.get(b"Encoding") else {
            return Encoding::standard();
        };
        let Ok(encoding_object) = resolve(self.document, Some(encoding_object)) else {
            return Encoding::standard();
        };
        let mut encoding = Encoding::from_object(Some(&encoding_object)).unwrap_or_default();
        if let Object::Dictionary(dict) = &encoding_object {
            if let Some(differences) = dict.get(b"Differences") {
                if let Ok(Object::Array(items)) = resolve(self.document, Some(differences)) {
                    let _ = encoding.apply_differences(&items);
                }
            }
        }
        encoding
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

/// The `pair`th point in the operands, if all of its numbers are there.
///
/// Callers name a *point*, not a number: `c` takes three of them at slots 0, 1
/// and 2, which are the numbers `(0, 1)`, `(2, 3)` and `(4, 5)`. Reading the
/// slot as a number index silently turned every curve's last control point into
/// its end point, which is what a wrong index looks like on the page.
fn point_operand(operands: &[Object], pair: usize) -> Option<Point> {
    let start = pair * 2;
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
