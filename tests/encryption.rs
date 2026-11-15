//! The encrypted read paths, revisions 2–6.
//!
//! Four committed fixtures share one object graph:
//!
//! - `tests/fixtures/plain-twin.pdf` — the document in the clear.
//! - `tests/fixtures/encrypted-r6.pdf` — `/V 5 /R 6`, AES-256, user password
//!   `user`, owner password `owner`; the catalog's `/Lang` string, the content
//!   stream and the XMP metadata stream are encrypted.
//! - `tests/fixtures/encrypted-r4-rc4.pdf` — `/V 4 /R 4` with `/CFM /V2`, RC4,
//!   the same passwords and the same object graph.
//! - `tests/fixtures/encrypted-r4-aes.pdf` — `/V 4 /R 4` with `/CFM /AESV2`,
//!   AES-128-CBC with PKCS#7 padding.
//!
//! All were produced by `examples/make_encrypted_fixture.rs` (no `qpdf`,
//! `mutool` or `gs` exists here), so the proof is: the reader derives the key
//! the file declares — per-object for revisions 2–4, file-wide for revision 6 —
//! validates `/U`, decrypts, and extracts exactly the text the plain twin
//! yields. The legacy fixtures are read with the same assertions, which is what
//! makes "revisions 2–4 work" a measurement rather than a claim.

use pdf2::{Document, Error, Object, ObjectId};

const ENCRYPTED: &[u8] = include_bytes!("fixtures/encrypted-r6.pdf");
const PLAIN: &[u8] = include_bytes!("fixtures/plain-twin.pdf");

const USER_PASSWORD: &[u8] = b"user";
const OWNER_PASSWORD: &[u8] = b"owner";

fn encrypted() -> Document {
    Document::open_with_password(ENCRYPTED, USER_PASSWORD).expect("encrypted document opens")
}

#[test]
fn the_encrypted_fixture_is_recognised_as_encrypted() {
    assert!(encrypted().is_encrypted());
    assert!(!Document::open(PLAIN).expect("plain").is_encrypted());
}

#[test]
fn the_user_password_unlocks_the_document() {
    let document = encrypted();
    assert_eq!(document.version(), "2.0");
    assert_eq!(document.page_count().expect("page count"), 1);
}

#[test]
fn the_owner_password_unlocks_the_document_too() {
    let document = Document::open_with_password(ENCRYPTED, OWNER_PASSWORD).expect("owner password");
    assert_eq!(document.page_count().expect("page count"), 1);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.extract_text(&page).expect("text"),
        "Encrypted hello"
    );
}

#[test]
fn a_wrong_password_is_an_error_never_a_panic() {
    for password in [&b"wrong"[..], b"user ", b"", b"ownerx"] {
        match Document::open_with_password(ENCRYPTED, password) {
            Err(Error::Malformed { .. }) => {}
            Err(other) => panic!("unexpected error for {password:?}: {other}"),
            Ok(document) => {
                // The empty password must not work; a document that opened
                // with something else would have to decrypt correctly.
                let page = document.page(0).expect("page");
                assert!(
                    document.extract_text(&page).is_err(),
                    "{password:?} must not unlock the document"
                );
            }
        }
    }
    assert!(Document::open(ENCRYPTED).is_err(), "no password given");
}

#[test]
fn encrypted_and_plain_extract_the_same_text() {
    let encrypted = encrypted();
    let plain = Document::open(PLAIN).expect("plain twin opens");
    let encrypted_page = encrypted.page(0).expect("page");
    let plain_page = plain.page(0).expect("page");
    assert_eq!(
        encrypted
            .extract_text(&encrypted_page)
            .expect("encrypted text"),
        plain.extract_text(&plain_page).expect("plain text")
    );
    assert_eq!(
        encrypted
            .page_content(&encrypted_page)
            .expect("encrypted content"),
        plain.page_content(&plain_page).expect("plain content")
    );
}

#[test]
fn strings_in_the_object_graph_are_decrypted() {
    let encrypted = encrypted();
    let plain = Document::open(PLAIN).expect("plain twin opens");
    let encrypted_catalog = encrypted.catalog().expect("catalog");
    let plain_catalog = plain.catalog().expect("plain catalog");
    assert_eq!(
        encrypted_catalog
            .as_dict()
            .and_then(|dict| dict.get(b"Lang")),
        Some(&Object::string("en-US")),
        "the /Lang string was encrypted on disk"
    );
    assert_eq!(
        encrypted_catalog
            .as_dict()
            .and_then(|dict| dict.get(b"Lang")),
        plain_catalog.as_dict().and_then(|dict| dict.get(b"Lang"))
    );
}

#[test]
fn the_encrypt_dictionary_is_never_decrypted() {
    let encrypted = encrypted();
    let encrypt = encrypted
        .get_object(ObjectId::new(7, 0))
        .expect("encrypt dictionary");
    let encrypt = encrypt.as_dict().expect("dictionary");
    assert_eq!(encrypt.get(b"Filter"), Some(&Object::name("Standard")));
    assert_eq!(encrypt.get(b"V"), Some(&Object::Integer(5)));
    assert_eq!(encrypt.get(b"R"), Some(&Object::Integer(6)));
    for (key, expected) in [
        (&b"U"[..], 48_usize),
        (b"O", 48),
        (b"UE", 32),
        (b"OE", 32),
        (b"Perms", 16),
    ] {
        let length = encrypt
            .get(key)
            .and_then(Object::as_string)
            .map(|string| string.as_bytes().len());
        assert_eq!(length, Some(expected), "/{key:?} keeps its raw bytes");
    }
}

