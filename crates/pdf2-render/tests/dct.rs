//! `/DCTDecode` images through `zune-jpeg`, behind the `dct` feature.
//!
//! The corpus carries two JPEG pages — `cmykjpeg.pdf` (a CMYK-ish artwork page)
//! and `alphatrans.pdf` (a photograph under 50 % alpha squares) — and both used
//! to report `Do` and draw nothing. They are committed, so these tests run
//! wherever the renderer does; the decoder itself is a dependency, and what is
//! asserted here is that it is *wired in*: the operator stops being reported, the
//! image arrives at the size its own header declares, and the pixels are ink.

use std::path::{Path, PathBuf};

use pdf2::{Document, Object};
use pdf2_render::{render_page_number, Image, RenderOptions};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(path: &str) -> pdf2_render::RenderedPage {
    let bytes = std::fs::read(repo(path)).expect("fixture");
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

fn ink(rendered: &pdf2_render::RenderedPage) -> usize {
    let mut count = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if rendered.canvas.pixel(x, y) != Some([255, 255, 255, 255]) {
                count += 1;
            }
        }
    }
    count
}

/// The document and the first `/DCTDecode` image stream in it.
///
/// `cmykjpeg.pdf` keeps its image inside a form XObject, so walking the object
/// table is simpler than following resources — and the trailer's `/Size` bounds
/// the walk.
fn first_jpeg(path: &str) -> (Document, pdf2::ObjectId) {
    let bytes = std::fs::read(repo(path)).expect("fixture");
    let document = Document::open(bytes).expect("open");
    let size = document
        .trailer()
        .get(b"Size")
        .and_then(Object::as_integer)
        .unwrap_or(32);
    for number in 1..=u32::try_from(size).unwrap_or(32) {
        if !document.contains_object(number) {
            continue;
        }
        let Ok(object) = document.get_object_by_number(number) else {
            continue;
        };
        let Object::Stream(stream) = object else {
            continue;
        };
        let is_image =
            matches!(stream.dict().get(b"Subtype"), Some(Object::Name(name)) if name == b"Image");
        let is_jpeg = matches!(stream.dict().get(b"Filter"), Some(Object::Name(name)) if name == b"DCTDecode");
        if is_image && is_jpeg {
            return (document, pdf2::ObjectId::new(number, 0));
        }
    }
    panic!("no /DCTDecode image in {path}");
}

#[test]
fn the_jpeg_page_stops_reporting_do() {
    let rendered = render("corpus/tier-a/cmykjpeg.pdf");
    assert!(
        !rendered.report.to_string().contains("Do"),
        "the image is decoded, not skipped: {}",
        rendered.report
    );
    assert!(ink(&rendered) > 10_000, "the page has artwork on it");
}

#[test]
fn the_decoder_reports_the_size_the_header_declares() {
    let (document, id) = first_jpeg("corpus/tier-a/cmykjpeg.pdf");
    let stream = document.stream(id).expect("stream");
    let image = Image::from_stream(&document, &stream, Some(id)).expect("decode");
    assert_eq!((image.width(), image.height()), (200, 150));
}

#[test]
fn a_photograph_under_alpha_still_decodes() {
    // `alphatrans.pdf` draws a JPEG under three 50 % squares; the image itself
    // must be there even where the squares tint it.
    let rendered = render("corpus/tier-a/alphatrans.pdf");
    assert!(
        !rendered.report.to_string().contains("Do"),
        "no image was skipped: {}",
        rendered.report
    );
    assert!(ink(&rendered) > 50_000, "the page has the photograph on it");
}
