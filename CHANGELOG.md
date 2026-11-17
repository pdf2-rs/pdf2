# Changelog

All notable changes to this crate are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.0] — in progress

### Compatibility

- **The MSRV is now 1.100** (raised from 1.87). The reason is the one the previous
  release could not act on: the three decoders `docs/adr/0004-image-decoders-and-the-msrv.md`
  deferred — `hayro-jpeg2000` 0.5.0, `hayro-jbig2` 0.4.0 and `hayro-ccitt` 0.5.0 —
  declare `rust-version = "1.92"`, and a tool that cannot read JPEG 2000 or JBIG2
  is a tool that cannot open a scanned contract. `1.100` is the current stable, so
  the MSRV job also becomes the guard against adopting an API that only exists in
  1.101; `.clippy.toml`'s `msrv` moves with it, because that pin *overrides* the
  manifest and would otherwise veto the 1.88-era APIs this release starts using.
  The library still builds with `--no-default-features --features alloc` for
  `thumbv7em-none-eabihf`, verified on 1.100.
- **What the new floor unblocks is recorded, with a number**
  ([ADR 0005](docs/adr/0005-msrv-1-100.md)): JPEG 2000 and JBIG2 need `rust-version =
  "1.92"` and the floor is now 1.100, so nothing technical blocks them — measured by
  linking them, `hayro-jpeg2000` costs **+76 052 gzip** and `hayro-jbig2` **+62 495**
  (both **+137 502**) against a 356 KiB npm bundle, so rank 9 is now a budget
  decision rather than a toolchain one. ICC colour management's dependency question
  also closed: `moxcms` 0.9.2 needs 1.89, is `BSD-3-Clause OR Apache-2.0` and pure
  Rust, so what remains there is the fidelity decision. The browser and WASI budgets
  are unaffected by either, because those modules contain no renderer.
