# The deferred plans

[`docs/DEFERRED.md`](../DEFERRED.md) says what this library does not do yet, and why.
A file in this directory says **what doing it would take**: the clause, the decision
that comes before the code, the design, the proof, and the rounds.

The rule from the objective stands: **nothing here is an implementation**. These are
plans, written so that a later objective can be cut from them without re-reading the
register or the standard.

## The template

Every plan has these five headings, in this order. Each exists because of a way a plan
goes wrong without it.

### Clause

The ISO 32000-2 clause, with the sub-clause where it matters, **checked against the
published 2020 edition** — the PDF Association's errata quotes the clause headings —
and marked when a number comes from the ISO DIS draft instead, which is numbered
differently in chapter 10. *Why*: an implementation aimed at the wrong clause is
correct code for the wrong rule, and no test will say so.

### Verdict

The decision that comes before the work: licence, MSRV, wasm cost, or the refusal with
the numbers that justify it. *Why*: the expensive part of a deferred item is usually not
the code — it is a licence (`OFL-1.1` is not on `deny.toml`'s allow-list), a budget
(`+137 502` gzip for two decoders against a 356 KiB bundle) or a fidelity target
(8.455 mean against the engines' 2.000).

### Design

Where it lands: the files, the types, what it reuses, and the bounds it needs (a budget
shaped like `MAX_FORM_DEPTH`, not "should be fine"). *Why*: a plan without file names is
a wish, and a plan without a bound is how an untrusted file becomes a denial of service.

### Acceptance

The fixture and the assertion that prove it — a corpus page where one exists, a
generated fixture where none does (`tests/fixtures/make_fixtures.py` builds them), and
the command or oracle case that fails today and passes then. *Why*: "done" has to be
checkable by the gate or by a named command, not by reading the diff.

### Rounds

The sequence, each round-sized at ~40 turns, with the measurable or decidable step
first. *Why*: a plan that cannot be cut into rounds is not a plan; it is an ambition.

## The index

Ten plans, one per deferred item. "Planned in" in `docs/DEFERRED.md` points here until a
plan is written, then at the file itself. The plan names below are **not links yet** —
the round that writes a plan turns its cell into one, so this index never points at a
file that does not exist.

| Plan | Deferred item | Clause | State |
| --- | --- | --- | --- |
| `jpx-jbig2.md` | JPEG 2000 and JBIG2 decoding (rank 9) | `7.4.9`, `7.4.7` | not written |
| `icc.md` | ICC colour management (rank 10b) | `8.6.5.5`, `8.6.5.8`, `10.4` | not written |
| `tiling-patterns.md` | `/PatternType 1` tiling patterns (rank 10) | `8.7.3` | not written |
| `colour-key-mask.md` | Colour-key `/Mask` (rank 10) | `8.9.6.4` | not written |
| `tr-clipping.md` | The clipping half of `Tr` 4–7 (rank 10) | `9.3.6` | not written |
| `editing.md` | Editing an opened document (rank 11) | `7.5.6` | not written |
| `forms.md` | Forms filling and annotation authoring (rank 12) | `12.7.2`, `12.7.3`, `12.5.5` | not written |
| `validation.md` | Conformance validation (rank 13) | scope decision, no single clause | not written |
| `standard-14-fonts.md` | Standard-14 text with real outlines | no clause; a licence question | not written |
| `mesh-shadings.md` | `/ShadingType` 4–7, refused by name | `8.7.4.5.5`–`.8` | not written |

The clause numbers above are the ones confirmed against the published 2020 edition and
listed in [`docs/SPEC-MAP.md`](../SPEC-MAP.md); chapter 10 is the one place where the
ISO DIS draft the map's preamble followed is numbered differently, and `icc.md` says so.
