// The demo drives the wasm package through its public API only: open a
// document, read its facts, render a page straight onto the canvas opaque.
//
// The module is imported with a build stamp from `build.json`. GitHub Pages
// serves a fixed filename with a cache lifetime, so without it a browser can
// pair this page with a wasm build from before composite fonts existed and show
// a blank page where the current renderer draws text.
const stamp = await fetch('build.json', { cache: 'no-store' }).then((response) => response.json());
const module = await import(`./pkg/pdf2_wasm.js?v=${encodeURIComponent(stamp.build)}`);
// The page says which build it is, from the same file the import stamp came
// from: a version alone cannot tell two builds of one release apart.
document.querySelector('#appVersion').textContent =
  `pdf2-wasm ${stamp.version} (${stamp.rev ?? 'unknown'})`;
const init = module.default;
const open = module.open;

const SAMPLES = [
  { file: 'samples/text.pdf', title: 'Text and accents' },
  { file: 'samples/paths.pdf', title: 'Paths, fills, dashes' },
  { file: 'samples/images.pdf', title: 'Images and text' },
  // No page count in the title: it is a fact that goes stale when the fixture
  // grows, and the panel already reports the real number.
  { file: 'samples/calibration.pdf', title: 'Calibration' },
  // Deliberately no password: the point of this sample is the password field.
  { file: 'samples/book.pdf', title: 'A page of a book, in an embedded serif' },
  { file: 'samples/encrypted.pdf', title: 'AES-256 encrypted (password: user)' },
  // 1.2's own samples, in the order a reader would try them: a modern font
  // program, an older encryption generation, a page whose visible box is not its
  // media box, and a page whose text only exists through a CMap.
  { file: 'samples/cff.pdf', title: 'CFF outlines, CID-keyed (no TrueType)' },
  { file: 'samples/encrypted-legacy.pdf', title: 'AES-128 encrypted, revision 4 (password: user)' },
  { file: 'samples/cropbox.pdf', title: '/CropBox and /UserUnit page' },
  { file: 'samples/cjk.pdf', title: 'CJK text from Shift-JIS (text panel)' },
  // 1.3's own samples: the painting family the release is about.
  { file: 'samples/shading.pdf', title: 'Gradients: axial, radial, function-based' },
  { file: 'samples/blend-modes.pdf', title: 'The sixteen blend modes' },
  { file: 'samples/type1.pdf', title: 'A Type 1 font program, drawn' },
  // 1.4's own samples: the three painting features, then the page that shows what
  // happens when a clip has no outline — reported, not silently blank.
  { file: 'samples/colorkeymask.pdf', title: 'A tiling pattern whose key hides its samples' },
  { file: 'samples/colour-key.pdf', title: 'An image /Mask that hides its own samples' },
  { file: 'samples/tr-clip.pdf', title: 'Clipped text: Tr modes 4 to 7' },
  { file: 'samples/tr-clip-type3.pdf', title: 'A Type 3 glyph under a text clip (reported)' },
  // 1.5's own samples: the two image codecs, which the site's build carries and
  // the npm package deliberately does not (ADR 0007).
  { file: 'samples/jpx.pdf', title: 'JPEG 2000: a /JPXDecode image' },
  { file: 'samples/jbig2-scan.pdf', title: 'JBIG2: a bi-level scan' },
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
  { label: 'A page of prose in an embedded serif font', sample: 4, page: 0 },
  { label: 'AES-256 encrypted file (password: user)', sample: 5, page: 0 },
  { label: 'Base-14 fallback glyphs and accents', sample: 0, page: 0 },
  { label: 'Text extraction for the page', sample: 2, page: 0 },
  // 1.2
  { label: 'CFF outlines: a CID-keyed font program', sample: 6, page: 0 },
  { label: 'Encrypted file, revision 4 (password: user)', sample: 7, page: 0 },
  { label: '/CropBox and /UserUnit decide the page', sample: 8, page: 0 },
  { label: 'CJK text through a predefined CMap', sample: 9, page: 0 },
  // 1.3
  { label: 'Shadings through /Function objects', sample: 10, page: 0 },
  { label: 'A radial gradient with its own colour stops', sample: 10, page: 1 },
  { label: 'All sixteen /BM blend modes', sample: 11, page: 0 },
  { label: 'A Type 1 font program (`/FontFile`)', sample: 12, page: 0 },
  // 1.4
  { label: 'A tiling pattern filling a rectangle', sample: 13, page: 0 },
  { label: 'Its colour key: the red band is the page', sample: 13, page: 0 },
  { label: 'A colour key that hides its own samples', sample: 14, page: 0 },
  { label: '`Tr` 7 clips to the glyph and paints nothing', sample: 15, page: 0 },
  { label: '`Tr` 4 fills the glyph and clips what follows', sample: 15, page: 1 },
  // 1.5
  { label: 'A JPEG 2000 codestream decoded and drawn', sample: 17, page: 0 },
  { label: 'A JBIG2 scan, ink on white', sample: 18, page: 0 },
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
  factSkipped: document.querySelector('#factSkipped'),
  text: document.querySelector('#text'),
  download: document.querySelector('#download'),
  features: document.querySelector('#features'),
  fitPage: document.querySelector('#fitPage'),
  minimapBox: document.querySelector('#minimapBox'),
  minimapInner: document.querySelector('#minimapInner'),
  minimap: document.querySelector('#minimap'),
  minimapViewport: document.querySelector('#minimapViewport'),
  pageBox: document.querySelector('#pageBox'),
  textLayer: document.querySelector('#textLayer'),
  find: document.querySelector('#find'),
  findPrev: document.querySelector('#findPrev'),
  findNext: document.querySelector('#findNext'),
  findInfo: document.querySelector('#findInfo'),
  copySelection: document.querySelector('#copySelection'),
  selectionInfo: document.querySelector('#selectionInfo'),
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
  spans: [],
  hits: [],
  hitIndex: 0,
  canvasWidth: 0,
  canvasHeight: 0,
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
  const byWidth = availableWidth / pageWidth;
  if (state.zoom === 'page') {
    // The wrap's **height** is content: it grows with the canvas up to its CSS
    // cap, so measuring `clientHeight` feeds the page's own size back into the
    // scale — and with a ResizeObserver re-rendering on every wrap resize that
    // feeds itself: choosing "Page" shrank the page a few pixels per frame, all
    // the way to the floor. The cap is the honest number, and it does not depend
    // on what is currently drawn.
    const cap = Number.parseFloat(getComputedStyle(wrap).maxHeight);
    const height = Number.isFinite(cap) ? cap : (globalThis.innerHeight || 0) * 0.78;
    const availableHeight = Math.max(height - 8, 120);
    return Math.min(byWidth, availableHeight / pageHeight);
  }
  return byWidth;
}

