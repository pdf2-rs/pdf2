//! Writing PDF 2.0 files (§7.5.7, §7.5.8).
//!
//! [`DocumentBuilder`] collects objects and serializes them in one pass:
//! a `%PDF-2.0` header, a Flate-compressed `/ObjStm` holding every non-stream
//! object, the stream objects as ordinary indirect objects, and a compressed
//! `/Type /XRef` cross-reference stream that carries `/Root` and `/Size` — so
//! the output is 2.0-clean, with no `trailer` dictionary, no `/Info`
//! dictionary, and XMP `/Metadata` in the catalog.
//!
//! Offsets are recorded as bytes are written, which is what makes the
//! cross-reference stream correct; `save()` output is verified by reading the
//! file back with [`crate::Document::open`].

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::document::Document;
use crate::error::{Error, Result};
use crate::object::{Dictionary, Object, ObjectId, Stream};
use crate::page::Rect;

/// A minimal XMP packet written when the caller does not supply one.
///
/// PDF 2.0 requires metadata to travel in an XMP stream rather than in the
/// (deprecated) `Info` dictionary, so every file written by this crate has one.
pub const XMP_PACKET: &str = concat!(
    "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n",
    " <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n",
    "  <rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"",
    " pdf:Producer=\"pdf2\"/>\n",
    " </rdf:RDF>\n",
    "</x:xmpmeta>\n",
    "<?xpacket end=\"w\"?>",
);

/// The cross-reference stream field widths: type, offset or object-stream
/// number, generation or index.
const XREF_WIDTHS: [u8; 3] = [1, 4, 2];

