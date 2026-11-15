//! Content-stream interpretation and text extraction (Q7, M7).
//!
//! `tests/fixtures/text.pdf` has an uncompressed `/Contents` stream that walks
//! through the text operators (`Tj`, `TJ` with a gap, `'`, `TL` + `T*`, `Tm`),
//! two fonts (WinAnsiEncoding and a `/Differences` array) and therefore covers
//! the whole extraction path without any geometry guessing.

use pdf2::Document;

const TEXT: &[u8] = include_bytes!("fixtures/text.pdf");
const PAGE_TREE: &[u8] = include_bytes!("fixtures/page-tree.pdf");
const TWIN: &[u8] = include_bytes!("fixtures/classic-twin.pdf");
const XREF_STREAM: &[u8] = include_bytes!("fixtures/xref-stream.pdf");

fn open(bytes: &[u8]) -> Document {
    Document::open(bytes).expect("fixture opens")
}

fn doc(objects: &[(u32, &str)]) -> Document {
    Document::open(pdf(objects)).expect("document opens")
}

/// Assemble a classic-xref PDF around `(object number, body)` pairs.
fn pdf(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets = Vec::new();
    for (number, body) in objects {
        offsets.push((*number, out.len()));
        out.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n");
    for (number, offset) in &offsets {
        out.extend_from_slice(format!("{number} 1\n{offset:010} 00000 n \n").as_bytes());
    }
    let size = objects.iter().map(|(number, _)| *number).max().unwrap_or(0) + 1;
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

#[test]
fn a_real_fixture_extracts_its_text() {
    let document = open(TEXT);
    let page = document.page(0).expect("page");
    let text = document.extract_text(&page).expect("text");
    assert_eq!(
        text, "Hello, PDF 2.0\nSecond line\nspaced out\nfourth line\n\u{2022} dot\ncaf\u{e9} ok",
        "newlines come from Td/T*/Tm, the space in the TJ array from its -300 adjustment, \
         the bullet from /Differences and the accent from WinAnsiEncoding"
    );
}

#[test]
fn the_content_stream_of_a_page_is_uncompressed_and_reachable() {
    let document = open(TEXT);
    let page = document.page(0).expect("page");
    let content = document.page_content(&page).expect("content");
    assert!(content.starts_with(b"BT\n"));
    assert!(
        content.windows(12).any(|window| window == b"(Second line"),
        "the fixture is meant to be readable: {}",
        String::from_utf8_lossy(&content)
    );
}

#[test]
fn other_fixtures_extract_text_too() {
    let document = open(PAGE_TREE);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("page-tree text"),
        "page one"
    );

    let document = open(TWIN);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("twin text"),
        "Hello, PDF 2.0"
    );

    // The xref-stream fixture keeps its page inside an object stream.
    let document = open(XREF_STREAM);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("xref-stream text"),
        "Hello, PDF 2.0"
    );
}

fn text_of(content: &str) -> String {
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ]);
    let resources = document
        .page(0)
        .expect("page")
        .resources
        .expect("resources");
    document
        .extract_text_from(content.as_bytes(), Some(&resources))
        .expect("text")
}

#[test]
fn tj_adjustments_only_become_spaces_when_they_are_wide() {
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td [(A) -300 (B)] TJ ET"),
        "A B"
    );
    assert_eq!(text_of("BT /F1 12 Tf 10 700 Td [(A) -100 (B)] TJ ET"), "AB");
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td [(A) -1000 (B)] TJ ET"),
        "A B"
    );
}

#[test]
fn line_breaks_come_from_positioning_operators() {
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td (one) Tj 0 -20 Td (two) Tj ET"),
        "one\ntwo"
    );
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td (one) Tj 10 0 Td (two) Tj ET"),
        "onetwo",
        "moving along the line does not break it"
    );
    assert_eq!(
        text_of("BT /F1 12 Tf 12 TL 10 700 Td (one) Tj T* (two) Tj ET"),
        "one\ntwo"
    );
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td (one) Tj 1 0 0 1 10 600 Tm (two) Tj ET"),
        "one\ntwo"
    );
    assert_eq!(
        text_of("BT /F1 12 Tf 12 TL 10 700 Td (one) ' (two) ' ET"),
        "one\ntwo",
        "the quote operator moves down by the leading before showing text"
    );
}

#[test]
fn unsupported_operators_are_ignored_not_fatal() {
    // Graphics operators around the text must not disturb extraction.
    assert_eq!(
        text_of(
            "q 1 0 0 1 0 0 cm 0.5 g 0 0 100 100 re f Q \
             BT /F1 12 Tf 10 700 Td (still here) Tj ET"
        ),
        "still here"
    );
}

#[test]
fn inline_images_are_skipped() {
    let mut content = b"q BI /W 1 /H 1 /BPC 8 /CS /G ID ".to_vec();
    content.extend_from_slice(&[0x00, 0x20, 0xff]);
    content.extend_from_slice(b" EI Q BT /F1 12 Tf 10 700 Td (after image) Tj ET");
    assert_eq!(text_of(&String::from_utf8_lossy(&content)), "after image");

    // A BI with no EI is an error, not an endless loop.
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    assert!(document
        .extract_text_from(b"BI /W 1 ID \x00\x01", None)
        .is_err());
}

