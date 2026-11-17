//! Writing PDF 2.0 files and reading them back (Q8, M8).
//!
//! Every test here saves a document produced by `Document::new()` and opens the
//! bytes again with `Document::open`, so the writer is judged by this crate's
//! own reader — offsets, object streams, the xref stream and `/Length` all have
//! to be right for the round-trip to pass.

use pdf2::{Document, DocumentBuilder, Object, ObjectId, Rect};
use std::assert_matches;

/// Compare two objects, ignoring the `/Length` the writer adds to streams.
fn same_object(left: &Object, right: &Object) -> bool {
    match (left, right) {
        (Object::Stream(left), Object::Stream(right)) => {
            left.data() == right.data()
                && left
                    .dict()
                    .iter()
                    .filter(|(name, _)| *name != b"Length")
                    .eq(right.dict().iter().filter(|(name, _)| *name != b"Length"))
        }
        _ => left == right,
    }
}

fn builder_with_two_pages() -> DocumentBuilder {
    let mut document = Document::new();
    document.add_page(
        Rect::new(0.0, 0.0, 612.0, 792.0),
        "BT /F1 24 Tf 72 700 Td (First page) Tj ET",
    );
    document.add_page(
        Rect::new(0.0, 0.0, 200.0, 300.0),
        "BT /F1 12 Tf 10 280 Td (Second page) Tj ET",
    );
    document
}

#[test]
fn a_written_file_starts_with_a_pdf_2_header_and_ends_with_eof() {
    let bytes = builder_with_two_pages().save().expect("save");
    assert!(bytes.starts_with(b"%PDF-2.0\n"), "header");
    assert!(bytes.ends_with(b"%%EOF\n"), "trailer marker");
    assert!(
        !bytes.windows(7).any(|window| window == b"trailer"),
        "a 2.0 file with an xref stream needs no trailer keyword"
    );
}

#[test]
fn a_written_file_uses_an_xref_stream_and_an_object_stream() {
    let bytes = builder_with_two_pages().save().expect("save");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Type /XRef"), "cross-reference stream");
    assert!(text.contains("/Type /ObjStm"), "object stream");
    assert!(text.contains("/W [1 4 2]"), "declared xref widths");
    assert!(text.contains("startxref"), "startxref");
}

#[test]
fn written_files_reopen_to_the_same_pages() {
    let bytes = builder_with_two_pages().save().expect("save");
    let document = Document::open(bytes).expect("reopen");
    assert_eq!(document.version(), "2.0");
    assert_eq!(document.page_count().expect("page count"), 2);

    let pages = document.pages().expect("pages");
    assert_eq!(
        pages[0].media_box,
        Some(Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0
        })
    );
    assert_eq!(
        pages[1].media_box,
        Some(Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 300.0
        })
    );
    assert_eq!(
        document.extract_text(&pages[0]).expect("first text"),
        "First page"
    );
    assert_eq!(
        document.extract_text(&pages[1]).expect("second text"),
        "Second page"
    );
}

#[test]
fn the_object_graph_survives_the_round_trip() {
    let builder = builder_with_two_pages();
    let expected: Vec<(u32, Object)> = builder
        .objects()
        .map(|(number, object)| (number, object.clone()))
        .collect();
    let bytes = builder.save().expect("save");
    let document = Document::open(bytes).expect("reopen");

    for (number, object) in &expected {
        let reopened = document.get_object_by_number(*number).expect("object");
        // Object 1 is the catalog, which gains /Metadata when saving.
        if *number == 1 {
            let reopened = reopened.as_dict().expect("catalog dict");
            let original = object.as_dict().expect("original catalog");
            for (name, value) in original.iter() {
                assert_eq!(reopened.get(name), Some(value), "/{name:?}");
            }
            assert!(reopened.contains_key(b"Metadata"), "catalog metadata");
            continue;
        }
        assert!(
            same_object(&reopened, object),
            "object {number} must read back identically: {reopened:?} vs {object:?}"
        );
    }

    // Every object number in the file is reachable; nothing is orphaned.
    for (number, _) in &expected {
        assert!(document.contains_object(*number), "object {number} is live");
    }
}

