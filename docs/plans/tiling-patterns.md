# Tiling patterns (`/PatternType 1`)

Deferred item: **tiling patterns** (`docs/DEFERRED.md` §1). Today the renderer handles
`/PatternType 2` (a shading fills the path) and *silently ignores* `/PatternType 1`:
`crates/pdf2-render/src/walk.rs:1239` returns early from `select_pattern` when the type
is not 2, with no note, so a page filled with a tile paints the previous colour and says
nothing. That silence is the first thing this plan fixes, before any tile is drawn.

## Clause

**`8.7.3` Tiling Patterns** — `.1` general (the pattern dictionary, Table 73), `.2`
coloured tiling patterns (`/PaintType 1`), `.3` uncoloured (`/PaintType 2`, colour comes
from `scn`'s operands). Verified against the published 2020 edition through the PDF
Association's errata, and listed in `docs/SPEC-MAP.md`'s "verified for the deferred
plans" table.

The four traps the clause states, each a test in the acceptance below:

- `/BBox` "shall be used to clip the pattern cell" — content that overflows the box does
  not leak past it.
- `/XStep` and `/YStep` "may be either positive or negative but shall not be zero", and
  may differ from the cell's size (an irregular lattice of regularly shaped cells).
- Nothing bounds the number of cells, so the renderer must — `MAX_FORM_DEPTH`'s shape: a
  named constant, a reported operator, a test that proves it fires.
- The cell's own content is a content stream with its own `/Resources`, painted in
  pattern space, then placed by `/Matrix`.

## Verdict

**No dependency, no licence, no wasm cost.** This is the cheapest item in the register:
the work is a nested-content run this renderer already does for Form XObjects, and the
paint path `/PatternType 2` already uses (`walk.rs:701`). The only new number is a cell
budget, and the only new cost is drawing work proportional to cells × cell area, which
the budget closes.

Two corpus pages carry `/PatternType 1`, checked by grep, and they are **not** alike:

| File | What it does with it | What its report says today |
| --- | --- | --- |
| `corpus/tier-a/colorkeymask.pdf` | paints with it: `/Pattern cs /p5 scn … re f`, a 200×267 cell at `/XStep 1905` (one cell), whose content is a colour-keyed image | "all operators supported" — while the fill silently paints the previous colour |
| `corpus/tier-a/ContentStreamCycleType3insideType3.pdf` | selects it from inside a **Type 3 char procedure** (`/Pattern cs /P1 scn`, corrected 2026-11-17 when the first implementation reached it) | "all operators supported" — the silence also hid a pattern fill inside a glyph |

So `ContentStreamCycleType3insideType3.pdf` cannot be this plan's acceptance: a round that
made tiling perfect would not change a pixel of it. `colorkeymask.pdf` is the corpus
proof — and it needs `docs/plans/colour-key-mask.md` as well, so it is the joint case, not
this plan's alone.

## Design

Where it lands, in the order the rounds build it:

1. **Parse and report** (`crates/pdf2-render/src/walk.rs`, `select_pattern`): a
   `/PatternType 1` dictionary becomes a `Tile` — `/BBox` (`rect_of`), `/Matrix`
   (`matrix_operand`, default identity), `/XStep`/`/YStep` (reject zero), `/PaintType`,
   `/TilingType`, `/Resources`, and the decoded stream (the same
   `document.decode_stream` the form path uses). A dictionary missing a required key, or
   with a zero step, is **reported** (`pattern`) instead of ignored. `state.pattern`
   becomes an enum — `Shading(Rc<Shading>)` stays, `Tile(Rc<Tile>)` is new — so the fill
   path can branch instead of guessing.
2. **The lattice** (`paint`, beside the shading branch at `walk.rs:701`): the path's
   polygons become a clip (`clip_mask`, exactly as the shading branch does); the cell
   lattice is generated in device space from the tile's matrix composed with the CTM,
   clipped to the path's device bounding box. For each cell: intersect the mask with the
   cell rectangle, then run the tile's content with its own resources through the same
   nested-run machinery as `run_form_nested` (`walk.rs:965`), counted against
   `form_depth` so a tile that paints its own pattern terminates.
3. **The budget**: `MAX_TILE_CELLS` beside `MAX_FORM_DEPTH`, checked before the loop; over
   it, the fill is reported (`pattern`) and skipped rather than attempted. The
   arithmetic that makes it necessary is `/XStep 0.01` over a whole page.
4. **`/PaintType 2`**: the fill colour comes from `scn`'s operands, so the selected colour
   travels in the state and the tile's content paints with it; `/PaintType 1` ignores
   those operands.
5. **`/TilingType` 1–3** — *settled 2026-11-17*. The clause's three values describe how
   spacing is adjusted to avoid distortion at a device resolution: 1 constant spacing
   with up to a pixel of cell distortion, 2 no distortion but spacing may vary by a
   pixel, 3 constant spacing with more distortion for speed. Type 2 keeps the exact
   pattern-space lattice, which is what the implementation always did; **types 1 and 3
   now snap each cell's device origin to a whole pixel**, so the spacing stays constant
   on the device grid and a cell moves by at most half a pixel. The earlier note said no
   fixture could measure this — that was wrong once `RenderOptions.scale` was considered:
   `tests/fixtures/tiling-type.pdf` renders the same 2-unit lattice at 1.25 px/pt (a 2.5
   px step) and device pixel 2 is **white** on the type-1 page against **half covered**
   on the type-2 page (`crates/pdf2-render/tests/tiling.rs`).

## Acceptance

Three checks, two of which are fixtures this plan specifies and `tests/fixtures/make_fixtures.py`
would generate:

- **`tests/fixtures/tiling.pdf`** — `/BBox [0 0 10 10]`, `/XStep 10`, `/YStep 10`,
  `/PaintType 1`, cell = left half black, right half white; a 20×20 rectangle filled at
  the origin paints four cells. The assertion is arithmetic, not "looks like a pattern":
  pixels at `(2, y)` black, `(8, y)` white, `(12, y)` black, `(18, y)` white, and the
  cell boundary at `x = 10`, for four rows across the fill.
- **Uncoloured**: a second page with `/PaintType 2` whose `scn` supplies a known red — the
  same pixel assertions with red in place of black, proving the colour came through
  `scn` and not from the tile.
- **Refusals**: a zero `/YStep`, a `/BBox` with no area, and a lattice over
  `MAX_TILE_CELLS` are each reported `pattern` and paint nothing — the tests assert the
  report string and that the render returns, rather than hanging.
- **`corpus/tier-a/colorkeymask.pdf`** — when this plan *and* the colour-key plan are
  built, the page's fill paints its one cell; until then, the report must at least stop
  lying, which is round 1 of this plan and is testable on its own.

## Rounds

1. **Parse, select, and report** — `Tile`, the dictionary keys, the enum in `state`, and
   the `pattern` note for a type or a key this renderer cannot honour. No painting. The
   acceptance is the report: `colorkeymask.pdf` stops claiming "all operators supported"
   for a fill it cannot paint.
2. **The lattice and `/PaintType 1`** — cells clipped by `/BBox` and by the path, the
   nested run, `MAX_TILE_CELLS`; `tests/fixtures/tiling.pdf` passes and the budget test
   fires.
3. **`/PaintType 2`, `/TilingType` as a hint, and the joint corpus case** — the uncoloured
   page, the note about 1–3, and `colorkeymask.pdf` once `colour-key-mask.md`'s work is in;
   it then joins `corpus/oracle-cases.txt` with that plan's case.
