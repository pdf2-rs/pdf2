//! Serialization half of the `Object` round-trip (Q1).
//!
//! `corpus()` pairs each canonical text form with the object it must parse to.
//! Both directions are pinned here: the writer reproduces every entry
//! byte-for-byte, and the parser turns those same bytes back into the object.

use pdf2::{
    parse_indirect_object, parse_object, Dictionary, IndirectObject, Object, ObjectId, PdfString,
};

/// The inline corpus shared by the write and parse halves of the round-trip.
///
/// Every entry is the exact text [`Object::to_bytes`] must produce.
fn corpus() -> Vec<(&'static str, Object)> {
    let mut page = Dictionary::new();
    page.insert(b"Type", Object::name("Page"));
    page.insert(b"Count", Object::Integer(3));
    page.insert(
        b"Kids",
        Object::array([Object::reference(ObjectId::new(1, 0))]),
    );

    let mut nested = Dictionary::new();
    nested.insert(
        b"Outer",
        Object::Dictionary({
            let mut inner = Dictionary::new();
            inner.insert(b"Inner", Object::Null);
            inner
        }),
    );

    let mut literal_page = Dictionary::new();
    literal_page.insert(b"Type", Object::name("Page"));

    vec![
        ("null", Object::Null),
        ("true", Object::Boolean(true)),
        ("false", Object::Boolean(false)),
        ("0", Object::Integer(0)),
        ("-17", Object::Integer(-17)),
        ("9223372036854775807", Object::Integer(i64::MAX)),
        ("2.375", Object::Real(2.375)),
        ("-0.5", Object::Real(-0.5)),
        ("1.0", Object::Real(1.0)),
        ("0.0000001", Object::Real(1e-7)),
        ("(Hello, World!)", Object::string("Hello, World!")),
        ("()", Object::string("")),
        ("(a\\(b\\)c)", Object::string("a(b)c")),
        ("(back\\\\slash)", Object::string("back\\slash")),
        ("(line\\nbreak)", Object::string("line\nbreak")),
        ("(caf\\303\\251)", Object::string("caf\u{e9}")),
        ("(\\000\\007\\377)", Object::string(vec![0x00, 0x07, 0xff])),
        ("<>", Object::hex_string(Vec::new())),
        ("<48656C6C6F>", Object::hex_string("Hello")),
        ("/", Object::name("")),
        ("/Type", Object::name("Type")),
        ("/A#20Name", Object::name("A Name")),
        ("/#23hash", Object::name("#hash")),
        ("/Name#C3#A9", Object::name("Name\u{e9}")),
        ("[]", Object::array(Vec::new())),
        (
            "[1 2 3]",
            Object::array([Object::Integer(1), Object::Integer(2), Object::Integer(3)]),
        ),
        (
            "[[1] [2 3]]",
            Object::array([
                Object::array([Object::Integer(1)]),
                Object::array([Object::Integer(2), Object::Integer(3)]),
            ]),
        ),
        (
            "[null true -17 2.375 (s) /N]",
            Object::array([
                Object::Null,
                Object::Boolean(true),
                Object::Integer(-17),
                Object::Real(2.375),
                Object::string("s"),
                Object::name("N"),
            ]),
        ),
        ("<<>>", Object::Dictionary(Dictionary::new())),
        ("<< /Type /Page >>", Object::Dictionary(literal_page)),
        (
            "<< /Type /Page /Count 3 /Kids [1 0 R] >>",
            Object::Dictionary(page),
        ),
        ("<< /Outer << /Inner null >> >>", Object::Dictionary(nested)),
        ("12 0 R", Object::reference(ObjectId::new(12, 0))),
    ]
}

#[test]
fn corpus_serializes_byte_identically() {
    for (text, object) in corpus() {
        assert_eq!(
            object.to_bytes(),
            text.as_bytes(),
            "serializing the object that must parse from {text}"
        );
    }
}

#[test]
fn corpus_is_plain_ascii_so_it_can_live_in_fixtures() {
    for (text, _) in corpus() {
        assert!(
            text.is_ascii(),
            "corpus entry {text:?} must be ASCII to stay editable in a .pdf fixture"
        );
        assert!(!text.contains('\n'), "corpus entry {text:?} spans lines");
    }
}

#[test]
fn every_corpus_entry_is_distinct() {
    let entries = corpus();
    for (index, (text, _)) in entries.iter().enumerate() {
        for (other, _) in &entries[index + 1..] {
            assert_ne!(text, other, "duplicate corpus entry {text}");
        }
    }
}

