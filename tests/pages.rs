//! The document model: page tree inheritance and XMP metadata (Q6, M6).
//!
//! `tests/fixtures/page-tree.pdf` states `/MediaBox` and `/Resources` once at
//! the root, `/Rotate 90` on an intermediate node, and overrides on individual
//! pages, so every inheritance rule is exercised by one file.

use pdf2::{Document, Error, Object, ObjectId, Rect};

const PAGE_TREE: &[u8] = include_bytes!("fixtures/page-tree.pdf");
const TWIN: &[u8] = include_bytes!("fixtures/classic-twin.pdf");
const XREF_STREAM: &[u8] = include_bytes!("fixtures/xref-stream.pdf");

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

fn open(objects: &[(u32, &str)]) -> Document {
    Document::open(pdf(objects)).expect("document opens")
}

fn page_tree() -> Document {
    Document::open(PAGE_TREE).expect("page-tree.pdf opens")
}

#[test]
fn the_page_count_comes_from_walking_the_tree() {
    let document = page_tree();
    assert_eq!(document.page_count().expect("page count"), 3);
    assert_eq!(document.pages().expect("pages").len(), 3);
}

#[test]
fn pages_keep_document_order() {
    let document = page_tree();
    let ids: Vec<Option<ObjectId>> = document
        .pages()
        .expect("pages")
        .iter()
        .map(|page| page.id)
        .collect();
    assert_eq!(
        ids,
        vec![
            Some(ObjectId::new(5, 0)),
            Some(ObjectId::new(6, 0)),
            Some(ObjectId::new(7, 0))
        ],
        "root /Kids is [3 0 R 7 0 R] and node 3's kids are [5 0 R 6 0 R]"
    );
}

#[test]
fn a_page_can_override_the_inherited_media_box() {
    let page = page_tree().page(0).expect("first page");
    assert_eq!(
        page.media_box,
        Some(Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 200.0,
            y1: 300.0
        })
    );
    assert_eq!(
        page.crop_box, page.media_box,
        "/CropBox defaults to the media box"
    );
    assert_eq!(page.rotate, 90, "inherited from node 3");
    assert!(page.contains(100.0, 150.0));
    assert!(!page.contains(300.0, 150.0));
}

#[test]
fn an_inherited_media_box_and_an_own_crop_box_combine() {
    let page = page_tree().page(1).expect("second page");
    assert_eq!(
        page.media_box,
        Some(Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0
        }),
        "inherited from the root node"
    );
    assert_eq!(
        page.crop_box,
        Some(Rect {
            x0: 10.0,
            y0: 20.0,
            x1: 190.0,
            y1: 280.0
        })
    );
    assert_eq!(page.rotate, 90);
}

#[test]
fn a_page_can_override_the_inherited_rotation() {
    let page = page_tree().page(2).expect("third page");
    assert_eq!(
        page.rotate, 180,
        "/Rotate 180 on the page beats node 3's 90"
    );
    assert_eq!(page.media_box.map(|rect| rect.height()), Some(792.0));
}

#[test]
fn resources_are_inherited_from_the_root_node() {
    let document = page_tree();
    for page in document.pages().expect("pages") {
        let resources = page.resources.expect("resources are inherited");
        let fonts = resources.get(b"Font").expect("/Font");
        assert_eq!(
            fonts,
            &Object::Dictionary({
                let mut fonts = pdf2::Dictionary::new();
                fonts.insert("F1", Object::reference(ObjectId::new(8, 0)));
                fonts
            })
        );
    }
}

#[test]
fn a_page_exposes_its_content_streams_and_their_bytes() {
    let document = page_tree();
    let page = document.page(0).expect("first page");
    assert_eq!(page.contents, vec![ObjectId::new(4, 0)]);
    assert_eq!(
        document.page_content(&page).expect("content"),
        b"BT /F1 18 Tf 20 40 Td (page one) Tj ET"
    );
    // Pages without /Contents have no content at all.
    let page = document.page(1).expect("second page");
    assert!(page.contents.is_empty());
    assert!(document.page_content(&page).expect("empty").is_empty());
}

