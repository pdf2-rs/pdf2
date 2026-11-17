# Colour-key masking (`/Mask` array)

Deferred item: **colour-key `/Mask`** (`docs/DEFERRED.md` §1). An image may carry
`/Mask` as an array of `2 × n` integers naming colours that shall not be painted. This
renderer reads `/Mask` nowhere: `crates/pdf2-render/src/image.rs` never looks at the key,
so an image that declares one paints those samples as if they were colour. The corpus's
`colorkeymask.pdf` is exactly that image, and it is the page the tiling plan also waits on.

## Clause

**`8.9.6` Masked Images**, `.4` Colour Key Masking. The clause is precise about three
things this plan has to get right:

- `/Mask` is `2 × n` integers, each in `0 … 2^BitsPerComponent − 1`, "representing colour
  values **before decoding with the Decode array**" — so the key is tested against the raw
  sample, not against the value `/Decode` produces.
- A sample is masked when **all** its components fall in their range. The clause prints
  the comparison as `min ≤ c ≤ max`; the ISO DIS text this project can read renders it as
  strict inequalities, and the inclusive reading is the only one under which `min == max`
  means anything — which the corpus relies on: `colorkeymask.pdf`'s image carries
  `/Mask [255 255 0 255 0 255]`, i.e. "red exactly 255", and an exclusive rule would mask
  nothing. The acceptance below pins the inclusive reading explicitly.
- "When colour key masking is specified, the use of a DCTDecode filter for the stream
  shall not be used" — a lossy filter moves samples across the boundary. This renderer
  must refuse that pairing by name rather than paint it approximately.

`.2` and `.3` are the other half of the clause — `/Mask` naming a *stencil image mask*
stream — and are **not** this plan: a stencil is a second image with its own size,
resampling rule and interpolation semantics. It is refused by name in round 1 and left for
its own plan, so the two mechanisms cannot be half-built together.

## Verdict

**No dependency, no licence, no wasm cost.** The work is one comparison per source sample
at decode time and one `Vec<bool>` per image — `width × height` bytes, which the existing
`/Width × /Height × components` budget already bounds, so no new limit is needed.

It composes with what exists rather than adding a mechanism: `Image::paint`
(`crates/pdf2-render/src/image.rs:262`) already takes a `ClipMask` and an alpha, and
"masked samples shall not be painted" is *skipping* a pixel, not blending it — so the
background shows through exactly, with no rounding.

## Design

Where it lands, in the order the rounds build it:

1. **Parse** (`image.rs::from_stream`, beside the `/Decode` handling): `/Mask` as an
   `Array` of `2 × per_pixel` integers becomes `Key { min: [i32; 4], max: [i32; 4] }`;
   a wrong count or a value outside `0 … 2^bits − 1` is reported, not clamped.
2. **Test the raw samples**: the mask is computed from `data` **before** `decode_ranges`,
   while the bytes are still the stream's own values. For a 1-bit image that means the
   key is tested against the packed bit (`0` or `1`), which `widen_one_bit`
   (`image.rs:477`) currently maps to `0`/`255`; the mask is computed in that same pass so
   the mapping cannot silently change the comparison.
3. **Carry it** (`Image` gains `masked: Option<Vec<bool>>`) and **apply it**: `paint`
   skips a device pixel whose nearest source sample is masked, before the blend.
4. **Refuse the two cases the clause names**: a `/DCTDecode` stream with `/Mask`
   (`unsupported: colour-key /Mask on a /DCTDecode image`, quoting the clause's reason),
   and `/Mask` as a stream reference (`unsupported: a /Mask image stencil`), each with a
   test.
5. **Interaction with `/SMask`**: independent. A `/SMask` is page-level coverage that
   multiplies into the clip; the colour key is per sample. Both apply, and the pixel's
   coverage is the product — which the existing `mask.alpha(x, y)` already computes.

## Acceptance

Four assertions, three of them on one generated fixture
(`tests/fixtures/make_fixtures.py` builds it):

- **`tests/fixtures/colour-key.pdf`** — a 2×2 `/DeviceRGB` 8-bpc image drawn over a green
  rectangle, `/Mask [255 255 0 255 0 255]`: the pure-red corner shows **green** (the
  background through the hole) and the other three corners show their own colours. Pixel
  exact, so an off-by-one in the key fails it.
- **The boundary pins the inclusive reading**: the same image with `/Mask [0 0 0 0 0 0]`
  masks its pure-black sample and nothing else. An exclusive implementation paints it.
- **1-bit**: a 1-bpc image with `/Mask [0 0]` masks the white bits, proving the test runs
  on the raw sample rather than the widened byte.
- **Refusals**: a `/DCTDecode` image carrying `/Mask`, and a `/Mask` that is a stream
  reference, are each reported with their own message and paint nothing.
- **Corpus**: `corpus/tier-a/colorkeymask.pdf` — a 200×267 `/DeviceRGB` image with
  `/Mask [255 255 0 255 0 255]` inside a tiling pattern. It is the joint case with
  `docs/plans/tiling-patterns.md`: this plan alone changes nothing on that page, and the
  page's acceptance is recorded by whichever plan ships second. It joins
  `corpus/oracle-cases.txt` then, not before.

## Rounds

1. **Parse and apply, 8-bit RGB** — `Key`, the raw-sample test, `Image.masked`, the skip in
   `paint`, and `tests/fixtures/colour-key.pdf` with its green hole. One round, and it is
   the round that changes `colorkeymask.pdf`'s painted output once tiling exists.
2. **The edges**: the inclusive boundary fixture, the 1-bpc case, and the two refusals
   (`/DCTDecode`, stream-reference `/Mask`) with their report strings.
3. **The corpus joint case**: `colorkeymask.pdf` with `tiling-patterns.md`'s rounds, the
   oracle case added by whichever lands second, and the ledger row in `docs/FEATURES.md`
   updated from "scheduled 10" to shipped.
