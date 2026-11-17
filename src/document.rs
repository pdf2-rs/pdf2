//! Opening a PDF 2.0 file through its cross-reference data (§7.5).
//!
//! [`Document::open`] sniffs the header, follows `startxref`, and reads every
//! cross-reference section in the `/Prev` chain — classic tables, `/Type /XRef`
//! cross-reference streams, and the hybrid `/XRefStm` form — keeping the newest
//! entry for each object number. Objects are fetched lazily: the file bytes
//! stay put, and an object is parsed (and cached) the first time somebody asks
//! for it. Objects stored in `/ObjStm` streams are extracted on first use.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use alloc::collections::{BTreeMap, BTreeSet};
use core::cell::{Cell, RefCell};

use crate::crypto::Encryption;
use crate::error::{Error, Result};
use crate::filter::{self, FilterSpec};
use crate::lexer::Token;
use crate::object::{Dictionary, Object, ObjectId, Stream};
use crate::parser::{LengthResolver, Parser};

/// The header every file of this crate must start with.
pub const HEADER: &[u8] = b"%PDF-";

/// How far into the file the `%PDF-` header may sit (§7.5.2).
const HEADER_SEARCH_LIMIT: usize = 1024;

/// How many `/Prev` sections are followed before giving up on a loop.
const MAX_XREF_SECTIONS: usize = 128;

/// How many objects a document may hold before reading one is refused: a
/// crafted cross-reference can name tens of millions, and every one that is read
/// costs a parse and a cache slot.
pub const DEFAULT_OBJECT_LIMIT: usize = 1 << 20;

/// How many pages a page tree may list before it is refused, so a crafted
/// `/Pages` cannot make every `pages()` call walk a million leaves.
pub const DEFAULT_PAGE_LIMIT: usize = 1 << 17;

/// How deep an object may be resolved while parsing another one, which bounds
/// `/Length` and `/Filter` chains that point at each other.
const MAX_RESOLVE_DEPTH: usize = 32;

/// Where one object lives according to the cross-reference data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum XrefEntry {
    /// A free entry (`f`, type 0): the object number is unused.
    Free { generation: u16 },
    /// An in-use entry (`n`, type 1): the object is at a byte offset.
    InUse { offset: usize, generation: u16 },
    /// A compressed entry (type 2): the object is inside an `/ObjStm` stream.
    Compressed { stream: u32, index: u32 },
}

/// A parsed PDF file.
///
/// Opening is cheap: only the header, the cross-reference sections and the
/// trailer are read. Pages, streams and content are parsed on demand by
/// [`Document::get_object`], which caches what it reads.
#[derive(Debug)]
pub struct Document {
    data: Vec<u8>,
    version: String,
    trailer: Dictionary,
    xref: BTreeMap<u32, XrefEntry>,
    cache: RefCell<BTreeMap<ObjectId, Object>>,
    /// Object numbers held by each object stream, in index order.
    object_streams: RefCell<BTreeMap<u32, Vec<u32>>>,
    /// The open security handler, when the document is encrypted.
    encryption: Option<Encryption>,
    /// The number of the `/Encrypt` dictionary, which is never decrypted.
    encrypt_number: Option<u32>,
    depth: Cell<usize>,
    /// The decoded-bytes budget every stream is held to.
    decode_limit: Cell<usize>,
    /// How many objects may be read before `get_object` refuses.
    object_limit: Cell<usize>,
    /// How many pages a page tree may list.
    page_limit: Cell<usize>,
}

impl Document {
    /// Open a PDF from its bytes.
    ///
    /// Accepts `%PDF-1.x` and `%PDF-2.x` files whose cross-reference is a classic
    /// table, an
    /// xref stream, or a hybrid of both, with any number of `/Prev` sections.
    ///
    /// An encrypted file is opened with the empty password, which covers the
    /// common "no password needed, but encrypted" case; use
    /// [`Document::open_with_password`] otherwise.
    pub fn open(data: impl Into<Vec<u8>>) -> Result<Self> {
        Self::open_with_password(data, b"")
    }

    /// Open a PDF with a password for its `/V 5` (AES-256) security handler.
    pub fn open_with_password(data: impl Into<Vec<u8>>, password: &[u8]) -> Result<Self> {
        let data = data.into();
        let version = header_version(&data)?;
        let (xref, trailer) = read_xref_chain(&data)?;
        match trailer.get(b"Root") {
            Some(Object::Reference(_)) => {}
            _ => {
                return Err(Error::Malformed {
                    what: "trailer /Root must be an indirect reference",
                    offset: None,
                });
            }
        }
        let mut document = Self {
            data,
            version,
            trailer,
            xref,
            cache: RefCell::new(BTreeMap::new()),
            object_streams: RefCell::new(BTreeMap::new()),
            encryption: None,
            encrypt_number: None,
            depth: Cell::new(0),
            decode_limit: Cell::new(filter::DEFAULT_DECODE_LIMIT),
            object_limit: Cell::new(DEFAULT_OBJECT_LIMIT),
            page_limit: Cell::new(DEFAULT_PAGE_LIMIT),
        };
        if let Some(encrypt) = document.trailer.get(b"Encrypt").cloned() {
            // The `/Encrypt` dictionary is plain even in an encrypted file, so
            // it is fetched before any handler is installed.
            let (dictionary, number) = match &encrypt {
                Object::Reference(id) => match document.get_object(*id)? {
                    Object::Dictionary(dict) => (dict, Some(id.number)),
                    _ => {
                        return Err(Error::Malformed {
                            what: "trailer /Encrypt is not a dictionary",
                            offset: None,
                        });
                    }
                },
                Object::Dictionary(dict) => (dict.clone(), None),
                _ => {
                    return Err(Error::Malformed {
                        what: "trailer /Encrypt must be a reference or a dictionary",
                        offset: None,
                    });
                }
            };
            // Revisions 2–4 mix the first `/ID` element into the key, so the
            // handler is built from the trailer rather than the dictionary alone.
            let id = document
                .trailer
                .get(b"ID")
                .and_then(|value| match value {
                    Object::Array(items) => items.first(),
                    _ => None,
                })
                .and_then(|value| value.as_string())
                .map(|string| string.as_bytes().to_vec())
                .unwrap_or_default();
            document.encryption = Some(Encryption::new_with_id(&dictionary, password, &id)?);
            document.encrypt_number = number;
        }
        Ok(document)
    }

