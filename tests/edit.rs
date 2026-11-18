//! Editing an opened document: the overlay (M2), and the save once it lands.
//!
//! The base document is never modified, which is the property the whole model
//! rests on (ADR 0006): a reader that keeps the `Document` sees the file as it
//! was, and only the overlay knows about the edit.

use pdf2::{Document, Error, MutableDocument, Object, ObjectId};

const TEXT: &[u8] = include_bytes!("fixtures/text.pdf");

fn opened() -> Document {
    Document::open(TEXT).expect("text.pdf opens")
}

/// The catalog's id, from the trailer's `/Root`.
fn catalog_id(document: &Document) -> ObjectId {
    match document.trailer().get(b"Root") {
        Some(Object::Reference(id)) => *id,
        other => panic!("trailer /Root is {other:?}"),
    }
}

#[test]
fn a_replaced_object_reads_back_new_from_the_overlay_and_old_from_the_base() {
    let base = opened();
    let id = catalog_id(&base);
    let original = base.get_object(id).expect("the catalog reads");
    let mut edit = MutableDocument::new(base);

    let mut replaced = pdf2::Dictionary::new();
    replaced.insert("Type", Object::Name(b"Catalog".to_vec()));
    replaced.insert("AnEdit", Object::Integer(42));
    edit.set_object(id, Object::Dictionary(replaced));

    assert_eq!(
        edit.get_object(id).expect("the overlay answers"),
        Object::Dictionary({
            let mut dict = pdf2::Dictionary::new();
            dict.insert("Type", Object::Name(b"Catalog".to_vec()));
            dict.insert("AnEdit", Object::Integer(42));
            dict
        }),
        "the overlay returns what was set"
    );
    assert_eq!(
        edit.base().get_object(id).expect("the base still answers"),
        original,
        "the base document is untouched"
    );
    assert_eq!(edit.changes().len(), 1, "one change is recorded");
}

#[test]
fn a_deleted_object_is_absent_from_the_overlay_and_still_there_in_the_base() {
    let base = opened();
    let id = catalog_id(&base);
    let original = base.get_object(id).expect("the catalog reads");
    let mut edit = MutableDocument::new(base);

    edit.delete_object(id);

    assert!(
        edit.get_object(id).is_err(),
        "a deleted object reads as absent"
    );
    assert!(
        edit.base().get_object(id).is_ok(),
        "the base still has it: the deletion is an overlay fact, not a byte edit"
    );
    assert_eq!(
        edit.deleted().iter().copied().collect::<Vec<_>>(),
        vec![id.number]
    );

    // Deleting what the overlay added leaves no trace at all: there is no `f` row
    // to write for an object the file never had.
    let added = edit.add_object(Object::Integer(7));
    edit.delete_object(added);
    assert!(!edit.changes().contains_key(&added));
    assert!(!edit.deleted().contains(&added.number));
    assert_eq!(edit.base().get_object(id).unwrap(), original);
}

#[test]
fn added_objects_take_numbers_from_size_and_never_reuse_them() {
    let base = opened();
    let size = match base.trailer().get(b"Size") {
        Some(Object::Integer(size)) => *size as u32,
        other => panic!("trailer /Size is {other:?}"),
    };
    let mut edit = MutableDocument::new(base);
    assert_eq!(edit.next_object(), size.max(edit.next_object()));

    let first = edit.add_object(Object::Integer(1));
    let second = edit.add_object(Object::Integer(2));
    assert_eq!(
        first.number, size,
        "the first added number is the base's /Size"
    );
    assert_eq!(second.number, size + 1, "numbers increase by one");
    assert_eq!(edit.get_object(first).unwrap(), Object::Integer(1));
    assert_eq!(edit.get_object(second).unwrap(), Object::Integer(2));
}

#[test]
fn an_added_page_appears_in_the_overlay_and_not_in_the_base() {
    let base = opened();
    let before = base.page_count().expect("text.pdf has pages");
    let base_first = base.page(0).expect("the first page reads");
    let mut edit = MutableDocument::new(base);

    let page = edit
        .add_page(b"BT /F1 24 Tf 72 720 Td (a page added by an edit) Tj ET")
        .expect("the page is added");

    assert_eq!(
        edit.page_count().expect("the overlay counts"),
        before + 1,
        "the page count grows in the overlay"
    );
    assert_eq!(
        edit.base().page_count().expect("the base counts"),
        before,
        "the base document still reports the old count"
    );
    let added = edit.page(before).expect("the added page is last");
    assert_eq!(
        added.id,
        Some(page),
        "the added page has the id handed back"
    );
    assert_eq!(
        added.media_box, base_first.media_box,
        "the new page inherits the document's box rather than the default"
    );
    assert_eq!(added.contents.len(), 1, "the page has its content stream");
}

