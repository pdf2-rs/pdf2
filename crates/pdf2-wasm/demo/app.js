// The demo drives the wasm package through its public API only: open a
// document, read its facts, render a page straight onto the canvas opaque.
import init, { open } from './pkg/pdf2_wasm.js';

const SAMPLES = [
  { file: 'samples/text.pdf', title: 'Text and accents' },
  { file: 'samples/paths.pdf', title: 'Paths, fills, dashes' },
  { file: 'samples/images.pdf', title: 'Images and text' },
  { file: 'samples/calibration.pdf', title: 'Calibration, 6 pages' },
  { file: 'samples/encrypted.pdf', title: 'AES-256 encrypted (password: user)', password: 'user' },
];

const ZOOM_STEPS = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];

const elements = {
  canvas: document.querySelector('#canvas'),
  canvasWrap: document.querySelector('#canvasWrap'),
  viewer: document.querySelector('#viewer'),
  status: document.querySelector('#status'),
  file: document.querySelector('#file'),
  sample: document.querySelector('#sample'),
  passwordRow: document.querySelector('#passwordRow'),
  password: document.querySelector('#password'),
  prev: document.querySelector('#prev'),
  next: document.querySelector('#next'),
  pageInfo: document.querySelector('#pageInfo'),
  zoomIn: document.querySelector('#zoomIn'),
  zoomOut: document.querySelector('#zoomOut'),
  zoomInfo: document.querySelector('#zoomInfo'),
  fit: document.querySelector('#fit'),
  factName: document.querySelector('#factName'),
  factVersion: document.querySelector('#factVersion'),
  factPages: document.querySelector('#factPages'),
  factSize: document.querySelector('#factSize'),
  factEncrypted: document.querySelector('#factEncrypted'),
  factTime: document.querySelector('#factTime'),
  text: document.querySelector('#text'),
};

const state = {
  document: null,
  bytes: null,
  password: null,
  name: '–',
  page: 0,
  sampleIndex: null,
  zoom: 'fit',
  renderToken: 0,
};

function status(message, isError = false) {
  elements.status.textContent = message;
  elements.status.classList.toggle('error', isError);
}

function clampPage(index) {
  const pages = state.document ? state.document.pageCount() : 0;
  return Math.max(0, Math.min(index, pages - 1));
}

function scaleFor(pageWidth) {
  if (state.zoom !== 'fit') {
    return state.zoom;
  }
  const available = Math.max(elements.canvasWrap.clientWidth - 4, 120);
  return available / pageWidth;
}

function fact(element, value) {
  element.textContent = value;
}

async function render() {
  if (!state.document) {
    return;
  }
  const token = ++state.renderToken;
  try {
    const [width, height] = state.document.pageSize(state.page);
    const base = scaleFor(width);
    const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
    const started = performance.now();
    state.document.renderPageToCanvas(state.page, base * dpr, elements.canvas);
    const elapsed = performance.now() - started;
    if (token !== state.renderToken) {
      return;
    }
    elements.canvas.style.width = `${Math.round(width * base)}px`;
    elements.canvas.style.height = `${Math.round(height * base)}px`;
    const pages = state.document.pageCount();
    elements.pageInfo.textContent = `${state.page + 1} / ${pages}`;
    elements.zoomInfo.textContent = state.zoom === 'fit' ? 'fit' : `${Math.round(base * 100)}%`;
    elements.prev.disabled = state.page === 0;
    elements.next.disabled = state.page >= pages - 1;
    fact(elements.factPages, String(pages));
    fact(elements.factSize, `${Math.round(width)} × ${Math.round(height)} pt`);
    fact(elements.factTime, `${elapsed.toFixed(1)} ms at ${base.toFixed(2)}×`);
    const text = (state.document.text(state.page) || '').trim();
    elements.text.textContent = text.length > 0 ? text.slice(0, 800) : '(no extractable text)';
    status(
      `${state.name}: PDF ${state.document.version()}, ${pages} page(s), ` +
        `rendered page ${state.page + 1} in ${elapsed.toFixed(1)} ms`,
    );
  } catch (error) {
    status(`${state.name}: ${error.message || error}`, true);
  }
}

function looksEncrypted(error) {
  return /password|encrypt|decrypt|security/i.test(String(error.message || error));
}

// The select is the visible record of what is loaded: the chosen sample keeps
// its own option, and a dropped file gets an option of its own so the control
// never shows something that is not on screen.
function selectSource(sampleIndex, label) {
  const custom = elements.sample.querySelector('option[data-custom]');
  if (custom) {
    custom.remove();
  }
  if (sampleIndex !== null) {
    elements.sample.value = String(sampleIndex);
    return;
  }
  const option = document.createElement('option');
  option.value = 'custom';
  option.dataset.custom = 'true';
  option.textContent = label;
  elements.sample.prepend(option);
  elements.sample.value = 'custom';
}