    /// Open a PDF from a file path.
    #[cfg(feature = "std")]
    pub fn open_path(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::open(std::fs::read(path)?)
    }

    /// Open a PDF from a file path with a password.
    #[cfg(feature = "std")]
    pub fn open_path_with_password(
        path: impl AsRef<std::path::Path>,
        password: &[u8],
    ) -> Result<Self> {
        Self::open_with_password(std::fs::read(path)?, password)
    }

    /// Whether the document was opened with a security handler.
    pub fn is_encrypted(&self) -> bool {
        self.encryption.is_some()
    }

    /// The version from the header, e.g. `"2.0"`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The newest trailer dictionary.
    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    /// The document catalog, reached through the trailer's `/Root`.
    ///
    /// The catalogue is the entry point of the whole object graph (§7.7.2).
    pub fn catalog(&self) -> Result<Object> {
        let root = match self.trailer.get(b"Root") {
            Some(Object::Reference(id)) => *id,
            _ => {
                return Err(Error::Malformed {
                    what: "trailer /Root must be an indirect reference",
                    offset: None,
                });
            }
        };
        self.get_object(root)
    }

    /// Fetch the object with this id, following the cross-reference.
    ///
    /// Only the object number selects the entry, so a stale generation still
    /// resolves — that is what real readers do and what incremental updates
    /// rely on. Use [`Document::get_object_by_number`] to fetch with the
    /// generation the cross-reference records.
    ///
    /// Results are cached, so fetching the same object twice parses it once.
    pub fn get_object(&self, id: ObjectId) -> Result<Object> {
        if let Some(cached) = self.cache.borrow().get(&id) {
            return Ok(cached.clone());
        }
        let entry = *self.xref.get(&id.number).ok_or(Error::Malformed {
            what: "object number is not in the cross-reference",
            offset: None,
        })?;
        let object = match entry {
            XrefEntry::Free { .. } => {
                return Err(Error::Malformed {
                    what: "object is free in the cross-reference",
                    offset: None,
                });
            }
            XrefEntry::InUse { offset, .. } => self.parse_at(id.number, offset)?,
            XrefEntry::Compressed { stream, index } => {
                self.compressed_object(id.number, stream, index)?
            }
        };
        let object = match &self.encryption {
            // The `/Encrypt` dictionary holds `/U`, `/O` and `/Perms` verbatim.
            Some(encryption) if Some(id.number) != self.encrypt_number => {
                // The object number is what revisions 2–4 derive their key from.
                encryption.decrypt_object_at(&object, Some(id))?
            }
            _ => object,
        };
        {
            // The cache is what grows with what has been read, so it is what the
            // object budget counts. Read before the insert: the limit is a
            // ceiling on the number *held*, not on the number seen.
            let mut cache = self.cache.borrow_mut();
            if cache.len() >= self.object_limit.get() {
                return Err(Error::TooMany {
                    what: "the number of objects read",
                    limit: self.object_limit.get(),
                });
            }
            cache.insert(id, object.clone());
        }
        Ok(object)
    }

    /// Fetch an object by number, using the generation the cross-reference has.
    pub fn get_object_by_number(&self, number: u32) -> Result<Object> {
        match self.xref.get(&number) {
            Some(XrefEntry::InUse { generation, .. }) => {
                self.get_object(ObjectId::new(number, *generation))
            }
            Some(XrefEntry::Compressed { .. }) => self.get_object(ObjectId::new(number, 0)),
            Some(XrefEntry::Free { .. }) => Err(Error::Malformed {
                what: "object is free in the cross-reference",
                offset: None,
            }),
            None => Err(Error::Malformed {
                what: "object number is not in the cross-reference",
                offset: None,
            }),
        }
    }

    /// Whether the cross-reference knows `number` as a live object.
    pub fn contains_object(&self, number: u32) -> bool {
        matches!(
            self.xref.get(&number),
            Some(XrefEntry::InUse { .. } | XrefEntry::Compressed { .. })
        )
    }

    /// Fetch a stream object by id.
    pub fn stream(&self, id: ObjectId) -> Result<Stream> {
        match self.get_object(id)? {
            Object::Stream(stream) => Ok(stream),
            _ => Err(Error::Malformed {
                what: "object is not a stream",
                offset: None,
            }),
        }
    }

    /// The decoded-bytes budget: the most any one stream may decode to.
    ///
    /// Defaults to [`filter::DEFAULT_DECODE_LIMIT`] (256 MiB). A stream that
    /// would decode past it fails with [`crate::Error::Limit`] instead of
    /// allocating: without the cap a 1 MiB file can expand to a gigabyte, which
    /// is a denial of service rather than a rendering failure.
    pub fn decoded_limit(&self) -> usize {
        self.decode_limit.get()
    }

    /// Set the decoded-bytes budget, in bytes; `usize::MAX` lifts it.
    ///
    /// Lower it when the caller knows its own ceiling (a browser tab, a
    /// server-side preview), or raise it for a document whose images legitimately
    /// decode to more than the default.
    pub fn set_decoded_limit(&self, bytes: usize) {
        self.decode_limit.set(bytes);
    }

