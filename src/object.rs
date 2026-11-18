//! PDF base objects and their serialization.
//!
//! [`Object`] models every base type of ISO 32000-2 §7.3: null, booleans,
//! integers, reals, strings, names, arrays, dictionaries and indirect
//! references. The serializers here are the writer side of the round-trip that
//! M2 requires: the tokenizer added in Q2 parses exactly the bytes produced
//! below, so `parse → write` and `write → parse` both preserve bytes.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use core::fmt;

/// An indirect object number together with its generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId {
    /// Object number, the first value of an `n g R` reference.
    pub number: u32,
    /// Generation number, the second value of an `n g R` reference.
    pub generation: u16,
}

impl ObjectId {
    /// Build an object id.
    pub const fn new(number: u32, generation: u16) -> Self {
        Self { number, generation }
    }
}

/// The syntax a [`PdfString`] is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StringFormat {
    /// `(...)`, with escapes for parentheses, backslash and binary bytes.
    Literal,
    /// `<...>`, one hex pair per byte, uppercase.
    Hex,
}

/// A PDF string: the decoded bytes plus the syntax they were written with.
///
/// Both forms are kept so that a parsed file writes back byte-for-byte, but
/// equality compares the decoded bytes only — `(Hello)` and `<48656C6C6F>` are
/// the same string object.
#[derive(Debug, Clone)]
pub struct PdfString {
    format: StringFormat,
    bytes: Vec<u8>,
}

impl PdfString {
    /// A string written in literal `(...)` form.
    pub fn literal(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            format: StringFormat::Literal,
            bytes: bytes.into(),
        }
    }

    /// A string written in hex `<...>` form.
    pub fn hex(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            format: StringFormat::Hex,
            bytes: bytes.into(),
        }
    }

    /// The syntax this string is written with.
    pub fn format(&self) -> StringFormat {
        self.format
    }

    /// The decoded bytes, without any escaping.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consume the string and return its decoded bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Replace the decoded bytes, keeping the serialization format.
    ///
    /// The decryption path in Q9 uses this to swap in plain text.
    pub fn set_bytes(&mut self, bytes: impl Into<Vec<u8>>) {
        self.bytes = bytes.into();
    }
}

impl PartialEq for PdfString {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for PdfString {}

impl From<&str> for PdfString {
    fn from(value: &str) -> Self {
        Self::literal(value)
    }
}

impl From<Vec<u8>> for PdfString {
    fn from(value: Vec<u8>) -> Self {
        Self::literal(value)
    }
}

/// A PDF dictionary: ordered name → object entries.
///
/// Insertion order is preserved and reproduced by the serializer, so a parsed
/// dictionary writes back as the bytes it came from. PDF dictionaries are not
/// sorted on disk and re-ordering them would break byte-exact round-trips.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dictionary {
    entries: Vec<(Vec<u8>, Object)>,
}

impl Dictionary {
    /// An empty dictionary.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `name` → `value`, replacing an existing entry in place.
    ///
    /// An entry that is already present keeps its position and its old value is
    /// returned.
    pub fn insert(&mut self, name: impl Into<Vec<u8>>, value: Object) -> Option<Object> {
        let name = name.into();
        if let Some((_, slot)) = self.entries.iter_mut().find(|(key, _)| *key == name) {
            return Some(core::mem::replace(slot, value));
        }
        self.entries.push((name, value));
        None
    }

    /// The value stored under `name`, if any.
    pub fn get(&self, name: &[u8]) -> Option<&Object> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// The value stored under `name`, for in-place updates.
    pub fn get_mut(&mut self, name: &[u8]) -> Option<&mut Object> {
        self.entries
            .iter_mut()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// Remove `name`'s entry, returning the value it held.
    ///
    /// An edit that drops a reference needs this: a key cannot be "set to
    /// absent" by writing `null`, because a reader distinguishes a missing key
    /// (inherit, fall back) from an explicit `null` (present, empty).
    pub fn remove(&mut self, name: &[u8]) -> Option<Object> {
        let position = self.entries.iter().position(|(key, _)| key == name)?;
        Some(self.entries.remove(position).1)
    }

    /// Whether `name` has an entry.
    pub fn contains_key(&self, name: &[u8]) -> bool {
        self.get(name).is_some()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the dictionary has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries in insertion order.
    pub fn entries(&self) -> &[(Vec<u8>, Object)] {
        &self.entries
    }

    /// Iterate over `(name, value)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&[u8], &Object)> + '_ {
        self.entries
            .iter()
            .map(|(name, value)| (name.as_slice(), value))
    }

    fn write_to(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"<<");
        for (name, value) in &self.entries {
            out.push(b' ');
            write_name(name, out);
            out.push(b' ');
            value.write_to(out);
        }
        if !self.entries.is_empty() {
            out.push(b' ');
        }
        out.extend_from_slice(b">>");
    }
}

