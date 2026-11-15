# 0002 — What extraction returns: text, and why not boxes yet

- **Status:** accepted
- **Date:** 2026-11-15
- **Deciders:** repository owner
- **Inputs:** roadmap item 2b ([`docs/ROADMAP-NEXT.md`](../ROADMAP-NEXT.md)),
  `src/content.rs`, D1 of [`0001-differentiators.md`](0001-differentiators.md).

## Context

`Document::extract_text(&Page) -> Result<String>` is the extraction API that
shipped in 1.0 and the one the CLI, the wasm demo and `docs/PROOF.md` call. Item 2
adds composite-font mapping and item 2b asks for extraction *quality* — reading
order, spaces, and the question underneath both: should extraction keep
returning a string, or start returning runs with boxes?

The question could not be left to the code. Whichever the answer, it decides what
the `/ToUnicode` parser produces (item 2, chunk 1), what `text()` accumulates
(chunk 2), and whether 1.2 ships a public type that 1.0's freeze makes permanent.

Two measured facts bound the options:

- The current interpreter emits text in **paint order** and breaks lines from
  text-positioning operators. `ShowText-ShadingPattern.pdf` in the corpus paints
  its four lines 2, 1, 4, 3 and a reader expects 1, 2, 3, 4 — so even the string
  answer is wrong today, and has to be fixed by *ordering*, not by new API.
- The interpreter deliberately ignores the CTM: it tracks text space only
  (`src/content.rs`, `Td`/`TM`). Any box it produced today would be in the wrong
  coordinate system for anything that printed it next to a render.

## Decision

1. **The public API stays text-only in 1.2.** `extract_text` keeps its
   `Result<String>` signature and remains the single extraction entry point; then
   `extract_text_from` (content + resources) is the same path. No `TextSpan`, no
   boxes, no second source of truth.
2. **Order is a property of the model, not of a printer.** The interpreter
   accumulates positioned lines (`Line { x, y, text }`) and sorts them for the
   result — descending `y`, then ascending `x` — and the string is the projection
   of that order. Reading order is therefore fixed once, for every caller, rather
   than reimplemented by each one.
3. **Boxes are refused in 1.2** and revisited in 1.3. The reasons in order:
   a public type is a permanent commitment under the 1.0 freeze, and no consumer
   of this crate has demonstrated a need for boxes (the corpus tool, the CLI and
   the demo want text); a *correct* box needs the CTM, which this interpreter does
   not track, so shipping one now would ship a coordinate lie; and the claims of
   item 2b that matter — order, spaces, mapping — are provable without exposing
   spans.
4. **Consequences for items 2, 2b and 5.** The `/ToUnicode` parser (Q4) returns
   code→text mappings only, with no geometry. Wiring (Q5) feeds characters into
   the same `Line` accumulation, so composite text inherits the ordering and the
   space rules for free. Tests assert ordered text (`tests/content.rs`:
   `reading-order.pdf`, and the corpus page named below), never spans.

### Limits this decision accepts, stated rather than hidden

- Intra-line order is paint order; a writer that paints a line's words
  right-to-left yields them in that order.
- A page with several columns sorts left-to-right within a `y` band. Reading a
  two-column article therefore interleaves its columns. This is a layout engine's
  job and item 2b's ranking does not buy one.
- Spaces come from space glyphs and wide `TJ` adjustments, not from advance
  widths. Advances arrive with `/Widths` in Q5, for composite fonts, where they
  are also needed to keep glyphs apart.
- Ligatures are not decomposed (`ﬁ` stays one character), which matches what
  `text()` returns today.

## Proven by

- `tests/fixtures/reading-order.pdf` — three lines painted third, first, second
  — extracts `first line`, `second line`, `third line` with one source of space
  being a `TJ` adjustment (`tests/content.rs`).
- `corpus/tier-a/ShowText-ShadingPattern.pdf` — painted 2, 1, 4, 3, extracts
  1, 2, 3, 4 with its spaces intact (`pdf2 text`, recorded in
  [`docs/CORPUS.md`](../CORPUS.md)).
- The whole workspace test suite, unchanged expectations: every existing fixture
  paints top-down, so geometric order and paint order agree there, which is why
  the change moved one corpus page and no fixture.

## Revisit when

A consumer shows a box is needed — then the work is to track the CTM in the
interpreter (the renderer already does) and to introduce one public run type in a
minor release, with this ADR superseded rather than edited.
