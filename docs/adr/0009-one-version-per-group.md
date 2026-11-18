# 0009 — One version for the whole shipped group

- **Status:** accepted
- **Date:** 2026-11-18
- **Deciders:** repository owner
- **Supersedes:** the per-crate version lines in every published manifest, and the
  "independent versioning" passages of `AGENTS.md` and `docs/RELEASING.md`
- **Inputs:** the 1.6.0 release (`rounds/plan-20261118-release-1.6.md`), where
  `pdf2`/`pdf2-render`/`pdf2-cli` shipped at 1.6.0 while `pdf2-wasm` stayed at 1.4.0

## Context

The 1.6.0 release published three crates and left `pdf2-wasm` two minors behind. The
crate's published tarball is a **compiled snapshot**: `pdf2-wasm 1.4.0` on crates.io
declared `pdf2 ^1.5.0` and its `.wasm` contained 1.5.0's code, so web users were handed
an older library than the version number suggested — and `cargo add pdf2-wasm` quietly
resolved `pdf2 1.6.0` beside it, which reads as if they had matched. The version broke
away from the pair it used to keep (`wasm-v1.1.0` shipped library 1.2.0,
`wasm-v1.2.0` → 1.3.0, `wasm-v1.3.0` → 1.4.0, `wasm-v1.4.0` → 1.5.0, then nothing),
and nothing in the tree made that visible: four manifests and a `package.json` held
five copies of a number that had to agree.

## Decision

1. **One version for the group.** `pdf2`, `pdf2-render`, `pdf2-cli` and `pdf2-wasm`
   carry the same number, always. `[workspace.package] version` is the only Rust copy
   and every published manifest says `version.workspace = true`; the unpublished
   chores (`xtask`, `pdf2-bench`, `pdf2-oracle`, `pdf2-wasm-smoke`) keep their own
   `0.0.0`/`1.0.0`, because they are not shipped and a fake number would be a lie
   about what they are.
2. **The npm package moves with them.** `crates/pdf2-wasm/package.json` cannot inherit,
   so it is the one copy written by hand — and the one a check must guard.
3. **`cargo run -q -p xtask -- versions` is that check**, and it runs in the gate: the
   workspace number, each shipped crate's `cargo pkgid`, `package.json` and the newest
   `## [x.y.z]` CHANGELOG heading must agree, and a manifest that stops inheriting is
   itself a failure — a literal version line is the drift this forbids.
4. **Tags stay per crate**, because `publish.yml` maps a tag prefix to one crate
   (`pdf2-v*`, `render-v*`, `cli-v*`, `wasm-v*`, `npm-v*`); every tag of a release
   carries the same number, and they are still pushed one at a time.
5. **The semver line is the group's**, not each crate's: a patch release of the
   library is a patch release of the tools that wrap it, and a breaking change
   anywhere forces a major bump everywhere.

## Consequences

- The two-behind state cannot be created again by a bump that misses a crate; the
  manifests make it unwritable and the gate catches the two files Cargo cannot reach.
- Every release publishes all five artefacts, including a `pdf2-wasm` rebuild that
  changed nothing — the cost of one number is five publishers, five GitHub releases
  and one extra wasm build. A library patch is no longer a one-crate release.
- A breaking change in a tool now forces a major bump on the library, which says more
  than the change did. Accepted: the artefacts are one product page, one demo and one
  documentation set.
- `pdf2-wasm` jumps 1.4.0 → 1.6.0 to rejoin the line; nothing in its API breaks, so
  the jump is a number catching up rather than a compatibility claim.

## Alternatives rejected

- **Independent versions plus a cross-check** (keeping five copies and validating
  them): the check would catch drift after the fact, and the copies would still invite
  it — the mechanism that produced 1.4.0 against 1.6.0 stays available.
- **One version, computed at publish time** (`cargo release`-style): a tag would have
  to carry a version the manifests do not, which breaks `publish.yml`'s rule that the
  tag equals `cargo pkgid -p <crate>` — the check that stops a wrong crate shipping.
- **Nothing, and document the rule**: exactly as reliable as the process that left the
  wasm package behind, and this decision exists because that process ran once.
