# Proving the renderer right

A golden PNG proves that the renderer still draws what it drew last time. It
cannot prove that what it drew is *correct* — a bug blessed once is a bug
forever. Correctness here comes from a second, independent source of truth: a
fixture whose right answer can be written down from ISO 32000 before the
renderer runs.

`tests/fixtures/calibration.pdf` is that fixture. Its generator
(`tests/fixtures/make_fixtures.py`) states the arithmetic in comments, and
`crates/pdf2-render/tests/calibration.rs` asserts it pixel by pixel, naming the
pixel centre it tests (a probe at `(x, y)` tests `(x + 0.5, y + 0.5)`, because
the scanline samples centres and does not antialias).

## What each page proves

| Page | Clause | What is asserted |
| --- | --- | --- |
| 1 colour | 8.6.4, 8.6.5 | `g`/`rg`/`k` produce the arithmetic bytes: 0.5 gray is 128, 0.25 is 64, `k` uses `R = (1 − c)(1 − k)` |
| 2 caps | 8.4.3.3 | butt stops at the end point, round and square reach half the width, and only square fills the corner past the end |
| 2 joins | 8.4.3.4 | a 90° miter fills the outer square corner, a bevel cuts it |
| 2 dash phase | 8.4.3.6 | `[20 10] 5 d` starts 5 into the first dash: painted 120–135, empty 135–145 |
| 2 fill rules | 8.5.3.3 | on identical nested rectangles, non-zero keeps the inner one and even-odd punches it out |
| 2 curves | 8.5.2.2, 8.5.2.4 | a four-cubic circle covers πr² = 2827 px at 1 px/pt, within 4% |
| 3 images | 8.9.5.2 | a 2×2 checkerboard at 4× scale keeps exact 60 px block edges; `/Decode [1 0 …]` inverts; `/Indexed` picks palette entries; a quarter-turn matrix maps image space to device space without mirroring |
| 4 page box | 14.11.2 | a `/MediaBox` with a non-zero origin shifts content to the canvas corner |
| 5 rotate | 7.7.3.3 | `/Rotate 90` swaps the canvas dimensions and turns the page clockwise |
| 6 text | 9.3.3, 9.4.4 | `Tz 50` halves the run width; `Tw 40` on two spaces widens it by 80 units |

Every page also asserts `unsupported_total() == 0`, so a fixture cannot quietly
pass by being ignored.

## Regression net

`tests/fixtures/golden/calibration-*.png` pin every page, page 1–6, as the
renderer draws them today; `golden/paths.png` and `golden/images.png` do the
same for the older fixtures. They catch accidental changes, and the probes catch
wrong ones.

```sh
cargo run -p pdf2-render --bin pdf2-render-golden -- --fixture tests/fixtures/calibration.pdf --page 3 --out /tmp/p3.png
BLESS=1 cargo test -p pdf2-render --test calibration   # re-bless after an intended change
```

Re-blessing is a review event, not a repair: a golden that moves has to be
explained by a probe or by the clause it implements.

## Oracles and tolerances

Hand-computable pages stop scaling once text layout, transparency and fonts are
involved. For those, the claim has to be measured against independent engines —
and the tolerance has to come from *their* disagreement rather than from a
number someone liked.

**[`docs/PROOF.md`](PROOF.md) is the proof strategy**: the three tiers of
evidence (arithmetic fixtures, a committed curated corpus, pinned upstream
corpora in CI), the licences behind each corpus, which engines are oracles
(poppler, mupdf, Ghostscript in the gate; pdfium, pdf.js and PDFBox as manual
cross-checks), the three metrics per page (geometry, text, pixels at 144 DPI),
and the rule that our tolerance is the largest inter-oracle spread on that page
plus 10% — with a 1% fallback bound where no calibration exists yet.

A golden PNG pins a regression; it never proves a new claim. A file outside its
tolerance is a finding in `docs/CORPUS.md`, not a re-blessing.

## What the calibration found

The fixture was written after the 1.0 release, and it immediately failed on four
real bugs:

1. **Every Bézier curve was drawn to its last control point.** `point_operand`
   treated a caller's point slot as a number index, so `c x1 y1 x2 y2 x3 y3`
   moved to `(x2, y2)`. Curves in 1.0.0 are visibly wrong; a semicircle fixture
   is now part of the coverage.
2. **`/Decode` was ignored**, so inverted and rescaled images rendered as raw
   samples.
3. **Joins were always round** — `j` was parsed and dropped, and the miter limit
   `M` was not read at all.
4. **`/Rotate` was ignored**, so a rotated page rendered unrotated on the media
   box canvas. The renderer now honours 90/180/270; `page-tree.pdf` page 2
   (which inherits `/Rotate 90`) exposed the change in the older golden test.

## Fonts

