# pdf2-wasm

WebAssembly bindings for the [`pdf2`](https://crates.io/crates/pdf2) crate and
its renderer: open a PDF 2.0 file in the browser, read its facts and paint a page.

```js
import init, { open } from "pdf2-wasm";

await init();
const response = await fetch("samples/text.pdf");
const doc = open(new Uint8Array(await response.arrayBuffer()), null);
console.log(doc.version(), doc.pageCount(), doc.pageSize(0));
console.log(doc.text(0));
doc.renderPageToCanvas(0, 1.5, document.querySelector("canvas"));
```

## API

| Member | Description |
| --- | --- |
| `open(bytes, password?)` | Open a document; throws on a malformed file or a wrong password |
| `new PdfDocument(bytes, password?)` | The same as a constructor |
| `version()` | Header version, e.g. `"2.0"` |
| `isEncrypted()` | Whether the file was opened with a security handler |
| `pageCount()` | Number of pages |
| `pageSize(index)` | `Float64Array` of `[width, height]` in PDF points |
| `text(index)` | Extracted text of a page |
| `metadata()` | XMP metadata packet as text, or `undefined` |
| `renderPage(index, scale)` | `Uint8ClampedArray` of RGBA8 pixels (`scale` is device pixels per point) |
| `renderPageToCanvas(index, scale, canvas)` | Render straight onto an `HtmlCanvasElement` |

Rendering covers what `pdf2-render` covers: paths, fills, strokes, dashes, clips,
transforms, text (embedded TrueType or the built-in base-14 fallback) and image
XObjects. No anti-aliasing, no colour management, no `SMask` alpha.

## Building

The bindgen output must match the `wasm-bindgen` version the crate is compiled
against (pinned in `Cargo.toml`):

```sh
cargo install wasm-bindgen-cli --version 0.2.127 --locked
cargo build -p pdf2-wasm --release --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir crates/pdf2-wasm/pkg-node \
    target/wasm32-unknown-unknown/release/pdf2_wasm.wasm
wasm-bindgen --target web --no-typescript --out-dir crates/pdf2-wasm/demo/pkg \
    target/wasm32-unknown-unknown/release/pdf2_wasm.wasm
node --no-warnings crates/pdf2-wasm/check.mjs   # API smoke test + size budget
```

`check.mjs` covers `open`, password handling, page count and size, text,
metadata, `renderPage` and the gzip size budget (260 KiB for the wasm module
plus its JS glue; the current build is about 199 KiB gzipped).

## Demo

`demo/` is a static page: it renders a bundled sample on load, offers the crate's
own fixtures as samples (text, paths, images, the six-page calibration file and
an AES-256 encrypted file whose password is `user`), and lets you drop any PDF
in — nothing is uploaded. Pages, zoom (fit or 50–300%), keyboard navigation and
the extracted text of the current page are part of the demo rather than the
library. Build `demo/pkg/` as above
and serve the directory:

```sh
python3 -m http.server -d crates/pdf2-wasm/demo 8080
```

The `Demo` GitHub workflow builds and deploys it to GitHub Pages on every push
that touches the package.

## Licence

Dual-licensed under `MIT` or the [UNLICENSE](http://unlicense.org/), like `pdf2`.
