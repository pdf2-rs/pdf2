//! Colour-key masking (`8.9.6.4`): a keyed sample is not painted.
//!
//! The fixture's image is 2×2 — red/blue on top, cyan/black below — drawn over a
//! green square, so a hidden sample shows **green** and a painted one shows its own
//! colour. Two keys: the corpus page's own `[255 255 0 255 0 255]` ("red exactly
//! 255", inclusive) and `[0 0 0 0 0 0]`, which is the boundary case — an exclusive
//! reading would mask nothing there.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{Image, RenderOptions, RenderedPage, render_page_number};

fn fixture() -> Vec<u8> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/colour-key.pdf");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(page: usize) -> RenderedPage {
    let document = Document::open(fixture()).expect("open fixture");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("in bounds")
}

const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn a_keyed_sample_shows_the_background_through_it() {
    let rendered = render(0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the keyed image draws: {}",
        rendered.report
    );
    // Image space: red and blue are the top row (screen rows 10..30), cyan and
    // black the bottom (30..50); the columns split at x = 30.
    assert_eq!(pixel(&rendered, 20, 20), GREEN, "the red sample is hidden");
    assert_eq!(pixel(&rendered, 40, 20), [0, 0, 255, 255], "blue is not");
    assert_eq!(pixel(&rendered, 20, 40), [0, 255, 255, 255], "cyan is not");
    assert_eq!(pixel(&rendered, 40, 40), [0, 0, 0, 255], "black is not");
}

#[test]
fn min_equal_to_max_masks_its_sample_and_nothing_else() {
    // `/Mask [0 0 0 0 0 0]` — the inclusive boundary the clause's ranges need.
    let rendered = render(1);
    assert_eq!(
        pixel(&rendered, 20, 20),
        [255, 0, 0, 255],
        "red is not keyed"
    );
    assert_eq!(
        pixel(&rendered, 40, 20),
        [0, 0, 255, 255],
        "blue is not keyed"
    );
    assert_eq!(
        pixel(&rendered, 20, 40),
        [0, 255, 255, 255],
        "cyan is not keyed"
    );
    assert_eq!(pixel(&rendered, 40, 40), GREEN, "the pure black sample is");
}

#[test]
fn a_dct_stream_with_a_key_is_refused_by_name() {
    // The clause forbids the pairing (lossy samples drift across the boundary), so
    // the refusal happens before the stream is decoded — the payload here is not a
    // JPEG at all, and the error still names the rule rather than the bytes.
    let pdf = pdf_with_image(b"/Filter /DCTDecode /Mask [0 0 0 0 0 0]", b"not a jpeg");
    let error = decode(&pdf);
    assert!(
        matches!(&error, pdf2_render::Error::Unsupported { what } if what.contains("DCTDecode")),
        "refused by name: {error:?}"
    );
}

#[test]
fn a_mask_that_is_a_stream_is_refused_by_name() {
    // `/Mask` as a reference is a stencil image (8.9.6.3), a different mechanism
    // with its own size and resampling; it is not read as a colour key.
    let pdf = pdf_with_image(b"/Mask 9 0 R", &[0_u8; 12]);
    let error = decode(&pdf);
    assert!(
        matches!(&error, pdf2_render::Error::Unsupported { what } if what.contains("stencil")),
        "refused by name: {error:?}"
    );
}

/// A one-page PDF whose only image carries `extra` in its dictionary.
fn pdf_with_image(extra: &[u8], pixels: &[u8]) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    {
        let mut object = |number: usize, body: &[u8], out: &mut Vec<u8>| {
            offsets.insert(number, out.len());
            out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        };
        object(1, b"<< /Type /Catalog /Pages 2 0 R >>", &mut out);
        object(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>", &mut out);
        object(
            3,
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>",
            &mut out,
        );
        object(
            4,
            b"<< /Length 24 >>\nstream\nq 20 0 0 20 0 0 cm /Im1 Do Q\nendstream",
            &mut out,
        );
    }
    // The image itself: its dict, then its bytes, then `endstream`.
    offsets.insert(5, out.len());
    out.extend_from_slice(b"5 0 obj\n");
    out.extend_from_slice(
        format!(
            "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} {} >>\nstream\n",
            pixels.len(),
            String::from_utf8_lossy(extra)
        )
        .as_bytes(),
    );
    out.extend_from_slice(pixels);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    // A bare object 9 so a `/Mask 9 0 R` reference resolves to something.
    {
        offsets.insert(9, out.len());
        out.extend_from_slice(b"9 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 10\n0000000000 65535 f \n");
    for number in 1..10_usize {
        match offsets.get(&number) {
            Some(offset) => out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 00000 f \n"),
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 10 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn decode(pdf: &[u8]) -> pdf2_render::Error {
    let document = Document::open(pdf.to_vec()).expect("open");
    let Object::Stream(stream) = document.get_object_by_number(5).expect("image") else {
        panic!("not a stream");
    };
    Image::from_stream(&document, &stream, Some(pdf2::ObjectId::new(5, 0))).expect_err("refused")
}

use pdf2::Object;