| What the PDF carries | What the renderer does |
| --- | --- |
| Simple font with `/FontFile2` (embedded TrueType) | glyph outlines from `ttf-parser`, placed by the PDF's encoding — proven by page 9, whose font this repository generates from the same 5×7 alphabet (`A` and `B` are letters with closed holes, so a probe can tell an outline from the fallback) |
| Simple font with `/Encoding` (`WinAnsiEncoding`, `/Differences`) | codes map through the encoding; `A` remapped to `/bullet` draws the bullet |
| `/Widths` and the descriptor's `/MissingWidth` | used for advances, so spacing matches the file instead of a fixed 0.6 em |
| Base-14 with no program (Helvetica and friends) | the built-in 5×7 bitmap alphabet below |
| A real book page | `book.pdf` — six paragraphs of public-domain prose set in an embedded Crimson Text subset, typeset by the fixture generator from the font's own advances, with `text()` extraction asserted on the result |
| `/Type0` + `/Identity-H` + `CIDFontType2` with `/FontFile2` | two-byte codes, `/CIDToGIDMap` (identity or stream) to glyph ids, `/W`/`/DW` advances — proven by pages 10 and 11, where the same codes draw different glyphs once the map is swapped |
| `/ExtGState` (`8.4.5`): `/ca`, `/CA`, `/BM` (all sixteen), `/LW`, `/LC`, `/LJ`, `/ML`, `/D`, `/SMask` `/Luminosity` | **honoured**, and carried by `q`/`Q`. A luminosity soft mask is rendered from its `/G` onto black and multiplied into the coverage mask; `/SMask /None` is not a mask; an `/S /Alpha` mask is reported as `gs`. The sixteen blend modes follow the CSS formulae the standard refers to, `(1-a)·Cb + a·B(Cb, Cs)` over an opaque backdrop |
| `/DCTDecode` (JPEG) images | **drawn** through `zune-jpeg` (the `dct` feature, on by default); the size comes from the JPEG's own header and is checked against the decode budget before the pixels are built |
| Scan conversion (`10.7`) | **anti-aliased**: four sub-scanlines per row with exact horizontal coverage, summed and blended once per pixel; the vertical error is the only one left, and `corpus/aa-ceilings.tsv` gates it at 1×1 |
| Annotation appearance streams (`12.5.5`) | **drawn**: `/Annots → /AP → /N`, placed by mapping the appearance's transformed `/BBox` onto the annotation's `/Rect`, after the page content; `/AS` selects from a state dictionary and `/F` invisible/hidden/no-view annotations are skipped |
| `/Subtype /Form` XObjects (`8.10`) | **drawn**: the form's `/Matrix` is concatenated, its `/BBox` clips, its `/Resources` replace (or inherit) the current ones, and nesting is capped at `MAX_FORM_DEPTH` (8) with the excess reported as `Do` |
| `/Type0` without a usable program or with a CMap stream | **not drawn**, reported by reason: `font:Type0-no-program`, `font:Type0-cmap`, `font:CIDFontType0` (CFF) |
| `/Type3` | **not drawn**: `font:Type3` |
| `/FontFile3` CFF and OpenType programs on a `/Type0` font | **drawn**: the bare CFF is wrapped in the smallest OpenType container and its charset is inverted so a CID finds its glyph (`docs/adr/0003-cff-without-a-dependency.md`) |
| A TrueType program with no `cmap` table | **not drawn**, reported: `font:TrueType-without-cmap`. The codes cannot be decoded without a map, and guessing glyph ids draws wrong glyphs (`docs/RENDERING-QUALITY.md`) |
| A page that paints and produces an empty raster | reported: `drew-nothing`, so a blank page that should have had content is visible instead of silent |
| `/FontFile` (Type 1) and simple-font `/FontFile3` programs | **not parsed**: the bitmap fallback is drawn and `font:FontFile`/`font:FontFile3` is reported |
| Text render modes other than fill (`Tr`), vertical writing | not implemented |

The report is the honest part: a page that used a font this renderer cannot draw
says so, rather than looking like a page of boxes with no explanation.

## The built-in text fallback

A base-14 font with no embedded program is drawn with a 5×7 bitmap alphabet:
ASCII letters, digits and punctuation, plus Latin-1 accents composed onto their
base letter (`é` is `E` with an acute above the glyph box, `ç` is `C` with a
cedilla below the baseline, `ø` is `O` with a slash across it). Anything outside
that set — `ß`, `æ`, Cyrillic, CJK — is drawn as a `.notdef` box, which is the
standard way to say "this glyph is not here". Embedding a real font remains the
honest fix, and is what the fallback exists to avoid for simple documents.

## Still unproven

The renderer does not implement these, so no fixture can prove them and content
using them is reported rather than silently guessed:

- shadings (`sh`), mesh gradients and axial/radial gradient fills;
- patterns (`PatternType 1` and 2), tiling and `/PaintType`;
- transparency groups, soft masks (`/SMask`), `/ca`/`/CA`, blend modes — the
  `gs` operator is counted as unsupported;
- text render modes other than fill (`Tr`), Type 3 fonts, and `TJ` kerning
  beyond the per-element adjustment;
- images above 8 bits per component, `/Mask`, and `/Interpolate` as anything
  other than bilinear sampling.

Those are the next coverage candidates: each needs an implementation first, then
a page in `calibration.pdf` whose right answer is a sentence, not a screenshot.
