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
use pdf2_render::{render_page_number, RenderOptions};

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
    #[wasm_bindgen(js_name = renderPageToCanvas)]
    pub fn render_page_to_canvas(
        &self,
        index: usize,
        scale: f64,
        canvas: HtmlCanvasElement,
    ) -> Result<(), JsError> {
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
            .map_err(|error| JsError::new(&format!("putImageData: {error:?}")))
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
