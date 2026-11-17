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

use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
use crate::font::{Font, Glyph};
use crate::geom::{Matrix, Point};
use crate::image::{Image, resolve};
use crate::path::{Cap, Dash, Join, Path, dash_polyline, stroke_outline};
use crate::raster::{clip_mask, fill_polygons};

/// How deep form XObjects may nest before a further `/Do` is reported instead of
/// followed.
///
/// PDF lets a form draw another form (8.10), and nothing in the format forbids a
/// cycle. Eight levels is far past what real files use — a form inside a form
/// inside a form is already unusual — and mirrors [`pdf2::Document`]'s
/// `MAX_RESOLVE_DEPTH`: a named constant, a report, and a test that proves it
/// fires rather than a stack overflow.
const MAX_FORM_DEPTH: usize = 8;

/// How many pattern cells one fill may paint.
///
/// The clause sets no bound on the lattice ("Painting with the pattern replicates
/// the cell at fixed horizontal and vertical intervals"), so `/XStep 0.01` over a
/// page is millions of cells; the budget is the renderer's own, in
/// [`MAX_FORM_DEPTH`]'s shape: a named constant, a reported operator, a test.
const MAX_TILE_CELLS: usize = 4096;

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
    /// Operators that were recognised but not drawn, and the conditions that
    /// stopped a page drawing, with how often each appeared.
    pub unsupported: BTreeMap<String, usize>,
    /// Painting operators the walker executed: fills, strokes, text runs, images
    /// and shadings. Zero means the page had nothing to draw.
    pub painted: usize,
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

/// What a font resource yields, with the reason it is not drawable when it is
/// not.
#[derive(Debug, Clone, Default)]
struct LoadedFont {
    font: Option<Font>,
    widths: Option<SimpleWidths>,
    cid: Option<CidFont>,
    type3: Option<Type3Font>,
    note: Option<&'static str>,
}

/// A Type 3 font: each glyph is a content stream, not an outline (9.6.5).
#[derive(Debug, Clone)]
struct Type3Font {
    /// `/CharProcs`: glyph name → content stream.
    char_procs: pdf2::Dictionary,
    /// Code → glyph name, from `/Encoding`'s `/Differences` (12.5.5, 9.6.6.1).
    names: Vec<Option<Vec<u8>>>,
    /// `/FontMatrix`: glyph space to text space, default `0.001` scale.
    font_matrix: Matrix,
    /// The font's own `/Resources`, which its char procedures paint with.
    resources: Option<pdf2::Dictionary>,
}

/// What a `/Pattern` fill paints (8.7.3): a shading, or a tiling pattern's cell.
#[derive(Debug, Clone)]
enum PatternFill {
    /// `/PatternType 2`: a shading that fills the path.
    Shading(std::rc::Rc<crate::shading::Shading>),
    /// `/PatternType 1`: a cell replicated over the path, and — for an
    /// uncoloured (`/PaintType 2`) pattern — the colour given at use time, which
    /// the cell's stencil is poured with (`8.7.3.3`).
    Tile(std::rc::Rc<crate::pattern::Tile>, Option<Color>),
}

/// A composite (Type 0) font: two-byte codes, CIDs mapped to glyph ids through
/// `/CIDToGIDMap`, and advances from `/W` with `/DW` as the default (9.7.4).
#[derive(Debug, Clone)]
struct CidFont {
    font: Font,
    to_gid: CidToGid,
    widths: CidWidths,
}

#[derive(Debug, Clone)]
enum CidToGid {
    Identity,
    Table(Vec<u16>),
}

#[derive(Debug, Clone, Default)]
struct CidWidths {
    /// `(first CID, widths)` runs from `/W`, in 1/1000 em.
    ranges: Vec<(u32, Vec<f64>)>,
    /// `/DW`, which the standard sets to 1000 when absent.
    default: f64,
}

impl CidFont {
    fn glyph_id(&self, cid: u16) -> u16 {
        match &self.to_gid {
            CidToGid::Identity => cid,
            CidToGid::Table(table) => table.get(usize::from(cid)).copied().unwrap_or(0),
        }
    }

    fn advance(&self, cid: u16) -> f64 {
        for (start, widths) in &self.widths.ranges {
            let index = u32::from(cid).wrapping_sub(*start);
            if let Some(width) = widths.get(index as usize) {
                return width / 1000.0;
            }
        }
        self.widths.default / 1000.0
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
    // The visible box is `/CropBox` when the page states one (14.11.2): it
    // defines the raster and the origin, and the media box only bounds it.
    let media = page
        .crop_box
        .or(page.media_box)
        .unwrap_or(pdf2::Rect::new(0.0, 0.0, 612.0, 792.0));
    // `/UserUnit` scales default user space (7.7.3.4), so it multiplies the
    // device resolution rather than the page's coordinates.
    let scale = scale * document.page_user_unit(page).unwrap_or(1.0);
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
        form_depth: 0,
        stream_starts: Vec::new(),
        uncoloured_cell: false,
    };
    walker.run(&content)?;
    // Annotation appearance streams are drawn after the page content (12.5.5),
    // and are the reason a page with an empty `/Contents` can still show
    // something: 25 of the 49 silent-blank sample pages were exactly this.
    walker.draw_annotations(page)?;
    // A page that ran painting operators and produced an empty raster is not
    // "all operators supported" — it is a page that drew nothing, and a reader
    // deserves to know which. Silent blanks are the worst failure this renderer
    // has: 49 of 160 blank pages in a 300-file sample, measured 2026-11-15.
    if report.painted > 0 && report.unsupported.is_empty() && canvas.is_blank() {
        report.note(b"drew-nothing");
    }
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
    /// `/ca`: constant alpha for fills and text, 0..=1.
    fill_alpha: f64,
    /// `/CA`: constant alpha for strokes.
    stroke_alpha: f64,
    /// `/BM`: how a painted colour combines with what is already there.
    blend: BlendMode,
    /// The colour space `cs`/`CS` selected, when it is one this renderer can tint.
    space: Option<crate::shading::Space>,
    /// For a `/Pattern` colour space, the underlying space an uncoloured
    /// pattern's colour is given in (`8.7.3.3`).
    underlying: Option<crate::shading::Space>,
    /// What a `/Pattern` fill paints, when `scn` named one.
    pattern: Option<PatternFill>,
    font: Option<Font>,
    font_size: f64,
    miter_limit: f64,
    widths: Option<SimpleWidths>,
    cid: Option<CidFont>,
    type3: Option<Type3Font>,
    char_spacing: f64,
    word_spacing: f64,
    horizontal_scale: f64,
    leading: f64,
    rise: f64,
    /// `Tr`: 0 fill, 1 stroke, 2 both, 3 neither (invisible), 4–7 the same with
    /// clipping (9.3).
    text_mode: i64,
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
            fill_alpha: 1.0,
            stroke_alpha: 1.0,
            blend: BlendMode::Normal,
            space: None,
            underlying: None,
            pattern: None,
            font: None,
            font_size: 0.0,
            miter_limit: 10.0,
            widths: None,
            cid: None,
            type3: None,
            char_spacing: 0.0,
            word_spacing: 0.0,
            horizontal_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
            text_mode: 0,
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
    /// Form XObjects currently being drawn, for [`MAX_FORM_DEPTH`].
    form_depth: usize,
    /// The state each enclosing content stream started with, innermost last: a
    /// pattern cell is painted with the parent stream's *initial* state (8.7.3.1).
    stream_starts: Vec<State>,
    /// Whether the content being run is an uncoloured pattern's cell, whose
    /// operators "shall not explicitly specify any colours" (8.7.3.3).
    uncoloured_cell: bool,
}

