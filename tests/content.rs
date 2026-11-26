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
fn a_simple_fonts_stated_widths_size_the_line() {
    // Two codes, 500 and 1000 units at 10 pt: the line is 15 units wide. Without
    // the widths the box would fall back to the documented 0.5 em estimate and be
    // 10 units -- the difference a search hit inherits.
    let content = "BT /F1 10 Tf 10 10 Td (AB) Tj ET";
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
        (
            5,
            "<< /Type /Font /Subtype /Type1 /BaseFont /X /Encoding /WinAnsiEncoding \
             /FirstChar 65 /Widths [500 1000] >>",
        ),
    ]);
    let page = document.page(0).expect("page");
    let spans = document.spans(&page).expect("spans");
    let span = spans.first().expect("one span");
    assert_eq!(span.text, "AB");
    assert!(
        (span.bounds.width() - 15.0).abs() < 0.01,
        "the stated widths size the line: {} against 15",
        span.bounds.width()
    );
}

#[test]
fn an_embedded_fonts_line_is_as_wide_as_its_stated_widths() {
    // `book.pdf` states `/Widths` for its serif: the line's box must be the sum of
    // those widths, character by character, and not the half-em estimate.
    const BOOK: &[u8] = include_bytes!("fixtures/book.pdf");
    let document = open(BOOK);
    let page = document.page(0).expect("page");
    let span = document
        .spans(&page)
        .expect("spans")
        .into_iter()
        .next()
        .expect("one span");
    let resources = page.resources.clone().expect("/Resources");
    let value = |object: &pdf2::Object| match object {
        pdf2::Object::Reference(id) => document.get_object(*id).expect("object reads"),
        other => other.clone(),
    };
    let fonts = value(resources.get(b"Font").expect("/Font"));
    let pdf2::Object::Dictionary(fonts) = fonts else {
        panic!("/Font is not a dictionary");
    };
    let (_, font) = fonts.iter().next().expect("a font");
    let pdf2::Object::Dictionary(font) = value(font) else {
        panic!("the font is not a dictionary");
    };
    let first = font
        .get(b"FirstChar")
        .and_then(pdf2::Object::as_integer)
        .unwrap_or(0);
    let pdf2::Object::Array(widths) = value(font.get(b"Widths").expect("/Widths")) else {
        panic!("/Widths is not an array");
    };
    let size = span.bounds.height();
    let expected: f64 = span
        .text
        .chars()
        .filter(|character| character.is_ascii())
        .map(|character| {
            let index = usize::try_from(i64::from(character as u32) - first).unwrap_or(0);
            widths
                .get(index)
                .and_then(pdf2::Object::as_real)
                .unwrap_or(0.0)
                / 1000.0
                * size
        })
        .sum();
    assert!(
        (span.bounds.width() - expected).abs() < 0.05,
        "the box is the stated widths: {} against {expected}",
        span.bounds.width()
    );
    assert!(
        (span.bounds.width() - 0.5 * size * span.text.chars().count() as f64).abs() > 1.0,
        "and not the estimate: {}",
        span.bounds.width()
    );
}