    /// How many objects may be read before one is refused.
    ///
    /// Defaults to [`DEFAULT_OBJECT_LIMIT`]. The count is of objects *held*
    /// after being read, so a document that lists millions in its
    /// cross-reference is still opened — the ceiling is on parsing them, which is
    /// the part that costs memory.
    pub fn object_limit(&self) -> usize {
        self.object_limit.get()
    }

    /// Set the object ceiling; `usize::MAX` lifts it.
    pub fn set_object_limit(&self, objects: usize) {
        self.object_limit.set(objects);
    }

    /// How many pages a page tree may list.
    ///
    /// Defaults to [`DEFAULT_PAGE_LIMIT`]. Every [`pages`](crate::Document::pages)
    /// call walks the tree, so this bounds the work a crafted `/Pages` can ask
    /// for.
    pub fn page_limit(&self) -> usize {
        self.page_limit.get()
    }

    /// Set the page ceiling; `usize::MAX` lifts it.
    ///
    /// A document that legitimately has more pages than the default raises it,
    /// and a caller rendering one page at a time can lower it.
    pub fn set_page_limit(&self, pages: usize) {
        self.page_limit.set(pages);
    }

    /// Decode a stream's payload through its filter chain.
    ///
    /// Indirect `/Filter` and `/DecodeParms` entries are resolved through the
    /// object table first, which is what the plain [`Stream::decode`] cannot
    /// do on its own.
    pub fn decode_stream(&self, stream: &Stream) -> Result<Vec<u8>> {
        let dict = stream.dict();
        let mut resolved = Dictionary::new();
        if let Some(filter) = self.resolve(dict.get(b"Filter"))? {
            resolved.insert(b"Filter", filter);
        }
        if let Some(parms) = self.resolve(dict.get(b"DecodeParms"))? {
            resolved.insert(b"DecodeParms", parms);
        }
        // Decryption happens before decompression (§7.6.1).
        let data = match &self.encryption {
            Some(encryption) => encryption.decrypt_stream(stream)?,
            None => stream.data().to_vec(),
        };
        // `/CCITTFaxDecode` is a bitmap decoder rather than a byte filter, so it
        // is dispatched here — after decryption, before the chain is parsed —
        // instead of widening the public `Filter` enum with a variant that would
        // break every exhaustive match downstream.
        if Self::is_fax_stream(&resolved) {
            let parms = resolved.get(b"DecodeParms").and_then(Object::as_dict);
            return filter::fax_decode(&data, parms, self.decode_limit.get());
        }
        let chain = FilterSpec::from_dict(&resolved)?;
        filter::decode_chain_with_limit(&data, &chain, self.decode_limit.get())
    }

    /// Fetch a stream's payload with decryption applied and its filters left alone.
    ///
    /// A caller that brings its own decoder — the renderer's `/DCTDecode` path —
    /// needs the bytes the file holds, decrypted if the document is encrypted,
    /// because [`Document::decode_stream`] would refuse an image filter.
    pub fn stream_bytes(&self, id: ObjectId) -> Result<Vec<u8>> {
        let stream = self.stream(id)?;
        match &self.encryption {
            // Cross-reference and cleartext metadata streams pass through, as in
            // `decode_stream`; both rules live in `decrypt_stream`.
            Some(encryption) => encryption.decrypt_stream(&stream),
            None => Ok(stream.data().to_vec()),
        }
    }

    /// Fetch a stream by id and decode its payload in one step.
    pub fn stream_data(&self, id: ObjectId) -> Result<Vec<u8>> {
        let stream = self.stream(id)?;
        self.decode_stream(&stream)
    }

    /// Whether a resolved stream dictionary names `/CCITTFaxDecode`.
    fn is_fax_stream(dict: &Dictionary) -> bool {
        match dict.get(b"Filter") {
            Some(Object::Name(name)) => name == b"CCITTFaxDecode" || name == b"CCF",
            Some(Object::Array(items)) => items
                .first()
                .and_then(Object::as_name)
                .is_some_and(|name| name == b"CCITTFaxDecode" || name == b"CCF"),
            _ => false,
        }
    }

    /// Replace indirect references with the objects they point at.
    pub(crate) fn resolve(&self, object: Option<&Object>) -> Result<Option<Object>> {
        match object {
            None => Ok(None),
            Some(Object::Reference(id)) => Ok(Some(self.get_object(*id)?)),
            Some(Object::Array(items)) => {
                let mut resolved = Vec::with_capacity(items.len());
                for item in items {
                    resolved.push(
                        self.resolve(Some(item))?
                            .ok_or(Error::malformed("array entry"))?,
                    );
                }
                Ok(Some(Object::Array(resolved)))
            }
            Some(other) => Ok(Some(other.clone())),
        }
    }

    fn parse_at(&self, number: u32, offset: usize) -> Result<Object> {
        let depth = self.depth.get() + 1;
        if depth > MAX_RESOLVE_DEPTH {
            return Err(Error::Malformed {
                what: "object resolution nested too deeply",
                offset: Some(offset),
            });
        }
        self.depth.set(depth);
        let result = self.parse_at_inner(number, offset);
        self.depth.set(depth - 1);
        result
    }

    fn parse_at_inner(&self, number: u32, offset: usize) -> Result<Object> {
        let slice = self.data.get(offset..).ok_or(Error::Malformed {
            what: "cross-reference offset is past the end of the file",
            offset: Some(offset),
        })?;
        let mut parser = Parser::with_resolver(slice, self);
        let indirect = parser.parse_indirect_object()?;
        if indirect.id.number != number {
            return Err(Error::Malformed {
                what: "cross-reference offset points at a different object",
                offset: Some(offset),
            });
        }
        Ok(indirect.object)
    }
}

