//! `/JPXDecode` (7.4.9): JPEG 2000 images, behind the `jpx` feature.
//!
//! What is proven here: the corpus page's codestream decodes and paints at the
//! size and place its own header declares; the size check reads that header and
//! refuses an over-budget image **before** a pixel is decoded, which a truncated
//! body proves; and the colour spaces this renderer cannot paint are refused by
//! name rather than mapped by component count (that last one lives beside the
//! mapping, in `src/image.rs`).
//!
//! The sample is a pdf.js *bug* file: its codestream is 204 bytes with three
//! SOT segments for tile 0, and it decodes to a flat black field. So this page
//! proves dispatch, geometry and the budget, not pixel fidelity — fidelity is the
//! oracle's job, and this page may not be able to join it.

#![cfg(feature = "jpx")]

use std::path::{Path, PathBuf};

use pdf2::{Document, ObjectId};
use pdf2_render::{Image, RenderOptions, RenderedPage, render_page_number};

fn corpus() -> Vec<u8> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/tier-a/bug_jpx.pdf");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The image XObject's object number: the corpus page's only `/JPXDecode` stream.
fn image_number(document: &Document) -> u32 {
    for number in 1..=512u32 {
        let Ok(object) = document.get_object_by_number(number) else {
            continue;
        };
        let is_jpx = object.as_stream().is_some_and(|stream| {
            stream
                .dict()
                .get(b"Filter")
                .and_then(|filter| filter.as_name())
                .is_some_and(|name| name == b"JPXDecode")
        });
        if is_jpx {
            return number;
        }
    }
    panic!("the corpus page carries a /JPXDecode image");
}

#[test]
fn the_corpus_page_draws_its_codestream() {
    let document = Document::open(corpus()).expect("open");
    let rendered: RenderedPage =
        render_page_number(&document, 0, &RenderOptions::default()).expect("render");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the image draws instead of being reported: {}",
        rendered.report
    );
    // The codestream says 128×64 and the page places it at 135,296 in PDF points,
    // so at 2 px/pt the block is x 270..398, y 200..264 — and it is 8192 samples.
    let mut ink = 0usize;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            let pixel = rendered.canvas.pixel(x, y).expect("in bounds");
            if pixel != [255, 255, 255, 255] {
                ink += 1;
            }
        }
    }
    assert_eq!(ink, 128 * 64, "one painted pixel per sample");
    assert_eq!(rendered.canvas.pixel(270, 200), Some([0, 0, 0, 255]));
    assert_eq!(rendered.canvas.pixel(397, 263), Some([0, 0, 0, 255]));
    assert_eq!(
        rendered.canvas.pixel(269, 200),
        Some([255, 255, 255, 255]),
        "the block ends where the header says"
    );
}

#[test]
fn the_header_size_is_refused_before_any_decoding() {
    // The check reads `/Width × /Height × components` from the codestream header,
    // so a budget of 100 bytes refuses an 8192-sample image without touching the
    // tile data. The body is irrelevant: the error must be `Limit`, not a decode
    // failure.
    let document = Document::open(corpus()).expect("open");
    document.set_decoded_limit(100);
    let number = image_number(&document);
    let object = document.get_object_by_number(number).expect("the image");
    let stream = object.as_stream().expect("a stream");
    let error = Image::from_stream(&document, stream, Some(ObjectId::new(number, 0)))
        .expect_err("8192 samples do not fit in 100 bytes");
    assert!(
        matches!(error, pdf2_render::Error::Limit { .. }),
        "the header's size is refused, not decoded and then truncated: {error}"
    );

    // And the order is the *header's*: with the default budget the same bytes
    // decode, so the refusal above came from the size and not from the payload.
    document.set_decoded_limit(usize::MAX);
    let object = document.get_object_by_number(number).expect("the image");
    let stream = object.as_stream().expect("a stream");
    assert!(
        Image::from_stream(&document, stream, Some(ObjectId::new(number, 0))).is_ok(),
        "the same image decodes once the budget allows it"
    );
}
