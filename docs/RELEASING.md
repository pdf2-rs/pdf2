# Releasing

`pdf2` publishes to crates.io from GitHub Actions using
[trusted publishing](https://crates.io/docs/trusted-publishing/): the publish
job proves its identity over OIDC, receives a ~30-minute token and revokes it
when the job ends. There is no crates.io API token in this repository or in its
secrets.

## One-time setup

The crate already exists on crates.io (0.1.0 from April 2023, then the
0.2.0-alpha series), which is the prerequisite for trusted publishing.

1. crates.io → `pdf2` → Settings → **Trusted Publishing** → *Add* → GitHub:

   | Field | Value |
   | --- | --- |
   | Repository owner | `pdf2-rs` |
   | Repository name | `pdf2` |
   | Workflow filename | `publish.yml` |
   | Environment | `release` (optional, but must match the workflow if set) |

2. GitHub → Settings → **Environments** → new environment `release`. Optionally
   add required reviewers or restrict deployment branches; the publish job runs
   in this environment, so those rules apply before a token is requested.

3. Nothing else: no `CARGO_REGISTRY_TOKEN` secret is needed.
   `.github/workflows/publish.yml` uses `rust-lang/crates-io-auth-action@v1`,
   which fills `CARGO_REGISTRY_TOKEN` for the single publish step.

## Workspace crates

`pdf2` is the root package and the tools live under `crates/`. Each crate is
published on its own, and **the first version of every new crate must be
published by hand**: crates.io only accepts a trusted-publisher configuration
for a crate that already exists.

1. Commit first — `cargo publish` refuses to run from a dirty tree.
2. `cargo publish -p pdf2-cli` (uses the token in `~/.cargo/credentials`, or
   `CARGO_REGISTRY_TOKEN`).
3. crates.io → that crate → Settings → Trusted Publishing → GitHub, with the
   same owner (`pdf2-rs`), repository (`pdf2`), workflow (`publish.yml`) and
   environment (`release`) as the library.
4. Later versions can be published by the workflow; moving the workspace
   publish order and per-crate tags into `publish.yml` is Q10 in
   `rounds/ROADMAP.md`.

Current state (2026-11-14):

| Crate | Version | First publish |
| --- | --- | --- |
| `pdf2` | `0.7.0-rc.1` | trusted publishing from a `pdf2-v*` tag |
| `pdf2-render` | `0.1.0-rc.1` | manual, token |
| `pdf2-cli` | `0.1.0-rc.1` | manual, token |
| `pdf2-wasm` | `0.1.0-rc.1` | manual, token |

Trusted publishers still need adding on crates.io for `pdf2-cli`,
`pdf2-render` and `pdf2-wasm` (same repository, workflow `publish.yml`,
environment `release`); crates.io only accepts the configuration once the crate
exists, which is why the first version of each went out by hand.

### Order matters

A crate that depends on another cannot be published until that dependency
exists at the required version. The order is `pdf2` → `pdf2-render` →
`pdf2-cli` / `pdf2-wasm`.

### After tagging

Both publish workflows create the GitHub release for their tag in a `GitHub
release` job: the notes are the CHANGELOG section for that version plus the
registry links, npm releases are titled `pdf2-wasm <version> (npm)`, and
`-rc`/`-alpha` versions are marked pre-release. Nothing needs to be typed by
hand; `gh release list` should show a release for every tag. Older tags cannot run the job — a tag executes the workflow file from the
tagged commit — so they are backfilled by dispatching the same workflow with
`release_tag: <tag>`, which records the release from `main` and publishes
nothing. A recreated tag comes back as a draft release; the job publishes it
with `--draft=false`.

### Before tagging

Run `cargo gate --full`: it is the Gate job plus the `no_std` builds, the wasm
modules (with their size budgets) and the benchmark comparison. See `AGENTS.md`
for the release checklist and the tag namespaces.

### The npm package

`crates/pdf2-wasm/package.json` describes the npm package (`pdf2-wasm`, homepage
the Pages demo). Its four files are the wasm-bindgen
`--target web` output in `crates/pdf2-wasm/demo/pkg/` plus the README, and
`crates/pdf2-wasm/check.mjs` smoke-tests them and enforces the size budget.

`.github/workflows/npm.yml` builds, tests and publishes it:

- **First publish, by hand** — npm only accepts a trusted publisher once the
  package exists. `pdf2-wasm` 1.0.0 and `pdf2` 1.0.6 were published this way: an owner
  web-2FA login, where npm stages the version behind a `0.0.0-stage`
  placeholder until the browser flow completes (the placeholder stays in the
  version list without a dist-tag and is harmless). Run it locally with an npm
  login (or `NPM_TOKEN`):

  ```sh
  cargo xtask wasm-package          # builds and checks crates/pdf2-wasm/pkg
  cd crates/pdf2-wasm/pkg && npm publish
  ```

  The published package is that directory: the wasm, its glue, the TypeScript
  declarations, `package.json`, the README and both licences. The demo site has
  its own copy of the two files it loads, which is why the package is not the
  demo's directory.

- **Trusted publishing (OIDC) — configured and proven.** The trusted publisher
  on npmjs.com is `pdf2-rs/pdf2`, workflow `npm.yml`, environment `release`.
  the package was published by tag with no token: npm logged
  `Signed provenance statement with source and build information from GitHub
  Actions` with a transparency-log entry, so `npm audit signatures` and the
  package page verify where the tarball came from. The workflow installs
  `npm@latest` because Node 22 ships npm 10 and OIDC needs 11.5.1 or newer.
- **Or a token** — add an `NPM_TOKEN` repository secret (automation token); the
  workflow passes it as `NODE_AUTH_TOKEN` and publishes with `--provenance`.

Releases run on an `npm-v*` tag whose version matches `package.json` and which
is on `main` (`wasm-v*` stays reserved for the crates.io release of the same
crate, and the two versions may differ); a manual dispatch defaults to
`dry_run`, and a version already on npm is skipped.

The npm version tracks the crate version, but a packaging-only change (manifest
metadata, README) or a renderer fix that only needs a rebuilt wasm is released
as an npm patch without a new crate: `1.0.1` was the canonical `git+…`
repository URL npm had been rewriting, and `1.0.2` carries the renderer fixes
from `pdf2-render` 1.0.1.

## Cutting a release

Tags are **per crate**, because each crate has its own version:

| Tag | Publishes |
| --- | --- |
| `pdf2-v0.2.0-alpha.3` | `pdf2` |
| `render-v0.1.0` | `pdf2-render` (and needs the matching `pdf2` first) |
| `cli-v0.1.0` | `pdf2-cli` |
| `wasm-v0.1.0` | `pdf2-wasm` |

1. Bump that crate's version and add the entry to `CHANGELOG.md`. Every version
   already on crates.io is taken, so the release must use a new number.
2. Commit on `main` and wait for CI to be green.
3. Tag and push:

   ```sh
   git tag pdf2-v0.2.0-alpha.3
   git push origin pdf2-v0.2.0-alpha.3
   ```

4. `Publish` checks that the tag matches the crate's manifest version and that
   the commit is on `main`, runs the workspace tests, exchanges an OIDC token and
   publishes. A crate whose version is already on crates.io is skipped, so a
   re-run is harmless.
5. The GitHub release is created for you: the same workflow records it from the
   tag, with the CHANGELOG section as the notes and `--latest` only for `pdf2`
   (`publish.yml`, job `release`). A tag that predates that job is backfilled by
   dispatching the workflow with `release_tag`.

### Publishing the whole train

Actions → **Publish** → *Run workflow*: `crate: all` walks the crates in
dependency order (`pdf2` → `pdf2-render` → `pdf2-cli` / `pdf2-wasm`), skipping
every version that already exists. `dry_run` defaults to `true`, which runs
`cargo publish --dry-run` for the whole plan and uploads nothing; uncheck it to
publish. A single crate can be selected the same way.

## Dry runs

Actions → **Publish** → *Run workflow*: the `dry_run` input defaults to `true`,
so a manual run executes `cargo publish --dry-run` from the selected branch and
uploads nothing. Only a `v*` tag (or a manual run with `dry_run` unchecked)
publishes for real.

## Notes

- `Cargo.lock` is intentionally not tracked, so the workflow never passes
  `--locked`; the package build resolves the dependency requirements as
  declared.
- `rust-toolchain.toml` pins stable for local work; every command in both
  workflows names its toolchain explicitly (`cargo +stable …`,
  `cargo +1.100 …`), so that file cannot silently change what CI tests.
- The crate is dual-licensed `Unlicense OR MIT`; both licence files ship in the
  package.
- CI (`.github/workflows/ci.yml`) runs the same gate as the project plan:
  `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo doc` with warnings denied, plus a job on the declared
  MSRV (1.100) and on beta, and a `cargo package` build.
