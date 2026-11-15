# The corpus run: what this library does with 23 real files

First measurement of the proof strategy in [`docs/PROOF.md`](PROOF.md): Tier A,
23 files from the pdf.js test suite, pinned and hashed in
[`corpus/manifest.json`](../corpus/manifest.json), run on 2026-11-14 with
`pdf2 1.0.0` and `pdf2-render 1.1.0`.

## The result, before and after opening the header gate

**Before** (2026-11-14, `pdf2 1.0.0`): all 23 files rejected at the header, not one
construct reached.

```
$ target/debug/pdf2 info corpus/tier-a/Embedded_font.pdf
error: … malformed only PDF 2.x is supported at offset 0
```

`src/document.rs:486` required the header version to start with `2.`, and
`src/document.rs:967` asserted that `%PDF-1.7` is an error. Deliberate policy —
and the single most consequential fact in the survey, because the real world is
PDF 1.4–1.7.

**After** the spike relaxed that one condition (accept `1.x` and `2.x`, refuse
everything else; no other code is version-gated), measured with
`cargo xtask corpus`:

| Measure | Files | Note |
| --- | --- | --- |
| parse (`pdf2 info`) | 23 of 23 | every file opens |
| fully readable (`check` + `text`) | 21 of 23 | two remain, both composite fonts with no text to find (below) |
| page 1 rendered | 23 of 23 | every file draws |

Failure taxonomy, and every class maps to a gap already in the scorecard:

| Class | Files | Which |
| --- | --- | --- |
| `unsupported: CMap-encoded composite (Type0) fonts` | 1 | `90ms_rksj_h_sample.pdf` — needs the `/90ms-RKSJ-H` resource, which is item 5's CMap data |
| `unsupported: composite (Type0) fonts without a /ToUnicode map` | 1 | `arial_unicode_en_cidfont.pdf` — its embedded program carries neither a `cmap` nor a `post` table, so there is no mapping to find |


**History, so the movement is auditable**: the first run stopped all 23 files at
the header (`only PDF 2.x is supported`); opening the header gate gave 21 parsed
/ 14 fully readable with seven composite-font refusals; the xref generation edge
(item 6) gave 22 / 15; composite extraction (item 2) gave 22 / 20; the encryption
work (item 3) gave **23 / 21** — its `startxref` repair and its acceptance of
`/Identity` crypt filters were the last two blockers, and neither was the RC4 /
AES-128 support the file looked like it needed. Two failures remain, both
composite fonts with no mapping to extract.

Read that table as the first honest measurement of the library on real files: the
renderer is much further along than the extractor, and the three failure classes
are exactly gaps 1, 9 and 7 of `docs/GAPS.md`.

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

## What the measurement changed

- **The gate was the whole blocker.** One condition plus its test; nothing else
  in the tree branches on the header version. The 2.0-only policy was buying
  nothing except unmeasurability.
- **Type 0 extraction is now confirmed by real files**, not by inference: seven
  of the twenty-three, and all seven render — the renderer/extractor split the
  ADR's D1 is about, visible in one table.
- **A new gap surfaced**: `/Encrypt` revisions 2–4 (RC4 and AES-128) are 1.x-era
  and unimplemented; the library has AES-256 revision 6 only. It goes into
  `docs/GAPS.md` with this file as evidence.
- **Images are not the blocker they were assumed to be**: `bug_jpx.pdf`,
  `ccitt_EndOfBlock_false.pdf`, `cmykjpeg.pdf` and `colorkeymask.pdf` all parse
  and render, because the renderer draws unsupported images as blanks and
  reports them rather than failing the page.
- **Tier B is now worth fetching**: a batch run over 690 pdf.js files would
  measure the same classes at scale.

## Reproducing

```sh
cargo xtask corpus
```

The chore builds the CLI and the renderer, runs `check`, `text` and page 1 of
every Tier A file, and prints the counts and the failure classes above.

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

### Reading order, measured on one page

`ShowText-ShadingPattern.pdf` paints its four lines **2, 1, 4, 3** — the second
line is reached with a positive `Td` from the first, so paint order is not
reading order. `pdf2 text` returns **1, 2, 3, 4** with the words' spaces intact,
because extraction now orders lines geometrically (descending `y`, then `x`)
rather than by the order the content stream draws them. The decision and its
limits (columns, intra-line order, ligatures) are
[`docs/adr/0002-text-extraction-shape.md`](adr/0002-text-extraction-shape.md).

### Composite extraction, measured file by file

The five files that moved from reported to readable when `/ToUnicode` extraction
landed, and what they now return — the check is that the text is the file's, not
that a call returns:

| File | Text the map produces | Note |
| --- | --- | --- |
| `Embedded_font.pdf` | `除` | one code, `A911 → U+9664`, from a `bfchar` over an embedded subset |
| `cid_cff.pdf` | ` processor reference chart ` | a CFF CID font; the padding spaces are in the file |
| `cidfont_cmap_overflow.pdf` | `㐂㐄㐆㐈㐊…` (100 ideographs) | a `bfchar` table of 100 codes → `U+3402`, `U+3404`, … — the file that tests a destination overflow, which the parser's carry handles |
| `ArabicCIDTrueType.pdf` | `ﺔﻴﺑﺮﻌﻟا طﻮﻄﳋا عاﻮﻧا`, four times | one line per embedded font, in the presentation forms the file maps to. Order is the file's glyph order: reordering right-to-left is a layout job, not extraction's |
| `arial_unicode_ab_cidfont.pdf` | `￿￿￿￿` | **not a bug**: the file's own `/ToUnicode` maps four codes to `<ffff>` deliberately. Faithful extraction of a placeholder is `U+FFFF`, and silencing it would be a lie about the file |

### `encrypted-attachment.pdf`, the last file that did not open

It looked like an encryption gap and was not one. The file is `/V 5 /R 6` — the
revision this library already read — with two ordinary defects and one legal
oddity:

- its `startxref` says **2242** where the `xref` keyword is at **2223**, so the
  pointer lands inside the table (repaired by searching back for the keyword);
- `/StmF` and `/StrF` are both **`/Identity`**, which is the standard way of
  saying *nothing in this file is encrypted* — refusing it for failing a password
  check is refusing a file that has no ciphertext.

Both were real reader bugs on our side, and both are now pinned by fixtures
(`startxref-hint.pdf`, and the `/Identity` case in `src/crypto.rs`'s tests).
