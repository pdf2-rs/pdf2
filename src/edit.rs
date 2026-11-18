//! Editing an opened document (§7.5.6 Incremental Updates).
//!
//! An edit is a copy-on-write overlay: [`MutableDocument`] holds the changed and
//! deleted objects, the base [`Document`] keeps answering with the file as it was
//! read, and a save appends to the base's bytes instead of rewriting them. The
//! model is decided in `docs/adr/0006-editing-mutation-model.md`.
//!
//! ```
//! # fn main() -> pdf2::Result<()> {
//! let bytes = std::fs::read("tests/fixtures/text.pdf")?;
//! let base = pdf2::Document::open(bytes.clone())?;
//! let before = base.page_count()?;
//!
//! let mut edit = pdf2::MutableDocument::new(base);
//! edit.add_page(b"BT /F1 12 Tf 72 720 Td (a page added by an edit) Tj ET")?;
//! assert_eq!(edit.page_count()?, before + 1);
//! assert_eq!(edit.base().page_count()?, before);
//!
//! let saved = edit.save_incremental()?;
//! assert!(saved.starts_with(&bytes), "an update appends, it never rewrites");
//! let reopened = pdf2::Document::open(&saved[..])?;
//! assert_eq!(reopened.page_count()?, before + 1);
//! # Ok(())
//! # }
//! ```
//!
//! Reading goes through the overlay first and the base second, so every reader
//! that takes a [`Document`] keeps working on the unedited file — the base is
//! never modified.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use crate::document::{Document, ObjectSource, XrefEntry};
use crate::error::{Error, Result};
use crate::object::{Dictionary, Object, ObjectId, PdfString, Stream};
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

    /// Save the edits as an incremental update (§7.5.6).
    ///
    /// The returned bytes begin with the base document's bytes, byte for byte —
    /// which is what keeps a signature over the original valid and makes a failed
    /// save harmless — and continue with each changed or added object, one
    /// classic cross-reference section covering exactly those numbers (`f` rows
    /// for the deleted ones), a trailer carrying `/Size`, `/Prev` and the base's
    /// first `/ID` string, then `startxref` and its own `%%EOF`.
    ///
    /// An encrypted base is refused for now: appending an object that has not
    /// been encrypted with the base's per-object key would produce a file that
    /// opens but reads as garbage, and a refusal says so.
    pub fn save_incremental(&self) -> Result<Vec<u8>> {
        if self.base.base_encryption().is_some() {
            return Err(Error::Unsupported {
                what: "saving an incremental update to an encrypted document",
            });
        }
        // The ceiling comes before the bytes: a save that would write a document
        // this crate then refuses to read is a bug, and a caller cannot see the
        // file, so there is nothing to do with the bytes but discard them.
        let writing = self.changes.len();
        let limit = self.base.object_limit();
        if writing > limit {
            return Err(Error::TooMany {
                what: "the number of objects an update writes",
                limit,
            });
        }
        let mut out = self.base.base_bytes().to_vec();
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }

        let mut written: BTreeMap<u32, (usize, u16)> = BTreeMap::new();
        for (id, object) in &self.changes {
            let offset = out.len();
            match object {
                Object::Stream(stream) => {
                    crate::writer::write_stream_object(id.number, stream, &mut out);
                }
                other => {
                    out.extend_from_slice(
                        format!("{} {} obj\n", id.number, id.generation).as_bytes(),
                    );
                    other.write_to(&mut out);
                    out.extend_from_slice(b"\nendobj\n");
                }
            }
            written.insert(id.number, (offset, id.generation));
        }

        let xref_offset = out.len();
        self.write_appended_xref(&mut out, &written);
        let trailer = self.appended_trailer(&out);
        out.extend_from_slice(b"trailer\n");
        Object::Dictionary(trailer).write_to(&mut out);
        out.extend_from_slice(format!("\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes());
        Ok(out)
    }

    /// One classic cross-reference section over exactly the numbers this save
    /// wrote: `n` rows for the new copies, `f` rows for the deletions, grouped
    /// into consecutive subsections (`7.5.4`).
    fn write_appended_xref(&self, out: &mut Vec<u8>, written: &BTreeMap<u32, (usize, u16)>) {
        let mut freed: BTreeMap<u32, u16> = BTreeMap::new();
        for number in &self.deleted {
            // A deletion increments the generation, which is what stops a stale
            // reference from resolving to whatever occupies the number next.
            let generation = match self.base.base_xref().get(number) {
                Some(XrefEntry::InUse { generation, .. })
                | Some(XrefEntry::Free { generation }) => generation.saturating_add(1),
                _ => 1,
            };
            freed.insert(*number, generation);
        }
        let mut numbers: Vec<u32> = written.keys().chain(freed.keys()).copied().collect();
        numbers.sort_unstable();
        numbers.dedup();

        out.extend_from_slice(b"xref\n");
        let mut index = 0;
        while index < numbers.len() {
            let mut end = index;
            while end + 1 < numbers.len() && numbers[end + 1] == numbers[end] + 1 {
                end += 1;
            }
            out.extend_from_slice(format!("{} {}\n", numbers[index], end - index + 1).as_bytes());
            for number in &numbers[index..=end] {
                match written.get(number) {
                    Some((offset, generation)) => out.extend_from_slice(
                        format!("{offset:010} {generation:05} n\r\n").as_bytes(),
                    ),
                    None => {
                        let generation = freed.get(number).copied().unwrap_or(1);
                        out.extend_from_slice(
                            format!("0000000000 {generation:05} f\r\n").as_bytes(),
                        );
                    }
                }
            }
            index = end + 1;
        }
    }

    /// The base trailer minus `/Prev`, with `/Size`, `/Prev` and `/ID` composed
    /// the way `14.4` and `7.5.6` require: the first identifier string survives
    /// the update unchanged, the second is new.
    fn appended_trailer(&self, written: &[u8]) -> Dictionary {
        let mut trailer = Dictionary::new();
        for (key, value) in self.base.trailer().iter() {
            if key == b"Prev" {
                continue;
            }
            trailer.insert(key.to_vec(), value.clone());
        }
        let highest = self
            .changes
            .keys()
            .map(|id| id.number + 1)
            .max()
            .unwrap_or(0);
        let size = match self.base.trailer().get(b"Size") {
            Some(Object::Integer(size)) => (*size as u32).max(self.next_object),
            _ => self.next_object,
        }
        .max(highest);
        trailer.insert("Size", Object::Integer(size as i64));
        if let Ok(previous) = self.base.base_startxref() {
            trailer.insert("Prev", Object::Integer(previous as i64));
        }
        if let Some(Object::Array(mut ids)) = self.base.trailer().get(b"ID").cloned()
            && ids.len() == 2
        {
            let digest = fingerprint(written);
            ids[1] = Object::String(PdfString::hex(digest.to_vec()));
            trailer.insert("ID", Object::Array(ids));
        }
        trailer
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

/// Sixteen bytes derived from what this save appended, for `/ID`'s second
/// string: two FNV-1a passes with different seeds. Deterministic on purpose, so
/// the same edit saved twice produces the same file.
fn fingerprint(data: &[u8]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (half, seed) in [0xcbf2_9ce4_8422_2325_u64, 0x9e37_79b9_7f4a_7c15]
        .into_iter()
        .enumerate()
    {
        let mut hash = seed;
        for byte in data {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        out[half * 8..half * 8 + 8].copy_from_slice(&hash.to_be_bytes());
    }
    out
}