- **The 1.88–1.100 sweep is recorded and adopted where it pays**
  ([`docs/MSRV.md`](docs/MSRV.md)): one line per release with a verdict. Used:
  `as_chunks` (ten sites), let-chains (thirteen), `array_windows` (the path stroker
  and the CFF ROS test), `assert_matches!` (twelve assertions in five test files).
  Refused with the reason: `strict_*` arithmetic (it panics where this crate must
  return an error), `if let` guards (every candidate is "do nothing when the operand
  is absent", so a guard needs a second arm that says nothing), the allocator API
  (a `#![forbid(unsafe_code)]` library cannot offer it).
- **`resolver = "3"`**, the Rust-version-aware resolver that edition 2024 makes
  the default: it prefers dependency versions whose own `rust-version` fits ours, so
  a cold `Cargo.lock` cannot pick a crate that needs a newer compiler than 1.100.
  Measured, not assumed: a lock rebuilt from nothing resolves **46 packages, the
  same versions as before, with nothing added or removed** (`aes 0.8.4`,
  `cbc 0.1.2`, `miniz_oxide 0.9.1`, `sha2 0.10.9` unchanged), and the benchmark
  stays inside its 10 % gate, so no baseline was re-recorded.
- **Edition 2024.** Every crate moves, and the migration found nothing to fix: both
  `cargo fix --edition` runs (root and each member) were no-ops, so the source was
  already 2024-compatible. What the edition *unlocks* is let-chains, and clippy's
  `collapsible_if` immediately asks for them at the thirteen nested `if let` sites
  that had no `else` and no escaping binding — one chain each, shape-only. The
  rustfmt 2024 style edition then reformatted the tree in a commit of its own
  (self-imports lead their group; a macro call in tail position loses its
  semicolon), so the next diff stays readable.

## [1.3.0] — 2026-11-17

**The renderer draws the page a writer meant, and says so when it cannot.** Twenty
rounds of measured work land together: every layer the earlier releases skipped —
`/Subtype /Form` XObjects, annotation appearance streams, `/ExtGState` with its
alpha, soft masks and sixteen blend modes, shadings through `/Function` objects,
the colour spaces that tint them, Type 1 / simple CFF / Type 3 fonts, JPEG (now
including CMYK) and CCITT fax images in all three modes — plus the bounds a reader
of untrusted files needs: a decoded-bytes budget, an object ceiling and a page
ceiling, each with a typed error naming the limit. Anti-aliasing was judged before
it was built and is gated at 1×1.

The corpus is unchanged at 23 files (22 fully readable, 23 rendered) because it was
the wrong instrument for this work; what moved is the *reports*, measured on the
302-page pdf.js sample as each round landed (2026-11-16): `Do` notes 119 → 100, `gs`
38 → 0, pages that render but say nothing 49 → 10, and a page that paints nothing is
now named (`drew-nothing`) instead of passing as blank.

Tool versions: `pdf2-render` and `pdf2-cli` follow the library at 1.3.0;
`pdf2-wasm` moves to **1.2.0** (crate and npm) because the wasm blob and its
dependency pin both carry this release. `docs/DEFERRED.md` records what is
deliberately still missing, with the reason and the unblock condition for each.

### Rendering

- **Type 1 fonts draw** (`/FontFile`, `/Subtype /Type1`): `crates/pdf2-render/src/type1.rs`
  reads the PostScript program in PFA or PFB form, decrypts its `eexec` section with
  the format's fixed key (and the charstrings with `4330` unless `/lenIV` is `-1`),
  reads `/Subrs` and `/CharStrings`, and interprets the drawing operators —
  `hsbw`, `sbw`, the moveto/lineto family, `rrcurveto`, `closepath`, `callsubr`,
  `callgsubr`, `return`, `endchar`, `div` and the flex group — with hints popped and
  ignored. `seac` and `callothersubr` are refused by name rather than approximated.
  No corpus page carries a Type 1 program, so the proof is the generated
  `tests/fixtures/type1.pdf`: three glyphs, one drawn through `callsubr`, checked as
  three ink bands in `crates/pdf2-render/tests/type1.rs`, and a second test that a
  program without an `eexec` section is reported (`font:FontFile`) rather than drawn
  as nothing.

- **Two-dimensional CCITT fax images draw** (`/CCITTFaxDecode`, `/K` > 0 and
  `/K` −1): T.4 §4.2.1.2's changing-element model — the coding line is a list of
  change positions whose parity is the run's colour, `b1`/`b2` are tracked on the
  reference line as `a0` advances, and the modes are pass, horizontal and vertical
  ±3 with a backward vertical walking the list back. G4 codes every line this way;
  G3 mixed mode reads the tag bit after an EOL, and the EOL is *looked for* even
  when `/EndOfLine` is absent because writers emit it anyway. The corpus page draws
  all six of its images — `/K` −1, 0 and 1, both `/EndOfBlock` values — and reports
  every operator supported, where it reported four `Do` notes. The structure
  follows pdf.js's `ccitt.js` (Apache-2.0, pinned), recorded in
  `THIRD-PARTY-NOTICES.md`; an earlier attempt at this was withdrawn as
  plausible-but-wrong, and the difference between the two is the tracked reference
  position.

- **CMYK JPEG images draw** (`7.4.8`): a four-component `/DCTDecode` image is
  decoded as CMYK (its RGB path assumes uninverted samples and comes out black),
  the Adobe APP14 inversion is undone — unless the writer stated it as `/Decode`,
  which is the same statement and must not be applied twice — and the samples
  become RGB by the multiplicative rule. `cmykjpeg.pdf`'s photograph draws instead
  of being reported, and `tests/fixtures/cmyk-jpeg.pdf` pins four ink quadrants
  (white, cyan, red, black) to ±8 of the arithmetic. Against an independent
  decoder (Pillow) the page matches to **8 mean absolute units** on all three
  channels.

### Documentation

- **`docs/DEFERRED.md`**: one page for everything deliberately not done yet — the
  seven items out of scope for the 2026-11-16 plan (editing, forms filling and
  annotation authoring, conformance validation, JPX/JBIG2 until the MSRV moves,
  ICC colour management, tiling patterns, colour-key `/Mask`, the `Tr` clipping
  half), the ADR's non-goals, and the two constructs refused by name (CMYK DCT,
  CCITT two-dimensional coding) with the brief for whoever picks each up. Each row
  names its ledger row or ADR line, so the register and `docs/FEATURES.md` cannot
  drift apart.
- `docs/ROADMAP-NEXT.md`'s ranking now marks what shipped: ranks 8, 10d, 10e, 10g
  and 12b ✓, ranks 9, 10, 10c and 12 as half, and 10b, 11, 12's second half and 13
  as deferred with the reason.

- **Object and page ceilings** (`Error::TooMany`): `Document::set_object_limit`
  (default 1 048 576) bounds how many objects may be *held*, and
  `Document::set_page_limit` (default 131 072) bounds a page-tree walk, so a
  crafted cross-reference or `/Pages` is refused with a typed error naming the
  limit instead of costing unbounded parse time and memory. Both are settable and
  both can be lifted; `tests/bounds.rs` proves they fire and that raising them lets
  the same document through. A generated 65 000-object document measures the cost
  this bounds (~10.7 ms and 9 MB per open and page walk).
