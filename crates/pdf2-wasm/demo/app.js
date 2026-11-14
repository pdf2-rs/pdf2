// The demo drives the wasm package through its public API only: open a
// document, read its facts, render a page straight onto the canvas opaque.
import init, { open } from './pkg/pdf2_wasm.js';

const SAMPLES = [
  { file: 'samples/text.pdf', title: 'Text and accents' },
  { file: 'samples/paths.pdf', title: 'Paths, fills, dashes' },
  { file: 'samples/images.pdf', title: 'Images and text' },
  { file: 'samples/calibration.pdf', title: 'Calibration, 6 pages' },
  // Deliberately no password: the point of this sample is the password field.
  { file: 'samples/encrypted.pdf', title: 'AES-256 encrypted (password: user)' },
];

// Each entry names a sample (index into SAMPLES) and the page it starts on, so a
// reader can jump straight to the thing that demonstrates a capability.
const FEATURES = [
  { label: 'Colour spaces: gray, RGB, CMYK', sample: 3, page: 0 },
  { label: 'Caps: butt, round, square', sample: 3, page: 1 },
  { label: 'Joins: miter, bevel, miter limit', sample: 3, page: 1 },
  { label: 'Non-zero and even-odd fill rules', sample: 3, page: 1 },
  { label: 'Dash patterns and dash phase', sample: 3, page: 1 },
  { label: 'Bézier curves: a filled circle of known area', sample: 3, page: 1 },
  { label: 'Images: exact scale, /Decode, /Indexed', sample: 3, page: 2 },
  { label: 'Image on a quarter-turn matrix', sample: 3, page: 2 },
  { label: 'Media box with a non-zero origin', sample: 3, page: 3 },
  { label: '/Rotate 90 pages', sample: 3, page: 4 },
  { label: 'Text metrics: horizontal scaling, word spacing', sample: 3, page: 5 },
  { label: '/Widths and /MissingWidth advances', sample: 3, page: 6 },
  { label: 'Embedded TrueType program', sample: 3, page: 8 },
  { label: 'Composite (Type 0) fonts over a CID map', sample: 3, page: 9 },
  { label: 'AES-256 encrypted file (password: user)', sample: 4, page: 0 },
  { label: 'Base-14 fallback glyphs and accents', sample: 0, page: 0 },
  { label: 'Text extraction for the page', sample: 2, page: 0 },
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
  download: document.querySelector('#download'),
  features: document.querySelector('#features'),
  fitPage: document.querySelector('#fitPage'),
  minimapBox: document.querySelector('#minimapBox'),
  minimapInner: document.querySelector('#minimapInner'),
  minimap: document.querySelector('#minimap'),
  minimapViewport: document.querySelector('#minimapViewport'),
};

