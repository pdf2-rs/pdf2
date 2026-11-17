# ICC colour management

Deferred item: **ICC colour management** (`docs/DEFERRED.md` §1, rank 10b). Today an
`/ICCBased` colour space is read **by component count only** — `ColourSpace::from_object`
in `crates/pdf2-render/src/image.rs` maps `/N 1/3/4` to Gray/RGB/CMYK and the profile's
own mappings are never applied — and a `/DeviceCMYK` page is converted by ink arithmetic
(`(1−c)(1−k)`), which is why `cmykjpeg.pdf` sits **8.455 mean** from the engines' 2.000
(`docs/RENDERING-QUALITY.md`).

## Clause

**`8.6.5.5` ICCBased Colour Spaces** (the profile is a stream, `/N` states its component
count), **`8.6.5.8` Rendering Intents** (the `/Intent` a dictionary or the graphics state
carries), and **`10.4` Conversions among Device Colour Spaces** for the device-CMYK case —
all three in `docs/SPEC-MAP.md`'s verified table.

## Verdict — measured, 2026-11-17

A spike in ADR 0005's shape: `moxcms` 0.9.2 linked behind a temporary feature of
`pdf2-render`, reached from `Image::from_stream` when the colour space is an `/ICCBased`
array, the transform run once on a real buffer, the npm bundle built, and everything
reverted (the baseline reproduced at **361 056 gzip** afterwards).

| Configuration | npm bundle gzip | Δ | usable for |
| --- | --- | --- | --- |
| baseline | 361 056 | — | — |
| `default-features = false` (no `lut`, no SIMD paths) | 450 049 | **+88 993** | matrix/shaper profiles only |
| `default-features = false, features = ["lut"]` | 482 569 | **+121 513** | LUT profiles — the common case, and the only one for CMYK |
| crate defaults (`lut` + SIMD shaper paths) | 482 561 | **+121 505** | the same; SIMD costs nothing on wasm |

So **`lut` is the whole cost** (32 520 gzip) and the SIMD feature set is free at
`wasm32`. Licence and floor are clear: `BSD-3-Clause OR Apache-2.0` (already on
`deny.toml`'s allow-list) and `rust-version = 1.89`, well inside 1.100.

**Two things the measurement also settled, which the plan must not paper over:**

- **`cmykjpeg.pdf` will not move without a profile the document does not carry.** `moxcms`
  ships *no* default CMYK profile — its own SWOP test reads
  `./assets/us_swop_coated.icc` from disk, and that file is not in the published crate. So
  the 8.455-mean gap on a `/DeviceCMYK` page is a **second decision** (ship a CMYK profile
  — licence + size, the same shape as the standard-14 font decision) and not this plan's
  first win.
- **The honest alternative is to stay refused with these numbers**: +121 513 gzip is a
  third of the current bundle for a fidelity difference the engines themselves disagree
  about by 8 %, on a page whose *decode* is already right (it matches an independent
  decoder to 8 mean absolute units — ADR 0005's spike).

Options, with the files each touches, as the decoder plan did:

| Option | What changes | Cost |
| --- | --- | --- |
| **A. `/ICCBased` only, behind a feature** | `pdf2-render/Cargo.toml` (`icc = ["dep:moxcms"]`, off by default), `image.rs`'s `ColourSpace` carrying the parsed profile, `pdf2-wasm/Cargo.toml` pass-through | +121 513 gzip for whoever enables it; no default-bundle change |
| **B. Default on** | the same plus `crates/pdf2-wasm/check.mjs`'s `BUDGET_GZIP` (356 → 500 KiB) and `docs/PERFORMANCE.md` | a third more bundle for every page load |
| **C. Refused for now** | `docs/DEFERRED.md` keeps the row, this file keeps the numbers, and nothing in the tree changes | none — and the 8.455 stays, recorded |

## Design (for A or B)

1. **Carry the profile**: `ColourSpace::from_object` gains an `Icc { profile: Rc<ColorProfile>, components }` variant built from the `/ICCBased` array's stream (`new_from_slice`), falling back to today's component-count mapping when the stream does not parse — so a broken profile degrades to the current behaviour instead of failing the page.
2. **One transform per (profile, intent, destination)**: an `Rc`-keyed cache beside the glyph cache in `font.rs`'s style, because a transform is expensive to build (moxcms's own tests note a 256 KiB thread stack for building one) and profiles repeat across a page.
3. **Apply where the colour is made**: in `image.rs`'s per-pixel loop, and for page painting in `walk.rs` where `cs`/`scn` select a space — the same place `/Separation` and `/DeviceN` already route a tint through a function.
4. **Intent**: `TransformOptions { rendering_intent, .. }` from the dictionary's `/Intent` (`8.6.5.8`), defaulting to `Perceptual`.
5. **Refuse by name what a profile cannot do**: an `/N` that disagrees with the profile's own component count, a profile with no transform to sRGB (a device-link profile), and `/ICCBased` on an image whose `/Alternates` would matter — reported, not approximated.

## Acceptance

- **A known transform, not a vibe**: `tests/fixtures/icc.pdf` carries a small sRGB profile
  and a 2×2 image whose four samples are named, plus a second profile (a gamma-1.8 grey) so
  that a sample's RGB must differ between the two — the assertion is the *difference*
  between the two renders, which no fallback can fake.
- **The existing behaviour is the control**: the same fixture rendered with `/ICCBased`
  replaced by `/DeviceRGB` must show the unmanaged values, so a page that ignores profiles
  fails the first assertion and a page that always applies one fails the second.
- **`cid_cff.pdf`** (the corpus page with an `/ICCBased` image) keeps drawing, and its
  report does not gain a note.
- **What is *not* claimed**: `cmykjpeg.pdf`'s 8.455 stays until a default CMYK profile is
  a decision of its own; the fixture, not that page, is this plan's proof.
- **The budget verdict is the chosen option's**: A leaves `check.mjs` alone (the feature is
  off), B moves it to 500 KiB with the table above in `docs/PERFORMANCE.md`.

## Rounds

1. **The decision** — A, B or C, recorded in `docs/PERFORMANCE.md`, with the measurement
   table as the evidence. No code.
2. **`/ICCBased` with a transform**, option A's shape: the `Icc` variant, the cache, the
   image path, and the fixture proving the two profiles differ.
3. **The page path and the refusals** — `cs`/`scn` through a profile, the three refusal
   notes, and `cid_cff.pdf` unchanged.
4. **Only if a later objective asks**: the default CMYK profile decision, which is where
   `cmykjpeg.pdf`'s 8.455 actually lives.
