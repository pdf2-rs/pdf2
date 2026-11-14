# AGENTS.md

Rules for anyone — human or agent — changing this repository. `README.md` says
what the crate is, `docs/RELEASING.md` is the authority on releases, and
`docs/RENDERING.md` on rendering coverage. This file says how to work here.

## Shape

`pdf2` is the library **and** the root of a Cargo workspace; tools live under
`crates/`. Do not move the library's sources out of `src/`: the published crate
paths and 1.0's layout depend on them.

| Crate | Version line | Published to |
| --- | --- | --- |
| `pdf2` (root) | library semver, currently 1.x | crates.io |
| `crates/pdf2-render` | independent, currently 1.0.x | crates.io |
| `crates/pdf2-cli` | independent, currently 1.0.x | crates.io |
| `crates/pdf2-wasm` | independent, currently 1.0.x | crates.io **and** npm (`pdf2-wasm`) |
| `crates/pdf2-bench` | `publish = false` | nothing |
| `crates/pdf2-wasm-smoke` | `publish = false` | nothing |
| `crates/xtask` | `publish = false` | nothing |

## Non-negotiables

- **No `unsafe` without a stated reason.** The workspace denies
  `unsafe_code`; the two exceptions (`pdf2-bench`'s counting allocator,
  `pdf2-wasm-smoke`'s C ABI) allow it at the crate root with a comment. The
  library keeps `#![forbid(unsafe_code)]`.
- **`no_std` keeps building.** `cargo +stable build -p pdf2
  --no-default-features --features alloc` must pass for
  `thumbv7em-none-eabihf`; `alloc`/`core` only, no new `std` dependency.
- **MSRV is 1.87.** CI tests it explicitly; do not raise it without a
  CHANGELOG note and a reason.
- **No new dependencies without a justification in the commit message.** The
  library has four, chosen so `no_std` works; `cargo deny` gates licences
  (allow-list in `deny.toml`) and advisories.
- **Every public item is documented.** `#![warn(missing_docs)]` plus rustdoc
  under `-D warnings`.
- **Fixtures are generated, not hand-edited.** `tests/fixtures/*.pdf` come from
  `tests/fixtures/make_fixtures.py` (`cargo fixtures`); the encrypted fixture
  comes from `examples/make_encrypted_fixture.rs`.
- **A golden image is a regression net, not an oracle.** Re-blessing
  (`cargo bless`) is a review event: explain in the commit message what moved
  and which assertion or clause justifies it. `docs/RENDERING.md` says which
  page proves what.
- `Cargo.lock` is git-ignored on purpose; do not commit it.

## Branching and pull requests

`main` is protected by the repository ruleset **"main: pull requests must pass
the gate"**: a direct push is refused (`push declined due to repository rule
violations`), force pushes and deletion are blocked, and history stays linear.
Every change reaches `main` through a pull request whose checks are green.

```sh
git switch -c fix/curve-operand-index      # one concern, one branch
cargo gate                                 # the same steps CI will run
git push -u origin fix/curve-operand-index
gh pr create --fill                        # or with a body that explains why
gh pr checks --watch                       # eleven required jobs, see the table below
gh pr merge --squash --delete-branch
```

- Branch prefixes: `feat/`, `fix/`, `docs/`, `chore/`, `release/`.
- The required checks are every job in `ci.yml`: Gate, Test (1.87), Test (beta),
  Package, both `no_std` jobs, `wasm`, `wasm package`, Licences and advisories,
  Semver and Benchmarks. A red one blocks the merge; a flaky one is re-run, not
  skipped.
- Squash-merge by default, so `main` reads as one commit per change; the PR body
  is the only place the reasoning can live, so write it.
- The ruleset does not cover tags. Releases still push `pdf2-v*`/`render-v*`/
  `cli-v*`/`wasm-v*`/`npm-v*` tags directly to the tagged commit on `main`, as
  described below; a tag must point at a commit that is already on `main`.
- Repository administrators can bypass the *check* requirement when merging a
  pull request (the ruleset's bypass mode is `pull_request`), which is for an
  emergency only; a direct push to `main` is still refused for everyone. A
  bypass is an incident note in the PR, not the normal path.
- Automated and agent work follows the same flow: open a pull request, let the
  checks run, merge when green. There is no "just this once".

## Warnings are bugs

A `warning:` line in a pipeline is a defect, not background noise: fix it in the
same change that introduced it, or open an issue. That includes manifest fields
(`homepage` must not repeat `repository` — cargo lints it), rustc and clippy
warnings (denied in `[workspace.lints]`), rustdoc warnings (`-D warnings`), and
`::warning` annotations. Third-party action runtimes are the one exception;
those are fixed by pinning a newer action version, which is why the versions in
`.github/workflows` are exact majors rather than floating.

## The gate

```sh
cargo gate          # exactly what the CI Gate job runs
cargo gate --full   # plus no_std, wasm modules and the benchmark budget
```

`cargo gate` runs, in order:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo test -p pdf2 --all-features`
5. `cargo doc --workspace --no-deps --lib` with `RUSTDOCFLAGS=-D warnings`

The same lint levels live in `[workspace.lints]` in the root `Cargo.toml`, so
clippy and `unsafe_code` failures surface in a plain `cargo build`/`cargo test`
as well; CI is the backstop, not the only check. The chores are implemented once,
in `crates/xtask`, and exposed as `cargo gate`/`fixtures`/`bless` by
`.cargo/config.toml`.

Hooks (optional, per clone): `scripts/install-hooks.sh` points git at
`.githooks/`, which formats-checks on commit and runs `cargo gate` before a push
(`SKIP_GATE=1` for a docs-only push). With `main` protected they protect a
feature branch from arriving at review with a red gate.

## Pipeline

Every push to `main` and every pull request runs `.github/workflows/ci.yml`.
Read the status before claiming a change works:

```sh
gh run list --workflow ci.yml --limit 3
gh run watch <run-id> --exit-status
gh run view <run-id> --log-failed
```

Every job below runs on `pull_request` as well as on `main`, and each is a
required check in the ruleset.

| Job | Guards | What a failure means |
| --- | --- | --- |
| Gate | fmt, clippy, tests, all-features, docs | the change is not ready; fix locally with `cargo gate` |
| Test (1.87, beta) | MSRV and the next toolchain | a 1.87 break or a new warning on beta |
| Package | `cargo package -p pdf2` and `-p pdf2-cli` | a file the tests need is missing from the tarball, or metadata is invalid |
| no_std | `alloc` build on x86_64 and thumbv7em | a `std` dependency crept into the library |
| wasm | browser and WASI smoke tests, size budgets | the wasm build broke or grew past the budget in `docs/PERFORMANCE.md` |
| wasm package | wasm-bindgen output, `check.mjs`, size budget | the npm artefact would be broken or too large |
| Licences and advisories | `cargo deny` | a new dependency outside `deny.toml`, or an advisory |
| Semver | `cargo-semver-checks` for `pdf2` | a breaking change to a published API (informational pre-1.0, blocking after) |
| Benchmarks | `pdf2-bench --check` against `docs/performance-baseline.json` | allocation/throughput regression past 10% |

A benchmark regression is either fixed or accepted deliberately: re-record the
baseline (`cargo run --release -p pdf2-bench -- --write-baseline
docs/performance-baseline.json`, then commit it) **and** say why in the commit
message. Adding a fixture to `tests/fixtures/` legitimately moves the fixture
totals; say so.

`fuzz.yml` runs nightly and on dispatch (`seconds` input); a crash is a bug in
the parser, not a flaky test. `.github/workflows/pages.yml` deploys the wasm
demo from `crates/pdf2-wasm/demo`.

## The dual publish model

Two registries, two tag namespaces, two workflows. They are independent: a
release may publish either, both, or only one.

### crates.io

`pdf2`, `pdf2-render`, `pdf2-cli`, `pdf2-wasm` are published by
`.github/workflows/publish.yml` using **trusted publishing** (OIDC, no stored
token), in dependency order: `pdf2` → `pdf2-render` → `pdf2-cli` / `pdf2-wasm`.

Every tag also gets a GitHub release, created by the workflow that published it
(`GitHub release` job): the title names the crate and version, the notes are the
`CHANGELOG.md` section plus the registry link, and a `-rc`/`-alpha` version is
marked pre-release. `npm-v*` releases are titled `pdf2-wasm <version> (npm)` and
say which crate version the wasm was built from. Only a `pdf2` release carries
the **Latest** flag; a tool or npm release sets `--latest=false`, so the landing
page keeps pointing at the library.

Two traps this design has already met:

- **A tag runs the workflow as it was at the tagged commit.** A tag created
  before the release job existed will never use it. Backfill such tags through
  `workflow_dispatch` with `release_tag: <tag>`, which runs the release job from
  `main` and skips publishing.
- **Deleting a tag drafts its release.** If a tag is deleted and recreated, the
  release comes back as a draft; the job edits with `--draft=false`, and a
  hand-made draft has to be published or it is invisible.

| Tag | Publishes | Version source |
| --- | --- | --- |
| `pdf2-v1.2.3` | `pdf2` | root `Cargo.toml` |
| `render-v1.2.3` | `pdf2-render` | `crates/pdf2-render/Cargo.toml` |
| `cli-v1.2.3` | `pdf2-cli` | `crates/pdf2-cli/Cargo.toml` |
| `wasm-v1.2.3` | `pdf2-wasm` (crate) | `crates/pdf2-wasm/Cargo.toml` |

- The tag's version must equal the manifest version and be on `main`, or the
  workflow fails on purpose. A mismatch is not a skip.
- `workflow_dispatch` with `crate: all` publishes whatever is not on the
  registry yet, in order; `dry_run` defaults to `true`. Use it for a train.
- A version already on the registry is skipped, so re-running a tag is safe.
- Never move a tag whose version is published. Never re-publish a version with
  different content.
- Packaging metadata matters: published crates need `readme`, `license`,
  `repository`, and path dependencies need an explicit `version` next to
  `path`, or `cargo publish` fails only at the last step.
- Trusted publishers must exist per crate on crates.io (owner action, recorded
  in `docs/RELEASING.md`); without one the workflow fails at upload and a local
  `cargo publish` with a token is the fallback.

### npm

`crates/pdf2-wasm/package.json` publishes as the npm package `pdf2-wasm`, from
`.github/workflows/npm.yml`, also with trusted publishing (OIDC) — provenance is
generated automatically. Tags are **`npm-v<version>`**, where the version is
`package.json`'s and may differ from the crate's.

- The workflow builds the wasm, runs `check.mjs` (API smoke test plus the size
  budget), refuses a tag that disagrees with `package.json` or is off `main`,
  and skips a version npm already has.
- `npm publish` locally needs an interactive login and 2FA; the first-ever
  publish was done that way (npm stages the version behind a `0.0.0-stage`
  placeholder until the browser flow finishes). Later publishes need nothing
  but the tag.
- A rebuild-only release (renderer fixes, manifest metadata) bumps only
  `package.json` and uses `npm-v*`; the crates keep their versions. `wasm-v*`
  belongs to the crate — do not reuse it for npm.
- The tarball is four files from `crates/pdf2-wasm/demo/pkg/`, which is
  generated and git-ignored: never publish from a clean clone without building
  it first (the workflow builds it).

### Release checklist

1. Bump the version(s) — library semver for `pdf2`, independent for tools; keep
   path-dependency `version` pins in step.
2. Add the `CHANGELOG.md` entry; state breaking changes, fixes and the reason.
3. `cargo gate --full` (and `cargo package -p <crate> --list` for what ships).
4. Land the version bump and CHANGELOG through a pull request (the parity of the
   release commit is what the tags will point at), then update `main` locally.
5. Tag per registry (`pdf2-v*`, `render-v*`, `cli-v*`, `wasm-v*`, `npm-v*`) on
   the merged commit and push the tags one namespace at a time; tags bypass the
   branch ruleset, so watch each run to green.
6. Verify from the registries, not from the logs: `cargo search`/the crates.io
   API, `npm view <package> version`, `curl -I` the docs.rs page, and the Pages
   demo.
7. GitHub releases appear from the tag runs themselves. For a tag that predates
   the automation (the workflow comes from the tagged commit, so old tags cannot
   run it), dispatch `Publish`/`Publish to npm` with `release_tag: <tag>` — it
   records the release from `main` without publishing anything. Then check
   `gh release list`: every tag must have a release, and `pdf2` should hold the
   "Latest" flag.

`docs/RELEASING.md` has the exact commands, the owner-only steps (trusted
publisher configuration, npm account) and the traps hit so far.

## Working style

- Keep changes reviewable: one concern per commit, message says **why** and what
  would have broken otherwise. Bug fixes name the failing case.
- Comments explain reasoning, not mechanics; do not restate code.
- Prefer parameters and fixtures over magic numbers; `tests/fixtures/` and
  `docs/RENDERING.md` show the pattern.
- Anything that changes rendering behaviour updates `docs/RENDERING.md` and, if
  it is a fix, the CHANGELOG.
- Anything that changes the public `pdf2` API runs `cargo semver-checks` (or
  relies on the Semver job) and is a minor/major bump once 1.0 semantics apply.
- Do not edit or commit loop state (`rounds/`, `RALPH_PROGRESS.md`,
  `.rounds-state.*`); it is git-ignored working material.
- Before saying a change is done: `cargo gate` green, the relevant CI run green,
  and the claim verified from the artefact (registry, PNG, CLI output) rather
  than from a log line.
