# Competitors: what exists, what it does, and where we stand

Checked **2026-11-14**. Every number below came from a command in this file's
provenance section, not from memory; every capability claim points at a source
tag. Cells marked `?` are the ones this survey could not verify — they are listed
under "Not verified" with what would settle them.

This file is the input to the gap scorecard (M4) and the ADR (M5). It is not the
ADR: nothing here decides what we build.

## Adoption

| Implementation | Version | Downloads (recent) | Stars | Last release / push | Licence | Source |
| --- | --- | --- | --- | --- | --- | --- |
| `hayro` | 0.9.0 | 5 384 557 (3 891 211) | 868 | 2026-11-02 / 2026-11-14 | Apache-2.0 | [cr] [gh] [h1] |
| `pdf` (pdf-rs) | 0.10.0 | 656 166 (80 967) | 1 724 | 2026-03-02 / 2026-11-09 | MIT | [cr] [gh] [p1] |
| `lopdf` | 0.47.0 | 30 289 120 (14 766 962) | 2 294 | 2026-11-04 / 2026-11-12 | MIT | [cr] [gh] [l1] |
| `printpdf` | 0.12.9 | 4 070 864 (1 776 259) | — | 2026-11-11 | MIT | [cr] [pr1] |
| `pdfium-render` | 0.9.5 | 3 954 648 (1 990 705) | 724 | 2026-11-01 / 2026-11-14 | MIT/Apache | [cr] [gh] [fr1] |
| `mupdf` (bindings) | 0.8.0 | 1 786 662 (219 902) | — | 2026-06-22 | AGPL | [cr] [mu1] |
| `pdf-extract` | 0.12.1 | 8 236 722 (4 243 547) | 608 | 2026-09-16 | **none declared** | [cr] [gh] [pe1] |
| `pdf.js` | — (JS) | — (npm `pdfjs-dist`) | 54 134 | — / 2026-11-14 | Apache-2.0 | [gh] [js1] |
| `pdfium` (C++) | — | — | 541 | — / 2026-11-14 | BSD-3 (Chromium; GitHub reports NOASSERTION) | [gh] [fr1] |
| `pdf2` (this repo) | 1.0.0 | 1 664 (115) | — | 2026-11-14 | Unlicense OR MIT | [cr] [repo] |

Reading it: the Rust ecosystem's demand is concentrated in **editing**
(`lopdf`, 30M) and **text extraction** (`pdf-extract`, 8.2M), while the newest
and fastest-growing implementation is a **renderer** (`hayro`, 3.9M in 90 days).
Our own adoption is three orders of magnitude below any of them, so the plan has
to win on capability and packaging, not on switching costs.

## Capabilities

Legend: ✓ supported · ~ partial, experimental or with a caveat · ✗ absent ·
? unverified (see "Not verified").

