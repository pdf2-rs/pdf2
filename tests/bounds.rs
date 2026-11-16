//! The decoded-bytes budget: a stream that would expand past it is refused
//! instead of allocated.
//!
//! Without the cap a 1 MiB file expands to a gigabyte — measured at 0.43 s into
//! 1 073 741 824 bytes — which in a browser tab is a denial of service rather
//! than a rendering failure (`docs/RENDERING-QUALITY.md`). The bomb here is
//! built at test time, never committed: `tests/fixtures` is the benchmark's
//! input set, so a gigabyte-inflating fixture would make `cargo gate --full`
//! inflate a gigabyte on every run.

use pdf2::filter::DEFAULT_DECODE_LIMIT;
use pdf2::{Document, Error};

/// A committed fixture, read as bytes.
fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A one-page PDF whose content stream is `payload`, Flate-encoded.
fn pdf_with_flate_stream(payload: &[u8]) -> Vec<u8> {
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(payload, 1);
    let mut out = b"%PDF-2.0\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let mut object = |number: usize, body: &[u8], out: &mut Vec<u8>| {
        offsets.push(out.len());
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    object(1, b"<< /Type /Catalog /Pages 2 0 R >>", &mut out);
    object(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>", &mut out);
    object(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>",
        &mut out,
    );
    offsets.push(out.len());
    out.extend_from_slice(b"4 0 obj\n");
    out.extend_from_slice(
        format!(
            "<< /Length {} /Filter /FlateDecode >>\nstream\n",
            compressed.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&compressed);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
    );
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn content_of(document: &Document) -> pdf2::Result<Vec<u8>> {
    let page = document.page(0).expect("page");
    document.page_content(&page)
}

#[test]
fn the_default_budget_is_a_quarter_of_a_gigabyte() {
    let document = Document::open(pdf_with_flate_stream(b"BT ET")).expect("open");
    assert_eq!(document.decoded_limit(), 256 << 20);
    assert_eq!(document.decoded_limit(), DEFAULT_DECODE_LIMIT);
}

#[test]
fn a_stream_past_the_budget_is_refused_rather_than_allocated() {
    // 16 MiB of zeros compress to a few kilobytes: the file size says nothing
    // about what decoding it costs.
    let bomb = pdf_with_flate_stream(&vec![0_u8; 16 << 20]);
    assert!(
        bomb.len() < 256 * 1024,
        "the bomb file is small: {}",
        bomb.len()
    );
    let document = Document::open(bomb).expect("open");
    document.set_decoded_limit(1 << 20);
    let started = std::time::Instant::now();
    let error = content_of(&document).expect_err("past the budget");
    assert!(
        matches!(error, Error::Limit { limit, .. } if limit == 1 << 20),
        "{error}"
    );
    assert!(
        error.to_string().contains("1048576"),
        "names the budget: {error}"
    );
    assert!(
        started.elapsed().as_secs_f64() < 5.0,
        "refused in bounded time: {:?}",
        started.elapsed()
    );
}

#[test]
fn the_setter_both_raises_and_lowers() {
    let payload = vec![b'x'; 4 << 20];
    let document = Document::open(pdf_with_flate_stream(&payload)).expect("open");
    document.set_decoded_limit(1 << 20);
    assert!(content_of(&document).is_err(), "1 MiB is not enough");

    document.set_decoded_limit(8 << 20);
    let content = content_of(&document).expect("8 MiB is enough");
    assert_eq!(content.len(), payload.len());

    document.set_decoded_limit(1 << 20);
    assert!(content_of(&document).is_err(), "lowering refuses it again");
}

#[test]
fn the_budget_does_not_disturb_the_committed_fixtures() {
    for fixture in ["text.pdf", "calibration.pdf", "book.pdf"] {
        let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
        let document = Document::open_path(&path).expect("open fixture");
        assert_eq!(document.decoded_limit(), DEFAULT_DECODE_LIMIT);
        for index in 0..document.pages().expect("pages").len() {
            let page = document.page(index).expect("page");
            document.page_content(&page).expect("the fixture decodes");
        }
    }
}

/// A zlib stream that inflates to `total` bytes, built with a 1 MiB working set
/// so the test does not need the gigabyte to exist.
fn bomb_stream(total: usize) -> Vec<u8> {
    use miniz_oxide::deflate::core::{
        compress, create_comp_flags_from_zip_params, CompressorOxide, TDEFLFlush, TDEFLStatus,
    };
    let mut state = CompressorOxide::new(create_comp_flags_from_zip_params(1, 15, 0));
    let chunk = vec![0_u8; 1 << 20];
    let mut buffer = vec![0_u8; 1 << 16];
    let rounds = total / chunk.len();
    let mut out = Vec::new();
    for round in 0..rounds {
        let flush = if round + 1 == rounds {
            TDEFLFlush::Finish
        } else {
            TDEFLFlush::None
        };
        let mut input: &[u8] = &chunk;
        loop {
            let (status, consumed, written) = compress(&mut state, input, &mut buffer, flush);
            out.extend_from_slice(&buffer[..written]);
            input = &input[consumed..];
            if matches!(status, TDEFLStatus::Done) || (consumed == 0 && written == 0) {
                break;
            }
        }
    }
    out
}

#[test]
fn a_stream_past_the_default_budget_is_refused() {
    // The pre-fix measurement (F1) was a 1 043 364-byte file expanding to
    // 1 073 741 824 bytes; the cap is size-independent, so this generates
    // 320 MiB — a quarter of the way in, five times a debug-mode second cheaper
    // per gate run — and the refusal is the same code path. Never committed:
    // the compressed form is small and the expansion stops at the budget.
    let compressed = bomb_stream(320 << 20);
    assert!(
        compressed.len() < 8 * 1024 * 1024,
        "the stream compresses small: {} bytes",
        compressed.len()
    );
    let document = Document::open(b"not used").expect_err("not a document");
    assert!(document.to_string().contains("malformed"), "{document}");

    let started = std::time::Instant::now();
    let error = pdf2::filter::decode_filter(pdf2::filter::Filter::FlateDecode, &compressed, None)
        .expect_err("past the default budget");
    let elapsed = started.elapsed();
    assert!(
        matches!(error, Error::Limit { limit, .. } if limit == DEFAULT_DECODE_LIMIT),
        "{error}"
    );
    assert!(
        error.to_string().contains("268435456"),
        "names the budget: {error}"
    );
    assert!(
        elapsed.as_secs() < 30,
        "refused in bounded time: {elapsed:?}"
    );
}

#[test]
fn the_object_ceiling_refuses_a_document_that_asks_for_too_much() {
    // The counting is of objects *read*, so the fixture is an ordinary document
    // and the budget is what makes it refuse: a crafted cross-reference naming
    // millions costs the parse of every one of them.
    let document = Document::open(read("tests/fixtures/page-tree.pdf")).expect("open");
    document.set_object_limit(2);
    assert_eq!(document.object_limit(), 2);
    let mut seen = 0;
    let mut refused = None;
    for number in 1..=64_u32 {
        match document.get_object_by_number(number) {
            Ok(_) => seen += 1,
            Err(Error::TooMany { what, limit }) => {
                refused = Some((what, limit));
                break;
            }
            Err(_) => {}
        }
    }
    let (what, limit) = refused.expect("a third object is refused");
    assert_eq!((what, limit), ("the number of objects read", 2));
    assert_eq!(seen, 2, "exactly the budget is read");
}

#[test]
fn the_page_ceiling_refuses_a_page_tree_that_lists_too_many() {
    let document = Document::open(read("tests/fixtures/page-tree.pdf")).expect("open");
    let total = document.page_count().expect("count");
    assert!(total > 1, "the fixture has several pages");
    document.set_page_limit(total - 1);
    let error = document.pages().expect_err("one page over the ceiling");
    match error {
        Error::TooMany { what, limit } => {
            assert_eq!(what, "the page tree");
            assert_eq!(limit, total - 1);
        }
        other => panic!("wrong error: {other:?}"),
    }
    // Lifting it lets the same document through: the limit is a budget, not a
    // verdict about the file.
    document.set_page_limit(usize::MAX);
    assert_eq!(document.page_count().expect("count"), total);
}
