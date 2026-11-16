//! An image is measured against the budget *before* its stream is decoded.
//!
//! The order matters: `/Width`, `/Height` and the colour space are dictionary
//! entries, so the decoded size is known without touching the data. A reader
//! that decodes first pays for whatever the file claims — `image.rs` used to do
//! exactly that (`docs/RENDERING-QUALITY.md`, F5) — which is how a lying
//! `/Width` becomes a memory bomb with a ten-byte stream behind it.

use pdf2::{Document, Error};
use pdf2_render::Image;

/// A document with one image XObject (object 4) whose stream is `data`.
fn pdf_with_image(width: u32, height: u32, data: &[u8], filter: Option<&str>) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets: Vec<(usize, usize)> = Vec::new();
    let mut object = |number: usize, body: &[u8], out: &mut Vec<u8>| {
        offsets.push((number, out.len()));
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(1, b"<< /Type /Catalog /Pages 2 0 R >>", &mut out);
    object(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>", &mut out);
    object(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /I1 4 0 R >> >> >>",
        &mut out,
    );
    let filter = filter
        .map(|name| format!(" /Filter /{name}"))
        .unwrap_or_default();
    offsets.push((4, out.len()));
    out.extend_from_slice(b"4 0 obj\n");
    out.extend_from_slice(
        format!(
            "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
             /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {}{filter} >>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for (_, offset) in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn image_stream(document: &Document) -> pdf2::Stream {
    document
        .stream(pdf2::ObjectId::new(4, 0))
        .expect("the image stream")
}

#[test]
fn a_lying_width_is_refused_before_the_stream_is_decoded() {
    // 10 000 × 10 000 × 3 bytes is 300 MB, past the 256 MiB default — and the
    // stream is not even valid Flate. If the code decoded first, the error would
    // be about the payload; it is about the budget, which proves the order.
    let bytes = pdf_with_image(10_000, 10_000, b"not deflate at all", Some("FlateDecode"));
    let document = Document::open(bytes).expect("open");
    let stream = image_stream(&document);
    let error = Image::from_stream(&document, &stream).expect_err("past the budget");
    assert!(
        matches!(error, Error::Limit { limit, .. } if limit == 256 << 20),
        "{error}"
    );
    assert!(error.to_string().contains("268435456"), "{error}");
    assert!(
        !error.to_string().contains("Flate"),
        "not a decode failure: {error}"
    );
}

#[test]
fn the_budget_bounds_a_real_image_too_and_the_setter_moves_it() {
    // 400 × 400 × 3 = 480 000 bytes: refused under a 256 KiB budget, drawn under
    // a 1 MiB one.
    let data = vec![0x40_u8; 400 * 400 * 3];
    let bytes = pdf_with_image(400, 400, &data, None);
    let document = Document::open(bytes).expect("open");
    let stream = image_stream(&document);

    document.set_decoded_limit(256 << 10);
    assert!(matches!(
        Image::from_stream(&document, &stream),
        Err(Error::Limit { limit, .. }) if limit == 256 << 10
    ));

    document.set_decoded_limit(1 << 20);
    let image = Image::from_stream(&document, &stream).expect("480 kB fits");
    assert_eq!((image.width(), image.height()), (400, 400));
}

#[test]
fn an_image_shorter_than_its_dimensions_still_says_so() {
    // Under the budget, the existing length check still owns the verdict: the
    // new limit must not swallow the honest "too short" case.
    let bytes = pdf_with_image(100, 100, &[0u8; 10], None);
    let document = Document::open(bytes).expect("open");
    let stream = image_stream(&document);
    let error = Image::from_stream(&document, &stream).expect_err("short");
    assert!(error.to_string().contains("shorter than width"), "{error}");
    assert!(!matches!(error, Error::Limit { .. }), "{error}");
}
