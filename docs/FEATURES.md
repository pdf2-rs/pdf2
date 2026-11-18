# The feature ledger: what is shipped, scheduled, refused, or unplanned

`docs/ROADMAP-NEXT.md` ranks the gaps we *measured*; this file is the ledger of
everything else, so an omission is a decision rather than an accident. Every row
has exactly one status:

- **shipped** — in the tree and exercised by a test or the corpus;
- **scheduled** — an item in `docs/ROADMAP-NEXT.md` (number given);
- **refused** — a non-goal in `docs/adr/0001-differentiators.md` (line given);
- **unplanned** — nothing owns it yet, with the reason and what would change that;
- **deferred** — a recorded decision not to do it yet, with the reason; the list
  lives in [`docs/DEFERRED.md`](DEFERRED.md);
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
| Decoder crates behind item 9 | `hayro-jpeg2000` **0.5.0**, `hayro-jbig2` **0.4.0**, `hayro-ccitt` **0.5.0** (`Apache-2.0 OR MIT`, `rust-version = "1.92"`); `zune-jpeg` **0.5.15** (`MIT OR Apache-2.0 OR Zlib`, `rust-version = "1.75"`); `moxcms` **0.9.2** (`BSD-3-Clause OR Apache-2.0`, `rust-version = "1.89"`) | crates.io API, `…/crates/{name}` and `…/crates/{name}/{version}` | *shipped behind features*: `dct` is on by default, `jpx` and `jbig2` are not (ADR 0007 — **+78 174** and **+64 583 gzip**, measured on the 1.8.0 tree). All three integrations take `default-features = false, features = ["std"]`, which keeps `fearless_simd`'s unsafe and the `image`/`moxcms` path out. Owner: item 9 (shipped 2026-11-17) |
| Our MSRV | **1.100** | `Cargo.toml` `rust-version`, `.clippy.toml` `msrv`, `rust-toolchain.toml`, the `Test (1.100)` CI job | the same row, and every dependency decision — raised from 1.87 on 2026-11-17, which unblocks the 1.92 decoders (`docs/DEFERRED.md`); what each release 1.88–1.100 brought and whether it was used is catalogued in [`docs/MSRV.md`](MSRV.md) |
| Adobe CMap licence | **BSD-3-Clause**, copyright 1990–2023 | `https://raw.githubusercontent.com/adobe-type-tools/cmap-resources/master/LICENSE.md`, `gh api repos/adobe-type-tools/cmap-resources` (301★, last push 2023-11-15) | *scheduled 5*: the licence is answerable, see the next row |
| `deny.toml` allow-list | MIT, Unlicense, Apache-2.0, Zlib, 0BSD — **no BSD-3-Clause** | `deny.toml:13-19` | *scheduled 5*: shipping CMaps needs that entry plus the notice, or the data lives outside the crate; recorded as a fact, not fixed here |
| Tier A corpus | pdf.js `test/pdfs` at commit `d60bc7426aa0`, **Apache-2.0**, 25 files with SHA-256s | `corpus/manifest.json`, `corpus/README.md`, `corpus/LICENSES/Apache-2.0.txt` | the corpus row |
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

