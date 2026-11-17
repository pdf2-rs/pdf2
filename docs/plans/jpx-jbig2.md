# JPEG 2000 and JBIG2 decoding

Deferred item: **JPEG 2000 and JBIG2 decoding** (`docs/DEFERRED.md` §1, rank 9). Both
filters are reported today — `crates/pdf2-render/src/image.rs` dispatches only
`/DCTDecode` (`is_dct`), so a `/JPXDecode` or `/JBIG2Decode` image paints nothing and the
page says so. The corpus has one page of each kind (`bug_jpx.pdf`, and the pdf.js
`bitmap-*` family for JBIG2) and the decoders are written, maintained and permissively
licensed.

## Clause

**`7.4.9` JPXDecode** and **`7.4.7` JBIG2Decode**, both filters over the image
dictionaries of `8.9`. `7.4.7` adds the one structural wrinkle that a plan must not miss:
a `/JBIG2Decode` stream may carry `/DecodeParms /JBIG2Globals`, a *separate* stream of
shared segments that must be decoded before the image's own.

Both numbers are confirmed against the published 2020 edition and listed in
`docs/SPEC-MAP.md`'s verified table.

## Verdict

The first measurement is **history**, recorded in
[`docs/adr/0005-msrv-1-100.md`](../adr/0005-msrv-1-100.md): a 1.3.0 spike that linked
both crates behind temporary features, reached them from `Image::from_stream`,
measured the npm bundle and reverted. It is kept because it shows what the decoder
*choice* costs; the numbers the decision rests on are the 1.4.0 ones under "Option B"
below.

| Configuration (1.3.0, historical) | npm bundle gzip | Δ |
| --- | --- | --- |
| baseline (1.3.0) | 361 056 | — |
| `hayro-jbig2` 0.4.0, `default-features = false, features = ["std"]` | 423 551 | +62 495 |
| `hayro-jpeg2000` 0.5.0, same | 437 108 | +76 052 |
| both, same | 498 558 | +137 502 |
| both, default features (`std`+`image`+`simd`) | 498 939 | +137 883 |

Licences are on `deny.toml`'s allow-list (`Apache-2.0 OR MIT`), the MSRV clears the 1.100
floor (both declare 1.92), and `image` costs 381 bytes because its `ImageDecoder`
implementation is unreachable from a direct `decode` call — so an integration takes
`default-features = false, features = ["std"]`.

**The budget decision was taken on 2026-11-17: option B**
([ADR 0007](../adr/0007-decoder-features.md)). Both decoders ship as non-default
features of `pdf2-render`, `pdf2-wasm` passes them through without turning them on,
and the demo site gets a second, decoder-enabled artefact with its own budget. The
three options and why the other two lost:

| Option | What changes | Cost to the browser |
| --- | --- | --- |
| **A. Raise the budget** — rejected | `crates/pdf2-wasm/check.mjs` (`BUDGET_GZIP` 364 → ~493 KiB) and the budget paragraph in `docs/PERFORMANCE.md` | +138 481 gzip on every page load, for two codecs most PDFs never use |
| **B. A non-default feature** — **chosen** | `crates/pdf2-render/Cargo.toml` (`jpx`/`jbig2` features), `crates/pdf2-wasm/Cargo.toml` (pass-through, off by default), `crates/xtask/src/main.rs` (`wasm_package` builds a second bundle), `crates/pdf2-wasm/check.mjs` (a budget per artefact), `crates/pdf2-wasm/demo` (which bundle to load), `.github/workflows/ci.yml` (build and size-check both in the `wasm package` job), `docs/PERFORMANCE.md` | nothing on the default bundle; a second artefact to build and test |
| **C. JPX only, JBIG2 later** — rejected | as B, with only `jpx` enabled | +76 705 gzip, and it leaves the scanned-document case (JBIG2) broken while the feature machinery is already built |

Re-measured on the 1.4.0 tree before choosing (raw commands from
`docs/PERFORMANCE.md`, gzip level 9, the same `wasm-bindgen`): no decoders **365 734**,
`jpx` **443 257** (+77 523), `jbig2` **429 879** (+64 145), both **506 481** (+140 747) —
re-measured once the decode paths were wired, since the decision-time numbers priced a
header-only link. A first attempt that parsed only codestream headers measured +23 410 for
both — the decode path was dead-code eliminated — so the numbers above come from a
build that decodes before it refuses, which is the shape the rounds below keep.