#[test]
fn hex_and_literal_spellings_are_the_same_string_object() {
    let literal = Object::string("Hello");
    let hex = Object::hex_string("Hello");
    assert_eq!(literal, hex);
    assert_ne!(literal.to_bytes(), hex.to_bytes());
}

#[test]
fn a_parsed_string_can_have_its_bytes_replaced() {
    let mut string = PdfString::hex("cipher");
    string.set_bytes("plain");
    assert_eq!(
        Object::String(string).to_bytes(),
        b"<706C61696E>",
        "replacement keeps the hex syntax chosen at parse time"
    );
}

#[test]
fn corpus_round_trips_through_parse_and_serialize() {
    for (text, object) in corpus() {
        let parsed = parse_object(text.as_bytes())
            .unwrap_or_else(|error| panic!("parsing {text:?}: {error}"));
        assert_eq!(parsed, object, "parsing {text:?}");
        assert_eq!(
            parsed.to_bytes(),
            text.as_bytes(),
            "re-serializing {text:?} must reproduce the corpus bytes"
        );
    }
}

#[test]
fn malformed_objects_are_errors() {
    let cases = [
        "",
        "(",
        "[1 2",
        "<< /A >>",
        "<< 1 2 >>",
        "]",
        ">>",
        "/Bad#zz",
        "1.2.3",
        "obj",
        "12 R",
        "9223372036854775808",
        "4294967296 0 R",
        "1 65536 R",
        "1 2",
    ];
    for case in cases {
        assert!(
            parse_object(case.as_bytes()).is_err(),
            "{case:?} must not parse"
        );
    }
}

#[test]
fn truncated_corpus_never_panics() {
    for (text, _) in corpus() {
        for end in 0..text.len() {
            let _ = parse_object(&text.as_bytes()[..end]);
        }
    }
}

fn indirect(input: &str) -> IndirectObject {
    parse_indirect_object(input.as_bytes()).unwrap_or_else(|error| panic!("{input:?}: {error}"))
}

#[test]
fn indirect_objects_round_trip_through_the_parser() {
    let object = indirect("7 0 obj\n<< /Type /Page /Parent 2 0 R >>\nendobj\n");
    assert_eq!(object.id, ObjectId::new(7, 0));
    let dict = object.object.as_dict().expect("dictionary");
    assert_eq!(dict.get(b"Type"), Some(&Object::name("Page")));
    assert_eq!(
        dict.get(b"Parent"),
        Some(&Object::reference(ObjectId::new(2, 0)))
    );
    assert_eq!(
        object.object.to_bytes(),
        b"<< /Type /Page /Parent 2 0 R >>",
        "the value serializes back to the bytes it was parsed from"
    );
}

#[test]
fn a_stream_keeps_its_payload_and_serializes_back() {
    let object = indirect("8 0 obj\n<< /Length 12 >>\nstream\nBT /F1 12 Tf\nendstream\nendobj\n");
    let stream = object.object.as_stream().expect("stream");
    assert_eq!(stream.data(), b"BT /F1 12 Tf");
    assert_eq!(
        object.object.to_bytes(),
        b"<< /Length 12 >>\nstream\nBT /F1 12 Tf\nendstream"
    );
}

#[test]
fn an_indirect_stream_length_falls_back_to_the_endstream_marker() {
    // Without a document the reference cannot be followed, so the payload is
    // delimited by `endstream` (tests/filters.rs proves the resolved path).
    let object = indirect("1 0 obj\n<< /Length 9 0 R >>\nstream\nhello\nendstream\nendobj\n");
    assert_eq!(object.object.as_stream().expect("stream").data(), b"hello");
}

#[test]
fn stream_length_and_endstream_must_agree() {
    let cases = [
        // length one byte short of the payload
        "1 0 obj\n<< /Length 4 >>\nstream\nhello\nendstream\nendobj\n",
        // length one byte past the payload and past the input
        "1 0 obj\n<< /Length 900 >>\nstream\nhello\nendstream\nendobj\n",
        // no /Length at all
        "1 0 obj\n<< /Type /XObject >>\nstream\nhello\nendstream\nendobj\n",
        // /Length that is not a number
        "1 0 obj\n<< /Length /Five >>\nstream\nhello\nendstream\nendobj\n",
    ];
    for case in cases {
        assert!(
            parse_indirect_object(case.as_bytes()).is_err(),
            "{case:?} must not parse"
        );
    }
}
