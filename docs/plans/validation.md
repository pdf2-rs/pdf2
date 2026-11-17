# Conformance validation — a verdict, not an architecture

Deferred item: **conformance validation** (`docs/DEFERRED.md` §1, rank 13). This file is a
decision, as the round plan asked: what validation this crate can own and will, and what it
will not. It is written as a plan because the answer is "one of the two", with the work that
follows from it.

## Clause

There is **no single clause**: conformance is a family of external profiles (ISO 19005
PDF/A parts 1–4, PDF 2.0's own rules, PDF/UA) layered on ISO 32000-2. The *structural*
invariants underneath them are ordinary clauses this crate already reads: **`7.5.4`**
cross-reference table, **`7.5.5`** file trailer (`/Size`, `/Root`, `/Prev`), **`7.5.8`**
cross-reference streams (`/W`, `/Index`), **`7.3.8`** stream objects (`/Length` and its
extent), **`7.7.3`** page tree (`/Kids`, `/Count`, `/Parent`), **`7.6`** encryption.

## Verdict

**Structural checking: yes, in `pdf2-cli`'s `check` verb as a `--strict` mode. Profile
conformance (PDF/A, PDF/UA, PDF 2.0): refused, with the reasons below.**

`check` today is a *functional* test — open, walk the pages, decode each content stream,
extract text, report what fails (`crates/pdf2-cli/src/commands.rs:144`). It answers "can this
crate read the file", not "is the file self-consistent". Those are different questions and
the second is the one a writer, a pipeline or a reviewer asks before trusting a document.

The structural checks are cheap because the parser computes most of them already, and each
one is a fact, not a heuristic:

| Check | What it asserts | Where the fact lives |
| --- | --- | --- |
| xref integrity | every in-use entry resolves to an object whose header number and generation match, at the stated offset | `src/document.rs`'s `get_object`, run over every entry |
| `/Size` coherence | `/Size` ≥ the highest object number + 1, and no entry exceeds it | the trailer and the xref map |
| `/Prev` chain | terminates, with no cycle | `read_xref_chain` already bounds it at `MAX_XREF_SECTIONS` |
| stream extent | each stream's `/Length` matches the bytes before `endstream`, direct or indirect | `src/parser.rs`'s stream reader |
| page tree | walking `/Kids` reaches every leaf once, `/Count` equals the leaf count, each page has a `/MediaBox` | `src/page.rs`'s `collect_pages` |
| object streams | `/N` and `/First` agree with the entries the stream actually holds | `object_streams` in `src/document.rs` |
| encryption dictionary | the handler is one this crate resolves, and `/ID` is present when the revision needs it | `src/crypto.rs` |

**PDF/A is refused as a product, not as an impossibility.** What it would take: the ISO
19005 profile rules per part (version gating, forbidden constructs, metadata conformance,
font embedding, colour space rules), an XMP model this crate does not have, and a reference
implementation to be measured against — `veraPDF` **v1.30.4** (2026-11-10), which is Java and
a large application in its own right. The Rust prior art exists but is young and thin:
`pdf-compliance` 2.0.0 (756 downloads), `page_validation` 0.13.0 (3 979), `pdf-lint` 0.1.0
(172, Arlington model). None of them is a validator a document pipeline would stake
compliance on yet, and building one is a project whose buyer is different from this crate's.

**The honest middle** — and the verdict — is that this crate validates *its own
preconditions* and says so out loud, which is exactly what the read side already promises
("report, don't guess"). A PDF/A claim, if it is ever wanted, is a separate crate reading the
same objects: `pdf-compliance`-shaped, not a mode of `check`.

## Design (structural mode)

1. **`check --strict`** in `crates/pdf2-cli/src/commands.rs`: the existing functional run,
   plus the seven checks above. Each produces a named finding (`xref`, `size`, `prev`,
   `length`, `page-tree`, `objstm`, `encrypt`), never a bare "damaged".
2. **Every check is bounded**: the xref walk is capped by the object limit, `/Prev` by
   `MAX_XREF_SECTIONS`, the page tree by `page_limit` — a validator that can be made to hang
   by its input is worse than none.
3. **Exit codes**: `0` clean, `1` findings, `2` the file does not open — the same contract
   `check` has now, so a pipeline can adopt the mode without changing its own logic.
4. **JSON**: every finding carries the clause it tested (`"7.5.5"`), so a report can be
   traced to the rule rather than to our opinion.
5. **No repair**: the mode reports; it never rewrites (ADR 0006's incremental writer is the
   only write path, and a validator that silently fixes a file is a different tool).

## Acceptance

Fixtures are **built in the test and never committed**, for the same reason the decompression
bomb is (`tests/fixtures` is the benchmark's input set): a deliberately damaged file would be
decoded by every other measurement.

- **Each check has a case that trips it and one that does not**: a byte-offset shifted by
  one (`xref`), a `/Size` one short (`size`), a `/Prev` pointing at itself (`prev`), a
  `/Length` one too long (`length`), a `/Count` that disagrees with the walk (`page-tree`),
  an `/ObjStm` whose `/N` exceeds its contents (`objstm`), and a `/Encrypt` whose `/ID` is
  missing (`encrypt`). Every one asserts the *finding name and clause*, not just "failed".
- **The committed corpus is clean under `--strict`**: `cargo run -p pdf2-cli -- check
  --strict` over `corpus/tier-a/*.pdf` reports zero findings — the control that proves the
  checks are not trigger-happy. Files that fail this are a bug in the check, not in the file,
  until proven otherwise.
- **The functional mode is unchanged**: `check` without `--strict` produces byte-identical
  output on the corpus, so adopting the mode is opt-in.

## Rounds

1. **The checks and their fixtures** — `--strict`, the seven findings, the damaged-file tests
   built in the test, and the corpus-as-control run. One round if the parser exposes what it
   already knows; two if `/Length` and `/Size` need plumbing out.
2. **The output contract** — exit codes, the clause in every JSON finding, and a
   `docs/FEATURES.md` row for rank 13 that says "structural only, PDF/A refused" with a link
   here. Half a round.
3. **Only if a buyer appears**: the PDF/A question reopens as a separate crate, and this file
   is where the evidence for saying no is kept.
