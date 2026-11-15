# The feature ledger: what is shipped, scheduled, refused, or unplanned

`docs/ROADMAP-NEXT.md` ranks the gaps we *measured*; this file is the ledger of
everything else, so an omission is a decision rather than an accident. Every row
has exactly one status:

- **shipped** — in the tree and exercised by a test or the corpus;
- **scheduled** — an item in `docs/ROADMAP-NEXT.md` (number given);
- **refused** — a non-goal in `docs/adr/0001-differentiators.md` (line given);
- **unplanned** — nothing owns it yet, with the reason and what would change that.

Clause numbers are ISO 32000-2:2020; module paths live in
[`docs/SPEC-MAP.md`](SPEC-MAP.md), which already keys clauses to files — this
ledger names the clause and the evidence, it does not repeat the map.

Sources: `README.md` (status, limits), `src/` (model, filters, crypto, writer),
`crates/pdf2-render/src/` (rendering), `crates/pdf2-cli/src/` (tooling),
`crates/pdf2-wasm/src/` (browser API), `docs/CORPUS.md` (measured behaviour).

## File structure and objects

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Classic xref tables, `/Prev` chains, incremental updates (read) | shipped | `src/document.rs`, `tests/document.rs` |
| Cross-reference streams, `/ObjStm`, hybrid files | shipped | `src/document.rs`, `tests/xref_streams.rs` |
| Object streams at scale (65k objects, budgeted time) | scheduled 8 | `docs/GAPS.md` robustness row |
| Damaged files: repair/rebuild policy beyond error reporting | unplanned | We report and stop. Demand unmeasured; a `--repair` mode would need a corpus of damaged files first. |
| Linearised files (read) | shipped by construction | Linearisation is an ordering; the parser does not care |
| Linearisation (write) | unplanned | No demand evidence; `qpdf` owns the use case |
| Incremental update (write) | scheduled 11 | `src/writer.rs` builds new documents only |

## Filters and images

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

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| DeviceGray, DeviceRGB, DeviceCMYK (naive), `/ICCBased` by component count | shipped | `src/content.rs`, `crates/pdf2-render/src/image.rs` |
| ICC colour management (profiles actually applied) | unplanned | The oracle gate measures an **8% spread** on the CMYK page; the oracles disagree because they manage colour. This is a fidelity decision the next version must take, not a bug. |
| `8.6.6.4` `/Separation`, `/DeviceN`, spot colours — and `10.8` rendering for separations | unplanned | No corpus sample yet; would ride with ICC management. Separations need both the colour space and the rendering rule: a separation tint is a function, and `10.8` says how it becomes device colour when no separation device exists. |

## Fonts and text

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
| `10.7` **Scan conversion** — anti-aliasing, glyph cache, render speed budget | unplanned | Coverage is decided at pixel centres, with no anti-aliasing (`crates/pdf2-render/src/raster.rs:108`, `docs/RENDERING.md`), which is why the oracle gate compares at 4×4 blocks. The speed half has no item either: ADR non-goal 1 refuses *competing* on speed, which is not the same as having no target; a budget in the oracle job would make it measurable. |

## Documents, forms, signing

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Page tree with inherited attributes, XMP read, `Info` dictionary | shipped | `src/page.rs`; `Info` is legacy 2.0 |
| Annotations: read, render, appearance streams | scheduled 12 | Nothing in the tree today |
| Forms: read, render, **fill** | scheduled 12 | The ADR's non-goal 3 says "forms authoring"; filling is not authoring — the wording is corrected there |
| Forms authoring, JavaScript, XFA | refused | ADR non-goal 3 |
| Digital signatures (read and write) | refused | ADR non-goal 3; a signature implementation is a security product with its own review burden |
| 3D content | refused | ADR non-goal 3 |
| Tagged PDF: structure tree, accessibility (PDF/UA), reading order as structure | unplanned | Absent from the roadmap *and* from the refusals — the one genuine decision this ledger forces |

## Encryption

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| AES-256, revision 6, read side, user and owner passwords, `/Perms` check | shipped | `src/crypto.rs`, `tests/encryption.rs` |
| RC4 and AES-128, revisions 2–4 (read) | scheduled 3 | `encrypted-attachment.pdf` fails on this |
| Encryption on write | unplanned | No demand evidence; the writer emits unencrypted files by design |
| `/Crypt` filters and per-stream encryption | unplanned | Rides with revisions 2–4 |

## API, tooling, proof

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `Document`, `Page` model, lazy objects, caching, `Result`-only errors | shipped | `src/` |
| Editing API (mutate an opened document) | scheduled 11 | |
| CLI: `info`, `pages`, `text`, `check`, `--json`, `--password` | shipped | `crates/pdf2-cli` |
| CLI `render` subcommand | unplanned | The corpus run uses the renderer's golden binary; a batch render was not needed yet |
| wasm: open, page count/size, text, metadata, render to pixels/canvas, render report | shipped | `crates/pdf2-wasm` |
| wasm/npm API growth (spans, page boxes, structured errors) | unplanned | Follows the extraction-quality decision above |
| Fixtures with arithmetic (calibration, book) | shipped | `tests/fixtures`, `docs/RENDERING.md` |
| Tier A corpus, oracle gate with calibrated tolerances | shipped | `corpus/`, `crates/pdf2-oracle`, `ci.yml` |
| Tier B corpus in CI (pdf.js full, veraPDF) | scheduled 7 | |
| Fuzzing (five targets, nightly), deny, semver, bench gates | shipped | `.github/workflows` |
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