- **Type 3 fonts draw** (`9.6.5`): a Type 3 glyph is a content stream, and each
  now runs in glyph space — `/FontMatrix` first, then the text rendering matrix —
  with the font's own `/Resources`; codes reach glyph names through `/Encoding`'s
  `/Differences`, `d0`/`d1` are read, and the advance comes from `/Widths`. A
  glyph that shows text in its own font stops at the form depth cap instead of
  recursing. `Type3WordSpacing.pdf` and `ContentStreamCycleType3insideType3.pdf`
  draw and report every operator supported.
- **Simple-font CFF programs draw** (`9.9`, `/FontFile3` with `/Subtype /Type1C`):
  a bare CFF has no `cmap`, so its charset's glyph **names** are the route — the
  Top DICT's ROS says whether the charset holds CIDs or names, and a name comes
  back to a character. `ShowText-ShadingPattern.pdf` sets its text in the
  embedded Arial subset and no longer reports `font:FontFile3`. Type 1
  (`/FontFile`, `eexec`) is still reported.
- **`cs`/`scn` select colour spaces, and a shading pattern paints** (`8.6.8`,
  `8.7.4`): the content stream's colour operators now choose a tint space
  (`/Separation`, `/DeviceN`) or a `/Pattern` space, and an `scn` naming a
  `/PatternType 2` fills the path with that shading — `ShowText-ShadingPattern.pdf`
  draws its gradient swatch and reports only its simple-CFF font. Tiling patterns
  (`/PatternType 1`) are still reported.
- **`/Separation` and `/DeviceN` colour spaces are read** (`8.6.6.4`,
  `8.6.6.5`): where a shading or an image names one, its tint runs through the
  space's own `/Function` into the alternate space —
  `function_based_shading_cmyk.pdf`'s `/Spot` gradient now draws, and
  `tests/fixtures/separation.pdf` pins the arithmetic (half a tint is half ink).
  `/Pattern` colour spaces in the content stream are still reported.
- **Shadings draw** (`8.7.4`, types 1–3): axial, radial and function-based
  shadings are sampled per pixel through their `/Function`, inside the current
  clip and with the fill alpha and blend mode, so a gradient under `/ca` looks
  like the rest of the page. `axial_shading_many_inputs.pdf` and
  `function_based_shading*.pdf` — five corpus pages of gradients — now draw with
  no `sh` note. Mesh shadings (types 4–7) are reported by name.
- **`/Function` objects are read** (`7.10`): the renderer parses and evaluates
  types 0 (sampled, with bit unpacking and multilinear interpolation), 2
  (exponential), 3 (stitching, choosing by `/Bounds` and mapping through
  `/Encode`) and 4 (the PostScript calculator — Table 42's operators including
  `if`/`ifelse`, bounded to 4096 steps and 128 stack entries because a function is
  data from an untrusted file). Type 1 is refused by name. Every `/Function` in
  the five corpus shading pages parses and evaluates; nothing paints with them
  yet, since `sh` is the next step.
- **One-dimensional CCITT fax images decode** (`7.4.6`): `src/fax.rs` reads the
  bit stream, honours the whole `/DecodeParms` set (`/K`, `/Columns`, `/Rows`,
  `/EndOfLine`, `/EncodedByteAlign`, `/EndOfBlock`, `/BlackIs1`) and decodes the
  T.4 run-length lines into one-bit rows, which the image path widens to samples.
  The public `Filter` enum is deliberately unchanged — a fax stream is a bitmap
  decoder rather than a byte filter, so `Document::decode_stream` dispatches it
  after decryption, and `cargo semver-checks` stays quiet.
  `ccitt_EndOfBlock_false.pdf` draws its two `/K 0` images — legible text, which is
  the proof the tables are right — and reports the four two-dimensional ones
  rather than half-decoding them.
- **`/DCTDecode` (JPEG) images draw** (`7.4.8`): `zune-jpeg` decodes them behind
  the `dct` feature (on by default, `MIT OR Apache-2.0 OR Zlib`, MSRV 1.75), the
  size is taken from the JPEG's own header and checked against the decode budget
  before the pixels are built. `cmykjpeg.pdf` and `alphatrans.pdf` — the two
  corpus pages that used to render blank — now draw their artwork, and
  `cmykjpeg.pdf` joins the oracle cases. The dependency costs **66.8 KB gzip** on
  the npm bundle (245 656 → 312 415), so that budget moved 260 → 320 KiB with the
  measurement recorded, and the feature is optional for a size-sensitive build.
