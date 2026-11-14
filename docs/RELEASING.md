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

## Cutting a release

1. Bump the version in `Cargo.toml` and add the entry to `CHANGELOG.md`. Every
   version already on crates.io is taken, so the release must use a new number
   (`0.2.0-alpha.2`, `0.2.0`, …).
2. Commit on `main` and wait for CI to be green.
3. Tag and push the tag:

   ```sh
   git tag v0.2.0
   git push origin v0.2.0
   ```

4. The `Publish` workflow checks that the tag matches `Cargo.toml` and that the
   commit is on `main`, runs the test suite, then uploads the crate.
5. Create the GitHub release from the tag (optional).

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
