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

// The text geometry the demo's text layer and find box read: four numbers per
// span in a Float64Array, with the strings parallel to it, so no JavaScript type
// of its own (the wasm bundle pays for every bit of engine glue).
const spans = text.spans(0);
const spanTexts = text.spanTexts(0);
expect(spans instanceof Float64Array, 'spans(0) is a Float64Array');
expect(spans.length > 0 && spans.length === spanTexts.length * 4, `spans: ${spans.length} numbers, ${spanTexts.length} texts`);
expect(
  typeof spanTexts[0] === 'string' && text.text(0).includes(spanTexts[0]),
  `the first span's text is the page's text: ${spanTexts[0]}`,
);
const [sx0, sy0, sx1, sy1] = Array.from(spans.slice(0, 4));
expect(
  sx0 >= 0 && sy0 >= 0 && sx1 > sx0 && sy1 > sy0 && sx1 <= 612 && sy1 <= 792,
  `the box is inside the page: ${sx0}, ${sy0}, ${sx1}, ${sy1}`,
);

const hits = text.search('hello');
const hitTexts = text.searchTexts('hello');
expect(hits instanceof Float64Array && hits.length === 5, `one hit is page + four numbers: ${hits.length}`);
expect(hitTexts.length === 1 && hitTexts[0].toLowerCase() === 'hello', `the hit's text: ${hitTexts[0]}`);
expect(hits[0] === 0, 'the hit is on page 0');
expect(text.search('').length === 0, 'an empty needle matches nothing');
expect(text.search('nothing on this page').length === 0, 'a missing phrase matches nothing');

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
  expect(manifest.name === 'pdf2-wasm', `the package is published as ${manifest.name}`);
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

// Size budgets, one per artefact: the npm package ships the lean build, the demo
// site ships one with the image decoders compiled in (ADR 0007 — they cost about
// 138 KiB gzip, which is why they are not in the default). Budgets are documented
// in docs/PERFORMANCE.md; change both together.
const BUDGET_GZIP = 384 * 1024;
const DEMO_BUDGET_GZIP = 520 * 1024;

/// Measure one artefact: the wasm module plus its JS glue, gzipped.
const measure = (directory, budget, name) => {
  const files = [join(directory, 'pdf2_wasm_bg.wasm'), join(directory, 'pdf2_wasm.js')];
  let total = 0;
  for (const file of files) {
    const raw = readFileSync(file);
    const compressed = gzipSync(raw, { level: 9 }).length;
    total += compressed;
    console.log(`size ${name} ${file.split('/').pop()}: ${raw.length} raw, ${compressed} gzip`);
  }
  console.log(`size ${name} total: ${total} gzip (budget ${budget})`);
  expect(total <= budget, `the ${name} bundle is ${total} gzip bytes, over the ${budget} budget`);
  return total;
};

const npmSize = measure(pkg, BUDGET_GZIP, 'npm');
const demoSize = measure(join(here, 'demo', 'pkg'), DEMO_BUDGET_GZIP, 'demo');
// The site must not be smaller than the package by accident: that would mean the
// feature build silently fell back to the lean one, and the demo's decoder
// samples would paint nothing.
expect(
  demoSize > npmSize + 100 * 1024,
  `the demo build carries the decoders (${demoSize} against the npm ${npmSize})`,
);

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
  // The samples are copies of the crate's fixtures: a committed copy went stale
  // once the fixture grew pages, and the feature list began to point past the
  // end of the file. Compare the bytes instead of trusting the copy.
  const sources = {
    'samples/text.pdf': 'text.pdf',
    'samples/paths.pdf': 'paths.pdf',
    'samples/images.pdf': 'images.pdf',
    'samples/book.pdf': 'book.pdf',
    'samples/calibration.pdf': 'calibration.pdf',
    'samples/encrypted.pdf': 'encrypted-r6.pdf',
  };
  for (const [sample, source] of Object.entries(sources)) {
    const copied = readFileSync(join(demo, sample));
    const fixture = readFileSync(join(fixtures, source));
    expect(
      copied.equals(fixture),
      `${sample} is the current tests/fixtures/${source}`,
    );
  }
  // And every page the feature list advertises has to exist in its sample.
  const featurePages = [...script.matchAll(/sample: (\d+), page: (\d+)/g)].map(
    (match) => ({ sample: Number(match[1]), page: Number(match[2]) }),
  );
  expect(featurePages.length > 10, 'the feature list is populated');
  const files = [...script.matchAll(/file: '(samples\/[^']+)'/g)].map((match) => match[1]);
  for (const feature of featurePages) {
    const bytes = readFileSync(join(demo, files[feature.sample]));
    let pages;
    try {
      pages = wasm.open(bytes, undefined).pageCount();
    } catch (error) {
      // The encrypted sample needs a password; the feature list only ever
      // points at its first page.
      expect(
        /password/.test(String(error.message || error)) && feature.page === 0,
        `${files[feature.sample]}: ${error.message || error}`,
      );
      continue;
    }
    expect(
      feature.page < pages,
      `${files[feature.sample]} has page ${feature.page + 1} of ${pages}`,
    );
  }

  // A sample title with a page count in it is a fact that goes stale the next
  // time the fixture grows: the panel reports the real number instead.
  const titles = [...script.matchAll(/title: '([^']+)'/g)].map((match) => match[1]);
  for (const title of titles) {
    expect(!/\d+\s+pages?/.test(title), `the sample title "${title}" carries no page count`);
  }

  // The build stamp the page imports the module with: without it a cached wasm
  // from an older build silently draws blank pages.
  const stamp = JSON.parse(readFileSync(join(demo, 'build.json'), 'utf8'));
  const manifest = JSON.parse(readFileSync(join(here, 'package.json'), 'utf8'));
  expect(stamp.version === manifest.version, `build.json names version ${manifest.version}`);
  expect(/\/^[0-9]+$/.test('') || /^[0-9]+$/.test(stamp.build), 'build.json carries a build stamp');

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
