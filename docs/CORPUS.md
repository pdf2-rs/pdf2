# The corpus run: what this library does with 23 real files

First measurement of the proof strategy in [`docs/PROOF.md`](PROOF.md): Tier A,
23 files from the pdf.js test suite, pinned and hashed in
[`corpus/manifest.json`](../corpus/manifest.json), run on 2026-11-14 with
`pdf2 1.0.0` and `pdf2-render 1.1.0`.

## The result, before the details

**All 23 files are rejected at the file header.** Not one construct in the corpus
was reached:

```
$ target/debug/pdf2 info corpus/tier-a/Embedded_font.pdf
error: corpus/tier-a/Embedded_font.pdf: malformed only PDF 2.x is supported at offset 0
```

`src/document.rs:486` requires the header version to start with `2.`, and
`src/document.rs:967` asserts that `%PDF-1.7` is an error, so this is deliberate
policy rather than a bug. It is also the most consequential fact in this survey:
the real world is PDF 1.4–1.7, and this library refuses every one of those files
before reading a single object.

The same run through the CLI and the renderer:

| Command | Files reaching the parser | Files rejected | First error |
| --- | --- | --- | --- |
| `pdf2 info` / `check` | 0 of 23 | 23 | `malformed only PDF 2.x is supported at offset 0` |
| `pdf2 text` | 0 of 23 | 23 | same |
| `pdf2-render-golden` (page 1) | 0 of 23 | 23 | same |

## What the corpus contains

Every file was chosen to exercise a construct this repository cannot generate:

| File | Bytes | Construct the upstream test targets |
| --- | --- | --- |
| `90ms_rksj_h_sample.pdf` | 1 039 | CJK text, Identity-H |
| `ArabicCIDTrueType.pdf` | 39 370 | Arabic, CID TrueType |
| `arial_unicode_ab_cidfont.pdf` | 13 110 | Arabic CID font |
| `arial_unicode_en_cidfont.pdf` | 15 779 | English CID font |
| `IdentityToUnicodeMap_charCodeOf.pdf` | small | `/ToUnicode` mapping |
| `cid_cff.pdf` | 19 970 | CID font with CFF outlines |
| `cidfont_cmap_overflow.pdf` | 94 168 | CMap edge case |
| `ContentStreamCycleType3insideType3.pdf` | 2 440 | Type 3 inside Type 3 |
| `Type3WordSpacing.pdf` | 2 693 | Type 3 metrics |
| `issue18768_type3_wide_glyph.pdf` | 1 059 | Type 3 wide glyph |
| `bug_jpx.pdf` | 5 451 | JPEG 2000 image |
| `ccitt_EndOfBlock_false.pdf` | 3 256 | CCITT image |
| `cmykjpeg.pdf` | 374 080 | CMYK JPEG |
| `ShowText-ShadingPattern.pdf` | 4 671 | shading pattern fill |
| `axial_shading_many_inputs.pdf` | 3 336 | axial shading |
| `function_based_shading.pdf` | 3 869 | function-based shading |
| `function_based_shading_cmyk.pdf` | 3 266 | CMYK shading |
| `alphatrans.pdf` | 16 910 | transparency |
| `blendmode.pdf` | 50 567 | blend modes |
| `colorkeymask.pdf` | 161 533 | `/Mask` colour key |
| `Embedded_font.pdf` | 3 323 | embedded font program |
| `TrueType_without_cmap.pdf` | 3 989 | font without a cmap |
| `encrypted-attachment.pdf` | 2 719 | encryption with an attachment |

Total 827 249 bytes; the manifest records each SHA-256 and the pdf.js commit
(`d60bc7426aa0968d5b95ff8ca3a76dbc6ce23a3a`).

## Why this cannot be classified further yet

The taxonomy the plan wanted — image filters, composite fonts, renderer reports —
needs files that reach those code paths. With the header gate closed, every
failure is the same failure, and no amount of corpus breadth changes it.

Reading PDF 1.x is therefore **not one gap among the eight** in
[`docs/GAPS.md`](GAPS.md): it is the precondition for measuring any of them.
Two consequences follow, and both are recorded there and in the round plan:

1. The scorecard gains a new top row, and the M7 spike is retargeted at it.
2. Tier B corpora (pdf.js complete, veraPDF) are pointless to fetch until a
   single Tier A file parses; the proof strategy stays at Tier A for now.

## What the spike has to measure

The next round relaxes the gate in a branch, then answers with numbers rather
than opinion:

- how many of the 23 files parse, extract and render once the header is accepted;
- which of them fail on what next — RC4/AES-128 encryption (`/Encrypt` revisions
  2–4 are PDF 1.x-era and this library implements AES-256 revision 6 only),
  CCITT/JPX/JBIG2 images (`src/filter.rs:41`), Type 0 extraction
  (`src/content.rs:253`), CFF programs, Type 3, transparency;
- what the change costs in code and risk, and whether the 2.0-only policy was
  buying anything real.

Until that measurement exists, the honest summary is: **the library reads PDF 2.0
and cannot read the corpus at all.**

## Method

```sh
# every file, three ways, 2026-11-14
for f in corpus/tier-a/*.pdf; do
  target/debug/pdf2 info  "$f"; target/debug/pdf2 check "$f"; target/debug/pdf2 text "$f"
  target/debug/pdf2-render-golden --fixture "$f" --page 1 --scale 1 --bless --out /tmp/x.png
done
```

The run is reproducible from the manifest alone; the oracle calibration promised
in `docs/PROOF.md` is not attempted here because the files never reach a
rasteriser.