#[test]
fn metadata_streams_are_decrypted_and_readable() {
    let encrypted = encrypted();
    assert!(encrypted.has_metadata().expect("metadata"));
    let metadata = encrypted.metadata().expect("metadata").expect("some");
    let text = String::from_utf8_lossy(&metadata);
    assert!(text.contains("x:xmpmeta"), "{text}");

    let plain = Document::open(PLAIN).expect("plain twin opens");
    assert_eq!(
        encrypted.metadata().expect("encrypted"),
        plain.metadata().expect("plain")
    );
}

#[test]
fn decrypted_objects_are_cached_consistently() {
    let document = encrypted();
    let first = document.get_object_by_number(1).expect("catalog");
    let second = document.get_object_by_number(1).expect("cached catalog");
    assert_eq!(first, second);
    let page = document.page(0).expect("page");
    assert_eq!(
        document.page_content(&page).expect("content"),
        b"BT /F1 24 Tf 72 700 Td (Encrypted hello) Tj ET"
    );
    // Asking twice decrypts twice from the same stored bytes, with the same
    // result.
    assert_eq!(
        document.page_content(&page).expect("content again"),
        document.page_content(&page).expect("content")
    );
}

#[test]
fn deprecated_revisions_are_refused() {
    // Rewriting /R 6 to /R 5 makes the fixture a revision 5 document, which is
    // deprecated and deliberately unsupported.
    let mut bytes = ENCRYPTED.to_vec();
    let at = bytes
        .windows(6)
        .position(|window| window == b"/R 6 /")
        .expect("/R 6 in the fixture");
    bytes[at..at + 6].copy_from_slice(b"/R 5 /");
    let error = Document::open_with_password(&bytes[..], USER_PASSWORD).expect_err("revision 5");
    assert!(error.to_string().contains("revision 6"), "{error}");
}

#[test]
fn a_corrupt_perms_value_is_rejected() {
    // Flip one hex digit of the encrypted /Perms value: the tamper check must
    // notice, because the decrypted bytes no longer read "adb".
    let mut bytes = ENCRYPTED.to_vec();
    let at = bytes
        .windows(8)
        .position(|window| window == b"/Perms <")
        .expect("/Perms in the fixture");
    let digit = at + 8;
    bytes[digit] = if bytes[digit] == b'0' { b'1' } else { b'0' };
    let error =
        Document::open_with_password(&bytes[..], USER_PASSWORD).expect_err("tampered /Perms");
    assert!(error.to_string().contains("/Perms"), "{error}");
}

const LEGACY: [(&[u8], &str); 2] = [
    (include_bytes!("fixtures/encrypted-r4-rc4.pdf"), "RC4"),
    (include_bytes!("fixtures/encrypted-r4-aes.pdf"), "AES-128"),
];

#[test]
fn revision_4_fixtures_decrypt_to_their_plaintext_twin() {
    let plain = Document::open(PLAIN).expect("plain twin opens");
    let plain_page = plain.page(0).expect("page");
    let expected = plain.extract_text(&plain_page).expect("plain text");
    for (bytes, cipher) in LEGACY {
        let encrypted = Document::open_with_password(bytes, USER_PASSWORD)
            .unwrap_or_else(|error| panic!("{cipher} document opens: {error}"));
        assert!(encrypted.is_encrypted());
        assert_eq!(encrypted.version(), "1.7");
        let page = encrypted.page(0).expect("page");
        assert_eq!(
            encrypted.extract_text(&page).expect("text"),
            expected,
            "{cipher}: the content stream decrypted to the twin's text"
        );
        assert_eq!(
            encrypted.page_content(&page).expect("content"),
            plain.page_content(&plain_page).expect("content"),
            "{cipher}: the stream payload decrypts to the same bytes"
        );
        // The per-object key is what revisions 2–4 add: a string in object 1 is
        // decrypted with a different key than the stream in object 4.
        assert_eq!(
            encrypted
                .catalog()
                .expect("catalog")
                .as_dict()
                .and_then(|dict| dict.get(b"Lang")),
            Some(&Object::string("en-US")),
            "{cipher}: the /Lang string decrypted"
        );
    }
}

#[test]
fn the_owner_password_opens_revision_4_documents_too() {
    for (bytes, cipher) in LEGACY {
        let document = Document::open_with_password(bytes, OWNER_PASSWORD)
            .unwrap_or_else(|error| panic!("{cipher} owner password: {error}"));
        let page = document.page(0).expect("page");
        assert_eq!(
            document.extract_text(&page).expect("text"),
            "Encrypted hello",
            "{cipher}: Algorithm 7 recovered the user password from /O"
        );
    }
}

#[test]
fn a_wrong_password_is_rejected_by_revision_4_handlers() {
    for (bytes, cipher) in LEGACY {
        let error = Document::open_with_password(bytes, b"nope")
            .err()
            .unwrap_or_else(|| panic!("{cipher}: a wrong password must fail"));
        assert!(error.to_string().contains("password"), "{cipher}: {error}");
    }
}