impl Walker<'_> {
    fn run(&mut self, content: &[u8]) -> crate::Result<()> {
        self.stream_starts.push(self.state.clone());
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
        self.stream_starts.pop();
        Ok(())
    }

    fn execute(
        &mut self,
        operator: &[u8],
        operands: &mut [Object],
        parser: &mut Parser<'_>,
    ) -> crate::Result<()> {
        // Painting operators, counted so that a page which produced an empty
        // raster can be told apart from one that had nothing to draw.
        if matches!(
            operator,
            b"f" | b"F"
                | b"f*"
                | b"S"
                | b"s"
                | b"B"
                | b"B*"
                | b"b"
                | b"b*"
                | b"Tj"
                | b"TJ"
                | b"'"
                | b"\""
                | b"Do"
                | b"BI"
        ) {
            self.report.painted += 1;
        }
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
            b"gs" => {
                if let Some(Object::Name(name)) = operands.first() {
                    let name = name.clone();
                    self.apply_ext_gstate(&name)?;
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
            // An uncoloured pattern's cell "shall not explicitly specify any
            // colours" (8.7.3.3); the operator is reported and the stencil keeps the
            // poured colour, so the page names the defect instead of losing the tile.
            b"g" | b"G" | b"rg" | b"RG" | b"k" | b"K" | b"cs" | b"CS" | b"sc" | b"SC" | b"scn"
            | b"SCN"
                if self.uncoloured_cell =>
            {
                self.report.note(operator);
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
            // `d0`/`d1` declare a Type 3 glyph's width (and, for `d1`, its bbox)
            // in glyph space, which is what a Type 1 charstring's `setcachedevice`
            // does. The width a page advances by comes from `/Widths`, so these
            // are read and not acted on (9.6.5.2).
            b"d0" | b"d1" => {}
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
                let (font, widths, cid, type3) = self.load_font(&name);
                self.state.font = font;
                self.state.widths = widths;
                self.state.cid = cid;
                self.state.type3 = type3;
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
            b"Tr" => {
                let mode = number(operands, 0);
                let mode = if mode.is_finite() {
                    #[allow(clippy::cast_possible_truncation)]
                    let mode = mode as i64;
                    mode.clamp(0, 7)
                } else {
                    0
                };
                // Modes 4–7 add clipping, which the rasteriser does not model
                // yet; the fill and stroke halves are what a page shows.
                self.state.text_mode = mode;
            }
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
            b"cs" | b"CS" => {
                if let Some(Object::Name(name)) = operands.first() {
                    let name = name.clone();
                    self.select_colour_space(&name)?;
                }
            }
            b"sc" | b"SC" | b"scn" | b"SCN" => {
                let stroke = operator == b"SC" || operator == b"SCN";
                // `scn` names a pattern; for an uncoloured one the components come
                // first and the name last (`0.30 0.75 0.21 /P2 scn`, 8.7.3.3).
                if let Some(name) = operands.iter().find_map(|operand| match operand {
                    Object::Name(name) => Some(name.clone()),
                    _ => None,
                }) {
                    let values: Vec<f64> = operands.iter().filter_map(Object::as_real).collect();
                    self.select_pattern(&name, &values)?;
                } else if let Some(space) = self.state.space.clone() {
                    let values: Vec<f64> = operands.iter().filter_map(Object::as_real).collect();
                    let colour = crate::shading::color_in(&space, &values);
                    if stroke {
                        self.state.stroke = colour;
                    } else {
                        self.state.fill = colour;
                    }
                }
            }
            b"sh" => {
                if let Some(Object::Name(name)) = operands.first() {
                    let name = name.clone();
                    self.paint_shading(&name)?;
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
            if let Some(pattern) = self.state.pattern.clone() {
                // A pattern paints where the path fills: the path becomes the
                // clip and the pattern the paint (8.7.3).
                let mut mask = self.mask.clone();
                mask.intersect(&clip_mask(
                    self.canvas.width(),
                    self.canvas.height(),
                    &polylines,
                    even_odd,
                ));
                match &pattern {
                    PatternFill::Shading(shading) => {
                        let to_device = shading.matrix().multiply(ctm);
                        shading.with_matrix(to_device).paint_through(
                            self.canvas,
                            &mask,
                            self.state.fill_alpha,
                            self.state.blend,
                        );
                    }
                    PatternFill::Tile(tile, colour) => {
                        let (tile, colour) = (tile.clone(), *colour);
                        self.paint_tile(&tile, colour, &polylines, &mask, ctm)?;
                    }
                }
                self.path = Path::new();
                return Ok(());
            }
            fill_polygons(
                self.canvas,
                &self.mask,
                &polylines,
                self.state.fill,
                even_odd,
                self.state.fill_alpha,
                self.state.blend,
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
                    self.state.stroke_alpha,
                    self.state.blend,
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
        if let Some(cid) = self.state.cid.clone() {
            // Identity-H and friends: the string is a run of two-byte codes.
            for pair in bytes.as_chunks::<2>().0 {
                let code = u16::from_be_bytes(*pair);
                if let Some(glyph) = cid.font.glyph_by_id(cid.glyph_id(code)) {
                    self.place_glyph(&glyph);
                }
                let advance = cid.advance(code) * self.state.font_size + self.state.char_spacing;
                self.advance_text(advance * self.state.horizontal_scale);
            }
            return Ok(());
        }
        if let Some(type3) = self.state.type3.clone() {
            for &code in bytes {
                self.run_type3_glyph(&type3, code)?;
                let em = self
                    .state
                    .widths
                    .as_ref()
                    .map_or(0.0, |widths| widths.advance(code));
                let mut advance = em * self.state.font_size + self.state.char_spacing;
                if code == b' ' {
                    advance += self.state.word_spacing;
                }
                self.advance_text(advance * self.state.horizontal_scale);
            }
            return Ok(());
        }
        let Some(font) = self.state.font.clone() else {
            return Ok(());
        };
        for &code in bytes {
            if let Some(glyph) = font.glyph(code) {
                self.place_glyph(&glyph);
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
            self.advance_text(advance * self.state.horizontal_scale);
        }
        Ok(())
    }

    /// Fill one glyph outline at the current text position.
    fn place_glyph(&mut self, glyph: &Glyph) {
        if glyph.contours.is_empty() {
            return;
        }
        // `Tr` 3 and 7 draw nothing; 1, 5 stroke only; 2, 6 fill and stroke.
        let mode = self.state.text_mode;
        let invisible = matches!(mode, 3 | 7);
        if invisible {
            return;
        }
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
        let filled = matches!(mode, 0 | 2 | 4 | 6);
        if filled {
            fill_polygons(
                self.canvas,
                &self.mask,
                &polygons,
                self.state.fill,
                false,
                self.state.fill_alpha,
                self.state.blend,
            );
        }
        if matches!(mode, 1 | 2 | 5 | 6) {
            // A stroked glyph uses the text state's line width, cap and join.
            let mut outline = Vec::new();
            for polygon in &polygons {
                let mut closed = polygon.clone();
                if let Some(first) = closed.first().copied() {
                    closed.push(first);
                }
                outline.extend(stroke_outline(
                    &[closed],
                    self.state.line_width,
                    self.state.cap,
                    self.state.join,
                    self.state.miter_limit,
                ));
            }
            if !outline.is_empty() {
                fill_polygons(
                    self.canvas,
                    &self.mask,
                    &outline,
                    self.state.stroke,
                    false,
                    self.state.stroke_alpha,
                    self.state.blend,
                );
            }
        }
    }

    /// Move the text position by `advance` in unscaled text space.
    fn advance_text(&mut self, advance: f64) {
        self.state.text_matrix =
            Matrix::new(1.0, 0.0, 0.0, 1.0, advance, 0.0).multiply(self.state.text_matrix);
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
        let object_id = match &entry {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        let object = resolve(self.document, Some(&entry))?;
        let Object::Stream(stream) = object else {
            self.report.note(b"Do");
            return Ok(());
        };
        match stream.dict().get(b"Subtype").and_then(Object::as_name) {
            Some(b"Image") => {}
            Some(b"Form") => return self.run_form(&stream),
            _ => {
                self.report.note(b"Do");
                return Ok(());
            }
        }
        match Image::from_stream(self.document, &stream, object_id) {
            Ok(image) => {
                image.paint(
                    self.canvas,
                    &self.mask,
                    self.state.ctm,
                    self.state.fill_alpha,
                    self.state.blend,
                );
            }
            // An image we cannot decode is reported, not fatal: the rest of the
            // page still renders.
            Err(_) => self.report.note(b"Do"),
        }
        Ok(())
    }

    /// Run a `/Subtype /Form` XObject as a nested content stream (8.10).
    ///
    /// The form's `/Matrix` maps its space into the current one, its `/BBox`
    /// clips what it draws, and its `/Resources` replace the ones in force —
    /// falling back to them when the form has none, which is what the standard
    /// says and what most writers rely on. Depth is capped by
    /// [`MAX_FORM_DEPTH`] so a cycle is reported instead of followed.
    fn run_form(&mut self, stream: &pdf2::Stream) -> crate::Result<()> {
        self.run_form_nested(stream, Matrix::IDENTITY)
    }

    /// Paint a tiling pattern over the filled path (8.7.3.1).
    ///
    /// The path is already the clip. The lattice is computed in **pattern
    /// space** — the path's device box mapped back through the pattern matrix —
    /// and each cell is clipped to its own `/BBox` and run as nested content.
    /// The clause sets no bound on the lattice, so [`MAX_TILE_CELLS`] is the
    /// renderer's: over it the fill is reported instead of attempted.
    fn paint_tile(
        &mut self,
        tile: &crate::pattern::Tile,
        colour: Option<Color>,
        polylines: &[Vec<Point>],
        mask: &ClipMask,
        ctm: Matrix,
    ) -> crate::Result<()> {
        let to_device = tile.matrix.multiply(ctm);
        let Some(to_pattern) = to_device.invert() else {
            self.report.note(b"pattern");
            return Ok(());
        };
        // The path's device box, and the pattern-space box it covers.
        let (mut min_x, mut min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for point in polylines.iter().flatten() {
            min_x = min_x.min(point.x);
            min_y = min_y.min(point.y);
            max_x = max_x.max(point.x);
            max_y = max_y.max(point.y);
        }
        if !min_x.is_finite() || !min_y.is_finite() {
            return Ok(());
        }
        let (mut p_min_x, mut p_min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut p_max_x, mut p_max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for corner in [
            Point::new(min_x, min_y),
            Point::new(max_x, min_y),
            Point::new(max_x, max_y),
            Point::new(min_x, max_y),
        ] {
            let pattern = to_pattern.apply(corner);
            p_min_x = p_min_x.min(pattern.x);
            p_min_y = p_min_y.min(pattern.y);
            p_max_x = p_max_x.max(pattern.x);
            p_max_y = p_max_y.max(pattern.y);
        }
        // Cells sit at multiples of the step, with the key cell at the origin.
        #[allow(clippy::cast_possible_truncation)]
        let first_i = (p_min_x / tile.x_step).floor() as i64;
        #[allow(clippy::cast_possible_truncation)]
        let last_i = (p_max_x / tile.x_step).ceil() as i64;
        #[allow(clippy::cast_possible_truncation)]
        let first_j = (p_min_y / tile.y_step).floor() as i64;
        #[allow(clippy::cast_possible_truncation)]
        let last_j = (p_max_y / tile.y_step).ceil() as i64;
        let columns = last_i.saturating_sub(first_i).saturating_add(1);
        let rows = last_j.saturating_sub(first_j).saturating_add(1);
        let cells = columns.saturating_mul(rows);
        if cells <= 0 || usize::try_from(cells).unwrap_or(usize::MAX) > MAX_TILE_CELLS {
            self.report.note(b"pattern");
            return Ok(());
        }
        // The cell's own box, in pattern space, as a device quad per cell.
        let cell_corners = [
            Point::new(tile.bbox[0], tile.bbox[1]),
            Point::new(tile.bbox[2], tile.bbox[1]),
            Point::new(tile.bbox[2], tile.bbox[3]),
            Point::new(tile.bbox[0], tile.bbox[3]),
        ];
        for row in first_j..=last_j {
            for column in first_i..=last_i {
                #[allow(clippy::cast_precision_loss)]
                let offset = Matrix::new(
                    1.0,
                    0.0,
                    0.0,
                    1.0,
                    column as f64 * tile.x_step,
                    row as f64 * tile.y_step,
                );
                let cell = offset.multiply(to_device);
                let quad: Vec<Point> = cell_corners
                    .iter()
                    .map(|point| cell.apply(*point))
                    .collect();
                let mut cell_mask = mask.clone();
                cell_mask.intersect(&clip_mask(
                    self.canvas.width(),
                    self.canvas.height(),
                    &[quad],
                    false,
                ));
                if cell_mask.count() == 0 {
                    continue;
                }
                self.run_tile(tile, colour, cell, cell_mask)?;
            }
        }
        Ok(())
    }

    /// Run one pattern cell's content with its own state and resources.
    ///
    /// The clause says a cell is painted with "the graphics state that was in
    /// effect at the beginning of the pattern's parent content stream", so the
    /// base is the state this stream started with, and only the CTM is the
    /// pattern matrix composed with the one in force at fill time.
    fn run_tile(
        &mut self,
        tile: &crate::pattern::Tile,
        colour: Option<Color>,
        ctm: Matrix,
        mask: ClipMask,
    ) -> crate::Result<()> {
        if self.form_depth >= MAX_FORM_DEPTH {
            self.report.note(b"pattern");
            return Ok(());
        }
        let saved_state = self.state.clone();
        let saved_mask = core::mem::replace(&mut self.mask, mask);
        let saved_resources = self.resources.clone();
        let saved_path = core::mem::replace(&mut self.path, Path::new());

        let mut base = self
            .stream_starts
            .last()
            .cloned()
            .unwrap_or_else(|| self.state.clone());
        base.ctm = ctm;
        // An uncoloured cell is a stencil: it is poured with the use-time colour,
        // and its own colour operators are refused rather than obeyed (8.7.3.3).
        if let Some(colour) = colour {
            base.fill = colour;
            base.stroke = colour;
        }
        self.state = base;
        self.resources = tile.resources.clone();
        self.form_depth += 1;
        self.uncoloured_cell = colour.is_some();
        let result = self.run(&tile.content);
        self.uncoloured_cell = false;
        self.form_depth -= 1;

        self.state = saved_state;
        self.mask = saved_mask;
        self.resources = saved_resources;
        self.path = saved_path;
        result
    }

    /// Run a form with `outer` composed after its own `/Matrix`.
    ///
    /// An annotation appearance is placed by mapping its transformed `/BBox`
    /// onto the annotation's `/Rect` (12.5.5): the form's `/Matrix` runs first,
    /// the placement is a transform of the enclosing user space, so it is
    /// applied after — `Matrix::multiply` applies its receiver first.
    fn run_form_nested(&mut self, stream: &pdf2::Stream, outer: Matrix) -> crate::Result<()> {
        if self.form_depth >= MAX_FORM_DEPTH {
            self.report.note(b"Do");
            return Ok(());
        }
        let dict = stream.dict();
        let bbox = match dict.get(b"BBox").and_then(rect_of) {
            Some(bbox) => bbox,
            None => {
                // `/BBox` is required; without it there is nothing to clip to.
                self.report.note(b"Do");
                return Ok(());
            }
        };
        let matrix = dict
            .get(b"Matrix")
            .and_then(Object::as_array)
            .and_then(matrix_operand)
            .unwrap_or(Matrix::IDENTITY);
        let resources = match dict.get(b"Resources") {
            Some(value) => match resolve(self.document, Some(value))? {
                Object::Dictionary(dict) => Some(dict),
                _ => self.resources.clone(),
            },
            None => self.resources.clone(),
        };
        let content = match self.document.decode_stream(stream) {
            Ok(content) => content,
            Err(_) => {
                // A form we cannot decode is reported like any other undrawn `Do`.
                self.report.note(b"Do");
                return Ok(());
            }
        };

        let saved_state = self.state.clone();
        let saved_mask = self.mask.clone();
        let saved_resources = self.resources.clone();
        let saved_path = core::mem::replace(&mut self.path, Path::new());

        self.state.ctm = matrix.multiply(outer).multiply(self.state.ctm);
        let corners = [
            Point::new(bbox.x0, bbox.y0),
            Point::new(bbox.x1, bbox.y0),
            Point::new(bbox.x1, bbox.y1),
            Point::new(bbox.x0, bbox.y1),
        ]
        .map(|point| self.state.ctm.apply(point));
        self.mask.intersect(&clip_mask(
            self.canvas.width(),
            self.canvas.height(),
            &[corners.to_vec()],
            false,
        ));
        self.resources = resources;
        self.form_depth += 1;
        let result = self.run(&content);
        self.form_depth -= 1;

        self.state = saved_state;
        self.mask = saved_mask;
        self.resources = saved_resources;
        self.path = saved_path;
        result
    }

    /// Apply an `/ExtGState` dictionary (8.4.5).
    ///
    /// What is honoured: `/ca` and `/CA` (constant alpha), a `/BM` of `/Normal`,
    /// and the line state — `/LW`, `/LC`, `/LJ`, `/ML`, `/D`. What is not is
    /// reported as `gs` rather than ignored, because a page that blends or masks
    /// and says nothing would look correct and not be.
    fn apply_ext_gstate(&mut self, name: &[u8]) -> crate::Result<()> {
        let Some(resources) = self.resources.clone() else {
            self.report.note(b"gs");
            return Ok(());
        };
        let states = match resolve(self.document, resources.get(b"ExtGState")) {
            Ok(Object::Dictionary(states)) => states,
            _ => {
                self.report.note(b"gs");
                return Ok(());
            }
        };
        let Some(entry) = states.get(name).cloned() else {
            self.report.note(b"gs");
            return Ok(());
        };
        let Object::Dictionary(dict) = resolve(self.document, Some(&entry))? else {
            self.report.note(b"gs");
            return Ok(());
        };
        // A soft mask or a blend mode this renderer cannot honour keeps the note:
        // the state is only "supported" when everything it asks for is applied.
        // `/SMask /None` is how a writer says "no mask" — `cmykjpeg.pdf` carries
        // exactly that, so it is not a mask to report.
        if let Some(mask) = dict.get(b"SMask") {
            match resolve(self.document, Some(mask))? {
                Object::Name(name) if name == b"None" => {}
                Object::Dictionary(mask) => {
                    if !self.apply_soft_mask(&mask)? {
                        self.report.note(b"gs");
                        return Ok(());
                    }
                }
                _ => {
                    self.report.note(b"gs");
                    return Ok(());
                }
            }
        }
        if let Some(blend) = dict.get(b"BM") {
            // `/BM` may be a name or an array whose first entry is used.
            let name = match blend {
                Object::Name(name) => Some(name.clone()),
                Object::Array(items) => items.first().and_then(Object::as_name).map(<[u8]>::to_vec),
                _ => None,
            };
            match name.as_deref().and_then(BlendMode::from_name) {
                Some(mode) => self.state.blend = mode,
                None => {
                    self.report.note(b"gs");
                    return Ok(());
                }
            }
        }
        if let Some(alpha) = dict.get(b"ca") {
            self.state.fill_alpha = alpha.as_real().unwrap_or(1.0).clamp(0.0, 1.0);
        }
        if let Some(alpha) = dict.get(b"CA") {
            self.state.stroke_alpha = alpha.as_real().unwrap_or(1.0).clamp(0.0, 1.0);
        }
        if let Some(width) = dict.get(b"LW")
            && let Some(width) = width.as_real()
        {
            self.state.line_width = width;
        }
        if let Some(cap) = dict.get(b"LC").and_then(Object::as_integer) {
            self.state.cap = if cap == 1 {
                Cap::Round
            } else if cap == 2 {
                Cap::Square
            } else {
                Cap::Butt
            };
        }
        if let Some(join) = dict.get(b"LJ").and_then(Object::as_integer) {
            self.state.join = if join == 1 { Join::Round } else { Join::Miter };
        }
        if let Some(limit) = dict.get(b"ML").and_then(Object::as_real) {
            self.state.miter_limit = limit;
        }
        // `/D` is `[dashArray dashPhase]` (table 58), not a flat list.
        if let Some(Object::Array(items)) = dict.get(b"D")
            && let Some(Object::Array(pattern)) = items.first()
        {
            let pattern: Vec<f64> = pattern.iter().filter_map(|item| item.as_real()).collect();
            let phase = items.get(1).and_then(Object::as_real).unwrap_or(0.0);
            self.state.dash = Dash { pattern, phase };
        }
        Ok(())
    }

    /// Apply an `/SMask` entry (11.6.5.2) by multiplying its luminosity into the
    /// current coverage mask.
    ///
    /// The mask's `/G` form is rendered onto **black**, which is the backdrop the
    /// standard specifies: unpainted areas then have luminance 0 and hide
    /// everything, white shows everything, and greys fade proportionally. Only
    /// `/S /Luminosity` is honoured; an `/Alpha` mask needs a transparent canvas,
    /// which this rasteriser does not have, so it keeps the `gs` note instead of
    /// being approximated by a different definition of alpha.
    fn apply_soft_mask(&mut self, mask: &Dictionary) -> crate::Result<bool> {
        let kind = mask
            .get(b"S")
            .and_then(Object::as_name)
            .unwrap_or(b"Luminosity")
            .to_vec();
        if kind != b"Luminosity" {
            return Ok(false);
        }
        let Some(group) = mask.get(b"G") else {
            return Ok(false);
        };
        let Object::Stream(stream) = resolve(self.document, Some(group))? else {
            return Ok(false);
        };
        let content = match self.document.decode_stream(&stream) {
            Ok(content) => content,
            Err(_) => return Ok(false),
        };
        let resources = match stream.dict().get(b"Resources") {
            Some(value) => match resolve(self.document, Some(value))? {
                Object::Dictionary(dict) => Some(dict),
                _ => self.resources.clone(),
            },
            None => self.resources.clone(),
        };
        let mut backdrop = Canvas::new(
            self.canvas.width(),
            self.canvas.height(),
            Color::rgb(0.0, 0.0, 0.0),
        );
        let mut group_report = Report::default();
        let mut walker = Walker {
            canvas: &mut backdrop,
            report: &mut group_report,
            document: self.document,
            resources,
            state: State {
                ctm: self.state.ctm,
                // The mask group starts from a clean graphics state.
                ..State::new(self.state.ctm)
            },
            stack: Vec::new(),
            path: Path::new(),
            pending_clip: None,
            mask: ClipMask::all(self.canvas.width(), self.canvas.height()),
            form_depth: self.form_depth,
            stream_starts: Vec::new(),
            uncoloured_cell: false,
        };
        walker.run(&content)?;
        // A `Do` inside the mask group is the page's business too: merging the
        // counts keeps the report honest about what was not drawn.
        for (name, count) in &group_report.unsupported {
            for _ in 0..*count {
                self.report.note(name.as_bytes());
            }
        }
        self.mask.multiply_luminosity(&backdrop);
        Ok(true)
    }

    /// `cs`/`CS`: select a colour space from the page's `/ColorSpace` resources.
    ///
    /// Only the spaces this renderer can tint are kept — a tint space
    /// (`/Separation`, `/DeviceN`) or a named device space; a `/Pattern` space is
    /// remembered as "no tint", because the next `scn` names the pattern instead.
    fn select_colour_space(&mut self, name: &[u8]) -> crate::Result<()> {
        self.state.pattern = None;
        let Some(resources) = self.resources.clone() else {
            return Ok(());
        };
        let Some(entry) = resources.get(b"ColorSpace").cloned() else {
            return Ok(());
        };
        let Object::Dictionary(spaces) = resolve(self.document, Some(&entry))? else {
            return Ok(());
        };
        let Some(value) = spaces.get(name).cloned() else {
            return Ok(());
        };
        // A `/Pattern` colour space may be an array `[/Pattern /DeviceRGB]` whose
        // second element is the underlying space an uncoloured pattern's colour is
        // given in (8.7.3.3); anything else here is a tint space `scn` converts.
        self.state.underlying = match &value {
            Object::Array(items) => items
                .get(1)
                .and_then(|item| crate::shading::parse_space(self.document, item).ok()),
            _ => None,
        };
        self.state.space = crate::shading::parse_space(self.document, &value).ok();
        Ok(())
    }

    /// `scn` naming a pattern: a shading becomes the fill's paint, and a tiling
    /// pattern becomes a cell — with the colour `values` pours through it when it
    /// is uncoloured (`8.7.3.3`).
    fn select_pattern(&mut self, name: &[u8], values: &[f64]) -> crate::Result<()> {
        let Some(resources) = self.resources.clone() else {
            return Ok(());
        };
        let Some(entry) = resources.get(b"Pattern").cloned() else {
            return Ok(());
        };
        let Object::Dictionary(patterns) = resolve(self.document, Some(&entry))? else {
            return Ok(());
        };
        let Some(pattern) = patterns.get(name).cloned() else {
            return Ok(());
        };
        // A pattern object is a stream (its content paints the cell or nothing);
        // the type lives in its dictionary.
        let object = resolve(self.document, Some(&pattern))?;
        let dict = match &object {
            Object::Stream(stream) => stream.dict().clone(),
            Object::Dictionary(dict) => dict.clone(),
            _ => {
                self.report.note(b"pattern");
                return Ok(());
            }
        };
        match dict.get(b"PatternType").and_then(Object::as_integer) {
            Some(2) => {
                let Some(shading) = dict.get(b"Shading") else {
                    self.report.note(b"pattern");
                    return Ok(());
                };
                if let Ok(shading) = crate::shading::Shading::from_object(self.document, shading) {
                    self.state.pattern = Some(PatternFill::Shading(std::rc::Rc::new(shading)));
                } else {
                    self.report.note(b"pattern");
                }
            }
            // A tiling pattern (`8.7.3`): a coloured cell paints its own content,
            // an uncoloured one is a stencil poured with the colour `scn` gave —
            // "the entire pattern cell is painted with a separately specified
            // colour each time the pattern is used" (8.7.3.1).
            Some(1) => match &object {
                Object::Stream(stream) => {
                    match crate::pattern::Tile::from_stream(self.document, stream) {
                        Ok(tile) => {
                            let colour = match tile.paint_type {
                                crate::pattern::PaintType::Coloured => {
                                    // "For other types of patterns, these operands shall
                                    // not be specified" (8.6.8): extra ones are reported,
                                    // not silently ignored.
                                    if !values.is_empty() {
                                        self.report.note(b"scn");
                                    }
                                    None
                                }
                                crate::pattern::PaintType::Uncoloured => {
                                    let Some(space) = self.state.underlying.clone() else {
                                        // Without `[/Pattern /Device]` there is no space
                                        // to read the colour in.
                                        self.report.note(b"pattern");
                                        return Ok(());
                                    };
                                    Some(crate::shading::color_in(&space, values))
                                }
                            };
                            self.state.pattern =
                                Some(PatternFill::Tile(std::rc::Rc::new(tile), colour));
                        }
                        Err(_) => self.report.note(b"pattern"),
                    }
                }
                _ => self.report.note(b"pattern"),
            },
            _ => self.report.note(b"pattern"),
        }
        Ok(())
    }

    /// `sh`: paint a `/Shading` resource (8.7.4.3).
    ///
    /// The shading is sampled per pixel inside the current clip and its own
    /// bounding box, with the current fill alpha and blend mode — the same
    /// treatment a fill gets, which is what keeps a gradient under `/ca` and
    /// inside a `/BM` looking like the rest of the page.
    fn paint_shading(&mut self, name: &[u8]) -> crate::Result<()> {
        let Some(resources) = self.resources.clone() else {
            self.report.note(b"sh");
            return Ok(());
        };
        let Object::Dictionary(shadings) = resolve(self.document, resources.get(b"Shading"))?
        else {
            self.report.note(b"sh");
            return Ok(());
        };
        let Some(entry) = shadings.get(name).cloned() else {
            self.report.note(b"sh");
            return Ok(());
        };
        let Object::Dictionary(dict) = resolve(self.document, Some(&entry))? else {
            self.report.note(b"sh");
            return Ok(());
        };
        let shading = match crate::shading::Shading::parse(self.document, &dict) {
            Ok(shading) => shading,
            Err(_) => {
                self.report.note(b"sh");
                return Ok(());
            }
        };
        // The shading's own space is mapped into the page by its `/Matrix` and
        // then the CTM; painting goes through the inverse of that product.
        let to_device = shading.matrix().multiply(self.state.ctm);
        crate::shading::paint(
            &shading.with_matrix(to_device),
            self.canvas,
            &self.mask,
            self.state.fill_alpha,
            self.state.blend,
        );
        Ok(())
    }

    /// Draw every annotation that has an appearance stream (12.5.5).
    ///
    /// `/Rect` is the area the annotation occupies in page user space;
    /// `/AP /N` is the normal appearance, either a form stream or a dictionary
    /// of them keyed by `/AS`. Annotations flagged invisible, hidden or
    /// no-view are skipped, which is what a viewer does with them.
    fn draw_annotations(&mut self, page: &Page) -> crate::Result<()> {
        let Some(id) = page.id else {
            return Ok(());
        };
        let annotations = match self.document.get_object(id)? {
            Object::Dictionary(dict) => match dict.get(b"Annots") {
                Some(value) => resolve(self.document, Some(value))?,
                None => return Ok(()),
            },
            _ => return Ok(()),
        };
        let Object::Array(annotations) = annotations else {
            return Ok(());
        };
        for annotation in annotations {
            let annotation = resolve(self.document, Some(&annotation))?;
            let Object::Dictionary(dict) = annotation else {
                continue;
            };
            if annotation_is_hidden(&dict) {
                continue;
            }
            let Some(rect) = dict.get(b"Rect").and_then(rect_of) else {
                continue;
            };
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                continue;
            }
            let Some(appearance) = self.normal_appearance(&dict)? else {
                continue;
            };
            let Some(bbox) = appearance.dict().get(b"BBox").and_then(rect_of) else {
                continue;
            };
            let matrix = appearance
                .dict()
                .get(b"Matrix")
                .and_then(Object::as_array)
                .and_then(matrix_operand)
                .unwrap_or(Matrix::IDENTITY);
            // The appearance's box, after its own matrix, is what gets mapped
            // onto `/Rect`.
            let corners = [
                Point::new(bbox.x0, bbox.y0),
                Point::new(bbox.x1, bbox.y0),
                Point::new(bbox.x1, bbox.y1),
                Point::new(bbox.x0, bbox.y1),
            ]
            .map(|point| matrix.apply(point));
            let (min_x, max_x) = corners
                .iter()
                .fold((f64::MAX, f64::MIN), |(low, high), point| {
                    (low.min(point.x), high.max(point.x))
                });
            let (min_y, max_y) = corners
                .iter()
                .fold((f64::MAX, f64::MIN), |(low, high), point| {
                    (low.min(point.y), high.max(point.y))
                });
            let width = max_x - min_x;
            let height = max_y - min_y;
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            let scale_x = rect.width() / width;
            let scale_y = rect.height() / height;
            let place = Matrix::new(
                scale_x,
                0.0,
                0.0,
                scale_y,
                rect.x0 - min_x * scale_x,
                rect.y0 - min_y * scale_y,
            );
            self.run_form_nested(&appearance, place)?;
        }
        Ok(())
    }

    /// The annotation's normal appearance: a form stream, or the `/AS` entry of
    /// a dictionary of them. `None` when there is nothing to draw.
    fn normal_appearance(&self, dict: &Dictionary) -> crate::Result<Option<pdf2::Stream>> {
        let Some(ap) = dict.get(b"AP") else {
            return Ok(None);
        };
        let Object::Dictionary(ap) = resolve(self.document, Some(ap))? else {
            return Ok(None);
        };
        let Some(normal) = ap.get(b"N") else {
            return Ok(None);
        };
        let normal = resolve(self.document, Some(normal))?;
        match normal {
            Object::Stream(stream) => Ok(Some(stream)),
            Object::Dictionary(states) => {
                // `/AS` names the state a viewer shows; without it the first
                // entry is the only thing to go on.
                let state = dict
                    .get(b"AS")
                    .and_then(Object::as_name)
                    .and_then(|name| states.get(name))
                    .cloned()
                    .or_else(|| states.iter().next().map(|(_, value)| value.clone()));
                match state {
                    Some(value) => match resolve(self.document, Some(&value))? {
                        Object::Stream(stream) => Ok(Some(stream)),
                        _ => Ok(None),
                    },
                    None => Ok(None),
                }
            }
            _ => Ok(None),
        }
    }

    #[allow(clippy::type_complexity)]
    fn load_font(
        &mut self,
        name: &[u8],
    ) -> (
        Option<Font>,
        Option<SimpleWidths>,
        Option<CidFont>,
        Option<Type3Font>,
    ) {
        let Some(loaded) = self.font_entry(name) else {
            return (None, None, None, None);
        };
        if let Some(note) = loaded.note {
            self.report.note(note.as_bytes());
        }
        (loaded.font, loaded.widths, loaded.cid, loaded.type3)
    }

    /// Everything the walker needs from a font resource, with a note when the
    /// family is one this renderer cannot draw.
    fn font_entry(&mut self, name: &[u8]) -> Option<LoadedFont> {
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
        match subtype.as_slice() {
            b"Type0" => return Some(self.cid_entry(&font)),
            b"Type3" => return Some(self.type3_entry(&font)),
            _ => {}
        }
        let encoding = self.encoding_for(&font);
        let widths = self.widths_for(&font);
        if let Some(descriptor) = font.get(b"FontDescriptor") {
            let descriptor = resolve(self.document, Some(descriptor)).ok()?;
            if let Object::Dictionary(descriptor) = descriptor {
                // A simple CFF font (`/FontFile3`, `/Subtype /Type1C`) carries no
                // `cmap`: its codes reach glyphs through the charset's names, so
                // it draws once the name map is read. Other `/FontFile3`
                // subtypes (OpenType-wrapped CFF is handled too) and Type 1
                // programs (`/FontFile`, `eexec`) are still the fallback font.
                if let Some(Object::Reference(id)) = descriptor.get(b"FontFile3")
                    && let Some(font) = self
                        .document
                        .stream_data(*id)
                        .ok()
                        .and_then(|data| Font::cff(encoding.clone(), data))
                    && font.has_glyph_names()
                {
                    return Some(LoadedFont {
                        font: Some(font),
                        widths,
                        cid: None,
                        type3: None,
                        note: None,
                    });
                }
                // A Type 1 program (`/FontFile`) is a PostScript program with
                // charstrings, which `ttf-parser` cannot read: `crate::type1` does,
                // and a simple font's code reaches its glyphs by the *names* the
                // encoding declares.
                if let Some(Object::Reference(id)) = descriptor.get(b"FontFile")
                    && let Some(font) = self
                        .document
                        .stream_data(*id)
                        .ok()
                        .and_then(|data| crate::type1::Program::parse(&data))
                        .and_then(|program| Font::type1(encoding.clone(), program))
                {
                    return Some(LoadedFont {
                        font: Some(font),
                        widths,
                        cid: None,
                        type3: None,
                        note: None,
                    });
                }
                for (key, kind) in [
                    (&b"FontFile3"[..], "font:FontFile3"),
                    (&b"FontFile"[..], "font:FontFile"),
                ] {
                    if descriptor.get(key).is_some() {
                        return Some(LoadedFont {
                            font: Some(Font::builtin(encoding)),
                            widths,
                            cid: None,
                            type3: None,
                            note: Some(kind),
                        });
                    }
                }
                if let Some(font_file) = descriptor.get(b"FontFile2") {
                    let data = match font_file {
                        Object::Reference(id) => self.document.stream_data(*id).ok(),
                        _ => None,
                    };
                    if let Some(data) = data
                        && let Some(font) = Font::true_type(encoding.clone(), data)
                    {
                        // A program with no `cmap` cannot be decoded from its
                        // codes; say so here, where the font is loaded, so the
                        // page reports it once rather than looking blank.
                        let note = if font.has_character_map() {
                            None
                        } else {
                            Some("font:TrueType-without-cmap")
                        };
                        return Some(LoadedFont {
                            font: Some(font),
                            widths,
                            cid: None,
                            type3: None,
                            note,
                        });
                    }
                }
            }
        }
        Some(LoadedFont {
            font: Some(Font::builtin(encoding)),
            widths,
            cid: None,
            type3: None,
            note: None,
        })
    }

    /// A Type 0 font: a descendant CIDFontType2 with an embedded program and an
    /// identity CMap is drawable; anything else is reported by reason.
    /// A Type 3 font: a `/CharProcs` dictionary of content streams, selected by
    /// the glyph names its `/Encoding` declares (9.6.5).
    ///
    /// Without `/Differences` there are no names to select by, and the note says
    /// so rather than drawing nothing.
    fn type3_entry(&mut self, font: &Dictionary) -> LoadedFont {
        let char_procs = match resolve(self.document, font.get(b"CharProcs")) {
            Ok(Object::Dictionary(procs)) => procs,
            _ => {
                return LoadedFont {
                    note: Some("font:Type3"),
                    ..LoadedFont::default()
                };
            }
        };
        let Some(names) = self.type3_names(font) else {
            return LoadedFont {
                note: Some("font:Type3"),
                ..LoadedFont::default()
            };
        };
        let font_matrix = font
            .get(b"FontMatrix")
            .and_then(Object::as_array)
            .and_then(matrix_operand)
            .unwrap_or(Matrix::new(0.001, 0.0, 0.0, 0.001, 0.0, 0.0));
        let resources = match resolve(self.document, font.get(b"Resources")) {
            Ok(Object::Dictionary(resources)) => Some(resources),
            _ => self.resources.clone(),
        };
        let widths = self.widths_for(font);
        LoadedFont {
            type3: Some(Type3Font {
                char_procs,
                names,
                font_matrix,
                resources,
            }),
            widths,
            ..LoadedFont::default()
        }
    }

    /// Code → glyph name from `/Encoding`'s `/Differences`, 256 entries.
    fn type3_names(&mut self, font: &Dictionary) -> Option<Vec<Option<Vec<u8>>>> {
        let Ok(encoding) = resolve(self.document, font.get(b"Encoding")) else {
            return None;
        };
        let Object::Dictionary(encoding) = encoding else {
            // A named base encoding alone gives no glyph names this renderer has.
            return None;
        };
        let Ok(Object::Array(differences)) = resolve(self.document, encoding.get(b"Differences"))
        else {
            return None;
        };
        let mut names = vec![None; 256];
        let mut code: Option<usize> = None;
        for entry in &differences {
            match entry {
                Object::Integer(value) => {
                    code = usize::try_from(*value).ok().filter(|value| *value < 256);
                }
                Object::Name(name) => {
                    if let Some(slot) = code.and_then(|at| names.get_mut(at)) {
                        *slot = Some(name.clone());
                    }
                    code = code.and_then(|at| at.checked_add(1)).filter(|at| *at < 256);
                }
                _ => {}
            }
        }
        Some(names)
    }

    /// Run one Type 3 glyph's content stream in glyph space (9.6.5).
    ///
    /// The char procedure paints in glyph space, so it is placed by
    /// `/FontMatrix`, then the text rendering matrix — which is why the font
    /// matrix is the *first* factor. The glyph's own state is isolated: colour,
    /// width and clip a char procedure sets belong to that glyph.
    fn run_type3_glyph(&mut self, type3: &Type3Font, code: u8) -> crate::Result<()> {
        let Some(name) = type3.names.get(usize::from(code)).and_then(Option::as_ref) else {
            return Ok(());
        };
        // A char procedure with no glyph name in `/CharProcs` draws nothing.
        let Some(entry) = type3.char_procs.get(name).cloned() else {
            return Ok(());
        };
        let Object::Reference(id) = entry else {
            return Ok(());
        };
        let Ok(stream) = self.document.get_object(id) else {
            return Ok(());
        };
        let Object::Stream(stream) = stream else {
            return Ok(());
        };
        let content = match self.document.decode_stream(&stream) {
            Ok(content) => content,
            Err(_) => {
                self.report.note(b"Tj");
                return Ok(());
            }
        };
        if self.form_depth >= MAX_FORM_DEPTH {
            // `ContentStreamCycleType3insideType3.pdf` is exactly this: a glyph
            // whose procedure shows more text in the same font.
            self.report.note(b"Tj");
            return Ok(());
        }
        // Text rendering mode 3 and 7 paint nothing (9.3).
        if matches!(self.state.text_mode, 3 | 7) {
            return Ok(());
        }

        let scale = Matrix::new(
            self.state.font_size * self.state.horizontal_scale,
            0.0,
            0.0,
            self.state.font_size,
            0.0,
            self.state.rise,
        );
        let glyph_ctm = type3
            .font_matrix
            .multiply(scale)
            .multiply(self.state.text_matrix)
            .multiply(self.state.ctm);

        let saved_state = self.state.clone();
        let saved_mask = self.mask.clone();
        let saved_resources = self.resources.clone();
        let saved_path = core::mem::replace(&mut self.path, Path::new());

        self.state.ctm = glyph_ctm;
        // The procedure's own text runs in *its* scope, not in this font's:
        // `d0`/`d1` and any `Tf` inside belong to the glyph.
        self.state.font = None;
        self.state.type3 = None;
        self.state.cid = None;
        self.state.widths = None;
        self.resources = type3.resources.clone();
        self.form_depth += 1;
        let result = self.run(&content);
        self.form_depth -= 1;

        self.state = saved_state;
        self.mask = saved_mask;
        self.resources = saved_resources;
        self.path = saved_path;
        result
    }

    fn cid_entry(&mut self, font: &Dictionary) -> LoadedFont {
        let identity = matches!(
            resolve(self.document, font.get(b"Encoding")).ok(),
            Some(Object::Name(name)) if name.starts_with(b"Identity")
        );
        if !identity {
            return LoadedFont {
                note: Some("font:Type0-cmap"),
                ..LoadedFont::default()
            };
        }
        let descendant = match resolve(self.document, font.get(b"DescendantFonts")).ok() {
            Some(Object::Array(items)) => items
                .first()
                .and_then(|item| resolve(self.document, Some(item)).ok()),
            _ => None,
        };
        let Some(Object::Dictionary(descendant)) = descendant else {
            return LoadedFont {
                note: Some("font:Type0-descendant"),
                ..LoadedFont::default()
            };
        };
        let subtype = match descendant.get(b"Subtype") {
            Some(Object::Name(name)) => name.clone(),
            _ => Vec::new(),
        };
        // CIDFontType2 is TrueType, CIDFontType0 is CFF; both draw once the
        // program is wrapped, so the only difference here is which descriptor
        // entry carries it.
        if subtype != b"CIDFontType2" && subtype != b"CIDFontType0" {
            return LoadedFont {
                note: Some("font:CIDFontType0-unknown"),
                ..LoadedFont::default()
            };
        }
        let is_cff = subtype == b"CIDFontType0";
        let program = descendant
            .get(b"FontDescriptor")
            .and_then(|descriptor| resolve(self.document, Some(descriptor)).ok())
            .and_then(|descriptor| match descriptor {
                Object::Dictionary(descriptor) => {
                    let entry: &[u8] = if is_cff { b"FontFile3" } else { b"FontFile2" };
                    descriptor.get(entry).and_then(|file| match file {
                        Object::Reference(id) => self.document.stream_data(*id).ok(),
                        _ => None,
                    })
                }
                _ => None,
            })
            .and_then(|data| {
                if is_cff {
                    // CFF arrives bare (`/CIDFontType0C`) or wrapped (`/OpenType`);
                    // `Font::cff` accepts both.
                    Font::cff(Encoding::standard(), data)
                } else {
                    Font::true_type(Encoding::standard(), data)
                }
            });
        let Some(program) = program else {
            return LoadedFont {
                note: Some("font:Type0-no-program"),
                ..LoadedFont::default()
            };
        };
        // Read the entry unresolved: resolving it would dereference the stream
        // reference and lose the id the bytes are behind.
        let to_gid = match descendant.get(b"CIDToGIDMap") {
            Some(Object::Name(name)) if name == b"Identity" => CidToGid::Identity,
            Some(Object::Reference(id)) => self
                .document
                .stream_data(*id)
                .map(|data| {
                    CidToGid::Table(
                        data.as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| u16::from_be_bytes(*pair))
                            .collect(),
                    )
                })
                .unwrap_or(CidToGid::Identity),
            // Absent means identity.
            _ => CidToGid::Identity,
        };
        LoadedFont {
            font: None,
            widths: None,
            cid: Some(CidFont {
                font: program,
                to_gid,
                widths: self.cid_widths(&descendant),
            }),
            type3: None,
            note: None,
        }
    }

    /// `/W` runs and `/DW` from a CID font dictionary (9.7.4.3).
    fn cid_widths(&self, descendant: &Dictionary) -> CidWidths {
        let mut ranges = vec![];
        if let Ok(Object::Array(items)) = resolve(self.document, descendant.get(b"W")) {
            let number = |item: Option<&Object>| {
                item.and_then(|item| resolve(self.document, Some(item)).ok())
                    .and_then(|item| item.as_real())
            };
            let mut index = 0;
            while index + 1 < items.len() {
                let Some(first) = number(items.get(index)) else {
                    break;
                };
                match items.get(index + 1) {
                    Some(Object::Array(list)) => {
                        let widths = list
                            .iter()
                            .map(Object::as_real)
                            .map(|w| w.unwrap_or(0.0))
                            .collect();
                        ranges.push((first as u32, widths));
                        index += 2;
                    }
                    second => {
                        let Some(last) = number(second) else { break };
                        let Some(width) = number(items.get(index + 2)) else {
                            break;
                        };
                        let count = (last - first).max(0.0) as usize + 1;
                        ranges.push((first as u32, vec![width; count]));
                        index += 3;
                    }
                }
            }
        }
        let default = number_of(self.document, descendant.get(b"DW")).unwrap_or(1000.0);
        CidWidths { ranges, default }
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
            .map(|item| number_of(self.document, Some(item)).unwrap_or(0.0))
            .collect();
        let missing = font
            .get(b"FontDescriptor")
            .and_then(|descriptor| resolve(self.document, Some(descriptor)).ok())
            .and_then(|descriptor| match descriptor {
                Object::Dictionary(descriptor) => {
                    number_of(self.document, descriptor.get(b"MissingWidth"))
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
        if let Object::Dictionary(dict) = &encoding_object
            && let Some(differences) = dict.get(b"Differences")
            && let Ok(Object::Array(items)) = resolve(self.document, Some(differences))
        {
            let _ = encoding.apply_differences(&items);
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

fn number_of(document: &Document, object: Option<&Object>) -> Option<f64> {
    resolve(document, object)
        .ok()
        .and_then(|object| object.as_real())
}

/// Whether an annotation's `/F` flags say a viewer hides it (12.5.3, table 165):
/// invisible (bit 1), hidden (bit 2) and no-view (bit 6).
fn annotation_is_hidden(dict: &Dictionary) -> bool {
    let flags = dict
        .get(b"F")
        .and_then(Object::as_integer)
        .unwrap_or_default();
    flags & 0b100011 != 0
}

/// A `/BBox` (or any four-number rectangle) as a [`pdf2::Rect`].
fn rect_of(object: &Object) -> Option<pdf2::Rect> {
    let items = object.as_array()?;
    if items.len() < 4 {
        return None;
    }
    let value = |index: usize| items.get(index).and_then(|item| item.as_real());
    let (x0, y0, x1, y1) = (value(0)?, value(1)?, value(2)?, value(3)?);
    Some(pdf2::Rect::new(
        x0.min(x1),
        y0.min(y1),
        x0.max(x1),
        y0.max(y1),
    ))
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
    use super::{Report, dash_operand};
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
