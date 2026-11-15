# The feature ledger: what is shipped, scheduled, refused, or unplanned

`docs/ROADMAP-NEXT.md` ranks the gaps we *measured*; this file is the ledger of
everything else, so an omission is a decision rather than an accident. Every row
has exactly one status:

- **shipped** — in the tree and exercised by a test or the corpus;
- **scheduled** — an item in `docs/ROADMAP-NEXT.md` (number given);
- **refused** — a non-goal in `docs/adr/0001-differentiators.md` (line given);
- **unplanned** — nothing owns it yet, with the reason and what would change that;
- **not applicable** — the clause describes no behaviour this library can have
  (a registry, a viewer-only feature, or a legacy construct with no reader);
- *partial* qualifies a status: the clause is mostly answered, and the cell says
  what is not.

Clause numbers are ISO 32000-2:2020; module paths live in
[`docs/SPEC-MAP.md`](SPEC-MAP.md), which already keys clauses to files — this
ledger names the clause and the evidence, it does not repeat the map.

Sources: `README.md` (status, limits), `src/` (model, filters, crypto, writer),
`crates/pdf2-render/src/` (rendering), `crates/pdf2-cli/src/` (tooling),
`crates/pdf2-wasm/src/` (browser API), `docs/CORPUS.md` (measured behaviour).

## Outside facts

Every fact a *scheduled* or *unplanned* row leans on, with the command or URL
that produced it. Recorded 2026-11-15; a fact whose date is older than the row it
supports is the row's problem, not the fact's.

| Fact | Value | Source | Leaned on by |
| --- | --- | --- | --- |
| Oracle engines on the CI runner | poppler-utils **26.01.0**, mupdf-tools **1.27.0**, ghostscript **10.06.0** | `gh run view 43707082330 --log`, the `Setting up …` and `pdftoppm version` lines | the oracle-gate row; `corpus/tolerances.tsv` is only comparable on these versions |
| Decoder crates behind item 9 | `hayro-jpeg2000` **0.5.0**, `hayro-jbig2` **0.4.0**, `hayro-ccitt` **0.5.0** (published 2026-11-02, `Apache-2.0 OR MIT`, **`rust-version = "1.92"`**); `zune-jpeg` **0.5.15** stable (2026-03-26, `MIT OR Apache-2.0 OR Zlib`, `rust-version = "1.75"`) | crates.io API, `…/crates/{name}` and `…/crates/{name}/{version}` | *scheduled 9*: only `zune-jpeg` clears our MSRV; the three hayro crates need 1.92, so they cannot land until the MSRV moves (or stay behind a feature built on a newer toolchain) |
| Our MSRV | **1.87** | `Cargo.toml` `rust-version`, `rust-toolchain.toml`, the `Test (1.87)` CI job | the same row, and every dependency decision |
| Adobe CMap licence | **BSD-3-Clause**, copyright 1990–2023 | `https://raw.githubusercontent.com/adobe-type-tools/cmap-resources/master/LICENSE.md`, `gh api repos/adobe-type-tools/cmap-resources` (301★, last push 2023-11-15) | *scheduled 5*: the licence is answerable, see the next row |
| `deny.toml` allow-list | MIT, Unlicense, Apache-2.0, Zlib, 0BSD — **no BSD-3-Clause** | `deny.toml:13-19` | *scheduled 5*: shipping CMaps needs that entry plus the notice, or the data lives outside the crate; recorded as a fact, not fixed here |
| Tier A corpus | pdf.js `test/pdfs` at commit `d60bc7426aa0`, **Apache-2.0**, 23 files with SHA-256s | `corpus/manifest.json`, `corpus/README.md`, `corpus/LICENSES/Apache-2.0.txt` | the corpus row |
| Tier B corpora planned | veraPDF corpus **CC BY 4.0** (attribution required); pdf.js full tree **Apache-2.0** | `https://raw.githubusercontent.com/veraPDF/veraPDF-corpus/master/README.md`; `gh api repos/mozilla/pdf.js` | roadmap item 7 |

## Clause coverage

Checked 2026-11-15 against the standard's contents list (clauses 7–14, annexes D, E, F, I, K, L, O, Q); every clause resolves to a row below.

