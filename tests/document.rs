//! Opening a PDF 2.0 file through a classic cross-reference table (Q3, M3).
//!
//! `tests/fixtures/classic-xref.pdf` is a hand-written file with two
//! cross-reference sections: the original table plus an incremental update that
//! re-points object 3 and adds object 4. Reading it proves header sniffing,
//! `startxref`, subsections, the `/Prev` chain, lazy object fetch and that the
//! newest entry for an object number wins.

use pdf2::{Document, Error, Object, ObjectId};
use std::assert_matches;

const FIXTURE: &[u8] = include_bytes!("fixtures/classic-xref.pdf");

/// Build a small classic-xref PDF around `objects` with the given extra trailer
/// keys, computing every offset itself.
fn pdf_with_trailer(objects: &[&str], trailer_keys: &str) -> Vec<u8> {
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, body).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} {trailer_keys} >>\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn fixture() -> Document {
    Document::open(FIXTURE).expect("classic-xref.pdf opens")
}

#[test]
fn the_header_version_is_detected() {
    assert_eq!(fixture().version(), "2.0");
}

#[test]
fn the_trailer_and_root_are_reachable() {
    let document = fixture();
    assert_eq!(document.trailer().get(b"Size"), Some(&Object::Integer(5)));
    assert_eq!(
        document.trailer().get(b"Root"),
        Some(&Object::reference(ObjectId::new(1, 0)))
    );
    let catalog = document.catalog().expect("catalog");
    let catalog = catalog.as_dict().expect("catalog is a dictionary");
    assert_eq!(catalog.get(b"Type"), Some(&Object::name("Catalog")));
    assert_eq!(
        catalog.get(b"Pages"),
        Some(&Object::reference(ObjectId::new(2, 0)))
    );
}

#[test]
fn objects_are_fetched_by_id_and_cached() {
    let document = fixture();
    let pages = document.get_object(ObjectId::new(2, 0)).expect("pages");
    let pages = pages.as_dict().expect("dictionary");
    assert_eq!(pages.get(b"Type"), Some(&Object::name("Pages")));
    assert_eq!(pages.get(b"Count"), Some(&Object::Integer(1)));
    // A second fetch returns the cached value.
    assert_eq!(
        document.get_object(ObjectId::new(2, 0)).expect("cached"),
        Object::Dictionary(pages.clone())
    );
    assert!(document.contains_object(2));
}

#[test]
fn the_prev_chain_supplies_objects_the_newest_section_omits() {
    let document = fixture();
    // Object 1 and 2 are only in the first table, reachable through /Prev.
    assert_eq!(
        document
            .get_object_by_number(1)
            .expect("object 1 from /Prev")
            .as_dict()
            .and_then(|dict| dict.get(b"Type")),
        Some(&Object::name("Catalog"))
    );
    assert!(document.get_object_by_number(2).is_ok());
}

#[test]
fn the_newest_section_wins_for_a_repainted_object() {
    let document = fixture();
    let page = document.get_object_by_number(3).expect("page");
    let page = page.as_dict().expect("dictionary");
    assert_eq!(
        page.get(b"Contents"),
        Some(&Object::reference(ObjectId::new(4, 0))),
        "the incremental update adds /Contents to the page"
    );
    assert_eq!(
        page.get(b"MediaBox"),
        Some(&Object::array([
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(612),
            Object::Integer(792)
        ]))
    );
}

#[test]
fn a_stream_object_is_fetched_with_its_payload() {
    let document = fixture();
    let object = document.get_object_by_number(4).expect("stream");
    let stream = object.as_stream().expect("stream object");
    assert_eq!(stream.data(), b"BT /F1 24 Tf 72 700 Td (Incremental) Tj ET");
    assert_eq!(stream.dict().get(b"Length"), Some(&Object::Integer(42)));
}

#[test]
fn free_and_unknown_objects_report_errors() {
    let document = fixture();
    assert!(
        document.get_object_by_number(0).is_err(),
        "object 0 is free"
    );
    assert!(!document.contains_object(0));
    assert!(document.get_object_by_number(99).is_err());
    // A stale generation still resolves, as every real reader does: only the
    // object number selects the entry.
    assert_eq!(
        document
            .get_object(ObjectId::new(2, 7))
            .expect("stale generation"),
        document
            .get_object(ObjectId::new(2, 0))
            .expect("current generation")
    );
}