- **Anti-aliased rasterisation** (`10.7`): polygons and glyphs are filled from
  four sub-scanlines per device row with exact horizontal coverage, summed and
  blended once per pixel — so the staircase along every glyph and curve is gone.
  Measured, not asserted: at full resolution `book#1`'s mean distance to the
  oracle median falls from 3.68 to 2.58 channels, and the new
  `corpus/aa-ceilings.tsv` gate sits between the two numbers, so a renderer
  without it fails CI. Every committed golden was re-blessed.
- **The sixteen `/BM` blend modes are applied** (`11.3.5`): every mode in Table
  136 — the twelve separable ones per channel and the four non-separable ones
  over the colour triplet — uses the CSS formulae the standard refers to, composed
  as `(1-a)·Cb + a·B(Cb, Cs)` over an opaque backdrop. `blendmode.pdf`, which
  sets all sixteen, now reports nothing unsupported.
- **Luminosity soft masks are applied** (`11.6.5.2`): an `/ExtGState` `/SMask`
  with `/S /Luminosity` renders its `/G` form onto black and multiplies the
  result into the page's coverage mask, so a masked logo fades by its mask
  instead of drawing as a rectangle. `/SMask /None` — which `cmykjpeg.pdf`
  carries, and which an earlier round mistook for a mask — is no mask at all,
  and an `/S /Alpha` mask keeps the `gs` note instead of being approximated,
  because this rasteriser has no transparent canvas.
- **Constant alpha and the line state come from `/ExtGState`** (`8.4.5`):
  `/ca` and `/CA` now reach fills, strokes, text and images (the canvas already
  blended with an alpha, so this was wiring plus the state plumbing), `/BM
  /Normal` is accepted, and `/LW`, `/LC`, `/LJ`, `/ML` and `/D` are read from the
  dictionary. All of it travels with `q`/`Q`. A state that carries a `/SMask` or
  a non-normal `/BM` keeps its `gs` note — half-applying a soft mask would look
  correct and not be.
- **Annotation appearance streams draw** (`12.5.5`): a page whose `/Contents` is
  empty and whose visible content is `/Annots → /AP → /N` now renders — the
  appearance is placed by mapping its transformed `/BBox` onto the annotation's
  `/Rect`, `/AS` picks from a state dictionary, and invisible, hidden or no-view
  annotations (`/F`) are skipped. It is the shape behind 25 of the 49 sample
  pages that were blank *and reported nothing*, which was this renderer's worst
  failure mode: an empty page with no explanation.
- **Form XObjects draw** (`8.10`): `Do` on a `/Subtype /Form` stream now runs it
  as a nested content stream — the form's `/Matrix` concatenated into the CTM,
  its `/BBox` clipping what it paints, its `/Resources` replacing the current
  dictionary (and inherited when the form declares none), and nesting capped at
  `MAX_FORM_DEPTH` so a form that draws itself is reported once instead of
  followed. This is the measured coverage gap: **39.4 %** of the pages in a
  302-page sample report `Do` without it (`docs/RENDERING-QUALITY.md`).

- **An image is measured before it is decoded** (`8.9.5`): `/Width × /Height ×
  components` is compared with the document's budget *before* the stream is
  touched, so a page claiming 10 000 × 10 000 pixels over ten bytes of payload
  fails with `Error::Limit` rather than allocating what it claims. Verified by
  decoding order in `crates/pdf2-render/tests/bounds.rs`: the same stream is
  invalid Flate, and the error is about the budget, not the payload.
- **Decoded streams are bounded** (`7.4`, `7.6.1`): every stream decodes under a
  256 MiB budget (`filter::DEFAULT_DECODE_LIMIT`), settable per document with
  `Document::set_decoded_limit`, and a stream that would pass it fails with the
  new `Error::Limit` instead of allocating. Flate is capped *while* inflating, so
  the refusal costs no more memory than the budget itself. Measured before the
  change: a 1 043 364-byte file expanded to 1 073 741 824 bytes in 0.43 s — a
  decompression bomb that a browser tab or a server parsing uploads cannot
  survive. The default matches mupdf's `FZ_STORE_DEFAULT` (256 MiB); the budget
  is a new behaviour, which is why this is a patch release and not a silent fix.

Renderer work from [`docs/RENDERING-QUALITY.md`](docs/RENDERING-QUALITY.md), the
first three items of a measured plan:

- **Filling only touches the rows an outline reaches** (`10.7`). `fill_polygons`
  scanned the whole canvas for every fill, so a page of 2 000 glyphs walked
  2 000 × height rows. Bounding the scanlines to the polygon's extent takes the
  `book.pdf` page from **74.8 ms to 4.3 ms** (17×) in a release build.