Check against the standard's contents: every clause from 7 to 14 and every
normative annex has rows answering for it. Clauses 1–6 (scope, references,
terms, notation, version designations, conformance) describe the document, not
behaviour.

| Clause | Rows that answer for it |
| --- | --- |
| `7` Syntax (`7.2`–`7.12`) | lexical, objects, filters, file structure, encryption, document structure, content streams — §File structure and objects; `7.9` dates are not modelled, `7.10` functions `unplanned` (shadings need them), `7.11` embedded files `unplanned`, `7.12` extensions `unplanned` |
| `8` Graphics (`8.1`–`8.11`) | painting rows, colour spaces, images (`8.9`), Form XObjects (`8.10`) `unplanned`, optional content (`8.11`) `unplanned` — §Painting and page geometry, §Colour, §Images |
| `9` Text (`9.1`–`9.10`) | fonts, encodings, `/Widths`, extraction — §Fonts and text |
| `10` Rendering (`10.1`–`10.8`) | colour conversion and ICC (`10.3`–`10.4`) — §Colour; transfer functions (`10.5`) and halftones (`10.6`) `unplanned`; scan conversion (`10.7`) `unplanned`; separations (`10.8`) with `8.6.6.4` |
| `11` Transparency (`11.1`–`11.7`) | the transparency row, `scheduled 10` — §Painting and page geometry |
| `12` Interactive features (`12.1`–`12.11`) | §Interactive features |
| `13` Multimedia (`13.1`–`13.7`) | §Multimedia |
| `14` Document interchange (`14.1`–`14.13`) | §Document interchange, plus the metadata and `Info` rows in §File structure and objects |
| Annexes `D`, `E`, `F`, `I`, `K`, `L`, `O`, `Q` | §Normative annexes |

## File structure and objects

Checked 2026-11-15 — `cargo test --workspace` (322 tests, including `tests/document.rs`, `tests/xref_streams.rs`, `tests/writer.rs`); `cargo run -p pdf2-cli -- info tests/fixtures/classic-xref.pdf` (incremental update) and `… info corpus/tier-a/bug_jpx.pdf` (the one committed file that declares `/Linearized`).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Classic xref tables, `/Prev` chains, incremental updates (read) | shipped | `src/document.rs`, `tests/document.rs` |
| Cross-reference streams, `/ObjStm`, hybrid files | shipped | `src/document.rs`, `tests/xref_streams.rs` |
| Object streams at scale (65k objects, budgeted time) | scheduled 8 | `docs/GAPS.md` robustness row |
| Damaged files: repair/rebuild policy beyond error reporting | unplanned | We report and stop. Demand unmeasured; a `--repair` mode would need a corpus of damaged files first. |
| Linearised files (read) | shipped | `corpus/tier-a/bug_jpx.pdf` declares `/Linearized` and the CLI reads it (`pdf2 check`, 2026-11-15); linearisation is an ordering, so the parser reads it by construction |
| Linearisation (write) | unplanned | No demand evidence; `qpdf` owns the use case |
| Incremental update (write) | scheduled 11 | `src/writer.rs` builds new documents only |
| `7.9` Common data structures — dates, names, strings, rectangles | shipped, partial | `Rect` and the name/string objects are modelled (`src/object.rs`); **dates are not**: no `Date` type exists, so `/CreationDate` and `/ModDate` are unread strings |
| `7.10` Functions (type 0–4, used by shadings and separations) | scheduled 10 | Nothing evaluates PDF functions; shadings cannot be drawn without them, so the row rides with item 10. Owner: item 10 |
| `7.11` Embedded files and file specifications (`/EmbeddedFiles`, `/Filespec`) | unplanned | No `EmbeddedFiles` or `/Filespec` handling anywhere; attachments are invisible. Owner: none |
| `7.12` Extensions dictionary | unplanned | No `Extensions` handling; extension levels are ignored rather than reported. Owner: none |
| `7.5.8` Cross-reference generation numbers (the 1.x generation edge) | shipped | A free entry's generation above 16 bits is clamped (the free-list head is never used for a lookup): `corpus/tier-a/ContentStreamCycleType3insideType3.pdf` and `tests/fixtures/xref-generation.pdf` both read, the latter pinned by `tests/xref_streams.rs`. Owner: item 6 (shipped 2026-11-15) |

