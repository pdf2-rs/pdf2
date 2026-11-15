# Our gaps, ranked by value per round

Input to the ADR (M5). It ranks the gaps `docs/COMPETITORS.md` exposed by how much
user pain they remove per round of work, and it states what it does *not* know.
Demand is a proxy until the corpus run in Q4 measures our own users' files; cost
is an estimate from code inspection and crate availability, not from a spike
(that is Q5, for whichever item wins).

## Method

- **Demand 1–5**, from evidence that someone other than us wanted the capability:
  5 = the gap blocks a whole class of files, or a dedicated competitor crate
  serving it has more than 8M downloads; 4 = more than 2M downloads, or more than
  20 competitor issues; 3 = 0.5–2M downloads, or 5–20 issues; 2 = under 0.5M and
  few issues; 1 = cosmetic.
- **Cost in rounds**, where one round is about 40 turns: counted from the code
  that has to change (file:line), the vetted crates that already do the hard part
  (checked on crates.io), and any data that has to ship.
- **Score = demand ÷ cost** — value per round. Ranking by demand alone would just
  restate the competitor table.
- **Tie-break, stated before it is needed**: prefer (1) no new dependency,
  (2) code we already have, (3) more independent evidence of pain.

## The scorecard

| Rank | Gap | Demand | Cost (rounds) | Score | Demand evidence | Cost evidence |
| --- | --- | --- | --- | --- | --- | --- |
| **0** | **Reads PDF 1.x at all** (the header gate) | 5 | ? (spike first) | **n/a** | The corpus run (`docs/CORPUS.md`, 2026-11-14) rejected **23 of 23** real files at the header: `malformed only PDF 2.x is supported at offset 0`. Every competitor in `docs/COMPETITORS.md` reads 1.x; the real world is PDF 1.4–1.7. | `src/document.rs:486` requires the header version to start with `2.`, and `src/document.rs:967` asserts `%PDF-1.7` is an error — the gate is one condition, but what breaks behind it (RC4/AES-128 encryption, image filters, composite extraction) is unknown until measured. |
| 1= | **Composite-font text extraction** (Type 0/CID, `/ToUnicode`, predefined CMaps) | 5 | 2.5 | **2.0** | `pdf-extract` 8 236 722 downloads; 42 of its issues mention "font", 15 Type 0/CID/CMap, 13 `/ToUnicode`; 19 `hayro` issues ask for text extraction [i1] [i2] | `src/content.rs` has only `Font { encoding }` and rejects Type 0 at `src/content.rs:253`; the renderer already parses `/CIDToGIDMap` and `/W` (`crates/pdf2-render/src/walk.rs:132,799,854`) to reuse; CMap parsing is `bfchar`/`bfrange` with no new crate. Predefined CMaps need shipped data (~250 KB, hayro's own figure) and are deferred out of the first slice. |
| 1= | **CFF / Type 1 embedded programs** | 4 | 2 | **2.0** | Renderer reports `font:FontFile`/`font:FontFile3` instead of drawing (`docs/RENDERING.md`); the pure-Rust font stack is heavily used: `skrifa` 50 016 463 downloads, `read-fonts` 52 860 543 [cr] | `crates/pdf2-render/src/font.rs` needs a variant and a smoke test; `skrifa`/`read-fonts` (MIT OR Apache-2.0) already parse CFF and Type 1, with `std` optional and `libm` for `no_std` [k1] |
| 3 | **Image codecs** (DCT, JPX, CCITT, JBIG2) | 5 | 3 | **1.67** | Scanned PDFs cannot be drawn at all (`src/filter.rs:41`); every reference engine has them; the decoders are already written and widely used: `hayro-jpeg2000` 6 679 726, `hayro-jbig2` 6 484 186, `hayro-ccitt` 7 349 884, `zune-jpeg` 163 700 801 [cr] | Four permissive crates, each with `std` as an opt-out feature (checked in their `Cargo.toml`); the work is dependency wiring behind features, a decoder→`Image` path in `crates/pdf2-render/src/image.rs`, fixtures and goldens. Risk: four dependencies widen the licence and wasm-size surface. |
| 4 | **Robustness at scale and on hostile input** | 3 | 2 | **1.5** | `hayro` lists timeouts/OOM as remaining work; a 65 542-object stream took 37 s before an O(N²) fix [i3]; we have five fuzz targets but no complexity test [repo] | A synthetic large-object-stream fixture, a time/iteration budget test, parser limits, and a fuzz target for object streams |
| 5 | **Editing an opened document** | 5 | 5+ | **1.0** | `lopdf` 30 289 120 downloads (14 766 962 recent) is the ecosystem's editing crate, and nothing in Rust edits *and* renders [cr] [gh] | `src/writer.rs` builds only new documents (`Document::new`, `add_page`, `save` at lines 66, 99, 162); editing needs object mutation, incremental saves (`/Prev`, xref deltas) and preservation of unknown objects — the most expensive item here |
| 6 | **Shadings, patterns, transparency** | 4 | 6 | **0.67** | Every modern PDF uses at least one; `hayro` covers them; ours reports `gs` unsupported (`docs/RENDERING.md`) | Our own rasteriser work: blend modes, soft masks, gradient shading, tiling, and a golden per feature — no crate shortcut |
| 7 | **Annotations and forms** | 3 | 5 | **0.6** | `pdfium-render` 3 954 648 downloads covers them; Rust demand is mostly viewing and filling, not authoring [cr] | `/Annots` model, appearance streams, widget/field tree, rendering and filling; nothing in this repo today |
| 8 | **Conformance validation** (PDF/A, PDF 2.0 rules) | 2 | 4 | **0.5** | veraPDF is the reference and is Java; `qpdf --check` has 5 588★; no Rust validator appears in the competitor survey [gh] | A rule engine over the object model plus a decision about which profile to target — arguably a different product from a reader |

Ties are broken by the rule above: **composite-font extraction beats CFF/Type 1**
because it needs no new dependency, reuses the renderer's CID parsing, and has
the larger pile of independent complaints (`pdf-extract`'s 42 font issues against
none for adding a font crate); **image codecs beat both on a different axis** —
they are the only item that unlocks a file class we cannot open at all, which the
ADR must weigh explicitly rather than let the tie-break rule decide.

Rank 0 is not a ninth gap in the same sense as the others: it is a
**precondition**. Until it is opened, no corpus file reaches the code paths gaps
1–8 live in, and no cost estimate for them can be tested — which is why its cost
is left unestimated and the M7 spike is retargeted at it.

## What this implies, without deciding

- **Rank 0 first, as a measurement**: relaxing the header gate answers, with
  numbers, how much of the gap list is actually reachable and what breaks next.
  Carrying the 2.0-only policy into the next version is a decision that has to be
  argued against the corpus, not assumed.
- **Gap 1 next** is the cheapest defensible move: no dependency, reuses code we
  wrote for the renderer, and turns "the demo says text extraction unavailable"
  into a capability on the exact files the renderer already draws.
- **Gap 2 is the biggest visible hole** and the least code: four crates do the
  work, so it is dependency-risk rather than effort-risk. It belongs in the same
  release as gap 1 if the ADR wants a "opens real-world files" theme.
- **Gap 3 (editing) is the largest demand and the largest cost**; it should be
  scheduled deliberately as a theme of its own, not slipped in beside features.
- **Gap 4 is the cheap credibility item**: two crates, one enum variant, and the
  renderer stops reporting the fonts that print-era PDFs actually embed.

## Assumptions to verify

- The decoders' `no_std` readiness: `std` is an opt-out feature in each
  `Cargo.toml`, but none has been built for `thumbv7em-none-eabihf` inside our
  feature matrix yet.
- `skrifa`/`read-fonts` with our MSRV (1.87) and targets; `libm` is the `no_std`
  path.
- Demand proxies measure other projects' users, not ours; the Q4 corpus run will
  measure failures on real files and should re-rank 1–3 if it disagrees.
- Cost estimates are round counts from code inspection, not from timed spikes.

## Provenance

Checked **2026-11-14** with:

```sh
for c in pdf2 hayro pdf lopdf printpdf pdfium-render mupdf pdf-extract \
         hayro-jpeg2000 hayro-jbig2 hayro-ccitt zune-jpeg skrifa read-fonts; do
  curl -s "https://crates.io/api/v1/crates/$c" -H 'User-Agent: competitor-scan'
done
gh api -X GET search/issues -f q='repo:jrmuizel/pdf-extract Type0 OR CID OR CMap' -f per_page=1
gh api -X GET search/issues -f q='repo:jrmuizel/pdf-extract ToUnicode' -f per_page=1
gh api -X GET search/issues -f q='repo:jrmuizel/pdf-extract font' -f per_page=1
gh api -X GET search/issues -f q='repo:LaurenzV/hayro text extraction' -f per_page=1
gh api -X GET search/issues -f q='repo:pdf-rs/pdf Type0 OR CID' -f per_page=1
```

Sources: [cr] crates.io API (`downloads`, `recent_downloads`, `versions[0].license`,
`features`); [gh] `api.github.com/repos/{owner}/{repo}`; [i1] GitHub issue search
counts above; [i2] <https://github.com/LaurenzV/hayro> README ("remaining aspects:
robustness, performance, API surface"); [i3]
<https://github.com/storytold/pdfcraft/pull/2427>; [k1] `skrifa/Cargo.toml`
(`std` optional, `libm` feature) and `read-fonts`; [repo] `src/filter.rs:41`,
`src/content.rs:12,253`, `src/writer.rs:66,99,162`,
`crates/pdf2-render/src/walk.rs:132,799,854`, `docs/RENDERING.md`.