- **A font program with no `cmap` is named, not guessed** (`9.6.4`). A simple
  TrueType font with no character map cannot be decoded from its codes; the page
  now reports `font:TrueType-without-cmap` instead of drawing an empty page under
  "all operators supported". Using the code as the glyph id was tried and
  rejected: pdf.js's `TrueType_without_cmap.pdf` has 141 glyph ids, no map and
  mostly empty charstrings, so the guess drew nothing there and would draw wrong
  glyphs where it drew anything.
- **A page that paints and stays empty says `drew-nothing`** (`10.7`). Every
  painting operator is counted, and a page that ran them and produced a raster of
  nothing but its background is reported instead of claiming full support. Silent
  blanks were the worst failure this renderer had: 49 of 160 blank pages in a
  300-file pdf.js sample, 25 of them annotation form streams.
- **Glyphs are parsed once, not per occurrence**: a per-font cache of
  character → glyph id and glyph id → outline (`book.pdf` at scale 0.25:
  5.4 → 2.5 ms), shared by a font's clones.

## [1.2.0] — 2026-11-15

**Text and rendering agree.** Nine roadmap items land together, and the proof is
one number: the committed corpus of 23 real files goes from **21 parsed and 14
fully readable** to **23 parsed and 22 fully readable**. Composite fonts extract
their text, CFF fonts draw, encrypted files from the last twenty years open, CJK
pages read, and the mistakes that made those fail were as often ours as the
files' — a `startxref` counting the wrong byte, a `bfrange` increment, a charset
used as a glyph id.

- **`/CropBox` is the visible page box** (`14.11.2`): the renderer rasterises the
  crop box rather than the media box, so a page with a crop box draws the region
  a viewer shows. The golden fixture `cropbox.pdf` pins it.
- **CJK text extracts through a predefined CMap** (item 5): the Adobe-Japan1
  `90ms-RKSJ-H` code → text table ships as generated data (`cargo xtask cmaps`,
  7 876 mappings, 19 967 B), so a page that encodes Shift-JIS without a
  `/ToUnicode` map extracts its text — `90ms_rksj_h_sample.pdf` now reads
  `日本語テスト`, moving the corpus to **22 of 23 fully readable**. Adobe's CMap
  resources are BSD-3-Clause: the notice travels in `THIRD-PARTY-NOTICES.md`,
  `deny.toml` allows the licence, and the wasm budget moved 150 → 160 KiB with
  the +13.5 KB gzip measured and explained in `docs/PERFORMANCE.md`. Other CMaps
  are refused by name until their data is added.
- **CFF fonts draw their outlines** (`9.6.5`, item 4): a `/CIDFontType0C`
  program — bare CFF with no OpenType container — is wrapped in the smallest
  sfnt `ttf-parser` will read, and its charset is inverted so a CID finds its
  glyph instead of being used as a glyph id. The two corpus pages that reported
  `font:CIDFontType0` (`cid_cff.pdf`, `Embedded_font.pdf`) now render. **No new
  dependency**: `skrifa` needs the same container and was measured to reject a
  bare CFF (`Invalid sfnt version 0x01000404`) — see
  `docs/adr/0003-cff-without-a-dependency.md`.
- **`Tr` (text rendering mode) is honoured** (`9.3`): modes 1, 2, 5 and 6 stroke
  the glyphs, 3 and 7 draw nothing, and the mode is saved with the graphics
  state. Invisible text layers no longer leak into renders, and `Tr 0` is no
  longer reported as an unsupported operator.
- **Encrypted files from revisions 2–4 are read** (`7.6.2`–`7.6.4`): `/V 1`,
  `/V 2` and `/V 4` — RC4 with 40- to 128-bit keys, and AES-128-CBC — with the
  file key MD5 derives from the padded password, `/O`, `/P` and the trailer's
  `/ID`, the per-object key each string and stream needs, and the owner-password
  path of Algorithm 7. `/R 5` stays refused. Two generated revision-4 fixtures
  (RC4 and AES-128) decrypt to the same object graph as their plaintext twin.
- **A `startxref` that points inside the table is repaired** (`7.5.5`): the
  nearest `xref` keyword before the hint is tried before the file is refused, so
  a writer that counted the wrong offset no longer costs the whole document.
  `tests/fixtures/startxref-hint.pdf` pins the pattern, and the corpus's last
  unparseable file now reads: **23 of 23 parse, 21 of 23 fully readable**.
- **`/StmF` and `/StrF` `/Identity` mean no ciphertext** (`7.6.5`): a file that
  declares both is legal, holds nothing encrypted, and no longer has to be
  refused for failing a password check it does not have.
