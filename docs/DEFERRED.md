# The deferred register

One page for everything this library deliberately does **not** do yet, so a
decision recorded once stops being re-litigated every round. It is not the ledger:
[`docs/FEATURES.md`](FEATURES.md) answers "what is the status of this clause", row
by row. This file answers "why is it not happening, and what would change that" for
the items a reader is most likely to expect and not find.

Two rules keep the two files honest. **Every item here names its ledger row** (or
its ADR line, when it is a refusal rather than a gap). **A ledger row marked
`unplanned` need not appear here** — most are simply unowned, and the ledger's own
column is the answer. An item belongs here when somebody has asked for it, or will.

Recorded 2026-11-16, at `1.2.1`.

## Reinstated on 2026-11-17

Three items below used to sit in section 1. They are not deferred any more: the owner decided
they are options this project does not drop, so each has a rank in
[`docs/ROADMAP-NEXT.md`](ROADMAP-NEXT.md), an acceptance there, and a plan.

| Item | Rank | Plan |
| --- | --- | --- |
| Editing an opened document — object mutation, incremental saves | 11 | [plan](plans/editing.md), [ADR 0006](adr/0006-editing-mutation-model.md) |
| Forms *filling* and annotation *authoring* | 12 | [plan](plans/forms.md) |
| Conformance validation — **PDF/A** and PDF 2.0 | 13 | [plan](plans/validation.md); veraPDF 1.30 is the CI oracle |

## 1. Out of scope for the current objective, with a reason

These are the eight things the 2026-11-16 plan (`rounds/plan-20261116.md`, Q13)
names as deferred rather than missing. Each is a decision with a cost, not an
oversight.

| Item | Why not now | Evidence | What would unblock it | Planned in |
| --- | --- | --- | --- | --- |
| ~~**JPX and JBIG2 decoding**~~ — **shipped 2026-11-17 behind features** | Both are cargo features, off by default ([ADR 0007](adr/0007-decoder-features.md)): `jpx` costs **+76 705 gzip** and `jbig2` **+62 685** on the wasm bundle, against a 364 KiB npm budget, so the npm package decodes neither and the demo site ships a second, decoder-enabled artefact (506 424 gzip). `/JBIG2Globals` is decoded before the page's own segments, and the JPX codestream header decides size, colour space and depth. `bug_jpx.pdf` is an oracle case; the two JBIG2 pages draw and are gated too (**0.243040 mean** against an engines' spread of 0.326558), after pdfium's reference raster exposed a **row-stride bug in this renderer's sink** that had put them 31.261 away | `docs/adr/0007-decoder-features.md`, `docs/RENDERING-QUALITY.md`, `corpus/manifest.json` | Nothing left to decide. The 31.261 JBIG2 gap is a decoder-fidelity question, not a scheduling one | [plan](plans/jpx-jbig2.md) |
| **ICC colour management** (`10b`) | Still a fidelity decision, not a bug: the engines disagree with each other by **8 %** on the CMYK page, and ICCBased **images** already decode by component count. Measured 2026-11-16: `cmykjpeg.pdf` decodes correctly — 8 mean absolute units from an independent decoder — yet sits **8.455 mean** from the engines' 2.000 because they honour `/Intent /RelativeColorimetric`. **Since 2026-11-17 the dependency question is answered**: `moxcms` 0.9.2 needs 1.89 (below the floor), is `BSD-3-Clause OR Apache-2.0` (already allowed by `deny.toml`), and is pure Rust — so the blocker is now purely the fidelity decision and a size measurement like JPX's | `docs/RENDERING-QUALITY.md`; `docs/adr/0005-msrv-1-100.md` | Decide to be more precise than the inter-oracle spread, measure the profile paths before and after, and take the size verdict (`cargo xtask wasm-package` with the feature on) | [plan](plans/icc.md) |
| ~~**Tiling patterns** (`/PatternType 1`)~~ — **shipped 2026-11-17** | The cell's `/BBox` clips each copy, `/XStep`/`/YStep` are the lattice, `/PaintType 1` paints the cell's own content and `/PaintType 2` is a stencil poured with the colour `scn` gave it; `MAX_TILE_CELLS` bounds the lattice (`crates/pdf2-render/src/pattern.rs`, `tests/tiling.rs`, `tests/pattern.rs`). `/TilingType` 1/3 device-pixel spacing is recorded as a later decision | `docs/RENDERING.md` says the operator is reported; `crates/pdf2-render/src/walk.rs` handles `/PatternType 2` only | A pattern-tile renderer — a canvas, a cell, and a repeat rule — which is a round of its own | [plan](plans/tiling-patterns.md) |
| ~~**Colour-key `/Mask`**~~ — **shipped 2026-11-17** | An array `/Mask` names the sample values that are transparent, applied per image sample (`tests/colour_key.rs`); the stream `/Mask` stencil beside it is still refused by name | `docs/RENDERING.md` masks row | Either the tiling patterns above (the page needs both), or a generated fixture proving the key alone | [plan](plans/colour-key-mask.md) |
| ~~**The clipping half of `Tr` modes 4–7**~~ — **shipped 2026-11-17** | Modes 4–7 add the glyph outlines to the clip without painting them, proved by the generated `tr-clip.pdf` and `tr-clip-type3.pdf` (`tests/tr_clip.rs`, `tests/tr_clip_type3.rs`) | `crates/pdf2-render/src/walk.rs` honours the fill and stroke halves (modes 0–3) and reports the rest | A generated fixture with `Tr 7` around an image — a hand-built page, not something the corpus offers | [plan](plans/tr-clipping.md) |