/// Builds a new PDF 2.0 file.
///
/// Obtained from [`Document::new`] or [`DocumentBuilder::new`]; the catalog
/// (object 1) and the page tree root (object 2) exist from the start.
#[derive(Debug, Clone)]
pub struct DocumentBuilder {
    objects: BTreeMap<u32, Object>,
    next_number: u32,
    catalog: u32,
    pages: u32,
    font: Option<u32>,
    metadata: Option<Vec<u8>>,
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentBuilder {
    /// A new, empty document.
    pub fn new() -> Self {
        let mut objects = BTreeMap::new();
        let mut catalog = Dictionary::new();
        catalog.insert("Type", Object::name("Catalog"));
        catalog.insert("Pages", Object::reference(ObjectId::new(2, 0)));
        objects.insert(1, Object::Dictionary(catalog));
        let mut pages = Dictionary::new();
        pages.insert("Type", Object::name("Pages"));
        pages.insert("Kids", Object::array(Vec::new()));
        pages.insert("Count", Object::Integer(0));
        objects.insert(2, Object::Dictionary(pages));
        Self {
            objects,
            next_number: 3,
            catalog: 1,
            pages: 2,
            font: None,
            metadata: None,
        }
    }

    /// Add an object and return the number it was assigned.
    pub fn add_object(&mut self, object: Object) -> u32 {
        let number = self.allocate();
        self.objects.insert(number, object);
        number
    }

    /// Append a page with `content` as its content stream.
    ///
    /// Every page gets `/Resources << /Font << /F1 … >> >>` pointing at one
    /// shared Helvetica font, so written content streams that use `/F1` are
    /// immediately extractable.
    pub fn add_page(&mut self, media_box: Rect, content: impl Into<Vec<u8>>) -> u32 {
        let font = self.ensure_font();
        let contents = self.add_object(Object::Stream(Stream::new(
            Dictionary::new(),
            content.into(),
        )));
        let mut resources = Dictionary::new();
        let mut fonts = Dictionary::new();
        fonts.insert("F1", Object::reference(ObjectId::new(font, 0)));
        resources.insert("Font", Object::Dictionary(fonts));

        let mut page = Dictionary::new();
        page.insert("Type", Object::name("Page"));
        page.insert("Parent", Object::reference(ObjectId::new(self.pages, 0)));
        page.insert("MediaBox", rect_object(media_box));
        page.insert("Resources", Object::Dictionary(resources));
        page.insert("Contents", Object::reference(ObjectId::new(contents, 0)));
        let number = self.add_object(Object::Dictionary(page));

        let pages_number = self.pages;
        if let Some(Object::Dictionary(pages)) = self.objects.get_mut(&pages_number) {
            let count = match pages.get_mut(b"Kids") {
                Some(Object::Array(kids)) => {
                    kids.push(Object::reference(ObjectId::new(number, 0)));
                    kids.len()
                }
                _ => {
                    pages.insert(
                        "Kids",
                        Object::array([Object::reference(ObjectId::new(number, 0))]),
                    );
                    1
                }
            };
            pages.insert("Count", Object::Integer(count as i64));
        }
        number
    }

    /// Number of pages appended so far.
    pub fn page_count(&self) -> usize {
        match self.objects.get(&self.pages) {
            Some(Object::Dictionary(pages)) => pages
                .get(b"Kids")
                .and_then(Object::as_array)
                .map_or(0, <[Object]>::len),
            _ => 0,
        }
    }

    /// Replace the XMP metadata packet written into the catalog.
    pub fn set_metadata(&mut self, xmp: impl Into<Vec<u8>>) {
        self.metadata = Some(xmp.into());
    }

    /// Iterate over the objects collected so far, in object-number order.
    pub fn objects(&self) -> impl Iterator<Item = (u32, &Object)> {
        self.objects
            .iter()
            .map(|(number, object)| (*number, object))
    }

    /// Serialize the document as a PDF 2.0 file.
    pub fn save(&self) -> Result<Vec<u8>> {
        let mut objects = self.objects.clone();
        let mut next_number = self.next_number;

        // The catalog always gains an XMP metadata stream.
        let metadata_number = next_number;
        next_number += 1;
        let xmp = self
            .metadata
            .clone()
            .unwrap_or_else(|| XMP_PACKET.as_bytes().to_vec());
        let mut metadata_dict = Dictionary::new();
        metadata_dict.insert("Type", Object::name("Metadata"));
        metadata_dict.insert("Subtype", Object::name("XML"));
        objects.insert(
            metadata_number,
            Object::Stream(Stream::new(metadata_dict, xmp)),
        );
        match objects.get_mut(&self.catalog) {
            Some(Object::Dictionary(catalog)) => {
                catalog.insert(
                    "Metadata",
                    Object::reference(ObjectId::new(metadata_number, 0)),
                );
            }
            _ => {
                return Err(Error::Malformed {
                    what: "the catalog object was removed from the document",
                    offset: None,
                })
            }
        }

        // Streams cannot live inside an object stream, everything else can.
        let mut top_level: Vec<(u32, Stream)> = Vec::new();
        let mut compressed: Vec<(u32, Object)> = Vec::new();
        for (number, object) in objects {
            match object {
                Object::Stream(stream) => top_level.push((number, stream)),
                other => compressed.push((number, other)),
            }
        }

        let object_stream_number = next_number;
        next_number += 1;
        let (object_stream_data, object_stream_first, object_stream_index) =
            pack_object_stream(&compressed);
        let mut object_stream_dict = Dictionary::new();
        object_stream_dict.insert("Type", Object::name("ObjStm"));
        object_stream_dict.insert("N", Object::Integer(compressed.len() as i64));
        object_stream_dict.insert("First", Object::Integer(object_stream_first as i64));
        object_stream_dict.insert("Filter", Object::name("FlateDecode"));
        top_level.push((
            object_stream_number,
            Stream::new(object_stream_dict, deflate(&object_stream_data)?),
        ));
        top_level.sort_by_key(|(number, _)| *number);

        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-2.0\n");
        // A binary comment marks the file as containing binary data (§7.5.2).
        out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

        let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
        for (number, stream) in &top_level {
            offsets.insert(*number, out.len());
            write_stream_object(*number, stream, &mut out);
        }

        let xref_number = next_number;
        let size = xref_number + 1;
        let xref_offset = out.len();
        let rows = xref_rows(
            size,
            xref_number,
            &offsets,
            &object_stream_index,
            object_stream_number,
            xref_offset,
        );
        let compressed_rows = deflate(&rows)?;
        let mut xref_dict = Dictionary::new();
        xref_dict.insert("Type", Object::name("XRef"));
        xref_dict.insert("Size", Object::Integer(i64::from(size)));
        xref_dict.insert(
            "W",
            Object::array([
                Object::Integer(i64::from(XREF_WIDTHS[0])),
                Object::Integer(i64::from(XREF_WIDTHS[1])),
                Object::Integer(i64::from(XREF_WIDTHS[2])),
            ]),
        );
        xref_dict.insert("Filter", Object::name("FlateDecode"));
        xref_dict.insert("Root", Object::reference(ObjectId::new(self.catalog, 0)));
        let xref_stream = Stream::new(xref_dict, compressed_rows);
        write_stream_object(xref_number, &xref_stream, &mut out);

        out.extend_from_slice(format!("startxref\n{xref_offset}\n%%EOF\n").as_bytes());
        Ok(out)
    }

    /// Write the document to a file.
    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<()> {
        std::fs::write(path, self.save()?)?;
        Ok(())
    }

    fn allocate(&mut self) -> u32 {
        let number = self.next_number;
        self.next_number += 1;
        number
    }

    fn ensure_font(&mut self) -> u32 {
        if let Some(font) = self.font {
            return font;
        }
        let mut dict = Dictionary::new();
        dict.insert("Type", Object::name("Font"));
        dict.insert("Subtype", Object::name("Type1"));
        dict.insert("BaseFont", Object::name("Helvetica"));
        dict.insert("Encoding", Object::name("WinAnsiEncoding"));
        let number = self.add_object(Object::Dictionary(dict));
        self.font = Some(number);
        number
    }
}

impl Document {
    /// Start a new, empty PDF 2.0 document.
    ///
    /// Returns a [`DocumentBuilder`] rather than a reader: collect objects and
    /// pages with it, then [`DocumentBuilder::save`] the file and
    /// [`Document::open`] it back. The name follows the plan's
    /// `Document::new()` … `save()` shape; the builder is the only thing that
    /// can be "new", because a reader always comes from bytes.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> DocumentBuilder {
        DocumentBuilder::new()
    }
}

