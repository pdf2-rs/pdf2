# Standard-14 text with real outlines — a refusal, with the numbers

Not a deferred *item* in `docs/DEFERRED.md` §1 but a deferred *non-decision* there (§2:
"Rendering with system fonts for standard-14 text — measured, not decided"). This file
decides it: **refused for now**, and here is what the refusal costs and what would change it.

## Clause

No clause needs implementing: a standard-14 font carries **no program**, and the reader is
expected to supply a face with the same metrics (`9.6.2.2` names the fourteen, and `9.6.2.1`
says the viewer's substitution is out of scope of the standard). What this crate does instead
is draw the code with a **5×7 bitmap alphabet** (`crates/pdf2-render/src/font.rs`'s
`builtin_glyph`), which is the honest fallback and the one the oracle gate cannot compare.

## Verdict — refused, with the numbers

**What it costs today, measured** (`docs/RENDERING-QUALITY.md`): `ShowText-ShadingPattern.pdf`
sets two of its four rows in standard-14 Helvetica and two in an embedded Arial subset; with
the fallback the page measures **21.271 mean** against the engines' 1.400 — entirely from the
two fallback rows. That page is therefore proven by its report and its ink, never by a pixel
comparison, and it is the only page in the corpus that says what the gap is worth.

**The licence landscape, checked rather than recalled:**

| Source | Licence | Usable under `deny.toml`? |
| --- | --- | --- |
| `pdf-standard-fonts` 1.4.0 (all fourteen programs) | `AGPL-3.0-only OR PDFluent-Commercial` | **no** — copyleft, and a commercial licence is not ours to take |
| `pdf-substitute-fonts` 1.1.1 (metric-compatible replacements) | `AGPL-3.0-only OR PDFluent-Commercial` | **no**, same |
| `pdf-base14-metrics` 0.0.2 (metrics only, **no outlines**) | `MIT AND APAFML` | **no** — APAFML is not on the allow-list; and metrics are what we already have from `/Widths` |
| Liberation (Arimo/Tinos/Cousine — metric-compatible with Arial/Times/Courier) | `SIL OFL 1.1` | **not yet** — OFL is not on the allow-list; adding it is a decision, not a burden |

**The size, bounded by a figure already in hand:** the crate that carries **all fourteen**
programs is **265 796 bytes** on crates.io — and a `.crate` is compressed, so that is
roughly the gzip cost of shipping every one of them: **+73 % on a 356 KiB bundle**. One face
(a single Helvetica style) is about a fourteenth of that, ≈19 KB compressed, and the three
faces a typical page needs (Helvetica, Times, Courier, regular) ≈57 KB — which is the shape
any future decision should start from.

**So the refusal is three independent "no"s, and the plan says which one is cheapest to
lift**: the licence (add `OFL-1.1` to `deny.toml` and a notice), the size (≈57 KB for three
faces, measured properly rather than divided), and the fidelity risk (the fallback affects
*every* standard-14 page, so changing it moves goldens and oracle distances at once — a
measured before/after, not a swap).

## What would change the answer

1. **Demand evidence**: a user-visible failure caused by the fallback — text *positions* from
   `/Widths` are already right, so the gap is glyph shape only, and nobody has reported it.
2. **A licence decision**: adding `OFL-1.1` is compatible with this project's permissive-only
   policy (it is a permissive font licence with a reserved-name clause) but it is a policy
   change and gets recorded in `deny.toml` with its own note, like `BSD-3-Clause` did.
3. **A subset plan**: three faces, or a single face, with the wasm cost measured by the ADR
   0005 spike method rather than estimated.

## Acceptance (if it is ever built)

- `ShowText-ShadingPattern.pdf`'s standard-14 rows render, and its 1×1 mean against the
  engines moves from 21.271 toward the inter-oracle spread — the number that justifies the
  work, and the one that proves it.
- A generated fixture whose text is measured for *advance width* against the spec's standard
  metrics, so a metrically wrong substitute fails even if it looks right.
- Goldens re-blessed with a written before/after, as the anti-aliasing round did.

## Rounds (only if the answer changes)

1. The licence change and a **measurement**: three faces behind a temporary feature, the npm
   delta by ADR 0005's method, reverted.
2. The face loader (a `Font` variant over an embedded program, reusing `ttf-parser`'s path)
   and the standard-14 name→face table.
3. The re-bless and the oracle movement, with the numbers in `docs/RENDERING-QUALITY.md`.