impl Document {
    /// Fetch an object stored inside an object stream, extracting the whole
    /// stream once and caching every object it holds.
    fn compressed_object(&self, number: u32, stream: u32, index: u32) -> Result<Object> {
        self.load_object_stream(stream)?;
        {
            let object_streams = self.object_streams.borrow();
            let numbers = object_streams
                .get(&stream)
                .ok_or_else(|| Error::malformed("object stream was not indexed"))?;
            match numbers.get(usize::try_from(index).unwrap_or(usize::MAX)) {
                Some(found) if *found == number => {}
                Some(_) => {
                    return Err(Error::Malformed {
                        what: "object stream index points at a different object",
                        offset: None,
                    });
                }
                None => {
                    return Err(Error::Malformed {
                        what: "object stream index is out of range",
                        offset: None,
                    });
                }
            }
        }
        self.cache
            .borrow()
            .get(&ObjectId::new(number, 0))
            .cloned()
            .ok_or_else(|| Error::malformed("object missing from its object stream"))
    }

    /// Decode an `/ObjStm` stream and cache the objects it contains.
    fn load_object_stream(&self, stream_number: u32) -> Result<()> {
        if self.object_streams.borrow().contains_key(&stream_number) {
            return Ok(());
        }
        let stream = match self.get_object_by_number(stream_number)? {
            Object::Stream(stream) => stream,
            _ => {
                return Err(Error::Malformed {
                    what: "object stream entry points at an object that is not a stream",
                    offset: None,
                });
            }
        };
        let count = stream_integer(&stream, b"N")?;
        let first = stream_integer(&stream, b"First")?;
        let data = self.decode_stream(&stream)?;
        if first > data.len() {
            return Err(Error::Malformed {
                what: "object stream /First is past the end of its data",
                offset: None,
            });
        }
        // The header is `count` pairs of object number and relative offset.
        let mut parser = Parser::new(&data);
        let mut entries = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let number = parser
                .parse_object()?
                .as_integer()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| Error::malformed("object stream header object number"))?;
            let offset = parser
                .parse_object()?
                .as_integer()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| Error::malformed("object stream header offset"))?;
            entries.push((number, offset));
        }
        let mut numbers = Vec::with_capacity(entries.len());
        let mut objects = Vec::with_capacity(entries.len());
        for (number, offset) in entries {
            let start = first.checked_add(offset).ok_or_else(|| {
                Error::malformed("object stream offset overflows the file length")
            })?;
            let slice = data.get(start..).ok_or(Error::Malformed {
                what: "object stream offset is past the end of its data",
                offset: Some(start),
            })?;
            let mut parser = Parser::new(slice);
            objects.push((number, parser.parse_object()?));
            numbers.push(number);
        }
        let mut cache = self.cache.borrow_mut();
        for (number, object) in objects {
            cache.insert(ObjectId::new(number, 0), object);
        }
        drop(cache);
        self.object_streams
            .borrow_mut()
            .insert(stream_number, numbers);
        Ok(())
    }
}

/// A required integer entry of a stream dictionary, as a `usize`.
fn stream_integer(stream: &Stream, key: &[u8]) -> Result<usize> {
    match stream.dict().get(key) {
        Some(Object::Integer(value)) => usize::try_from(*value).map_err(|_| Error::Malformed {
            what: "stream dictionary entry must not be negative",
            offset: None,
        }),
        _ => Err(Error::Malformed {
            what: "object stream needs integer /N and /First entries",
            offset: None,
        }),
    }
}

impl LengthResolver for Document {
    fn resolve_length(&self, id: ObjectId) -> Result<usize> {
        match self.get_object(id)? {
            Object::Integer(length) => usize::try_from(length).map_err(|_| Error::Malformed {
                what: "stream /Length must not be negative",
                offset: None,
            }),
            _ => Err(Error::Malformed {
                what: "stream /Length must be an integer",
                offset: None,
            }),
        }
    }
}

/// Read the version out of the `%PDF-` header.
fn header_version(data: &[u8]) -> Result<String> {
    let limit = data.len().min(HEADER_SEARCH_LIMIT);
    let start = data[..limit]
        .windows(HEADER.len())
        .position(|window| window == HEADER)
        .ok_or(Error::Malformed {
            what: "missing %PDF- header",
            offset: None,
        })?;
    let rest = &data[start + HEADER.len()..];
    let end = rest
        .iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .unwrap_or(rest.len())
        .min(8);
    let version = core::str::from_utf8(&rest[..end]).map_err(|_| Error::Malformed {
        what: "header version is not ASCII",
        offset: Some(start),
    })?;
    let version = version.trim();
    // Both major versions this reader understands. 1.x is the format of almost
    // every file in the world; refusing it at the header made the library
    // unmeasurable on real documents (docs/CORPUS.md).
    let valid = version.split_once('.').is_some_and(|(major, minor)| {
        matches!(major, "1" | "2") && !minor.is_empty() && minor.bytes().all(|b| b.is_ascii_digit())
    });
    if !valid {
        return Err(Error::Malformed {
            what: "header version is not 1.x or 2.x",
            offset: Some(start),
        });
    }
    Ok(version.to_owned())
}

/// Offset of the last occurrence of `needle`, if any.
fn find_last(data: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || data.len() < needle.len() {
        return None;
    }
    data.windows(needle.len())
        .rposition(|window| window == needle)
}

