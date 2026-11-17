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