## Filters and images

Checked 2026-11-15 — `cargo test -p pdf2 --test filters`; the four image filters are refused by `src/filter.rs` and the corpus rows show the result (`cmykjpeg.pdf`, `bug_jpx.pdf`, `ccitt_EndOfBlock_false.pdf` render blank).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| FlateDecode (+ PNG/TIFF predictors), LZW, ASCIIHex, ASCII85, RunLength | shipped | `src/filter.rs`, `tests/filters.rs` |
| `/DCTDecode` (JPEG) | scheduled 9 | Refused today with "image filters are out of scope" (`src/filter.rs:41`); `cmykjpeg.pdf` renders blank. The decoder crates and their `rust-version` requirement are in §Outside facts |
| `/JPXDecode` (JPEG 2000) | scheduled 9 | `bug_jpx.pdf` in the corpus |
| `/CCITTFaxDecode` | scheduled 9 | `ccitt_EndOfBlock_false.pdf` |
| `/JBIG2Decode` | scheduled 9 | No committed sample; `jbig2-bench` shows decoder divergence |
| Images above 8 bits per component | unplanned | `crates/pdf2-render/src/image.rs` accepts 8 only; rare in practice, needs a corpus count first |
| `/SMask`, `/Mask` (soft mask, colour key) | scheduled 10 | `colorkeymask.pdf` renders today without the mask — visibly wrong; it folds into item 10 with transparency. Owner: item 10 |
| `/Interpolate` beyond bilinear | unplanned | Nearest and bilinear are both implemented; other filters are viewer-defined |

## Colour

Checked 2026-11-15 — `cargo test -p pdf2-render` (calibration page 1 golden) and the oracle gate's calibrated comparison of that page (widest spread: 8.0%).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| DeviceGray, DeviceRGB, DeviceCMYK (naive), `/ICCBased` by component count | shipped | `src/content.rs`, `crates/pdf2-render/src/image.rs` |
| ICC colour management (profiles actually applied) | scheduled 10b | The oracle gate measures an **8% spread** on the CMYK page; the oracles disagree because they manage colour. Owner: item 10b |
| `8.6.6.4` `/Separation`, `/DeviceN`, spot colours — and `10.8` rendering for separations | unplanned | No corpus sample yet; would ride with ICC management. Separations need both the colour space and the rendering rule: a separation tint is a function, and `10.8` says how it becomes device colour when no separation device exists. |

## Fonts and text

