// Node side of the wasm package: API smoke test plus the size budget.
//
//   cargo build -p pdf2-wasm --release --target wasm32-unknown-unknown
//   wasm-bindgen --target nodejs --out-dir crates/pdf2-wasm/pkg-node target/.../pdf2_wasm.wasm
//   wasm-bindgen --target web --out-dir crates/pdf2-wasm/demo/pkg target/.../pdf2_wasm.wasm
//   node crates/pdf2-wasm/check.mjs

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fixtures = join(repo, 'tests', 'fixtures');
const require = createRequire(import.meta.url);
// The crate's package.json says `"type": "module"` for the npm build; the
// wasm-bindgen nodejs glue is CommonJS, so give its directory its own scope.
mkdirSync(join(here, 'pkg-node'), { recursive: true });
writeFileSync(
  join(here, 'pkg-node', 'package.json'),
  JSON.stringify({ type: 'commonjs' }),
);
const wasm = require(join(here, 'pkg-node', 'pdf2_wasm.js'));

function expect(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

const text = wasm.open(readFileSync(join(fixtures, 'text.pdf')), undefined);
expect(text.pageCount() === 1, 'text.pdf: page count');
expect(Math.round(text.pageSize(0)[0]) === 612, 'text.pdf: page width');
expect(Math.round(text.pageSize(0)[1]) === 792, 'text.pdf: page height');
expect(text.text(0).includes('Hello, PDF 2.0'), 'text.pdf: extracted text');
expect(text.isEncrypted() === false, 'text.pdf: not encrypted');
expect(text.version() === '2.0', 'text.pdf: version');

const pixels = text.renderPage(0, 1.0);
expect(pixels instanceof Uint8ClampedArray, 'renderPage returns a Uint8ClampedArray');
expect(pixels.length === 612 * 792 * 4, `renderPage length: ${pixels.length}`);
expect(pixels[0] === 255, 'renderPage starts on the white background');

const tree = wasm.open(readFileSync(join(fixtures, 'page-tree.pdf')), undefined);
expect(tree.pageCount() === 3, 'page-tree.pdf: page count');
const metadata = tree.metadata();
expect(typeof metadata === 'string' && metadata.includes('xmpmeta'), 'page-tree.pdf: metadata');

const encrypted = wasm.open(readFileSync(join(fixtures, 'encrypted-r6.pdf')), 'user');
expect(encrypted.isEncrypted() === true, 'encrypted.pdf: flag');
expect(encrypted.text(0).includes('Encrypted hello'), 'encrypted.pdf: text');

let rejected = false;
try {
  wasm.open(readFileSync(join(fixtures, 'encrypted-r6.pdf')), 'wrong');
} catch {
  rejected = true;
}
expect(rejected, 'a wrong password throws');

// Size budget: the wasm-bindgen output that ships to the browser, gzipped.
// Budgets are documented in docs/PERFORMANCE.md; change both together.
const BUDGET_GZIP = 260 * 1024;
const files = [
  join(here, 'demo', 'pkg', 'pdf2_wasm_bg.wasm'),
  join(here, 'demo', 'pkg', 'pdf2_wasm.js'),
];
let total = 0;
for (const file of files) {
  const raw = readFileSync(file);
  const compressed = gzipSync(raw, { level: 9 }).length;
  total += compressed;
  console.log(`size ${file.split('/').pop()}: ${raw.length} raw, ${compressed} gzip`);
}
console.log(`size total: ${total} gzip (budget ${BUDGET_GZIP})`);
expect(total <= BUDGET_GZIP, `bundle is ${total} gzip bytes, over the ${BUDGET_GZIP} budget`);

console.log('wasm package: all checks passed');