impl From<Vec<(Vec<u8>, Object)>> for Dictionary {
    fn from(entries: Vec<(Vec<u8>, Object)>) -> Self {
        Self { entries }
    }
}

/// A stream object: a dictionary plus the raw bytes between `stream` and
/// `endstream`.
///
/// The data is kept exactly as it appears in the file — still encoded, still
/// encrypted if the document is. Decoding is the filter layer's job (Q4);
/// decryption is Q9's, and both use [`Stream::set_data`] so that `/Length`
/// never drifts away from the bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    dict: Dictionary,
    data: Vec<u8>,
}

impl Stream {
    /// A stream from a dictionary and its raw data.
    ///
    /// `/Length` is not rewritten here: the dictionary is used exactly as
    /// given, which is what a parsed stream needs for a byte-exact write-back.
    pub fn new(dict: Dictionary, data: impl Into<Vec<u8>>) -> Self {
        Self {
            dict,
            data: data.into(),
        }
    }

    /// The stream dictionary.
    pub fn dict(&self) -> &Dictionary {
        &self.dict
    }

    /// The stream dictionary, for in-place edits.
    pub fn dict_mut(&mut self) -> &mut Dictionary {
        &mut self.dict
    }

    /// The raw stream bytes, as stored in the file.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Consume the stream and return its raw bytes.
    pub fn into_data(self) -> Vec<u8> {
        self.data
    }

    /// Replace the raw data and keep `/Length` in step with it.
    ///
    /// Used after decoding or decrypting, when the bytes change but the
    /// dictionary must stay consistent with what would be written out.
    pub fn set_data(&mut self, data: impl Into<Vec<u8>>) {
        let data = data.into();
        self.dict
            .insert(b"Length", Object::Integer(data.len() as i64));
        self.data = data;
    }
}

/// A PDF object: any value that can appear inside a document.
///
/// `Real` holds an `f64`, so the enum is `PartialEq` but not `Eq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    /// `null`.
    Null,
    /// `true` or `false`.
    Boolean(bool),
    /// An integer, written without a decimal point.
    Integer(i64),
    /// A real number, always written with a decimal point.
    Real(f64),
    /// A string, literal or hex.
    String(PdfString),
    /// A name, stored decoded: no leading `/`, `#xx` escapes resolved.
    Name(Vec<u8>),
    /// An array.
    Array(Vec<Object>),
    /// A dictionary.
    Dictionary(Dictionary),
    /// A stream, only valid as the value of an indirect object.
    Stream(Stream),
    /// An indirect reference, `n g R`.
    Reference(ObjectId),
}

impl Object {
    /// A name object from decoded bytes.
    pub fn name(name: impl Into<Vec<u8>>) -> Self {
        Self::Name(name.into())
    }

    /// A literal string object.
    pub fn string(bytes: impl Into<Vec<u8>>) -> Self {
        Self::String(PdfString::literal(bytes))
    }

    /// A hex string object.
    pub fn hex_string(bytes: impl Into<Vec<u8>>) -> Self {
        Self::String(PdfString::hex(bytes))
    }

    /// An array object.
    pub fn array(items: impl IntoIterator<Item = Object>) -> Self {
        Self::Array(items.into_iter().collect())
    }

    /// A reference object.
    pub const fn reference(id: ObjectId) -> Self {
        Self::Reference(id)
    }

    /// Whether this is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The integer value, if this is an integer.
    pub fn as_integer(&self) -> Option<i64> {
        match self {
            Self::Integer(value) => Some(*value),
            _ => None,
        }
    }

