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

## 1. Out of scope for the current objective, with a reason

These are the eight things the 2026-11-16 plan (`rounds/plan-20261116.md`, Q13)
names as deferred rather than missing. Each is a decision with a cost, not an
oversight.

| Item | Why not now | Evidence | What would unblock it | Planned in |
| --- | --- | --- | --- | --- |
| **Editing an opened document** (`11` in the ranked list) — object mutation, incremental saves | `src/writer.rs` only builds new files; `Document` is read-only by construction (its cache is `RefCell`, its data is borrowed through `Arc`-free slices) | Rank 11: `lopdf`'s 30.3M downloads is the demand; a release of its own | A mutation model (copy-on-write objects, a free-list, an `/Prev` chain) — i.e. a release, not a round | [plan](plans/editing.md) |
| **Forms *filling* and annotation *authoring*** (rank `12`'s second half) | Reading and *rendering* annotation appearance streams shipped (`12b`); writing `/Annots` and `/AcroForm` values is editing again | `docs/FEATURES.md` `12.7`, `12.5` rows: appearance streams shipped, authoring unplanned | Rank 11, plus a form model and a `/NeedAppearances` decision | [plan](plans/forms.md) |
| **Conformance validation** (PDF/A, PDF 2.0, rank `13`) | A validator is a different product: no rendering synergy, and the reference is `veraPDF` (Java) | Rank 13 in `docs/ROADMAP-NEXT.md` | A separate crate reading the same objects — a project, not a round | [verdict](plans/validation.md) |
| **JPX and JBIG2 decoding** — **unblocked 2026-11-17**, now a size decision | Both decoders need `rust-version = "1.92"`, and the floor is now **1.100** ([ADR 0005](adr/0005-msrv-1-100.md)), so nothing technical blocks them. What remains is the bundle: measured by linking them, JPEG 2000 costs **+76 052 gzip**, JBIG2 **+62 495**, both **+137 502** — against an npm budget of 356 KiB, so both would need **~499 KiB** (37 % more), or a non-default `pdf2-wasm` feature that the browser demo would not get | `docs/adr/0005-msrv-1-100.md` (the measurement table); rank 9 in `docs/ROADMAP-NEXT.md` | A budget decision: raise the npm budget, or ship them behind a feature and give the demo a second build. The browser/WASI budgets are unaffected either way, because those modules contain no renderer | [plan](plans/jpx-jbig2.md) |
| **ICC colour management** (`10b`) | Still a fidelity decision, not a bug: the engines disagree with each other by **8 %** on the CMYK page, and ICCBased **images** already decode by component count. Measured 2026-11-16: `cmykjpeg.pdf` decodes correctly — 8 mean absolute units from an independent decoder — yet sits **8.455 mean** from the engines' 2.000 because they honour `/Intent /RelativeColorimetric`. **Since 2026-11-17 the dependency question is answered**: `moxcms` 0.9.2 needs 1.89 (below the floor), is `BSD-3-Clause OR Apache-2.0` (already allowed by `deny.toml`), and is pure Rust — so the blocker is now purely the fidelity decision and a size measurement like JPX's | `docs/RENDERING-QUALITY.md`; `docs/adr/0005-msrv-1-100.md` | Decide to be more precise than the inter-oracle spread, measure the profile paths before and after, and take the size verdict (`cargo xtask wasm-package` with the feature on) | [plan](plans/icc.md) |
| **Tiling patterns** (`/PatternType 1`) | Painting a tile means flattening a content stream into a cell and repeating it; two corpus pages use one (`colorkeymask.pdf` is one of them) | `docs/RENDERING.md` says the operator is reported; `crates/pdf2-render/src/walk.rs` handles `/PatternType 2` only | A pattern-tile renderer — a canvas, a cell, and a repeat rule — which is a round of its own | [plan](plans/tiling-patterns.md) |
| **Colour-key `/Mask`** | One corpus page (`colorkeymask.pdf`), and it is the *same* page as the tiling pattern, so its case cannot be measured in isolation | `docs/RENDERING.md` masks row | Either the tiling patterns above (the page needs both), or a generated fixture proving the key alone | [plan](plans/colour-key-mask.md) |
| **The clipping half of `Tr` modes 4–7** | Clipping text to its glyph outlines needs the path as a clip *without* painting; no corpus page asks for it, so nothing would prove it | `crates/pdf2-render/src/walk.rs` honours the fill and stroke halves (modes 0–3) and reports the rest | A generated fixture with `Tr 7` around an image — a hand-built page, not something the corpus offers | [plan](plans/tr-clipping.md) |

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
| PDF/A conversion, rewriting a document into a profile | non-goal 4 | Validation is item 13 above; conversion is editing | — |
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