/// Put a selectable, transparent copy of the page's text over the canvas.
///
/// The spans come from the same geometry the find box highlights: page-space
/// rectangles turned into CSS pixels against the page box, with the y axis
/// flipped (PDF counts from the bottom, the DOM from the top). The glyphs are not
/// reproduced — the canvas drew those — only the selectable text and its box, and
/// a span's own box is the unit of selection, which `ADR 0008` says is what a span
/// is.
function buildTextLayer(base, pageHeight) {
  elements.textLayer.replaceChildren();
  state.spans = [];
  state.hits = [];
  state.hitIndex = 0;
  if (!state.document) {
    return;
  }
  let boxes;
  let texts;
  try {
    boxes = state.document.spans(state.page);
    texts = state.document.spanTexts(state.page);
  } catch {
    return;
  }
  const layer = elements.textLayer;
  // The width is the browser's own text width, not the line's: the transform below
  // is what makes the glyph run fill the line, and a width of the line box would be
  // scaled too, leaving the run `box / measured` of the way across it.
  const built = [];
  for (let index = 0; index < texts.length; index += 1) {
    const [x0, y0, x1, y1] = Array.from(boxes.slice(index * 4, index * 4 + 4));
    const span = document.createElement('span');
    span.textContent = texts[index];
    span.dataset.index = String(index);
    span.style.left = `${x0 * base}px`;
    span.style.top = `${(pageHeight - y1) * base}px`;
    span.style.width = 'max-content';
    span.style.height = `${Math.max(1, (y1 - y0) * base)}px`;
    span.style.fontSize = `${Math.max(1, (y1 - y0) * base)}px`;
    layer.append(span);
    built.push({ span, box: Math.max(1, (x1 - x0) * base) });
    state.spans.push({ text: texts[index], x0, y0, x1, y1 });
  }
  // The invisible text is laid out with the browser's advances, which are not the
  // page's. Scaling each span by the ratio of the painted line to the measured text
  // puts every glyph — and with it hit-testing and `::selection` — where the canvas
  // painted it; pdf.js's text layer scales the same way. Measured after the spans
  // are in the document, so the whole set shares one layout pass.
  for (const { span, box } of built) {
    const measured = span.getBoundingClientRect().width;
    if (measured > 0.5) {
      span.style.transform = `scaleX(${box / measured})`;
    }
    span.dataset.box = box.toFixed(2);
  }
  layer.style.width = `${Math.round(state.canvasWidth)}px`;
  layer.style.height = `${Math.round(state.canvasHeight)}px`;
  renderMinimap();
}