    /// The numeric value as a real, accepting an integer too.
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Real(value) => Some(*value),
            _ => None,
        }
    }

    /// The name bytes, if this is a name.
    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            Self::Name(name) => Some(name),
            _ => None,
        }
    }

    /// The array items, if this is an array.
    pub fn as_array(&self) -> Option<&[Object]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The dictionary, if this is one.
    pub fn as_dict(&self) -> Option<&Dictionary> {
        match self {
            Self::Dictionary(dict) => Some(dict),
            _ => None,
        }
    }

    /// The stream, if this is one.
    pub fn as_stream(&self) -> Option<&Stream> {
        match self {
            Self::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    /// The string, if this is one.
    pub fn as_string(&self) -> Option<&PdfString> {
        match self {
            Self::String(string) => Some(string),
            _ => None,
        }
    }

    /// The reference target, if this is a reference.
    pub fn as_reference(&self) -> Option<ObjectId> {
        match self {
            Self::Reference(id) => Some(*id),
            _ => None,
        }
    }

    /// Append the serialized form of this object to `out`.
    ///
    /// The output is the canonical text form used by this crate: names keep
    /// only regular characters, strings keep the format they were parsed with,
    /// reals always carry a decimal point, and containers separate their
    /// elements with single spaces.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self {
            Self::Null => out.extend_from_slice(b"null"),
            Self::Boolean(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
            Self::Integer(value) => out.extend_from_slice(value.to_string().as_bytes()),
            Self::Real(value) => out.extend_from_slice(real_to_string(*value).as_bytes()),
            Self::String(string) => match string.format() {
                StringFormat::Literal => write_literal(string.as_bytes(), out),
                StringFormat::Hex => write_hex(string.as_bytes(), out),
            },
            Self::Name(name) => write_name(name, out),
            Self::Array(items) => {
                out.push(b'[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(b' ');
                    }
                    item.write_to(out);
                }
                out.push(b']');
            }
            Self::Dictionary(dict) => dict.write_to(out),
            Self::Stream(stream) => {
                stream.dict.write_to(out);
                out.extend_from_slice(b"\nstream\n");
                out.extend_from_slice(stream.data());
                out.extend_from_slice(b"\nendstream");
            }
            Self::Reference(id) => {
                out.extend_from_slice(id.number.to_string().as_bytes());
                out.push(b' ');
                out.extend_from_slice(id.generation.to_string().as_bytes());
                out.extend_from_slice(b" R");
            }
        }
    }

    /// Serialize this object into a fresh buffer.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_to(&mut out);
        out
    }
}

/// Whether `byte` may appear in a name without a `#xx` escape.
///
/// Regular characters are printable ASCII minus delimiters and `#`, which is
/// the escape introducer itself (ISO 32000-2 §7.3.5).
fn is_regular(byte: u8) -> bool {
    matches!(byte, 0x21..=0x7e)
        && !matches!(
            byte,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%' | b'#'
        )
}

fn write_name(name: &[u8], out: &mut Vec<u8>) {
    out.push(b'/');
    for &byte in name {
        if is_regular(byte) {
            out.push(byte);
        } else {
            out.push(b'#');
            out.extend_from_slice(format!("{byte:02X}").as_bytes());
        }
    }
}

fn write_literal(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'(');
    for &byte in bytes {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            0x20..=0x7e => out.push(byte),
            _ => {
                out.push(b'\\');
                out.extend_from_slice(format!("{byte:03o}").as_bytes());
            }
        }
    }
    out.push(b')');
}

fn write_hex(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'<');
    for &byte in bytes {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.push(b'>');
}

