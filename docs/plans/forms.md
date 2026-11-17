# Forms filling and annotation authoring

Deferred item: **forms filling and annotation authoring** (`docs/DEFERRED.md` §1, rank
12's second half). Reading and *rendering* annotation appearance streams shipped (rank
12b): `Walker::draw_annotations` draws `/Annots → /AP → /N`, with `/AS` choosing a state
for a button. What is missing is the write side, and that now has a mutation model to write
through ([ADR 0006](../adr/0006-editing-mutation-model.md)).

## Clause

**`12.7` Interactive Forms** — `.1` general, `.2` the interactive form dictionary
(`/Fields`, `/DA`, `/DR`, `/Q`, `/NeedAppearances`), `.3` field dictionaries (`/T`, `/FT`,
`/V`, `/DV`, `/Kids`, `/Parent`; `FT`, `Ff`, `V`, `DV`, `DA`, `Q` are **inheritable**) and
`.4` field types (`Tx` text, `Btn` button, `Ch` choice, `Sig` signature) — plus **`12.5.5`**
appearance streams and **`12.5.2`** annotation flags, since a field's widget is an
annotation. All are in `docs/SPEC-MAP.md`'s verified table or the map's existing rows.

`/NeedAppearances` is in the published dictionary (optional, default `false`): a flag
telling a *reader* to construct appearance streams for the widget annotations. That it is
a reader flag is the whole of the decision below.

## Verdict

**No dependency, no licence, no wasm cost.** The write path is overlay writes
(ADR 0006), and the read path already exists: the renderer draws appearances and the
document model walks dictionaries.

**The one real decision is what happens to the appearance.** The plan chooses:

| Field type | What the first model writes | Why |
| --- | --- | --- |
| `Btn` (checkbox, radio) | `/V` **and** `/AS` — the state names already exist in `/AP /N`, so the widget renders correctly with no regeneration | the appearance is authored by the *document*, and choosing a state is choosing a name |
| `Tx`, `Ch` | `/V` (and `/DV` on request) and `/AcroForm /NeedAppearances true` | the text's appearance depends on the field's `/DA` font — see the trap below |

**The trap, and why regeneration is a separate plan**: regenerating an `/AP` for a text
field means drawing the value with the `/DA` font, and a typical `/DA` is `/Helv 10 Tf`
— standard-14. This renderer draws standard-14 with its built-in bitmap alphabet, the
choice `docs/RENDERING-QUALITY.md` measures at **21.271 mean** against the engines' 1.400.
Writing that into a file would bake wrong glyphs into someone's document, which is worse
than leaving the appearance to a viewer that has a real Helvetica. So: **values now,
appearances when the standard-14 plan lands** — and `/NeedAppearances true` is the
standard's own mechanism for exactly this hand-off, not a shortcut.

**Flattening is recorded, not built**: our renderer draws annotations and content
separately, so a "flattened" page for *display* already works; a flattened *file* would
merge the appearance into the page's content stream, which is content-stream authoring —
a different item. The plan says so rather than promising a one-liner.

## Design

1. **The field tree** (`src/forms.rs`): walk `/AcroForm /Fields`, concatenating `/T` with a
   `.` for the fully-qualified name and **inheriting `FT`, `Ff`, `V`, `DV`, `DA`, `Q`** from
   ancestors as `12.7.3` requires — a plan that reads `/FT` only from the leaf fails every
   file that puts it on the parent, which is most of them. A widget is a field with `/Subtype
   /Widget` and a `/Rect`; a field may own several widgets.
2. **Read**: `Field { name, kind: Text|Button|Choice|Signature, value, states }` — enough to
   list and to fill, no more.
3. **Write** (`MutableDocument`, ADR 0006): `set_text(name, value)` writes `/V` as a PDF
   string (UTF-16BE with BOM when the value is not PDFDocEncoding-representable — the same
   encoding rule `src/object.rs` already implements for strings); `set_button(name, state)`
   writes `/V` as the name and `/AS` on each widget whose `/AP /N` carries that state.
4. **The flag**: any `Tx`/`Ch` write sets `/AcroForm /NeedAppearances true` unless the
   caller says otherwise; a `Btn`-only fill leaves it alone, because the appearances are
   correct already.
5. **Refusals**: an unknown field name, a type mismatch (`set_text` on a button), a button
   state the `/AP` does not carry, and `Sig` fields are each reported — a signature field
   cannot be "filled" and must not be turned into one.

## Acceptance

A generated fixture, `tests/fixtures/forms-fill.pdf`, built by
`tests/fixtures/make_fixtures.py` (no committed corpus page has an `/AcroForm`):

- **Nested names**: a text field at `Name.First` (a `/Kids` node with `/T /Name` and a child
  `/T /First`, the child carrying neither `/FT` nor `/DA`) so inheritance is exercised, and
  a checkbox `Agree` with `/AP /N << /On … /Off … >>`.
- **Round trip**: fill `Name.First` with `Ada`, set `Agree` to `/On`, save incrementally
  (ADR 0006), reopen the saved bytes — `/V` reads back `Ada`, the checkbox's `/V` is `/On`
  and its widget's `/AS` is `/On`, and `/AcroForm /NeedAppearances` is `true` because a text
  value changed.
- **The button renders checked without regeneration**: the page is rendered before and
  after the fill, and the checkbox's pixel changes from the `/Off` appearance to the `/On`
  one — the assertion that proves `/AS` is the mechanism.
- **The text does not**: the text field's appearance is unchanged after the fill (that is
  the consequence of the decision, asserted so nobody mistakes it for a bug), and the
  plan's ledger row says "values in, appearances pending the standard-14 plan".
- **Refusals**: unknown name, `set_text` on `Agree`, a state `/AP` does not carry, and `Sig`
  each return an error naming the field.
- **No oracle case**: the fixture is generated and compared against its own before/after,
  which is stronger than a pixel diff against engines that would each regenerate the text
  appearance differently.

## Rounds

1. **The field tree, read-only** — `src/forms.rs`, inheritance, widgets, the read API, and a
   unit test over the fixture listing `Name.First` and `Agree`.
2. **Writes** — `set_text`, `set_button`, the string-encoding rule, the `/NeedAppearances`
   policy, and the round-trip test through `save_incremental`.
3. **The rendering assertion and the refusals** — the checkbox before/after pixel, the four
   error cases, the fixture committed, and `docs/FEATURES.md`'s `12.7` rows split into
   "read" (shipped) and "fill" (shipped) with "regenerate appearances" left open and pointed
   at the standard-14 plan.
