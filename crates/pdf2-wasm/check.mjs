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
// What npm publishes, built by `cargo xtask wasm-package`.
const pkg = join(here, 'pkg');
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

// The npm package is what is published, so its contents are checked, not just
// the demo's: the bindgen output at the root, with the manifest, README and
// licences beside it, TypeScript declarations included, and no demo directory.
{
  const { readdirSync } = require('node:fs');
  const manifest = JSON.parse(readFileSync(join(pkg, 'package.json'), 'utf8'));
  expect(manifest.main === './pdf2_wasm.js', `main is ${manifest.main}`);
  expect(manifest.types === './pdf2_wasm.d.ts', `types is ${manifest.types}`);
  expect(manifest.type === 'module', 'the package is ESM');
  const shipped = readdirSync(pkg).filter((name) => name !== 'package.json');
  for (const name of [
    'pdf2_wasm.js',
    'pdf2_wasm.d.ts',
    'pdf2_wasm_bg.wasm',
    'pdf2_wasm_bg.wasm.d.ts',
    'README.md',
    'LICENSE-MIT',
    'LICENSE-UNLICENSE',
  ]) {
    expect(shipped.includes(name), `the package ships ${name}`);
  }
  expect(!shipped.includes('demo'), 'the package has no demo directory');
  for (const entry of manifest.files) {
    expect(!entry.includes('demo/'), `the manifest does not list ${entry}`);
    expect(shipped.includes(entry), `the manifest lists ${entry}, which exists`);
  }
  const listed = new Set(['package.json', ...manifest.files]);
  for (const name of shipped) {
    expect(listed.has(name), `${name} is listed in the manifest files`);
  }
}

// Size budget: the wasm-bindgen output that ships to the browser, gzipped.
// Budgets are documented in docs/PERFORMANCE.md; change both together.
const BUDGET_GZIP = 260 * 1024;
const files = [join(pkg, 'pdf2_wasm_bg.wasm'), join(pkg, 'pdf2_wasm.js')];
let total = 0;
for (const file of files) {
  const raw = readFileSync(file);
  const compressed = gzipSync(raw, { level: 9 }).length;
  total += compressed;
  console.log(`size ${file.split('/').pop()}: ${raw.length} raw, ${compressed} gzip`);
}
console.log(`size total: ${total} gzip (budget ${BUDGET_GZIP})`);
expect(total <= BUDGET_GZIP, `bundle is ${total} gzip bytes, over the ${BUDGET_GZIP} budget`);

// The demo is a site, not part of the package: every sample it names must
// exist, every element it reaches for must be in the page, and the two files it
// loads must have been copied there beside it. A missing file or a renamed id
// would only show up as a blank page in a browser.
{
  const { readdirSync } = require('node:fs');
  const demo = join(here, 'demo');
  const page = readFileSync(join(demo, 'index.html'), 'utf8');
  const script = readFileSync(join(demo, 'app.js'), 'utf8');
  const samples = [...script.matchAll(/file: '(samples\/[^']+)'/g)].map((match) => match[1]);
  expect(samples.length > 0, 'the demo names at least one sample');
  for (const sample of samples) {
    expect(
      readdirSync(join(demo, 'samples')).includes(sample.split('/').pop()),
      `the demo sample ${sample} is committed`,
    );
  }
  // Zooming past the container width used to clamp the canvas with CSS while its
  // height kept the zoomed value, so the page stretched vertically only.
  expect(
    !/canvas\s*\{[^}]*max-width/.test(page),
    'the canvas is not clamped by CSS, so zoom cannot stretch it',
  );
  // The loaded file can be saved and checked in another viewer.
  expect(
    /link\.download/.test(script) && /createObjectURL/.test(script),
    'the demo can download the document it is showing',
  );
  expect(
    (script.match(/const FEATURES = \[/) || []).length === 1,
    'the demo lists what it supports',
  );
  // The sample select is the visible record of what is loaded; clearing it after
  // a successful open made the choice vanish the moment it worked.
  expect(
    !/elements\.sample\.value = ''/.test(script),
    'the demo does not clear the sample selection after opening',
  );
  expect(
    (script.match(/selectSource\(/g) || []).length >= 2,
    'the demo selects the sample the user chose',
  );
  const ids = new Set([...page.matchAll(/id="([^"]+)"/g)].map((match) => match[1]));
  const demoPkg = readdirSync(join(demo, 'pkg'));
  for (const name of ['pdf2_wasm.js', 'pdf2_wasm_bg.wasm']) {
    expect(demoPkg.includes(name), `the demo loads its own copy of ${name}`);
  }
  const wanted = [...script.matchAll(/querySelector\('#([^']+)'\)/g)].map((match) => match[1]);
  for (const id of wanted) {
    expect(ids.has(id), `the demo's #${id} exists in index.html`);
  }
}

console.log('wasm package: all checks passed');