#[test]
fn a_font_without_widths_keeps_the_documented_estimate() {
    // `text.pdf`'s Helvetica states no `/Widths`, so the half-em estimate stands:
    // 14 characters at 18 pt is 126 units. (The renderer paints 6/9 em instead,
    // which is why a rendered mark still welds glyph cells -- ADR 0002's note.)
    let document = open(TEXT);
    let page = document.page(0).expect("page");
    let spans = document.spans(&page).expect("spans");
    let span = spans.first().expect("one span");
    assert_eq!(span.text, "Hello, PDF 2.0");
    assert!(
        (span.bounds.width() - 126.0).abs() < 0.01,
        "the estimate stands at 14 x 0.5 em x 18 pt: {} against 126",
        span.bounds.width()
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
    assert!(
        document
            .extract_text_from(b"BI /W 1 ID \x00\x01", None)
            .is_err()
    );
}

#[test]
fn a_missing_font_resource_is_an_error() {
    let document = doc(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    assert!(
        document
            .extract_text_from(b"BT /F9 12 Tf 10 700 Td (x) Tj ET", None)
            .is_err()
    );
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
    assert!(
        document
            .extract_text_from(&content, resources.as_ref())
            .is_ok()
    );
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
        "first line\nsecond line\nthird line\nscaled line"
    );
    // `ADR 0008`: the same accumulator carries the geometry. The three lines are
    // placed by `Tm` with an identity CTM, so their page-space origins are the
    // numbers in the content stream; the fourth is painted under `q 2 0 0 2 20 100
    // cm`, so its origin moves to (20, 100) and its box doubles.
    let spans = document.spans(&page).expect("spans");
    let texts: Vec<&str> = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(
        texts,
        ["first line", "second line", "third line", "scaled line"]
    );
    let bounds = |index: usize| spans[index].bounds;
    for (index, expected) in [(0, 400.0), (1, 350.0), (2, 300.0)] {
        let rect = bounds(index);
        assert!(
            (rect.x0 - 50.0).abs() < 0.01 && (rect.y0 - expected).abs() < 0.01,
            "line {index} starts at (50, {expected}), got ({}, {})",
            rect.x0,
            rect.y0
        );
        assert!(
            rect.width() > 10.0,
            "line {index} has a real width: {}",
            rect.width()
        );
        assert!(
            (rect.height() - 12.0).abs() < 0.01,
            "line {index} is one em tall, got {}",
            rect.height()
        );
    }
    let scaled = bounds(3);
    assert!(
        (scaled.x0 - 20.0).abs() < 0.01 && (scaled.y0 - 100.0).abs() < 0.01,
        "the scaled line starts at (20, 100), got ({}, {})",
        scaled.x0,
        scaled.y0
    );
    assert!(
        (scaled.height() - 24.0).abs() < 0.01,
        "the CTM doubles the em, got {}",
        scaled.height()
    );
}

/// A one-page PDF whose content stream is `content`, on a base-14 font.
fn page_with(content: &str) -> Vec<u8> {
    let body = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, &body),
        (5, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"),
    ])
}

#[test]
fn actual_text_replaces_the_codes_inside_its_marked_content() {
    // The CV that started this work wraps each `fi` ligature in
    // `/Span << /ActualText (fi) >> BDC … EMC`: the glyph's code says one thing and
    // the writer says another, and `14.9.4` makes the writer right. Without this the
    // text reads "Pro|le" and a search for "Profile" finds nothing.
    let bytes = page_with(
        "BT /F1 12 Tf 10 50 Td (Pro) Tj /Span << /ActualText (fi) >> BDC (X) Tj EMC (le) Tj ET",
    );
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "Profile");

    // The same content without the marker keeps the code's own text.
    let plain = page_with("BT /F1 12 Tf 10 50 Td (Pro) Tj (X) Tj (le) Tj ET");
    let document = open(&plain);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "ProXle");
}

#[test]
fn a_search_finds_the_text_actual_text_states() {
    let bytes = page_with(
        "BT /F1 12 Tf 10 50 Td (Pro) Tj /Span << /ActualText (fi) >> BDC (X) Tj EMC (le) Tj ET",
    );
    let document = open(&bytes);
    let hits = document.search("Profile").expect("search");
    assert_eq!(hits.len(), 1, "the writer's text is what the reader sees");
    assert_eq!(hits[0].hits[0].text, "Profile");
    assert!(
        document.search("ProXle").expect("search").is_empty(),
        "the glyph codes are not searchable"
    );
}

#[test]
fn an_artifact_marked_with_bmc_contributes_no_text() {
    // `/Artifact` is furniture (ISO 32000-2, 14.8.2.2.2): a running header is not
    // content, so it is not text a reader can copy. The body around it still is.
    let bytes =
        page_with("BT /F1 12 Tf 10 50 Td (Body ) Tj /Artifact BMC (HDR) Tj EMC (end) Tj ET");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "Body end");
    assert!(
        document.search("HDR").expect("search").is_empty(),
        "an artefact is not searchable"
    );
}

