# Renderer quality: where the gap is, measured

This is the analysis behind [`docs/ROADMAP-NEXT.md`](ROADMAP-NEXT.md)'s painting
items. It exists because "match the competitors" is not a target — the measured
answer is that **fidelity is already at parity and the gap is coverage,
smoothness and speed**, and each of those has a number.

Measured 2026-11-15 with `pdf2 1.2.0`, `pdf2-render 1.2.0`.

## Fidelity: we are inside the engines' own disagreement

The oracle job compares each case with poppler, mupdf and Ghostscript. Our
distance to them is *smaller* than their distance to each other:

| Case | ours/poppler | ours/mupdf | allowed (spread × 1.1) |
| --- | --- | --- | --- |
| `book#1` (embedded serif, prose) | 0.0081 | 0.0041 | 0.0148 |
| `calibration#1` (CMYK conversion) | 0.0800 | 0.0000 | 0.0880 |
| `calibration#2`, `#3` (caps, joins, Béziers) | 0.0000–0.0115 | 0.0000 | 0.0172 / 0.0127 |

So the pages we draw, we draw as correctly as three independent engines agree a
page can be drawn. Chasing pixel distance further buys nothing until the list
below is empty.

## Coverage: what we do not draw

Two measurements. **Tier A** is the committed 23-file corpus; **the sample** is
the first three pages of 300 files from pdf.js's `test/pdfs` (302 pages).

| Missing construct | Tier A pages | Sample pages | Sample share |
| --- | --- | --- | --- |
| `Do` — Form XObjects and image codecs | 4 | **119** | **39.4 %** |
| `gs` — `/ExtGState`: alpha, blend modes, soft masks | 7 | 38 | 12.6 % |
| `cs` / `scn` — ICC, Separation, DeviceN, Pattern | 3 / 3 | 16 / 15 | ~5 % |
| `font:FontFile` — Type 1 PFB/PFA | 0 | 13 | 4.3 % |
| `font:FontFile3` — simple (non-CID) CFF | 1 | 12 | 4.0 % |
| `font:Type3` — glyph procedures | 3 | 8 | 2.6 % |
| `sh` — shading patterns (gradients) | 5 | 5 | 1.7 % |

**160 of the 302 sample pages render completely blank**, and 24 of the Tier A
corpus's 24 pages include 10 with no ink at all. That is the number a user
experiences, and it is dominated by two things: Form XObjects (everything an
office suite or Illustrator writes lives in one) and the image codecs item 9
owns (JPX, CCITT, CMYK JPEG).

## The worst failure mode is silence

49 of those 160 blank pages report **nothing wrong** — the report says "all
operators supported" while the page is empty. Two causes, both fixable cheaply:

- **Annotation appearance streams.** A page whose `/Contents` is empty and whose
  visible content is `/Annots → /AP → /N` (a `/Subtype /Form` stream) draws
  nothing because annotations are unplanned. 25 of the 31 silent blanks with real
  paint operators in their file are this.
- **TrueType programs with no `cmap`.** `TrueType_without_cmap.pdf` (Tier A and
  the sample) sets `/Encoding /WinAnsiEncoding` and a font whose `cmap` table is
  absent. `face.glyph_index(character)` returns `None`, we draw nothing, and
  nothing is reported. Competitors use the code as the glyph id when there is no
  cmap to consult.

Both are now fixed or scheduled: the cmap fallback shipped with the
`drew-nothing` report (1.2.1), and appearance streams are rank 12b.

## Smoothness: no anti-aliasing, and the gate cannot see it

`fill_polygons` samples pixel centres (`crates/pdf2-render/src/raster.rs`), so
every edge is hard. Users see this first at 100 % zoom; the oracle comparison
cannot see it at all, because it compares 4×4 blocks — a tolerance that exists
precisely because we have no anti-aliasing.