/// The text the browser currently has selected, inside the text layer only.
function selectedText() {
  const selection = globalThis.getSelection();
  if (!selection || selection.rangeCount === 0) {
    return '';
  }
  const node = selection.anchorNode;
  if (node && elements.textLayer.contains(node.nodeType === 1 ? node : node.parentNode)) {
    return selection.toString();
  }
  return '';
}

/// Highlight every hit, with the active one brighter.
function paintHits() {
  for (const mark of elements.textLayer.querySelectorAll('.find-mark')) {
    mark.remove();
  }
  if (state.hits.length === 0) {
    return;
  }
  const hit = state.hits[state.hitIndex];
  const [x0, y0, x1, y1] = hit.box;
  const mark = document.createElement('div');
  mark.className = 'find-mark active';
  mark.style.left = `${x0 * hit.base}px`;
  mark.style.top = `${(hit.pageHeight - y1) * hit.base}px`;
  mark.style.width = `${Math.max(1, (x1 - x0) * hit.base)}px`;
  mark.style.height = `${Math.max(1, (y1 - y0) * hit.base)}px`;
  elements.textLayer.prepend(mark);
}

/// Run the current needle, moving to the first hit's page when it is elsewhere.
async function runFind(needle, move = 0) {
  state.hits = [];
  state.hitIndex = 0;
  if (!needle) {
    elements.findInfo.textContent = '–';
    paintHits();
    return;
  }
  let boxes;
  let texts;
  try {
    boxes = state.document.search(needle);
    texts = state.document.searchTexts(needle);
  } catch {
    elements.findInfo.textContent = '0 / 0';
    return;
  }
  const count = Math.floor(boxes.length / 5);
  for (let index = 0; index < count; index += 1) {
    const [page, x0, y0, x1, y1] = Array.from(boxes.slice(index * 5, index * 5 + 5));
    state.hits.push({ page, text: texts[index], box: [x0, y0, x1, y1] });
  }
  if (state.hits.length === 0) {
    elements.findInfo.textContent = '0 / 0';
    paintHits();
    return;
  }
  state.hitIndex = (state.hitIndex + move + state.hits.length) % state.hits.length;
  await focusHit();
}