#[test]
fn an_artifact_marked_with_bdc_and_a_property_list_contributes_no_text() {
    // The shape a writer actually emits: a `BDC` whose tag is `/Artifact` and whose
    // property list names the kind of furniture.
    let bytes = page_with(
        "BT /F1 12 Tf 10 50 Td (Stamp) Tj \
         /Artifact << /Type /Pagination >> BDC (7) Tj EMC ET",
    );
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "Stamp");
}

#[test]
fn an_artifact_nested_in_a_body_sequence_contributes_no_text() {
    let bytes = page_with(
        "BT /F1 12 Tf 10 50 Td /Document BMC (Body ) Tj /Artifact BMC (X) Tj EMC (tail) Tj EMC ET",
    );
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "Body tail");
}

#[test]
fn actual_text_inside_an_artifact_produces_nothing() {
    // The outer rule wins: an `/ActualText` inside furniture is furniture too, and
    // the stack still unwinds so the text after the sequence is read normally.
    let bytes = page_with(
        "BT /F1 12 Tf 10 50 Td (Keep ) Tj \
         /Artifact BMC /Span << /ActualText (fi) >> BDC (X) Tj EMC EMC (after) Tj ET",
    );
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "Keep after");
    assert!(
        document.search("fi").expect("search").is_empty(),
        "the replacement belongs to the furniture"
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

/// A one-page PDF whose text is shown in a composite font mapped by `to_unicode`.
///
/// The font has no program on purpose: with an Identity encoding and a `/ToUnicode`
/// map the text is readable without one, which is the case this pins.
fn composite_with(to_unicode: &str, content: &str) -> Vec<u8> {
    let stream = |body: &str| format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len());
    let map = stream(to_unicode);
    let page = stream(content);
    pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>",
        ),
        (4, &page),
        (
            5,
            "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H \
             /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>",
        ),
        (
            6,
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /DW 1000 >>",
        ),
        (7, &map),
    ])
}

/// A `/ToUnicode` CMap whose `bfchar` and `bfrange` targets are several characters.
fn ligature_map() -> &'static str {
    concat!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n",
        "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        // One code, three characters: the ligature case a writer states.
        "1 beginbfchar\n<0001> <006600660069>\nendbfchar\n",
        // A range whose array targets are two characters each.
        "1 beginbfrange\n<0010> <0012> [<00660066> <006C006C> <00740074>]\nendbfrange\n",
        "endcmap\nend\nend\n"
    )
}

#[test]
fn a_to_unicode_target_of_several_characters_is_those_characters() {
    // A ligature is one glyph whose text is several characters, and a reader who
    // copies it expects "ffi", not the single code point U+FB03 that a search for
    // "office" would never match. MuPDF makes this a flag — TEXT_PRESERVE_LIGATURES
    // = 1 is on by default and expands a fixed table of seven — while here the
    // `/ToUnicode` map decides, which is wider and needs no table.
    let bytes = composite_with(ligature_map(), "BT /F1 12 Tf 10 50 Td <0001> Tj ET");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "ffi");
    assert_eq!(
        document.search("ffi").expect("search").len(),
        1,
        "the decomposed text is searchable"
    );
}