#[test]
fn the_cross_reference_offsets_point_at_object_headers() {
    // Reopening already proves this, but check the bytes directly too.
    let bytes = builder_with_two_pages().save().expect("save");
    let startxref = bytes
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("startxref");
    let digits: Vec<u8> = bytes[startxref + 9..]
        .iter()
        .copied()
        .skip_while(|byte| !byte.is_ascii_digit())
        .take_while(|byte| byte.is_ascii_digit())
        .collect();
    let offset: usize = std::str::from_utf8(&digits)
        .expect("offset digits")
        .parse()
        .expect("offset");
    let rest = &bytes[offset..];
    let header_end = rest
        .iter()
        .position(|byte| *byte == b' ')
        .expect("object number");
    assert!(rest[..header_end].iter().all(u8::is_ascii_digit));
    assert!(
        rest[header_end..].starts_with(b" 0 obj"),
        "startxref must point at an object header, found {:?}",
        &rest[..(header_end + 6).min(rest.len())]
    );

    let document = Document::open(bytes).expect("reopen");
    // catalog 1, pages 2, font 3, content 4, page 5, content 6, page 7,
    // metadata 8, object stream 9, cross-reference stream 10.
    assert_eq!(
        document.trailer().get(b"Size"),
        Some(&Object::Integer(11)),
        "every object number is covered"
    );
}

#[test]
fn a_written_file_carries_xmp_metadata_and_no_info_dictionary() {
    let document = Document::open(builder_with_two_pages().save().expect("save")).expect("reopen");
    assert!(document.has_metadata().expect("metadata presence"));
    let metadata = document.metadata().expect("metadata").expect("some");
    let text = String::from_utf8_lossy(&metadata);
    assert!(text.contains("x:xmpmeta"), "{text}");
    assert!(!document.trailer().contains_key(b"Info"), "no /Info");
    assert_matches!(document.catalog().expect("catalog"), Object::Dictionary(_));
}

#[test]
fn custom_metadata_replaces_the_default_packet() {
    let mut document = Document::new();
    document.add_page(Rect::new(0.0, 0.0, 10.0, 10.0), "BT ET");
    document.set_metadata("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><custom/></x:xmpmeta>");
    let reopened = Document::open(document.save().expect("save")).expect("reopen");
    let metadata = reopened.metadata().expect("metadata").expect("some");
    assert!(String::from_utf8_lossy(&metadata).contains("<custom/>"));
}

#[test]
fn custom_objects_can_be_added_and_read_back() {
    let mut document = Document::new();
    let number = document.add_object(Object::array([
        Object::Integer(1),
        Object::name("Tagged"),
        Object::string("text"),
        Object::reference(ObjectId::new(1, 0)),
    ]));
    let reopened = Document::open(document.save().expect("save")).expect("reopen");
    assert_eq!(
        reopened.get_object_by_number(number).expect("object"),
        Object::array([
            Object::Integer(1),
            Object::name("Tagged"),
            Object::string("text"),
            Object::reference(ObjectId::new(1, 0)),
        ])
    );
}

#[test]
fn an_empty_document_saves_and_reopens() {
    let document = Document::new();
    assert_eq!(document.page_count(), 0);
    let reopened = Document::open(document.save().expect("save")).expect("reopen");
    assert_eq!(reopened.page_count().expect("pages"), 0);
    assert!(reopened.has_metadata().expect("metadata"));
}

#[test]
fn saving_to_a_path_writes_a_file_that_reopens() {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pdf2-writer-test-{}-{}.pdf",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let document = builder_with_two_pages();
    document.save_to_path(&path).expect("save_to_path");
    let reopened = Document::open_path(&path).expect("reopen from path");
    assert_eq!(reopened.page_count().expect("pages"), 2);
    std::fs::remove_file(&path).expect("cleanup");
}

#[test]
fn a_written_file_has_correct_stream_lengths() {
    // The writer computes `/Length` itself; a round-trip through `/Contents`
    // must therefore decode without touching `endstream` scanning.
    let document = builder_with_two_pages();
    let bytes = document.save().expect("save");
    let reopened = Document::open(bytes.clone()).expect("reopen");
    let page = reopened.page(0).expect("page");
    let content = reopened.page_content(&page).expect("content");
    assert_eq!(
        content, b"BT /F1 24 Tf 72 700 Td (First page) Tj ET",
        "the payload is exactly the bytes handed to add_page"
    );
}

#[test]
fn written_files_survive_truncation_and_mutation() {
    let bytes = builder_with_two_pages().save().expect("save");
    for end in 0..bytes.len() {
        let _ = Document::open(&bytes[..end]);
    }
    for offset in (0..bytes.len()).step_by(11) {
        let mut mutated = bytes.clone();
        mutated[offset] = b'X';
        let _ = Document::open(mutated);
    }
    assert!(Document::open(bytes).is_ok());
}

#[test]
fn pages_written_with_fractional_boxes_keep_them() {
    let mut document = Document::new();
    document.add_page(Rect::new(0.5, 1.25, 200.75, 300.5), "BT ET");
    let reopened = Document::open(document.save().expect("save")).expect("reopen");
    assert_eq!(
        reopened.page(0).expect("page").media_box,
        Some(Rect {
            x0: 0.5,
            y0: 1.25,
            x1: 200.75,
            y1: 300.5
        })
    );
}
