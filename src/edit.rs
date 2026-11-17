//! Editing an opened document (§7.5.6 Incremental Updates).
//!
//! An edit is a copy-on-write overlay: [`MutableDocument`] holds the changed and
//! deleted objects, the base [`Document`] keeps answering with the file as it was
//! read, and a save appends to the base's bytes instead of rewriting them. The
//! model is decided in `docs/adr/0006-editing-mutation-model.md`.
//!
//! ```
//! # fn main() -> pdf2::Result<()> {
//! let base = pdf2::Document::open(std::fs::read("tests/fixtures/text.pdf")?)?;
//! let before = base.page_count()?;
//! let mut edit = pdf2::MutableDocument::new(base);
//! edit.add_page(b"BT /F1 12 Tf 72 720 Td (a page added by an edit) Tj ET")?;
//! assert_eq!(edit.page_count()?, before + 1);
//! assert_eq!(edit.base().page_count()?, before);
//! # Ok(())
//! # }
//! ```
//!
//! Reading goes through the overlay first and the base second, so every reader
//! that takes a [`Document`] keeps working on the unedited file — the base is
//! never modified.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;

use crate::document::{Document, ObjectSource};
use crate::error::{Error, Result};
use crate::object::{Dictionary, Object, ObjectId, Stream};
use crate::page::Page;

/// A document being edited, overlaid on the one that was opened.
///
/// Changes are recorded by object id; nothing is written until a save, and the
/// base document stays readable and unchanged (ADR 0006).
#[derive(Debug)]
pub struct MutableDocument {
    base: Document,
    changes: BTreeMap<ObjectId, Object>,
    deleted: BTreeSet<u32>,
    next_object: u32,
}

impl MutableDocument {
    /// Overlay edits on an opened document.
    ///
    /// New object numbers start at the base's `/Size`, which is one past the
    /// highest number the file used, and never reuse one: a saved file grows
    /// monotonically (§7.5.6).
    pub fn new(base: Document) -> Self {
        let size = match base.trailer().get(b"Size") {
            Some(Object::Integer(size)) if *size > 0 => *size as u32,
            _ => 1,
        };
        let highest = base
            .base_xref()
            .keys()
            .next_back()
            .map_or(0, |number| number + 1);
        Self {
            changes: BTreeMap::new(),
            deleted: BTreeSet::new(),
            next_object: size.max(highest),
            base,
        }
    }

    /// The document as it was opened, unchanged.
    pub fn base(&self) -> &Document {
        &self.base
    }

    /// The objects this edit has changed or added, by id.
    pub fn changes(&self) -> &BTreeMap<ObjectId, Object> {
        &self.changes
    }

    /// The object numbers this edit has deleted.
    pub fn deleted(&self) -> &BTreeSet<u32> {
        &self.deleted
    }

    /// The number the next added object will take.
    pub fn next_object(&self) -> u32 {
        self.next_object
    }

    /// Fetch an object, through the overlay first and the base second.
    ///
    /// A deleted object reads as absent — the same answer a free entry gives
    /// from the base document.
    pub fn get_object(&self, id: ObjectId) -> Result<Object> {
        if self.deleted.contains(&id.number) {
            return Err(Error::malformed("object was deleted by an edit"));
        }
        match self.changes.get(&id) {
            Some(object) => Ok(object.clone()),
            None => self.base.get_object(id),
        }
    }

    /// Replace or add the object at `id`.
    ///
    /// The id keeps its number, which is what an incremental update relies on
    /// (`7.5.6` NOTE 3: the most recent copy of a number wins).
    pub fn set_object(&mut self, id: ObjectId, object: Object) {
        self.deleted.remove(&id.number);
        self.next_object = self.next_object.max(id.number + 1);
        self.changes.insert(id, object);
    }

    /// Add an object under a fresh number and return its id.
    pub fn add_object(&mut self, object: Object) -> ObjectId {
        let id = ObjectId::new(self.next_object, 0);
        self.next_object += 1;
        self.changes.insert(id, object);
        id
    }

    /// Delete an object.
    ///
    /// An object this edit added is dropped outright; one the base holds becomes
    /// an `f` row in the appended section, which is how a deletion is expressed
    /// without touching the original bytes.
    pub fn delete_object(&mut self, id: ObjectId) {
        let was_added = self.changes.remove(&id).is_some() && !self.base.contains_object(id.number);
        if !was_added && self.base.contains_object(id.number) {
            self.deleted.insert(id.number);
        }
    }

