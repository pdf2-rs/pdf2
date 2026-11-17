# 0008 — Positioned text: `TextSpan`, and where the boxes come from

- **Status:** accepted
- **Date:** 2026-11-17
- **Deciders:** repository owner
- **Supersedes:** §3 of [ADR 0002](0002-text-extraction-shape.md) ("boxes are refused in
  1.2 and revisited in 1.3"), on the grounds the frozen type exists to protect
- **Inputs:** selection and search from the demo and the library
  (`rounds/plan-20261117-gaps.md`), pdfium's `public/fpdf_text.h` as prior art,
  `src/content.rs`, [`docs/RENDERING.md`](../RENDERING.md)

## Context

ADR 0002 refused boxes in 1.2 and said to revisit them in 1.3. The reasons were
sound and both have expired: "a public type is a permanent commitment" while no
consumer had demonstrated a need — a demo that must **select text and search it**
is that need, and the same geometry serves both, as pdfium's text page shows
(`FPDFText_GetCharBox`/`GetRect` for selection, `FindStart`/`FindNext` for search
over the same boxes); and "a correct box needs the CTM, which this interpreter
does not track" — so the work is not the type, it is teaching the extractor the
CTM. Shipping boxes without it would be the coordinate lie ADR 0002 warned about.

## Decision

1. **`Document::spans(&self, page: &Page) -> Result<Vec<TextSpan>>`** joins
   `extract_text` and `extract_text_from` in the same `impl Document`
   (`src/content.rs:721`), reading the **same** accumulator: a line is one span,
   so text and geometry cannot disagree.
2. **`TextSpan { text: String, bounds: Rect }`, in page space.** `Rect` is the
   existing user-space type (`src/page.rs:31`), normalised as it always is. The
   bounds are the four corners of the line box — baseline origin, its advance,
   the em above it — each corner transformed by the text matrix composed with
   the CTM, so a rotated or scaled line yields the rotated or scaled rectangle
   rather than an axis-aligned approximation of it.
3. **The extractor learns `q`, `Q` and `cm`.** That is the whole of the new
   state: a CTM (identity at the start of a content stream, saved and restored by
   `q`/`Q`) and the text matrix, which `BT` resets and `Tm`/`Td`/`TD`/`T*` move.
   `Line.x`/`Line.y` keep their meaning — text-space position of the line's first
   character — and the geometry is derived from them plus the matrices, so the
   reading order ADR 0002 fixed does not change.
4. **What a span is not.** It is a **line** box, not a glyph box: a caller that
   needs per-character rectangles can split `text` by advance, and one that needs
   a selection highlight wants the union of the line boxes it touched, which is
   what the demo does. No layout analysis, no hyphenation, no column detection —
   the same limits ADR 0002 accepted for order.

## Consequences

- `extract_text` is unchanged: same signature, same string, and `spans()` is the
  geometry view of it. A test asserts the two agree character for character.
- The public surface grows by one type and one method, a **minor** bump after the
  1.0 freeze — the freeze exists to make such a decision deliberate, not to make
  it impossible, and ADR 0002 itself scheduled the revisit.
- `Search` (`Document::search`) reads spans, so its hit rectangles and the demo's
  selection highlight are the same numbers.
- The CTM is now tracked by the extractor, which also makes the **rendered** and
  extracted views of a page agree about where a line is — something a calling
  application (a text layer over a canvas) cannot paper over.

## Proven by

- `tests/fixtures/reading-order.pdf` — three lines at (50, 400), (50, 350),
  (50, 300) with the identity CTM, plus a fourth painted under
  `q 2 0 0 2 20 100 cm`, whose bounds must be doubled and offset by (20, 100).
- `tests/content.rs` — the extracted string and its spans agree, and each span's
  `bounds` is the arithmetic above.
