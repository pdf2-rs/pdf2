# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.2] — 2026-11-14

Text gets the document's own metrics, and fonts that cannot be drawn are said out
loud instead of being approximated:

- **`/Widths` and `/MissingWidth`** are used for advances, so a base-14 font
  spaces its text the way the file asks instead of a uniform 0.6 em.
- **Unsupported font families are reported**: `/Type0` (CID) and `/Type3` are not
  painted at all, and embedded Type 1 (`/FontFile`) and CFF (`/FontFile3`)
  programs are painted with the fallback *and* counted, so a page that could not
  be drawn says so through `Report`.
- `tests/fixtures/calibration.pdf` gained a `/Widths` page and an unsupported
  font page, both pinned by probes and goldens; `docs/RENDERING.md` has the
  support matrix.

## [1.0.1] — 2026-11-14

Renderer bug fixes, found by the new calibration fixture rather than by the
golden images, which had nothing curved in them:

- **Curves.** `c`, `v` and `y` took their end point from the wrong operands, so
  every Bézier was drawn to its last control point; curves are now the curve.
- **`/Decode`** is applied to image samples, including `/Indexed` index ranges.
- **Joins.** Miter, round and bevel are drawn as asked, and the miter limit
  `M` is honoured with the standard cut-back to a bevel.
- **`/Rotate`** turns the page and swaps the canvas dimensions for 90/180/270.

`docs/RENDERING.md` maps every covered clause to the page and assertion that
proves it, and lists what is still unimplemented.

## [1.0.0] — 2026-11-14

The 1.0 release. The public API is frozen: `1.x` changes no signatures and no
documented behaviour.

### Contents

- `pdf2` 1.0.0 — PDF 2.0 reader and writer: classic and stream cross-references,
  object streams, Flate/LZW/ASCII/RunLength filters with predictors, the
  document model with inherited page attributes and XMP metadata, text
  extraction for simple fonts, the writer (`Document::new()` … `save()`), and
  AES-256 `/V 5 /R 6` decryption.
- `pdf2-render` 1.0.0 — paths, fills, strokes, dashes, clips, transforms, text
  and image XObjects, pinned by golden images.
- `pdf2-cli` 1.0.0 — `info`, `pages`, `text`, `check` with `--json`.
- `pdf2-wasm` 1.0.0 — the same library and renderer in the browser, with a
  GitHub Pages demo; also on npm as `pdf2-wasm` (`npm install pdf2-wasm`).

### Compatibility

The API, the fixture corpus and the renderer goldens are unchanged since
`0.7.0-rc.1`. Semver from here covers the public API and the file-format
behaviour documented in `docs/SPEC-MAP.md`; see the README's compatibility
policy for what it does not cover.

## [0.7.0-rc.1] — 2026-11-14

First release candidate. **The public API is frozen from this release**: after
it, only bug fixes and internal changes until 1.0.0.

### Changed

- Version scheme: the library jumps to `0.7.0-rc.1` so that `1.0.0` is the next
  minor; the tools in this repository move to `0.1.0-rc.1` in step
  (`pdf2-render`, `pdf2-cli`, `pdf2-wasm`).

### Compatibility

Everything from `0.2.0-alpha.2` is unchanged: no public item was removed or
renamed, and the fixture corpus plus the renderer goldens are byte-identical.
See `docs/MIGRATING.md` for the alpha.1 → alpha.2 feature notes.

## [0.2.0-alpha.2] — 2026-11-14

The library gained its `no_std` build and a public encoding API; the rendering,
CLI and WebAssembly crates were released on top of it.

### Added

- `pdf2::Encoding`: simple-font code → character mapping (`StandardEncoding`,
  `WinAnsiEncoding`, `/Encoding` names and dictionaries, `/Differences`, glyph
  names), shared by text extraction and rendering.
- Feature flags: `std` (default) and `alloc`; the crate builds as `no_std` with
  `core` + `alloc` on bare-metal and wasm targets. `Document::open(&[u8])` is
  the entry point without a file system.

### Changed

- `flate2` was replaced by `miniz_oxide`, dropping a dependency and the last
  `std::io` use in the library.
- Dictionary and xref lookups use `alloc`-only collections (`BTreeMap`,
  `BTreeSet`), removing the last `std`-only container from the core.

### Fixed

- `classic-xref.pdf` declared no `/Resources` for the page it repaints; the
  content stream's `/F1` is now declared.

## [0.2.0-alpha.1] — 2026-11-14

First release of the PDF 2.0 implementation. The crate is still alpha: the
read side is broad and fixture-tested, the write side is deliberately small,
and the public API may change before 0.2.0.

### Reading

- Object model for every base type, parsed and re-serialized byte-for-byte,
  with correct escaping for names, literal strings and hex strings.
- Tokenizer and parser for direct and indirect objects, including `stream` …
  `endstream` payloads delimited by `/Length`.
- Classic cross-reference tables with subsections, free entries and `/Prev`
  chains; cross-reference streams (`/Type /XRef` with `/W` and `/Index`);
  object streams (`/ObjStm`); hybrid files (`/XRefStm`).
- Lazy object fetching with a cache.
- Filters: FlateDecode (zlib and raw DEFLATE), LZWDecode with `/EarlyChange`,
  ASCIIHexDecode, ASCII85Decode, RunLengthDecode, plus `/Filter` and
  `/DecodeParms` arrays, PNG 10–15 and TIFF 2 predictors, and indirect
  `/Length`.
- Document model: catalog, page tree with inherited `/MediaBox`, `/CropBox`,
  `/Rotate` and `/Resources`, page content streams, and XMP `/Metadata`.
  `/Count` is treated as a hint and never trusted.
- Content-stream interpretation and text extraction for simple fonts
  (`StandardEncoding`, `WinAnsiEncoding`, `/Differences`), with `BT`/`ET`,
  `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tj`, `TJ`, `'` and `"`; inline images
  are skipped.
- AES-256 decryption: standard security handler `/V 5 /R 6`, user and owner
  passwords, Algorithm 2.B password hashing, per-object-string and stream
  decryption honouring `/EncryptMetadata`, and the `/Perms` tamper check.
  Deprecated revisions (RC4, AES-128, `/R 5`) are refused with an error.
- No `unsafe` (`#![forbid(unsafe_code)]`) and no panics on malformed input:
  `tests/robustness.rs` opens every fixture from every truncation all the way
  through sampled mutations and deletions.

### Writing

- `Document::new()` … `save()` emits a `%PDF-2.0` file with a Flate-compressed
  `/ObjStm`, a `/Type /XRef` cross-reference stream with recorded byte offsets,
  XMP `/Metadata`, and no `trailer` or `Info` dictionary.
- `add_page` appends pages with a content stream and a shared Helvetica `/F1`
  resource; `save_to_path` writes to disk.

### Tooling

- Nine committed fixtures, six generated by `tests/fixtures/make_fixtures.py`
  and the encryption pair by `examples/make_encrypted_fixture.rs`; both are
  deterministic.
- `docs/SPEC-MAP.md` maps ISO 32000-2 clauses to modules.
- CI on stable, beta and MSRV 1.87; publishing to crates.io through trusted
  publishing on a `v*` tag.

[0.2.0-alpha.1]: https://crates.io/crates/pdf2/0.2.0-alpha.1
[0.2.0-alpha.2]: https://crates.io/crates/pdf2/0.2.0-alpha.2
[0.7.0-rc.1]: https://crates.io/crates/pdf2/0.7.0-rc.1
[1.0.0]: https://crates.io/crates/pdf2/1.0.0