Checked 2026-11-17 — `cargo test --workspace` (including `tests/edit.rs`, which asserts the incremental update and drives `pdftoppm`/`mutool` over a saved file when they are installed); `cargo run -p pdf2-cli -- info tests/fixtures/classic-xref.pdf` (incremental update) and `… info corpus/tier-a/bug_jpx.pdf` (the one committed file that declares `/Linearized`).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Classic xref tables, `/Prev` chains, incremental updates (read) | shipped | `src/document.rs`, `tests/document.rs` |
| Cross-reference streams, `/ObjStm`, hybrid files | shipped | `src/document.rs`, `tests/xref_streams.rs` |
| Object streams at scale (65k objects, budgeted time) | shipped | `crates/pdf2-bench/src/corpus.rs:91` builds the 65 000-object document and `:195` asserts its size, `docs/performance-baseline.json` gates its throughput (`objects-65000`, 347.5 MB/s in `docs/PERFORMANCE.md`), and the parser refuses past `DEFAULT_OBJECT_LIMIT` rather than growing without bound (`src/document.rs:37,344`). Owner: item 8 (shipped 2026-11-16) |
| Damaged files: repair/rebuild policy beyond error reporting | unplanned | We report and stop. Demand unmeasured; a `--repair` mode would need a corpus of damaged files first. |
| Linearised files (read) | shipped | `corpus/tier-a/bug_jpx.pdf` declares `/Linearized` and the CLI reads it (`pdf2 check`, 2026-11-15); linearisation is an ordering, so the parser reads it by construction |
| Linearisation (write) | unplanned | No demand evidence; `qpdf` owns the use case |
| Incremental update (write) | shipped | `MutableDocument::save_incremental` (`src/edit.rs`): append, one classic xref section with `f` rows for deletions, trailer with `/Prev`/`/Size`/`/ID` (first string frozen), encrypted with the base's handler. Proved by `tests/edit.rs` — the round trip, the trailer to `14.4`, encryption on all three fixtures, and `pdftoppm`/`mutool` rendering the appended page (2026-11-17). Owner: item 11 |
| `7.9` Common data structures — dates, names, strings, rectangles | shipped, partial | `Rect` and the name/string objects are modelled (`src/object.rs`); **dates are not**: no `Date` type exists, so `/CreationDate` and `/ModDate` are unread strings |
| `7.10` Functions (types 0–4) | shipped (partial) | `crates/pdf2-render/src/function.rs` parses and evaluates **type 0** (sampled, bit-unpacked, multilinear), **type 2** (exponential), **type 3** (stitching, with `/Bounds` and `/Encode`) and **type 4** (the PostScript calculator: Table 42's operators, `if`/`ifelse` over procs, bounded to 4096 steps and 128 stack entries). Type 1 is refused by name as the deprecated form. Every `/Function` in the five shading pages parses and evaluates (`function.rs` tests); **nothing paints with them yet** — `sh` is item 10's next child. Owner: item 10 (functions, 2026-11-16) |
| `7.10` Functions used for anything else | scheduled 10 | Nothing evaluates PDF functions; shadings cannot be drawn without them, so the row rides with item 10. Owner: item 10 |
| `7.11` Embedded files and file specifications (`/EmbeddedFiles`, `/Filespec`) | unplanned | No `EmbeddedFiles` or `/Filespec` handling anywhere; attachments are invisible. Owner: none |
| `7.12` Extensions dictionary | unplanned | No `Extensions` handling; extension levels are ignored rather than reported. Owner: none |
| `7.5.8` Cross-reference generation numbers (the 1.x generation edge) | shipped | A free entry's generation above 16 bits is clamped (the free-list head is never used for a lookup): `corpus/tier-a/ContentStreamCycleType3insideType3.pdf` and `tests/fixtures/xref-generation.pdf` both read, the latter pinned by `tests/xref_streams.rs`. Owner: item 6 (shipped 2026-11-15) |

## Filters and images

Checked 2026-11-15 — `cargo test -p pdf2 --test filters`; the four image filters are refused by `src/filter.rs` and the corpus rows show the result (`cmykjpeg.pdf`, `bug_jpx.pdf`, `ccitt_EndOfBlock_false.pdf` render blank).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| FlateDecode (+ PNG/TIFF predictors), LZW, ASCIIHex, ASCII85, RunLength | shipped | `src/filter.rs`, `tests/filters.rs` |
| `/CCITTFaxDecode`, one-dimensional (`7.4.6`) | shipped | `src/fax.rs`: a bit reader, the full `/DecodeParms` set and the T.4 one-dimensional tables, decoding to one-bit rows that `image.rs` widens. Owner: item 9 (shipped 2026-11-16) |
| `/CCITTFaxDecode`, two-dimensional (G3 2-D and G4) | shipped | The changing-element model of T.4 §4.2.1.2: pass, horizontal and vertical ±3 modes with the reference line tracked as `a0` advances, and a backward vertical that walks the change list back. G3 mixed mode reads the tag bit after an EOL and looks for the EOL even when `/EndOfLine` is absent. `ccitt_EndOfBlock_false.pdf` draws all six of its images and reports every operator supported (`crates/pdf2-render/tests/fax.rs`). Owner: item 9 (2026-11-16) |
| `/DCTDecode` (JPEG), 1- and 3-component | shipped | `zune-jpeg` 0.5.15 behind the `dct` feature (on by default): `Image::from_dct` decodes, checks the header's size against the budget and honours `/Decode`. `alphatrans.pdf`'s photograph draws (`crates/pdf2-render/tests/dct.rs`); costs +66.8 KB gzip on the npm bundle, recorded in `docs/PERFORMANCE.md`. Owner: item 9 (DCT half, shipped 2026-11-16) |
| `/DCTDecode` CMYK images | shipped | Four-component JPEG: decoded as CMYK (the RGB path assumes uninverted samples), the Adobe APP14 inversion undone unless `/Decode` states it, then converted multiplicatively. `cmykjpeg.pdf` draws its photograph and `tests/fixtures/cmyk-jpeg.pdf` pins four ink quadrants to ±8 (`crates/pdf2-render/tests/dct.rs`); against an independent decoder the page matches to 8 mean absolute units. Owner: item 9 (2026-11-16) |
| `/JPXDecode` (JPEG 2000) | shipped | `hayro-jpeg2000` 0.5.0 behind the **`jpx` feature, off by default** (**+78 174 gzip** on the 1.8.0 tree; ADR 0007 keeps it out of the npm bundle and in the demo's: 365 665 → 506 424). The codestream header decides size, colour space and depth (`/ColorSpace` optional, `/BitsPerComponent` ignored), the size is checked against `decoded_limit()` before decoding, and alpha/`Icc`/`Unknown` are refused by name. `bug_jpx.pdf` draws (8 192 pixels), joins `corpus/oracle-cases.txt` at 0.0008 over a 0.0100 budget, and poppler segfaults on it, so mupdf and Ghostscript are its engines. Tests: `crates/pdf2-render/tests/jpx.rs`. Owner: item 9 (shipped 2026-11-17) |
| `/CCITTFaxDecode` | shipped | `ccitt_EndOfBlock_false.pdf` |
| `/JBIG2Decode` | shipped | `hayro-jbig2` 0.4.0 behind the **`jbig2` feature, off by default** (**+64 583 gzip** on the 1.8.0 tree). `/DecodeParms /JBIG2Globals` is decoded before the page's own segments, the sink writes 1-bit rows with 1 for white, and both pdf.js pages (`bitmap-composite-and-xnor.pdf`, `bitmap-p32-eof.pdf`) draw identical rasters — 11 315 ink on 148 285 white. **Gated by the oracle** at 0.243040 mean against the engines' 0.326558 spread (4×4) and 0.521491 against 0.528947 (1×1), with a 0.7 AA ceiling and an 80 ms budget. The 31.261 that kept them out was this renderer's row-stride bug, found against pdfium's reference raster and fixed on 2026-11-17 (`docs/RENDERING-QUALITY.md`). Tests: `crates/pdf2-render/tests/jbig2.rs`. Owner: item 9 (shipped 2026-11-17) |
| Images above 8 bits per component | unplanned | `crates/pdf2-render/src/image.rs` accepts 8 only; rare in practice, needs a corpus count first |
| `/SMask` (soft mask) | shipped (partial) | `/S /Luminosity` is rendered from its `/G` onto black and multiplied into the coverage mask (`ClipMask::multiply_luminosity`); `/SMask /None` is not a mask, and `/S /Alpha` keeps the `gs` note because this rasteriser has no transparent canvas. `tests/fixtures/soft-mask.pdf` (page 1 masked, page 2 refused) and `tests/ext_gstate.rs`. Owner: item 10e (partial 2026-11-16) |
| `/Mask`, colour key (`8.9.6.4`) | shipped | The key is tested on the **raw** samples before `/Decode`, and masked pixels are skipped so the background shows through; the ranges are inclusive, which the corpus's own `min == max` key requires. `/DCTDecode` with a key and a stream `/Mask` (a stencil image, `8.9.6.3`) are refused by name. `colorkeymask.pdf` paints its cell through its key and joined `corpus/oracle-cases.txt` (0.0006 over tolerance, 0.0100 budget); `tests/fixtures/colour-key.pdf` pins the green-through-the-key and the boundary case (`crates/pdf2-render/tests/colour_key.rs`). Owner: item 10 (shipped 2026-11-17) |
| `/Interpolate` beyond bilinear | unplanned | Nearest and bilinear are both implemented; other filters are viewer-defined |

## Colour

Checked 2026-11-15 — `cargo test -p pdf2-render` (calibration page 1 golden) and the oracle gate's calibrated comparison of that page (widest spread: 8.0%).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| DeviceGray, DeviceRGB, DeviceCMYK (naive), `/ICCBased` by component count | shipped | `src/content.rs`, `crates/pdf2-render/src/image.rs` |
| ICC colour management (profiles actually applied) | scheduled 10b | The oracle gate measures an **8% spread** on the CMYK page; the oracles disagree because they manage colour. The dependency question closed 2026-11-17: `moxcms` 0.9.2 needs 1.89 (inside the floor), is `BSD-3-Clause OR Apache-2.0` (allowed), and is pure Rust — what remains is the fidelity decision and a size measurement. Owner: item 10b |
| `8.6.6.4` `/Separation`, `/DeviceN`, spot colours — and `10.8` rendering for separations | shipped (partial) | Both spaces are read wherever a shading, an image or the content stream's `cs`/`scn` names one: the tint runs through the space's own `/Function` into the alternate space (`crates/pdf2-render/src/shading.rs`, `tests/fixtures/separation.pdf`, `function_based_shading_cmyk.pdf`). `/DeviceN` is unit-tested against a two-colourant space because no corpus page carries one. What `10.8` still owes is a *separation device* (ink-limited output), which is print-only and deferred with ICC — `docs/DEFERRED.md`. Owner: item 10 (2026-11-16) |

## Fonts and text

Checked 2026-11-15 — `cargo test -p pdf2-render` (font unit tests, pages 9–11 goldens) and `cargo run -q -p xtask -- corpus` (22 of 23 render, 15 fully readable; the 7 composite-font files draw).

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Type 3 fonts | shipped | Each glyph in `/CharProcs` is a content stream run in glyph space (`/FontMatrix` × the text rendering matrix) with the font's own `/Resources`; codes reach glyph names through `/Encoding`'s `/Differences` (a named base encoding alone does not provide names this renderer has, and is reported). `d0`/`d1` are read, the advance comes from `/Widths`, and a glyph that shows text in its own font stops at `MAX_FORM_DEPTH`. `Type3WordSpacing.pdf` and `ContentStreamCycleType3insideType3.pdf` draw and report every operator supported (`crates/pdf2-render/tests/type3.rs`). Owner: item 4 (2026-11-16) |
| Robustness at scale: object and page ceilings | shipped | `Document::object_limit`/`set_object_limit` (default `DEFAULT_OBJECT_LIMIT`, 1 048 576) refuses reading past the number of objects held with `Error::TooMany`; `Document::page_limit`/`set_page_limit` (default `DEFAULT_PAGE_LIMIT`, 131 072) bounds every page-tree walk the same way, on top of `MAX_PAGE_TREE_DEPTH`. The budget file is generated by `crates/pdf2-bench` (65 000 objects) and its parse cost is in `docs/performance-baseline.json`; `tests/bounds.rs` proves both ceilings fire and that raising them lets the same file through. Owner: item 8 (2026-11-16) |
| Simple fonts: Type 1, TrueType with `/FontFile2`, encodings and `/Differences` | shipped | `crates/pdf2-render/src/font.rs`, page 9 of `calibration.pdf` |
| Base-14 without a program: built-in 5×7 fallback with composed Latin-1 accents | shipped | `crates/pdf2-render/src/font.rs` |
| Composite fonts (Type 0/CID) **rendering** with Identity-H and `/CIDToGIDMap` | shipped | pages 10–11, `cid_cff.pdf` renders |
| Composite fonts **extraction** (`/ToUnicode`, code splitting, CID widths) | shipped | `src/content.rs` reads Type0 with an Identity encoding: codes split by the CMap's code space, text from `bfchar`/`bfrange` (`src/cmap.rs`), advances from `/W`+`/DW` so a gap becomes a space. Five corpus files moved from reported to readable (`cargo run -q -p xtask -- corpus`: 20 of 23), `tests/fixtures/tounicode.pdf` extracts `ADabcxyz A` (`tests/content.rs`). Owner: item 2 (shipped 2026-11-15) |
| Predefined CMaps (CJK) | partial | One is shipped as data: Adobe-Japan1 `90ms-RKSJ-H` (code → text, 7 876 mappings, 19 967 B, generated by `cargo xtask cmaps` from the BSD-3-Clause resources in `vendor/adobe-cmaps/`). `90ms_rksj_h_sample.pdf` extracts `日本語テスト` (`tests/content.rs`), and the size is recorded against the wasm budget in `docs/PERFORMANCE.md`. Other CMaps are data additions. Owner: item 5 (partial 2026-11-15) |
| Predefined CMaps (other than Japan1 `90ms-RKSJ-H`) | scheduled 5 | Needs ~250 KB of data. The licence question is answered in §Outside facts: Adobe's CMaps are **BSD-3-Clause**, `deny.toml` does not allow it, so the allow-list entry (and the notice) or a home outside the crate is a prerequisite, not an afterthought |
| CFF program parsing and outlines | shipped | `/CIDFontType0C` is wrapped in a five-table OpenType container and read by `ttf-parser`; the charset is inverted for CID-keyed fonts. `cid_cff.pdf` and `Embedded_font.pdf` draw their outlines and no longer report `font:CIDFontType0` (`crates/pdf2-render/tests/cff.rs`). A **simple** (non-CID) `/Type1C` font selects glyphs by the charset's **names**: the CID-keyed test is the Top DICT's ROS, so a name-keyed font is not read as CIDs and each name comes back to a character. `ShowText-ShadingPattern.pdf` draws its text and stopped reporting `font:FontFile3` (`crates/pdf2-render/tests/cff.rs`). `/FontFile` Type 1 is read by `crates/pdf2-render/src/type1.rs` — `eexec`, the PFB/PFA containers, `/Subrs`, `/CharStrings` and the charstring operators — proven by the generated `tests/fixtures/type1.pdf` (`crates/pdf2-render/tests/type1.rs`). `seac` and `callothersubr` are refused by name. Owner: item 4 (shipped 2026-11-16) |
| `/FontFile` Type 1 (PFB/PFA) programs | shipped | `crates/pdf2-render/src/type1.rs` reads `eexec`, the PFA/PFB containers, `/Subrs` and `/CharStrings`, and `Font::Type1` (`crates/pdf2-render/src/font.rs:60`) draws them through the same outline path as CFF (`crates/pdf2-render/src/walk.rs:1945-1954`); proved by `crates/pdf2-render/tests/type1.rs` and the generated `tests/fixtures/type1.pdf`. `seac` and `callothersubr` are refused by name. Owner: item 4 (shipped 2026-11-16) |
| Text render modes (`Tr`, `9.3.6` Table 104) | shipped | Modes 0–7 all paint as Table 104 says; 4–7 also add the glyph's polygons to the current clip under the non-zero winding rule (`8.5.3.3`), accumulating glyph by glyph in the graphics state. Mode 7 used to share mode 3's early return, so it neither painted nor clipped — it clips now. A Type 3 glyph has no outline, so modes 4–7 report `text-clip` instead of clipping to an empty mask. `tests/fixtures/tr-clip.pdf` and `tr-clip-type3.pdf` (`crates/pdf2-render/tests/tr_clip.rs`, `tr_clip_type3.rs`). Owner: item 10 (shipped 2026-11-17) |
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
| `/Separation` and `/DeviceN` colour spaces | shipped (partial) | Resolved where a shading or an image names one: the tint runs through the space's own `/Function` into the alternate space (`crates/pdf2-render/src/shading.rs`, `tests/fixtures/separation.pdf`). `function_based_shading_cmyk.pdf`'s `/Spot` gradient draws. `cs`/`scn` in the content stream are read (below). Owner: item 10 (colour spaces, 2026-11-16) |
| `cs`/`scn` colour operators and `/PatternType 2` | shipped (partial) | `ContentStreamCycle…`'s sibling in the corpus — `ShowText-ShadingPattern.pdf` — names a `/Pattern` space and a shading pattern; the walker selects the space, and a `scn` naming a `/PatternType 2` fills the path with its shading (the path becomes the clip). That page now reports only its simple-CFF font. Tiling patterns (`/PatternType 1`) are drawn — see the row below. Owner: item 10 (2026-11-16) |
| `/PatternType 1` tiling patterns (`8.7.3`) | shipped | The cell's `/BBox` clips each copy, `/XStep`/`/YStep` are the lattice (a zero step is reported), `/PaintType 1` paints the cell's own content and `/PaintType 2` is a stencil poured with the colour `scn` gave it — a colour operator inside such a cell is reported, not obeyed. The cell runs with the parent stream's **initial** state and its own `/Resources`, and `MAX_TILE_CELLS` bounds the lattice. `colorkeymask.pdf` and `ContentStreamCycleType3insideType3.pdf` paint their tiles, and `tests/fixtures/tiling.pdf` pins four cells plus the over-budget page (`crates/pdf2-render/tests/tiling.rs`). `/TilingType` 1/3 device-pixel spacing is recorded as a later decision. Owner: item 10 (shipped 2026-11-17) |
| Shadings (`sh`, types 1–3) | shipped (partial) | `crates/pdf2-render/src/shading.rs` samples axial, radial and function-based shadings per pixel through `/Function`, honouring `/Extend`, `/Matrix` and the shading's colour space; `axial_shading_many_inputs.pdf` and `function_based_shading*.pdf` draw with no `sh` note, and `tests/fixtures/shading.pdf` pins both gradients by formula (`tests/shading.rs`). Mesh shadings (types 4–7) and tiling patterns are still reported. Owner: item 10 (shadings half, 2026-11-16) |
| Shadings and patterns | scheduled 10 | `ShowText-ShadingPattern.pdf`, `axial_shading_many_inputs.pdf` render with skips |
| Transparency groups, soft masks, blend modes (`11`) | scheduled 10 | `alphatrans.pdf`, `blendmode.pdf` |
| `8.10` **Form XObjects** (`/Subtype /Form`) | shipped | `crates/pdf2-render/src/walk.rs:1152` runs a `/Subtype /Form` XObject as a nested content stream: its `/BBox` clips, its `/Matrix` composes with the caller's, its own `/Resources` are used, and `MAX_FORM_DEPTH` bounds the nesting. Owner: item 10d (shipped 2026-11-16) |
| `/ExtGState`: `/ca`, `/CA`, `/BM`, `/SMask` (`8.4.5`) | shipped (partial) | Constant alpha (`/ca`, `/CA`), `/BM /Normal` and the line state (`/LW`, `/LC`, `/LJ`, `/ML`, `/D`) are honoured and carried by `q`/`Q`; `/SMask` and and all sixteen `/BM` blend modes are applied with the CSS formulae (separable per channel, the four non-separable ones over the triplet) — `tests/fixtures/blend-modes.pdf` and `tests/blend_modes.rs` check six of them by arithmetic. `tests/fixtures/alpha.pdf` (arithmetic: 50 % red over white is `(255,128,128)`) and `tests/ext_gstate.rs`; `alphatrans.pdf` lost its `gs` note. Owner: item 10e (partial 2026-11-16) |
| Annotation appearance streams (`/Annots → /AP → /N`) | shipped | `Walker::draw_annotations` (`crates/pdf2-render/src/walk.rs`) draws each annotation's normal appearance after the page content, mapping the transformed `/BBox` onto `/Rect`, selecting `/AS` from a state dictionary and skipping invisible/hidden/no-view flags. `tests/fixtures/annotations.pdf` (one rule per annotation) and `tests/annotations.rs`; the fixture joins `corpus/oracle-cases.txt`. Owner: item 12b (shipped 2026-11-16) |
| `8.11` **Optional content** (layers, `/OCProperties`) | unplanned | No `OCProperties` or `OCG` handling exists anywhere in `src/` or `crates/`; layer visibility is not modelled, so a page with layers draws every layer or none. Owner: item 10d — the roadmap's revision assigned it after the ledger's decision 8 named it unowned. |
| `10.5` **Transfer functions** and `10.6` **Halftones** | deferred | Neither exists: `TransferFunction` and `Halftone` appear nowhere in `src/` or `crates/`. For screen output both are largely moot; for print fidelity they are not, and `10.8` separations need `10.5`. Owner: none (print-only, no demand evidence) |
| Decoded-stream budget (`7.4`) | shipped | `filter::DEFAULT_DECODE_LIMIT` is 256 MiB (mupdf's `FZ_STORE_DEFAULT`), settable with `Document::set_decoded_limit`; `Error::Limit` names the budget. Flate is capped during inflation, so a 1 MiB bomb is refused rather than allocated (`tests/bounds.rs`, `src/filter.rs` tests). An image is measured from its `/Width`, `/Height` and colour space **before** its stream is decoded, so a lying `/Width` never allocates (`crates/pdf2-render/tests/bounds.rs`). Owner: the bounds item (shipped 2026-11-16) |
| Glyph cache (`10.7`) | shipped | A per-font cache of character → glyph id and glyph id → outline, shared by clones (`crates/pdf2-render/src/font.rs`); `book.pdf` at scale 0.25 goes 5.4 → 2.5 ms (`docs/RENDERING-QUALITY.md`). Owner: item 10c (shipped 2026-11-15) |
| Report honesty: `drew-nothing`, `font:TrueType-without-cmap` | shipped | A page that ran painting operators and left an empty raster reports `drew-nothing`; a simple TrueType program with no `cmap` is named instead of guessed. `crates/pdf2-render/tests/quality.rs`, fixtures `invisible-ink.pdf` and `blank-page.pdf`. Owner: item 10g's honesty half (shipped 2026-11-15) |
| Full-resolution oracle measurement (`--blocks 1`) | shipped | The oracle tool writes `ours_over`/`ours_mean` beside the inter-oracle spread, and the workflow takes `blocks`/`summary` inputs; `corpus/oracle-summary-1x1.csv` is the committed 14-case baseline (`docs/RENDERING-QUALITY.md`). Owner: the anti-aliasing item's measurement half (shipped 2026-11-16) |
| Anti-aliasing (`10.7`) | shipped | `fill_polygons` samples four sub-scanlines per row with exact horizontal coverage (`crates/pdf2-render/src/raster.rs`), so edges fade instead of stepping. Judged by `corpus/aa-ceilings.tsv`: at 1×1 `book#1` measures 2.58 mean with AA and 3.68 without, and the ceiling sits between them, gated by the `oracle` job. `docs/RENDERING-QUALITY.md` has the before/after table. Owner: item 10g (shipped 2026-11-16) |ixel centres, with no anti-aliasing (`crates/pdf2-render/src/raster.rs:108`, `docs/RENDERING.md`), which is why the oracle gate compares at 4×4 blocks. The speed half has no item either: ADR non-goal 1 refuses *competing* on speed, which is not the same as having no target; Owner: none for anti-aliasing; the *speed* half is item 10c below. |
| Render-time budget in the oracle job | shipped (partial) | `crates/pdf2-oracle` times each case's render, prints `ms` and gates it against `corpus/render-budget.tsv` (twice the measured time, 25 ms floor; the `oracle` job fails a case with `SLOW`), set from a measured run on `ubuntu-latest` (`docs/PERFORMANCE.md` §Render time). Anti-aliasing and the glyph cache both shipped after this row was written; what rank 10c still lacks is a **page-count** timing — no 200-page document is timed. Owner: item 10c |
| Anti-aliasing and a glyph cache | shipped | `fill_polygons` samples four sub-scanlines with exact horizontal coverage and blends once per pixel (`AA_SUBROWS`), so edges are covered, not hard; goldens were re-blessed with the before/after written in `docs/RENDERING-QUALITY.md` (1×1 `book#1` mean 3.68 → 2.58). Glyph outlines are cached per code (`GlyphCache`). The oracle gates both: 4×4 blocks plus `corpus/aa-ceilings.tsv` at 1×1. Owner: item 10g (shipped 2026-11-16) |
| Render-time budget (no AA, no glyph cache, no time gate) | refused | superseded by the shipped budget row above; the original text is kept for the roadmap's history |age; the roadmap's item 10c owns the budget. Owner: item 10c |

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
| `12.7` Forms — read, render, **fill** | scheduled 12 | Filling **and authoring** are both scheduled since 2026-11-17 ([plan](plans/forms.md)); the ADR's non-goal 3 refuses JavaScript, not writing `/V`. The ADR says so |
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
| `14.4` File identifiers (`/ID`) | shipped (partial) | An incremental save reads the base trailer's `/ID`, freezes its first string and writes a deterministic second one derived from what the save appended (`src/edit.rs:373-378`, ADR 0006 §5), proved by `tests/edit.rs`. A document built with `Document::new` still emits no `/ID`. Owner: item 11 (shipped 2026-11-17) |
| `14.5` Page-piece dictionaries | unplanned | Private producer data; not retained, and the writer does not carry unknown page keys forward |
| `14.6` Marked content — `BMC`/`BDC`/`EMC`, `/ActualText`, `/Artifact` | shipped (partial) | `BMC`/`BDC`/`EMC` are tracked as nested sequences and a `BDC` carrying `/ActualText` replaces what the codes say in extraction (`src/content.rs:400-738`). `/Artifact` is **not** distinguished, so a header or footer marked as an artefact is still extracted as text — that is the remaining half. Owner: item 2b |
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
| Encryption on write | shipped (partial) | An incremental save of an encrypted document re-encrypts every appended object with the base's handler — RC4, AES-128 and AES-256 — with the IV derived from the key, the object number and the payload (ADR 0006 §5, `tests/edit.rs`). A document built with `Document::new` is still written unencrypted. Owner: item 11 |
| `/Crypt` filters and per-stream encryption | partial | The `/CF` crypt filters of `/V 4` and `/V 5` are read for their method (`/V2`, `/AESV2`, `/AESV3`, `/None`), `/Identity` is honoured, and a file naming different filters for strings and streams is refused rather than guessed at; per-stream `/Crypt` filters remain unplanned |

## API, tooling, proof

Checked 2026-11-17 — `cargo test --workspace` (editing's 16 tests in `tests/edit.rs`); `cargo run -p pdf2-cli -- info|pages|text|check`; `cargo run -q -p xtask -- wasm-package` (API smoke test and size budget: 206 823 B gzip of 266 240); `cargo run -q -p xtask -- corpus`; five fuzz targets, last nightly run green 2026-11-15; the oracle gate's last run green 2026-11-15.

| Feature | Status | Evidence / owner |
| --- | --- | --- |
| `Document`, `Page` model, lazy objects, caching, `Result`-only errors | shipped | `src/` |
| Editing API (mutate an opened document) | shipped | `MutableDocument` (`src/edit.rs`) overlays a `Document` — `set_object`/`add_object`/`delete_object`/`add_page`, reads through the overlay, the base untouched — and saves incrementally per [ADR 0006](adr/0006-editing-mutation-model.md). `tests/edit.rs` (16 tests): overlay reads, the page tree, the `14.4` trailer, the write-side ceiling, and the round trip on plain and encrypted files, with poppler and mupdf rendering the appended page (2026-11-17). Owner: item 11 |
| CLI: `info`, `pages`, `text`, `check`, `--json`, `--password` | shipped | `crates/pdf2-cli` |
| CLI `render` subcommand | unplanned | The corpus run uses the renderer's golden binary; a batch render was not needed yet |
| wasm: open, page count/size, text, metadata, render to pixels/canvas, render report | shipped | `crates/pdf2-wasm` |
| wasm/npm API growth (spans, page boxes, structured errors) | shipped (partial) | `crates/pdf2-wasm/src/lib.rs` exposes `spans`, `span_texts`, `glyphs`, `glyph_texts`, `page_size`, `search`, `search_texts`, `search_matches`, `metadata` and the render calls. Structured errors are the gap: every failure crosses the boundary as a `JsError` message string. Owner: item 2b |
| Fixtures with arithmetic (calibration, book) | shipped | `tests/fixtures`, `docs/RENDERING.md` |
| Tier B corpus in CI (pdf.js full tree, veraPDF) | shipped | `.github/workflows/tier-b.yml` (weekly + dispatch with a file cap) fetches pdf.js `test/pdfs` at `d60bc7426aa0` (Apache-2.0) and the veraPDF corpus (CC BY 4.0), and `cargo run -q -p xtask -- tier-b <dir>…` publishes `tier-b-report.md` — files, readable share, failure classes with an example each. First measured sample: **184 of 200 pdf.js files readable (92.0%)**, `docs/CORPUS.md` §Tier B. Owner: item 7 (shipped 2026-11-15) |
| Tier A corpus, oracle gate with calibrated tolerances | shipped | `corpus/` (25 files, hashed), `corpus/tolerances.tsv`, `crates/pdf2-oracle`, the `oracle` job; last run green 2026-11-15. Owner: item 1 (shipped). The tolerances are only comparable on the engines recorded in §Outside facts (poppler 26.01.0, mupdf 1.27.0, ghostscript 10.06.0) |
| Tier B histogram as a gate | refused | It publishes a distribution and never fails a merge: a tree of other people's broken files is evidence, not a test suite (`docs/CORPUS.md` §Tier B). Owner: none — item 7 keeps it a measurement |
| Fuzzing (five targets, nightly), deny, semver, bench gates | shipped | `fuzz/fuzz_targets` (5), `.github/workflows/{fuzz,ci}.yml`; last nightly fuzz run green 2026-11-15 |
| Validation (PDF/A, PDF 2.0 conformance) | scheduled 13 | **Scheduled 2026-11-17**: `check --strict` first (seven structural facts the parser already computes), then the profile clauses per part, with **veraPDF 1.30 as the CI oracle** over a subset of its CC BY 4.0 corpus — the relationship poppler has with the renderer. [plan](plans/validation.md). Owner: item 13 keeps it unscheduled. Owner: none — roadmap item 13 keeps it unscheduled |

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
