# PDF 2.0 Rust library

[![CI](https://github.com/pdf2-rs/pdf2/actions/workflows/ci.yml/badge.svg)](https://github.com/pdf2-rs/pdf2/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/pdf2.svg)](https://crates.io/crates/pdf2)
[![docs.rs](https://docs.rs/pdf2/badge.svg)](https://docs.rs/pdf2)

A reader and writer for PDF 2.0 — the file format defined by ISO 32000-2. No
`unsafe`, no C dependencies, and the format is implemented in this crate rather
than wrapped: the four dependencies (`miniz_oxide`, `aes`, `cbc`, `sha2`) only
supply DEFLATE and AES/SHA primitives, and everything else is `core` + `alloc`,
so the library also builds without a standard library.

## Status

The gate is `cargo test --workspace`, `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -- -D warnings`; all three pass on every
commit.

Working today:

- Classic cross-reference tables, `/Prev` chains and incremental updates.
- Cross-reference streams (`/Type /XRef` with `/W` and `/Index`), object
  streams (`/ObjStm`) and hybrid files (`/XRefStm`).
- Filters: FlateDecode (with PNG 10–15 and TIFF 2 predictors), LZWDecode,
  ASCIIHexDecode, ASCII85Decode and RunLengthDecode, including `/Filter` and
  `/DecodeParms` arrays and indirect `/Length`.
- Every base object type, parsed and re-serialized byte-for-byte.
- The document model: catalog, page tree with inherited `/MediaBox`,
  `/CropBox`, `/Rotate` and `/Resources`, page content streams, and XMP
  `/Metadata`.
- Content-stream interpretation and text extraction for simple fonts:
  `BT`/`ET`, `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tj`, `TJ`, `'`, `"`, with
  `StandardEncoding`, `WinAnsiEncoding` and `/Differences` arrays. Line breaks
  come from the positioning operators, spaces from the text itself or from wide
  `TJ` adjustments; inline images are skipped.
- Lazy object fetching with a cache, and every malformed input reported as an
  `Err` instead of a panic — `tests/robustness.rs` opens every fixture from
  every truncation and from sampled mutations and exercises the whole read API
  on whatever still opens.
- Writing PDF 2.0 files: `Document::new()` builds a document, `add_page` appends
  pages, `save()` emits a `%PDF-2.0` header, a Flate-compressed `/ObjStm` for
  non-stream objects, a compressed `/Type /XRef` cross-reference stream with
  recorded byte offsets, XMP `/Metadata` and no `trailer`/`Info` dictionaries.
  Files written by this crate are read back by it — the writer has no other
  proof, and `tests/writer.rs` compares the whole object graph.
- AES-256 encryption, read side: `/V 5 /R 6` with the standard security
  handler. User and owner passwords are both accepted; strings, streams, object
  streams and XMP metadata are decrypted, `/EncryptMetadata` is honoured, and
  the `/Perms` tamper check is verified. Deprecated revisions (RC4, AES-128,
  `/R 5`) are refused with an error rather than half-implemented.

## Limits

Deliberately out of scope: rendering, PDF/A conformance, RC4 and AES-128, the
`Info` dictionary, form filling, JavaScript, XFA and image codecs (DCT, JPX,
CCITT, JBIG2). A file that needs one of these is reported as an error rather
than silently misread.

Known limits inside the supported surface:

- Text extraction: composite (Type0) fonts and `/ToUnicode` CMaps are reported
  as unsupported; the upper half of `MacRomanEncoding` is not decoded; glyph
  names outside the built-in table are skipped rather than guessed. Spaces are
  recognised from the text itself and from wide `TJ` adjustments, not from glyph
  advance widths.
- Encryption is read-only and revision 6 only; the writer always produces
  unencrypted files.
- `/Count` in a page tree is treated as a hint and never trusted; pages are
  counted by walking.
- No external validator (`qpdf`, `verapdf`, `mutool`, `gs`) exists in the
  environment this was built in. Proof rests on committed fixtures, byte-exact
  round-trips and cross-checks against independent implementations (NIST
  SP 800-38A for AES-256-CBC, `base64.a85encode` for ASCII85, a separate
  encoder for LZW, Python for the PNG predictors).

## Features

| Feature | What it enables |
| --- | --- |
| `std` (default) | everything: the file-system helpers `Document::open_path`, `open_path_with_password` and `save_to_path`, and the `std::io::Error` conversion |
| `alloc` | the `no_std` build: `core` + `alloc`, no file system, no standard library |

```sh
cargo build                                  # std, the normal build
cargo build --no-default-features --features alloc
cargo build --no-default-features --features alloc --target thumbv7em-none-eabihf
```

Without `std`, `Document::open(&[u8])` and the in-memory writer are the entry
points; everything the parser, filters, document model, text extraction and
AES-256 decryption need lives in `core` and `alloc`. CI builds both targets for
`x86_64-unknown-linux-gnu` and `thumbv7em-none-eabihf`.

WebAssembly is tested the same way: `crates/pdf2-wasm-smoke` builds a browser
module (`wasm32-unknown-unknown`) driven from Node through a small C ABI, and a
WASI binary (`wasm32-wasip1`) that reads a fixture through `std::fs`; CI runs
both and fails if their gzip size passes the budget in
[docs/PERFORMANCE.md](docs/PERFORMANCE.md).

## Rendering

`crates/pdf2-render` rasterises a page into RGBA8 with the library's own
tokenizer: paths, fills (non-zero and even-odd), strokes with caps/joins and
dashes, clipping through `q`/`Q`, the `cm` transform, text through simple-font
encodings (embedded TrueType via `ttf-parser`, a built-in base-14 fallback
otherwise) and `/Image` XObjects with nearest or bilinear sampling. Golden
images pin the output:

```sh
cargo run -p pdf2-render --bin pdf2-render-golden              # verify
cargo run -p pdf2-render --bin pdf2-render-golden -- --bless   # regenerate
```

## Layout

The repository is a Cargo workspace: `pdf2` is the root package (the library in
`src/`, its tests and fixtures in `tests/`), and the tools live under `crates/`.
`crates/pdf2-cli` is the first member; it installs a `pdf2` binary.

| Module | Responsibility |
| --- | --- |
| `src/error.rs` | one `Error` enum, no panics in the read paths |
| `src/lexer.rs` | tokens: whitespace, comments, numbers, names, strings |
| `src/object.rs` | the object model and its serialization |
| `src/parser.rs` | objects and indirect objects, streams by `/Length` |
| `src/filter.rs` | stream filters and predictors |
| `src/document.rs` | cross-reference tables and streams, object fetching |
| `src/page.rs` | page tree, boxes, XMP metadata |
| `src/content.rs` | content streams and text extraction |
| `src/crypto.rs` | AES-256 revision 6 security handler |
| `src/writer.rs` | `Document::new()` … `save()` |
| `crates/pdf2-cli/src/main.rs` | the `pdf2` command: `info`, argument parsing, exit codes |
| `crates/pdf2-render/src/` | the rasteriser: canvas, geometry, paths, scanline fill, content walker, PNG |
| `crates/pdf2-bench/src/` | the benchmark binary and baseline comparison |

## Command line

The workspace also ships a CLI, installed from
[`pdf2-cli`](crates/pdf2-cli/README.md):

```sh
cargo install pdf2-cli
pdf2 info file.pdf          # version, pages, encryption, metadata
pdf2 pages file.pdf         # object id, boxes, rotation, content ids per page
pdf2 text file.pdf --all    # extracted text
pdf2 check file.pdf         # decode and extract every page, report failures
```

Every command takes `--password <pw>` for encrypted files and `--json` for
machine-readable output; exit codes are 0 (ok), 1 (bad command line) and 2
(unreadable file).

## Usage

```rust,no_run
use pdf2::{Document, ObjectId};

fn main() -> Result<(), pdf2::Error> {
    let document = Document::open(std::fs::read("file.pdf")?)?;
    println!("PDF {}", document.version());

    for page in document.pages()? {
        println!("media box: {:?}, rotate: {}", page.media_box, page.rotate);
        let content = document.page_content(&page)?;
        println!("{} content bytes", content.len());
    }

    if let Some(stream) = document.get_object(ObjectId::new(4, 0))?.as_stream() {
        let decoded = document.decode_stream(stream)?;
        println!("{} decoded bytes", decoded.len());
    }

    if let Some(xmp) = document.metadata()? {
        println!("{} bytes of XMP", xmp.len());
    }

    Ok(())
}
```

An encrypted file takes its password:

```rust,no_run
use pdf2::Document;

fn main() -> Result<(), pdf2::Error> {
    let bytes = std::fs::read("secret.pdf")?;
    let document = Document::open_with_password(bytes, b"user")?;
    println!("{} page(s)", document.page_count()?);
    Ok(())
}
```

Writing looks like this (also available as `cargo run --example write`):

```rust,no_run
use pdf2::{Document, Rect};

fn main() -> Result<(), pdf2::Error> {
    let mut document = Document::new();
    document.add_page(
        Rect::new(0.0, 0.0, 612.0, 792.0),
        "BT /F1 24 Tf 72 700 Td (Hello from pdf2) Tj ET",
    );
    let bytes = document.save()?;
    std::fs::write("hello.pdf", bytes)?;
    Ok(())
}
```

## Tests and fixtures

`cargo test --workspace` runs unit tests next to each module, integration tests
in `tests/*.rs`, doc-tests that compile the examples above, and the `pdf2-cli`
binary tests, which execute the built binary against the committed fixtures.

The fixtures in `tests/fixtures/` are small, committed PDFs, each named after
the structure it proves — see `tests/fixtures/README.md`. Six are generated by
`tests/fixtures/make_fixtures.py` (offsets and zlib streams computed there), and
the encryption pair by `cargo run --example make_encrypted_fixture`, because no
external PDF tool is available to produce them. Both generators are deterministic:
re-running them reproduces every byte.

Benchmarks and the memory numbers live in `crates/pdf2-bench`, gated in CI
against [`docs/performance-baseline.json`](docs/performance-baseline.json) —
method, reference numbers and budgets are in
[docs/PERFORMANCE.md](docs/PERFORMANCE.md).

`tests/robustness.rs` is the single corpus that opens every fixture from every
truncation, from sampled byte mutations and from single-byte deletions, and then
exercises pages, metadata, content decoding and text extraction on anything that
still opens.

## CI and releases

Changes are listed in [CHANGELOG.md](CHANGELOG.md).

`.github/workflows/ci.yml` runs the gate in this README's *Status* section on
stable, on beta and on the declared MSRV (1.87), builds the docs with warnings
denied, and packages the crate.

`rust-toolchain.toml` pins stable (with `rustfmt` and `clippy`) for local work.
The MSRV stays checkable with `cargo +1.87 test`, because an explicit
`+toolchain` argument outranks that file — which is also how CI names every
toolchain it uses.

Releases are published from `.github/workflows/publish.yml` on a `v*` tag using
[crates.io trusted publishing](https://crates.io/docs/trusted-publishing/) —
OIDC, short-lived tokens, no secrets. See [docs/RELEASING.md](docs/RELEASING.md)
for the one-time crates.io configuration and the tag-and-push flow.

The way from here to 1.0 — proof-of-use tools, `no_std` and wasm targets, a
rendering crate and a wasm package, with the gates between alpha, beta, rc and
1.0 — is planned in [docs/ROADMAP.md](docs/ROADMAP.md). Upgrading from
`0.2.0-alpha.1` needs nothing beyond the version bump, but the feature flags are
worth knowing: see [docs/MIGRATING.md](docs/MIGRATING.md).

## Compatibility policy

From `0.7.0-rc.1` the public API is frozen; `1.0.0` changes no signatures.

- **Covered by semver**: every public item of `pdf2` (types, functions, trait
  implementations) and the file-format behaviour documented in
  `docs/SPEC-MAP.md` — the features listed under *Status* keep working for the
  documents they claim to support.
- **Not covered**: rendering fidelity beyond the documented scope, benchmark
  numbers, the content of error messages, and anything listed under *Limits* or
  out of scope. Adding a new API item, a new filter, a new font case or a new
  document feature is a minor release; changing or removing one is a major
  release.
- The tools (`pdf2-cli`, `pdf2-render`, `pdf2-wasm`) follow their own semver
  lines, starting at 1.0 alongside the library.

## Legal

Dual-licensed under `MIT` or the [UNLICENSE](http://unlicense.org/).
