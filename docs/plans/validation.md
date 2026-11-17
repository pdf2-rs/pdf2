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

**Structural checking, and profile conformance, both in `pdf2-cli`.** `check --strict`
answers "is the file self-consistent"; `check --profile pdfa-2b` (later, per part) answers
"does it conform to a profile". **This reverses the 2026-11-17 verdict**, which refused
profile conformance as a different product: the owner decided PDF/A is not an option to
drop, so the honest plan is the one below rather than a refusal with good reasons.

The two are separable, and the split is what keeps the second from becoming a project this
crate cannot carry: the structural checks are facts the parser already computes, and the
profile rules are a **table of clauses with a reference implementation to check them
against**.

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

**What profile conformance costs, stated before it is built.** The ISO 19005 rules per part
are: version gating, forbidden constructs, metadata conformance (**an XMP model this crate
does not have**), font embedding rules, colour space rules, and the part-specific structural
requirements. The reference is `veraPDF` **1.30** (Open Preservation Foundation), which is
Java, and it is the oracle — the same relationship poppler has with this crate's renderer, so
verification is external and this crate stays a pure-Rust reader.

**Where the work goes**: a `pdfa` module in `pdf2-cli` (the library stays the reader) that
walks the profile's clauses over the object model, with a finding per clause — the same shape
`--strict` uses, so a report names a clause rather than an opinion. The Rust prior art
(`pdf-compliance`, `page_validation`, `pdf-lint`/Arlington) is worth reading and not worth
depending on; none of it is a validator a pipeline would stake compliance on yet.

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
- **PDF/A is measured against veraPDF, not against our reading of the clause**: a subset of
  the veraPDF corpus (CC BY 4.0) — one file per profile part and per expected verdict — is
  validated by both tools in CI, and the two verdicts agree case by case. A disagreement is
  the bug, whichever side has it.
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
2. **The output contract** — exit codes, the clause in every JSON finding, and the
   `docs/FEATURES.md` row for rank 13 that says what is in scope, with a link here. Half a
   round.
3. **PDF/A, per part** — the `pdfa` module in `pdf2-cli`, one part at a time (2b first: it is
   the one a pipeline asks for), each clause a named finding, each part gated on the veraPDF
   corpus subset agreeing case by case. One round per part, and the XMP model the metadata
   rules need is the first of them to land.
