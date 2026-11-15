# Ship 1.2: text and rendering agree

"Done" means the items `docs/ROADMAP-NEXT.md` schedules for `1.2` — 2, 3, 4, 5,
6, 6b, 7, 2b and 10c (item 1 shipped 2026-11-15) — are finished together with the
release's own checkable outcomes: `cargo run -q -p xtask -- corpus` prints **21 of
23 fully readable** and **23 of 23 parsed**, the oracle job exists and fails a page
outside its tolerance, `docs/RENDERING.md` loses the composite-extraction limit,
a `/CropBox`/`/UserUnit` fixture passes the oracle, extraction returns reading
order, and the oracle job records render time against a committed budget. Out of
scope: everything ranked for `1.3`/`1.4` (image codecs, painting completeness,
Form XObjects, ICC, editing, annotations, validation), the MSRV move item 9 needs,
any new dependency without a `no_std` and wasm-size verdict, and rewriting
`docs/ROADMAP-NEXT.md` itself — this file plans the work, it does not re-rank it.

## Research

- The starting point is measured, not assumed: `cargo run -q -p xtask -- corpus` (2026-11-15) reports **23 files: 14 fully readable, 21 rendered**, with exactly three failure classes — 7 × composite (Type 0) fonts, 1 × cross-reference generation, 1 × encryption — so item 2's proof is a single number moving 14 → 21, and items 6 and 3 each own one file.
- Composite extraction is refused at `src/content.rs:253` and `/ToUnicode` is documented unimplemented at `src/content.rs:12`; `extract_text` returns `Result<String>` (`src/content.rs:343`), with no spans — the API decision `2b` needs comes before its code, and item 2 must not quietly invent one.
- `/ToUnicode` parsing has traps that other projects already paid for: hex strings appear with and without separators (`<0001><0070>` maps nothing in a whitespace tokenizer), `bfrange` increments the last byte of the destination and is *undefined* past `255 - (srcCode2 − srcCode1)`, destinations can be arrays, codes are 1–4 bytes chosen by the codespace, and overlapping ranges can demand millions of entries — <https://github.com/storytold/pdfcraft/pull/495>, <https://github.com/pdf-association/pdf-issues/issues/277> — the parser needs a byte-level lexer, checked arithmetic and hard bounds (≤ 1 Mi mappings, ≤ 64 Ki codespace ranges) from its first commit, not as hardening later.
- Encryption is revision 6 only: `src/crypto.rs:64` rejects everything else, and `README.md:48` says RC4/AES-128 are refused by design — item 3 adds a second security handler (revisions 2–4, RC4 and AES-128) rather than enabling a flag, and needs its own generated fixture because no corpus file is readable today.
- CFF/Type 1 is buildable inside the current MSRV: `skrifa` 0.50.0 and `read-fonts` 0.47.0 declare `rust-version = "1.85"`, are `MIT OR Apache-2.0`, expose `std` as an opt-out with `libm`, and skrifa carries a `type1` feature — crates.io API, 2026-11-15 — so item 4 does not inherit the `1.92` problem that blocks item 9's decoders.
- The CMap data item has a licence prerequisite before any code: Adobe's CMap resources are BSD-3-Clause and `deny.toml:13-19` allows no BSD entry, so item 5 starts with the allow-list change and the notice (or a home outside the crate).
- `/CropBox` is parsed and exposed (`src/page.rs:97`) but the renderer reads `/MediaBox` only (`crates/pdf2-render/src/walk.rs:183`), and `/UserUnit` appears nowhere in the page model (`grep -c user_unit src/page.rs` is 0) — item 6b touches the model and the renderer, and both halves need an oracle case to be provable.
- The 1.x xref edge is a single file: `corpus/tier-a/ContentStreamCycleType3insideType3.pdf` fails with `malformed cross-reference generation at offset 2111` — item 6 needs one fixture reproducing the generation pattern, not a general xref rewrite.
- The O(n²) object-stream trap the earlier research feared does **not** apply here: `compressed_object` calls `load_object_stream`, which early-returns once the stream is indexed (`src/document.rs:370-372`) — item 8 is about measuring scale under a budget, not repairing a known quadratic, and it is a `1.3` item either way.
- The oracle tool measures distance only — no `Instant`, no `Duration` anywhere in `crates/pdf2-oracle/src/main.rs` — so item 10c is a measurement and a committed budget added to the existing job, not a new tool.
- No Tier B job exists in `.github/workflows` (no `pdf.js`/`veraPDF` reference outside the corpus docs), so item 7 is a scheduled workflow over the pinned trees already identified (pdf.js `d60bc7426aa0`, Apache-2.0; veraPDF, CC BY 4.0), not new code.
- The ledger is now a gate: `cargo run -q -p xtask -- ledger` runs inside `cargo gate` and in CI, and it fails when a roadmap rank has no `docs/FEATURES.md` row — so every item finished here must update its row in the same change, and no round may add a rank without one.
- `cargo gate` is green on `main` today (run in this planning turn: 291-tests-and-then-some, fmt, clippy, docs, ledger) and is the check every round ends on.

