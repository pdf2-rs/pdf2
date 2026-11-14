## What and why

<!-- What changes, and what would have broken without it. A bug fix names the failing case. -->

## How it was checked

<!-- `cargo gate`, plus the artefact you looked at: a PNG, a CLI output, a registry page. -->

## Checklist

- [ ] `cargo gate` is green locally (`cargo gate --full` for `no_std`, wasm or benchmark changes)
- [ ] `CHANGELOG.md` has an entry when behaviour, the API or a published artefact changes
- [ ] docs updated when the change is user-visible (`README.md`, `docs/RENDERING.md`, `docs/SPEC-MAP.md`, `docs/RELEASING.md`)
- [ ] fixture or golden changes are explained above, not just re-blessed
- [ ] public API changes state their semver impact (`pdf2` is 1.x)
- [ ] no version bump mixed into a feature change (releases use their own PR)

## Publishing

<!-- Does this need a crates.io tag, an npm tag, both, or nothing? -->
