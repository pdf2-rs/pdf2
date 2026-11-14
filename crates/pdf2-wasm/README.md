# pdf2

PDF 2.0 in the browser: open, inspect and render PDF files with the [`pdf2`]
crate compiled to WebAssembly. No dependencies, no server, and the bytes never
leave the page.

```sh
npm install pdf2
```

```js
import init, { open } from 'pdf2';

await init(); // fetches and instantiates pdf2_wasm_bg.wasm
const response = await fetch('file.pdf');
const doc = open(new Uint8Array(await response.arrayBuffer()));

doc.version();      // "2.0"
doc.pageCount();    // 1
doc.pageSize(0);    // [612, 792]
doc.text(0);        // extracted text
doc.renderPageToCanvas(0, 1.5, canvasElement);
```

TypeScript declarations ship with the package. The wasm module is fetched
relative to the glue by default; pass `{ module_or_path }` to `init()` to load
it yourself, for example from a bundler asset or a `Uint8Array`.

## API

| Export | What it does |
| --- | --- |
| `init(options?)` | instantiate the module; call once before using anything else |
| `open(bytes, password?)` | parse a document; throws on a malformed file or a wrong password |
| `doc.version()` | header version, e.g. `"2.0"` |
| `doc.isEncrypted()` | whether the file was encrypted (AES-256 R6 is supported) |
| `doc.pageCount()` | number of pages |
| `doc.pageSize(index)` | `[width, height]` in points, after `/Rotate` |
| `doc.text(index)` | text on a page; throws `unsupported: composite (Type0) fonts` for fonts this version cannot decode |
| `doc.metadata()` | the XMP packet as text, when the document has one |
| `doc.renderPage(index, scale)` | RGBA8 `Uint8ClampedArray`; `scale` is device pixels per point |
| `doc.renderPageToCanvas(index, scale, canvas)` | the same, drawn straight onto a canvas |

Rendering covers paths and curves, fills (non-zero and even-odd), strokes with
caps, joins, the miter limit and dash phases, clips, transforms, `/MediaBox`
origins, `/Rotate`, text through simple-font encodings with `/Widths` (embedded
TrueType via `ttf-parser`, a built-in base-14 fallback otherwise), and
`/Image` XObjects (DeviceGray/RGB/CMYK, `/Indexed`, `/ICCBased`, `/Decode`).
Type 0/Type 3 fonts and embedded Type 1/CFF programs are reported rather than
faked. [`docs/RENDERING.md`][rendering] has the clause-by-clause matrix.

## Packages

- npm: `pdf2` — this package: `pdf2_wasm.js`, `pdf2_wasm.d.ts`,
  `pdf2_wasm_bg.wasm`, README and the two licences, nothing else.
- crates.io: [`pdf2-wasm`][crate] — the Rust crate it is built from.
- npm, former name: `pdf2-wasm` is deprecated and points here.
- Demo: <https://pdf2-rs.github.io/pdf2/> renders the crate's own fixtures, with
  page navigation, zoom and a password field for the encrypted sample.

## Building from source

The package is built by `cargo xtask wasm-package` from the workspace root: it
runs `cargo build` for `wasm32-unknown-unknown`, `wasm-bindgen --target web`
into `pkg/`, copies the manifest, README and licences beside the output, and
then checks the result (API smoke test, package contents, size budget). The
`wasm-bindgen` CLI must match the `wasm-bindgen` dependency exactly — the
bindgen schema is not compatible across versions.

## Licence

Unlicense OR MIT. The full texts ship with the package as `LICENSE-UNLICENSE`
and `LICENSE-MIT`.

[`pdf2`]: https://crates.io/crates/pdf2
[crate]: https://crates.io/crates/pdf2-wasm
[rendering]: https://github.com/pdf2-rs/pdf2/blob/main/docs/RENDERING.md
