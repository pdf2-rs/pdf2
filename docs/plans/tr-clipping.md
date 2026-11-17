# The clipping half of `Tr` modes 4–7

Deferred item: **the clipping half of `Tr` modes 4–7** (`docs/DEFERRED.md` §1). The
renderer already paints by mode — `crates/pdf2-render/src/walk.rs:821` reads
`state.text_mode`, `place_glyph` fills for 0/2/4/6 and strokes for 1/2/5/6 — but it never
**clips**, and mode 7 is treated as "invisible" and returns before doing anything at all
(`walk.rs:822`). So a page that uses `7 Tr` to mask an image paints an unmasked image and
reports nothing.

## Clause

**`9.3.6` Text Rendering Mode**, Table 104: mode 0 fill, 1 stroke, 2 fill+stroke, 3
invisible, **4 fill+clip, 5 stroke+clip, 6 fill+stroke+clip, 7 clip only**. When a mode
has the clipping bit the glyph outlines are added to the *current clipping path* — "as if
they were a path" — which means the non-zero winding rule of **`8.5.3.3` Filling** applies,
and the clip **accumulates** across glyphs, text objects and later painting until the
graphics state is restored.

Both clauses are in `docs/SPEC-MAP.md`'s verified table (`9.3.6`) or long-standing in the
map (`8.5.3.3`).

## Verdict

**No dependency, no licence, no wasm cost** — the machinery exists twice over:
`clip_mask()` builds a coverage mask from polylines and `ClipMask::intersect` composes it
(`walk.rs:1892`, `apply_pending_clip`), and `place_glyph` already computes the glyph's
device polygons through the text rendering matrix (`scale × text_matrix × ctm`). The clip
that text accumulates also needs no new state, because `q`/`Q` already save and restore
`self.mask` with the state (`walk.rs:405`).

The only cost is correctness: a wrong clip is invisible in a report and very visible on a
page, which is why the acceptance is pixel arithmetic on a fixture rather than a report
string.

**No corpus page uses it** — checked by grep across `corpus/tier-a` — so there is no
oracle case to inherit and the fixture is the whole proof.

## Design

1. **Clip where the glyphs are placed** (`place_glyph`): after the polygons are built for
   the text rendering matrix, when `mode & 4 != 0` (modes 4–7) intersect the mask with
   `clip_mask(width, height, &polygons, false)` — `false` is the non-zero winding rule the
   clause names. This accumulates by construction: every glyph, and every later `Tj` in
   the same state, further restricts the mask.
2. **Move mode 7's early return**: `7 Tr` must clip and paint nothing, while `3 Tr` paints
   nothing and clips nothing. Today both hit the same `invisible` early return, so the
   mask survives; after this change `7` clips first and `3` still returns immediately.
3. **Keep paint and clip independent**: 4/5/6 continue to fill and/or stroke *and* clip —
   the existing `filled`/stroke branches stay, the clip is added beside them, so a mode-6
   glyph is painted and then contributes its outline.
4. **Type 3 and bitmap glyphs**: a Type 3 glyph is a content stream, not contours, so its
   `Glyph` has none and `place_glyph` returns before the clip. The plan **reports** that
   (`text-clip`) rather than silently clipping to nothing, and leaves geometric clipping
   for Type 3 to the round that gives Type 3 glyphs a path.
5. **Interaction with paths**: `W`/`W*` clips and text clips are the same mask, so a page
   that does both gets the intersection — which is what the clause's "added to the current
   clipping path" means, and needs no special case.

## Acceptance

One generated fixture, `tests/fixtures/tr-clip.pdf`, with three pages:

- **Mode 7 clips**: `BT /F1 96 Tf 7 Tr 72 600 Td (A) Tj ET` followed by a filled
  `0 0 612 792 re f` in a known colour. Assertions are the glyph's shape, not "some ink":
  the centre of the "A" counter is **background** (non-zero winding, so the counter is a
  hole), a point on the left stroke is the fill colour, and a point outside the glyph but
  inside the rectangle is **background** — an implementation that ignores the clip fails
  the third.
- **Mode 4 fills and clips**: the glyph is painted *and* the rectangle after it is clipped,
  so both a painted-stroke assertion and a clipped-outside assertion hold on one page. An
  implementation that clips but does not paint fails the first.
- **Mode 0 control**: ordinary text followed by the same rectangle must **not** clip it —
  the assertion that proves the clip is conditional on the mode.
- **`Q` restores**: the clipping text inside `q … Q` must not affect a rectangle painted
  after `Q`, proving the clip lives in the state `walk.rs:405` already saves.
- **Type 3**: a Type 3 glyph with `7 Tr` is reported `text-clip` and clips nothing, so the
  gap is visible in a report instead of being a silent empty clip.
- **No oracle case**: the page joins `corpus/oracle-cases.txt` only if a corpus page with
  `Tr` 4–7 appears, which none does today; the fixture is the proof, and the plan says so
  rather than adding a case the engines would render identically-but-for-our-fonts.

## Rounds

1. **The clip and mode 7** — `clip_mask` in `place_glyph` for modes 4–7, the mode-7 early
   return moved, and fixture pages 1 and 3 (clip, control). One round, and the shape of the
   change is a dozen lines.
2. **Paint plus clip, and the state** — pages 2 and 4 (mode 4, `Q` restore), plus the
   committed golden if the fixture joins `tests/fixtures`' rendered set.
3. **The Type 3 report** — `text-clip` for a glyph with no contours, its test, and the
   `docs/FEATURES.md` row for `9.3` moving from `partial` to `done for geometric glyphs`.