Checked 2026-11-15 — `cargo test -p pdf2-render` (font unit tests, pages 9–11 goldens) and `cargo run -q -p xtask -- corpus` (22 of 23 render, 15 fully readable; the 7 composite-font files draw).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Simple fonts: Type 1, TrueType with `/FontFile2`, encodings and `/Differences` | shipped | `crates/pdf2-render/src/font.rs`, page 9 of `calibration.pdf` |
| Base-14 without a program: built-in 5×7 fallback with composed Latin-1 accents | shipped | `crates/pdf2-render/src/font.rs` |
| Composite fonts (Type 0/CID) **rendering** with Identity-H and `/CIDToGIDMap` | shipped | pages 10–11, `cid_cff.pdf` renders |
| Composite fonts **extraction** (`/ToUnicode`, code splitting, CID widths) | shipped | `src/content.rs` reads Type0 with an Identity encoding: codes split by the CMap's code space, text from `bfchar`/`bfrange` (`src/cmap.rs`), advances from `/W`+`/DW` so a gap becomes a space. Five corpus files moved from reported to readable (`cargo run -q -p xtask -- corpus`: 20 of 23), `tests/fixtures/tounicode.pdf` extracts `ADabcxyz A` (`tests/content.rs`). Owner: item 2 (shipped 2026-11-15) |
| Predefined CMaps (CJK) | partial | One is shipped as data: Adobe-Japan1 `90ms-RKSJ-H` (code → text, 7 876 mappings, 19 967 B, generated by `cargo xtask cmaps` from the BSD-3-Clause resources in `vendor/adobe-cmaps/`). `90ms_rksj_h_sample.pdf` extracts `日本語テスト` (`tests/content.rs`), and the size is recorded against the wasm budget in `docs/PERFORMANCE.md`. Other CMaps are data additions. Owner: item 5 (partial 2026-11-15) |
| Predefined CMaps (other than Japan1 `90ms-RKSJ-H`) | scheduled 5 | Needs ~250 KB of data. The licence question is answered in §Outside facts: Adobe's CMaps are **BSD-3-Clause**, `deny.toml` does not allow it, so the allow-list entry (and the notice) or a home outside the crate is a prerequisite, not an afterthought |
| CFF program parsing and outlines | shipped | `/CIDFontType0C` is wrapped in a five-table OpenType container and read by `ttf-parser`; the charset is inverted for CID-keyed fonts. `cid_cff.pdf` and `Embedded_font.pdf` draw their outlines and no longer report `font:CIDFontType0` (`crates/pdf2-render/tests/cff.rs`). Simple-font `/Type1C` (glyph names against the encoding) is still reported, as is `/FontFile` Type 1. Owner: item 4 (shipped 2026-11-15) |
| `/FontFile` Type 1 (PFB/PFA) programs | scheduled 4 | Not parsed: the format is not CFF and `ttf-parser` does not read it; the bitmap fallback is drawn and `font:FontFile` is reported |
| Text render modes (`Tr`) other than fill | scheduled 10 | Folded into item 10 (painting completeness) when the ledger's first pass found it unowned. Owner: item 10 |
| Type 3 fonts | scheduled 10 | Same: measured in the corpus (`Type3WordSpacing.pdf` renders with skips). Owner: item 10 |
| Vertical writing mode | unplanned | No corpus sample; rare outside CJK vertical documents |
| Extraction quality: reading order, spaces from advances, ligatures, spans/positions | shipped (partial) | **Reading order shipped** (geometric line order, `src/content.rs`; `reading-order.pdf`, `ShowText-ShadingPattern.pdf`); **spans refused for 1.2** and the rest scheduled: the decision and its reasons are `docs/adr/0002-text-extraction-shape.md`, advances arrive with item 2 (composite `/Widths`). Owner: item 2b (partial 2026-11-15) | Needs an API decision (text vs spans) Owner: item 2b (an API decision comes first) |
| Font subsetting (write) | not applicable | The writer does not embed fonts; callers supply programs |

## Painting and page geometry

Checked 2026-11-15 — `cargo test -p pdf2-render` (calibration pages 1–5, `tests/calibration.rs`) and the oracle check in CI (9 cases within tolerance).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Paths, curves, fills (both rules), strokes with caps/joins/miter limit, dashes, clipping, transforms | shipped | pages 1–3, `docs/RENDERING.md` |
| `/MediaBox` origin, `/Rotate` 0/90/180/270 | shipped | pages 4–5 |
| `/CropBox` as the visible box | shipped | `crates/pdf2-render/src/walk.rs` uses `crop_box.or(media_box)`: `tests/fixtures/cropbox.pdf` renders 400×400 with the crop box at the origin and content clipped at its edge (`crates/pdf2-render/tests/golden.rs`), and the oracle gate cannot judge it — poppler agrees with this renderer's 800×800 at 144 dpi, mupdf ignores crop box and user unit and gives 600×400 (`docs/PROOF.md`), so the page is proven by arithmetic instead. Owner: item 6b (shipped 2026-11-15) |
| `/UserUnit` | shipped | `Document::page_user_unit` (`src/page.rs`, inheritable, defaulting to 1) multiplies the renderer's scale; the cropbox fixture at `/UserUnit 2` proves it (`tests/pages.rs`, `golden.rs`). Owner: item 6b (shipped 2026-11-15) |
| Shadings and patterns | scheduled 10 | `ShowText-ShadingPattern.pdf`, `axial_shading_many_inputs.pdf` render with skips |
| Transparency groups, soft masks, blend modes (`11`) | scheduled 10 | `alphatrans.pdf`, `blendmode.pdf` |
| `8.10` **Form XObjects** (`/Subtype /Form`) | scheduled 10d | **Not drawn at all**: `Do` calls `draw_image`, which reports `Do` and returns for any subtype that is not `/Image` — `crates/pdf2-render/src/walk.rs:534` (the `Do` dispatch) and `:701` (inside `draw_image`, the `/Subtype /Image` test that bails out) — so a page whose vector content lives in a Form XObject draws nothing. Owner: item 10d — the roadmap's revision assigned it after the ledger's decision 8 named it unowned. |
| `8.11` **Optional content** (layers, `/OCProperties`) | unplanned | No `OCProperties` or `OCG` handling exists anywhere in `src/` or `crates/`; layer visibility is not modelled, so a page with layers draws every layer or none. Owner: item 10d — the roadmap's revision assigned it after the ledger's decision 8 named it unowned. |
| `10.5` **Transfer functions** and `10.6` **Halftones** | unplanned | Neither exists: `TransferFunction` and `Halftone` appear nowhere in `src/` or `crates/`. For screen output both are largely moot; for print fidelity they are not, and `10.8` separations need `10.5`. Owner: none (print-only, no demand evidence) |
| `10.7` **Scan conversion** — anti-aliasing, glyph cache | unplanned | Coverage is decided at pixel centres, with no anti-aliasing (`crates/pdf2-render/src/raster.rs:108`, `docs/RENDERING.md`), which is why the oracle gate compares at 4×4 blocks. The speed half has no item either: ADR non-goal 1 refuses *competing* on speed, which is not the same as having no target; Owner: none for anti-aliasing; the *speed* half is item 10c below. |
| Render-time budget (no AA, no glyph cache, no time gate) | scheduled 10c | The oracle job can record render time per page; the roadmap's item 10c owns the budget. Owner: item 10c |