Consequence: **anti-aliasing cannot be gated by the current oracle**, so it needs
either a full-resolution comparison mode (mean error at 1×1, or SSE) or it ships
as a visible, reviewed change with re-blessed goldens and a written before/after.
Rank 10g.

## Speed: two hot spots, one of them unexpected

Measured on the committed fixtures (release build, best of five, scale 1):

| Page | Before | After | What changed |
| --- | --- | --- | --- |
| `book.pdf` p1 (embedded serif prose) | **74.8 ms** | **4.3 ms** | the rasteriser stops scanning rows the outline cannot reach |
| `book.pdf` p1 at scale 0.25 | 5.4 ms | 2.5 ms | the glyph cache, plus the row bound |
| `calibration.pdf` p9 | 0.1 ms | 0.1 ms | nothing to win |
| `Embedded_font.pdf` | 1.1 ms | 1.1 ms | one glyph |

**The surprise was where the time went.** The plan said "parse per glyph", and a
glyph cache was written — but measured alone it bought nothing on that page
(74.8 → 75.8 ms): `fill_polygons` scanned **every canvas row for every fill**, so
a page of 2 000 glyphs walked 2 000 × 595 rows regardless of how cheaply the
glyphs arrived. Bounding the scanlines to the outline's own extent took the page
to 4.3 ms — a 17× win from twelve lines. The cache then earns its keep where the
canvas is small and the parse dominates (5.4 → 2.5 ms at scale 0.25), and it
removes a per-occurrence parse that would otherwise grow with glyph count.

The oracle job's `ubuntu-latest` runner measured `book#1` at 437 ms before this;
the render-time budget in `corpus/render-budget.tsv` (900 ms) still holds with
room to spare, and the next recording should tighten it.

## What "matching" means, in order

Each step is a measurable claim, not a wish:

1. **`Do` on Form XObjects** (rank 10d): the 39.4 % of pages that currently lose
   their content entirely. The measured value of an item that was already ranked;
   `docs/ROADMAP-NEXT.md` now carries the number.
2. **`gs` — `/ExtGState`** (rank 10e): `/ca`, `/CA` and `/BM` first (the canvas
   already blends with alpha), soft masks after (a luminosity mask is a second
   render pass and a clip).
3. **Anti-aliasing** (rank 10g), with the oracle mode to prove it.
4. **Glyph cache** — shipped in 1.2.1; keeps the budget honest as the rest lands.
5. **Colour spaces** (item 10): ICCBased approximated by component count first,
   then Separation/DeviceN tint transforms; Pattern colour space rides on `sh`.
6. **Type 1 PFB and simple CFF** (item 4's remainder): `/FontFile` is not CFF and
   `ttf-parser` does not read it; the charset-name path is the work.
7. **Type 3 glyph procedures** (item 10): the content interpreter already exists,
   so a `d0`/`d1` char proc is a nested run with a per-glyph scope.
8. **Shadings** (item 10): smallest share of the sample, most conspicuous on the
   pages that have them.

## What is refused, and why

- **A faster renderer than `hayro`.** ADR 0001 refuses the feature race on
  speed; the budget exists so we do not silently lose it either.
- **C or `unsafe` geometry libraries.** The portability claim (`no_std`, wasm
  budgets, no build system) is the differentiator; a renderer that needs a
  rasteriser crate with `unsafe` SIMD gives it away.
- **Layout, forms *filling* and annotations *authoring*.** Reading and rendering
  appearance streams is in scope (rank 12b); editing them is item 11.

## How the numbers stay honest

- Every coverage claim above is reproducible: `cargo run -q -p xtask -- corpus`
  for Tier A, `cargo run -q -p xtask -- tier-b <tree>` for a sample, and the
  oracle job for fidelity and time.
- `pdf2-oracle`'s case list should grow from nine cases to the corpus pages that
  currently fail — a coverage item is "done" when its page joins the list.
- Tier B's histogram is the trend line: "blank" and "drew-nothing" counts should
  fall release by release, and a regression there is a bug like any other.
