# The objectives ledger: what is open, where it is owned, what it costs

This is the inventory the ranking in this file's next pass rests on, and the file the next
objective is cut from. It is generated from the repository, not from memory: every row carries the
`file:line` it came from, every demand number a command, and the counts are quoted beside the command
that produced them so a later round can diff them instead of re-reading the whole backlog.

Regenerate and verify the sources with:

```sh
cargo run -q -p xtask -- ledger      # 114 rows, 21 roadmap items, 28 citations, every count
cargo run -q -p xtask -- objective   # the counts block below, recomputed
grep -c '^|' docs/FEATURES.md        # 161 table lines (117 status rows once headers and separators are out)
ls docs/plans/*.md                   # 11 files: 10 plans + README
```

Neither command is advice: `cargo gate` runs both, and fails when this document disagrees
with the tree. What is checked, in one list —

- every citation here is `path#snippet` and the file must contain the snippet (28 of them);
- every `line` in the open-rows table is not checked (it is a reader's jump target), but every
  backticked path in `docs/FEATURES.md`'s evidence must exist (163 of them);
- the counts block below is compared line for line with `objective`'s output;
- no `docs/FEATURES.md` row may be `scheduled` under a rank `docs/ROADMAP-NEXT.md` marks ✓.

The CI Gate job runs `cargo gate`, so all of it runs on every pull request.

## Counts, as of this round

The numbers below come from one command, and `cargo gate` fails when this block and the recount
disagree — `cargo run -q -p xtask -- objective` prints exactly this:

```text
status rows: 117
  shipped              52
  shipped (partial)    14
  partial              2
  not applicable       5
  deferred             1
  refused              8
  scheduled            11
  unplanned            24
open rows: 35 (11 scheduled, 24 unplanned)
roadmap ranks: 21, open: 7
deferred open: 3, refusals: 5
plans: 10
```

Read it as: **117 status rows** in `docs/FEATURES.md` (52 `shipped`, 14 `shipped (partial)`, 2 bare
`partial`, 5 `not applicable`, 1 `deferred`, 8 `refused`, 11 `scheduled`, 24 `unplanned`), of which the
**35 open** ones are listed below; **21 roadmap ranks, 7 open** (`5, 9, 10, 10b, 10c, 12, 13`); **3 deferred
rows still open** in `docs/DEFERRED.md` (ICC, forms filling, validation — mesh shadings is a paragraph there,
not a row) beside **5 refusals** with an ADR line; and **10 plans** in `docs/plans/`.

`cargo run -q -p xtask -- ledger` adds its own line:

```text
ledger: 114 rows, 21 roadmap items, 28 citations, every item owned, every shipped row evidenced, every citation resolvable and every count matching
```

The three rows that used to read as deferred although the code shipped them (tiling patterns, colour-key
`/Mask`, the `Tr` 4-7 clipping half) are struck through with their tests named, as the JPX and JBIG2 row
already was. The ten contradictions this inventory started with were corrected in PR #160 (`bc9b72e`).

## Open rows in `docs/FEATURES.md` (35)

| line | item | status | owner |
| --- | --- | --- | --- |
| 72 | Damaged files: repair/rebuild policy beyond error reporting | unplanned | — |
| 74 | Linearisation (write) | unplanned | — |
| 78 | 7.10 Functions used for anything else | scheduled 10 | 10 |
| 79 | 7.11 Embedded files and file specifications (/EmbeddedFiles, /Filespec) | unplanned | — |
| 80 | 7.12 Extensions dictionary | unplanned | — |
| 97 | Images above 8 bits per component | unplanned | — |
| 100 | /Interpolate beyond bilinear | unplanned | — |
| 109 | ICC colour management (profiles actually applied) | scheduled 10b | 10b |
| 125 | Predefined CMaps (other than Japan1 90ms-RKSJ-H) | scheduled 5 | 5 |
| 129 | Type 3 fonts | scheduled 10 | 10 |
| 130 | Vertical writing mode | unplanned | — |
| 148 | Shadings and patterns | scheduled 10 | 10 |
| 149 | Transparency groups, soft masks, blend modes (11) | scheduled 10 | 10 |
| 153 | 8.11 Optional content (layers, /OCProperties) | unplanned | — |
| 180 | 12.2 Viewer preferences | unplanned | — |
| 181 | 12.3 Document-level navigation — outlines, destinations, /OpenAction | unplanned | — |
| 182 | 12.4 Page-level navigation — link annotations and /Dest | scheduled 12 | 12 |
| 183 | 12.5 Annotations — read, render, appearance streams | scheduled 12 | 12 |
| 184 | 12.6 Actions — /GoTo, /URI, JavaScript | unplanned | — |
| 185 | 12.7 Forms — read, render, fill | scheduled 12 | 12 |
| 187 | 12.9 Measurement properties (PDF 2.0) | unplanned | — |
| 188 | 12.10 Geospatial features (PDF 2.0) | unplanned | — |
| 189 | 12.11 Document requirements (PDF 2.0) | unplanned | — |
| 197 | 13.2 Multimedia, 13.7 Rich media | unplanned | — |
| 198 | 13.3 Sounds | unplanned | — |
| 199 | 13.4 Movies | unplanned | — |
| 200 | 13.5 Alternate presentations | unplanned | — |
| 211 | 14.5 Page-piece dictionaries | unplanned | — |
| 215 | 14.11 Prepress — output intents, separations | unplanned | — |
| 216 | 14.12 Document parts | unplanned | — |
| 217 | 14.13 Associated files | scheduled 12 | 12 |
| 231 | O Fragment identifiers | unplanned | — |
| 232 | Q Method for determining transparency on a page | unplanned | — |
| 254 | CLI render subcommand | unplanned | — |
| 262 | Validation (PDF/A, PDF 2.0 conformance) | scheduled 13 | 13 |

## Open ranks in `docs/ROADMAP-NEXT.md` (7)

The roadmap's own columns are kept, so an objective's effort and its proof strategy can be read without
opening the file. Ranks marked **✓** there are shipped and are not repeated here. **No line numbers**: the
rank is the key, because the six line numbers this table used to carry were already wrong (a rank that says
`5` does not move when the roadmap gains a paragraph above it).

| rank | what remains | effort | proof (done when) | depends on |
| --- | --- | --- | --- | --- |
| 5 | Predefined CMaps and CJK text — one CMap (90ms-RKSJ-H) shipped 2026-11-15, the rest data work | 2 | The CJK sample extracts its text; the shipped data is measured against the wasm budget | 2, and the licence first: Adobe's CMaps are BSD-3-Clause and deny.toml allows no BSD licence (docs/FEATURES.md §Outside facts), so the allow-list entry and notice — or a home outside the crate — come before the data |
| 9 | Image decoders behind features — DCT (RGB and CMYK) and CCITT 1-D/2-D shipped; JPEG 2000 and JBIG2 unblocked 2026-11-17 by the 1.100 floor and now a s | 3 | Those four pages have ink and pass the oracle check, with the budget decision recorded — raise the npm budget to ~499 KiB, or ship behind a non-defaul | 1 (to verify) |
| 10 | Painting completeness — shadings, functions, /Separation, /DeviceN, /PatternType 2, cs/scn, Type 3 and CFF fonts shipped 2026-11-16; tiling patterns a | 7 | Oracle diff on those corpus pages; docs/RENDERING.md's "still unproven" list shrinks to /UserUnit and >8-bit images | 1 |
| 10b | Colour management (ICC) — deferred: a fidelity decision, and since 2026-11-17 no longer a dependency one — moxcms 0.9.2 (MSRV 1.89, BSD-3-Clause OR Ap | 4 | Our distance to the median oracle on the colour page drops toward the inter-oracle spread, with the before/after measured | 1, 10 |
| 10c | Renderer performance budget — a per-page time budget gates the oracle job and a glyph cache exists; no 200-page document is timed yet | 2 | The oracle job records render time per page and fails above a committed budget; a 200-page document stays within it | 1 |
| 12 | Annotations and forms — reading and rendering appearance streams shipped (12b); filling and authoring scheduled 2026-11-17 ([plan](plans/forms.md)) (r | 5 | An annotated corpus page renders its appearance streams; a form field can be set and re-read | 11 for filling |
| 13 | Conformance validation — scheduled 2026-11-17, PDF/A included: check --strict for the structural facts, then the profile clauses per part, with veraPD | 4 | A damaged fixture per structural check, and a veraPDF corpus subset where both validators agree case by case | 11 (the writer edits what it validates) |

## `docs/DEFERRED.md`: open items and refusals

| line | item | state | plan or evidence |
| --- | --- | --- | --- |
| 37 | ICC colour management (`10b`) | open | [plan](plans/icc.md) |
| 38 | Tiling patterns (`/PatternType 1`) | **shipped 1.4.0**, row struck through in Q2 | `tests/tiling.rs`, `tests/pattern.rs` |
| 39 | Colour-key `/Mask` | **shipped 1.4.0**, row struck through in Q2 | `tests/colour_key.rs`; a stream `/Mask` stencil is still refused (`crates/pdf2-render/src/image.rs#a /Mask image stencil`) |
| 40 | The clipping half of `Tr` modes 4-7 | **shipped 1.5.0**, row struck through in Q2 | `tests/tr_clip.rs`, `tests/tr_clip_type3.rs` |
| (mesh) | Mesh shadings `/ShadingType` 4-7 | open | [plan](plans/mesh-shadings.md), refused by name at `crates/pdf2-render/src/shading.rs#mesh shadings (types 4 to 7)` |
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

## Corrections applied (Q2, PR #160 `bc9b72e`)

The inventory found ten rows that described work which had shipped, and the gate could not see them:
`cargo run -q -p xtask -- ledger` proves every row has an owner and a rank, not that a status matches the
tree. They are corrected, each with the file and the test that proves it:

| where | was | now |
| --- | --- | --- |
| `docs/FEATURES.md#Object streams at scale` | 65k-object document "scheduled 8" | `shipped` — `crates/pdf2-bench/src/corpus.rs#fn document_with_objects` and
  `crates/pdf2-bench/src/corpus.rs#the_budget_document_holds_sixty_five_thousand_objects`, `objects-65000` in `docs/PERFORMANCE.md`, `DEFAULT_OBJECT_LIMIT` at `src/document.rs#DEFAULT_OBJECT_LIMIT` |
| `docs/FEATURES.md#Type 1 (PFB/PFA) programs` | Type 1 "scheduled 4 / Not parsed" | `shipped` — `crates/pdf2-render/src/type1.rs`, `tests/type1.rs`, `tests/fixtures/type1.pdf` |
| `docs/FEATURES.md#**Form XObjects**` | `8.10` Form XObjects "not drawn at all" | `shipped` — `crates/pdf2-render/src/walk.rs#nested content stream (8.10)`, `MAX_FORM_DEPTH` |
| `docs/FEATURES.md#Render-time budget in the oracle job` | render-time budget "shipped" | `shipped (partial)` — the budget shipped, a page-count timing did not |
| `docs/FEATURES.md#File identifiers` | `/ID` "not read or written" | `shipped (partial)` — read and carried by an incremental save (`src/edit.rs#b"ID"`, ADR 0006 §5); `Document::new` emits none |
| `docs/FEATURES.md#Marked content` | `/ActualText` "never replaces what extraction returns" | `shipped (partial)` — it does (`src/content.rs#The open marked-content sequences`); `/Artifact` is still extracted as text |
| `docs/FEATURES.md#Encryption on write` | "encryption on write: unplanned" | `shipped (partial)` — updates are re-encrypted with the base's handler; a new document is not |
| `docs/FEATURES.md#wasm/npm API growth` | wasm API growth "unplanned" | `shipped (partial)` — `spans`, `span_texts`, `glyphs`, `glyph_texts`, `page_size`, `search*`, `metadata` exist; errors are still strings |
| `docs/ROADMAP-NEXT.md#CFF / Type 1 outlines` | rank 4 "Type 1 still scheduled" | `4 ✓`, shipped 2026-11-16 |
| `docs/DEFERRED.md#Tiling patterns`, `docs/DEFERRED.md#Colour-key`, `docs/DEFERRED.md#clipping half` | tiling patterns, colour-key `/Mask`, `Tr` 4-7 "deferred" | struck through as shipped, with `tests/tiling.rs`, `tests/pattern.rs`, `tests/colour_key.rs`, `tests/tr_clip.rs`, `tests/tr_clip_type3.rs` named |

`crates/xtask/src/main.rs`'s usage text also advertised `cargo xtask <chore>`, which is not a command — only
`gate`, `fixtures` and `bless` have an alias in `.cargo/config.toml`. It now names the three aliases and
`cargo run -q -p xtask -- <chore>` for everything else, because every exit criterion in the sequence below
is copied from there.

The **eight `version` pins** were the same class of defect one level down: `AGENTS.md`'s checklist bumps
them, `cargo run -q -p xtask -- versions` was documented as the proof, and `versions()` read each package's
own version through `cargo pkgid` and never the requirement beside `path` — so the gate was green with every
pin at 1.6.0 against a 1.8.0 group. PR #161 (`95f8719`) bumped all eight and made `versions()` walk the root
manifest and every `crates/*/Cargo.toml`, read each dependency's inline table, and fail on a pin that
disagrees with the group or on a path dependency with no `version` at all. Proven by breaking it on purpose:

```text
error: the group version is 1.8.0, but:
  crates/pdf2-render/Cargo.toml:36: a path dependency pins version 1.6.0, the group is 1.8.0
```

**Still open as a note, not a correction**: five shipped rows cite crate-relative paths
(`tests/ext_gstate.rs`, `tests/shading.rs`, `tests/blend_modes.rs`, `tests/annotations.rs` twice) that
resolve only under `crates/pdf2-render/`. They are correct where they live; a future `xtask ledger` path
check needs that convention decided first.

## Measurements

### What `moxcms` would cost the bundle (the ICC objective's missing figure)

Measured in a scratch worktree (`git worktree add /tmp/pdf2-icc HEAD`), so nothing lands in a shipped
manifest: `moxcms` 0.9.2 behind an `icc` feature on `pdf2-render`, with the conversion reachable from the
CMYK branch of `/DCTDecode`, then

```sh
cargo build -p pdf2-wasm --release --target wasm32-unknown-unknown [--features icc]
wasm-bindgen --target web --no-typescript --out-dir … target/…/pdf2_wasm.wasm
```

and gzip level 9 over the module plus its glue:

| build | wasm raw | wasm gzip | total gzip | delta |
| --- | --- | --- | --- | --- |
| baseline (no `icc`) | 1 123 404 | 390 442 | **396 248** | — |
| `icc`, `moxcms` `default-features = false` | 1 312 873 | 478 883 | **484 689** | **+88 441** |
| `icc`, `moxcms` with its default features | 1 434 645 | 511 846 | **517 652** | **+121 404** |

With `default-features = false` the dependency tree is `moxcms` → `num-traits` + `pxfm` only: no `rand`,
no `getrandom`, which is what a wasm target needs. The npm budget is 392 KiB (**401 408** gzip), so the
scalar build is **+83 281 over** it and the default-feature build **+116 281 over**: ICC cannot join the
default build any more than JPX or JBIG2 could, and the decision [ADR 0007](adr/0007-decoder-features.md)
records for the decoders is the shape it must take — a non-default feature, with the demo artefact carrying
it and the npm package not.

The number is a bound, not the finished cost: the scratch conversion parses a profile and builds one
transform per call, so the real implementation (a transform cached per image, an `/OutputIntents` lookup)
can come in lower — but not by an order of magnitude, and this is the figure the objective was missing.

### What the Chinese CMaps would cost (already known in source, not yet packaged)

| CMap | source bytes | the `.bin` it would become |
| --- | --- | --- |
| `90ms-RKSJ-H` (vendored) | 6 070 | 19 967 (`src/data/japan1_90ms_rksj.bin`) |
| `UniJIS-UCS2-H` (vendored) | 168 597 | 9 294 (`src/data/japan1_unijis_ucs2.bin`) |
| `UniGB-UCS2-H` (Adobe `Adobe-GB1-6/CMap`) | **274 383** | not measurable yet — `cargo run -q -p xtask -- cmaps` hardcodes the Japan1 pair (`crates/xtask/src/main.rs#fn cmaps`) |
| `UniCNS-UCS2-H` (Adobe `Adobe-CNS1-7/CMap`) | **326 349** | as above |

A *coverage* table (both Chinese CMaps are Unicode→CID like `UniJIS-UCS2-H`) compresses to a fraction of
its source — 168 597 → 9 294 is 5.5 % — while a byte-encoding table like `90ms-RKSJ-H` expands. So the
packaged figure is tens of kilobytes at most, and the CMap objective's first job is to generalise `cmaps()`
and measure rather than assume.

## The sequence

Ten objectives, ranked by demand ÷ cost with `docs/GAPS.md`'s scale and its tie-break (prefer no new
dependency, then code we already have, then more independent evidence of pain). Each row names the demand
number measured in this tree, the one command that says "done", the effort in rounds of ~40 turns, what must
land first, and the plan it is cut from. **A `/rounds-plan` on any row's objective sentence is a legal
next objective.**

| # | objective (one sentence) | demand, measured here | exit criterion | effort | depends on | plan |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | **Extraction quality: ligatures, `TJ` kerning and `/Artifact`** — make `text()` return what a reader would copy, including the parts a writer marked as decoration | the Tier B histogram's largest class: **6 of 16** failures in 200 pdf.js files are composite fonts without `/ToUnicode`; `/Artifact` is still extracted as text (`docs/FEATURES.md#Marked content`) | `cargo test -p pdf2 --test content` | 2 | — | none yet |
| 2 | **Render speed at page count** — a 200-page document timed page by page, with the budget tightened to what the measurement allows | **4.3 ms** per page on `book.pdf` against a **900 ms** budget (`corpus/render-budget.tsv`); nothing times a document by page count | `cargo run -q -p xtask -- bench` | 1 | — | none yet |
| 3 | **Chinese and Korean CMaps** — Generalise `cmaps()` past the Japan1 pair and ship `UniGB-UCS2-H`, `UniCNS-UCS2-H`, `UniKS-UCS2-H` under the existing BSD-3-Clause notice | one CMap family ships today; the two Chinese sources are **274 383 B** and **326 349 B** (5.5 % of a coverage table survives packing — 168 597 → 9 294 B) | `cargo run -q -p xtask -- corpus` | 2 | — | none yet |
| 4 | **The reader's document model: outlines, destinations, link actions, embedded files** — a page list is not a document | `/Outlines` **2**, `/URI` **2**, `/EmbeddedFiles` **1** of the 64 scanned files; nothing is modelled (`docs/FEATURES.md#Embedded files and file specifications`,
-      `docs/FEATURES.md#Document-level navigation`, `docs/FEATURES.md#Actions — `) | `cargo test -p pdf2 --test document` | 2 | — | none yet |
| 5 | **Repair a damaged file instead of stopping at the first error** — rebuild what can be rebuilt, report what cannot | Tier B: **2** missing `startxref`, **1** page-tree cycle, **1** malformed xref subsection out of 16 failures in 200 files | `cargo test -p pdf2 --test robustness` | 2 | — | none yet |
| 6 | **ICC colour management behind a non-default `icc` feature** — a `/DeviceCMYK` page converted through the profile it names | `cmykjpeg.pdf` sits **8.455 mean** from the engines' **2.000** (`docs/RENDERING-QUALITY.md#CMYK images are a colour-management gap`); `/ICCBased` in **2** of 64 files; the feature costs **+88 441 gzip** scalar, **+121 404** with `moxcms`' defaults | `cargo test -p pdf2-render --test quality` | 4 | — | [icc.md](plans/icc.md) |
| 7 | **Optional content: layers and their default configuration** — draw the configuration a reader would show, not every layer | `/OCProperties` in **2** of 64 files (`cmykjpeg.pdf`, `cid_cff.pdf`); nothing is modelled (`docs/FEATURES.md#Optional content`) | `cargo test -p pdf2-render --test golden` | 2 | 4 | none yet |
| 8 | **Forms filling and annotation authoring** — set a field and keep it when the file is saved | `/AcroForm` **0** of 64 files, so the evidence is the competitor: `pdfium-render` **4 079 913** downloads serves exactly this; the write side exists ([ADR 0006](adr/0006-editing-mutation-model.md)) | `cargo test -p pdf2 --test edit` | 3 | 4 | [forms.md](plans/forms.md) |
| 9 | **Tagged PDF: structure tree, marked content and `/Artifact`** — the model PDF/UA-2 is written against | `/StructTreeRoot` **0**, `/MarkInfo` **0** of 64 files: no corpus page exists, so the evidence must come from `veraPDF-corpus` (CC BY 4.0, UA1/UA2) and generated fixtures | `cargo test -p pdf2 --test pages` | 3 | 4 | none yet |
| 10 | **Conformance validation: PDF/A-4** — `check --strict` over the structural facts, then the part's clauses | ISO 19005-4:2020 (`pdf-issues.pdfa.org/19005-4-2020/`); veraPDF **1.30.4** is the CI oracle (GPL-3.0, so a binary, never a dependency); the corpus is CC BY 4.0 | `cargo run -q -p pdf2-cli -- check tests/fixtures/book.pdf` | 4 | 9 | [validation.md](plans/validation.md) |

Every criterion above **passes today** — that is what makes it runnable — and an objective is done when it
passes *with the cases that objective adds*: the command is the container, the new assertion is the work. The
runs are below, so a later round can tell "the container still works" from "the work landed".

**The four candidates with no corpus evidence**, ranked rather than guessed: tagged PDF (#9) and forms (#8)
are justified above by `veraPDF-corpus` and by competitor demand; **>8-bit images** (`/BitsPerComponent 16`
0 of 64) and **vertical writing** (`/WMode 1` 0 of 64) are below the line until a generated fixture proves
them — a fixture is the cheap half, and neither can be ranked higher without a page that needs it.

Below the line and not sequenced, with the reason: viewer preferences, page-piece dictionaries, measurement
properties, geospatial features, requirements, multimedia/rich media, sounds, movies, alternate
presentations, document parts, `O` fragment identifiers, `Q` transparency flag, and the CLI `render`
subcommand — each either a viewer's concern or without a page in the corpus (`docs/FEATURES.md` lists them
with this reasoning). Refusals stay refusals ([ADR 0001](adr/0001-differentiators.md)).

## Exit criteria, run once (2026-11-18)

Each command from the table above, executed on `d2f97bb` with the output it printed. All ten pass: none of
them is the objective's assertion yet, they are the harnesses it will extend.

```text
$ cargo test -q -p pdf2 --test content
test result: ok. 27 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo run -q -p xtask -- bench
compared 30 metric(s) with docs/performance-baseline.json (written 2026-11-17)
skipped 1 metric(s): no gated baseline value for platform macos-aarch64
within 10% tolerance

$ cargo run -q -p xtask -- corpus
25 files: 24 fully readable, 25 rendered
   1 x composite (Type0) fonts without a /ToUnicode map

$ cargo test -q -p pdf2 --test document
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -q -p pdf2 --test robustness
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -q -p pdf2-render --test quality
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -q -p pdf2-render --test golden
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -q -p pdf2 --test edit
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -q -p pdf2 --test pages
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo run -q -p pdf2-cli -- check tests/fixtures/book.pdf
ok: tests/fixtures/book.pdf: 1 page(s) readable
```

The corpus line is the one number that will move first: objective 3 adds a Chinese sample, and objective 1's
class ("composite (Type0) fonts without a `/ToUnicode` map") is the last one standing in that histogram.

## Figures this document does not have yet

Two numbers the ranking wants cannot be taken from here without work this objective does not own:

- **the packaged cost of the Chinese CMaps** — the two source files are 274 383 B (`UniGB-UCS2-H`) and
  326 349 B (`UniCNS-UCS2-H`), and the two `.bin`s already in `src/data/` were 19 967 B and 9 294 B from
  6 070 B and 168 597 B of CMap text; the packaged figure needs `cargo run -q -p xtask -- cmaps`
  generalised past its hardcoded Japan1 pair (`crates/xtask/src/main.rs#fn cmaps`), which is the CMap
  objective's first task;
- **a page-count render time** — `book.pdf` page 1 renders in 4.3 ms against a 900 ms
  `corpus/render-budget.tsv` entry, and nothing times a document by page count; writing that harness is the
  scale objective's first round.

Until then the two objectives are ranked on the absence, which is itself measured.