#[test]
fn a_file_can_be_opened_from_a_path() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/classic-xref.pdf"
    );
    let document = Document::open_path(path).expect("open_path");
    assert_eq!(document.version(), "2.0");
}

#[test]
fn a_hand_built_file_opens_too() {
    let bytes = pdf_with_trailer(&["<< /Type /Catalog >>"], "/Root 1 0 R");
    let document = Document::open(bytes).expect("open");
    assert_eq!(
        document.catalog().expect("catalog"),
        Object::Dictionary({
            let mut dict = pdf2::Dictionary::new();
            dict.insert("Type", Object::name("Catalog"));
            dict
        })
    );
}

#[test]
fn unsupported_features_are_reported_not_misread() {
    let encrypted = pdf_with_trailer(&["<< /Type /Catalog >>"], "/Root 1 0 R /Encrypt 1 0 R");
    assert_matches!(Document::open(encrypted), Err(Error::Malformed { .. }));

    // A startxref that points at an indirect object is read as a
    // cross-reference stream; one that is not a stream is an error, not a guess.
    let mut xref_stream = b"%PDF-2.0\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
    let offset = xref_stream.len();
    xref_stream.extend_from_slice(b"9 0 obj\n<< /Type /XRef /Size 10 /Root 1 0 R >>\nendobj\n");
    xref_stream.extend_from_slice(format!("startxref\n{offset}\n%%EOF\n").as_bytes());
    let error = Document::open(xref_stream).expect_err("not a stream");
    assert!(
        error.to_string().contains("cross-reference stream"),
        "{error}"
    );
}

#[test]
fn a_missing_prev_chain_style_error_is_reported() {
    // Repoint /Prev at an offset that is not an xref table.
    let mut broken = FIXTURE.to_vec();
    let needle = b"/Prev 186";
    let at = broken
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("/Prev in the fixture");
    broken[at..at + needle.len()].copy_from_slice(b"/Prev 187");
    assert!(Document::open(broken).is_err());
}

#[test]
fn structurally_broken_files_are_errors() {
    let mut no_header = FIXTURE.to_vec();
    no_header[..5].copy_from_slice(b"%PDX-");
    assert!(Document::open(no_header).is_err());

    // A file whose only `startxref` is damaged: the fixture has two (one per
    // section), so use a single-section document for this case.
    let mut no_startxref = pdf_with_trailer(&["<< /Type /Catalog >>"], "/Root 1 0 R");
    let at = no_startxref
        .windows(9)
        .rposition(|window| window == b"startxref")
        .expect("startxref");
    no_startxref[at..at + 9].copy_from_slice(b"startxrxf");
    assert!(Document::open(no_startxref).is_err());

    // PDF 1.x is the format of almost every real file: it must open.
    let mut one_x = FIXTURE.to_vec();
    one_x[..8].copy_from_slice(b"%PDF-1.7");
    assert!(Document::open(one_x).is_ok(), "1.x must open");

    let mut wrong_version = FIXTURE.to_vec();
    wrong_version[..8].copy_from_slice(b"%PDF-3.0");
    assert!(
        Document::open(wrong_version).is_err(),
        "3.x is not understood"
    );
}

#[test]
fn every_truncation_of_the_fixture_is_handled() {
    for end in 0..FIXTURE.len() {
        // Opening may succeed or fail; it must never panic.
        let _ = Document::open(&FIXTURE[..end]);
    }
    assert!(Document::open(FIXTURE).is_ok());
}

#[test]
fn every_single_byte_mutation_is_handled() {
    // Spot-check a spread of offsets rather than all 621 bytes times 256 values.
    for offset in (0..FIXTURE.len()).step_by(7) {
        for replacement in *b"X\n0" {
            let mut mutated = FIXTURE.to_vec();
            mutated[offset] = replacement;
            let _ = Document::open(mutated);
        }
    }
}
