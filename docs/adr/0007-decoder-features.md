# ADR 0007 — The image decoders are features, and the demo is a second artefact

- **Status**: accepted, 2026-11-17
- **Supersedes**: the budget question [ADR 0005](0005-msrv-1-100.md) left open — its
  own last section, "What this ADR does not decide"
- **See also**: [`docs/plans/jpx-jbig2.md`](../plans/jpx-jbig2.md) — the design;
  [`docs/PERFORMANCE.md`](../PERFORMANCE.md) — the budgets

## Context

ADR 0005 moved JPEG 2000 and JBIG2 from "blocked on the MSRV" to "scheduled, size
decision pending", and wrote the three options out: raise the npm bundle budget to
~499 KiB, ship them behind non-default features, or take JPX alone. It also named
what it was *not* deciding — whether 137 KiB of gzip belongs in every page load for
two codecs most documents never use.

Nothing about the decoders changed since: `hayro-jpeg2000` **0.5.0** and
`hayro-jbig2` **0.4.0**, both `rust-version = "1.92"` (the floor is 1.100), both
`Apache-2.0 OR MIT` and already inside `deny.toml`'s allow-list, both `no_std` with an
allocator. Their default features are the expensive ones — `simd` pulls
`fearless_simd` (which uses `unsafe`) and `image` pulls `moxcms` (an ICC engine) — so
an integration takes `default-features = false, features = ["std"]`.

## Decision

1. **Both decoders are cargo features of `pdf2-render`**, off by default:
   `jpx = ["dep:hayro-jpeg2000"]`, `jbig2 = ["dep:hayro-jbig2"]`, each dependency
   `default-features = false, features = ["std"]`. The `dct` feature of 1.2 is the
   model, and the reason is the same: a caller that reads image-less documents should
   not pay for a codec.
2. **`pdf2-wasm` passes them through and keeps them off**:
   `jpx = ["pdf2-render/jpx"]`, not a re-declared dependency — re-declaring it with
   `default-features = false` would also drop `dct` and stop JPEG drawing. The npm
   package therefore ships the lean build and cannot decode JPX or JBIG2; that is the
   price of the default staying under its budget.
3. **The demo site is the second artefact.** `cargo xtask wasm-package` builds the
   lean bundle for npm and a decoder-enabled bundle for `demo/pkg`, and
   `crates/pdf2-wasm/check.mjs` enforces a budget per artefact, so the site cannot
   silently fall back to the lean build and show a blank page where it claims a codec.
4. **The oracle tool enables both unconditionally.** `crates/pdf2-oracle` is evidence,
   not a shipped artefact; without the features its CI build would render the JPX and
   JBIG2 cases with the decoders off and report `OVER` for the wrong reason.

## The measurements this rests on

Taken 2026-11-17 on the 1.4.0 tree, by the raw commands `docs/PERFORMANCE.md`
documents (`cargo build -p pdf2-wasm --release --target wasm32-unknown-unknown`, then
`wasm-bindgen --target web`), gzipped at level 9 the way `check.mjs` does:

| Bundle | gzip | Δ |
| --- | --- | --- |
| no decoders | **365 705** | — |
| `jpx` | **442 410** | +76 705 |
| `jbig2` | **428 390** | +62 685 |
| both | **504 186** | +138 481 |

ADR 0005's 1.3.0 figures were +76 052, +62 495 and +137 502 — reproduced here within
about a kilobyte, which is what makes this a decision rather than a quotation.

**The method is part of the number.** A first attempt linked the crates by parsing
only the codestream and page headers, and measured **+23 410** for both — because the
decode path was unreachable and link-time dead-code elimination removed it. A decoder
costs what its *pixels* drag in, so the measurement decodes (`Image::decode`,
`Page::decode`) before refusing. That is the round-1 shape of the code; the round that
paints them replaces the discard with the paint and keeps the number.

## Consequences

- A consumer that needs JPX or JBIG2 enables a feature and pays ~138 KiB of gzip;
  everyone else pays nothing, and `cargo tree -p pdf2-render` stays free of `hayro-*`.
- The npm default bundle keeps its 364 KiB budget (365 705 measured — ~7 KiB
  headroom), and the demo gets its own budget line rather than a raised shared one.
- Two wasm artefacts must be built, size-checked and tested; `cargo gate --full`
  already drives the demo, so the site's build is covered by the same gate.
- The browser (160 KiB) and WASI (187 KiB) budgets are untouched: those modules are
  built from `pdf2-wasm-smoke`, whose dependency tree has no `pdf2-render`.
- No new `unsafe` in the tree we own: `simd` is off, which is the feature that would
  have brought `fearless_simd`'s.

## What this ADR does not decide

ICC colour management (rank 10b), mesh shadings, and the standard-14 fonts. It also
does not decide whether the npm package should ever ship the decoders — if that
becomes wanted, the honest move is a second npm entry point, not a raised default.