- **Composite (Type0) fonts extract text** (`9.7`, `9.10.3`): a font with an
  Identity encoding and a `/ToUnicode` map now decodes — codes split by the
  CMap's code space, text from `bfchar`/`bfrange`, and CID advances from `/W` and
  `/DW` so a gap between runs becomes a space. Five corpus files that reported
  `unsupported: composite (Type0) fonts` now extract their text, moving the
  corpus from 15 to 20 fully readable of 23. CMap-encoded CJK fonts
  (`/90ms-RKSJ-H`) and composite fonts without a `/ToUnicode` map are still
  reported, by name.
- **Extracted text is in reading order** (`9.4`): the interpreter accumulates
  positioned lines and sorts them (descending `y`, then `x`) before joining, so a
  page painted out of order reads top to bottom. The corpus page
  `ShowText-ShadingPattern.pdf`, painted 2, 1, 4, 3, now extracts 1, 2, 3, 4. The
  public API is unchanged — text only, no spans — and
  `docs/adr/0002-text-extraction-shape.md` records that decision, its limits and
  what would revisit it.
- **A free entry's generation above 16 bits is clamped** (`7.5.8`): the brain of
  the xref table's free-list head is never used for a lookup, and some writers
  say 65536 where the standard says 65535; the file now reads instead of failing
  with `malformed cross-reference generation`. The corpus's
  `ContentStreamCycleType3insideType3.pdf` and the new fixture
  `xref-generation.pdf` carry the pattern.
- **`/UserUnit` is honoured** (`7.7.3.4`): `Document::page_user_unit` reads the
  inheritable attribute (defaulting to 1) and the renderer multiplies its scale
  by it, so a page that declares a unit scale rasters at the size the file
  intends.
- **Extraction, rendering and the corpus are checked by jobs that can fail**:
  the oracle job now also times each case against `corpus/render-budget.tsv`
  (`docs/PERFORMANCE.md` §Render time), and a weekly Tier B job publishes a
  failure histogram over pdf.js's and veraPDF's trees — 92.0% readable on a
  200-file sample (`docs/CORPUS.md` §Tier B).

Released together, each requiring `pdf2` 1.2.0 or newer: **`pdf2-render` 1.2.0**,
**`pdf2-cli` 1.2.0**, **`pdf2-wasm` 1.1.0** and npm `pdf2-wasm` 1.1.0. `pdf2` is
the only crate whose public API changed shape is the renderer's behaviour; the
library's own API grew two methods (`Document::page_user_unit`,
`Encryption::new_with_id`) and changed none.

## [1.1.1] — 2026-11-14

Tool releases that pick up the PDF 1.x reader: `pdf2-render` 1.1.1,
`pdf2-cli` 1.1.0, `pdf2-wasm` 1.0.5 and npm `pdf2-wasm` 1.0.8 all require
`pdf2` 1.1.0 or newer, so a fresh resolve gets a renderer, a CLI and a wasm
build that open PDF 1.x files instead of refusing them at the header.

## [1.1.0] — 2026-11-14

**Reads PDF 1.x.** The header check accepted only `%PDF-2.x`, so the library
refused every real-world file at byte 0 — the corpus run in `docs/CORPUS.md`
rejected 23 of 23. The gate now accepts `1.x` and `2.x` (other versions are still
refused), which is the whole change: no parser behaviour is version-gated
elsewhere.

Measured on the same corpus (`cargo xtask corpus`): **21 of 23 files parse and
render, 14 are fully readable**; the nine remaining failures are pre-existing
gaps, now measurable — composite (Type 0) extraction on 7 files, one
cross-reference generation edge case, and one encrypted file (the 1.x-era
RC4/AES-128 revisions are not implemented).

### `pdf2-render` 1.1.0

Composite (Type 0) fonts render: two-byte codes from an Identity CMap, CIDs
mapped to glyph ids through `/CIDToGIDMap` (identity or a stream), advances from
`/W` with `/DW` as the default, and glyph outlines from the embedded TrueType
program. This is the font family most CJK and many modern Latin documents use,
and it used to be reported as unsupported.

`tests/fixtures/book.pdf` sets six paragraphs of public-domain prose in an
embedded Crimson Text subset (OFL, committed with its licence), typeset by the
fixture generator from the font's own advances: the embedded-TrueType path
exercised the way a reader would judge it.

The fixture carries the proof rather than a promise: the generator writes a real
TrueType program whose `A` is a box and whose `B` is the same box with a hole, one
page embeds it as a simple font, and two more draw `A B A` from two-byte codes —
the last one through a `/CIDToGIDMap` stream that swaps the glyphs, so the same
codes draw `B A B`.

