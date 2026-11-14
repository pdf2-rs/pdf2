# pdf2-cli

Command-line front end for the [`pdf2`](https://crates.io/crates/pdf2) crate:
inspect PDF 2.0 files from a shell. The binary is called `pdf2` and has no
dependencies of its own.

```sh
cargo install pdf2-cli
pdf2 info document.pdf
```

## Commands

| Command | Prints |
| --- | --- |
| `pdf2 info <file>` | document version, page count, encryption and metadata state |
| `pdf2 pages <file>` | every page: object id, media box, crop box, rotation, content stream ids |
| `pdf2 text <file> [--page <n> \| --all]` | extracted text, page 1 unless told otherwise |
| `pdf2 check <file>` | opens, decodes and extracts every page and reports what failed |

`pdf2 --help` prints the full usage, `pdf2 --version` the CLI version.

## Options

- `--password <pw>` (`-p`) — password for an AES-256 encrypted document.
  Wrong or missing passwords are reported, never guessed.
- `--json` — machine-readable output for every command.
- `--page <n>` (`-n`) — page for `text`, counting from 1.
- `--all` (`-a`) — every page for `text`.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | success |
| 1 | the command line is wrong (unknown command, unknown option, no file) |
| 2 | the file could not be read, decrypted, parsed or interpreted |

Error messages go to stderr and start with `error: `; the command never panics
on a malformed file.

## JSON shapes

```json
{"file":"a.pdf","version":"2.0","pages":3,"encrypted":false,"metadata":true}
{"file":"a.pdf","pages":[{"index":1,"object":"5 0","mediaBox":[0,0,200,300],"cropBox":[0,0,200,300],"rotate":90,"contents":["4 0"]}]}
{"file":"a.pdf","pages":[{"index":1,"text":"Hello"}]}
{"file":"a.pdf","pages":3,"ok":true,"failures":[]}
```

A page dictionary written directly in its parent's `/Kids` array has no object
id, so `object` is `null` in JSON and `-` in the human output.

## Examples

```sh
pdf2 info report.pdf
pdf2 pages report.pdf | head -20
pdf2 text report.pdf --all > report.txt
pdf2 check report.pdf || echo "the file has unreadable pages"
pdf2 info secret.pdf --password user --json
```

## Licence

Dual-licensed under `MIT` or the [UNLICENSE](http://unlicense.org/), like the
`pdf2` crate.