const state = {
  document: null,
  bytes: null,
  password: null,
  name: '–',
  page: 0,
  sampleIndex: null,
  // The document waiting for a password, if any: retrying must not reopen
  // whatever happened to be loaded before.
  pending: null,
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

const MINIMAP_WIDTH = 108;

function scaleFor(pageWidth, pageHeight) {
  if (typeof state.zoom === 'number') {
    return state.zoom;
  }
  const wrap = elements.canvasWrap;
  const availableWidth = Math.max(wrap.clientWidth - 4, 120);
  const availableHeight = Math.max(wrap.clientHeight - 4, 120);
  const byWidth = availableWidth / pageWidth;
  if (state.zoom === 'page') {
    return Math.min(byWidth, availableHeight / pageHeight);
  }
  return byWidth;
}

/// The thumbnail is only useful when the page does not fit, so it hides itself
/// when everything is visible, and its rectangle tracks the scroll position.
function renderMinimap() {
  const document_ = state.document;
  if (!document_) {
    elements.minimapBox.classList.remove('visible');
    return;
  }
  const [width, height] = document_.pageSize(state.page);
  const scale = MINIMAP_WIDTH / width;
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
  document_.renderPageToCanvas(state.page, scale * dpr, elements.minimap);
  elements.minimap.style.width = `${Math.round(width * scale)}px`;
  elements.minimap.style.height = `${Math.round(height * scale)}px`;
  const canvas = elements.canvas;
  const wrap = elements.canvasWrap;
  const overflows =
    canvas.offsetWidth > wrap.clientWidth + 1 || canvas.offsetHeight > wrap.clientHeight + 1;
  elements.minimapBox.classList.toggle('visible', overflows);
  updateViewport();
}

function updateViewport() {
  if (!elements.minimapBox.classList.contains('visible')) {
    return;
  }
  const canvas = elements.canvas;
  const wrap = elements.canvasWrap;
  const page = canvas.getBoundingClientRect();
  const view = wrap.getBoundingClientRect();
  if (page.width === 0 || page.height === 0) {
    return;
  }
  const left = Math.max(0, view.left - page.left);
  const top = Math.max(0, view.top - page.top);
  const right = Math.min(page.width, view.right - page.left);
  const bottom = Math.min(page.height, view.bottom - page.top);
  const width = elements.minimap.clientWidth;
  const height = elements.minimap.clientHeight;
  const box = elements.minimapViewport;
  box.style.left = `${(left / page.width) * width}px`;
  box.style.top = `${(top / page.height) * height}px`;
  box.style.width = `${(Math.max(0, right - left) / page.width) * width}px`;
  box.style.height = `${(Math.max(0, bottom - top) / page.height) * height}px`;
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
    const base = scaleFor(width, height);
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
    elements.zoomInfo.textContent =
      typeof state.zoom === 'number' ? `${Math.round(base * 100)}%` : state.zoom;
    renderMinimap();
    elements.prev.disabled = state.page === 0;
    elements.next.disabled = state.page >= pages - 1;
    fact(elements.factPages, String(pages));
    fact(elements.factSize, `${Math.round(width)} × ${Math.round(height)} pt`);
    fact(elements.factTime, `${elapsed.toFixed(1)} ms at ${base.toFixed(2)}×`);
    // Text extraction can fail for a page the renderer draws perfectly well —
    // composite fonts without ToUnicode, for one — so it gets its own report
    // instead of taking the render down with it.
    let extraction = '';
    try {
      const text = (state.document.text(state.page) || '').trim();
      elements.text.textContent = text.length > 0 ? text.slice(0, 800) : '(no extractable text)';
    } catch (error) {
      extraction = ' — text extraction unavailable: ';
      elements.text.textContent = `${extraction.trim()}
${error.message || error}`;
    }
    status(
      `${state.name}: PDF ${state.document.version()}, ${pages} page(s), ` +
        `rendered page ${state.page + 1} in ${elapsed.toFixed(1)} ms` +
        (extraction ? extraction.slice(3) : ''),
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
  state.pending = null;
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
    elements.download.disabled = false;
    elements.download.textContent = `Download ${name}`;
    document.title = `pdf2 — ${name}`;
    await render();
  } catch (error) {
    elements.download.disabled = true;
    if (looksEncrypted(error) && !password) {
      state.sampleIndex = source.sampleIndex ?? null;
      state.pending = { bytes, name, source };
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
  const [width, height] = state.document ? state.document.pageSize(state.page) : [612, 792];
  const current = scaleFor(width, height);
  const candidates = ZOOM_STEPS.filter((step) =>
    direction > 0 ? step > current + 0.01 : step < current - 0.01,
  );
  if (candidates.length === 0) {
    return;
  }
  zoomTo(direction > 0 ? candidates[0] : candidates[candidates.length - 1]);
}

async function loadSample(index, page = 0) {
  const sample = SAMPLES[index];
  if (!sample) {
    return;
  }
  const response = await fetch(sample.file);
  const bytes = new Uint8Array(await response.arrayBuffer());
  await showDocument(bytes, sample.file.split('/').pop(), sample.password ?? null, {
    sampleIndex: index,
  });
  if (page > 0) {
    setPage(page);
  }
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

elements.download.addEventListener('click', () => {
  if (!state.bytes) {
    return;
  }
  const url = URL.createObjectURL(new Blob([state.bytes], { type: 'application/pdf' }));
  const link = document.createElement('a');
  link.href = url;
  link.download = state.name === '–' ? 'document.pdf' : state.name;
  link.click();
  URL.revokeObjectURL(url);
});

for (const [index, feature] of FEATURES.entries()) {
  const item = document.createElement('li');
  const button = document.createElement('button');
  button.type = 'button';
  const sample = SAMPLES[feature.sample];
  button.innerHTML = `${feature.label} <span class="where">${sample.file.split('/').pop()}${
    feature.sample === 3 ? ` · page ${feature.page + 1}` : ''
  }</span>`;
  button.addEventListener('click', () => loadSample(feature.sample, feature.page));
  item.append(button);
  elements.features.append(item);
}

elements.prev.addEventListener('click', () => setPage(state.page - 1));
elements.next.addEventListener('click', () => setPage(state.page + 1));
elements.zoomIn.addEventListener('click', () => zoomBy(1));
elements.zoomOut.addEventListener('click', () => zoomBy(-1));
elements.fit.addEventListener('click', () => zoomTo('fit'));
elements.fitPage.addEventListener('click', () => zoomTo('page'));

elements.canvasWrap.addEventListener('scroll', updateViewport, { passive: true });

let draggingMinimap = false;
function gotoMinimap(event) {
  const canvas = elements.canvas;
  const wrap = elements.canvasWrap;
  const inner = elements.minimapInner.getBoundingClientRect();
  if (inner.width === 0 || inner.height === 0) {
    return;
  }
  const fx = Math.min(1, Math.max(0, (event.clientX - inner.left) / inner.width));
  const fy = Math.min(1, Math.max(0, (event.clientY - inner.top) / inner.height));
  const page = canvas.getBoundingClientRect();
  const view = wrap.getBoundingClientRect();
  // Move the point under the cursor to the middle of the viewer.
  wrap.scrollLeft += page.left + fx * page.width - (view.left + wrap.clientWidth / 2);
  wrap.scrollTop += page.top + fy * page.height - (view.top + wrap.clientHeight / 2);
  updateViewport();
}

elements.minimapInner.addEventListener('pointerdown', (event) => {
  draggingMinimap = true;
  elements.minimapInner.setPointerCapture(event.pointerId);
  gotoMinimap(event);
});
elements.minimapInner.addEventListener('pointermove', (event) => {
  if (draggingMinimap) {
    gotoMinimap(event);
  }
});
elements.minimapInner.addEventListener('pointerup', (event) => {
  draggingMinimap = false;
  elements.minimapInner.releasePointerCapture(event.pointerId);
});
elements.minimapInner.addEventListener('pointercancel', () => {
  draggingMinimap = false;
});

elements.password.addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && state.pending) {
    const pending = state.pending;
    showDocument(pending.bytes, pending.name, elements.password.value, pending.source);
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
  } else if (event.key === 'p') {
    zoomTo('page');
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
  if (!state.document) {
    return;
  }
  if (typeof state.zoom === 'number') {
    updateViewport();
    return;
  }
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(render, 120);
}).observe(elements.canvasWrap);

await init();
await loadSample(0);