/// Serialize a stream as an indirect object, keeping `/Length` in step with
/// the payload regardless of what the dictionary said.
fn write_stream_object(number: u32, stream: &Stream, out: &mut Vec<u8>) {
    let mut dict = stream.dict().clone();
    dict.insert("Length", Object::Integer(stream.data().len() as i64));
    out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
    Object::Dictionary(dict).write_to(out);
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(stream.data());
    out.extend_from_slice(b"\nendstream\nendobj\n");
}

/// Body of an `/ObjStm`: the header pairs followed by the objects themselves.
///
/// Returns the uncompressed data, the length of the header (`/First`) and the
/// object-number-to-index mapping the cross-reference stream needs.
fn pack_object_stream(objects: &[(u32, Object)]) -> (Vec<u8>, usize, Vec<(u32, u32)>) {
    let mut header = String::new();
    let mut body = Vec::new();
    let mut index = Vec::with_capacity(objects.len());
    for (position, (number, object)) in objects.iter().enumerate() {
        header.push_str(&format!("{number} {} ", body.len()));
        object.write_to(&mut body);
        body.push(b' ');
        index.push((*number, position as u32));
    }
    let mut data = header.into_bytes();
    let first = data.len();
    data.extend_from_slice(&body);
    (data, first, index)
}

fn xref_rows(
    size: u32,
    xref_number: u32,
    offsets: &BTreeMap<u32, usize>,
    object_stream_index: &[(u32, u32)],
    object_stream_number: u32,
    xref_offset: usize,
) -> Vec<u8> {
    let mut rows = Vec::with_capacity(size as usize * 7);
    for number in 0..size {
        let (kind, field_two, field_three) = if number == 0 {
            (0_u64, 0_u64, 65_535_u64)
        } else if number == xref_number {
            (1, xref_offset as u64, 0)
        } else if let Some((_, index)) = object_stream_index.iter().find(|(n, _)| *n == number) {
            (2, u64::from(object_stream_number), u64::from(*index))
        } else if let Some(offset) = offsets.get(&number) {
            (1, *offset as u64, 0)
        } else {
            (0, 0, 65_535)
        };
        push_field(&mut rows, kind, XREF_WIDTHS[0]);
        push_field(&mut rows, field_two, XREF_WIDTHS[1]);
        push_field(&mut rows, field_three, XREF_WIDTHS[2]);
    }
    rows
}

fn push_field(out: &mut Vec<u8>, value: u64, width: u8) {
    let bytes = value.to_be_bytes();
    let width = usize::from(width);
    out.extend_from_slice(&bytes[bytes.len() - width..]);
}