## Documents and metadata

Checked 2026-11-15 — `cargo test -p pdf2 --test pages --test document`; `cargo run -p pdf2-cli -- info tests/fixtures/text.pdf` reports `metadata: no`, `page-tree.pdf` reports `yes`.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `7.7.3` Page tree with inherited attributes | shipped | `src/page.rs`, `tests/pages.rs` |
| `14.3` Metadata: XMP (`/Metadata`) read | shipped | `src/page.rs` (`Document::metadata`), `tests/document.rs` |
| `14.3` `Info` dictionary (legacy) | refused | Deliberately out of scope in `README.md` (limits): it is not read and the writer emits none — XMP is the PDF 2.0 replacement. The earlier version of this row claimed it as *shipped*, which was false. |

## Interactive features (`12`)

Checked 2026-11-15 — no shipped row in this table; the absences were re-confirmed with `grep -rn "ViewerPreferences\|Outlines\|OpenAction\|Dests\|StructTreeRoot" src crates` (empty).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `12.2` Viewer preferences | unplanned | No `ViewerPreferences` in the model; a viewer's concern that a library does not need, and nothing else depends on it |
| `12.3` Document-level navigation — outlines, destinations, `/OpenAction` | unplanned | No `Outlines`, `Dests` or `OpenAction` handling anywhere; bookmarks are invisible |
| `12.4` Page-level navigation — link annotations and `/Dest` | scheduled 12 | Rides with `12.5`. Owner: item 12 |
| `12.5` Annotations — read, render, appearance streams | scheduled 12 | Nothing in the tree today |
| `12.6` Actions — `/GoTo`, `/URI`, JavaScript | unplanned | Actions are not modelled; JavaScript is refused (ADR non-goal 3) |
| `12.7` Forms — read, render, **fill** | scheduled 12 | The ADR's non-goal 3 refuses *authoring*; filling is not authoring, and the ADR says so |
| `12.8` Digital signatures | refused | ADR non-goal 3; a signature implementation is a security product with its own review burden |
| `12.9` Measurement properties (PDF 2.0) | unplanned | Not modelled |
| `12.10` Geospatial features (PDF 2.0) | unplanned | Not modelled |
| `12.11` Document requirements (PDF 2.0) | unplanned | Not modelled |

## Multimedia (`13`)

Checked 2026-11-15 — no shipped row; `grep -rn "Sound\|Movie\|RichMedia" src crates` is empty.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `13.2` Multimedia, `13.7` Rich media | unplanned | Not modelled; rendering rich media is a viewer's job |
| `13.3` Sounds | unplanned | Not modelled; playback is a viewer feature |
| `13.4` Movies | unplanned | Not modelled; superseded by rich media in PDF 2.0 practice |
| `13.5` Alternate presentations | unplanned | Not modelled |
| `13.6` 3D artwork | refused | ADR non-goal 3 |

