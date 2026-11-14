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

## Independent oracle (optional, needs a system renderer)

Hand-computable pages stop scaling once text layout, transparency and fonts are
involved. For those, compare against another implementation — poppler's
`pdftoppm`, mupdf's `mutool draw` or pdfium — and treat the oracle as the
expectation:

```sh
pdftoppm -r 72 -png -f 1 -l 1 fixture.pdf /tmp/oracle     # poppler
cargo run -p pdf2-render --bin pdf2-render-golden -- --fixture fixture.pdf --out /tmp/ours.png
# compare with a tolerance: neither renderer antialiases identically
```

Tolerances that have worked elsewhere: mean absolute channel difference under
2%, and no more than 1% of pixels differing by more than 32 per channel. This is
a developer tool, not a CI gate: it needs a system package, and a mismatch has
to be judged, not merely counted.

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
| Simple font with `/FontFile2` (embedded TrueType) | glyph outlines from `ttf-parser`, placed by the PDF's encoding — proven by page 9, whose font this repository generates (`A` a filled box, `B` a box with a hole, so a probe can tell an outline from the fallback) |
| Simple font with `/Encoding` (`WinAnsiEncoding`, `/Differences`) | codes map through the encoding; `A` remapped to `/bullet` draws the bullet |
| `/Widths` and the descriptor's `/MissingWidth` | used for advances, so spacing matches the file instead of a fixed 0.6 em |
| Base-14 with no program (Helvetica and friends) | the built-in 5×7 bitmap alphabet below |
| `/Type0` (CID) — most CJK text and many modern Latin files | **not drawn**: counted as `font:Type0` in the report |
| `/Type3` | **not drawn**: `font:Type3` |
| `/FontFile` (Type 1) and `/FontFile3` (CFF/OpenType) programs | **not parsed**: the bitmap fallback is drawn and `font:FontFile`/`font:FontFile3` is reported |
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
- `/CropBox` as the visible page box, and `/UserUnit`;
- images above 8 bits per component, `/Mask`, and `/Interpolate` as anything
  other than bilinear sampling.

Those are the next coverage candidates: each needs an implementation first, then
a page in `calibration.pdf` whose right answer is a sentence, not a screenshot.