Still unsupported, and now reported by reason: `/CIDFontType0` (CFF programs),
CMap streams other than Identity, and composite fonts with no embedded program.
Text *extraction* from composite fonts needs `ToUnicode` CMaps and is the next
step.

## [1.0.4] — 2026-11-14

- `renderPageToCanvas` returns the renderer's report as JSON (`{"reason": count}`,
  empty when everything was drawn), so a page that is blank because its font is
  unsupported can say so instead of looking broken.
- The npm package keeps the name `pdf2-wasm`: npm refuses `pdf2` with
  `403 Package name too similar to existing packages pbkdf2,pbf,pm2,pify`, and
  only a scoped name could change that. `publishConfig.provenance` also goes,
  because it only resolves under CI's OIDC and fails a local publish with
  `provider: null`; the tag path still publishes with provenance.
- The demo imports the wasm module with a build stamp from `build.json`:
  GitHub Pages serves fixed filenames with a cache lifetime, and a cached wasm
  from an older build drew blank pages where the current one draws text.

## [1.0.2] — 2026-11-14

Text gets the document's own metrics, and fonts that cannot be drawn are said out
loud instead of being approximated:

- **`/Widths` and `/MissingWidth`** are used for advances, so a base-14 font
  spaces its text the way the file asks instead of a uniform 0.6 em.
- **Unsupported font families are reported**: `/Type0` (CID) and `/Type3` are not
  painted at all, and embedded Type 1 (`/FontFile`) and CFF (`/FontFile3`)
  programs are painted with the fallback *and* counted, so a page that could not
  be drawn says so through `Report`.
- `tests/fixtures/calibration.pdf` gained a `/Widths` page and an unsupported
  font page, both pinned by probes and goldens; `docs/RENDERING.md` has the
  support matrix.
- The npm package is published as **`pdf2`** (1.0.6) rather than `pdf2-wasm`:
  `import init, { open } from 'pdf2'`. The old name is deprecated and points
  here; the crate keeps its name.
- npm `pdf2-wasm` 1.0.4 is published as a package root — wasm, glue, TypeScript
  declarations, manifest, README and licences — instead of a `demo/` subtree,
  and ships both licence texts.

## [1.0.1] — 2026-11-14

Renderer bug fixes, found by the new calibration fixture rather than by the
golden images, which had nothing curved in them:

- **Curves.** `c`, `v` and `y` took their end point from the wrong operands, so
  every Bézier was drawn to its last control point; curves are now the curve.
- **`/Decode`** is applied to image samples, including `/Indexed` index ranges.
- **Joins.** Miter, round and bevel are drawn as asked, and the miter limit
  `M` is honoured with the standard cut-back to a bevel.
- **`/Rotate`** turns the page and swaps the canvas dimensions for 90/180/270.

`docs/RENDERING.md` maps every covered clause to the page and assertion that
proves it, and lists what is still unimplemented.

## [1.0.0] — 2026-11-14

The 1.0 release. The public API is frozen: `1.x` changes no signatures and no
documented behaviour.

### Contents

- `pdf2` 1.0.0 — PDF 2.0 reader and writer: classic and stream cross-references,
  object streams, Flate/LZW/ASCII/RunLength filters with predictors, the
  document model with inherited page attributes and XMP metadata, text
  extraction for simple fonts, the writer (`Document::new()` … `save()`), and
  AES-256 `/V 5 /R 6` decryption.
- `pdf2-render` 1.0.0 — paths, fills, strokes, dashes, clips, transforms, text
  and image XObjects, pinned by golden images.
- `pdf2-cli` 1.0.0 — `info`, `pages`, `text`, `check` with `--json`.
- `pdf2-wasm` 1.0.0 — the same library and renderer in the browser, with a
  GitHub Pages demo; also on npm as `pdf2-wasm` (`npm install pdf2-wasm`).

### Compatibility

The API, the fixture corpus and the renderer goldens are unchanged since
`0.7.0-rc.1`. Semver from here covers the public API and the file-format
behaviour documented in `docs/SPEC-MAP.md`; see the README's compatibility
policy for what it does not cover.

## [0.7.0-rc.1] — 2026-11-14

First release candidate. **The public API is frozen from this release**: after
it, only bug fixes and internal changes until 1.0.0.

### Changed

- Version scheme: the library jumps to `0.7.0-rc.1` so that `1.0.0` is the next
  minor; the tools in this repository move to `0.1.0-rc.1` in step
  (`pdf2-render`, `pdf2-cli`, `pdf2-wasm`).