Mesh shadings (`/ShadingType` 4–7) are deferred by name in the renderer
(`crates/pdf2-render/src/shading.rs:127`) rather than by a row in this table:
[plan](plans/mesh-shadings.md).

## 2. Refused by design

These are non-goals in [`docs/adr/0001-differentiators.md`](adr/0001-differentiators.md),
not gaps. They will not be scheduled.

| Item | ADR line | Why | Planned in |
| --- | --- | --- | --- |
| Racing hayro's throughput | non-goal 1 | The differentiator is one model for render, extract and rewrite — not a benchmark | — |
| A C or C++ core, `unsafe`, or a bundled wasm decoder | non-goal 2 | `#![forbid(unsafe_code)]`, `no_std` and the wasm size budgets are the shape of the crate | — |
| JavaScript actions and scripting | non-goal 3 | A viewer's job, and a security surface | — |
| PDF/A *conversion* — rewriting a document into a profile | non-goal 4 | **Validation** is scheduled (rank 13, [plan](plans/validation.md)); conversion is editing, rank 11 | — |
| Rendering with system fonts for standard-14 text | measured, not decided | There is no font in the crate to draw with; `docs/RENDERING-QUALITY.md` records what that costs (the standard-14 page measures 21.271 mean against the oracles' 1.400) and it is why such a page is proven by its report and ink, not by a pixel comparison | [verdict](plans/standard-14-fonts.md) |

## 3. Reported, not drawn wrong

The rule behind several refusals: an operator this renderer cannot honour is
**named in the page report** (`Do`, `sh`, `gs`, `cs`, `Tj`, …) rather than
approximated. `docs/RENDERING.md` lists what is honoured and what is reported; a
report is a bug report that travels with the document, and `docs/RENDERING-QUALITY.md`
measures what is left of it (the sample: 3 pages still report a shading, all three
tiling patterns).

Two constructs are refused **by name** for the same reason, and each has a brief for
whoever picks it up:

- ~~**CMYK JPEG**~~ — drawn since 2026-11-16: the decoder returns CMYK, the Adobe
  APP14 inversion is undone, and the samples are converted multiplicatively. Proven
  by `tests/fixtures/cmyk-jpeg.pdf` (four ink quadrants) and by `cmykjpeg.pdf`.
- ~~**CCITT two-dimensional coding**~~ — drawn since 2026-11-16: the
  changing-element model with the reference position tracked, proven by the corpus
  page's six labelled images.
