# 0001 — What this implementation will own, and what it refuses to be

- **Status:** accepted
- **Date:** 2026-11-14
- **Deciders:** repository owner
- **Inputs:** [`docs/COMPETITORS.md`](../COMPETITORS.md) (ten implementations,
  ten capabilities, adoption numbers) and [`docs/GAPS.md`](../GAPS.md) (eight
  gaps ranked by demand ÷ cost).

## Context

The matrix says the Rust ecosystem is organised around one job per crate:
`hayro` renders and is read-only by design (5 384 557 downloads, a 1 600+ PDF
visual suite), `lopdf` edits (30 289 120), `pdf-extract` extracts (8 236 722,
78 open issues), `printpdf` writes, and `pdf-rs` keeps rendering and text in
separate repositories. Nothing in Rust reads, edits, renders and extracts from
one model, and the C/C++ bindings that do (`pdfium-render` 3 954 648, `mupdf`
1 786 662) cost a build system, `unsafe`, and most wasm-friendly deployments.

Our own adoption is 1 664 downloads. A plan that begins with "be a better
`hayro`" begins by losing a feature race against a faster, better-tested
renderer; a plan that begins with "be `lopdf`" begins a price war on
familiarity. The decision below is about the shape we can defend, not the
features we would like.

## Decision

Two differentiators, both of which must be *checkable* to count.

### D1 — Render, extract and rewrite from one model, and prove they agree

The claim: the same parse and the same font handling answer three questions —
what a page looks like, what text it holds, and what it becomes when written
again. What the rasteriser draws is what `text()` returns, and what the writer
emits re-opens into the same page.

Why this and not something else: one crate already does read, render, extract
and write-new (`src/`, `crates/pdf2-render`), and every competitor splits them —
`pdf-rs` into `pdf_render` and `pdf_text`, `hayro` render-only, `pdf-extract`
extraction-only, `lopdf` rewrite-only. The claim is also the only way to beat
`pdf-extract`'s 78 issues: its failures are font-mapping failures, and the
renderer already parses the fonts it cannot name.

What must become true: composite-font extraction (gap 1, `/ToUnicode` and
CMaps) and CFF/Type 1 programs (gap 4), so that the set of files we *draw* and
the set we *read* stop being different sets. The parity test is already the
template: `crates/pdf2-render/tests/golden.rs`
(`a_book_page_is_set_in_its_embedded_serif_font`) asserts a golden image **and**
`text()` on the same page, and `tests/writer.rs`
(`written_files_reopen_to_the_same_pages`) holds the write side of the claim.

### D2 — Portable by construction: no C, no `unsafe`, `no_std`, wasm

The claim: the whole library runs where a C engine cannot — a browser, a plugin,
an embedded target — with the size and robustness budgets enforced in CI rather
than promised in prose.

Why this and not something else: this is the one column in the matrix we hold
alone, and the 5.7M downloads of the C bindings are the demand side of it. It is
also a constraint that shapes every other decision, which is what makes it a
differentiator rather than a footnote.

What must become true: every dependency added for D1 or for coverage keeps
`no_std` and the wasm budget (browser module 150 KiB gzip, WASI 175 KiB,
`docs/PERFORMANCE.md`), and the image codecs land behind features so a
`no_std` user never links four decoders they did not ask for.

## Non-goals

1. **Not a faster `hayro`.** We do not chase raster fidelity with glyph caching
   and a 1 600-file suite; exotic painting (shadings, patterns, transparency,
   `Tr` modes, Type 3) stays *deferred and reported* in `docs/RENDERING.md`,
   not silently improved.
2. **Not a wrapper.** No `pdfium`, no `mupdf`, no vendored C or C++ — that
   would make D2 false and someone else's correctness ours to debug.
3. **Not an Acrobat replacement.** Forms authoring, digital signatures, 3D,
   knockout groups are out of scope; annotations and form *rendering* are
   deferred (gap 7).
