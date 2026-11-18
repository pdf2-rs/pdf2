//! WebAssembly bindings for the `pdf2` crate.
//!
//! The wrapper is deliberately thin: it exposes a document object with the
//! facts a viewer needs (page count, page size, extracted text, metadata) and
//! two ways to rasterise a page — pixel data for a canvas you own, or straight
//! onto an `HtmlCanvasElement`.
//!
//! Every error becomes a JavaScript exception with the library's message; a
//! malformed file never traps the module.

use js_sys::Uint8ClampedArray;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

use pdf2::Document as Pdf2Document;
use pdf2_render::{RenderOptions, render_page_number};

/// The renderer's report as a JSON object of `"reason": count`.
fn report_json(report: &pdf2_render::Report) -> String {
    let entries: Vec<String> = report
        .unsupported
        .iter()
        .map(|(reason, count)| {
            let escaped = reason.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\":{count}")
        })
        .collect();
    format!("{{{}}}", entries.join(","))
}

/// An open PDF document.
#[wasm_bindgen]
pub struct PdfDocument {
    document: Pdf2Document,
}

/// Open a PDF from bytes, with an optional password.
///
/// ```js
/// import init, { open } from "./pdf2_wasm.js";
/// await init();
/// const doc = open(bytes, null);
/// ```
#[wasm_bindgen(js_name = open)]
pub fn open(bytes: &[u8], password: Option<String>) -> Result<PdfDocument, JsError> {
    PdfDocument::from_bytes(bytes, password)
}

