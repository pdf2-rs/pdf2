# 0003 — CFF outlines without a new dependency

- **Status:** accepted
- **Date:** 2026-11-15
- **Deciders:** repository owner
- **Inputs:** roadmap item 4 ([`docs/ROADMAP-NEXT.md`](../ROADMAP-NEXT.md)),
  `docs/RENDERING.md`, the corpus's two `/CIDFontType0` pages.

## Context

Roadmap item 4 said CFF and Type 1 programs should render, and named `skrifa`
0.50.0 / `read-fonts` 0.47.0 as the library to do it: both declare
`rust-version = "1.85"` (inside our 1.87 MSRV), are `MIT OR Apache-2.0`, and
expose `std` as an opt-out with `libm`. The plan therefore asked for a verdict —
`no_std`, wasm size, licence — before any code.

The two corpus pages are the proof targets:

| File | Program |
| --- | --- |
| `cid_cff.pdf` | `/FontFile3`, `/Subtype /CIDFontType0C` — a **bare CFF** table |
| `Embedded_font.pdf` | `/FontFile3`, `/Subtype /OpenType` — already an sfnt |

## Measurement, not assumption

`FontRef::new` from `skrifa` 0.50.0, run against the extracted `cid_cff.pdf`
program (9 082 bytes, magic `01 00 04 04`):

```text
FontRef err: Invalid sfnt version 0x01000404
```

`skrifa`'s high-level API needs an sfnt container as much as `ttf-parser` does,
so adopting it would **not** have removed the container work for the bare CFF
case — it would have added a dependency and kept the hard part.

`ttf-parser` 0.25.1 is already a dependency of `pdf2-render` and reads CFF
outlines from an OpenType file. Given a synthesised container — `head`, `hhea`,
`maxp`, `hmtx` and the program as `CFF ` — it parses the same program:

```text
face ok: upem 1000 glyphs 49
glyph 1: contours 3
glyph 10: contours 2
```

## Decision

1. **No new dependency.** CFF outlines come from `ttf-parser`, which the
   renderer already uses, over a container this crate synthesises
   (`wrap_cff` in `crates/pdf2-render/src/font.rs`): five tables, 1000 units per
   em (CFF's default `/FontMatrix`), the glyph count from the `CharStrings`
   INDEX, and zero advances, because a CID font's advances come from `/W` in the
   PDF. Checksums stay zero: `ttf-parser` does not read them.
2. **CID-keyed fonts select glyphs through their charset.** Using the CID as a
   glyph id draws nothing — `Embedded_font.pdf` has two glyphs and asks for CID
   `0xA911`. The charset is inverted once at load (`cff_charset`) and stored as
   CID → glyph id; formats 0, 1 and 2 are read, and the three predefined charsets
   are not CID-keyed, so they map nothing.
3. **`/Subtype /OpenType` is read directly**, with the same charset inversion
   taken from the `CFF ` table inside it.
4. **Simple (non-CID) Type1C text is still not drawn.** Those programs reach
   glyphs through charset glyph *names* against the font's encoding, which this
   renderer does not carry; they keep reporting `font:FontFile3` rather than
   drawing a wrong glyph.

## Consequences

- `pdf2-render` is `std`-only today, so the `no_std` half of the verdict does not
  arise for this code; nothing was added to the library's dependency set, so the
  crate's wasm budget is unchanged by the dependency graph.
- The cost is about 200 lines in `font.rs`, with unit tests over a hand-built
  three-glyph CFF so the INDEX and Top DICT readers are checked without the
  corpus.
- `Tr` (text rendering mode) had to be honoured in the same change: a CFF page
  that hides text behind `Tr 3` would otherwise be reported as unsupported *and*
  drawn, which is the worst of both.

## Revisit when

A simple-font CFF page shows up in the corpus or in an issue — then the work is
to carry glyph names from the charset and the base encoding, which is the one
piece this decision leaves out.