## Document interchange (`14`)

Checked 2026-11-15 — no shipped row in this table (metadata is verified in §Documents and metadata); `grep -rn "OutputIntents\|EmbeddedFiles\|StructTreeRoot\|b\"BMC\"\|b\"BDC\"" src crates` is empty.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `14.2` Procedure sets | not applicable | Operator sets for PDF 1.0 viewers; a processor has nothing to do with them |
| `14.4` File identifiers (`/ID`) | unplanned | No `/ID` is read or written; the writer emits none |
| `14.5` Page-piece dictionaries | unplanned | Private producer data; not retained, and the writer does not carry unknown page keys forward |
| `14.6` Marked content — `BMC`/`BDC`/`EMC`, `/ActualText`, `/Artifact` | unplanned | Marked-content operators are not interpreted, so `/ActualText` never replaces what extraction returns and artefacts are extracted as text |
| `14.7` Logical structure, `14.8` Tagged PDF, `14.9` Accessibility support | refused | ADR non-goal 7 |
| `14.10` Web capture | not applicable | Legacy PDF 1.x capture format; no processor keeps it |
| `14.11` Prepress — output intents, separations | unplanned | No `/OutputIntents`; separations ride with `8.6.6.4`/`10.8`. Note: `docs/SPEC-MAP.md` keys `14.11.2` to clipping paths, which is a map error — recorded here, not repaired (out of scope) |
| `14.12` Document parts | unplanned | PDF 2.0 document parts are not modelled |
| `14.13` Associated files | scheduled 12 | Not modelled; item 12's file-attachment annotations need it. Owner: item 12 |

## Normative annexes

Checked 2026-11-15 — `cargo test --workspace` (`src/encoding.rs` unit tests, `tests/document.rs` for the version gate) and `pdf2 check corpus/tier-a/bug_jpx.pdf` for annex F.

| Annex | Status | Evidence / owner |
| --- | --- | --- |
| `D` Character sets and encodings | shipped, partial | `src/encoding.rs`: `StandardEncoding`, `WinAnsiEncoding`, `/Differences`; the upper half of `MacRoman` and glyph names outside the built-in table are skipped (README limits) |
| `E` PDF Name Registry | not applicable | A registry of names and abbreviations; no behaviour to implement |
| `F` Linearized PDF | not applicable | Linearisation is an ordering; the parser reads such files by construction (see the linearisation row in §File structure and objects), and no writer emits it |
| `I` PDF versions and compatibility | shipped | `src/document.rs` accepts `%PDF-1.x` and `%PDF-2.x`, refuses the rest (`tests/document.rs`) |
| `K` XFA forms | refused | ADR non-goal 3 |
| `L` Parent-child relationships between structure elements | refused | ADR non-goal 7 (structure trees) |
| `O` Fragment identifiers | unplanned | No fragment/destination handling; URIs with fragments are opaque strings to the library |
| `Q` Method for determining transparency on a page | unplanned | Rides with item 10: without transparency groups, there is nothing to determine |

## Encryption

Checked 2026-11-15 — `cargo test -p pdf2 --test encryption`; the corpus's `encrypted-attachment.pdf` now reads (its `startxref` is repaired and its `/Identity` filters mean nothing is encrypted) the row above schedules.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| AES-256, revision 6, read side, user and owner passwords, `/Perms` check | shipped | `src/crypto.rs`, `tests/encryption.rs` |
| RC4 and AES-128, revisions 2–4 (read) | shipped | `src/crypto.rs`: `/V 1`, `/V 2`, `/V 4` with `/CFM /V2` and `/AESV2`, Algorithm 2 key derivation over `/O`, `/P` and `/ID`, the per-object key, and Algorithm 7 for the owner password; `tests/encryption.rs` round-trips two generated revision-4 fixtures (`encrypted-r4-rc4.pdf`, `encrypted-r4-aes.pdf`) against `plain-twin.pdf`. Owner: item 3 (shipped 2026-11-15) |
| Encryption on write | unplanned | No demand evidence; the writer emits unencrypted files by design |
| `/Crypt` filters and per-stream encryption | partial | The `/CF` crypt filters of `/V 4` and `/V 5` are read for their method (`/V2`, `/AESV2`, `/AESV3`, `/None`), `/Identity` is honoured, and a file naming different filters for strings and streams is refused rather than guessed at; per-stream `/Crypt` filters remain unplanned |

