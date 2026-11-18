# The objectives ledger: what is open, where it is owned, what it costs

This is the inventory the ranking in this file's next pass rests on, and the file the next
objective is cut from. It is generated from the repository, not from memory: every row carries the
`file:line` it came from, every demand number a command, and the counts are quoted beside the command
that produced them so a later round can diff them instead of re-reading the whole backlog.

Regenerate the sources with:

```sh
cargo run -q -p xtask -- ledger      # 114 rows, 21 roadmap items
grep -c '^|' docs/FEATURES.md        # 161 table lines (117 status rows once headers and separators are out)
ls docs/plans/*.md                   # 11 files: 10 plans + README
```

## Counts, as of this round

`cargo run -q -p xtask -- ledger` prints:

```text
ledger: 114 rows, 21 roadmap items, every item owned and every shipped row evidenced
```

`docs/FEATURES.md` holds **117 status rows**: 59 `shipped`, 2 `shipped (partial)`, 5 `not applicable`,
1 `deferred`, 8 `refused`, **14 `scheduled`** and **28 `unplanned`** — the 42 open ones are listed below.
`docs/ROADMAP-NEXT.md` carries 21 ranks, of which **8 are open** (`4, 5, 9, 10, 10b, 10c, 12, 13`) and 13
are marked shipped. `docs/DEFERRED.md` holds **4 open items** (ICC, mesh shadings, forms filling, validation), 5 refusals
with an ADR line, and **3 rows that still read as deferred although the code shipped them** (tiling
patterns, colour-key `/Mask`, the `Tr` 4-7 clipping half — `tests/tiling.rs`, `tests/colour_key.rs`,
`tests/tr_clip.rs`, `tests/tr_clip_type3.rs`), which the corrections queue below picks up. `docs/plans/` holds 10 plans and a template readme.

## Open rows in `docs/FEATURES.md` (42)

| line | item | status | note |
| --- | --- | --- | --- |
| 71 | Object streams at scale (65k objects, budgeted time) | scheduled 8 | rank 8 is shipped in `ROADMAP-NEXT.md` — Q2 correction |
| 72 | Damaged files: repair/rebuild policy beyond error reporting | unplanned |  |
| 74 | Linearisation (write) | unplanned |  |
| 78 | 7.10 Functions used for anything else | scheduled 10 |  |
| 79 | 7.11 Embedded files and file specifications (/EmbeddedFiles, /Filespec) | unplanned |  |
| 80 | 7.12 Extensions dictionary | unplanned |  |
| 97 | Images above 8 bits per component | unplanned |  |
| 100 | /Interpolate beyond bilinear | unplanned |  |
| 109 | ICC colour management (profiles actually applied) | scheduled 10b |  |
| 125 | Predefined CMaps (other than Japan1 90ms-RKSJ-H) | scheduled 5 |  |
| 127 | /FontFile Type 1 (PFB/PFA) programs | scheduled 4 | code ships it (`crates/pdf2-render/src/type1.rs`) — Q2 correction |
| 129 | Type 3 fonts | scheduled 10 |  |
| 130 | Vertical writing mode | unplanned |  |
| 148 | Shadings and patterns | scheduled 10 |  |
| 149 | Transparency groups, soft masks, blend modes (11) | scheduled 10 |  |
| 150 | 8.10 Form XObjects (/Subtype /Form) | scheduled 10d | code ships it (`crates/pdf2-render/src/walk.rs:1152`) — Q2 correction |
| 153 | 8.11 Optional content (layers, /OCProperties) | unplanned |  |
| 180 | 12.2 Viewer preferences | unplanned |  |
| 181 | 12.3 Document-level navigation — outlines, destinations, /OpenAction | unplanned |  |
| 182 | 12.4 Page-level navigation — link annotations and /Dest | scheduled 12 |  |
| 183 | 12.5 Annotations — read, render, appearance streams | scheduled 12 |  |
| 184 | 12.6 Actions — /GoTo, /URI, JavaScript | unplanned |  |
| 185 | 12.7 Forms — read, render, fill | scheduled 12 |  |
| 187 | 12.9 Measurement properties (PDF 2.0) | unplanned |  |
| 188 | 12.10 Geospatial features (PDF 2.0) | unplanned |  |
| 189 | 12.11 Document requirements (PDF 2.0) | unplanned |  |
| 197 | 13.2 Multimedia, 13.7 Rich media | unplanned |  |
| 198 | 13.3 Sounds | unplanned |  |
| 199 | 13.4 Movies | unplanned |  |
| 200 | 13.5 Alternate presentations | unplanned |  |
| 210 | 14.4 File identifiers (/ID) | unplanned | code writes it (`src/edit.rs`) — Q2 correction |
| 211 | 14.5 Page-piece dictionaries | unplanned |  |
| 212 | 14.6 Marked content — BMC/BDC/EMC, /ActualText, /Artifact | unplanned | code reads it (`src/content.rs`) — Q2 correction |
| 215 | 14.11 Prepress — output intents, separations | unplanned |  |
| 216 | 14.12 Document parts | unplanned |  |
| 217 | 14.13 Associated files | scheduled 12 |  |
| 231 | O Fragment identifiers | unplanned |  |
| 232 | Q Method for determining transparency on a page | unplanned |  |
| 242 | Encryption on write | unplanned | incremental save carries encryption (`src/edit.rs`); what a new document cannot do needs a check in Q2 |
| 254 | CLI render subcommand | unplanned |  |
| 256 | wasm/npm API growth (spans, page boxes, structured errors) | unplanned | `spans`, `spansTexts` and `glyphs` exist in `crates/pdf2-wasm/src/lib.rs`; page boxes exist, structured errors do not — narrow in Q2 |
| 262 | Validation (PDF/A, PDF 2.0 conformance) | scheduled 13 |  |