| Capability | hayro | pdf-rs `pdf` | lopdf | printpdf | pdfium-render | mupdf | pdf-extract | pdf.js | pdfium | pdf2 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Read / parse (xref streams, object streams) | ✓ [h1] | ✓ [p1] | ✓ [l1] | ~ [pr1] | ✓ [fr1] | ✓ [mu1] | ✓ [pe1] | ✓ [js1] | ✓ [fr1] | ✓ [repo] |
| Edit an opened document | ✗ [h1] | ~ [p1] | ✓ [l1] | ~ [pr1] | ✓ [fr1] | ✓ [mu1] | ✗ [pe1] | ✗ [js1] | ✓ [fr1] | ✗ [w1] |
| Write / create from scratch | ✗ [h1] | ~ [p1] | ✓ [l1] | ✓ [pr1] | ✓ [fr1] | ✓ [mu1] | ✗ [pe1] | ✗ [js1] | ✓ [fr1] | ✓ [w1] |
| Text extraction | ✗ [h1] | ✗ [p1] | ✗ [l1] | ✗ [pr1] | ✓ [fr1] | ✓ [mu1] | ✓ [pe1] | ✓ [js1] | ✓ [fr1] | ~ [c1] |
| Raster rendering | ✓ [h1] | ✗ [p1] | ✗ [l1] | ~ SVG only [pr1] | ✓ [fr1] | ✓ [mu1] | ✗ [pe1] | ✓ [js1] | ✓ [fr1] | ~ [r1] |
| Simple fonts (Type 1 / TrueType, embedded) | ✓ [h1] | ✓ [p3] | ✗ outlines [l2] | ~ read, ✓ write [pr1] | ✓ [fr1] | ✓ [mu1] | ✓ [pe1] | ✓ [js1] | ✓ [fr1] | ✓ [r1] |
| Composite fonts (Type 0 / CID, CJK CMaps) | ✓ [h2] | ? [p3] | ✗ [l2] | ✗ [pr1] | ✓ [fr1] | ✓ [mu1] | ~ [pe1] | ✓ [js1] | ✓ [fr1] | ~ [r2] |
| Images: JPEG / JPX / CCITT / JBIG2 decode | ✓ [h1] | ~ JPEG only [p2] | ✗ [l2] | ~ decode experimental [pr1] | ✓ [fr1] | ✓ [mu1] | ✗ [pe1] | ✓ [js1] | ✓ [fr1] | ✗ [f1] |
| Encryption (RC4 / AES, R6) | ✓ [h3] | ✓ [p3] | ✓ [l2] | ✗ [pr1] | ✓ [fr1] | ✓ [mu1] | ~ via lopdf [pe1] | ✓ [js1] | ✓ [fr1] | ✓ [repo] |
| No C dependencies / `no_std` / wasm-friendly | ✗ [h1] | ✗ [p2] | ✗ [l1] | ✗ [pr1] | ✗ [fr1] | ✗ [mu1] | ~ [pe1] | n/a (JS) [js1] | ✗ [fr1] | ✓ [repo] |

What the table says, before any decision is taken:

- **One crate that reads, extracts, renders and writes** exists nowhere in Rust:
  hayro renders, lopdf edits, pdf-extract extracts, printpdf writes, and pdf-rs
  splits rendering and text into separate repositories.
- **Our only uncontested column is the last one** (no C, `no_std`, wasm), and it
  is worth little on its own: it makes us usable where others are not, not better
  where they are.
- **Our two widest gaps are the ecosystem's two biggest jobs**: image codecs
  (`src/filter.rs:41` refuses JPEG/JPX/CCITT/JBIG2) and text extraction for
  composite fonts (`src/content.rs:253`). `pdf-extract`'s 8.2M downloads with 78
  open issues say which of them has the most users behind it.
- **Editing is the largest demand we do not serve at all** (`src/writer.rs` only
  builds new files), and lopdf's 30M downloads are the evidence — but lopdf is
  also the most entrenched competitor, so it is the most expensive to take on.
- **Rendering is the most contested**: hayro is read-only, but it covers ground
  we do not (image codecs, shadings, transparency) and is improving faster than
  we can chase it.
- **A licence is itself a differentiator**: the most downloaded extractor
  (`pdf-extract`, 8.2M) ships **no licence file at all**, and `mupdf` is AGPL.
  A dual-licensed crate with a written licence file is a reason to pick us that
  has nothing to do with features.

## Not verified

- hayro's text-extraction API: `hayro` is documented as a rasteriser; whether
  `hayro-svg` or `hayro-interpret` offers glyph-level extraction was not checked
  against their public API.
- pdf-rs's composite-font depth: `pdf/src/font.rs` names `Type0`,
  `CIDFontType0/2`, but whether CMaps and CJK are implemented was not exercised.
- `mupdf`'s and `pdfium-render`'s wasm story: neither README claims it; pdfium
  can be built for wasm in other packaging, which was not checked.
- `printpdf`'s read path: its README calls font and image read support
  "experimental"; how much of a real document it can round-trip is untested here.
- Licence of `mupdf`: the crate is AGPL-3.0 upstream; a wrapper cannot relicense
  it, so any comparison of "free" must say so.

## Provenance

Commands, run 2026-11-14 from the repository root:

