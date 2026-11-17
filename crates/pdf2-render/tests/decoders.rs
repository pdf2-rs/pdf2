//! The decoders behind features: a codec that is not compiled in is named.
//!
//! `/JPXDecode` and `/JBIG2Decode` are off by default, because both cost tens of
//! kilobytes of gzip on the wasm bundle that most pages never use. What must not
//! happen is silence: the page loses its image and the report says `Do` with no
//! reason attached, which is what the library's generic filter path produces. The
//! arms below assert the message the *decoder dispatch* returns, one per codec,
//! so a build without the feature says which feature to turn on.
//!
//! These are the *absent* codec's tests, so the whole file is compiled only while
//! neither decoder is on; what a build *with* the feature does lives in
//! `tests/jpx.rs` and `tests/jbig2.rs`.

use pdf2::{Document, ObjectId};
use pdf2_render::Image;

#[cfg(any(not(feature = "jpx"), not(feature = "jbig2")))]
fn object(number: u32, body: &[u8], out: &mut Vec<u8>, offsets: &mut Vec<usize>) {
    offsets.push(out.len());
    out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendobj\n");
}

/// A one-page document whose object 5 is the image built by `image`.
#[cfg(any(not(feature = "jpx"), not(feature = "jbig2")))]
fn pdf(image: &[u8]) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets = Vec::new();
    object(
        1,
        b"<< /Type /Catalog /Pages 2 0 R >>",
        &mut out,
        &mut offsets,
    );
    object(
        2,
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &mut out,
        &mut offsets,
    );
    object(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20] /Contents 4 0 R \
          /Resources << /XObject << /Im1 5 0 R >> >> >>",
        &mut out,
        &mut offsets,
    );
    object(
        4,
        b"<< /Length 24 >>\nstream\nq 20 0 0 20 0 0 cm /Im1 Do Q\nendstream",
        &mut out,
        &mut offsets,
    );
    object(5, image, &mut out, &mut offsets);
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
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

/// Decode object 5 as an image, returning the error message the dispatcher gave.
#[cfg(any(not(feature = "jpx"), not(feature = "jbig2")))]
fn decode_error(image: &[u8]) -> String {
    let document = Document::open(pdf(image)).expect("open");
    let object = document
        .get_object_by_number(5)
        .expect("object 5 is the image");
    let stream = object.as_stream().expect("object 5 is a stream");
    Image::from_stream(&document, stream, Some(ObjectId::new(5, 0)))
        .expect_err("the image does not decode without its feature")
        .to_string()
}

#[cfg(not(feature = "jpx"))]
#[test]
fn a_jpx_without_the_feature_names_the_feature() {
    let message = decode_error(
        b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
          /Filter /JPXDecode /Length 4 >>\nstream\n\xff\x4f\xff\x51\nendstream",
    );
    assert!(
        message.contains("/JPXDecode images") && message.contains("jpx"),
        "the message names the codec and the feature: {message}"
    );
}

#[cfg(not(feature = "jbig2"))]
#[test]
fn a_jbig2_without_the_feature_names_the_feature() {
    let message = decode_error(
        b"<< /Type /XObject /Subtype /Image /Width 399 /Height 400 \
          /BitsPerComponent 1 /ColorSpace /DeviceGray /Filter /JBIG2Decode /Length 2 >>\nstream\n\x97\x4a\nendstream",
    );
    assert!(
        message.contains("/JBIG2Decode images") && message.contains("jbig2"),
        "the message names the codec and the feature: {message}"
    );
}
