# Reference rasters

Third-party **expected output** for pages this repository already carries, used to
arbitrate a decoder disagreement the engines alone cannot settle. `docs/CORPUS.md`
explains the tiers; this directory is not a tier — it is a second opinion.

## pdfium's expected rasters for the two JBIG2 pages (BSD-3-Clause)

`pdfium_tests` publishes the rendering its own JBIG2 decoder is expected to produce
for every file in its corpus. Two of them are pages we hold in `corpus/tier-a/`:

| Upstream file | Source |
| --- | --- |
| `third_party/jbig2/bitmap-composite-and-xnor_expected.pdf.0.png` | `pdfium_tests` at commit `2bcc4fdc9abe4c4c115c82c486ac4c46f1d474c5`, fetched 2026-11-17 |
| `third_party/jbig2/bitmap-p32-eof_expected.pdf.0.png` | the same commit |

Both PNGs are **byte-identical** (`sha256 2f89516e726e6fe5a753c71f0d3984f1c7409cfa2b3a1a1ec4035a9ab90bca71`,
1 042 bytes each), because the two pdf.js pages differ in how the JBIG2 segments are
*packed* — one inline, one through `/JBIG2Globals` — and not in what they decode to.
So one raster is committed here and both pages are compared against it:

- `bitmap-composite-and-xnor.pbm` — `P4`, 399×400, 10 950 black pixels,
  `sha256 2f61d4ecfd1139ccaa45a77177d340c6d952c05534502ec23d8c3c11eeff74b9`.

PBM rather than the upstream PNG because `crates/pdf2-render/src/png.rs` only
*encodes* and the crate has no dev-dependencies, so no PNG reader is available;
`P4` is 1 bit per pixel, which is what these rasters are, and a 15-line parser in
`crates/pdf2-render/tests/jbig2.rs` reads it.

`pdfium_tests` is BSD-3-Clause (PDFium Authors); the licence text is reproduced in
[`THIRD-PARTY-NOTICES.md`](../../THIRD-PARTY-NOTICES.md) and `corpus/` is excluded from
the published crate, so nothing here ships.

## What it is for

Our JBIG2 decoder and poppler, mupdf and Ghostscript disagree by 31.261 mean on these
pages, and three engines agreeing with each other is not proof of anything. This
raster is a **fourth implementation's** answer: our render differs from it by
19 553 pixels (12.25 %, mean |Δ| 31.24 of 255), which
`crates/pdf2-render/tests/jbig2.rs` pins so the day it changes — fix or regression —
is visible in a test rather than in a release note.
