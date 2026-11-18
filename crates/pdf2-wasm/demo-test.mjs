// Interaction test for the demo page, driving a real browser over the Chrome
// DevTools Protocol. No npm dependencies: Node's `fetch` and `WebSocket` are
// enough, and the browser is whatever is installed on the machine.
//
//   node crates/pdf2-wasm/demo-test.mjs
//   DEMO_URL=https://pdf2-rs.github.io/pdf2/ node crates/pdf2-wasm/demo-test.mjs
//
// It starts a static server over `crates/pdf2-wasm/demo`, launches headless
// Chrome, and asserts what a person would check by hand: the sample select keeps
// its choice, the feature list jumps to the page it names, zoom makes the page
// scrollable in both directions with its left edge reachable, the minimap moves
// with the scroll position, Download hands over the document that is on screen,
// the encrypted file asks for its password, and a dropped file replaces the one
// on screen. A failure prints the assertion and exits non-zero.
//
// With `DEMO_URL` set it drives the deployed site instead of the local build,
// which is how a deployment is checked rather than merely requested.

import { createServer } from 'node:http';
import { createReadStream, existsSync } from 'node:fs';
import { mkdtemp, readFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { extname, join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const here = resolve(new URL('.', import.meta.url).pathname);
const root = join(here, 'demo');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.pdf': 'application/pdf',
  '.json': 'application/json',
  '.css': 'text/css; charset=utf-8',
};

function startServer() {
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    const path = join(root, decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname));
    if (!path.startsWith(root)) {
      response.writeHead(403).end();
      return;
    }
    try {
      const info = await stat(path);
      if (!info.isFile()) {
        response.writeHead(404).end();
        return;
      }
      response.writeHead(200, {
        'content-type': MIME[extname(path)] ?? 'application/octet-stream',
        'content-length': info.size,
      });
      createReadStream(path).pipe(response);
    } catch {
      response.writeHead(404).end();
    }
  });
  return new Promise((done) => {
    server.listen(0, '127.0.0.1', () => done(server));
  });
}

function findChrome() {
  const candidates = [
    process.env.CHROME_BIN,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/usr/bin/google-chrome',
    '/usr/bin/google-chrome-stable',
    '/usr/bin/chromium',
    '/usr/bin/chromium-browser',
  ].filter(Boolean);
  for (const candidate of candidates) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    `no Chrome found; set CHROME_BIN (looked at ${candidates.join(', ')})`,
  );
}

async function launchChrome() {
  const profile = await mkdtemp(join(tmpdir(), 'pdf2-demo-test-'));
  const chrome = spawn(
    findChrome(),
    [
      '--headless=new',
      '--disable-gpu',
      '--no-sandbox',
      '--disable-dev-shm-usage',
      '--no-first-run',
      '--no-default-browser-check',
      '--window-size=1200,900',
      // A screenshot is compared channel by channel with canvas values, so the
      // compositor must work in sRGB: without this the profile of the machine
      // decides (macOS passed, a Linux runner turned 255 into 241).
      '--force-color-profile=srgb',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      'about:blank',
    ],
    { stdio: 'ignore' },
  );
  const portFile = join(profile, 'DevToolsActivePort');
  let port = null;
  // Ten seconds was tight on a loaded runner — the job failed with "Chrome did
  // not open a DevTools port" while every interaction check passed locally — so
  // the wait is thirty, and the watchdog above still bounds the whole run.
  for (let attempt = 0; attempt < 300 && port === null; attempt += 1) {
    await new Promise((done) => setTimeout(done, 100));
    try {
      const [first] = (await readFile(portFile, 'utf8')).split('\n');
      port = Number(first);
    } catch {
      // The file appears once the browser is listening.
    }
  }
  if (!port) {
    chrome.kill('SIGKILL');
    throw new Error('Chrome did not open a DevTools port');
  }
  const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
  return { chrome, profile, browserUrl: version.webSocketDebuggerUrl };
}

/// A small CDP client: one socket, promises keyed by message id.
function connect(url) {
  const socket = new WebSocket(url);
  const pending = new Map();
  let nextId = 1;
  const events = [];
  const ready = new Promise((done, fail) => {
    socket.addEventListener('open', done, { once: true });
    socket.addEventListener('error', fail, { once: true });
  });
  // A browser that dies mid-run closes the socket without answering: reject
  // everything in flight instead of waiting forever.
  socket.addEventListener('close', () => {
    for (const { reject } of pending.values()) {
      reject(new Error('the DevTools connection closed'));
    }
    pending.clear();
  });
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      const { resolve: done, reject } = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) {
        reject(new Error(`${message.error.message} (${message.error.code})`));
      } else {
        done(message.result);
      }
      return;
    }
    events.push(message);
  });
  return {
    events,
    async send(method, params = {}, sessionId) {
      await ready;
      const id = nextId++;
      const payload = { id, method, params };
      if (sessionId) {
        payload.sessionId = sessionId;
      }
      socket.send(JSON.stringify(payload));
      return new Promise((done, fail) => pending.set(id, { resolve: done, reject: fail }));
    },
    close() {
      socket.close();
    },
  };
}

const failures = [];

function check(condition, message, detail) {
  if (condition) {
    console.log(`ok   ${message}`);
    return;
  }
  failures.push(detail === undefined ? message : `${message} — ${detail}`);
  console.log(`FAIL ${message}${detail === undefined ? '' : ` — ${detail}`}`);
}