#[test]
fn adding_past_the_page_ceiling_is_refused_before_anything_changes() {
    let base = opened();
    let pages = base.page_count().expect("text.pdf has pages");
    let mut edit = MutableDocument::new(base);
    edit.base().set_page_limit(pages);
    let next_before = edit.next_object();

    let error = edit
        .add_page(b"BT (over the ceiling) Tj ET")
        .expect_err("the ceiling fires");
    match error {
        Error::TooMany { what, limit } => {
            assert_eq!(what, "the page tree");
            assert_eq!(limit, pages);
        }
        other => panic!("expected Error::TooMany, got {other:?}"),
    }
    assert_eq!(edit.page_count().unwrap(), pages, "no page was added");
    assert!(edit.changes().is_empty(), "nothing was written");
    assert_eq!(edit.next_object(), next_before, "no number was consumed");
}

#[test]
fn a_saved_edit_starts_with_the_base_and_reopens_with_the_page() {
    let base = opened();
    let pages_before = base.page_count().expect("text.pdf has pages");
    let first_page_before = base
        .extract_text(&base.page(0).expect("the first page reads"))
        .expect("the first page's text extracts");
    let mut edit = MutableDocument::new(base);
    edit.add_page(b"BT /F1 18 Tf 72 720 Td (a page added by an edit) Tj ET")
        .expect("the page is added");

    let saved = edit.save_incremental().expect("the edit saves");

    assert!(
        saved.starts_with(TEXT),
        "the saved bytes start with the base bytes: {} appended",
        saved.len() - TEXT.len()
    );
    let reopened = Document::open(&saved[..]).expect("the saved file opens");
    assert_eq!(
        reopened.page_count().expect("the saved file counts"),
        pages_before + 1,
        "one page more than the base"
    );
    assert_eq!(
        reopened
            .extract_text(&reopened.page(0).expect("the first page reads"))
            .expect("the first page's text extracts"),
        first_page_before,
        "the first page still extracts what it did"
    );
    let added = reopened.page(pages_before).expect("the added page is last");
    assert_eq!(
        reopened
            .extract_text(&added)
            .expect("the added page's text extracts"),
        "a page added by an edit",
        "the added page's own text extracts"
    );
}

#[test]
fn a_deleted_object_is_a_free_row_and_the_saved_file_still_opens() {
    // A base with an object nothing references: deleting a live one would leave
    // the page pointing at a free entry, which is a broken document, not a test.
    // The builder gives a file whose spare object can go without touching the
    // page's text.
    let mut builder = Document::new();
    builder.add_page(
        pdf2::Rect::new(0.0, 0.0, 612.0, 792.0),
        "BT /F1 18 Tf 72 700 Td (a page worth keeping) Tj ET",
    );
    let spare = builder.add_object(Object::Integer(1234));
    let base_bytes = builder.save().expect("the base saves");
    let base = Document::open(&base_bytes[..]).expect("the base opens");
    assert!(base.contains_object(spare), "the spare object is live");
    let text_before = base
        .extract_text(&base.page(0).expect("the page reads"))
        .expect("the page's text extracts");

    let mut edit = MutableDocument::new(base);
    edit.delete_object(ObjectId::new(spare, 0));
    assert!(
        edit.get_object(ObjectId::new(spare, 0)).is_err(),
        "deleted in the overlay"
    );

    let saved = edit.save_incremental().expect("the edit saves");
    assert!(
        saved.starts_with(&base_bytes[..]),
        "the saved bytes start with the base bytes"
    );
    let section = String::from_utf8_lossy(&saved[base_bytes.len()..]).to_string();
    assert!(
        section.contains("0000000000 00001 f"),
        "the deletion is a free row in the appended section:\n{section}"
    );

    let reopened = Document::open(&saved[..]).expect("the saved file opens");
    assert!(
        !reopened.contains_object(spare),
        "the deleted number is free in the saved file"
    );
    assert_eq!(
        reopened
            .extract_text(&reopened.page(0).expect("the page reads"))
            .expect("the page's text still extracts"),
        text_before,
        "the page is untouched by the deletion"
    );
}

#[test]
fn a_saved_edit_can_be_opened_edited_and_saved_again() {
    // A second update over the first: the appended section has to be readable as
    // a base, with its own `/Prev` chain, or editing is a one-shot operation.
    let first = {
        let mut edit = MutableDocument::new(opened());
        edit.add_page(b"BT /F1 18 Tf 72 700 Td (first edit) Tj ET")
            .expect("the first page is added");
        edit.save_incremental().expect("the first edit saves")
    };
    let second = {
        let base = Document::open(&first[..]).expect("the first save opens");
        let mut edit = MutableDocument::new(base);
        edit.add_page(b"BT /F1 18 Tf 72 600 Td (second edit) Tj ET")
            .expect("the second page is added");
        edit.save_incremental().expect("the second edit saves")
    };

    assert!(
        second.starts_with(&first[..]),
        "each save appends to what it opened"
    );
    let reopened = Document::open(&second[..]).expect("the second save opens");
    assert_eq!(
        reopened.page_count().expect("the second save counts"),
        3,
        "one page from the base and one from each edit"
    );
    let texts: Vec<String> = (0..3)
        .map(|index| {
            reopened
                .extract_text(&reopened.page(index).expect("the page reads"))
                .expect("the page's text extracts")
        })
        .collect();
    assert!(
        texts[1].contains("first edit"),
        "the first edit is intact: {texts:?}"
    );
    assert!(
        texts[2].contains("second edit"),
        "the second edit is there: {texts:?}"
    );
}