fn deflate(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(|error| Error::Io {
        message: error.to_string(),
    })?;
    encoder.finish().map_err(|error| Error::Io {
        message: error.to_string(),
    })
}

/// A rectangle as the four-number array PDF expects.
fn rect_object(rect: Rect) -> Object {
    Object::array([
        number_object(rect.x0),
        number_object(rect.y0),
        number_object(rect.x1),
        number_object(rect.y1),
    ])
}

fn number_object(value: f64) -> Object {
    if value.fract() == 0.0 && value.abs() < 9.0e15 && value.is_finite() {
        Object::Integer(value as i64)
    } else if value.is_finite() {
        Object::Real(value)
    } else {
        Object::Integer(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{pack_object_stream, rect_object, DocumentBuilder, XMP_PACKET};
    use crate::object::Object;
    use crate::page::Rect;

    #[test]
    fn a_new_builder_has_a_catalog_and_an_empty_page_tree() {
        let builder = DocumentBuilder::new();
        assert_eq!(builder.page_count(), 0);
        let names: Vec<u32> = builder.objects().map(|(number, _)| number).collect();
        assert_eq!(names, vec![1, 2]);
    }

    #[test]
    fn pages_are_appended_and_counted() {
        let mut builder = DocumentBuilder::new();
        let first = builder.add_page(Rect::new(0.0, 0.0, 612.0, 792.0), "BT ET");
        let second = builder.add_page(Rect::new(0.0, 0.0, 100.0, 200.0), "BT ET");
        assert!(second > first);
        assert_eq!(builder.page_count(), 2);
        assert_eq!(
            builder
                .objects()
                .find(|(number, _)| *number == 2)
                .and_then(|(_, object)| object.as_dict())
                .and_then(|dict| dict.get(b"Count")),
            Some(&Object::Integer(2))
        );
    }

    #[test]
    fn rectangles_become_integer_arrays_when_they_are_whole_numbers() {
        assert_eq!(
            rect_object(Rect::new(0.0, 0.0, 612.0, 792.0)),
            Object::array([
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792)
            ])
        );
        assert_eq!(
            rect_object(Rect::new(0.5, 0.0, 1.0, 2.25)),
            Object::array([
                Object::Real(0.5),
                Object::Integer(0),
                Object::Integer(1),
                Object::Real(2.25)
            ])
        );
    }

    #[test]
    fn the_object_stream_header_length_matches_the_packer() {
        let objects = vec![
            (1, Object::Integer(1)),
            (2, Object::name("Page")),
            (3, Object::array([Object::Integer(1), Object::Integer(2)])),
        ];
        let (data, first, index) = pack_object_stream(&objects);
        assert_eq!(index, vec![(1, 0), (2, 1), (3, 2)]);
        // Every header pair is two integers separated by spaces.
        let header = std::str::from_utf8(&data[..first]).expect("ascii header");
        let numbers: Vec<u32> = header
            .split_whitespace()
            .step_by(2)
            .map(|number| number.parse().expect("integer"))
            .collect();
        assert_eq!(numbers, vec![1, 2, 3]);
        // Each body offset lands exactly on the object it describes.
        for (number, position) in [(1, 0), (2, 1), (3, 2)] {
            let offset: usize = header
                .split_whitespace()
                .nth(position * 2 + 1)
                .expect("offset")
                .parse()
                .expect("integer");
            let object = crate::Parser::new(&data[first + offset..])
                .parse_object()
                .expect("object");
            assert_eq!(object, objects[position].1, "object {number}");
        }
    }

    #[test]
    fn the_xmp_packet_is_a_packet() {
        assert!(XMP_PACKET.starts_with("<?xpacket begin="));
        assert!(XMP_PACKET.ends_with("<?xpacket end=\"w\"?>"));
        assert!(XMP_PACKET.contains("x:xmpmeta"));
    }

    #[test]
    fn saving_a_minimal_document_produces_a_readable_file() {
        let builder = DocumentBuilder::new();
        let bytes = builder.save().expect("save");
        assert!(bytes.starts_with(b"%PDF-2.0\n"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let document = crate::Document::open(bytes).expect("reopen");
        assert_eq!(document.version(), "2.0");
        assert_eq!(document.page_count().expect("pages"), 0);
        assert!(document.has_metadata().expect("metadata"));
    }
}
