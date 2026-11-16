# 0004 — Which image decoders land, and the MSRV they cost

- **Status:** accepted
- **Date:** 2026-11-16
- **Deciders:** repository owner
- **Inputs:** roadmap rank 9, `docs/RENDERING-QUALITY.md`, the corpus's four
  blank image pages (`docs/CORPUS.md`), the version rows in
  [`docs/FEATURES.md`](../FEATURES.md) §Outside facts.

## Context

Four committed corpus pages render blank because their images use a codec the
library does not decode: `bug_jpx.pdf` (JPX), `ccitt_EndOfBlock_false.pdf`
(CCITT G3/G4), `cmykjpeg.pdf` and `alphatrans.pdf` (DCT). The decoders exist as
permissive crates, and their versions were checked rather than recalled:

| Crate | Version | `rust-version` | Licence |
| --- | --- | --- | --- |
| `zune-jpeg` | 0.5.15 (newest stable; 0.5.16 is an rc) | **1.75** | `MIT OR Apache-2.0 OR Zlib` |
| `hayro-jpeg2000` | 0.5.0 | **1.92** | `Apache-2.0 OR MIT` |
| `hayro-jbig2` | 0.4.0 | **1.92** | `Apache-2.0 OR MIT` |
| `hayro-ccitt` | 0.5.0 | **1.92** | `Apache-2.0 OR MIT` |

Our MSRV is **1.87**. Three of the four decoders therefore cannot be added
without raising it, and the roadmap's rank 9 had not said what happens then.

## Decision

1. **DCT lands on `zune-jpeg`**: 1.75 clears 1.87 with room, the licence is on
   `deny.toml`'s allow-list, and it is the codec behind two of the four pages.
   It ships behind a feature so the wasm budget can be paid deliberately.
2. **JPX and JBIG2 wait for an MSRV move** — not for a feature gate. A feature
   whose dependency needs 1.92 makes `cargo install pdf2 --features jpx` require
   a newer compiler than the crate declares, which is the same lie as an
   understated `rust-version`, and it would double the toolchain matrix in CI.
   They land with the next MSRV decision, which is a release decision of its own.
3. **CCITT is written in-crate** rather than taken from `hayro-ccitt`: the
   format is small (1-D modified Huffman, 2-D G3 and G4), one corpus page needs
   it, `hayro-ccitt` has the same 1.92 wall, and the PDF parameter list is
   already written down (`/K`, `/Columns`, `/Rows`, `/EndOfLine`,
   `/EncodedByteAlign`, `/EndOfBlock`, `/BlackIs1`). Hand-writing a decoder is
   the cheaper promise here: no MSRV coupling, no wasm growth from a general
   crate, and a test that can be the corpus page itself.
4. **No decoder is allowed to bypass the decoded-bytes budget.** Each one is fed
   the image's stream under the same 256 MiB cap and the same pre-decode
   dimension check as the raw path (`docs/RENDERING-QUALITY.md`; the check landed
   in 1.2.1), so a codec cannot reintroduce the bomb the ceiling removed.

## Consequences

- Two of the four blank pages (`cmykjpeg.pdf`, `alphatrans.pdf`) move on a
  dependency; one (`ccitt_EndOfBlock_false.pdf`) on ~600 lines; one
  (`bug_jpx.pdf`) on a future MSRV bump, and it stays reported by name until
  then.
- The MSRV row in `docs/FEATURES.md` §Outside facts now carries this decision
  rather than the vaguer "cannot land until the MSRV moves", so the next person
  reading rank 9 does not re-derive it.

## Revisit when

The MSRV moves for another reason — then JPX and JBIG2 are two dependency bumps
and a wasm-budget recording, and this ADR is superseded rather than edited.