/// Find the last `startxref` and the offset it points at.
fn startxref_offset(data: &[u8]) -> Result<usize> {
    let at = find_last(data, b"startxref").ok_or(Error::Malformed {
        what: "missing startxref",
        offset: None,
    })?;
    if find_last(data, b"%%EOF").is_none() {
        return Err(Error::Malformed {
            what: "missing %%EOF",
            offset: None,
        });
    }
    let rest = &data[at + b"startxref".len()..];
    let digits_start =
        rest.iter()
            .position(|byte| byte.is_ascii_digit())
            .ok_or(Error::Malformed {
                what: "startxref without an offset",
                offset: Some(at),
            })?;
    let digits_end = digits_start
        + rest[digits_start..]
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .unwrap_or(rest.len() - digits_start);
    let text =
        core::str::from_utf8(&rest[digits_start..digits_end]).map_err(|_| Error::Malformed {
            what: "startxref offset is not a number",
            offset: Some(at),
        })?;
    let offset: usize = text.parse().map_err(|_| Error::Malformed {
        what: "startxref offset is not a number",
        offset: Some(at),
    })?;
    if offset >= data.len() {
        return Err(Error::Malformed {
            what: "startxref offset is past the end of the file",
            offset: Some(at),
        });
    }
    Ok(offset)
}

/// Read the newest cross-reference section and every `/Prev` before it.
///
/// The newest entry for an object number wins: an incremental update re-points
/// objects and the older sections only fill in the numbers it does not mention.
fn read_xref_chain(data: &[u8]) -> Result<(BTreeMap<u32, XrefEntry>, Dictionary)> {
    let mut offset = startxref_offset(data)?;
    let mut merged = BTreeMap::new();
    let mut newest_trailer: Option<Dictionary> = None;
    let mut visited = BTreeSet::new();
    for _ in 0..MAX_XREF_SECTIONS {
        if !visited.insert(offset) {
            return Err(Error::Malformed {
                what: "cross-reference /Prev chain loops",
                offset: Some(offset),
            });
        }
        let (section, trailer) = parse_xref_section(data, offset)?;
        for (number, entry) in section {
            merged.entry(number).or_insert(entry);
        }
        // Hybrid files keep a classic table for old readers and hide the
        // compressed objects in a cross-reference stream named by `/XRefStm`.
        // The classic entries win; the stream only fills the gaps.
        if let Some(xref_stream) = trailer.get(b"XRefStm") {
            let stream_offset = xref_stream
                .as_integer()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or(Error::Malformed {
                    what: "trailer /XRefStm must be a non-negative integer",
                    offset: Some(offset),
                })?;
            if !visited.insert(stream_offset) {
                return Err(Error::Malformed {
                    what: "cross-reference /XRefStm loops",
                    offset: Some(stream_offset),
                });
            }
            let (stream_entries, _) = parse_xref_stream(data, stream_offset)?;
            for (number, entry) in stream_entries {
                merged.entry(number).or_insert(entry);
            }
        }
        let previous = match trailer.get(b"Prev") {
            None => {
                if newest_trailer.is_none() {
                    newest_trailer = Some(trailer);
                }
                break;
            }
            Some(Object::Integer(previous)) => *previous,
            Some(_) => {
                return Err(Error::Malformed {
                    what: "trailer /Prev must be an integer",
                    offset: Some(offset),
                });
            }
        };
        let previous = usize::try_from(previous).map_err(|_| Error::Malformed {
            what: "trailer /Prev must not be negative",
            offset: Some(offset),
        })?;
        if newest_trailer.is_none() {
            newest_trailer = Some(trailer);
        }
        if previous >= data.len() {
            return Err(Error::Malformed {
                what: "trailer /Prev is past the end of the file",
                offset: Some(offset),
            });
        }
        offset = previous;
    }
    let trailer = newest_trailer.ok_or(Error::Malformed {
        what: "no cross-reference section could be read",
        offset: None,
    })?;
    Ok((merged, trailer))
}

/// One `xref` table and the trailer that follows it.
/// Read the cross-reference section at `offset`, repairing a bad hint.
///
/// Writers get `startxref` wrong in ordinary ways — pointing inside the table
/// instead of at its keyword, or a few bytes off — and every reader that refuses
/// the file for it is wrong on a file that is otherwise fine: the corpus has one
/// (`encrypted-attachment.pdf`, offset 2242 against `xref` at 2223). The repair
/// is bounded and stated: if the offset does not lead to a section, the nearest
/// `xref` keyword before it is tried, and only then is the original error
/// reported.
fn parse_xref_section(
    data: &[u8],
    offset: usize,
) -> Result<(BTreeMap<u32, XrefEntry>, Dictionary)> {
    if let Ok(section) = parse_xref_section_at(data, offset) {
        return Ok(section);
    }
    let before = &data[..offset.min(data.len())];
    if let Some(found) = find_last(before, b"\nxref")
        && let Ok(section) = parse_xref_section_at(data, found + 1)
    {
        return Ok(section);
    }
    parse_xref_section_at(data, offset)
}

/// The section at exactly `offset`: a classic `xref` table or a `/Type /XRef`
/// stream, decided by the first token, because both are reached the same way
/// from `startxref`, `/Prev` and `/XRefStm`.
fn parse_xref_section_at(
    data: &[u8],
    offset: usize,
) -> Result<(BTreeMap<u32, XrefEntry>, Dictionary)> {
    let slice = data.get(offset..).ok_or(Error::Malformed {
        what: "cross-reference offset is past the end of the file",
        offset: Some(offset),
    })?;
    let mut parser = Parser::new(slice);
    match parser.next_token()? {
        Token::Keyword(keyword) if keyword == b"xref" => parse_xref_table(data, offset, parser),
        Token::Integer(_) => parse_xref_stream(data, offset),
        _ => Err(Error::Malformed {
            what: "startxref does not point at an xref table or stream",
            offset: Some(offset),
        }),
    }
}