#[test]
fn a_missing_font_resource_is_an_error() {
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    assert!(document
        .extract_text_from(b"BT /F9 12 Tf 10 700 Td (x) Tj ET", None)
        .is_err());
}

#[test]
fn composite_fonts_are_reported_as_unsupported() {
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /MediaBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (5, "<< /Type /Font /Subtype /Type0 /BaseFont /Mincho >>"),
    ]);
    let resources = document.page(0).expect("page").resources.expect("res");
    let error = document
        .extract_text_from(b"BT /F1 12 Tf 10 700 Td (x) Tj ET", Some(&resources))
        .expect_err("Type0");
    assert!(error.to_string().contains("Type0"), "{error}");
}

#[test]
fn a_font_without_a_declared_encoding_falls_back_to_standard() {
    // StandardEncoding maps the ASCII range straight through.
    assert_eq!(
        text_of("BT /F1 12 Tf 10 700 Td (ASCII stays ASCII) Tj ET"),
        "ASCII stays ASCII"
    );
}

#[test]
fn whitespace_and_comments_in_content_streams_are_skipped() {
    assert_eq!(
        text_of("BT % a comment\n /F1 12 Tf \n 10 700 Td \n (ok) Tj % trailing\n ET"),
        "ok"
    );
}

#[test]
fn every_truncation_of_the_content_stream_is_handled() {
    let document = open(TEXT);
    let page = document.page(0).expect("page");
    let content = document.page_content(&page).expect("content");
    let resources = page.resources.clone();
    for end in 0..content.len() {
        let _ = document.extract_text_from(&content[..end], resources.as_ref());
    }
    assert!(document
        .extract_text_from(&content, resources.as_ref())
        .is_ok());
}

#[test]
fn incomplete_text_runs_do_not_produce_garbage() {
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    // `Tj` with no operand and positioning operators with no operands are
    // simply ignored.
    assert_eq!(
        document
            .extract_text_from(b"BT Tj ET 10 700 Td T* ET", None)
            .expect("ignored"),
        ""
    );
    // Text shown before any font is selected yields nothing.
    assert_eq!(
        document
            .extract_text_from(b"BT (string) Tj ET", None)
            .expect("no font selected yet"),
        ""
    );
    // `Tf` without a name is a broken stream and says so.
    assert!(document.extract_text_from(b"BT 12 Tf ET", None).is_err());
    assert!(document.get_object(pdf2::ObjectId::new(1, 0)).is_ok());
}

#[test]
fn lines_come_out_in_reading_order_not_paint_order() {
    // The fixture paints third, first, second; the page reads first, second,
    // third, and the middle line's space is a `TJ` adjustment rather than a
    // space glyph.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/reading-order.pdf"
    ))
    .expect("read reading-order.pdf");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("text"),
        "first line\nsecond line\nthird line"
    );
}

#[test]
fn a_composite_font_reads_through_its_to_unicode_map() {
    // The fixture's codes bear no relation to their text, and its second run is
    // placed 120 units right of where the first stopped: the text can only be
    // right through `/ToUnicode` and the font's `/W` and `/DW` widths.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/tounicode.pdf"
    ))
    .expect("read tounicode.pdf");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "ADabcxyz A");
}

#[test]
fn composite_fonts_we_cannot_read_are_refused_by_name() {
    for (font, expected) in [
        (
            "<< /Type /Font /Subtype /Type0 /BaseFont /Mincho /Encoding /Identity-H              /DescendantFonts [5 0 R] >>",
            "without a /ToUnicode map",
        ),
        (
            "<< /Type /Font /Subtype /Type0 /BaseFont /Mincho /Encoding /Identity-V              /ToUnicode 5 0 R /DescendantFonts [5 0 R] >>",
            "vertical composite",
        ),
        (
            "<< /Type /Font /Subtype /Type0 /BaseFont /Mincho /Encoding /90pv-RKSJ-H              /ToUnicode 5 0 R /DescendantFonts [5 0 R] >>",
            "CMap-encoded composite",
        ),
    ] {
        let all: &[&str] = &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /MediaBox [0 0 612 792]              /Resources << /Font << /F1 4 0 R >> >> >>",
            font,
            "null",
        ];
        let objects: Vec<(u32, &str)> = all
            .iter()
            .enumerate()
            .map(|(index, body)| (u32::try_from(index + 1).expect("index"), *body))
            .collect();
        let document = doc(&objects);
        let resources = document.page(0).expect("page").resources.expect("res");
        let error = document
            .extract_text_from(b"BT /F1 12 Tf 10 700 Td (x) Tj ET", Some(&resources))
            .expect_err(expected);
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn a_predefined_cmap_font_extracts_cjk() {
    // The corpus file sets `/Encoding /90ms-RKSJ-H` over an Adobe-Japan1 CID
    // font with no `/ToUnicode`: the text can only come from the predefined CMap
    // the crate ships (`cargo xtask cmaps`).
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/corpus/tier-a/90ms_rksj_h_sample.pdf"
    );
    let bytes = std::fs::read(path).expect("read the corpus file");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("text"),
        "Hello ASCII\n\u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}"
    );
}