## Milestones

- [x] M1: the `1.2` starting point is recorded from a run, not from prose — 23 corpus files, 21 render, 14 fully readable, three failure classes (`cargo run -q -p xtask -- corpus`, 2026-11-15).
- [ ] M2: composite-font extraction works on the corpus — the seven files become fully readable, and the corpus prints 21/23 readable and 23/23 parsed.
- [ ] M3: a generated fixture with bfchar, bfrange and an array destination extracts known text, its CMap has a codespace, and a malformed CMap is an error rather than a panic or a hang.
- [ ] M4: encryption revisions 2–4 are read — `encrypted-attachment.pdf` opens, and a generated revision-4 fixture (RC4 and AES-128) decrypts the same strings as its plaintext twin.
- [ ] M5: CFF and Type 1 programs render — the corpus CFF files stop reporting `font:FontFile*`, and their pages pass the oracle check.
- [ ] M6: CJK text extracts from `90ms_rksj_h_sample.pdf`; the CMap data is licensed (allow-list entry and notice) and its size is recorded against the wasm budget.
- [ ] M7: `/CropBox` and `/UserUnit` are honoured — a fixture with both renders as the oracle renders it.
- [ ] M8: the 1.x cross-reference generation edge parses, and a fixture with the same generation pattern keeps it parsed.
- [ ] M9: extraction quality has a written API decision (text only, or spans with boxes) and one corpus page extracts in reading order with correct spaces.
- [ ] M10: a scheduled Tier B corpus job publishes a failure histogram, and the oracle job records per-page render time against a committed budget.

## Round plan

- [ ] Q1 -- [Q1, do now] Item 6b: `/CropBox` and `/UserUnit` in the page model and the renderer (0.5 rounds), with an oracle case that proves both.
- [ ] Q2 -- [Q1, do now] Item 6: the 1.x cross-reference generation edge, with a fixture that reproduces it (1 round).
- [ ] Q3 -- [Q2, split] Item 2, chunk 1: the `/ToUnicode` CMap parser, standing alone (finding 3).
  - [ ] a byte-level CMap lexer (hex strings with and without separators, names, comments) with the two hard bounds
  - [ ] bfchar, bfrange, array destinations and codespace-driven code width; malformed input is an `Err`
  - [ ] unit tests plus the generated fixture page (M3) — no wiring into `text()` yet
- [ ] Q4 -- [Q2, split] Item 2, chunk 2: wire it in and move the number (findings 1–2).
  - [ ] two-byte code splitting for Identity-H, `/DescendantFonts`, CID widths
  - [ ] `cargo xtask corpus` at 21 readable / 23 parsed; `docs/CORPUS.md`, the `docs/RENDERING.md` limit and the CHANGELOG updated
- [ ] Q5 -- [Q2, split] Item 3: revisions 2–4 (RC4, AES-128) and the revision-4 fixture (finding 4).
- [ ] Q6 -- [Q2, split] Item 4: CFF/Type 1 through `skrifa`, after the dependency's `no_std` and size verdict (finding 5).
- [ ] Q7 -- [Q2, split] Item 5: the allow-list and notice first, then the CMap data and CJK extraction (finding 6).
- [ ] Q8 -- [Q2, split] Item 2b: the API decision, then reading order and spaces on a corpus page (finding 2).
- [ ] Q9 -- [Q3, batch] Item 7: the scheduled Tier B corpus job and its histogram (finding 11).
- [ ] Q10 -- [Q3, batch] Item 10c: render time in the oracle job, plus the committed budget (finding 10).

## Checks

- `cargo gate`