## API, tooling, proof

Checked 2026-11-15 — `cargo test --workspace`; `cargo run -p pdf2-cli -- info|pages|text|check`; `cargo run -q -p xtask -- wasm-package` (API smoke test and size budget: 206 823 B gzip of 266 240); `cargo run -q -p xtask -- corpus`; five fuzz targets, last nightly run green 2026-11-15; the oracle gate's last run green 2026-11-15.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `Document`, `Page` model, lazy objects, caching, `Result`-only errors | shipped | `src/` |
| Editing API (mutate an opened document) | scheduled 11 | |
| CLI: `info`, `pages`, `text`, `check`, `--json`, `--password` | shipped | `crates/pdf2-cli` |
| CLI `render` subcommand | unplanned | The corpus run uses the renderer's golden binary; a batch render was not needed yet |
| wasm: open, page count/size, text, metadata, render to pixels/canvas, render report | shipped | `crates/pdf2-wasm` |
| wasm/npm API growth (spans, page boxes, structured errors) | unplanned | Follows the extraction-quality decision above |
| Fixtures with arithmetic (calibration, book) | shipped | `tests/fixtures`, `docs/RENDERING.md` |
| Tier B corpus in CI (pdf.js full tree, veraPDF) | scheduled 7 | Formula and licences are in §Outside facts; the roadmap's item 7 owns the scheduled run. Owner: item 7 |
| Tier A corpus, oracle gate with calibrated tolerances | shipped | `corpus/` (23 files, hashed), `corpus/tolerances.tsv`, `crates/pdf2-oracle`, the `oracle` job; last run green 2026-11-15. Owner: item 1 (shipped). The tolerances are only comparable on the engines recorded in §Outside facts (poppler 26.01.0, mupdf 1.27.0, ghostscript 10.06.0) |
| Tier B corpus in CI (pdf.js full, veraPDF) | scheduled 7 | |
| Fuzzing (five targets, nightly), deny, semver, bench gates | shipped | `fuzz/fuzz_targets` (5), `.github/workflows/{fuzz,ci}.yml`; last nightly fuzz run green 2026-11-15 |
| Validation (PDF/A, PDF 2.0 conformance) | unplanned | Different buyer; `veraPDF` is the reference. Owner: none — roadmap item 13 keeps it unscheduled. Owner: none — roadmap item 13 keeps it unscheduled |

## Decisions this ledger forces

1. **Text render modes and Type 3** are in the "still unproven" list with no
   owner → fold into item 10 (painting completeness), which already exists for
   transparency and shadings.
2. **Renderer performance** has no item at all → add a budget to the oracle job
   (render-time per page) and a `1.3` item with a target, distinct from ADR
   non-goal 1 (which refuses racing hayro, not having a number).
3. **ICC colour management** is a fidelity decision the 8% oracle spread makes
   unavoidable → schedule it with item 10, not later.
4. **Extraction quality** (order, spaces, ligatures, spans) needs an API decision
   before it can be scheduled → a design note, then an item in `1.2` next to
   item 2.
5. **Tagged PDF / PDF/UA** is neither planned nor refused → an explicit refusal
   in the ADR (different product, structure trees are a separate model) unless
   the answer is otherwise.
6. **Forms wording**: the ADR refuses "forms authoring"; item 12 includes
   filling → say so in both places (filling is planned, authoring is not).
7. **`/CropBox` and `/UserUnit`** were cheap and unowned — **resolved**: item 6b
   shipped them (`Document::page_user_unit`, the renderer's visible box) with a
   fixture and an oracle case.
8. **Form XObjects (`8.10`)** are not drawn, and they used to have no owner —
   a bigger practical hole than shadings, since vector content, nested pages and
   some text live in Form XObjects in almost every real document. **Resolved in
   the roadmap's revision: item `10d`, in `1.3` beside painting completeness.**