async function main() {
  const remote = process.env.DEMO_URL;
  let server = null;
  let origin = remote;
  if (remote) {
    console.log(`driving the deployed demo at ${remote}`);
  } else {
    // The demo is built, not committed: the wasm glue and the samples are
    // outputs of `cargo xtask wasm-package`. Testing a stale or missing build
    // would pass for the wrong reason, so say what is missing instead.
    for (const required of [
      'pkg/pdf2_wasm.js',
      'pkg/pdf2_wasm_bg.wasm',
      'samples/text.pdf',
      'samples/calibration.pdf',
    ]) {
      if (!existsSync(join(root, required))) {
        throw new Error(`missing ${required}; run \`cargo xtask wasm-package\` first`);
      }
    }
    server = await startServer();
    origin = `http://127.0.0.1:${server.address().port}`;
  }
  const { chrome, profile, browserUrl } = await launchChrome();
  // A hard stop: a hung browser must fail the job in minutes, not hours.
  const watchdog = setTimeout(() => {
    console.error('demo test failed: no progress within 180 s; killing the browser');
    chrome.kill('SIGKILL');
    process.exit(1);
  }, 180_000);
  const cdp = connect(browserUrl);

  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const send = (method, params) => cdp.send(method, params, sessionId);
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Log.enable');

  async function evaluate(expression) {
    const result = await send('Runtime.evaluate', {
      expression: `(async () => { ${expression} })()`,
      awaitPromise: true,
      returnByValue: true,
      userGesture: true,
    });
    if (result.exceptionDetails) {
      throw new Error(
        `in the page: ${result.exceptionDetails.exception?.description ?? result.exceptionDetails.text}`,
      );
    }
    return result.result.value;
  }

  async function waitFor(expression, description, timeout = 15000) {
    const deadline = Date.now() + timeout;
    let last;
    while (Date.now() < deadline) {
      try {
        last = await evaluate(`return (${expression});`);
        if (last) {
          return last;
        }
      } catch (error) {
        last = String(error);
      }
      await new Promise((done) => setTimeout(done, 100));
    }
    const status = await evaluate(`return document.querySelector('#status')?.textContent;`).catch(
      () => '<unavailable>',
    );
    const logged = cdp.events
      .filter((event) => event.method === 'Log.entryAdded')
      .map((event) => `${event.params?.entry?.level}: ${event.params?.entry?.text}`)
      .slice(-5);
    throw new Error(
      `timed out waiting for ${description} (last value: ${JSON.stringify(last)}; ` +
        `status ${JSON.stringify(status)}; log ${JSON.stringify(logged)})`,
    );
  }

  try {
    await send('Page.navigate', { url: `${origin}/index.html` });
    await waitFor(
      `document.querySelector('#factName')?.textContent === 'text.pdf' && document.querySelector('#pageInfo')?.textContent === '1 / 1'`,
      'the first sample to render',
      30000,
    );
    check(true, 'the demo loads its first sample');

    // The page says which build it is, and both halves come from `build.json`:
    // a version alone cannot tell two builds of one release apart, and a
    // hard-coded string would be the wrong build the moment either moved.
    const footer = await evaluate(`
      const build = await (await fetch('build.json', { cache: 'no-store' })).json();
      const shown = document.querySelector('#appVersion')?.textContent ?? '';
      const entry = (performance.getEntriesByType('resource') ?? []).find((item) =>
        /pdf2_wasm\\.js/.test(item.name),
      );
      return {
        build,
        shown,
        moduleUrl: entry ? entry.name : null,
      };
    `);
    const wanted = `pdf2-wasm ${footer.build.version} (${footer.build.rev ?? 'unknown'})`;
    check(
      footer.shown === wanted,
      `the footer names the build: ${JSON.stringify(footer.shown)}`,
      `expected ${JSON.stringify(wanted)}`,
    );
    check(
      footer.moduleUrl !== null &&
        footer.moduleUrl.endsWith(`?v=${encodeURIComponent(footer.build.build)}`),
      `the module is imported with the build stamp: ${footer.moduleUrl}`,
    );

    check(
      (await evaluate(`return document.querySelector('#sample').value;`)) === '0',
      'the sample select shows the sample that is loaded',
    );
    check(
      (await evaluate(`return document.querySelector('#canvas').width;`)) > 100,
      'the canvas has been rendered',
    );
    check(
      (await evaluate(`return document.querySelector('#download').disabled;`)) === false,
      'Download is enabled once a document is open',
    );

    // Choosing a sample must keep the choice visible (it used to be cleared).
    await evaluate(`
      const select = document.querySelector('#sample');
      select.value = '3';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#pageInfo').textContent.startsWith('1 / 11')`,
      'calibration.pdf to load with 11 pages',
    );
    check(
      (await evaluate(`return document.querySelector('#sample').value;`)) === '3',
      'the chosen sample stays selected',
    );
    check(
      (await evaluate(`return document.querySelector('#factPages').textContent;`)) === '11',
      'the facts panel agrees on the page count',
    );

    // A feature jumps to the page it names.
    await evaluate(`
      [...document.querySelectorAll('#features button')]
        .find((button) => button.textContent.includes('Composite (Type 0)'))
        .click();
    `);
    await waitFor(
      `document.querySelector('#pageInfo').textContent.startsWith('10 /')`,
      'the composite-font page',
    );
    check(true, 'a feature button jumps to its page');
    check(
      (await evaluate(`
        const px = document.querySelector('#canvas').getContext('2d')
          .getImageData(0, 0, document.querySelector('#canvas').width, document.querySelector('#canvas').height).data;
        let ink = 0;
        for (let i = 0; i < px.length; i += 4) if (px[i] === 0 && px[i + 1] === 0 && px[i + 2] === 0) ink += 1;
        return ink;
      `)) > 100,
      'the page it jumps to is actually painted',
    );

    // The book sample: a page of prose in an embedded serif font, which is what
    // a reader judges "normal text" by. It has to arrive with ink, and enough of
    // it to be a page rather than a line.
    await evaluate(`
      [...document.querySelectorAll('#features button')]
        .find((button) => button.textContent.includes('embedded serif'))
        .click();
      await new Promise((done) => setTimeout(done, 400));
    `);
    await waitFor(
      `document.querySelector('#factName').textContent === 'book.pdf'`,
      'the book sample to load',
    );
    const bookInk = await evaluate(`
      const canvas = document.querySelector('#canvas');
      const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
      let ink = 0;
      for (let i = 0; i < data.length; i += 4) {
        if (data[i] !== 255 || data[i + 1] !== 255 || data[i + 2] !== 255) ink += 1;
      }
      return { ink, page: document.querySelector('#pageInfo').textContent };
    `);
    check(
      bookInk.ink > 5000,
      'a page of prose in an embedded serif font is drawn',
      JSON.stringify(bookInk),
    );
    check(
      /The Time Traveller/.test(await evaluate(`return document.querySelector('#text').textContent;`)),
      'and its text is extractable',
    );

    // Zoom: the page must scroll in both directions, and its left edge must be
    // reachable — the bug was that centring in a scroll container hid it. Page 3
    // is 400 pt wide, so 300% is wider than any laptop viewer.
    await evaluate(`
      const select = document.querySelector('#sample');
      select.value = [...select.options].find((option) => /Calibration/.test(option.textContent)).value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#pageInfo').textContent.startsWith('1 / 11')`,
      'calibration again before the zoom checks',
    );
    await evaluate(`
      document.querySelector('#next').click();
      await new Promise((done) => setTimeout(done, 200));
      document.querySelector('#next').click();
      await new Promise((done) => setTimeout(done, 300));
    `);
    await waitFor(
      `document.querySelector('#pageInfo').textContent.startsWith('3 /')`,
      'the widest page before zooming',
    );
    await waitFor(
      `(() => {
        const wrap = document.querySelector('#canvasWrap');
        if (wrap.scrollWidth > wrap.clientWidth) return true;
        const zoomIn = document.querySelector('#zoomIn');
        if (zoomIn.disabled) return false;
        zoomIn.click();
        return false;
      })()`,
      'the zoomed page to overflow horizontally',
    );

    const geometry = await evaluate(`
      const wrap = document.querySelector('#canvasWrap');
      wrap.scrollLeft = 0;
      await new Promise((done) => requestAnimationFrame(done));
      const page = document.querySelector('#canvas').getBoundingClientRect();
      const view = wrap.getBoundingClientRect();
      return {
        reachable: page.left >= view.left - 1,
        offset: Math.round(page.left - view.left),
        scrollWidth: wrap.scrollWidth,
        clientWidth: wrap.clientWidth,
        zoom: document.querySelector('#zoomInfo').textContent,
      };
    `);
    check(
      geometry.reachable,
      'the left edge of a zoomed page can be scrolled to',
      `canvas starts ${geometry.offset}px from the viewer edge (${geometry.scrollWidth} of ${geometry.clientWidth} px, zoom ${geometry.zoom})`,
    );

    // The minimap appears when the page does not fit, and tracks the scroll.
    check(
      await evaluate(`return document.querySelector('#minimapBox').classList.contains('visible');`),
      'the minimap appears when the page does not fit',
    );
    const before = await evaluate(`return document.querySelector('#minimapViewport').style.left;`);
    await evaluate(`
      const wrap = document.querySelector('#canvasWrap');
      wrap.scrollLeft = wrap.scrollWidth;
      await new Promise((done) => requestAnimationFrame(done));
    `);
    const after = await evaluate(`return document.querySelector('#minimapViewport').style.left;`);
    check(
      parseFloat(after) > parseFloat(before),
      'the minimap viewport follows the scroll position',
      `${before} -> ${after}`,
    );

    // Clicking the minimap moves the page under the cursor.
    const moved = await evaluate(`
      const wrap = document.querySelector('#canvasWrap');
      wrap.scrollLeft = 0;
      wrap.scrollTop = 0;
      await new Promise((done) => requestAnimationFrame(done));
      const inner = document.querySelector('#minimapInner').getBoundingClientRect();
      document.querySelector('#minimapInner').dispatchEvent(new PointerEvent('pointerdown', {
        clientX: inner.left + inner.width / 2,
        clientY: inner.top + inner.height / 2,
        pointerId: 1,
        bubbles: true,
      }));
      await new Promise((done) => requestAnimationFrame(done));
      return { scrollLeft: wrap.scrollLeft, scrollTop: wrap.scrollTop };
    `);
    check(
      moved.scrollLeft > 0 || moved.scrollTop > 0,
      'clicking the minimap scrolls the page',
      `scrollLeft ${moved.scrollLeft}, scrollTop ${moved.scrollTop}`,
    );

    // "Page" fits the whole page, and it must stay fitted: the wrap's height is
    // content, so a scale measured from it fed the page's own size back into
    // itself, and a ResizeObserver re-rendering on every resize turned that into a
    // loop — the page shrank a few pixels per frame until it hit the floor. Two
    // clicks and a settle must leave the same size both times.
    const pageSize = async () => {
      await evaluate(`document.querySelector('#fitPage').click();`);
      await new Promise((done) => setTimeout(done, 600));
      return evaluate(`
        const canvas = document.querySelector('#canvas');
        const wrap = document.querySelector('#canvasWrap');
        return {
          canvas: Math.round(canvas.getBoundingClientRect().height),
          wrap: Math.round(wrap.getBoundingClientRect().height),
          zoom: document.querySelector('#zoomInfo').textContent,
        };
      `);
    };
    const firstFit = await pageSize();
    const secondFit = await pageSize();
    check(
      firstFit.canvas > 200 && secondFit.canvas === firstFit.canvas,
      `"Page" fits once and stays fitted: ${firstFit.canvas}px then ${secondFit.canvas}px`,
    );
    check(
      secondFit.wrap >= secondFit.canvas - 2,
      `the wrap is not clamped under the page: wrap ${secondFit.wrap}px, page ${secondFit.canvas}px`,
    );
    // And it really does fit: the whole page is inside the wrap's height cap.
    check(
      secondFit.canvas <= secondFit.wrap + 2,
      `the page fits inside the wrap: ${secondFit.canvas}px of ${secondFit.wrap}px`,
    );

    // Fit gives the page back its container, and hides the minimap.
    await evaluate(`document.querySelector('#fit').click();`);
    await waitFor(
      `document.querySelector('#canvasWrap').scrollWidth <= document.querySelector('#canvasWrap').clientWidth + 2`,
      'fit to remove the horizontal overflow',
    );
    check(
      !(await evaluate(`return document.querySelector('#minimapBox').classList.contains('visible');`)),
      'Fit hides the minimap again',
    );

    // The text layer: one selectable span per line, laid over the canvas at the
    // page's own coordinates. Selection is what a reader does with a PDF, and it
    // reads the same spans the find box highlights. The sample has to be one whose
    // text is on the page rather than inside a form or a Type 3 glyph run, so this
    // check switches to the plain text sample first.
    await evaluate(`
      const select = document.querySelector('#sample');
      select.value = [...select.options].find((option) => /Text and accents/.test(option.textContent)).value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'text.pdf'`, 'the text sample');
    await new Promise((done) => setTimeout(done, 300));
    const layer = await evaluate(`
      const canvas = document.querySelector('#canvas').getBoundingClientRect();
      const layer = document.querySelector('#textLayer').getBoundingClientRect();
      const spans = [...document.querySelectorAll('#textLayer span')];
      return {
        canvas: [Math.round(canvas.width), Math.round(canvas.height)],
        layer: [Math.round(layer.width), Math.round(layer.height)],
        count: spans.length,
        first: spans[0] ? spans[0].textContent : '',
        page: document.querySelector('#text').textContent,
      };
    `);
    check(layer.count > 0, `the text layer has spans: ${layer.count}`);
    check(
      Math.abs(layer.canvas[0] - layer.layer[0]) <= 2 && Math.abs(layer.canvas[1] - layer.layer[1]) <= 2,
      `the layer covers the canvas: ${layer.layer} against ${layer.canvas}`,
    );
    check(
      layer.first.trim().length > 0 && layer.page.includes(layer.first.trim().slice(0, 8)),
      `the first span is the page's own text: ${JSON.stringify(layer.first)}`,
    );

    // Selection has to land on the paint. The layer is one span per painted
    // glyph, positioned from the renderer's own numbers, so a word's selection
    // rectangle and the canvas's inked columns in that rectangle must agree --
    // the old layer scaled the browser's text into the *extractor's* line box,
    // which for a base-14 font is a quarter too narrow.
    const alignment = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      // The first word: the first run of spans up to the first space.
      const word = [];
      for (const span of spans) {
        if (span.textContent === ' ') break;
        if (span.textContent.trim().length > 0) word.push(span);
      }
      const range = document.createRange();
      range.setStartBefore(word[0]);
      range.setEndAfter(word[word.length - 1]);
      const native = window.getSelection();
      native.removeAllRanges();
      native.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      // What a reader sees is the overlay, not the browser's own rect.
      const marks = [...document.querySelectorAll('#selectionLayer i')].map((mark) => mark.getBoundingClientRect());
      const selection = marks.length === 1
        ? marks[0]
        : { left: Math.min(...marks.map((m) => m.left)), right: Math.max(...marks.map((m) => m.right)),
            top: Math.min(...marks.map((m) => m.top)), bottom: Math.max(...marks.map((m) => m.bottom)) };
      const text = word.map((span) => span.textContent).join('');
      // Ink inside that rectangle, in canvas pixels.
      const x0 = Math.floor((selection.left - canvasRect.left) * toBitmap);
      const x1 = Math.ceil((selection.right - canvasRect.left) * toBitmap);
      const y0 = Math.max(0, Math.floor((selection.top - canvasRect.top) * toBitmap));
      const y1 = Math.min(canvas.height, Math.ceil((selection.bottom - canvasRect.top) * toBitmap));
      let firstInk = null;
      let lastInk = null;
      for (let x = x0; x < x1; x += 1) {
        let ink = false;
        for (let y = y0; y < y1 && !ink; y += 1) {
          const offset = (y * canvas.width + x) * 4;
          ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
        }
        if (ink) { if (firstInk === null) firstInk = x; lastInk = x; }
      }
      const glyphs = spans.length;
      const scaled = spans.filter((span) => getComputedStyle(span).transform !== 'none').length;
      return {
        text,
        glyphs,
        scaled,
        selection: [selection.left, selection.right],
        ink: firstInk === null ? null : [firstInk / toBitmap + canvasRect.left, (lastInk + 1) / toBitmap + canvasRect.left],
      };
    `);
    check(alignment.text.length > 1, `the layer's first word is "${alignment.text}" over ${alignment.glyphs} glyph spans`);
    check(
      alignment.scaled === 0,
      `no span is scaled any more: ${alignment.scaled} of ${alignment.glyphs} carry a transform`,
    );
    check(alignment.ink !== null, 'the word was painted on the canvas');
    if (alignment.ink !== null) {
      // The word's ink must sit *inside* its selection rectangle: the rectangle
      // runs to the last glyph's advance, which a comma does not reach, but no
      // painted column may fall outside it. The old layer failed here by 58 px,
      // its box being 0.758 of the paint.
      const leftDelta = Math.abs(alignment.ink[0] - alignment.selection[0]);
      const overflow = Math.max(
        alignment.selection[0] - alignment.ink[0],
        alignment.ink[1] - alignment.selection[1],
        0,
      );
      check(
        leftDelta <= 2 && overflow <= 2,
        `the selection covers the painted word: ink ${alignment.ink.map((v) => v.toFixed(1))} ` +
          `inside selection ${alignment.selection.map((v) => v.toFixed(1))}, ` +
          `left gap ${leftDelta.toFixed(1)} px, overflow ${overflow.toFixed(1)} px`,
      );
    }

    // The highlight is ours: one rectangle per line, welded from the glyph cells,
    // so adjacent characters share an edge. The browser's own `::selection` is
    // transparent -- it follows the font it substituted (4.6 px for an "l" inside
    // a 14 px cell) and would show slivers between the letters.
    const merge = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      const word = spans.slice(0, 6);
      const range = document.createRange();
      range.setStartBefore(word[0]);
      range.setEndAfter(word[word.length - 1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      const marks = [...document.querySelectorAll('#selectionLayer i')];
      const cells = word.map((span) => span.getBoundingClientRect());
      const mark = marks[0] ? marks[0].getBoundingClientRect() : null;
      return {
        text: word.map((span) => span.textContent).join(''),
        marks: marks.length,
        width: mark ? mark.width : 0,
        sum: cells.reduce((total, cell) => total + cell.width, 0),
        left: mark ? mark.left : 0,
        firstLeft: cells[0].left,
        right: mark ? mark.right : 0,
        lastRight: cells[cells.length - 1].right,
        height: mark ? mark.height : 0,
        cellHeight: cells[0].height,
      };
    `);
    check(
      merge.marks === 1,
      `the ${JSON.stringify(merge.text)} selection is one rectangle per line: ${merge.marks} mark(s)`,
    );
    check(
      Math.abs(merge.width - merge.sum) <= 0.5,
      `its width is the cells it welds: ${merge.width.toFixed(1)} against ${merge.sum.toFixed(1)} px`,
    );
    check(
      Math.abs(merge.left - merge.firstLeft) <= 0.5 && Math.abs(merge.right - merge.lastRight) <= 0.5,
      `it runs from the first cell to the last: ${merge.left.toFixed(1)}..${merge.right.toFixed(1)} ` +
        `against ${merge.firstLeft.toFixed(1)}..${merge.lastRight.toFixed(1)}`,
    );
    check(
      Math.abs(merge.height - merge.cellHeight) <= 0.5,
      `its height is the glyph's cell, not the font's metrics: ${merge.height.toFixed(1)} against ` +
        `${merge.cellHeight.toFixed(1)} px`,
    );

    // It is an inversion, not a tint: under the overlay the page is the `255 -`
    // of itself. The sample below finds an inked pixel and a blank one inside the
    // *same* cell, reads both from the canvas, then reads the same two spots off a
    // screenshot -- the only way to see what the blend composed.
    const probes = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      const word = spans.slice(0, 6);
      const range = document.createRange();
      range.setStartBefore(word[0]);
      range.setEndAfter(word[word.length - 1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));

      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      const marks = [...document.querySelectorAll('#selectionLayer i')];
      if (marks.length !== 1) return { marks: marks.length };
      const mark = marks[0].getBoundingClientRect();
      // Sample *one CSS pixel* of the screenshot against the canvas pixels it
      // covers, and only accept a position where those pixels agree: a 1x
      // screenshot averages them, so a single dark pixel beside light ones would
      // compare an average against one of its terms.
      const sample = (x, y) => {
        const values = [];
        for (let bx = Math.floor((x - canvasRect.left) * toBitmap);
             bx < Math.ceil((x + 1 - canvasRect.left) * toBitmap); bx += 1) {
          for (let by = Math.floor((y - canvasRect.top) * toBitmap);
               by < Math.ceil((y + 1 - canvasRect.top) * toBitmap); by += 1) {
            values.push(image.data[(by * canvas.width + bx) * 4]);
          }
        }
        return values.length > 0 ? values : [255];
      };
      const y = Math.round(mark.top + mark.height / 2) + 0.5;
      const uniform = (values, low, high) => values.every((v) => v >= low && v <= high);
      const average = (values) => values.reduce((a, b) => a + b, 0) / values.length;
      let ink = null;
      let blank = null;
      for (let x = Math.round(mark.left) + 0.5; x < mark.right - 0.5 && (ink === null || blank === null); x += 1) {
        const values = sample(x, y);
        if (ink === null && uniform(values, 0, 20)) ink = { x, y, value: average(values) };
        if (blank === null && uniform(values, 240, 255)) blank = { x, y, value: average(values) };
      }
      return {
        marks: marks.length,
        ink: ink && { cssX: ink.x, cssY: ink.y, value: ink.value },
        blank: blank && { cssX: blank.x, cssY: blank.y, value: blank.value },
        rect: [mark.left, mark.top, mark.width, mark.height],
      };
    `);
    check(
      probes.marks === 1 && probes.ink !== null && probes.blank !== null,
      `the selected cell holds a solid ink pixel and a solid background pixel: ` +
        `${probes.ink?.value} and ${probes.blank?.value} over ${probes.marks} mark(s)`,
    );
    if (probes.ink && probes.blank) {
      const clip = {
        x: Math.max(0, Math.floor(probes.rect[0] - 2)),
        y: Math.max(0, Math.floor(probes.rect[1] - 2)),
        width: Math.ceil(probes.rect[2] + 4),
        height: Math.ceil(probes.rect[3] + 4),
        scale: 1,
      };
      const shot = await send('Page.captureScreenshot', { format: 'png', clip });
      const inverted = await evaluate(`
        const bytes = Uint8Array.from(atob(${JSON.stringify(shot.data)}), (c) => c.charCodeAt(0));
        const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
        const offscreen = new OffscreenCanvas(bitmap.width, bitmap.height);
        const context = offscreen.getContext('2d');
        context.drawImage(bitmap, 0, 0);
        const read = (cssX, cssY) => {
          const pixel = context.getImageData(Math.round(cssX - ${clip.x}), Math.round(cssY - ${clip.y}), 1, 1).data;
          return [pixel[0], pixel[1], pixel[2]];
        };
        return {
          ink: read(${probes.ink.cssX}, ${probes.ink.cssY}),
          blank: read(${probes.blank.cssX}, ${probes.blank.cssY}),
          size: [bitmap.width, bitmap.height],
        };
      `);
      const expectInverted = (shown, canvasValue) =>
        shown.every((channel) => Math.abs(channel - (255 - canvasValue)) <= 2);
      check(
        expectInverted(inverted.ink, probes.ink.value),
        `ink inverts: ${JSON.stringify(inverted.ink)} under canvas ${probes.ink.value}`,
      );
      check(
        expectInverted(inverted.blank, probes.blank.value),
        `the blank part of the cell inverts too: ${JSON.stringify(inverted.blank)} under canvas ${probes.blank.value}`,
      );
    }

    // The overlay is DOM, so the page's own pixels cannot move because of it.
    const untouched = await evaluate(`
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const sample = () => {
        const image = context.getImageData(0, 0, canvas.width, canvas.height).data;
        const values = [];
        for (let index = 0; index < image.length; index += 997) values.push(image[index]);
        return values;
      };
      const before = sample();
      window.getSelection().removeAllRanges();
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      const after = sample();
      return { equal: before.length === after.length && before.every((v, i) => v === after[i]), count: before.length };
    `);
    check(untouched.equal, `the canvas raster did not change with the selection: ${untouched.count} samples`);

    // High contrast throws an author's blend away, so the overlay must paint a
    // system colour there instead -- and the emulation has to come off again for
    // every check that follows.
    await send('Emulation.setEmulatedMedia', {
      features: [{ name: 'forced-colors', value: 'active' }],
    });
    try {
      const contrast = await evaluate(`
        const spans = [...document.querySelectorAll('#textLayer span')];
        const word = spans.slice(0, 6);
        const range = document.createRange();
        range.setStartBefore(word[0]);
        range.setEndAfter(word[word.length - 1]);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        document.dispatchEvent(new Event('selectionchange'));
        await new Promise((done) => requestAnimationFrame(done));
        const mark = document.querySelector('#selectionLayer i');
        const probe = document.createElement('i');
        probe.style.background = 'Highlight';
        document.body.append(probe);
        const highlight = getComputedStyle(probe).backgroundColor;
        probe.remove();
        const style = mark ? getComputedStyle(mark) : null;
        return {
          forced: matchMedia('(forced-colors: active)').matches,
          blend: style ? style.mixBlendMode : 'none',
          background: style ? style.backgroundColor : 'none',
          highlight,
        };
      `);
      check(contrast.forced, 'the page reports forced colours active');
      check(
        contrast.blend === 'normal',
        `the overlay drops the blend in high contrast: ${contrast.blend}`,
      );
      check(
        contrast.background === contrast.highlight,
        `it paints the system Highlight instead: ${contrast.background} against ${contrast.highlight}`,
      );
    } finally {
      await send('Emulation.setEmulatedMedia', { features: [] });
    }

    // A drag across the line still reads the line: per-glyph spans must not
    // turn copying into a character at a time.
    const lineCopy = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      const firstLine = [];
      const top = spans[0].getBoundingClientRect().top;
      for (const span of spans) {
        if (Math.abs(span.getBoundingClientRect().top - top) > 2) break;
        firstLine.push(span);
      }
      const range = document.createRange();
      range.setStartBefore(firstLine[0]);
      range.setEndAfter(firstLine[firstLine.length - 1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      return {
        spans: firstLine.length,
        text: selection.toString(),
        info: document.querySelector('#selectionInfo').textContent,
      };
    `);
    check(
      lineCopy.text.startsWith('Hello, PDF 2.0') && lineCopy.info.trim() === lineCopy.text.trim(),
      `a drag across the line still reads it: ${JSON.stringify(lineCopy.text)} over ${lineCopy.spans} spans`,
    );

    // And the same on the embedded serif, where the advances are proportional:
    // a fallback-to-a-browser-font layer drifts there in the opposite direction.
    await evaluate(`
      const select = document.querySelector('#sample');
      const option = [...select.options].find((o) => /book/.test(o.textContent));
      select.value = option.value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'book.pdf'`, 'the book sample');
    await new Promise((done) => setTimeout(done, 300));
    const serifAlignment = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      const word = [];
      for (const span of spans) {
        if (span.textContent === ' ') break;
        if (span.textContent.trim().length > 0) word.push(span);
      }
      const range = document.createRange();
      range.setStartBefore(word[0]);
      range.setEndAfter(word[word.length - 1]);
      const native = window.getSelection();
      native.removeAllRanges();
      native.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      const marks = [...document.querySelectorAll('#selectionLayer i')].map((mark) => mark.getBoundingClientRect());
      const selection = marks.length === 1
        ? marks[0]
        : { left: Math.min(...marks.map((m) => m.left)), right: Math.max(...marks.map((m) => m.right)),
            top: Math.min(...marks.map((m) => m.top)), bottom: Math.max(...marks.map((m) => m.bottom)) };
      const x0 = Math.floor((selection.left - canvasRect.left) * toBitmap);
      const x1 = Math.ceil((selection.right - canvasRect.left) * toBitmap);
      const y0 = Math.max(0, Math.floor((selection.top - canvasRect.top) * toBitmap));
      const y1 = Math.min(canvas.height, Math.ceil((selection.bottom - canvasRect.top) * toBitmap));
      let firstInk = null;
      let lastInk = null;
      for (let x = x0; x < x1; x += 1) {
        let ink = false;
        for (let y = y0; y < y1 && !ink; y += 1) {
          const offset = (y * canvas.width + x) * 4;
          ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
        }
        if (ink) { if (firstInk === null) firstInk = x; lastInk = x; }
      }
      return {
        text: word.map((span) => span.textContent).join(''),
        selection: [selection.left, selection.right],
        ink: firstInk === null ? null : [firstInk / toBitmap + canvasRect.left, (lastInk + 1) / toBitmap + canvasRect.left],
      };
    `);
    check(
      serifAlignment.ink !== null &&
        Math.abs(serifAlignment.ink[0] - serifAlignment.selection[0]) <= 2 &&
        serifAlignment.ink[1] <= serifAlignment.selection[1] + 2,
      `the embedded serif's word is covered too: "${serifAlignment.text}" ink ` +
        `${serifAlignment.ink?.map((v) => v.toFixed(1))} inside selection ` +
        `${serifAlignment.selection.map((v) => v.toFixed(1))}`,
    );
    // Back to the text sample for the checks that follow.
    await evaluate(`
      const select = document.querySelector('#sample');
      const option = [...select.options].find((o) => /Text and accents/.test(o.textContent));
      select.value = option.value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'text.pdf'`, 'the text sample again');
    await new Promise((done) => setTimeout(done, 300));

    const selected = await evaluate(`
      const span = document.querySelector('#textLayer span');
      const range = document.createRange();
      range.selectNodeContents(span);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      return {
        text: span.textContent,
        info: document.querySelector('#selectionInfo').textContent,
        copyDisabled: document.querySelector('#copySelection').disabled,
      };
    `);
    check(
      selected.info.trim() === selected.text.trim() && selected.copyDisabled === false,
      `selecting a line offers it: ${JSON.stringify(selected.info)}`,
    );

    // Copy hands the selection to the clipboard; stub it, since a headless profile
    // has no permission to read one back.
    await evaluate(`
      globalThis.__copied = null;
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: { writeText: async (text) => { globalThis.__copied = text; } },
      });
      document.querySelector('#copySelection').click();
    `);
    await new Promise((done) => setTimeout(done, 100));
    const copied = await evaluate(`return globalThis.__copied;`);
    check(
      typeof copied === 'string' && copied.trim() === selected.text.trim(),
      `the copy button passes the selection on: ${JSON.stringify(copied)}`,
    );

    // The overlay lives exactly as long as the selection: a click elsewhere, a
    // page change and a new document each leave no inverted band on the page.
    const lifetime = await evaluate(`
      const count = () => document.querySelectorAll('#selectionLayer i').length;
      const list = [...document.querySelectorAll('#textLayer span')];
      const range = document.createRange();
      range.setStartBefore(list[0]);
      range.setEndAfter(list[1]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      const selected = count();
      selection.removeAllRanges();
      document.dispatchEvent(new Event('selectionchange'));
      await new Promise((done) => requestAnimationFrame(done));
      return { selected, afterDeselect: count() };
    `);
    check(
      lifetime.selected > 0 && lifetime.afterDeselect === 0,
      `the overlay disappears with the selection: ${lifetime.selected} then ${lifetime.afterDeselect} mark(s)`,
    );

    // A page change rebuilds the layer, so the old highlight must not survive it.
    // `calibration.pdf` has eleven pages and only some carry text -- page 6 does.
    await evaluate(`
      const select = document.querySelector('#sample');
      const option = [...select.options].find((o) => /Calibration/.test(o.textContent));
      select.value = option.value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'calibration.pdf'`, 'the calibration sample');
    for (let page = 2; page <= 6; page += 1) {
      await evaluate(`document.querySelector('#next').click();`);
      await waitFor(
        `document.querySelector('#pageInfo').textContent.startsWith('${page} /')`,
        `the calibration sample to reach page ${page}`,
      );
    }
    await new Promise((done) => setTimeout(done, 300));
    await evaluate(`
      const list = [...document.querySelectorAll('#textLayer span')];
      const range = document.createRange();
      range.setStartBefore(list[0]);
      range.setEndAfter(list[Math.min(3, list.length - 1)]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
    `);
    await new Promise((done) => setTimeout(done, 100));
    const overlayBeforePage = await evaluate(`return document.querySelectorAll('#selectionLayer i').length;`);
    await evaluate(`document.querySelector('#next').click();`);
    await waitFor(
      `document.querySelector('#pageInfo').textContent.startsWith('7 /')`,
      'the calibration sample to page forward',
    );
    await new Promise((done) => setTimeout(done, 300));
    const overlayAfterPage = await evaluate(`return document.querySelectorAll('#selectionLayer i').length;`);
    check(
      overlayBeforePage > 0 && overlayAfterPage === 0,
      `a page change leaves no mark behind: ${overlayBeforePage} then ${overlayAfterPage}`,
    );

    // And neither does loading another document.
    await evaluate(`
      const list = [...document.querySelectorAll('#textLayer span')];
      const range = document.createRange();
      range.setStartBefore(list[0]);
      range.setEndAfter(list[Math.min(3, list.length - 1)]);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
    `);
    await new Promise((done) => setTimeout(done, 100));
    const overlayBeforeDocument = await evaluate(`return document.querySelectorAll('#selectionLayer i').length;`);
    await evaluate(`
      const select = document.querySelector('#sample');
      const option = [...select.options].find((o) => /Text and accents/.test(o.textContent));
      select.value = option.value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'text.pdf'`, 'the text sample again');
    await new Promise((done) => setTimeout(done, 300));
    const overlayAfterDocument = await evaluate(`return document.querySelectorAll('#selectionLayer i').length;`);
    check(
      overlayBeforeDocument > 0 && overlayAfterDocument === 0,
      `a new document leaves no mark behind: ${overlayBeforeDocument} then ${overlayAfterDocument}`,
    );

    // The find box reads the library's search: the counter, one mark per hit, and
    // next moving the active one.
    const needle = layer.first.trim().split(/\s+/)[0];
    const found = await evaluate(`
      const find = document.querySelector('#find');
      find.value = ${JSON.stringify(needle)};
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 200));
      return {
        info: document.querySelector('#findInfo').textContent,
        marks: document.querySelectorAll('#selectionLayer i.find').length,
        active: [...document.querySelectorAll('#selectionLayer i.find')].findIndex((mark) => mark.classList.contains('active')),
      };
    `);
    check(/^\d+ \/ \d+$/.test(found.info), `the find box counts its hits: ${found.info}`);
    const total = Number(found.info.split('/')[1].trim());
    check(total >= 1, `"${needle}" is found at least once: ${found.info}`);
    check(found.marks === 1, `one mark is drawn for the active hit: ${found.marks}`);

    const nextInfo = await evaluate(`
      document.querySelector('#findNext').click();
      await new Promise((done) => setTimeout(done, 200));
      return document.querySelector('#findInfo').textContent;
    `);
    check(
      total === 1 || nextInfo !== found.info,
      `next moves to another hit: ${found.info} then ${nextInfo}`,
    );

    // The mark sits on the word, not on the line the word is part of: a hit's
    // box is its line's (`src/content.rs:1036`), so the mark is built from the
    // painted glyphs instead -- measured against the canvas's own ink here.
    const hitMark = await evaluate(`
      const find = document.querySelector('#find');
      find.value = 'line';
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 300));
      const mark = document.querySelector('#selectionLayer i.find');
      if (!mark) return { mark: null };
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      const rect = mark.getBoundingClientRect();
      const x0 = Math.floor((rect.left - canvasRect.left) * toBitmap);
      const x1 = Math.ceil((rect.right - canvasRect.left) * toBitmap);
      const y0 = Math.max(0, Math.floor((rect.top - canvasRect.top) * toBitmap));
      const y1 = Math.min(canvas.height, Math.ceil((rect.bottom - canvasRect.top) * toBitmap));
      let firstInk = null;
      let lastInk = null;
      for (let x = x0; x < x1; x += 1) {
        let ink = false;
        for (let y = y0; y < y1 && !ink; y += 1) {
          const offset = (y * canvas.width + x) * 4;
          ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
        }
        if (ink) { if (firstInk === null) firstInk = x; lastInk = x; }
      }
      return {
        info: document.querySelector('#findInfo').textContent,
        mark: [rect.left, rect.right],
        ink: firstInk === null ? null : [firstInk / toBitmap + canvasRect.left, (lastInk + 1) / toBitmap + canvasRect.left],
      };
    `);
    check(hitMark.mark !== null, `"line" draws one mark: ${hitMark.info}`);
    check(hitMark.ink !== null, 'the word "line" is painted under its mark');
    if (hitMark.mark && hitMark.ink) {
      const leftGap = Math.abs(hitMark.ink[0] - hitMark.mark[0]);
      const overflow = Math.max(hitMark.mark[0] - hitMark.ink[0], hitMark.ink[1] - hitMark.mark[1], 0);
      check(
        leftGap <= 2 && overflow <= 2,
        `the mark is on the word, not the line: ink ${hitMark.ink.map((v) => v.toFixed(1))} ` +
          `inside mark ${hitMark.mark.map((v) => v.toFixed(1))} (left gap ${leftGap.toFixed(1)}, overflow ${overflow.toFixed(1)})`,
      );
    }

    // The two painters share one overlay and must not erase each other: a
    // selection change cannot wipe the find mark, nor a find the selection.
    const coexist = await evaluate(`
      const count = (selector) => document.querySelectorAll(selector).length;
      const spans = [...document.querySelectorAll('#textLayer span')];
      const select = () => {
        const range = document.createRange();
        range.setStartBefore(spans[0]);
        range.setEndAfter(spans[0]);
        const selection = window.getSelection();
        selection.removeAllRanges();
        selection.addRange(range);
        document.dispatchEvent(new Event('selectionchange'));
      };
      select();
      await new Promise((done) => requestAnimationFrame(done));
      const selectionAlone = count('#selectionLayer i.selection');
      const findBefore = count('#selectionLayer i.find');
      const find = document.querySelector('#find');
      find.value = 'line';
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 300));
      const selectionAfterFind = count('#selectionLayer i.selection');
      select();
      await new Promise((done) => requestAnimationFrame(done));
      return {
        selectionAlone,
        findBefore,
        selectionAfterFind,
        findAfterSelect: count('#selectionLayer i.find'),
      };
    `);
    check(
      coexist.selectionAlone > 0 && coexist.selectionAfterFind === coexist.selectionAlone,
      `a find does not wipe the selection: ${coexist.selectionAlone} then ${coexist.selectionAfterFind} mark(s)`,
    );
    check(
      coexist.findAfterSelect === 1,
      `a selection change does not wipe the find mark: ${coexist.findAfterSelect}`,
    );

    // Two hits, two words: stepping must move the mark to the second "line" and
    // not paint the first one again (`search` returns hits in reading order, so
    // occurrences are consumed in that order).
    const hitInk = async () =>
      evaluate(`
        const mark = document.querySelector('#selectionLayer i.find');
        if (!mark) return { mark: null };
        const canvas = document.querySelector('#canvas');
        const context = canvas.getContext('2d');
        const canvasRect = canvas.getBoundingClientRect();
        const toBitmap = canvas.width / canvasRect.width;
        const image = context.getImageData(0, 0, canvas.width, canvas.height);
        const rect = mark.getBoundingClientRect();
        const x0 = Math.floor((rect.left - canvasRect.left) * toBitmap);
        const x1 = Math.ceil((rect.right - canvasRect.left) * toBitmap);
        const y0 = Math.max(0, Math.floor((rect.top - canvasRect.top) * toBitmap));
        const y1 = Math.min(canvas.height, Math.ceil((rect.bottom - canvasRect.top) * toBitmap));
        let firstInk = null;
        let lastInk = null;
        for (let x = x0; x < x1; x += 1) {
          let ink = false;
          for (let y = y0; y < y1 && !ink; y += 1) {
            const offset = (y * canvas.width + x) * 4;
            ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
          }
          if (ink) { if (firstInk === null) firstInk = x; lastInk = x; }
        }
        return {
          info: document.querySelector('#findInfo').textContent,
          mark: [rect.left, rect.right, rect.top],
          ink: firstInk === null ? null : [firstInk / toBitmap + canvasRect.left, (lastInk + 1) / toBitmap + canvasRect.left],
          fallback: document.querySelectorAll('#selectionLayer i.find.fallback').length,
        };
      `);
    const firstHit = await hitInk();
    await evaluate(`
      document.querySelector('#findNext').click();
      await new Promise((done) => setTimeout(done, 300));
    `);
    const secondHit = await hitInk();

    // Both "line" words start at the same x, so the *vertical* move is what tells
    // the two occurrences apart.
    check(
      firstHit.mark && secondHit.mark &&
        (Math.abs(firstHit.mark[0] - secondHit.mark[0]) > 1 ||
          Math.abs(firstHit.mark[2] - secondHit.mark[2]) > 1),
      `stepping moves the mark to the next occurrence: ` +
        `${firstHit.mark?.slice(0, 2).map((v) => v.toFixed(1))} at ${firstHit.mark?.[2]?.toFixed(1)} then ` +
        `${secondHit.mark?.slice(0, 2).map((v) => v.toFixed(1))} at ${secondHit.mark?.[2]?.toFixed(1)}`,
    );
    check(
      secondHit.ink !== null &&
        Math.abs(secondHit.ink[0] - secondHit.mark[0]) <= 2 &&
        secondHit.ink[1] <= secondHit.mark[1] + 2,
      `the second hit's mark is on its own word too: ink ${secondHit.ink?.map((v) => v.toFixed(1))} ` +
        `inside ${secondHit.mark?.slice(0, 2).map((v) => v.toFixed(1))}`,
    );
    check(
      firstHit.fallback === 0 && secondHit.fallback === 0,
      `no mark fell back to the line box: ${firstHit.fallback} and ${secondHit.fallback}`,
    );

    // The same on the embedded serif, where the advances are proportional, and
    // with a zoom: the mark's scale is derived at paint time, so a zoom must keep
    // the counter and the word.
    await evaluate(`
      const select = document.querySelector('#sample');
      select.value = [...select.options].find((o) => /book/.test(o.textContent)).value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(`document.querySelector('#factName').textContent === 'book.pdf'`, 'the book sample');
    await new Promise((done) => setTimeout(done, 300));
    const bookHit = await evaluate(`
      const find = document.querySelector('#find');
      find.value = 'the';
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 400));
      const mark = document.querySelector('#selectionLayer i.find');
      const info = document.querySelector('#findInfo').textContent;
      if (!mark) return { mark: null, info, fallback: 0 };
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      const rect = mark.getBoundingClientRect();
      const x0 = Math.floor((rect.left - canvasRect.left) * toBitmap);
      const x1 = Math.ceil((rect.right - canvasRect.left) * toBitmap);
      const y0 = Math.max(0, Math.floor((rect.top - canvasRect.top) * toBitmap));
      const y1 = Math.min(canvas.height, Math.ceil((rect.bottom - canvasRect.top) * toBitmap));
      let firstInk = null;
      let lastInk = null;
      for (let x = x0; x < x1; x += 1) {
        let ink = false;
        for (let y = y0; y < y1 && !ink; y += 1) {
          const offset = (y * canvas.width + x) * 4;
          ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
        }
        if (ink) { if (firstInk === null) firstInk = x; lastInk = x; }
      }
      return {
        mark: [rect.left, rect.right],
        ink: firstInk === null ? null : [firstInk / toBitmap + canvasRect.left, (lastInk + 1) / toBitmap + canvasRect.left],
        info,
        fallback: document.querySelectorAll('#selectionLayer i.find.fallback').length,
      };
    `);
    check(
      bookHit.mark !== null && bookHit.ink !== null &&
        bookHit.mark[0] - bookHit.ink[0] <= 2 && bookHit.ink[1] - bookHit.mark[1] <= 2,
      `the serif's hit is on its word: ink ${bookHit.ink?.map((v) => v.toFixed(1))} inside ` +
        `${bookHit.mark?.map((v) => v.toFixed(1))} (${bookHit.info})`,
    );
    check(bookHit.fallback === 0, `the serif's hit did not fall back: ${bookHit.fallback}`);

    // A zoom repaints at the new scale: the counter stays and the mark stays on
    // its word -- the scale is derived at paint time, not remembered.
    await evaluate(`document.querySelector('#zoomIn').click();`);
    await new Promise((done) => setTimeout(done, 500));
    const zoomed = await evaluate(`
      const mark = document.querySelector('#selectionLayer i.find');
      const canvas = document.querySelector('#canvas');
      const context = canvas.getContext('2d');
      const canvasRect = canvas.getBoundingClientRect();
      const toBitmap = canvas.width / canvasRect.width;
      const image = context.getImageData(0, 0, canvas.width, canvas.height);
      const rect = mark ? mark.getBoundingClientRect() : null;
      if (!rect) return { mark: null, info: document.querySelector('#findInfo').textContent };
      const x0 = Math.floor((rect.left - canvasRect.left) * toBitmap);
      const x1 = Math.ceil((rect.right - canvasRect.left) * toBitmap);
      const y0 = Math.max(0, Math.floor((rect.top - canvasRect.top) * toBitmap));
      const y1 = Math.min(canvas.height, Math.ceil((rect.bottom - canvasRect.top) * toBitmap));
      let inked = 0;
      for (let x = x0; x < x1; x += 1) {
        let ink = false;
        for (let y = y0; y < y1 && !ink; y += 1) {
          const offset = (y * canvas.width + x) * 4;
          ink = image.data[offset] + image.data[offset + 1] + image.data[offset + 2] < 600;
        }
        if (ink) inked += 1;
      }
      return { mark: [rect.left, rect.right, rect.width], inked, info: document.querySelector('#findInfo').textContent };
    `);
    check(
      zoomed.mark !== null && zoomed.info === bookHit.info,
      `a zoom keeps the counter: ${bookHit.info} then ${zoomed.info}`,
    );
    check(
      zoomed.mark !== null && zoomed.mark[2] > 20 && zoomed.inked > 0,
      `the mark follows the zoom: ${zoomed.mark?.map((v) => v.toFixed(1))} with ${zoomed.inked} inked column(s) inside`,
    );

    // An empty box clears the counter rather than leaving the last result behind.
    const cleared = await evaluate(`
      const find = document.querySelector('#find');
      find.value = '';
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 100));
      return {
        info: document.querySelector('#findInfo').textContent,
        marks: document.querySelectorAll('#selectionLayer i.find').length,
      };
    `);
    check(cleared.info === '–' && cleared.marks === 0, `an empty needle clears: ${cleared.info}`);

    // Back to the sample the checks below expect, since this block borrowed the
    // text one for its spans.
    await evaluate(`
      const select = document.querySelector('#sample');
      select.value = [...select.options].find((option) => /Calibration/.test(option.textContent)).value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#factName').textContent === 'calibration.pdf'`,
      'the calibration sample back',
    );
    await new Promise((done) => setTimeout(done, 300));

    // Download hands over the document that is on screen.
    const download = await evaluate(`
      window.__downloads = [];
      const original = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = function () {
        window.__downloads.push({ download: this.download, href: this.href });
      };
      document.querySelector('#download').click();
      HTMLAnchorElement.prototype.click = original;
      return window.__downloads[0];
    `);
    check(
      download?.download === 'calibration.pdf' && String(download?.href).startsWith('blob:'),
      'Download offers the document on screen under its own name',
      JSON.stringify(download),
    );

    // A page the renderer cannot draw is reported: it is blank, and the panel
    // and status say which font was skipped instead of leaving a white page.
    const skipped = await evaluate(`
      const canvas = document.querySelector('#canvas');
      while (Number(document.querySelector('#pageInfo').textContent.split('/')[0].trim()) > 8) {
        document.querySelector('#prev').click();
        await new Promise((done) => setTimeout(done, 120));
      }
      while (Number(document.querySelector('#pageInfo').textContent.split('/')[0].trim()) < 8) {
        document.querySelector('#next').click();
        await new Promise((done) => setTimeout(done, 120));
      }
      await new Promise((done) => requestAnimationFrame(done));
      return {
        page: document.querySelector('#pageInfo').textContent,
        skipped: document.querySelector('#factSkipped').textContent,
        status: document.querySelector('#status').textContent,
      };
    `);
    check(
      skipped.page.startsWith('8 /') && /Type0/.test(skipped.skipped),
      'a page with an unsupported font says which font it skipped',
      JSON.stringify(skipped),
    );
    check(
      /blank where it used/i.test(skipped.status),
      'the status explains why such a page is blank',
      skipped.status.slice(0, 140),
    );

    // A page whose text cannot be extracted still renders: the status must not
    // claim the render failed, and the text panel must say why it is empty.
    const extraction = await evaluate(`
      // Page 10 draws (composite font) but has no extractable text.
      while (Number(document.querySelector('#pageInfo').textContent.split('/')[0].trim()) < 10) {
        document.querySelector('#next').click();
        await new Promise((done) => setTimeout(done, 150));
      }
      await new Promise((done) => requestAnimationFrame(done));
      return {
        status: document.querySelector('#status').textContent,
        failed: document.querySelector('#status').classList.contains('error'),
        text: document.querySelector('#text').textContent,
        page: document.querySelector('#pageInfo').textContent,
        ink: (() => {
          const canvas = document.querySelector('#canvas');
          const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
          let count = 0;
          for (let i = 0; i < data.length; i += 4) {
            if (data[i] !== 255 || data[i + 1] !== 255 || data[i + 2] !== 255) count += 1;
          }
          return count;
        })(),
      };
    `);
    check(
      !extraction.failed && extraction.page.startsWith('10 /') && extraction.ink > 100,
      'a page with no extractable text still renders, without an error status',
      JSON.stringify(extraction),
    );
    check(
      /unsupported|unavailable/i.test(extraction.text),
      'the text panel explains why a page has no extractable text',
      extraction.text.slice(0, 120),
    );

    // Keyboard paging: one page forward from wherever we are.
    const beforePage = await evaluate(
      `return Number(document.querySelector('#pageInfo').textContent.split('/')[0].trim());`,
    );
    await evaluate(`
      document.body.focus();
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    `);
    await waitFor(
      `Number(document.querySelector('#pageInfo').textContent.split('/')[0].trim()) === ${beforePage + 1}`,
      'the arrow key to change pages',
    );
    check(true, 'ArrowRight pages forward');

    // A dropped file replaces the sample.
    await evaluate(`
      const bytes = await (await fetch('samples/paths.pdf')).arrayBuffer();
      const file = new File([bytes], 'dropped.pdf', { type: 'application/pdf' });
      const data = new DataTransfer();
      data.items.add(file);
      window.dispatchEvent(new DragEvent('drop', {
        dataTransfer: data,
        bubbles: true,
        cancelable: true,
      }));
    `);
    await waitFor(
      `document.querySelector('#factName').textContent === 'dropped.pdf'`,
      'a dropped file to replace the sample',
    );
    check(true, 'a dropped file replaces the document on screen');

    // The encrypted file asks for its password and then opens.
    await evaluate(`
      const select = document.querySelector('#sample');
      // The option's value is its SAMPLES index; its position in the DOM is not,
      // because a placeholder option sits in front of the list.
      select.value = [...select.options].find((option) => /encrypted/.test(option.textContent)).value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#passwordRow').classList.contains('visible')`,
      'the password field for the encrypted sample',
    );
    check(true, 'the encrypted sample asks for a password');
    await evaluate(`
      const input = document.querySelector('#password');
      input.value = 'user';
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#factEncrypted').textContent === 'yes' &&
       document.querySelector('#pageInfo').textContent === '1 / 1'`,
      'the encrypted sample to open with its password',
    );
    check(true, 'the password opens the encrypted sample');


    // --- 1.2's samples -------------------------------------------------
    //
    // Each one exists to make a claim of the release visible in a browser:
    // CFF outlines, an older encryption generation, the visible page box, and
    // text that only exists through a predefined CMap.

    const pick = async (pattern) => {
      await evaluate(`
        const select = document.querySelector('#sample');
        select.value = [...select.options].find((option) => ${pattern}.test(option.textContent)).value;
        select.dispatchEvent(new Event('change', { bubbles: true }));
      `);
    };
    const ink = async () =>
      evaluate(`
        const canvas = document.querySelector('#canvas');
        const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
        let count = 0;
        for (let i = 0; i < data.length; i += 4) {
          if (data[i] !== 255 || data[i + 1] !== 255 || data[i + 2] !== 255) count += 1;
        }
        return count;
      `);

    // CFF: a CID-keyed program with no TrueType table, drawn from its charstrings.
    await pick('/CFF outlines/');
    await waitFor(`document.querySelector('#factName').textContent === 'cff.pdf'`, 'the CFF sample');
    await new Promise((done) => setTimeout(done, 500));
    check((await ink()) > 500, 'the CFF sample draws its outlines');
    check(
      !(await evaluate(`return document.querySelector('#status').textContent;`)).includes('font:'),
      'the CFF sample reports no missing font',
    );

    // Revision 4 encryption: the same password field, an older handler.
    await pick('/revision 4/');
    await waitFor(
      `document.querySelector('#passwordRow').classList.contains('visible')`,
      'the password field for the revision-4 sample',
    );
    await evaluate(`
      const input = document.querySelector('#password');
      input.value = 'user';
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    `);
    await waitFor(
      `document.querySelector('#factEncrypted').textContent === 'yes' &&
       document.querySelector('#text').textContent.includes('Encrypted hello')`,
      'the revision-4 sample to open with its password',
    );
    check(true, 'a revision-4 encrypted sample opens and extracts its text');

    // /CropBox and /UserUnit: the raster is the crop box at twice its size, so the
    // canvas is square — the media box would make it 3:2.
    await pick('/CropBox/');
    await waitFor(`document.querySelector('#factName').textContent === 'cropbox.pdf'`, 'the crop sample');
    await new Promise((done) => setTimeout(done, 400));
    const shape = await evaluate(`
      const canvas = document.querySelector('#canvas');
      return { width: canvas.width, height: canvas.height };
    `);
    check(
      Math.abs(shape.width - shape.height) <= 2,
      `the crop box decides the shape: ${shape.width}x${shape.height}`,
    );

    // CJK: the page has no /ToUnicode and no embedded program, so the text can
    // only come from the predefined CMap the crate ships as data.
    await pick('/Shift-JIS/');
    await waitFor(`document.querySelector('#factName').textContent === 'cjk.pdf'`, 'the CJK sample');
    await new Promise((done) => setTimeout(done, 400));
    const cjk = await evaluate(`return document.querySelector('#text').textContent;`);
    check(cjk.includes('日本語テスト'), `the CJK sample extracts its text: ${cjk.slice(0, 40)}`);

    // --- 1.3's samples -------------------------------------------------
    //
    // The painting family: a gradient drawn by a `/Shading` through its
    // `/Function`, the sixteen blend modes, and a Type 1 program. Each one is a
    // page of colour, so a blank canvas is the failure the check has to catch.

    await pick('/Gradients/');
    await waitFor(`document.querySelector('#factName').textContent === 'shading.pdf'`, 'the shading sample');
    await new Promise((done) => setTimeout(done, 400));
    const shadingInk = await ink();
    check(shadingInk > 20_000, `the shading sample fills its page: ${shadingInk} pixels`);
    const shadingStatus = await evaluate(`return document.querySelector('#status').textContent;`);
    check(
      !/sh×|unsupported/i.test(shadingStatus),
      `the shading sample reports no skipped \`sh\`: ${shadingStatus}`,
    );

    await pick('/blend modes/');
    await waitFor(
      `document.querySelector('#factName').textContent === 'blend-modes.pdf'`,
      'the blend-modes sample',
    );
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 10_000, 'the blend-modes sample draws its strips');

    await pick('/Type 1/');
    await waitFor(`document.querySelector('#factName').textContent === 'type1.pdf'`, 'the Type 1 sample');
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 3_000, 'the Type 1 sample draws its glyphs');

    // 1.4's samples: the tile is painted rather than reported, the key hides its
    // samples, and the Type 3 page says `text-clip` while still drawing the rest.
    await pick('/tiling pattern/');
    await waitFor(
      `document.querySelector('#factName').textContent === 'colorkeymask.pdf'`,
      'the tiling sample',
    );
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 3_000, 'the tiling sample paints its cells');
    const tilingStatus = await evaluate(`return document.querySelector('#status').textContent;`);
    check(
      !/unsupported|pattern×/i.test(tilingStatus),
      `the tiling sample reports no skipped pattern: ${tilingStatus}`,
    );

    await pick('/\\/Mask/');
    await waitFor(
      `document.querySelector('#factName').textContent === 'colour-key.pdf'`,
      'the colour-key sample',
    );
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 1_000, 'the colour-key sample draws its keyed image');

    await pick('/Clipped text/');
    await waitFor(`document.querySelector('#factName').textContent === 'tr-clip.pdf'`, 'the Tr sample');
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 3_000, 'the Tr sample clips its text');

    await pick('/JPEG 2000/');
    await waitFor(`document.querySelector('#factName').textContent === 'jpx.pdf'`, 'the JPX sample');
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 5_000, 'the JPX sample decodes and paints its codestream');
    const jpxStatus = await evaluate(`return document.querySelector('#status').textContent;`);
    check(!/Do×/.test(jpxStatus), `the JPX sample reports no skipped image: ${jpxStatus}`);

    await pick('/JBIG2/');
    await waitFor(
      `document.querySelector('#factName').textContent === 'jbig2-scan.pdf'`,
      'the JBIG2 sample',
    );
    await new Promise((done) => setTimeout(done, 400));
    check((await ink()) > 5_000, 'the JBIG2 sample decodes and paints its scan');
    const jbig2Status = await evaluate(`return document.querySelector('#status').textContent;`);
    check(!/Do×/.test(jbig2Status), `the JBIG2 sample reports no skipped image: ${jbig2Status}`);

    await pick('/Type 3 glyph under/');
    await waitFor(
      `document.querySelector('#factName').textContent === 'tr-clip-type3.pdf'`,
      'the Type 3 clip sample',
    );
    await new Promise((done) => setTimeout(done, 400));
    const type3ClipStatus = await evaluate(`return document.querySelector('#status').textContent;`);
    check(
      /text-clip/.test(type3ClipStatus),
      `the Type 3 clip sample names what it cannot do: ${type3ClipStatus}`,
    );
    check((await ink()) > 3_000, 'and still draws the rest of the page');

    // Nothing above should have thrown or logged an error.
    const errors = [];
    for (const event of cdp.events) {
      if (event.method === 'Runtime.exceptionThrown') {
        errors.push(event.params?.exceptionDetails?.exception?.description ?? 'exception');
      }
      if (event.method === 'Log.entryAdded' && event.params?.entry?.level === 'error') {
        errors.push(event.params.entry.text);
      }
      if (
        event.method === 'Runtime.consoleAPICalled' &&
        event.params?.type === 'error'
      ) {
        errors.push(event.params.args?.map((arg) => arg.value ?? arg.description).join(' '));
      }
    }
    check(errors.length === 0, 'the page logs no errors', errors.slice(0, 3).join(' | '));
  } finally {
    clearTimeout(watchdog);
    cdp.close();
    chrome.kill('SIGKILL');
    // Chrome's child processes outlive the parent for a moment and keep writing
    // into the profile directory, so removing it straight away fails with
    // ENOTEMPTY on a busy runner — which used to mark a passing run as failed.
    await new Promise((done) => {
      if (chrome.exitCode !== null) return done();
      chrome.once('exit', done);
      setTimeout(done, 2000);
    });
    server?.close();
    await removeProfile(profile);
  }

  if (failures.length > 0) {
    console.error(`\n${failures.length} demo interaction check(s) failed:`);
    for (const failure of failures) {
      console.error(` - ${failure}`);
    }
    process.exitCode = 1;
  } else {
    console.log('\ndemo: all interaction checks passed');
  }
}

/// Remove the throwaway browser profile, tolerating a directory a child process
/// is still emptying. A temp directory left behind is the operating system's
/// problem, not the run's: the checks are what the exit code reports.
async function removeProfile(profile) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    try {
      await rm(profile, { recursive: true, force: true });
      return;
    } catch (error) {
      if (!['ENOTEMPTY', 'EBUSY', 'ENOENT'].includes(error.code)) throw error;
      await new Promise((done) => setTimeout(done, 100 * (attempt + 1)));
    }
  }
  console.warn(`warning: could not remove ${profile}; leaving it to the system`);
}

main().catch((error) => {
  console.error(`demo test failed: ${error.message}`);
  process.exitCode = 1;
});