```sh
for c in hayro pdf lopdf printpdf pdfium-render mupdf pdf-extract genpdf \
         pdf-writer typst pdf2 pdf2-render pdf2-cli pdf2-wasm; do
  curl -s "https://crates.io/api/v1/crates/$c" -H 'User-Agent: competitor-scan'
done
for r in LaurenzV/hayro pdf-rs/pdf J-F-Liu/lopdf ajrcarey/pdfium-render \
         jrmuizel/pdf-extract mozilla/pdf.js; do gh api "repos/$r"; done
gh api repos/LaurenzV/hayro/contents/hayro-syntax/src/crypto
gh api repos/pdf-rs/pdf/contents/pdf/src
gh api repos/J-F-Liu/lopdf/contents/src
```

Sources referenced in the tables:

- [cr] <https://crates.io/api/v1/crates/{name}> — version, downloads,
  `recent_downloads` (90 days), `updated_at`, description, repository.
- [gh] <https://api.github.com/repos/{owner}/{repo}> — stars, `open_issues_count`,
  `pushed_at`, licence, description.
- [h1] <https://github.com/LaurenzV/hayro> `README.md` — read-only design, 1 600+
  visual regression PDFs, remaining gaps (robustness, performance, API).
- [h2] `hayro/Cargo.toml` — `embed-fonts`, `embed-cmaps` (61 predefined CMaps,
  ~250 KB brotli).
- [h3] `hayro-syntax/src/crypto/` — `aes.rs`, `md5.rs`, `rc4.rs`,
  `sha256.rs`, `sha384.rs`, `sha512.rs`.
- [p1] <https://github.com/pdf-rs/pdf> `README.md` — "Modifying and writing PDFs
  is still experimental"; rendering in `pdf_render`, text in `pdf_text`.
- [p2] `pdf/Cargo.toml` — `zune-jpeg` (JPEG), `aes`, `md5`, `sha2`, `libflate`;
  `std` not optional.
- [p3] `pdf/src/` — `crypt.rs`, `font.rs`, `content.rs`, `encoding.rs`,
  `repair.rs`, `xref.rs`; `font.rs` enumerates `Type0`, `Type1`, `TrueType`,
  `CIDFontType0`, `CIDFontType2`.
- [l1] <https://github.com/J-F-Liu/lopdf> `README.md` — manipulation, Rust 1.85+,
  `wasm_js` feature for encryption.
- [l2] `lopdf/src/` — `encryption.rs`, `cmap_section.rs`, `font.rs`, `filters/`,
  `xobject.rs`; no renderer or text-extraction module.
- [pr1] <https://github.com/fschutt/printpdf> `README.md` — create/read/write,
  page rendering to SVG, font/image read "experimental", no encryption mention.
- [fr1] <https://github.com/ajrcarey/pdfium-render> `README.md` — "Pdfium can
  render pages in PDF files to bitmaps, load, edit, and extract text and images
  from existing PDF files, and create new PDF files from scratch."
- [mu1] <https://github.com/messense/mupdf-rs> `README.md` — read/edit/write,
  render, text with bounding boxes, annotations and forms, encrypted files.
- [pe1] <https://github.com/jrmuizel/pdf-extract> `README.md` — content
  extraction over lopdf.
- [js1] <https://github.com/mozilla/pdf.js> and its comparator harness
  <https://github.com/mozilla/pdf.js.comparator>.
- [repo] this repository: `README.md`, `src/filter.rs:41`, `src/content.rs:253`,
  `src/writer.rs:66-162`, `docs/RENDERING.md` ("Still unproven"), and the
  registry check of the four published crates plus npm.
- [w1] `src/writer.rs` — `Document::new()`, `add_page()`, `save()`; no API that
  opens and alters an existing file.
- [c1] `src/content.rs:253` — `Error::Unsupported` for composite (Type 0) fonts in
  text extraction.
- [r1] `docs/RENDERING.md` — paths, curves, fills, strokes, clips, transforms,
  `/Rotate`, simple fonts with `/Widths`, embedded TrueType, images with
  `/Decode`, `/Indexed`, `/ICCBased`.
- [r2] `docs/RENDERING.md` font matrix — `/Type0` + `/Identity-H` +
  `CIDFontType2` with `/FontFile2`; CMap streams other than Identity and
  `CIDFontType0` are reported, not drawn.
- [f1] `src/filter.rs:41` — `/DCTDecode`, `/JPXDecode`, `/CCITTFaxDecode`,
  `/JBIG2Decode` return "image filters are out of scope".