async function showDocument(bytes, name, password, source) {
  try {
    state.document = open(bytes, password ?? null);
    state.bytes = bytes;
    state.name = name;
    state.password = password ?? null;
    state.page = 0;
    state.sampleIndex = source.sampleIndex ?? null;
    elements.passwordRow.classList.remove('visible');
    selectSource(source.sampleIndex ?? null, name);
    fact(elements.factName, name);
    fact(elements.factVersion, state.document.version());
    fact(elements.factEncrypted, state.document.isEncrypted() ? 'yes' : 'no');
    document.title = `pdf2 — ${name}`;
    await render();
  } catch (error) {
    if (looksEncrypted(error) && !password) {
      state.sampleIndex = source.sampleIndex ?? null;
      selectSource(source.sampleIndex ?? null, name);
      elements.passwordRow.classList.add('visible');
      elements.password.focus();
      status(`${name}: encrypted — enter the password to open it`, true);
      return;
    }
    state.document = null;
    fact(elements.factName, name);
    status(`${name}: ${error.message || error}`, true);
  }
}

function setPage(index) {
  const page = clampPage(index);
  if (page === state.page) {
    return;
  }
  state.page = page;
  render();
}

function zoomTo(scale) {
  state.zoom = scale;
  render();
}

function zoomBy(direction) {
  const [width] = state.document ? state.document.pageSize(state.page) : [612];
  const current = scaleFor(width);
  const candidates = ZOOM_STEPS.filter((step) =>
    direction > 0 ? step > current + 0.01 : step < current - 0.01,
  );
  if (candidates.length === 0) {
    return;
  }
  zoomTo(direction > 0 ? candidates[0] : candidates[candidates.length - 1]);
}

async function loadSample(index) {
  const sample = SAMPLES[index];
  if (!sample) {
    return;
  }
  const response = await fetch(sample.file);
  const bytes = new Uint8Array(await response.arrayBuffer());
  await showDocument(bytes, sample.file.split('/').pop(), sample.password ?? null, {
    sampleIndex: index,
  });
}

function fileChosen(file) {
  if (!file) {
    return;
  }
  const reader = new FileReader();
  reader.onload = () => showDocument(new Uint8Array(reader.result), file.name, null, {});
  reader.readAsArrayBuffer(file);
}

for (const [index, sample] of SAMPLES.entries()) {
  const option = document.createElement('option');
  option.value = String(index);
  option.textContent = sample.title;
  elements.sample.append(option);
}
const placeholder = document.createElement('option');
placeholder.value = '';
placeholder.textContent = 'Bundled samples…';
elements.sample.prepend(placeholder);

elements.sample.addEventListener('change', (event) => {
  const value = event.target.value;
  if (value !== '' && value !== 'custom') {
    loadSample(Number(value));
  }
});

elements.file.addEventListener('change', (event) => fileChosen(event.target.files[0]));

elements.prev.addEventListener('click', () => setPage(state.page - 1));
elements.next.addEventListener('click', () => setPage(state.page + 1));
elements.zoomIn.addEventListener('click', () => zoomBy(1));
elements.zoomOut.addEventListener('click', () => zoomBy(-1));
elements.fit.addEventListener('click', () => zoomTo('fit'));

elements.password.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && state.bytes) {
    showDocument(state.bytes, state.name, elements.password.value, {
      sampleIndex: state.sampleIndex,
    });
  }
});

window.addEventListener('keydown', (event) => {
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement) {
    return;
  }
  if (event.key === 'ArrowLeft') {
    setPage(state.page - 1);
  } else if (event.key === 'ArrowRight') {
    setPage(state.page + 1);
  } else if (event.key === '+' || event.key === '=') {
    zoomBy(1);
  } else if (event.key === '-') {
    zoomBy(-1);
  } else if (event.key === 'f') {
    zoomTo('fit');
  }
});

let dragDepth = 0;
window.addEventListener('dragenter', (event) => {
  event.preventDefault();
  dragDepth += 1;
  elements.viewer.classList.add('dragging');
});
window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('dragleave', () => {
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) {
    elements.viewer.classList.remove('dragging');
  }
});
window.addEventListener('drop', (event) => {
  event.preventDefault();
  dragDepth = 0;
  elements.viewer.classList.remove('dragging');
  fileChosen(event.dataTransfer?.files?.[0]);
});

let resizeTimer = null;
new ResizeObserver(() => {
  if (state.zoom !== 'fit' || !state.document) {
    return;
  }
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(render, 120);
}).observe(elements.canvasWrap);

await init();
await loadSample(0);