#[test]
fn a_to_unicode_range_with_several_character_targets_is_those_characters() {
    let bytes = composite_with(
        ligature_map(),
        "BT /F1 12 Tf 10 50 Td <0010> Tj <0011> Tj <0012> Tj ET",
    );
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    assert_eq!(document.extract_text(&page).expect("text"), "fflltt");
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

#[test]
fn search_finds_a_phrase_inside_a_line_and_ignores_case() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/reading-order.pdf"
    ))
    .expect("read reading-order.pdf");
    let document = open(&bytes);
    let page = document.page(0).expect("page");
    let spans = document.spans(&page).expect("spans");

    // "first line" is one span whose space came from a `TJ` adjustment, so it is a
    // phrase over two *runs*: found, with the line's own box.
    let found = document.search("first line").expect("search");
    assert_eq!(found.len(), 1, "one page carries the phrase");
    assert_eq!(found[0].page, 0);
    assert_eq!(found[0].hits.len(), 1);
    assert_eq!(found[0].hits[0].text, "first line");
    assert_eq!(found[0].hits[0].bounds, spans[0].bounds);

    // Case is a search convenience, not a rewrite: the hit keeps the page's text.
    let upper = document.search("FIRST LINE").expect("search");
    assert_eq!(upper[0].hits[0].text, "first line");

    // Two occurrences on the page, left to right.
    let lines = document.search("line").expect("search");
    assert_eq!(lines[0].hits.len(), 4, "every line ends in 'line'");
}

#[test]
fn search_does_not_match_across_a_line_break_and_an_empty_needle_matches_nothing() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/reading-order.pdf"
    ))
    .expect("read reading-order.pdf");
    let document = open(&bytes);

    // "first line" and "second line" are adjacent lines; a reader searching for
    // their junction should not be handed a hit that spans the break.
    assert!(
        document.search("line second").expect("search").is_empty(),
        "a line break is not a space"
    );
    assert!(
        document.search("").expect("search").is_empty(),
        "an empty needle matches nothing"
    );
    assert!(
        document.search("nothing here").expect("search").is_empty(),
        "a missing phrase is not a hit"
    );
}

#[test]
fn a_match_reports_the_rectangle_of_the_characters_it_names() {
    // `text.pdf`'s fourth line is "fourth line": the word starts at character 7, so
    // its own box begins seven half-em advances (9 units each at 18 pt) after the
    // line's origin and ends four advances later -- inside the line, and narrower.
    let document = open(TEXT);
    let matches = document.search_matches("line").expect("matches");
    let fourth = matches
        .iter()
        .flat_map(|page| page.matches.iter())
        .find(|hit| hit.line.y0 > 600.0 && hit.line.y0 < 640.0)
        .expect("the fourth line's hit");
    assert_eq!(fourth.text, "line");
    assert_eq!((fourth.start, fourth.end), (7, 11));
    assert!(
        (fourth.bounds.x0 - (fourth.line.x0 + 7.0 * 9.0)).abs() < 0.01,
        "the box starts at the word's advance: {} against {}",
        fourth.bounds.x0,
        fourth.line.x0 + 63.0
    );
    assert!(
        (fourth.bounds.width() - 4.0 * 9.0).abs() < 0.01,
        "and is four advances wide: {} against 36",
        fourth.bounds.width()
    );
    assert!(
        fourth.bounds.width() < fourth.line.width(),
        "the match is narrower than its line: {} against {}",
        fourth.bounds.width(),
        fourth.line.width()
    );
}

#[test]
fn a_one_character_match_is_one_advance_wide() {
    // The short needle PyMuPDF's own docs warn about: its box is a single advance,
    // at the character's own position rather than the line's.
    let document = open(TEXT);
    let matches = document.search_matches("P").expect("matches");
    let hit = &matches[0].matches[0];
    assert_eq!(hit.text, "P");
    assert_eq!(hit.end - hit.start, 1);
    assert!(
        (hit.bounds.width() - 9.0).abs() < 0.01,
        "one half-em advance at 18 pt: {}",
        hit.bounds.width()
    );
}

#[test]
fn a_match_without_stated_widths_still_falls_inside_its_line() {
    // `text.pdf` states no `/Widths`, so the box comes from the half-em estimate --
    // it must still be inside the line's own box and start where the estimate says.
    let document = open(TEXT);
    let matches = document.search_matches("PDF").expect("matches");
    let hit = &matches[0].matches[0];
    assert!(hit.bounds.x0 >= hit.line.x0 - 0.01);
    assert!(hit.bounds.x1 <= hit.line.x1 + 0.01);
    assert_eq!((hit.start, hit.end), (7, 10));
}