### Compatibility

Everything from `0.2.0-alpha.2` is unchanged: no public item was removed or
renamed, and the fixture corpus plus the renderer goldens are byte-identical.
See `docs/MIGRATING.md` for the alpha.1 → alpha.2 feature notes.

## [0.2.0-alpha.2] — 2026-11-14

The library gained its `no_std` build and a public encoding API; the rendering,
CLI and WebAssembly crates were released on top of it.

### Added

- `pdf2::Encoding`: simple-font code → character mapping (`StandardEncoding`,
  `WinAnsiEncoding`, `/Encoding` names and dictionaries, `/Differences`, glyph
  names), shared by text extraction and rendering.
- Feature flags: `std` (default) and `alloc`; the crate builds as `no_std` with
  `core` + `alloc` on bare-metal and wasm targets. `Document::open(&[u8])` is
  the entry point without a file system.

### Changed

- `flate2` was replaced by `miniz_oxide`, dropping a dependency and the last
  `std::io` use in the library.
- Dictionary and xref lookups use `alloc`-only collections (`BTreeMap`,
  `BTreeSet`), removing the last `std`-only container from the core.

### Fixed

- `classic-xref.pdf` declared no `/Resources` for the page it repaints; the
  content stream's `/F1` is now declared.

## [0.2.0-alpha.1] — 2026-11-14

First release of the PDF 2.0 implementation. The crate is still alpha: the
read side is broad and fixture-tested, the write side is deliberately small,
and the public API may change before 0.2.0.

### Reading

- Object model for every base type, parsed and re-serialized byte-for-byte,
  with correct escaping for names, literal strings and hex strings.
- Tokenizer and parser for direct and indirect objects, including `stream` …
  `endstream` payloads delimited by `/Length`.
- Classic cross-reference tables with subsections, free entries and `/Prev`
  chains; cross-reference streams (`/Type /XRef` with `/W` and `/Index`);
  object streams (`/ObjStm`); hybrid files (`/XRefStm`).
- Lazy object fetching with a cache.
- Filters: FlateDecode (zlib and raw DEFLATE), LZWDecode with `/EarlyChange`,
  ASCIIHexDecode, ASCII85Decode, RunLengthDecode, plus `/Filter` and
  `/DecodeParms` arrays, PNG 10–15 and TIFF 2 predictors, and indirect
  `/Length`.
- Document model: catalog, page tree with inherited `/MediaBox`, `/CropBox`,
  `/Rotate` and `/Resources`, page content streams, and XMP `/Metadata`.
  `/Count` is treated as a hint and never trusted.
- Content-stream interpretation and text extraction for simple fonts
  (`StandardEncoding`, `WinAnsiEncoding`, `/Differences`), with `BT`/`ET`,
  `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tj`, `TJ`, `'` and `"`; inline images
  are skipped.
- AES-256 decryption: standard security handler `/V 5 /R 6`, user and owner
  passwords, Algorithm 2.B password hashing, per-object-string and stream
  decryption honouring `/EncryptMetadata`, and the `/Perms` tamper check.
  Deprecated revisions (RC4, AES-128, `/R 5`) are refused with an error.
- No `unsafe` (`#![forbid(unsafe_code)]`) and no panics on malformed input:
  `tests/robustness.rs` opens every fixture from every truncation all the way
  through sampled mutations and deletions.

### Writing

- `Document::new()` … `save()` emits a `%PDF-2.0` file with a Flate-compressed
  `/ObjStm`, a `/Type /XRef` cross-reference stream with recorded byte offsets,
  XMP `/Metadata`, and no `trailer` or `Info` dictionary.
- `add_page` appends pages with a content stream and a shared Helvetica `/F1`
  resource; `save_to_path` writes to disk.

### Tooling

- Nine committed fixtures, six generated by `tests/fixtures/make_fixtures.py`
  and the encryption pair by `examples/make_encrypted_fixture.rs`; both are
  deterministic.
- `docs/SPEC-MAP.md` maps ISO 32000-2 clauses to modules.
- CI on stable, beta and MSRV 1.87; publishing to crates.io through trusted
  publishing on a `v*` tag.

[0.2.0-alpha.1]: https://crates.io/crates/pdf2/0.2.0-alpha.1
[0.2.0-alpha.2]: https://crates.io/crates/pdf2/0.2.0-alpha.2
[0.7.0-rc.1]: https://crates.io/crates/pdf2/0.7.0-rc.1
[1.0.0]: https://crates.io/crates/pdf2/1.0.0