/// Read a classic `xref` table; `parser` has already consumed the keyword.
fn parse_xref_table(
    slice_source: &[u8],
    offset: usize,
    mut parser: Parser<'_>,
) -> Result<(BTreeMap<u32, XrefEntry>, Dictionary)> {
    let slice = slice_source;
    let mut entries = BTreeMap::new();
    loop {
        match parser.peek_token()? {
            Token::Keyword(keyword) if keyword == b"trailer" => {
                parser.next_token()?;
                break;
            }
            Token::Integer(_) => {
                let first = as_integer(parser.next_token()?)?;
                let count = as_integer(parser.next_token()?)?;
                let first = u32::try_from(first).map_err(|_| Error::Malformed {
                    what: "cross-reference subsection start",
                    offset: Some(offset),
                })?;
                // Each entry needs at least three short tokens; refuse counts
                // that cannot possibly fit in the remaining bytes.
                let count = usize::try_from(count)
                    .ok()
                    .filter(|count| *count <= slice.len())
                    .ok_or(Error::Malformed {
                        what: "cross-reference subsection count",
                        offset: Some(offset),
                    })?;
                for index in 0..count {
                    let entry_offset = as_integer(parser.next_token()?)?;
                    let generation = as_integer(parser.next_token()?)?;
                    let kind = match parser.next_token()? {
                        Token::Keyword(kind) => kind,
                        _ => {
                            return Err(Error::Malformed {
                                what: "cross-reference entry kind",
                                offset: Some(offset),
                            });
                        }
                    };
                    if generation < 0 {
                        return Err(Error::Malformed {
                            what: "cross-reference generation",
                            offset: Some(offset),
                        });
                    }
                    // A free entry's generation is the next one to hand out and
                    // is never used for a lookup, so a writer that says 65536
                    // for the free head (the standard says 65535) is clamped
                    // rather than refused; an in-use entry stays strict.
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let generation = u16::try_from(generation).unwrap_or(u16::MAX);
                    let index = u32::try_from(index).map_err(|_| Error::Malformed {
                        what: "cross-reference subsection count",
                        offset: Some(offset),
                    })?;
                    let number = first.checked_add(index).ok_or(Error::Malformed {
                        what: "cross-reference object number",
                        offset: Some(offset),
                    })?;
                    let entry = match kind.as_slice() {
                        b"n" => XrefEntry::InUse {
                            offset: usize::try_from(entry_offset).map_err(|_| {
                                Error::Malformed {
                                    what: "cross-reference offset must not be negative",
                                    offset: Some(offset),
                                }
                            })?,
                            generation,
                        },
                        b"f" => XrefEntry::Free { generation },
                        _ => {
                            return Err(Error::Malformed {
                                what: "cross-reference entry kind must be n or f",
                                offset: Some(offset),
                            });
                        }
                    };
                    entries.entry(number).or_insert(entry);
                }
            }
            Token::Eof => {
                return Err(Error::Malformed {
                    what: "cross-reference table without a trailer",
                    offset: Some(offset),
                });
            }
            _ => {
                return Err(Error::Malformed {
                    what: "cross-reference subsection header",
                    offset: Some(offset),
                });
            }
        }
    }
    let trailer = match parser.parse_object()? {
        Object::Dictionary(trailer) => trailer,
        _ => {
            return Err(Error::Malformed {
                what: "trailer is not a dictionary",
                offset: Some(offset),
            });
        }
    };
    Ok((entries, trailer))
}

/// Read a `/Type /XRef` cross-reference stream at `offset` (§7.5.8).
fn parse_xref_stream(data: &[u8], offset: usize) -> Result<(BTreeMap<u32, XrefEntry>, Dictionary)> {
    let slice = data.get(offset..).ok_or(Error::Malformed {
        what: "cross-reference stream offset is past the end of the file",
        offset: Some(offset),
    })?;
    let mut parser = Parser::new(slice);
    let stream = match parser.parse_indirect_object()?.object {
        Object::Stream(stream) => stream,
        _ => {
            return Err(Error::Malformed {
                what: "startxref points at an object that is not a cross-reference stream",
                offset: Some(offset),
            });
        }
    };
    let dict = stream.dict().clone();
    if dict.get(b"Type") != Some(&Object::name("XRef")) {
        return Err(Error::Malformed {
            what: "cross-reference stream without /Type /XRef",
            offset: Some(offset),
        });
    }
    let widths = xref_widths(&dict)?;
    let index = xref_index(&dict)?;
    let chain = FilterSpec::from_dict(&dict)?;
    let decoded = filter::decode_chain(stream.data(), &chain)?;
    let row_bytes: usize = widths.iter().map(|width| usize::from(*width)).sum();
    if row_bytes == 0 {
        return Err(Error::Malformed {
            what: "/W of a cross-reference stream must not be all zero",
            offset: Some(offset),
        });
    }
    let mut entries = BTreeMap::new();
    let mut position = 0;
    for (first, count) in index {
        if usize::try_from(count).map_or(true, |count| count > decoded.len()) {
            return Err(Error::Malformed {
                what: "cross-reference stream /Index count is implausible",
                offset: Some(offset),
            });
        }
        for index in 0..count {
            let row = decoded
                .get(position..position + row_bytes)
                .ok_or(Error::unexpected_eof("cross-reference stream entry"))?;
            position += row_bytes;
            let mut cursor = 0;
            let kind = read_xref_field(row, &mut cursor, widths[0], 1)?;
            let field_two = read_xref_field(row, &mut cursor, widths[1], 0)?;
            let field_three = read_xref_field(row, &mut cursor, widths[2], 0)?;
            let number = first.checked_add(index).ok_or(Error::Malformed {
                what: "cross-reference stream object number overflows",
                offset: Some(offset),
            })?;
            let entry = match kind {
                0 => XrefEntry::Free {
                    generation: u16::try_from(field_three).unwrap_or(u16::MAX),
                },
                1 => XrefEntry::InUse {
                    offset: usize::try_from(field_two).map_err(|_| Error::Malformed {
                        what: "cross-reference stream offset is out of range",
                        offset: Some(offset),
                    })?,
                    generation: u16::try_from(field_three).unwrap_or(u16::MAX),
                },
                2 => XrefEntry::Compressed {
                    stream: u32::try_from(field_two).map_err(|_| Error::Malformed {
                        what: "object stream number is out of range",
                        offset: Some(offset),
                    })?,
                    index: u32::try_from(field_three).map_err(|_| Error::Malformed {
                        what: "object stream index is out of range",
                        offset: Some(offset),
                    })?,
                },
                other => {
                    return Err(Error::Malformed {
                        what: match other {
                            3.. => "unknown cross-reference stream entry type",
                            _ => "cross-reference stream entry type",
                        },
                        offset: Some(offset),
                    });
                }
            };
            entries.entry(number).or_insert(entry);
        }
    }
    Ok((entries, dict))
}

