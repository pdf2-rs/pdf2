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
| `7.10` Functions (type 0–4, used by shadings and separations) | unplanned | Nothing evaluates PDF functions; shadings (item 10) cannot be drawn without it |
| `7.11` Embedded files and file specifications (`/EmbeddedFiles`, `/Filespec`) | unplanned | No `EmbeddedFiles` or `/Filespec` handling anywhere; attachments are invisible |
| `7.12` Extensions dictionary | unplanned | No `Extensions` handling; extension levels are ignored rather than reported |

## Filters and images

Checked 2026-11-15 — `cargo test -p pdf2 --test filters`; the four image filters are refused by `src/filter.rs` and the corpus rows show the result (`cmykjpeg.pdf`, `bug_jpx.pdf`, `ccitt_EndOfBlock_false.pdf` render blank).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| FlateDecode (+ PNG/TIFF predictors), LZW, ASCIIHex, ASCII85, RunLength | shipped | `src/filter.rs`, `tests/filters.rs` |
| `/DCTDecode` (JPEG) | scheduled 9 | Refused today with "image filters are out of scope" (`src/filter.rs:41`); `cmykjpeg.pdf` renders blank |
| `/JPXDecode` (JPEG 2000) | scheduled 9 | `bug_jpx.pdf` in the corpus |
| `/CCITTFaxDecode` | scheduled 9 | `ccitt_EndOfBlock_false.pdf` |
| `/JBIG2Decode` | scheduled 9 | No committed sample; `jbig2-bench` shows decoder divergence |
| Images above 8 bits per component | unplanned | `crates/pdf2-render/src/image.rs` accepts 8 only; rare in practice, needs a corpus count first |
| `/SMask`, `/Mask` (soft mask, colour key) | unplanned | `colorkeymask.pdf` renders today without the mask — visibly wrong; cheap to fold into item 10 once transparency lands |
| `/Interpolate` beyond bilinear | unplanned | Nearest and bilinear are both implemented; other filters are viewer-defined |

## Colour

Checked 2026-11-15 — `cargo test -p pdf2-render` (calibration page 1 golden) and the oracle gate's calibrated comparison of that page (widest spread: 8.0%).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| DeviceGray, DeviceRGB, DeviceCMYK (naive), `/ICCBased` by component count | shipped | `src/content.rs`, `crates/pdf2-render/src/image.rs` |
| ICC colour management (profiles actually applied) | unplanned | The oracle gate measures an **8% spread** on the CMYK page; the oracles disagree because they manage colour. This is a fidelity decision the next version must take, not a bug. |
| `8.6.6.4` `/Separation`, `/DeviceN`, spot colours — and `10.8` rendering for separations | unplanned | No corpus sample yet; would ride with ICC management. Separations need both the colour space and the rendering rule: a separation tint is a function, and `10.8` says how it becomes device colour when no separation device exists. |

## Fonts and text

Checked 2026-11-15 — `cargo test -p pdf2-render` (font unit tests, pages 9–11 goldens) and `cargo run -q -p xtask -- corpus` (21 of 23 render, 14 fully readable; the 7 composite-font files draw).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Simple fonts: Type 1, TrueType with `/FontFile2`, encodings and `/Differences` | shipped | `crates/pdf2-render/src/font.rs`, page 9 of `calibration.pdf` |
| Base-14 without a program: built-in 5×7 fallback with composed Latin-1 accents | shipped | `crates/pdf2-render/src/font.rs` |
| Composite fonts (Type 0/CID) **rendering** with Identity-H and `/CIDToGIDMap` | shipped | pages 10–11, `cid_cff.pdf` renders |
| Composite fonts **extraction** (`/ToUnicode`, two-byte codes) | scheduled 2 | `src/content.rs:253`; 7 of 23 corpus files |
| Predefined CMaps (CJK) | scheduled 5 | Needs ~250 KB of data **and a licence check on the Adobe CMap files before shipping** |
| CFF and Type 1 program parsing | scheduled 4 | `font:FontFile3`/`font:FontFile` reported today |
| Text render modes (`Tr`) other than fill | unplanned | In `docs/RENDERING.md`'s unproven list with **no roadmap item** — should be folded into item 10 |
| Type 3 fonts | unplanned | Same: measured in the corpus (`Type3WordSpacing.pdf` renders with skips), no owner |
| Vertical writing mode | unplanned | No corpus sample; rare outside CJK vertical documents |
| Extraction quality: reading order, spaces from advances, ligatures, spans/positions | unplanned | Item 2 fixes *mapping*; this is the class of complaint behind most of `pdf-extract`'s 78 issues. Needs an API decision (text vs spans) before it can be scheduled |
| Font subsetting (write) | not applicable | The writer does not embed fonts; callers supply programs |