#[test]
fn xmp_metadata_is_reported_and_decoded() {
    let document = page_tree();
    assert!(document.has_metadata().expect("presence"));
    let metadata = document.metadata().expect("metadata").expect("some");
    let text = String::from_utf8_lossy(&metadata);
    assert!(text.contains("x:xmpmeta"), "{text}");
    assert!(text.contains("<?xpacket"), "{text}");
}

#[test]
fn a_document_without_metadata_says_so() {
    let document = Document::open(TWIN).expect("twin opens");
    assert!(!document.has_metadata().expect("presence"));
    assert!(document.metadata().expect("metadata").is_none());
}

#[test]
fn pages_stored_in_object_streams_are_walked_too() {
    let document = Document::open(XREF_STREAM).expect("xref stream opens");
    assert_eq!(document.page_count().expect("page count"), 1);
    let page = document.page(0).expect("page");
    assert_eq!(page.id, Some(ObjectId::new(3, 0)));
    assert_eq!(
        page.media_box,
        Some(Rect {
            x0: 0.0,
            y0: 0.0,
            x1: 612.0,
            y1: 792.0
        })
    );
    assert_eq!(page.contents, vec![ObjectId::new(4, 0)]);
    assert_eq!(
        document.page_content(&page).expect("content"),
        b"BT /F1 24 Tf 72 700 Td (Hello, PDF 2.0) Tj ET"
    );
}

#[test]
fn the_page_count_does_not_trust_slash_count() {
    // /Count says 5 but the tree holds two pages.
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 5 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 612 792] >>"),
        (4, "<< /Type /Page /MediaBox [0 0 300 300] >>"),
    ]);
    assert_eq!(document.page_count().expect("page count"), 2);
}

#[test]
fn a_page_dictionary_directly_in_kids_is_a_page() {
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [<< /Type /Page /MediaBox [0 0 10 20] >>] /Count 1 >>",
        ),
    ]);
    let pages = document.pages().expect("pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].id, None, "a direct page has no object id");
    assert_eq!(pages[0].media_box.map(|rect| rect.height()), Some(20.0));
}

#[test]
fn rotations_are_normalised_on_the_way_out() {
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] /Rotate 450 >>"),
    ]);
    assert_eq!(document.page(0).expect("page").rotate, 90);
}

#[test]
fn a_cyclic_page_tree_is_an_error() {
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [2 0 R] /Count 1 >>"),
    ]);
    assert!(matches!(document.pages(), Err(Error::Malformed { .. })));
}

#[test]
fn a_deeply_nested_page_tree_is_an_error_not_a_stack_overflow() {
    let mut objects: Vec<(u32, String)> = vec![(1, "<< /Type /Catalog /Pages 2 0 R >>".into())];
    let depth = pdf2::page::MAX_PAGE_TREE_DEPTH + 8;
    for level in 0..depth {
        let number = 2 + level as u32;
        objects.push((
            number,
            format!("<< /Type /Pages /Kids [{} 0 R] /Count 1 >>", number + 1),
        ));
    }
    let last = 2 + depth as u32;
    objects.push((last, "<< /Type /Page /MediaBox [0 0 10 10] >>".into()));
    let borrowed: Vec<(u32, &str)> = objects
        .iter()
        .map(|(number, body)| (*number, body.as_str()))
        .collect();
    let document = Document::open(pdf(&borrowed)).expect("document opens");
    assert!(matches!(document.pages(), Err(Error::Malformed { .. })));
}

#[test]
fn broken_page_entries_are_errors() {
    // A box with three numbers.
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10] >>"),
    ]);
    assert!(document.pages().is_err());

    // A node that is neither /Pages nor /Page.
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Font >>"),
    ]);
    assert!(document.pages().is_err());

    // /Kids that is not an array.
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids 3 0 R /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    assert!(document.pages().is_err());

    // A page index past the end.
    let document = open(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /MediaBox [0 0 10 10] >>"),
    ]);
    assert!(document.page(1).is_err());
    assert_eq!(document.page_count().expect("count"), 1);
}

#[test]
fn a_catalog_without_pages_is_an_error() {
    let document = open(&[(1, "<< /Type /Catalog >>")]);
    assert!(document.pages().is_err());
    assert!(!document.has_metadata().expect("no metadata"));
}