/// Format a real in plain decimal notation with no exponent.
///
/// Rust's `Display` for `f64` prints the shortest text that parses back to the
/// same value and never uses exponential notation, which is exactly what PDF
/// needs; the only adjustment is appending `.0` so the result stays a real and
/// not an integer. Non-finite values have no PDF representation and are written
/// as `0`.
fn real_to_string(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let mut text = format!("{value}");
    if !text.contains('.') {
        text.push_str(".0");
    }
    text
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Dictionary, Object, ObjectId, PdfString, Stream};

    fn dict(entries: &[(&str, Object)]) -> Dictionary {
        let mut dict = Dictionary::new();
        for (name, value) in entries {
            dict.insert(name.as_bytes(), value.clone());
        }
        dict
    }

    #[test]
    fn scalars_serialize_canonically() {
        assert_eq!(Object::Null.to_bytes(), b"null");
        assert_eq!(Object::Boolean(true).to_bytes(), b"true");
        assert_eq!(Object::Boolean(false).to_bytes(), b"false");
        assert_eq!(Object::Integer(-17).to_bytes(), b"-17");
        assert_eq!(Object::Real(2.375).to_bytes(), b"2.375");
        assert_eq!(Object::Real(-0.5).to_bytes(), b"-0.5");
    }

    #[test]
    fn reals_always_keep_a_decimal_point() {
        assert_eq!(Object::Real(1.0).to_bytes(), b"1.0");
        assert_eq!(Object::Real(123.0).to_bytes(), b"123.0");
        assert_eq!(Object::Real(-2.0).to_bytes(), b"-2.0");
        assert_eq!(Object::Real(1e-7).to_bytes(), b"0.0000001");
        assert_eq!(Object::Real(f64::NAN).to_bytes(), b"0");
    }

    #[test]
    fn names_escape_irregular_bytes() {
        assert_eq!(Object::name("Type").to_bytes(), b"/Type");
        assert_eq!(Object::name("A Name").to_bytes(), b"/A#20Name");
        assert_eq!(Object::name("#hash").to_bytes(), b"/#23hash");
        assert_eq!(
            Object::name("slash/percent%").to_bytes(),
            b"/slash#2Fpercent#25"
        );
        assert_eq!(Object::name("Name\u{e9}").to_bytes(), b"/Name#C3#A9");
        assert_eq!(Object::name("").to_bytes(), b"/");
    }

    #[test]
    fn literal_strings_escape_delimiters_and_binary() {
        assert_eq!(Object::string("Hello").to_bytes(), b"(Hello)");
        assert_eq!(Object::string("").to_bytes(), b"()");
        assert_eq!(Object::string("a(b)c").to_bytes(), b"(a\\(b\\)c)");
        assert_eq!(Object::string("back\\slash").to_bytes(), b"(back\\\\slash)");
        assert_eq!(Object::string("line\nbreak").to_bytes(), b"(line\\nbreak)");
        assert_eq!(Object::string("tab\there").to_bytes(), b"(tab\\there)");
        assert_eq!(
            Object::string(vec![0x00, 0x07, 0xff]).to_bytes(),
            b"(\\000\\007\\377)"
        );
    }

    #[test]
    fn hex_strings_are_uppercase() {
        assert_eq!(Object::hex_string("Hello").to_bytes(), b"<48656C6C6F>");
        assert_eq!(Object::hex_string(Vec::new()).to_bytes(), b"<>");
        assert_eq!(Object::hex_string(vec![0xde, 0xad]).to_bytes(), b"<DEAD>");
    }

    #[test]
    fn string_equality_ignores_the_serialization_format() {
        assert_eq!(PdfString::literal("Hi"), PdfString::hex("Hi"));
        assert_ne!(PdfString::literal("Hi"), PdfString::literal("ho"));
    }

    #[test]
    fn containers_separate_elements_with_one_space() {
        assert_eq!(Object::array(Vec::new()).to_bytes(), b"[]");
        assert_eq!(
            Object::array([Object::Integer(1), Object::Integer(2), Object::Integer(3)]).to_bytes(),
            b"[1 2 3]"
        );
        assert_eq!(
            Object::array([
                Object::array([Object::Integer(1)]),
                Object::array([Object::Integer(2), Object::Integer(3)])
            ])
            .to_bytes(),
            b"[[1] [2 3]]"
        );
        assert_eq!(
            Object::Dictionary(dict(&[("Type", Object::name("Page"))])).to_bytes(),
            b"<< /Type /Page >>"
        );
        assert_eq!(Object::Dictionary(Dictionary::new()).to_bytes(), b"<<>>");
        assert_eq!(
            Object::Dictionary(dict(&[
                ("Type", Object::name("Page")),
                ("Count", Object::Integer(3)),
                (
                    "Kids",
                    Object::array([Object::reference(ObjectId::new(1, 0))])
                ),
            ]))
            .to_bytes(),
            b"<< /Type /Page /Count 3 /Kids [1 0 R] >>"
        );
    }

    #[test]
    fn nested_dictionaries_serialize_in_insertion_order() {
        let inner = dict(&[("Inner", Object::Null)]);
        let outer = dict(&[
            ("Zed", Object::Integer(1)),
            ("Outer", Object::Dictionary(inner)),
        ]);
        assert_eq!(
            Object::Dictionary(outer).to_bytes(),
            b"<< /Zed 1 /Outer << /Inner null >> >>"
        );
    }

    #[test]
    fn dictionary_insert_replaces_in_place() {
        let mut dict = dict(&[("A", Object::Integer(1)), ("B", Object::Integer(2))]);
        assert_eq!(
            dict.insert("A", Object::Integer(9)),
            Some(Object::Integer(1))
        );
        assert_eq!(dict.len(), 2);
        assert_eq!(Object::Dictionary(dict).to_bytes(), b"<< /A 9 /B 2 >>");
    }

    #[test]
    fn streams_serialize_the_dictionary_then_the_raw_data() {
        let mut dict = Dictionary::new();
        dict.insert("Length", Object::Integer(5));
        dict.insert("Filter", Object::name("FlateDecode"));
        let stream = Stream::new(dict, b"hello");
        assert_eq!(
            Object::Stream(stream).to_bytes(),
            b"<< /Length 5 /Filter /FlateDecode >>\nstream\nhello\nendstream"
        );
    }

    #[test]
    fn set_data_keeps_length_in_step() {
        let mut dict = Dictionary::new();
        dict.insert("Length", Object::Integer(2));
        let mut stream = Stream::new(dict, b"ab");
        stream.set_data(b"abcdef");
        assert_eq!(stream.dict().get(b"Length"), Some(&Object::Integer(6)));
        assert_eq!(stream.data(), b"abcdef");
        assert_eq!(
            Object::Stream(stream).to_bytes(),
            b"<< /Length 6 >>\nstream\nabcdef\nendstream"
        );
    }

    #[test]
    fn references_serialize_as_three_tokens() {
        assert_eq!(
            Object::Reference(ObjectId::new(12, 0)).to_bytes(),
            b"12 0 R"
        );
        assert_eq!(ObjectId::new(7, 65535).generation, 65535);
    }
}
