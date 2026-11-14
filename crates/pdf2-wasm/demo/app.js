// The demo drives the wasm package through its public API: open a document,
// read its facts, render the first page straight onto the canvas.
import init, { open } from './pkg/pdf2_wasm.js';

const status = document.querySelector('#status');
const canvas = document.querySelector('#canvas');

async function show(bytes, label) {
  try {
    const doc = open(bytes, null);
    const pages = doc.pageCount();
    const [width, height] = doc.pageSize(0);
    const text = doc.text(0);
    doc.renderPageToCanvas(0, 1.5, canvas);
    const summary = text.replaceAll('\n', ' ').slice(0, 60);
    status.textContent =
      `${label}: PDF ${doc.version()}, ${pages} page(s), ` +
      `${Math.round(width)}×${Math.round(height)} pt — text: “${summary}”`;
    globalThis.document.title = `pdf2 — ${label}`;
  } catch (error) {
    status.textContent = `${label}: ${error}`;
  }
}

await init();
const sample = await fetch('sample.pdf').then((response) => response.arrayBuffer());
await show(new Uint8Array(sample), 'sample.pdf');

document.querySelector('#file').addEventListener('change', async (event) => {
  const file = event.target.files[0];
  if (!file) {
    return;
  }
  await show(new Uint8Array(await file.arrayBuffer()), file.name);
});
