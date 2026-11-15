# Corpus

Third-party PDFs used as evidence, not as fixtures. `docs/PROOF.md` explains the
tiers; `docs/CORPUS.md` records what the current library does with each file.

## Tier A — committed

`tier-a/` holds files from the [Mozilla pdf.js test suite](https://github.com/mozilla/pdf.js),
reproduced under the Apache License 2.0 (`LICENSES/Apache-2.0.txt`). They were
taken from commit `d60bc7426aa0968d5b95ff8ca3a76dbc6ce23a3a`, path `test/pdfs`,
and chosen because each carries a construct this repository does not generate:
composite (Type 0/CID) and CFF fonts, `/ToUnicode`, Type 3, JPX, CCITT, CMYK
JPEG, shading and pattern fills, transparency, blend modes, colour-key masks and
encryption.

`manifest.json` records the name, size, SHA-256 and source URL of every file, and
the upstream commit. A file whose hash does not match the manifest is not the file
this evidence was gathered on.

To refresh: download the names in the manifest from the pinned commit, verify the
hashes, and record any new hashes in the same commit as the evidence that
depends on them.

`corpus/` is excluded from the published crate (`exclude` in the root
`Cargo.toml`); it is repository evidence, not shipped code.