/// The offset the newest `startxref` names, read from the raw bytes.
fn last_startxref(bytes: &[u8]) -> i64 {
    let at = bytes
        .windows(b"startxref".len())
        .rposition(|window| window == b"startxref")
        .expect("a startxref");
    let rest = &bytes[at + b"startxref".len()..];
    let start = rest
        .iter()
        .position(|byte| byte.is_ascii_digit())
        .expect("a startxref offset");
    let end = start
        + rest[start..]
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .expect("the end of the offset");
    core::str::from_utf8(&rest[start..end])
        .expect("ASCII digits")
        .parse()
        .expect("a number")
}

#[test]
fn the_appended_trailer_names_the_previous_section_and_keeps_the_first_identifier() {
    const TWIN: &[u8] = include_bytes!("fixtures/plain-twin.pdf");
    let base = Document::open(TWIN).expect("plain-twin.pdf opens");
    let base_ids = match base.trailer().get(b"ID") {
        Some(Object::Array(ids)) => ids.clone(),
        other => panic!("the base /ID is {other:?}"),
    };
    assert_eq!(base_ids.len(), 2, "the base has two identifier strings");
    let mut edit = MutableDocument::new(base);
    edit.add_page(b"BT /F1 18 Tf 72 700 Td (added) Tj ET")
        .expect("the page is added");
    let expected_size = edit.next_object();

    let saved = edit.save_incremental().expect("the edit saves");

    // `/Prev` is the base's last `startxref`, read from the bytes rather than
    // remembered: that is the number a reader follows into the previous section.
    let appended = &saved[TWIN.len()..];
    let section = String::from_utf8_lossy(appended).to_string();
    assert!(
        section.contains(&format!("/Prev {}", last_startxref(TWIN))),
        "the trailer names the base's last section:\n{section}"
    );

    let reopened = Document::open(&saved[..]).expect("the saved file opens");
    let trailer = reopened.trailer();
    assert_eq!(
        trailer.get(b"Prev"),
        Some(&Object::Integer(last_startxref(TWIN))),
        "the newest trailer's /Prev is the base's startxref"
    );
    assert_eq!(
        trailer.get(b"Size"),
        Some(&Object::Integer(i64::from(expected_size))),
        "the newest trailer's /Size covers the highest number written"
    );

    // `14.4`: the first string is permanent — an encrypted file's key derives
    // from it — and the second is the one an update replaces.
    let ids = match trailer.get(b"ID") {
        Some(Object::Array(ids)) => ids.clone(),
        other => panic!("the saved /ID is {other:?}"),
    };
    assert_eq!(ids.len(), 2, "still two identifier strings");
    assert_eq!(ids[0], base_ids[0], "the first string is byte-identical");
    assert_ne!(ids[1], base_ids[1], "the second string changed");
}

#[test]
fn a_base_without_an_identifier_gets_no_identifier_from_the_update() {
    let base = opened();
    assert!(
        base.trailer().get(b"ID").is_none(),
        "text.pdf carries no /ID"
    );
    let mut edit = MutableDocument::new(base);
    edit.add_page(b"BT /F1 18 Tf 72 700 Td (added) Tj ET")
        .expect("the page is added");
    let saved = edit.save_incremental().expect("the edit saves");

    let section = String::from_utf8_lossy(&saved[TEXT.len()..]).to_string();
    assert!(
        !section.contains("/ID"),
        "an update invents no identifier:\n{section}"
    );
    let reopened = Document::open(&saved[..]).expect("the saved file opens");
    assert!(
        reopened.trailer().get(b"ID").is_none(),
        "the saved file has no /ID either"
    );
}

#[test]
fn a_save_past_the_object_ceiling_returns_too_many_and_produces_no_bytes() {
    let base = opened();
    let pages = base.page_count().expect("text.pdf has pages");
    let mut edit = MutableDocument::new(base);
    // Three pages is six objects: a page and its content stream each, plus the
    // rewritten page tree. A ceiling of two is below that and above nothing.
    edit.base().set_object_limit(2);
    for index in 0..3 {
        edit.add_page(format!("BT /F1 18 Tf 72 700 Td (page {index}) Tj ET"))
            .expect("the page is added in memory");
    }
    assert!(
        edit.changes().len() > 2,
        "the overlay holds more than the ceiling: {}",
        edit.changes().len()
    );

    let error = edit
        .save_incremental()
        .expect_err("the ceiling fires before any bytes");
    match error {
        Error::TooMany { what, limit } => {
            assert_eq!(what, "the number of objects an update writes");
            assert_eq!(limit, 2);
        }
        other => panic!("expected Error::TooMany, got {other:?}"),
    }
    assert_eq!(
        edit.page_count().expect("the overlay still counts"),
        pages + 3,
        "nothing was lost: the ceiling stopped the save, not the edit"
    );
}