## Open ranks in `docs/ROADMAP-NEXT.md` (8)

The roadmap's own columns are kept, so an objective's effort and its proof strategy can be read without
opening the file. Ranks marked **✓** there are shipped and are not repeated here.

| line | rank | what remains | effort | proof (done when) | depends on |
| --- | --- | --- | --- | --- | --- |
| 38 | 4 | CFF / Type 1 outlines — CFF shipped 2026-11-15, Type 1 (/FontFile) still scheduled | 2 | No font:FontFile report on the corpus CFF files, and their pages pass the oracle check from item 1 | 1 (to verify), D2's dependency verdict |
| 39 | 5 | Predefined CMaps and CJK text — one CMap (90ms-RKSJ-H) shipped 2026-11-15, the rest data work | 2 | The CJK sample extracts its text; the shipped data is measured against the wasm budget | 2, and the licence first: Adobe's CMaps are BSD-3-Clause and deny.toml allows no BSD licence (docs/FEATURES.md §Outside facts), so the allow-list entry and notice — or a home outside the crate — come before the data |
| 45 | 9 | Image decoders behind features — DCT (RGB and CMYK) and CCITT 1-D/2-D shipped; JPEG 2000 and JBIG2 unblocked 2026-11-17 by the 1.100 floor and now a s | 3 | Those four pages have ink and pass the oracle check, with the budget decision recorded — raise the npm budget to ~499 KiB, or ship behind a non-defaul | 1 (to verify) |
| 46 | 10 | Painting completeness — shadings, functions, /Separation, /DeviceN, /PatternType 2, cs/scn, Type 3 and CFF fonts shipped 2026-11-16; tiling patterns a | 7 | Oracle diff on those corpus pages; docs/RENDERING.md's "still unproven" list shrinks to /UserUnit and >8-bit images | 1 |
| 47 | 10b | Colour management (ICC) — deferred: a fidelity decision, and since 2026-11-17 no longer a dependency one — moxcms 0.9.2 (MSRV 1.89, BSD-3-Clause OR Ap | 4 | Our distance to the median oracle on the colour page drops toward the inter-oracle spread, with the before/after measured | 1, 10 |
| 52 | 10c | Renderer performance budget — a per-page time budget gates the oracle job and a glyph cache exists; no 200-page document is timed yet | 2 | The oracle job records render time per page and fails above a committed budget; a 200-page document stays within it | 1 |
| 54 | 12 | Annotations and forms — reading and rendering appearance streams shipped (12b); filling and authoring scheduled 2026-11-17 ([plan](plans/forms.md)) (r | 5 | An annotated corpus page renders its appearance streams; a form field can be set and re-read | 11 for filling |
| 55 | 13 | Conformance validation — scheduled 2026-11-17, PDF/A included: check --strict for the structural facts, then the profile clauses per part, with veraPD | 4 | A damaged fixture per structural check, and a veraPDF corpus subset where both validators agree case by case | 11 (the writer edits what it validates) |

## `docs/DEFERRED.md`: open items and refusals

| line | item | state | plan or evidence |
| --- | --- | --- | --- |
| 37 | ICC colour management (`10b`) | open | [plan](plans/icc.md) |
| 38 | Tiling patterns (`/PatternType 1`) | **shipped 1.4.0** — row is stale (Q2) | `tests/tiling.rs`, `tests/pattern.rs` |
| 39 | Colour-key `/Mask` | **shipped 1.4.0** — row is stale (Q2) | `tests/colour_key.rs` |
| 40 | The clipping half of `Tr` modes 4-7 | **shipped 1.5.0** — row is stale (Q2) | `tests/tr_clip.rs`, `tests/tr_clip_type3.rs` |
| (mesh) | Mesh shadings `/ShadingType` 4-7 | open | [plan](plans/mesh-shadings.md), refused by name at `crates/pdf2-render/src/shading.rs:125-129` |
| 24 | Editing an opened document | **shipped 1.6.0** | `src/edit.rs`, `tests/edit.rs` |
| 25 | Forms filling and annotation authoring | open | [plan](plans/forms.md) |
| 26 | Conformance validation, PDF/A included | open | [plan](plans/validation.md), veraPDF as CI oracle |

| line | refusal | ADR line |
| --- | --- | --- |
| 53 | Racing hayro's throughput | ADR 0001 non-goal 1 |
| 54 | A C/C++ core, `unsafe`, or a bundled wasm decoder | ADR 0001 non-goal 2 |
| 55 | JavaScript actions and scripting | ADR 0001 non-goal 3 |
| 56 | PDF/A *conversion* | ADR 0001 non-goal 4 |
| 57 | Rendering with system fonts for standard-14 text | [verdict](plans/standard-14-fonts.md), measured not decided |

## Plans and the objective each would serve

| plan | deferred item it answers | rank |
| --- | --- | --- |
| [colour-key-mask.md](plans/colour-key-mask.md) | colour-key `/Mask` | 10 |
| [editing.md](plans/editing.md) | editing an opened document (shipped 1.6.0) | 11 |
| [forms.md](plans/forms.md) | forms filling and annotation authoring | 12 |
| [icc.md](plans/icc.md) | ICC colour management | 10b |
| [jpx-jbig2.md](plans/jpx-jbig2.md) | JPX and JBIG2 decoders (shipped 1.5.0) | 9 |
| [mesh-shadings.md](plans/mesh-shadings.md) | mesh shadings 4-7 | 10 |
| [standard-14-fonts.md](plans/standard-14-fonts.md) | standard-14 fidelity (a verdict, not a plan) | — |
| [tiling-patterns.md](plans/tiling-patterns.md) | tiling patterns (shipped 1.4.0) | 10 |
| [tr-clipping.md](plans/tr-clipping.md) | `Tr` clipping half (shipped 1.5.0) | 10 |
| [validation.md](plans/validation.md) | conformance validation | 13 |

## Demand numbers: the 64-file construct scan

`corpus/tier-a` (25 files, Apache-2.0, `corpus/manifest.json`) plus `tests/fixtures` (39 files) — 64
PDFs, scanned for the constructs the open rows above schedule. A zero is evidence too: it says an
objective has no corpus page to prove itself on and must be justified by a generated fixture or a proof
strategy.

| construct | files | first examples |
| --- | --- | --- |
| `/Annots` | 3 | alphatrans.pdf, blendmode.pdf, annotations.pdf |
| `/AcroForm` | 0 |  |
| `/XFA` | 0 |  |
| `/Outlines` | 2 | ContentStreamCycleType3insideType3.pdf, bug_jpx.pdf |
| `/Dest` | 0 |  |
| `/URI` | 2 | alphatrans.pdf, blendmode.pdf |
| `/EmbeddedFiles` | 1 | encrypted-attachment.pdf |
| `/Filespec` | 1 | encrypted-attachment.pdf |
| `/OCProperties` | 2 | cid_cff.pdf, cmykjpeg.pdf |
| `/StructTreeRoot` | 0 |  |
| `/MarkInfo` | 0 |  |
| `/Lang` | 7 | arial_unicode_ab_cidfont.pdf, blendmode.pdf |
| `/OutputIntents` | 0 |  |
| `/Intent` | 1 | cmykjpeg.pdf |
| `/BitsPerComponent 16` | 0 |  |
| `/WMode 1` | 0 |  |
| `/ShadingType 4` | 0 |  |
| `/ShadingType 5` | 0 |  |
| `/ShadingType 6` | 0 |  |
| `/ShadingType 7` | 0 |  |
| `/Mask` | 2 | colorkeymask.pdf, colour-key.pdf |
| `/ICCBased` | 2 | cid_cff.pdf, icc-paint.pdf |
| `/Separation` | 2 | function_based_shading_cmyk.pdf, separation.pdf |
| `/DeviceN` | 0 |  |
| `/PieceInfo` | 1 | cmykjpeg.pdf |
| `/Extensions` | 0 |  |
| `/Linearized` | 1 | bug_jpx.pdf |
| `/Metadata` | 11 | ShowText-ShadingPattern.pdf, alphatrans.pdf |

## Corrections queue (Q2)

Ten rows state a status the code contradicts (seven of them in `docs/FEATURES.md`), and two more need
narrowing rather than correction. Each is a `file:line` a later round can check directly:

| where | says | the code says |
| --- | --- | --- |
| `docs/FEATURES.md:127` | Type 1 "scheduled 4 / Not parsed" | `crates/pdf2-render/src/type1.rs`, `tests/type1.rs`, `tests/fixtures/type1.pdf` |
| `docs/FEATURES.md:150` | `8.10` Form XObjects "not drawn at all" | `crates/pdf2-render/src/walk.rs:1152` |
| `docs/FEATURES.md:210` | `/ID` "not read or written" | `src/edit.rs:245-423`, and row 75 of the same file |
| `docs/FEATURES.md:212` | `/ActualText` "never replaces what extraction returns" | `src/content.rs:400-738` |
| `docs/FEATURES.md:160` | render-time budget "shipped" | `docs/ROADMAP-NEXT.md:52` says no page-count timing exists |
| `docs/FEATURES.md:71` | "scheduled 8" | `docs/ROADMAP-NEXT.md:44` marks rank 8 shipped |
| `docs/FEATURES.md:150` | "scheduled 10d" | `docs/ROADMAP-NEXT.md` marks 10d shipped |
| `docs/ROADMAP-NEXT.md:38` | Type 1 "still scheduled" | as row 127 |
| `docs/FEATURES.md:242, :256` | encryption on write, wasm API growth "unplanned" | partly shipped; narrow the statement rather than flip it |
| `docs/DEFERRED.md:38` | tiling patterns deferred | drawn since 1.4.0 — `tests/tiling.rs`, `tests/pattern.rs` |
| `docs/DEFERRED.md:39` | colour-key `/Mask` deferred | drawn since 1.4.0 — `tests/colour_key.rs` |
| `docs/DEFERRED.md:40` | `Tr` 4-7 clipping deferred | drawn since 1.5.0 — `tests/tr_clip.rs`, `tests/tr_clip_type3.rs` |

Five shipped rows also cite crate-relative paths (`tests/ext_gstate.rs`, `tests/shading.rs`,
`tests/blend_modes.rs`, `tests/annotations.rs` twice) that resolve only under `crates/pdf2-render/` —
recorded here because a future `xtask ledger` path check needs the convention decided first.

## Figures this document does not have yet

Two numbers the ranking wants cannot be taken from here without work this objective does not own:

- **the packaged cost of the Chinese CMaps** — the two source files are 274 383 B (`UniGB-UCS2-H`) and
  326 349 B (`UniCNS-UCS2-H`), and the two `.bin`s already in `src/data/` were 19 967 B and 9 294 B from
  6 070 B and 168 597 B of CMap text; the packaged figure needs `cargo run -q -p xtask -- cmaps`
  generalised past its hardcoded Japan1 pair (`crates/xtask/src/main.rs:1092-1101`), which is the CMap
  objective's first task;
- **a page-count render time** — `book.pdf` page 1 renders in 4.3 ms against a 900 ms
  `corpus/render-budget.tsv` entry, and nothing times a document by page count; writing that harness is the
  scale objective's first round.

Until then the two objectives are ranked on the absence, which is itself measured.
