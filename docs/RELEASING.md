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

### The npm package

`crates/pdf2-wasm/package.json` describes the npm package (`pdf2-wasm`). The
files it publishes are the wasm-bindgen `--target web` output in
`crates/pdf2-wasm/demo/pkg/`, which the CI `wasm-package` job and the Demo
workflow build. Publishing requires an npm login or an automation token — an
owner action; nothing in this repository holds npm credentials. Once the
package exists on npm, a trusted publisher can replace the token the same way
crates.io does it.

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
5. Create the GitHub release from the tag (optional).

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
  `cargo +1.87 …`), so that file cannot silently change what CI tests.
- The crate is dual-licensed `Unlicense OR MIT`; both licence files ship in the
  package.
- CI (`.github/workflows/ci.yml`) runs the same gate as the project plan:
  `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test`, `cargo doc` with warnings denied, plus a job on the declared
  MSRV (1.87) and on beta, and a `cargo package` build.
