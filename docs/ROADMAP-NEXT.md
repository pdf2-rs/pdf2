# The next version: text and rendering agree

Inputs: the ADR's two differentiators (`docs/adr/0001-differentiators.md`), the
scorecard (`docs/GAPS.md`), and the measured corpus (`docs/CORPUS.md`). No new
external numbers appear here — every value below is a fact already recorded in
those three files.

**This file ranked the work that became 1.2, 1.3 and 1.4, and its ranks are statuses
now** (thirteen shipped, seven open). What comes *after* them is ranked in
[`docs/OBJECTIVES.md`](OBJECTIVES.md), which quotes these ranks, measures each open item
in the tree, and gives every objective one command as its exit criterion.

[`docs/FEATURES.md`](FEATURES.md) is the completeness check on this ranking: every
feature area of ISO 32000-2, the API and the tooling is marked shipped,
scheduled (with the item number below), refused (with the ADR line), or
unplanned. Its seven forced decisions are folded in here, which is why some items
below carry a letter.

**The theme of the next release, `1.2`, is text and rendering agreement.** The
corpus made the case better than any estimate could: of 23 real files, **21
render and 14 are fully readable**, and seven of the nine failures are the *same*
failure — composite (Type 0) extraction, on files the renderer already draws.
D1 says those two answers must agree; today they disagree on 30% of the corpus.
`1.2` closes that gap and puts the proof gate (oracles in CI) under it, so the
claims after it can be checked rather than asserted.

Two releases follow, each with a one-line theme: `1.3` **opens everything**
(image codecs, robustness at scale, painting features), and `1.4` **edit what you
read** (the incremental writer, the theme the ADR deferred rather than refused).

## Ranking

Value is measured where possible (corpus files affected, competitor downloads,
issue counts); effort is in rounds of ~40 turns; proof is the check that makes an
item "done"; dependencies are what must land first. Ranked by value ÷ effort,
with dependencies breaking ties.

