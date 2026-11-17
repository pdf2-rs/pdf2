# Mesh shadings (`/ShadingType` 4–7)

Deferred item: **mesh shadings** — refused **by name** in the renderer today
(`crates/pdf2-render/src/shading.rs:127`, `unsupported: mesh shadings (types 4 to 7)`) rather
than drawn wrong. Types 1–3 (function-based, axial, radial) are shipped; these four are the
triangle and patch meshes and no corpus page uses one.

## Clause

**`8.7.4.5.5`–`8.7.4.5.8`**, from the table of contents quoted in the objective's research:

- **`.5` Type 4** — free-form Gouraud-shaded triangle meshes: a stream of flags and vertices,
  each triangle carrying three colour specifications.
- **`.6` Type 5** — lattice-form Gouraud-shaded triangle meshes: a grid whose triangles are
  implied, with `/VerticesPerRow`.
- **`.7` Type 6** — Coons patch meshes: twelve control points per patch and four colours.
- **`.8` Type 7** — tensor-product patch meshes: sixteen control points, with the extra four
  steering the interior.

All four share the same keys: `/BitsPerCoordinate`, `/BitsPerComponent`, `/Decode` (which maps
the *coordinates* and the *colours* separately), `/BitsPerFlag` for 4/6/7, and an optional
`/Function`.

## Verdict

**No dependency, no licence, no wasm cost** — the shading module already parses colour spaces
(`parse_space`), functions (`parse_function`) and matrices, and `paint()` already takes a
clip and a blend mode. What is new is a *second* rasterisation: the existing shading samples a
function per pixel, while a mesh interpolates colour across triangles.

**No corpus page uses one** (checked by grep across `corpus/tier-a`), so there is no oracle
case to inherit and the fixtures are the whole proof. The refusal today is honest — a page
with a type-4 mesh reports rather than painting a flat colour — and that is why this is a plan
and not a bug.

## Design

1. **Decode** (`shading.rs`, a new `Mesh` kind): read the stream's bits at
   `/BitsPerCoordinate` and `/BitsPerComponent`, mapping each through its half of `/Decode`;
   assemble triangles for type 4 and 6/7 (flags: 0 = new triangle, 1/2 = continue from the
   previous edge) and the implied grid for type 5.
2. **Patch tessellation** (types 6 and 7): a Coons/tensor patch is a bilinear-plus-correction
   surface; subdivide each patch into a fixed grid of quads (the plan states the subdivision,
   e.g. 8×8, rather than choosing per pixel) and emit two triangles each.
3. **Rasterise**: a barycentric triangle painter into the current clip, interpolating the
   three vertex colours (or sampling the `/Function` at the interpolated parameter when one is
   present), with the fill alpha and blend mode `paint()` already carries. Degenerate
   triangles (zero area) are skipped, not divided by.
4. **Bounds**: the vertex count is bounded by the decode budget already applied to the stream,
   and the tessellation multiplies it by a fixed factor — the plan names that factor so a
   crafted mesh cannot turn one stream into a billion triangles.
5. **Refusal becomes success only where it is proved**: any malformed mesh (short stream,
   flag out of range, a patch with missing control points) keeps the `mesh shadings` report
   for that operator rather than painting the part it could read.

## Acceptance

- **Type 4**: a generated fixture with two triangles sharing an edge, vertex colours red,
  green, blue — the assertion is the three corner pixels and the midpoint of the shared edge
  (a 50/50 blend), which no flat fill can satisfy.
- **Type 6**: one Coons patch whose four corners are the four colours, so the centre is their
  average within a tolerance the subdivision sets — the plan states that tolerance rather than
  "looks smooth".
- **Type 5 and 7**: a grid and a tensor patch with the same corner assertions, smaller.
- **Refusals**: a truncated stream and an out-of-range flag each produce the `mesh shadings`
  report and paint nothing.
- **No oracle case** unless a corpus page appears; the fixtures are compared by arithmetic,
  like `tests/fixtures/shading.pdf` already is.

## Rounds

1. **Type 4** — decode, the triangle painter, the fixture, and the refusal cases. This is the
   bulk of the work and the type the others reduce to.
2. **Types 5 and 6** — the lattice grid and the first tessellation, with their fixtures.
3. **Type 7** and the docs — the tensor patch, `docs/RENDERING.md`'s row losing "reported", and
   the ledger row for `8.7.4.5.5`–`.8`.