    /// The page tree as this edit leaves it.
    pub fn pages(&self) -> Result<Vec<Page>> {
        crate::page::pages_of(self)
    }

    /// How many pages the document has after the edits.
    pub fn page_count(&self) -> Result<usize> {
        Ok(self.pages()?.len())
    }

    /// The page at `index`, counting from zero, after the edits.
    pub fn page(&self, index: usize) -> Result<Page> {
        self.pages()?
            .into_iter()
            .nth(index)
            .ok_or(Error::malformed("page index is past the last page"))
    }

    /// Add a page drawing `content` to the end of the page tree.
    ///
    /// The page inherits its box and resources from the page tree when it can, so
    /// a page added to a landscape document is landscape. The page tree's
    /// `/Count` is updated with it, and the addition counts against the document's
    /// page ceiling (§7.7.3.2).
    pub fn add_page(&mut self, content: impl Into<Vec<u8>>) -> Result<ObjectId> {
        let pages_id = self.pages_id()?;
        let mut pages = match self.get_object(pages_id)? {
            Object::Dictionary(dict) => dict,
            _ => return Err(Error::malformed("the page tree node is not a dictionary")),
        };
        let count = self.page_count()?;
        let limit = self.page_limit();
        if count + 1 > limit {
            return Err(Error::TooMany {
                what: "the page tree",
                limit,
            });
        }

        // A page states its own box or inherits one; taking the first page's
        // effective box is what makes the new page match the document rather than
        // the standard's default.
        let inherited_media_box = match self.page(0) {
            Ok(first) => first.media_box,
            Err(_) => None,
        };
        let media_box = match pages.get(b"MediaBox") {
            Some(Object::Array(items)) => Some(Object::Array(items.clone())),
            _ => inherited_media_box.map(|rect| {
                Object::Array(vec![
                    Object::Real(rect.x0),
                    Object::Real(rect.y0),
                    Object::Real(rect.x1),
                    Object::Real(rect.y1),
                ])
            }),
        };
        let resources = match pages.get(b"Resources") {
            Some(resources) => Some(resources.clone()),
            None => match self.page(0) {
                Ok(first) => first.resources.map(Object::Dictionary),
                Err(_) => None,
            },
        };

        let content = content.into();
        let mut content_dict = Dictionary::new();
        content_dict.insert("Length", Object::Integer(content.len() as i64));
        let content_id = self.add_object(Object::Stream(Stream::new(content_dict, content)));

        let mut page = Dictionary::new();
        page.insert("Type", Object::Name(b"Page".to_vec()));
        page.insert("Parent", Object::Reference(pages_id));
        page.insert("Contents", Object::Reference(content_id));
        if let Some(media_box) = media_box {
            page.insert("MediaBox", media_box);
        }
        if let Some(resources) = resources {
            page.insert("Resources", resources);
        }
        let page_id = self.add_object(Object::Dictionary(page));

        let mut kids = match pages.get(b"Kids") {
            Some(Object::Array(items)) => items.clone(),
            Some(kids) => match self.resolve(Some(kids))?.unwrap_or(Object::Null) {
                Object::Array(items) => items,
                _ => return Err(Error::malformed("/Kids must be an array")),
            },
            None => return Err(Error::malformed("the page tree node has no /Kids")),
        };
        kids.push(Object::Reference(page_id));
        pages.insert("Kids", Object::Array(kids));
        pages.insert("Count", Object::Integer((count + 1) as i64));
        self.set_object(pages_id, Object::Dictionary(pages));
        Ok(page_id)
    }

    /// The page tree node's id, reached through the catalog.
    fn pages_id(&self) -> Result<ObjectId> {
        let catalog = match self.get_object(self.root_id()?)? {
            Object::Dictionary(dict) => dict,
            _ => return Err(Error::malformed("catalog /Root is not a dictionary")),
        };
        match catalog.get(b"Pages") {
            Some(Object::Reference(id)) => Ok(*id),
            _ => Err(Error::malformed(
                "the catalog's /Pages must be an indirect reference to edit it",
            )),
        }
    }
}

impl ObjectSource for MutableDocument {
    fn get_object(&self, id: ObjectId) -> Result<Object> {
        MutableDocument::get_object(self, id)
    }

    fn page_limit(&self) -> usize {
        self.base.page_limit()
    }

    fn root_id(&self) -> Result<ObjectId> {
        ObjectSource::root_id(&self.base)
    }
}

impl core::fmt::Display for MutableDocument {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "{} changed, {} deleted",
            self.changes.len(),
            self.deleted.len()
        )
    }
}