/// The three field widths from `/W`.
fn xref_widths(dict: &Dictionary) -> Result<[u8; 3]> {
    let Some(Object::Array(items)) = dict.get(b"W") else {
        return Err(Error::Malformed {
            what: "cross-reference stream needs a /W array",
            offset: None,
        });
    };
    if items.len() != 3 {
        return Err(Error::Malformed {
            what: "/W must hold exactly three integers",
            offset: None,
        });
    }
    let mut widths = [0_u8; 3];
    for (slot, item) in widths.iter_mut().zip(items) {
        let width = item
            .as_integer()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|width| *width <= 8)
            .ok_or(Error::Malformed {
                what: "/W entries must be integers from 0 to 8",
                offset: None,
            })?;
        *slot = width;
    }
    Ok(widths)
}

/// The `(first, count)` subsections from `/Index`, defaulting to `0 … /Size`.
fn xref_index(dict: &Dictionary) -> Result<Vec<(u32, u32)>> {
    let Some(index) = dict.get(b"Index") else {
        let size = dict
            .get(b"Size")
            .and_then(Object::as_integer)
            .and_then(|size| u32::try_from(size).ok())
            .ok_or(Error::Malformed {
                what: "cross-reference stream needs a positive /Size",
                offset: None,
            })?;
        return Ok(vec![(0, size)]);
    };
    let Object::Array(items) = index else {
        return Err(Error::Malformed {
            what: "/Index must be an array of integer pairs",
            offset: None,
        });
    };
    if items.len() % 2 != 0 {
        return Err(Error::Malformed {
            what: "/Index must hold an even number of integers",
            offset: None,
        });
    }
    let mut subsections = Vec::with_capacity(items.len() / 2);
    for pair in items.chunks(2) {
        let first = pair[0].as_integer().and_then(|v| u32::try_from(v).ok());
        let count = pair[1].as_integer().and_then(|v| u32::try_from(v).ok());
        match (first, count) {
            (Some(first), Some(count)) => subsections.push((first, count)),
            _ => {
                return Err(Error::Malformed {
                    what: "/Index pairs must be non-negative integers",
                    offset: None,
                });
            }
        }
    }
    Ok(subsections)
}

/// One big-endian field of a cross-reference stream entry.
fn read_xref_field(row: &[u8], cursor: &mut usize, width: u8, default: u64) -> Result<u64> {
    let width = usize::from(width);
    if width == 0 {
        return Ok(default);
    }
    let field = row.get(*cursor..*cursor + width).ok_or(Error::Malformed {
        what: "cross-reference stream entry is shorter than /W says",
        offset: None,
    })?;
    *cursor += width;
    Ok(field
        .iter()
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte)))
}

