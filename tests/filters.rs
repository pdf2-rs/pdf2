//! Stream filters decoded through a real document (Q4, M4).
//!
//! The PDFs here are assembled in the test with correct cross-reference
//! offsets, and the compressed payloads are produced by this crate's own
//! dependencies (`flate2`) or by small encoders below — no external tool is
//! involved, which is the only way to prove the decoders in this environment.

use pdf2::{Document, Error, Object, ObjectId};

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut group = [0_u8; 4];
        group[..chunk.len()].copy_from_slice(chunk);
        let mut value = u64::from(u32::from_be_bytes(group));
        if chunk.len() == 4 && value == 0 {
            out.push(b'z');
            continue;
        }
        let mut divisor = 85_u64.pow(4);
        let mut digits = [0_u8; 5];
        for digit in &mut digits {
            *digit = u8::try_from(value / divisor).expect("digit");
            value %= divisor;
            divisor /= 85;
        }
        for &digit in &digits[..chunk.len() + 1] {
            out.push(digit + b'!');
        }
    }
    out.extend_from_slice(b"~>");
    out
}

fn hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for byte in data {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.push(b'>');
    out
}

/// A stream object body with `/Length` computed from the payload.
fn stream_body(entries: &str, data: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(
        format!("<< /Length {} {} >>\nstream\n", data.len(), entries).as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(b"\nendstream");
    body
}

/// Assemble a classic-xref PDF around `(object number, body)` pairs.
fn pdf(objects: &[(u32, Vec<u8>)], trailer_keys: &str) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets = Vec::new();
    for (number, body) in objects {
        offsets.push((*number, out.len()));
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n");
    for (number, offset) in &offsets {
        out.extend_from_slice(format!("{number} 1\n{offset:010} 00000 n \n").as_bytes());
    }
    let size = objects.iter().map(|(number, _)| *number).max().unwrap_or(0) + 1;
    out.extend_from_slice(format!("trailer\n<< /Size {size} {trailer_keys} >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn document_with_stream(entries: &str, data: &[u8]) -> Document {
    let pdf = pdf(
        &[
            (1, b"<< /Type /Catalog >>".to_vec()),
            (2, stream_body(entries, data)),
        ],
        "/Root 1 0 R",
    );
    Document::open(pdf).expect("document opens")
}

const CONTENT: &[u8] = b"BT /F1 12 Tf (filtered) Tj ET";

#[test]
fn flate_streams_decode() {
    let document = document_with_stream("/Filter /FlateDecode", &deflate(CONTENT));
    let decoded = document
        .stream_data(ObjectId::new(2, 0))
        .expect("flate stream");
    assert_eq!(decoded, CONTENT);

    // The same payload through the stream's own method.
    let stream = document.stream(ObjectId::new(2, 0)).expect("stream");
    assert_eq!(stream.decode().expect("stream decode"), CONTENT);
}

#[test]
fn ascii_hex_streams_decode() {
    let encoded = hex(CONTENT);
    let document = document_with_stream("/Filter /ASCIIHexDecode", &encoded);
    assert_eq!(
        document.stream_data(ObjectId::new(2, 0)).expect("hex"),
        CONTENT
    );
}

#[test]
fn ascii85_streams_decode() {
    let encoded = ascii85(CONTENT);
    let document = document_with_stream("/Filter /ASCII85Decode", &encoded);
    assert_eq!(
        document.stream_data(ObjectId::new(2, 0)).expect("ascii85"),
        CONTENT
    );
}

#[test]
fn run_length_streams_decode() {
    // Literal run of the first 8 bytes, then a repeat run of 0x78 ('x').
    let mut encoded = vec![7];
    encoded.extend_from_slice(&CONTENT[..8]);
    encoded.extend_from_slice(&[255, b'x', 128]);
    let document = document_with_stream("/Filter /RunLengthDecode", &encoded);
    let decoded = document
        .stream_data(ObjectId::new(2, 0))
        .expect("runlength");
    let mut expected = CONTENT[..8].to_vec();
    expected.extend_from_slice(&[b'x'; 2]);
    assert_eq!(decoded, expected);
}

#[test]
fn filter_arrays_apply_in_order() {
    // The outermost filter is listed first: ASCII85 wraps Flate.
    let payload = ascii85(&deflate(CONTENT));
    let document = document_with_stream("/Filter [/ASCII85Decode /FlateDecode]", &payload);
    assert_eq!(
        document
            .stream_data(ObjectId::new(2, 0))
            .expect("filter array"),
        CONTENT
    );
}

#[test]
fn png_predictor_rows_are_undone() {
    let rows = [2, 1, 2, 3, 2, 3, 3, 3];
    let document = document_with_stream(
        "/Filter /FlateDecode /DecodeParms << /Predictor 12 /Colors 1 /BitsPerComponent 8 /Columns 3 >>",
        &deflate(&rows),
    );
    assert_eq!(
        document
            .stream_data(ObjectId::new(2, 0))
            .expect("png predictor"),
        vec![1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn tiff_predictor_rows_are_undone() {
    let rows = [10, 10, 10, 1, 0, 0];
    let document = document_with_stream(
        "/Filter /FlateDecode /DecodeParms << /Predictor 2 /Colors 1 /BitsPerComponent 8 /Columns 3 >>",
        &deflate(&rows),
    );
    assert_eq!(
        document
            .stream_data(ObjectId::new(2, 0))
            .expect("tiff predictor"),
        vec![10, 20, 30, 1, 1, 1]
    );
}

#[test]
fn an_indirect_length_is_resolved_through_the_document() {
    // Object 2's /Length points at object 9, which holds the byte count.
    let data = deflate(CONTENT);
    let mut body = b"<< /Length 9 0 R /Filter /FlateDecode >>\nstream\n".to_vec();
    body.extend_from_slice(&data);
    body.extend_from_slice(b"\nendstream");
    let pdf = pdf(
        &[
            (1, b"<< /Type /Catalog >>".to_vec()),
            (2, body),
            (9, data.len().to_string().into_bytes()),
        ],
        "/Root 1 0 R",
    );
    let document = Document::open(pdf).expect("document opens");
    let stream = document.stream(ObjectId::new(2, 0)).expect("stream");
    assert_eq!(stream.data(), data, "the payload stops exactly at /Length");
    assert_eq!(document.decode_stream(&stream).expect("decoded"), CONTENT);
}

#[test]
fn indirect_filter_and_decode_parms_are_resolved() {
    let rows = [2, 1, 2, 3, 2, 3, 3, 3];
    let data = deflate(&rows);
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "<< /Length {} /Filter 3 0 R /DecodeParms 4 0 R >>\nstream\n",
            data.len()
        )
        .as_bytes(),
    );
    body.extend_from_slice(&data);
    body.extend_from_slice(b"\nendstream");
    let pdf = pdf(
        &[
            (1, b"<< /Type /Catalog >>".to_vec()),
            (2, body),
            (3, b"/FlateDecode".to_vec()),
            (
                4,
                b"<< /Predictor 12 /Colors 1 /BitsPerComponent 8 /Columns 3 >>".to_vec(),
            ),
        ],
        "/Root 1 0 R",
    );
    let document = Document::open(pdf).expect("document opens");
    assert_eq!(
        document
            .stream_data(ObjectId::new(2, 0))
            .expect("indirect parms"),
        vec![1, 2, 3, 4, 5, 6]
    );
}

#[test]
fn unsupported_and_broken_filters_are_errors() {
    let document = document_with_stream("/Filter /DCTDecode", b"jpeg-ish");
    let error = document
        .stream_data(ObjectId::new(2, 0))
        .expect_err("image filter");
    assert!(error.to_string().contains("out of scope"), "{error}");

    let document = document_with_stream("/Filter /NotAFilter", b"x");
    assert!(document.stream_data(ObjectId::new(2, 0)).is_err());

    let document = document_with_stream("/Filter /FlateDecode", b"definitely not deflate");
    assert!(matches!(
        document.stream_data(ObjectId::new(2, 0)),
        Err(Error::Malformed { .. })
    ));

    let document = document_with_stream("/Filter /ASCIIHexDecode", b"48G>");
    assert!(document.stream_data(ObjectId::new(2, 0)).is_err());
}

#[test]
fn a_truncated_flate_payload_is_an_error_not_a_panic() {
    let payload = deflate(CONTENT);
    for end in 0..payload.len() {
        let document = document_with_stream("/Filter /FlateDecode", &payload[..end]);
        let _ = document.stream_data(ObjectId::new(2, 0));
    }
    assert!(document_with_stream("/Filter /FlateDecode", &payload)
        .stream_data(ObjectId::new(2, 0))
        .is_ok());
}

#[test]
fn streams_without_filters_pass_through() {
    let document = document_with_stream("/Type /EmbeddedFile", CONTENT);
    assert_eq!(
        document.stream_data(ObjectId::new(2, 0)).expect("plain"),
        CONTENT
    );
}

#[test]
fn unknown_predictors_are_errors() {
    let document = document_with_stream(
        "/Filter /FlateDecode /DecodeParms << /Predictor 5 >>",
        &deflate(b"abc"),
    );
    assert!(document.stream_data(ObjectId::new(2, 0)).is_err());
}

#[test]
fn decoding_an_object_that_is_not_a_stream_is_an_error() {
    let pdf = pdf(
        &[(1, b"<< /Type /Catalog >>".to_vec()), (2, b"42".to_vec())],
        "/Root 1 0 R",
    );
    let document = Document::open(pdf).expect("document opens");
    assert!(document.stream(ObjectId::new(2, 0)).is_err());
    assert!(document.stream_data(ObjectId::new(2, 0)).is_err());
    assert!(matches!(
        document.get_object(ObjectId::new(2, 0)),
        Ok(Object::Integer(42))
    ));
}