| Rank | Item | Value (evidence) | Effort | Proof (done when) | Depends on |
| --- | --- | --- | --- | --- | --- |
| 1 ✓ | **Oracle CI job and tolerance calibration** — *shipped 2026-11-15* | Every rendering claim is currently unfalsifiable: the corpus shading and transparency files "render" with operators skipped. `docs/PROOF.md` defines the method; nothing runs it. | 1 | A CI job renders three fixtures and a corpus subset with poppler and mupdf, prints the inter-oracle spread, and fails when we are outside the calibrated tolerance | — |
| 2 ✓ | **Composite-font extraction** (`/Identity-H` code splitting, `/ToUnicode` `bfchar`/`bfrange`) — *shipped 2026-11-15* | **7 of 23 corpus files** fail only on this, and all seven render; `pdf-extract` has 8 236 722 downloads and 42 issues mentioning fonts | 2.5 | Those seven files become fully readable; `cargo xtask corpus` reports 21/23 readable; a fixture page with `/ToUnicode` pins the mapping | — |
| 3 ✓ | **`/Encrypt` revisions 2–4** (RC4, AES-128) — *shipped 2026-11-15* | 1 of 23 corpus files; every PDF 1.x reader implements them, and this library has AES-256 revision 6 only (gap 4b) | 2 | `encrypted-attachment.pdf` opens; a fixture encrypted with revision 4 is read; `docs/RENDERING.md` limits table updated | — |
| 4 ✓ | **CFF / Type 1 outlines** — *CFF shipped 2026-11-15, Type 1 (`/FontFile`) shipped 2026-11-16* | The renderer reports `font:FontFile3`/`font:FontFile` instead of drawing; `skrifa` (50.0M downloads) and `read-fonts` (52.9M) already parse both, `std` optional | 2 | No `font:FontFile*` report on the corpus CFF files, and their pages pass the oracle check from item 1 | 1 (to verify), D2's dependency verdict |
| 5 (half) | **Predefined CMaps and CJK text** — *one CMap (`90ms-RKSJ-H`) shipped 2026-11-15, the rest data work* | The corpus CJK sample (`90ms_rksj_h_sample.pdf`) is one of the seven; hayro ships 61 CMaps (~250 KB brotli) to make this work | 2 | The CJK sample extracts its text; the shipped data is measured against the wasm budget | 2, and **the licence first**: Adobe's CMaps are BSD-3-Clause and `deny.toml` allows no BSD licence (`docs/FEATURES.md` §Outside facts), so the allow-list entry and notice — or a home outside the crate — come before the data |
| 6b ✓ | **`/CropBox` and `/UserUnit`** — *shipped 2026-11-15* | Parsed (`src/page.rs:97`) but the renderer uses `/MediaBox` only and never reads `/UserUnit`; the oracle gate can measure the difference on a page with either | 0.5 | A fixture with a crop box and a user unit renders as the oracle does | 1 (to verify) |
| 6 ✓ | **1.x cross-reference generation edge** — *shipped 2026-11-15* | 1 corpus file (`ContentStreamCycleType3insideType3.pdf`) fails with `malformed cross-reference generation at offset 2111` | 1 | That file parses; the regression is a fixture with the same generation pattern | — |
| 7 ✓ | **Tier B corpus run in CI** — *shipped 2026-11-15* | 23 committed files are evidence; 690 pdf.js files and veraPDF's corpus (CC BY 4.0) are a measurement | 1 | A scheduled job fetches the pinned trees, runs `check`/`text`/render, and publishes a class histogram | 2, 6 (for signal) |
| 2b ✓ | **Extraction quality** (decided: `docs/adr/0002-text-extraction-shape.md`; reading order shipped 2026-11-15, spans and advances deferred by that ADR) (reading order, spaces from advances, ligatures, spans and positions) | Item 2 fixes *mapping*; this is the class behind most of `pdf-extract`'s 78 issues, and it needs an API decision (text only, or spans with boxes) before it can be built | 2 | A corpus page extracts in reading order with correct spaces; the API is documented | 2 |
| 8 ✓ | **Robustness at scale** — *shipped 2026-11-16* (object-stream complexity, parser limits, timeouts) | A 65 542-object stream took 37 s in a sibling project before an O(N²) fix; five fuzz targets exist, no complexity test | 2 | A synthetic large-object-stream fixture parses under a stated time budget; a fuzz target covers it | — |
| 9 (half) | **Image decoders behind features** — *DCT (RGB and CMYK) and CCITT 1-D/2-D shipped; JPEG 2000 and JBIG2 **unblocked 2026-11-17** by the 1.100 floor and now a size decision* — *measured on the 1.8.0 tree: `hayro-jpeg2000` 0.5.0 costs **+78 174 gzip** and `hayro-jbig2` 0.4.0 **+64 583**, against a 392 KiB npm budget ([ADR 0007](adr/0007-decoder-features.md))* (`docs/adr/0004-image-decoders-and-the-msrv.md`) | Four corpus files still render **blank**; the decoders exist as permissive crates and nothing but the bundle blocks them | 3 | Those four pages have ink and pass the oracle check, with the budget decision recorded — raise the npm budget to ~499 KiB, or ship behind a non-default feature | 1 (to verify) |
| 10 (half) | **Painting completeness** — *shadings, functions, `/Separation`, `/DeviceN`, `/PatternType 2`, `cs`/`scn`, Type 3 and CFF fonts shipped 2026-11-16; tiling patterns and the `Tr` clipping half deferred (`docs/DEFERRED.md`)*: shadings, patterns, transparency, soft and colour-key masks, colour spaces (`cs`/`scn`), `7.10` functions (shadings cannot be drawn without them), `Tr` render modes, Type 3 fonts | Corpus shading, blend-mode, alpha and mask files render today only because the operators are skipped; `Tr` modes and Type 3 were in `docs/RENDERING.md`'s unproven list with no owner until the ledger found them | 7 | Oracle diff on those corpus pages; `docs/RENDERING.md`'s "still unproven" list shrinks to `/UserUnit` and >8-bit images | 1 |
| 10b | **Colour management (ICC)** — *deferred: a fidelity decision, and since 2026-11-17 no longer a dependency one* — `moxcms` 0.9.2 (MSRV 1.89, `BSD-3-Clause OR Apache-2.0`) is inside the 1.100 floor and already on `deny.toml`'s allow-list | The oracle calibration measures an **8 % spread** between the engines on the CMYK page; our distance to them is 8.455 mean against their 2.000 | 4 | Our distance to the median oracle on the colour page drops toward the inter-oracle spread, with the before/after measured | 1, 10 |
| 10d ✓ | **Form XObjects** — *shipped 2026-11-16* (`8.10`, `/Subtype /Form`) — measured 2026-11-15: **39.4 % of the pages** in a 302-page pdf.js sample report `Do`, and it is the reason most of their blank pages are blank (`docs/RENDERING-QUALITY.md`) | Not drawn at all: `Do` dispatches to `draw_image`, which bails out at the `/Subtype /Image` test (`crates/pdf2-render/src/walk.rs:534`, `:701`), so vector content, nested pages and some text in Form XObjects draw nothing. The ledger's decision 8 named it as unowned | 3 | A corpus page whose content is a Form XObject renders as the oracle does (item 1), and `Do` stops being reported for it | 1 |
| 10e ✓ | **`/ExtGState`** — *shipped 2026-11-16* (`8.4.5`): constant alpha `/ca` `/CA`, blend modes `/BM`, soft masks `/SMask` | Measured: `gs` is unreported on **12.6 %** of the pages in the same sample, and `blendmode.pdf`, `alphatrans.pdf`, `ArabicCIDTrueType.pdf` and 4 more corpus pages lose their transparency entirely. The canvas already blends with an alpha (`Canvas::blend`), so the constant-alpha and normal-blend halves are wiring | 1–1.5 | `blendmode.pdf` and `alphatrans.pdf` render with `gs` reported zero times and join the oracle case list; a soft-mask fixture shows the mask through a cut-out | 10d for `/SMask` groups |
| 10g ✓ | **Anti-aliasing** — *shipped 2026-11-16* (`10.7` scan conversion) | Every page at 100 % zoom: `fill_polygons` samples pixel centres, so all edges are hard. The oracle cannot see it — comparisons are at 4×4 blocks, a tolerance that exists *because* we do not antialias | 1.5 | Coverage-based AA, goldens re-blessed with a written before/after, and an oracle mode comparing at 1×1 (mean error or SSE) that fails a page which loses AA | 10c |
| 12b ✓ | **Render annotation appearance streams** — *shipped 2026-11-16* | Measured: of 49 sample pages that are blank **and report nothing**, 25 have real paint operators and are annotation `/AP` form streams. A page that looks empty with no explanation is our worst failure mode | 1 | An annotated corpus page (or a generated `/AP` fixture) draws its annotation's appearance where the oracle draws it | 10d |
| 10c (half) | **Renderer performance budget** — *a per-page time budget gates the oracle job and a glyph cache exists; no 200-page document is timed yet* | No item exists: no glyph cache, no anti-aliasing, no time budget. ADR non-goal 1 refuses *racing* hayro, not having a number | 2 | The oracle job records render time per page and fails above a committed budget; a 200-page document stays within it | 1 |
| 11 ✓ | **Editing an opened document** — *shipped 2026-11-17* ([ADR 0006](adr/0006-editing-mutation-model.md), [plan](plans/editing.md)): `MutableDocument` overlays a `Document`, `save_incremental` appends, and an encrypted base is written encrypted | `lopdf` 30 289 120 downloads is the demand; `src/writer.rs` only builds new files (gap 5) | 5+ | Open a corpus file, add a page, save, reopen: the same page count in our reader and an oracle's — met: `tests/edit.rs` reopens the saved bytes and has `pdftoppm`/`mutool` render the appended page | — (a release of its own) |
| 12 (half) | **Annotations and forms** — *reading and rendering appearance streams shipped (`12b`); **filling and authoring scheduled 2026-11-17*** ([plan](plans/forms.md)) (read, render, fill) | `pdfium-render` 3 954 648 downloads covers them; nothing here does | 5 | An annotated corpus page renders its appearance streams; a form field can be set and re-read | 11 for filling |
| 13 | **Conformance validation** — ***scheduled 2026-11-17***, PDF/A included: `check --strict` for the structural facts, then the profile clauses per part, with **veraPDF 1.30 as the CI oracle** ([plan](plans/validation.md)) (PDF/A, PDF 2.0) | `veraPDF` is the Java reference, `qpdf --check` has 5 588★; no Rust validator | 4 | A damaged fixture per structural check, and a veraPDF corpus subset where both validators agree case by case | 11 (the writer edits what it validates) |