fn as_integer(token: Token) -> Result<i64> {
    match token {
        Token::Integer(value) => Ok(value),
        _ => Err(Error::Malformed {
            what: "cross-reference entries must be integers",
            offset: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{find_last, header_version, parse_xref_section, read_xref_chain, startxref_offset};

    /// A minimal classic-xref PDF built with correct offsets.
    fn minimal_pdf() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-2.0\n");
        let first = out.len();
        out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog >>\nendobj\n");
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 2\n");
        out.extend_from_slice(b"0000000000 65535 f \n");
        out.extend_from_slice(format!("{first:010} 00000 n \n").as_bytes());
        out.extend_from_slice(b"trailer\n<< /Size 2 /Root 1 0 R >>\n");
        out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    #[test]
    fn header_versions_are_sniffed() {
        assert_eq!(header_version(b"%PDF-2.0\nrest").expect("header"), "2.0");
        assert_eq!(
            header_version(b"junk%PDF-2.0\r\nrest").expect("header"),
            "2.0",
            "the header may sit within the first 1024 bytes"
        );
        assert_eq!(
            header_version(b"%PDF-1.7\n").expect("1.x header"),
            "1.7",
            "PDF 1.x is the format of almost every real file"
        );
        assert_eq!(header_version(b"%PDF-1.4\r\n").expect("1.x header"), "1.4");
        assert!(header_version(b"%PDF-3.0\n").is_err());
        assert!(header_version(b"%PDF-2.\n").is_err());
        assert!(header_version(b"%PDF-x.0\n").is_err());
        assert!(header_version(b"no header at all").is_err());
        assert!(header_version(b"").is_err());
    }

    #[test]
    fn startxref_is_found_after_the_tables() {
        let pdf = minimal_pdf();
        let offset = startxref_offset(&pdf).expect("startxref");
        assert!(pdf[offset..].starts_with(b"xref"));
        assert!(startxref_offset(b"%PDF-2.0\n%%EOF").is_err());
        assert!(startxref_offset(b"%PDF-2.0\nstartxref\n999999\n%%EOF").is_err());
    }

    #[test]
    fn cross_reference_sections_parse_subsections() {
        let pdf = minimal_pdf();
        let offset = startxref_offset(&pdf).expect("startxref");
        let (entries, trailer) = parse_xref_section(&pdf, offset).expect("section");
        assert_eq!(entries.len(), 2);
        assert_eq!(trailer.get(b"Size"), Some(&crate::Object::Integer(2)));
        let (merged, newest) = read_xref_chain(&pdf).expect("chain");
        assert_eq!(merged.len(), 2);
        assert_eq!(newest.get(b"Size"), Some(&crate::Object::Integer(2)));
    }

    fn dict_of(entries: &[(&str, crate::Object)]) -> crate::Dictionary {
        let mut dict = crate::Dictionary::new();
        for (name, value) in entries {
            dict.insert(name.as_bytes(), value.clone());
        }
        dict
    }

    #[test]
    fn xref_stream_widths_and_index_have_spec_defaults() {
        let dict = dict_of(&[
            ("Size", crate::Object::Integer(7)),
            (
                "W",
                crate::Object::array([
                    crate::Object::Integer(1),
                    crate::Object::Integer(4),
                    crate::Object::Integer(2),
                ]),
            ),
        ]);
        assert_eq!(super::xref_widths(&dict).expect("widths"), [1, 4, 2]);
        assert_eq!(
            super::xref_index(&dict).expect("default index"),
            vec![(0, 7)],
            "/Index defaults to one subsection covering 0..Size"
        );

        let dict = dict_of(&[
            (
                "W",
                crate::Object::array([
                    crate::Object::Integer(1),
                    crate::Object::Integer(4),
                    crate::Object::Integer(2),
                ]),
            ),
            (
                "Index",
                crate::Object::array([
                    crate::Object::Integer(4),
                    crate::Object::Integer(1),
                    crate::Object::Integer(7),
                    crate::Object::Integer(1),
                ]),
            ),
        ]);
        assert_eq!(
            super::xref_index(&dict).expect("explicit index"),
            vec![(4, 1), (7, 1)]
        );

        let bad = dict_of(&[("Size", crate::Object::Integer(1))]);
        assert!(super::xref_widths(&bad).is_err(), "/W is required");
        let bad = dict_of(&[
            ("Size", crate::Object::Integer(1)),
            ("W", crate::Object::array([crate::Object::Integer(9)])),
        ]);
        assert!(super::xref_widths(&bad).is_err(), "/W larger than 8 bytes");
        let bad = dict_of(&[
            ("Size", crate::Object::Integer(1)),
            ("W", crate::Object::array([crate::Object::Integer(0)])),
        ]);
        assert!(super::xref_widths(&bad).is_err(), "/W must not be all zero");
    }

    #[test]
    fn zero_width_fields_take_their_defaults() {
        // `w0 = 0` means "type 1" and `w2 = 0` means "generation 0".
        let row = [0x00, 0x00, 0x01];
        let mut cursor = 0;
        assert_eq!(
            super::read_xref_field(&row, &mut cursor, 0, 1).expect("default"),
            1
        );
        assert_eq!(cursor, 0, "a zero-width field consumes nothing");
        assert_eq!(
            super::read_xref_field(&row, &mut cursor, 2, 0).expect("value"),
            0x0000
        );
        assert_eq!(
            super::read_xref_field(&row, &mut cursor, 1, 0).expect("value"),
            1
        );
        assert_eq!(cursor, 3);
        // Reading past the row is an error, not a panic.
        assert!(super::read_xref_field(&row, &mut cursor, 2, 0).is_err());
    }

    #[test]
    fn cross_reference_streams_map_all_three_entry_types() {
        // Three rows of /W [1 2 1]: free, in-use, compressed.
        let mut data =
            b"%PDF-2.0\n7 0 obj\n<< /Type /XRef /Size 3 /Length 12 /W [1 2 1] >>\nstream\n"
                .to_vec();
        let offset = 9;
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&[1, 0x03, 0xE7, 5]);
        data.extend_from_slice(&[2, 0x00, 0x07, 3]);
        data.extend_from_slice(b"\nendstream\nendobj\n");
        let (entries, dict) = super::parse_xref_stream(&data, offset).expect("section");
        assert_eq!(dict.get(b"Size"), Some(&crate::Object::Integer(3)));
        assert_eq!(
            entries.get(&0),
            Some(&super::XrefEntry::Free { generation: 0 })
        );
        assert_eq!(
            entries.get(&1),
            Some(&super::XrefEntry::InUse {
                offset: 999,
                generation: 5
            })
        );
        assert_eq!(
            entries.get(&2),
            Some(&super::XrefEntry::Compressed {
                stream: 7,
                index: 3
            })
        );
    }

    #[test]
    fn a_cross_reference_stream_without_type_is_rejected() {
        let mut data = b"7 0 obj\n<< /Size 3 /Length 4 /W [1 2 1] >>\nstream\n".to_vec();
        let offset = 9;
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(b"\nendstream\nendobj\n");
        assert!(super::parse_xref_stream(&data, offset).is_err());
    }

    #[test]
    fn find_last_returns_the_final_match() {
        assert_eq!(find_last(b"abab", b"ab"), Some(2));
        assert_eq!(find_last(b"abab", b"zz"), None);
        assert_eq!(find_last(b"a", b"ab"), None);
    }
}