## Painting and page geometry

Checked 2026-11-15 — `cargo test -p pdf2-render` (calibration pages 1–5, `tests/calibration.rs`) and the oracle check in CI (9 cases within tolerance).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Paths, curves, fills (both rules), strokes with caps/joins/miter limit, dashes, clipping, transforms | shipped | pages 1–3, `docs/RENDERING.md` |
| `/MediaBox` origin, `/Rotate` 0/90/180/270 | shipped | pages 4–5 |
| `/CropBox` as the visible box | unplanned | Parsed and exposed (`src/page.rs:97`), ignored by the renderer; cheap, no evidence either way |
| `/UserUnit` | unplanned | Not parsed at all; affects page size and content scale |
| Shadings and patterns | scheduled 10 | `ShowText-ShadingPattern.pdf`, `axial_shading_many_inputs.pdf` render with skips |
| Transparency groups, soft masks, blend modes (`11`) | scheduled 10 | `alphatrans.pdf`, `blendmode.pdf` |
| `8.10` **Form XObjects** (`/Subtype /Form`) | unplanned | **Not drawn at all**: `Do` calls `draw_image`, which reports `Do` and returns for any subtype that is not `/Image` — `crates/pdf2-render/src/walk.rs:534` (the `Do` dispatch) and `:701` (inside `draw_image`, the `/Subtype /Image` test that bails out) — so a page whose vector content lives in a Form XObject draws nothing. No roadmap item owns it. |
| `8.11` **Optional content** (layers, `/OCProperties`) | unplanned | No `OCProperties` or `OCG` handling exists anywhere in `src/` or `crates/`; layer visibility is not modelled, so a page with layers draws every layer or none. No roadmap item owns it. |
| `10.5` **Transfer functions** and `10.6` **Halftones** | unplanned | Neither exists: `TransferFunction` and `Halftone` appear nowhere in `src/` or `crates/`. For screen output both are largely moot; for print fidelity they are not, and `10.8` separations need `10.5` |
| `10.7` **Scan conversion** — anti-aliasing, glyph cache, render speed budget | unplanned | Coverage is decided at pixel centres, with no anti-aliasing (`crates/pdf2-render/src/raster.rs:108`, `docs/RENDERING.md`), which is why the oracle gate compares at 4×4 blocks. The speed half has no item either: ADR non-goal 1 refuses *competing* on speed, which is not the same as having no target; a budget in the oracle job would make it measurable. |

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
| `12.4` Page-level navigation — link annotations and `/Dest` | unplanned | Rides with `12.5` (item 12) |
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
| `14.13` Associated files | unplanned | Not modelled, and item 12's annotation work would need it for file attachments |

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

Checked 2026-11-15 — `cargo test -p pdf2 --test encryption`; the corpus's `encrypted-attachment.pdf` fails on the RC4/AES-128 revisions the row above schedules.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| AES-256, revision 6, read side, user and owner passwords, `/Perms` check | shipped | `src/crypto.rs`, `tests/encryption.rs` |
| RC4 and AES-128, revisions 2–4 (read) | scheduled 3 | `encrypted-attachment.pdf` fails on this |
| Encryption on write | unplanned | No demand evidence; the writer emits unencrypted files by design |
| `/Crypt` filters and per-stream encryption | unplanned | Rides with revisions 2–4 |

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
| Tier A corpus, oracle gate with calibrated tolerances | shipped | `corpus/` (23 files, hashed), `corpus/tolerances.tsv`, `crates/pdf2-oracle`, the `oracle` job; last run green 2026-11-15 |
| Tier B corpus in CI (pdf.js full, veraPDF) | scheduled 7 | |
| Fuzzing (five targets, nightly), deny, semver, bench gates | shipped | `fuzz/fuzz_targets` (5), `.github/workflows/{fuzz,ci}.yml`; last nightly fuzz run green 2026-11-15 |
| Validation (PDF/A, PDF 2.0 conformance) | unplanned | Different buyer; `veraPDF` is the reference (roadmap item 13 keeps it unscheduled) |

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
7. **`/CropBox` and `/UserUnit`** are cheap and unowned → one small item in
   `1.2` with a corpus check.
8. **Form XObjects (`8.10`)** are not drawn, and nothing owns them. That is a
   bigger practical hole than shadings: vector content, nested pages and some
   text live in Form XObjects in almost every real document. It needs a roadmap
   decision before the next release, not a quiet row: either an item in `1.3`
   beside painting completeness, or an explicit refusal with the consequence
   written down.