## What `1.2` contains, and what it does not

**In**: items 1–7, plus 2b (extraction quality), 6b (`/CropBox`, `/UserUnit`) and
10c (a render-time budget). Together they turn "14 of 23 readable" into "21 of
23", put oracles under the claims, give rendering a number to hit, and make the
proof gate run on every release.

**Out, deliberately**: image codecs and painting features (they change what is
*drawn*, and item 1 has to exist before that can be judged); editing (a theme of
its own, per the ADR); annotations and validation (not scheduled at all).

**Item 1 is shipped** (2026-11-15): `crates/pdf2-oracle` compares this renderer
with poppler, mupdf and Ghostscript on PPM output at 4×4 blocks, the `oracle` job
in `ci.yml` gates every push at the inter-oracle spread plus the 1% floor, and
the measured spread is committed in `corpus/tolerances.tsv` (widest: 8.0% on the
CMYK colour page, where the engines themselves disagree).

**This ranking is checked against [`docs/FEATURES.md`](FEATURES.md)**, which lists
every ISO 32000-2 area as shipped, scheduled with one of these ranks, refused in
the ADR, or unplanned. `cargo run -q -p xtask -- ledger` (part of `cargo gate`
and of CI) fails when a rank here has no ledger row or a row names a rank that
does not exist, so adding an item means adding its row in the same change.

## Checkable outcomes for `1.2`

- `cargo xtask corpus` prints **21 of 23 fully readable** (from 14) and **23 of
  23 parsed** (from 21).
- The oracle job exists in CI and fails a page outside its calibrated tolerance.
- `docs/RENDERING.md` no longer lists composite extraction as unsupported, and
  its limits table names the encryption revisions that are now read.
- A `/CropBox`/`/UserUnit` fixture passes the oracle check, extraction returns
  reading order with correct spaces, and the oracle job records render time
  against a committed budget.
- No new dependency lands without the `no_std` build and the wasm-size budget
  recorded in `docs/PERFORMANCE.md`.

## Risks and the response

- **`/ToUnicode` is not always present.** Files without it need glyph-name or
  `cmap` fallbacks; the corpus measures how often (7 files now, more in Tier B).
  Response: ship the mapping when present, report when absent, and let item 7
  quantify the rest.
- **Per-deconstruct data sizes.** Predefined CMaps add ~250 KB; image decoders add
  binary size. Response: features, off by default for `no_std` users, with the
  budgets updated in the same change.
- **The oracle is not Acrobat.** Where engines disagree, tolerance comes from
  their spread (`docs/PROOF.md`), and the residual is documented rather than
  hidden.
