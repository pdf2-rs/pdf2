// Smoke tests for the two wasm targets, run with plain Node (no bundler, no
// wasm-bindgen):
//
//   cargo build -p pdf2-wasm-smoke --release --target wasm32-unknown-unknown --lib
//   cargo build -p pdf2-wasm-smoke --release --target wasm32-wasip1 --bin pdf2-wasi-smoke
//   node crates/pdf2-wasm-smoke/smoke.mjs
//
// The first half drives the browser-target module through its C ABI; the second
// runs the WASI binary and checks what it prints.

import { closeSync, openSync, readFileSync, unlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { gzipSync } from 'node:zlib';
import { WASI } from 'node:wasi';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// Budgets are documented in docs/PERFORMANCE.md; change both together.
const FILES = {
  browser: 'target/wasm32-unknown-unknown/release/pdf2_wasm_smoke.wasm',
  wasi: 'target/wasm32-wasip1/release/pdf2-wasi-smoke.wasm',
};
// Raised on 2026-11-18 with the painted glyph boxes (`GlyphBox`): the trace
// machinery costs +1 015 gzip on the browser module and +1 097 on the WASI
// binary, and both were within a kilobyte of the budget it replaces.
const BUDGET_GZIP = {
  browser: 170 * 1024,
  wasi: 193 * 1024,
};

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, '..', '..');
const fixture = join(repo, 'tests', 'fixtures', 'text.pdf');
const expected = 'Hello, PDF 2.0';

function checkSizes() {
  for (const [name, file] of Object.entries(FILES)) {
    const raw = readFileSync(join(repo, file));
    const compressed = gzipSync(raw, { level: 9 }).length;
    const budget = BUDGET_GZIP[name];
    console.log(
      `size ${name}: ${raw.length} bytes raw, ${compressed} gzip (budget ${budget})`,
    );
    if (compressed > budget) {
      throw new Error(
        `${name} wasm grew to ${compressed} gzip bytes, over the ${budget} budget`,
      );
    }
  }
}

function checkBrowserTarget() {
  const wasm = readFileSync(join(repo, FILES.browser));
  const bytes = readFileSync(fixture);
  const instance = new WebAssembly.Instance(new WebAssembly.Module(wasm));
  const exports = instance.exports;

  const pointer = exports.pdf2_input(bytes.length);
  new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);

  const pages = exports.pdf2_pages(bytes.length);
  if (pages !== 1) {
    throw new Error(`browser target: expected 1 page, got ${pages}`);
  }

  const length = exports.pdf2_text(bytes.length);
  const text = new TextDecoder().decode(
    new Uint8Array(exports.memory.buffer, exports.pdf2_output_ptr(), length),
  );
  if (!text.includes(expected)) {
    throw new Error(`browser target: text ${JSON.stringify(text)}`);
  }
  console.log(`browser target: ${pages} page(s), ${JSON.stringify(text.trim())}`);
}

async function checkWasiTarget() {
  const wasm = readFileSync(join(repo, FILES.wasi));
  // Node's WASI takes file descriptors, not callbacks: capture stdout in a
  // temporary file and read it back afterwards.
  const capture = join(tmpdir(), `pdf2-wasi-smoke-${process.pid}.txt`);
  const fd = openSync(capture, 'w');
  try {
    const wasi = new WASI({
      version: 'preview1',
      args: ['pdf2-wasi-smoke', 'tests/fixtures/text.pdf'],
      env: {},
      preopens: { '.': repo },
      stdout: fd,
      stderr: fd,
    });
    const module = await WebAssembly.compile(wasm);
    const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
    wasi.start(instance);
  } finally {
    closeSync(fd);
  }
  const printed = readFileSync(capture, 'utf8');
  unlinkSync(capture);

  if (!printed.includes('wasi: 1 page(s)')) {
    throw new Error(`wasi target: ${JSON.stringify(printed)}`);
  }
  if (!printed.includes(expected)) {
    throw new Error(`wasi target: ${JSON.stringify(printed)}`);
  }
  console.log(`wasi target: ${printed.trim().replaceAll('\n', ' | ')}`);
}

checkSizes();
checkBrowserTarget();
await checkWasiTarget();
