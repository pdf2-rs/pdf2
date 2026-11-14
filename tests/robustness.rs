//! One robustness corpus over every committed fixture (Q10, M10).
//!
//! The claim this file defends is narrow and checkable: **no input, however
//! truncated or mutated, makes the crate panic**. Each fixture is opened from
//! every truncation and from a spread of byte mutations; whenever an input does
//! open, the whole public read API is exercised on it, because the interesting
//! failures are the ones that parse far enough to reach the page tree, the
//! filters or the text interpreter.
//!
//! Sweeps are sampled (`mutation_step`) rather than exhaustive where a single
//! open is expensive: the AES-256 fixture runs a 64-round key stretch on every
//! successful open, so a full sweep would dominate the suite. The file written
//! by this crate's own writer is swept the same way, so the corpus covers what
//! `save()` emits as well as the fixtures.

use pdf2::{Document, Rect};

struct Fixture {
    name: &'static str,
    bytes: &'static [u8],
    /// Mutate every `step`-th byte; 1 means every byte.
    mutation_step: usize,
    /// Replacement byte values, applied one at a time.
    replacements: &'static [u8],
    /// Passwords tried when opening.
    passwords: &'static [&'static [u8]],
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "classic-xref.pdf",
        bytes: include_bytes!("fixtures/classic-xref.pdf"),
        mutation_step: 3,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "classic-twin.pdf",
        bytes: include_bytes!("fixtures/classic-twin.pdf"),
        mutation_step: 3,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "xref-stream.pdf",
        bytes: include_bytes!("fixtures/xref-stream.pdf"),
        mutation_step: 3,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "xref-stream-prev.pdf",
        bytes: include_bytes!("fixtures/xref-stream-prev.pdf"),
        mutation_step: 4,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "hybrid-xref.pdf",
        bytes: include_bytes!("fixtures/hybrid-xref.pdf"),
        mutation_step: 4,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "page-tree.pdf",
        bytes: include_bytes!("fixtures/page-tree.pdf"),
        mutation_step: 5,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "text.pdf",
        bytes: include_bytes!("fixtures/text.pdf"),
        mutation_step: 5,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "plain-twin.pdf",
        bytes: include_bytes!("fixtures/plain-twin.pdf"),
        mutation_step: 5,
        replacements: &[0x00, 0xFF, b'X'],
        passwords: &[b""],
    },
    Fixture {
        name: "encrypted-r6.pdf",
        bytes: include_bytes!("fixtures/encrypted-r6.pdf"),
        // One successful open runs the 64-round key stretch, so this sweep is
        // coarse on purpose; `tests/encryption.rs` covers the password paths.
        mutation_step: 97,
        replacements: &[0xFF],
        passwords: &[b"user"],
    },
];

/// The bytes `save()` produces for a small document.
fn written_bytes() -> Vec<u8> {
    let mut document = Document::new();
    document.add_page(
        Rect::new(0.0, 0.0, 612.0, 792.0),
        "BT /F1 24 Tf 72 700 Td (Robustness) Tj ET",
    );
    document.add_page(
        Rect::new(0.0, 0.0, 200.0, 300.0),
        "BT /F1 12 Tf 10 20 Td (two) Tj ET",
    );
    document.save().expect("save")
}

/// Exercise everything a caller can do with a document that did open.
fn exercise(document: &Document) {
    let _ = document.version();
    let _ = document.trailer();
    let _ = document.catalog();
    let _ = document.has_metadata();
    let _ = document.metadata();
    if let Ok(pages) = document.pages() {
        for page in pages.iter().take(4) {
            let _ = document.page_content(page);
            let _ = document.extract_text(page);
        }
    }
    let _ = document.page_count();
    let _ = document.page(0);
    for number in 0..12 {
        if let Ok(object) = document.get_object_by_number(number) {
            if let Some(stream) = object.as_stream() {
                let _ = document.decode_stream(stream);
            }
        }
    }
}

fn truncations(name: &str, bytes: &[u8], passwords: &[&[u8]]) {
    for end in 0..bytes.len() {
        for password in passwords {
            if let Ok(document) = Document::open_with_password(&bytes[..end], password) {
                exercise(&document);
            }
        }
    }
    let _ = name;
}

fn mutations(name: &str, bytes: &[u8], step: usize, replacements: &[u8], passwords: &[&[u8]]) {
    for offset in (0..bytes.len()).step_by(step) {
        for replacement in replacements {
            let mut mutated = bytes.to_vec();
            mutated[offset] = *replacement;
            for password in passwords {
                if let Ok(document) = Document::open_with_password(mutated.clone(), password) {
                    exercise(&document);
                }
            }
        }
    }
    let _ = name;
}

fn deletions(name: &str, bytes: &[u8], step: usize, passwords: &[&[u8]]) {
    for offset in (0..bytes.len()).step_by(step) {
        let mut damaged = bytes.to_vec();
        damaged.remove(offset);
        for password in passwords {
            if let Ok(document) = Document::open_with_password(damaged.clone(), password) {
                exercise(&document);
            }
        }
    }
    let _ = name;
}

#[test]
fn every_fixture_opens_cleanly() {
    for fixture in FIXTURES {
        for password in fixture.passwords {
            Document::open_with_password(fixture.bytes, password)
                .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));
        }
    }
    let written = written_bytes();
    Document::open(written).expect("written file opens");
}

#[test]
fn every_truncation_is_handled() {
    for fixture in FIXTURES {
        truncations(fixture.name, fixture.bytes, fixture.passwords);
    }
    truncations("written.pdf", &written_bytes(), &[b""]);
}

#[test]
fn every_sampled_mutation_is_handled() {
    for fixture in FIXTURES {
        mutations(
            fixture.name,
            fixture.bytes,
            fixture.mutation_step,
            fixture.replacements,
            fixture.passwords,
        );
    }
    mutations(
        "written.pdf",
        &written_bytes(),
        7,
        &[0x00, 0xFF, b'X'],
        &[b""],
    );
}

#[test]
fn deleting_a_byte_is_handled() {
    for fixture in FIXTURES {
        deletions(
            fixture.name,
            fixture.bytes,
            fixture.mutation_step * 3,
            fixture.passwords,
        );
    }
    deletions("written.pdf", &written_bytes(), 23, &[b""]);
}

#[test]
fn an_error_always_has_a_message() {
    // A caller must be able to log what went wrong, so no error may be empty.
    for fixture in FIXTURES {
        for end in (0..fixture.bytes.len()).step_by(7) {
            if let Err(error) = Document::open(&fixture.bytes[..end]) {
                assert!(
                    !error.to_string().is_empty(),
                    "{} truncated at {end} produced a silent error",
                    fixture.name
                );
                assert!(!format!("{error:?}").is_empty());
            }
        }
    }
}