/// Show one hit: its page, its highlight, and the counter.
async function focusHit() {
  const hit = state.hits[state.hitIndex];
  if (!hit) {
    return;
  }
  const [, pageHeight] = state.document.pageSize(hit.page);
  const base = scaleFor(...state.document.pageSize(hit.page));
  hit.base = base;
  hit.pageHeight = pageHeight;
  if (hit.page !== state.page) {
    state.page = hit.page;
    await render();
  }
  elements.findInfo.textContent = `${state.hitIndex + 1} / ${state.hits.length}`;
  paintHits();
  const mark = elements.textLayer.querySelector('.find-mark.active');
  if (mark) {
    mark.scrollIntoView({ block: 'center', inline: 'center' });
  }
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
    const skipped = JSON.parse(
      state.document.renderPageToCanvas(state.page, base * dpr, elements.canvas) || '{}',
    );
    const elapsed = performance.now() - started;
    if (token !== state.renderToken) {
      return;
    }
    elements.canvas.style.width = `${Math.round(width * base)}px`;
    elements.canvas.style.height = `${Math.round(height * base)}px`;
    state.canvasWidth = width * base;
    state.canvasHeight = height * base;
    // The text layer is rebuilt with every render: the geometry moves with the
    // page and the zoom, and a stale layer would select the wrong words.
    buildTextLayer(base, height);
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
    const skippedReasons = Object.entries(skipped);
    fact(
      elements.factSkipped,
      skippedReasons.length === 0
        ? 'nothing'
        : skippedReasons.map(([reason, count]) => `${reason} ×${count}`).join(', '),
    );
    // Text extraction can fail for a page the renderer draws perfectly well —
    // composite fonts without ToUnicode, for one — so it gets its own report
    // instead of taking the render down with it.
    let notes = [];
    if (skippedReasons.length > 0) {
      notes.push(
        `the page is blank where it used ${skippedReasons
          .map(([reason]) => reason.replace(/^font:/, 'the font '))
          .join(', ')}`,
      );
    }
    try {
      const text = (state.document.text(state.page) || '').trim();
      elements.text.textContent = text.length > 0 ? text.slice(0, 800) : '(no extractable text)';
    } catch (error) {
      elements.text.textContent = `text extraction unavailable\n${error.message || error}`;
      notes.push('its text cannot be extracted');
    }
    status(
      `${state.name}: PDF ${state.document.version()}, ${pages} page(s), ` +
        `rendered page ${state.page + 1} in ${elapsed.toFixed(1)} ms` +
        (notes.length > 0 ? ` — ${notes.join('; ')}` : ''),
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
elements.copySelection.addEventListener('click', async () => {
  const text = selectedText();
  if (!text) {
    return;
  }
  try {
    await navigator.clipboard.writeText(text);
    elements.selectionInfo.textContent = `copied ${text.length} character(s)`;
  } catch {
    elements.selectionInfo.textContent = 'the clipboard is not available';
  }
});

document.addEventListener('selectionchange', () => {
  const text = selectedText().trim();
  elements.copySelection.disabled = text.length === 0;
  elements.selectionInfo.textContent = text.length === 0 ? '' : text;
});

elements.find.addEventListener('input', () => {
  runFind(elements.find.value.trim());
});
elements.find.addEventListener('keydown', (event) => {
  if (event.key === 'Enter') {
    event.preventDefault();
    runFind(elements.find.value.trim(), event.shiftKey ? -1 : 1);
  }
});
elements.findNext.addEventListener('click', () => runFind(elements.find.value.trim(), 1));
elements.findPrev.addEventListener('click', () => runFind(elements.find.value.trim(), -1));

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
let wrapWidth = elements.canvasWrap.clientWidth;
new ResizeObserver((entries) => {
  const box = entries[entries.length - 1].contentRect;
  const widthChanged = Math.abs(box.width - wrapWidth) > 0.5;
  if (widthChanged) {
    wrapWidth = box.width;
  }
  if (!state.document) {
    return;
  }
  if (typeof state.zoom === 'number') {
    updateViewport();
    return;
  }
  // Rendering changes the wrap's *height* in the fit modes, so a render that
  // re-renders on every resize chases its own output. Only a width change — the
  // window, the layout — needs a new page; a height change just moves the
  // minimap's rectangle.
  if (!widthChanged) {
    updateViewport();
    return;
  }
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(render, 120);
}).observe(elements.canvasWrap);

await init();
await loadSample(0);