4. **Not a validator.** PDF/A and PDF 2.0 conformance checking is a different
   buyer with different evidence (gap 8, `veraPDF` owns it).
5. **Not a layout engine.** We extract text and place it; we do not reflow,
   shape for print, or typeset documents.
6. **Not upstream.** Contributions stay in this workspace; pdf-rs's policy
   ("code submitted needs to be written by a human", README) closes that path
   anyway.

## Consequences

- **The roadmap order follows the claim**: gap 1 (composite extraction) → gap 4
  (CFF/Type 1) → gap 2 (image codecs), because render/extract parity is what D1
  promises; robustness (gap 7) comes with them; editing (gap 5) is scheduled as
  a theme of its own, not slipped in beside features.
- **Proof is part of the product**: a feature without a fixture and an oracle is
  not done. M6 (corpus + oracle + tolerances) becomes a gate, not a document.
- **Dependencies are a budget**: four today; image codecs add up to four behind
  features and CFF adds `skrifa`/`read-fonts`. Each needs a `no_std` and size
  verdict before it lands, and `cargo deny` gains their licences.
- **Public statements get harder, not easier**: the README must state the gaps
  as plainly as the strengths (M9), because D1 and D2 are claims a reader can
  test.

## Alternatives considered and rejected

- **"Another fast renderer."** Rejected: `hayro` has 5.4M downloads, 1 600+
  visual regression PDFs and a read-only design that buys it performance we
  would have to earn from scratch; we would be a slower clone of a better
  product.
- **"Editing-first, `lopdf`-shaped."** Rejected *for now*: it is the largest
  demand on the board (30.3M) and the most expensive item (gap 5, five-plus
  rounds, incremental saves and object mutation), and entering head-on against
  an entrenched API is a familiarity war. It becomes a theme once D1's model can
  rewrite what it reads.
- **"A nicer API over `pdfium`/`mupdf`."** Rejected: 5.7M downloads show the
  demand, but it makes D2 false, ships someone else's C bugs, and turns the
  library into packaging.
- **"An extraction-only crate."** Rejected: `pdf-extract` owns the name and the
  downloads; the only durable edge over its 78 issues is the shared font engine
  that D1 creates.
- **"A conformance validator."** Rejected: different buyer, different evidence,
  no rendering synergy — and `veraPDF` is the reference.

## Decision log

- **2026-11-14 — the PDF 1.x header gate: GO, shipped.** The corpus run
  (`docs/CORPUS.md`) rejected 23 of 23 real files at the header because
  `src/document.rs:486` accepted only `2.`. The spike relaxed that one condition
  (accept `1.x` and `2.x`, refuse the rest) and measured the effect: 21 of 23
  files parse and render, 14 are fully readable, and the nine failures are
  pre-existing gaps (7 composite extraction, 1 xref edge, 1 RC4/AES-128). Cost:
  half a round — one condition, its unit test, the integration test that
  encoded the old policy, and the `cargo xtask corpus` chore. Risk: none
  observed; nothing else in the tree branches on the header version. Shipped as
  `pdf2` 1.1.0 because leaving a measurable library locked out of the world's
  files would have made D1 untestable.

## Revisit when

- `hayro` ships document manipulation or writing, closing the gap D1 relies on;
- `pdfium` becomes small and stable in wasm, weakening D2;
- the corpus run (Q4) shows composite-font extraction costs far more than the
  scorecard's two-and-a-half rounds;
- a funded pure-Rust editor appears, making the editing theme a race rather than
  a choice.

## References

- [`docs/COMPETITORS.md`](../COMPETITORS.md) — the matrix and its provenance.
- [`docs/GAPS.md`](../GAPS.md) — the ranking this ADR turns into a decision.
- [`docs/RENDERING.md`](../RENDERING.md) — what is drawn, what is reported, and
  the parity fixtures (`tests/fixtures/book.pdf`, `calibration.pdf`).
- [`docs/PERFORMANCE.md`](../PERFORMANCE.md) — the wasm size budgets D2 is held
  to.