#[wasm_bindgen]
impl PdfDocument {
    /// Open a PDF from bytes; `new PdfDocument(bytes)` and
    /// `new PdfDocument(bytes, password)` both work.
    #[wasm_bindgen(constructor)]
    pub fn from_bytes(bytes: &[u8], password: Option<String>) -> Result<PdfDocument, JsError> {
        let password = password.map_or_else(Vec::new, String::into_bytes);
        Pdf2Document::open_with_password(bytes, &password)
            .map(|document| Self { document })
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// The version from the header, e.g. `"2.0"`.
    #[wasm_bindgen(js_name = version)]
    pub fn version(&self) -> String {
        self.document.version().to_owned()
    }

    /// Whether the document was opened with a security handler.
    #[wasm_bindgen(js_name = isEncrypted)]
    pub fn is_encrypted(&self) -> bool {
        self.document.is_encrypted()
    }

    /// Number of pages.
    #[wasm_bindgen(js_name = pageCount)]
    pub fn page_count(&self) -> Result<usize, JsError> {
        self.document
            .page_count()
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// `[width, height]` of a page in PDF points, counting from 0.
    #[wasm_bindgen(js_name = pageSize)]
    pub fn page_size(&self, index: usize) -> Result<Vec<f64>, JsError> {
        let page = self
            .document
            .page(index)
            .map_err(|error| JsError::new(&error.to_string()))?;
        let rect = page
            .media_box
            .ok_or_else(|| JsError::new("page has no media box"))?;
        Ok(vec![rect.width(), rect.height()])
    }

    /// Extracted text of a page.
    #[wasm_bindgen(js_name = text)]
    pub fn text(&self, index: usize) -> Result<String, JsError> {
        let page = self
            .document
            .page(index)
            .map_err(|error| JsError::new(&error.to_string()))?;
        self.document
            .extract_text(&page)
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// The page's lines as page-space boxes, four numbers per span in reading
    /// order: `[x0, y0, x1, y1, …]`.
    ///
    /// A `Float64Array` rather than a JavaScript type of its own: the text layer
    /// over the canvas wants numbers, and the engine costs kilobytes in the wasm
    /// bundle. `spanTexts(index)` returns the parallel strings.
    #[wasm_bindgen(js_name = spans)]
    pub fn spans(&self, index: usize) -> Result<Vec<f64>, JsError> {
        let spans = self.spans_of(index)?;
        let mut out = Vec::with_capacity(spans.len() * 4);
        for span in spans {
            out.extend_from_slice(&[
                span.bounds.x0,
                span.bounds.y0,
                span.bounds.x1,
                span.bounds.y1,
            ]);
        }
        Ok(out)
    }

    /// The page's span texts, parallel to `spans(index)`.
    #[wasm_bindgen(js_name = spanTexts)]
    pub fn span_texts(&self, index: usize) -> Result<Vec<String>, JsError> {
        Ok(self
            .spans_of(index)?
            .into_iter()
            .map(|span| span.text)
            .collect())
    }

    /// The page's painted glyphs as page-space numbers, five per glyph in
    /// painting order: `[x, y, advance, size, …]`.
    ///
    /// The text layer over the canvas is built from these: the renderer paints
    /// the page, so only it knows where each glyph landed and how far the pen
    /// moved -- the extractor falls back to 500/1000 em when a font states no
    /// `/Widths`, which is a quarter too narrow on exactly the base-14 pages.
    /// `glyphTexts(index)` returns the parallel characters, empty for a code the
    /// font's encoding cannot name.
    #[wasm_bindgen(js_name = glyphs)]
    pub fn glyphs(&self, index: usize) -> Result<Vec<f64>, JsError> {
        let glyphs = self.glyphs_of(index)?;
        let mut out = Vec::with_capacity(glyphs.len() * 4);
        for glyph in glyphs {
            out.extend_from_slice(&[glyph.x, glyph.y, glyph.advance, glyph.size]);
        }
        Ok(out)
    }

    /// The page's glyph characters, parallel to `glyphs(index)`.
    #[wasm_bindgen(js_name = glyphTexts)]
    pub fn glyph_texts(&self, index: usize) -> Result<Vec<String>, JsError> {
        Ok(self
            .glyphs_of(index)?
            .into_iter()
            .map(|glyph| glyph.text)
            .collect())
    }

    /// Every match's own rectangle and character offsets: `[x0, y0, x1, y1,
    /// start, end, …]`, six numbers per match, in the same order as `search`.
    ///
    /// `search` gives each hit's *line*; this gives the rectangle of the matched
    /// characters, which is what a mark wants when the document's advances are the
    /// ones on the page. The offsets are character indices into the line's text.
    #[wasm_bindgen(js_name = searchMatches)]
    pub fn search_matches(&self, needle: &str) -> Result<Vec<f64>, JsError> {
        let mut out = Vec::new();
        for page in self
            .document
            .search_matches(needle)
            .map_err(|error| JsError::new(&error.to_string()))?
        {
            for hit in page.matches {
                out.extend_from_slice(&[
                    hit.bounds.x0,
                    hit.bounds.y0,
                    hit.bounds.x1,
                    hit.bounds.y1,
                    hit.start as f64,
                    hit.end as f64,
                ]);
            }
        }
        Ok(out)
    }

    /// Every search hit in the document as `[page, x0, y0, x1, y1, …]`, four box
    /// numbers after each page index, case-insensitively.
    ///
    /// `searchTexts(needle)` returns the matched strings in the same order.
    #[wasm_bindgen(js_name = search)]
    pub fn search(&self, needle: &str) -> Result<Vec<f64>, JsError> {
        let mut out = Vec::new();
        for hits in self
            .document
            .search(needle)
            .map_err(|error| JsError::new(&error.to_string()))?
        {
            for hit in hits.hits {
                out.extend_from_slice(&[
                    hits.page as f64,
                    hit.bounds.x0,
                    hit.bounds.y0,
                    hit.bounds.x1,
                    hit.bounds.y1,
                ]);
            }
        }
        Ok(out)
    }

    /// The matched strings, parallel to `search(needle)`.
    #[wasm_bindgen(js_name = searchTexts)]
    pub fn search_texts(&self, needle: &str) -> Result<Vec<String>, JsError> {
        let pages = self
            .document
            .search(needle)
            .map_err(|error| JsError::new(&error.to_string()))?;
        Ok(pages
            .into_iter()
            .flat_map(|hits| hits.hits.into_iter().map(|hit| hit.text))
            .collect())
    }

    /// The spans of one page, shared by the two accessors above.
    /// The painted glyphs of one page, at the render scale of `1.0` points.
    fn glyphs_of(&self, index: usize) -> Result<Vec<pdf2_render::GlyphBox>, JsError> {
        pdf2_render::render_page_number_traced(
            &self.document,
            index,
            &pdf2_render::RenderOptions::default(),
        )
        .map(|traced| traced.glyphs)
        .map_err(|error| JsError::new(&error.to_string()))
    }

    fn spans_of(&self, index: usize) -> Result<Vec<pdf2::TextSpan>, JsError> {
        let page = self
            .document
            .page(index)
            .map_err(|error| JsError::new(&error.to_string()))?;
        self.document
            .spans(&page)
            .map_err(|error| JsError::new(&error.to_string()))
    }

    /// The XMP metadata packet as text, when the document has one.
    #[wasm_bindgen(js_name = metadata)]
    pub fn metadata(&self) -> Option<String> {
        self.document
            .metadata()
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Render a page to RGBA8 pixels at `scale` device pixels per point.
    ///
    /// The result is a `Uint8ClampedArray` of `width * height * 4` bytes, ready
    /// for `ImageData` or a manual canvas `putImageData`.
    #[wasm_bindgen(js_name = renderPage)]
    pub fn render_page(&self, index: usize, scale: f64) -> Result<Uint8ClampedArray, JsError> {
        let rendered = self
            .render(index, scale)
            .map_err(|error| JsError::new(&error))?;
        Ok(Uint8ClampedArray::from(rendered.canvas.pixels()))
    }

    /// Render a page straight onto a canvas, resizing it to the page.
    ///
    /// Returns the report of what this renderer could not draw, as JSON: an
    /// object of `"reason": count`, empty when everything was drawn. A page
    /// that is blank because its font is unsupported says so here instead of
    /// looking broken.
    #[wasm_bindgen(js_name = renderPageToCanvas)]
    pub fn render_page_to_canvas(
        &self,
        index: usize,
        scale: f64,
        canvas: HtmlCanvasElement,
    ) -> Result<String, JsError> {
        let rendered = self
            .render(index, scale)
            .map_err(|error| JsError::new(&error))?;
        let width = rendered.canvas.width();
        let height = rendered.canvas.height();
        canvas.set_width(width);
        canvas.set_height(height);
        let context = canvas
            .get_context("2d")
            .map_err(|error| JsError::new(&format!("canvas context: {error:?}")))?
            .ok_or_else(|| JsError::new("no 2d context"))?;
        let context = context
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|error| JsError::new(&format!("2d context: {error:?}")))?;
        let image = ImageData::new_with_u8_clamped_array_and_sh(
            Clamped(rendered.canvas.pixels()),
            width,
            height,
        )
        .map_err(|error| JsError::new(&format!("ImageData: {error:?}")))?;
        context
            .put_image_data(&image, 0.0, 0.0)
            .map_err(|error| JsError::new(&format!("putImageData: {error:?}")))?;
        Ok(report_json(&rendered.report))
    }

    fn render(&self, index: usize, scale: f64) -> Result<pdf2_render::RenderedPage, String> {
        render_page_number(
            &self.document,
            index,
            &RenderOptions {
                scale,
                ..RenderOptions::default()
            },
        )
        .map_err(|error| error.to_string())
    }
}