**The browser and WASI smoke budgets are unaffected by any of them**, because
`cargo tree -p pdf2-wasm-smoke` contains no `pdf2-render` — checked, not assumed.

## Design

1. **Features** (`crates/pdf2-render/Cargo.toml`): `jpx = ["dep:hayro-jpeg2000"]`,
   `jbig2 = ["dep:hayro-jbig2"]`, both off by default, each dependency
   `default-features = false, features = ["std"]`. The existing `dct` feature is the model.
2. **Dispatch** (`image.rs::from_stream`): `is_jpx(dict)` / `is_jbig2(dict)` return
   `Self::from_jpx` / `Self::from_jbig2`, and they run **before** the
   `/BitsPerComponent` and `/ColorSpace` reads — not beside `is_dct` as the first draft
   of this plan said. `/ColorSpace` is optional for a JPX image and `/BitsPerComponent`
   is ignored (7.4.9), and `from_stream` rejects a null colour space before any codec
   runs, so a dispatch placed after that block would never see a JPX stream without
   one. The size-before-decode order is then per codec: JPX from the codestream header,
   JBIG2 from the page information, against `document.decoded_limit()`.
3. **JPX** (`from_jpx`): `hayro_jpeg2000::Image::new(bytes, &DecodeSettings::default())`
   gives `width()`/`height()`/`color_space()` **before** any pixels are produced — that is
   where the budget check belongs; then `decode(&mut DecoderContext::default())` and
   `data_u8()`, which returns interleaved 8-bit samples, mapped through the existing
   `ColourSpace` (`Gray`, `RGB`, `CMYK`; an alpha channel is reported, not guessed).
4. **JBIG2** (`from_jbig2`): read `/DecodeParms` for `/JBIG2Globals` and parse it with
   `Globals::new` — the clause's separate-segments case, which a plan wiring only
   `Page::new_embedded(data, None, …)` would fail; then `Page::new_embedded(data,
   globals.as_ref(), &settings)`, whose `width()`/`height()` feed the same budget check,
   and `decode(&mut sink)` into a 1-bit sink that joins the existing `widen_one_bit` path
   the CCITT decoder already uses.
5. **Refusals stay honest**: a JPX codestream with an alpha channel, a JBIG2 page with a
   segment type the crate reports as unsupported, and a decode error each end as the
   `Do` report the page carries today — the change is that a *supported* file draws, not
   that an unsupported one starts guessing.

## Acceptance

- **JPX**: `corpus/tier-a/bug_jpx.pdf` draws — a 128×64 `/DeviceGray` OpenJPEG codestream —
  and joins `corpus/oracle-cases.txt`; the page's report loses its `Do` note.
- **JBIG2**: `corpus/tier-a/bitmap-composite-and-xnor.pdf` joins Tier A through
  `cargo run -q -p xtask -- corpus` (pdf.js corpus, Apache-2.0, same pinned commit as the
  rest; manifest and SHA-256 recorded in the same change) and draws its 399×400 1-bit
  image. `bitmap-p32-eof.pdf` is the `/JBIG2Globals` case for the round that needs it.
- **Budget honoured before pixels**: a JPX or JBIG2 stream whose header claims more
  samples than `decoded_limit()` is refused with `Error::Limit` *without* decoding, which
  a test proves by feeding a valid header over a truncated body.
- **The wasm verdict is the chosen option's**: whichever of A/B/C lands, `cargo xtask
  wasm-package` builds it and `check.mjs` enforces its budget; the browser and WASI smoke
  budgets stay where they are.

## Rounds

0. **The features and the measurement** — done 2026-11-17: `jpx`/`jbig2` exist and are
   off by default, the dispatch runs before the metadata, and the numbers above come
   from a build that decodes before it refuses.
1. **The budget decision, recorded** — done 2026-11-17: option B in
   [`docs/adr/0007-decoder-features.md`](../adr/0007-decoder-features.md), with the
   table above as the evidence and the npm budget unchanged.
2. **JPX**: the feature, `from_jpx`, the pre-decode size check, and `bug_jpx.pdf` drawing
   plus its oracle case.
3. **JBIG2**: the feature, `from_jbig2` with `/JBIG2Globals`, the corpus promotion of
   `bitmap-composite-and-xnor.pdf`, and its oracle case — with `bitmap-p32-eof.pdf` added
   when the globals path is exercised.
