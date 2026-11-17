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

    // The glyphs have to fill the line they were painted in. The browser lays the
    // invisible text out with its own advances, so the app scales each span to the
    // library's line box; without that the browser hit-tests characters at advances
    // the page never had, and a drag highlights a word somewhere else entirely.
    const lines = await evaluate(`
      const spans = [...document.querySelectorAll('#textLayer span')];
      let worst = { delta: 0, text: '' };
      let withText = 0;
      let scaled = 0;
      for (const span of spans) {
        if (span.textContent.trim().length === 0) {
          // A padded line has no glyphs to place; there is nothing to scale.
          continue;
        }
        withText += 1;
        const box = Number(span.dataset.box);
        const rect = span.getBoundingClientRect();
        const delta = Math.abs(rect.width - box);
        if (delta > worst.delta) {
          worst = { delta, text: span.textContent.slice(0, 24), box, width: rect.width };
        }
        if (getComputedStyle(span).transform !== 'none') {
          scaled += 1;
        }
      }
      const wide = spans.reduce(
        (best, span) => (span.getBoundingClientRect().width > best.getBoundingClientRect().width ? span : best),
        spans[0],
      );
      const rect = wide.getBoundingClientRect();
      const node = wide.firstChild;
      const text = wide.textContent;
      let last = text.length - 1;
      while (last > 0 && text[last] === ' ') {
        last -= 1;
      }
      const glyph = document.createRange();
      glyph.setStart(node, last);
      glyph.setEnd(node, last + 1);
      const lastGlyphRight = glyph.getBoundingClientRect().right;
      const applied = wide.style.transform;
      wide.style.transform = 'none';
      const unscaledLastGlyphRight = glyph.getBoundingClientRect().right;
      wide.style.transform = applied;
      const caretAt = (fraction) => {
        const x = rect.left + rect.width * fraction;
        const y = rect.top + rect.height / 2;
        let position = document.caretPositionFromPoint ? document.caretPositionFromPoint(x, y) : null;
        if (!position && document.caretRangeFromPoint) {
          const fallback = document.caretRangeFromPoint(x, y);
          if (fallback) {
            position = { offsetNode: fallback.startContainer, offset: fallback.startOffset };
          }
        }
        if (!position) {
          return null;
        }
        return {
          inside: wide.contains(position.offsetNode),
          offset: position.offset,
          length: text.length,
        };
      };
      return {
        count: withText,
        scaled,
        worst,
        widest: { text: text.slice(0, 40), length: text.length, right: rect.right },
        lastGlyphRight,
        unscaledLastGlyphRight,
        quarter: caretAt(0.25),
        threeQuarters: caretAt(0.8),
      };
    `);
    check(
      lines.count > 0 && lines.scaled === lines.count,
      `every one of ${lines.count} text spans is scaled to its line box: ${lines.scaled} scaled`,
    );
    check(
      lines.worst.delta <= 1,
      `each span's rendered width is its line: worst ${lines.worst.delta.toFixed(2)}px on ${JSON.stringify(lines.worst.text)}`,
    );
    const glyphDelta = Math.abs(lines.lastGlyphRight - lines.widest.right);
    check(
      glyphDelta <= 2,
      `the last glyph ends where the line ends: ${glyphDelta.toFixed(2)}px off on ${JSON.stringify(lines.widest.text)}`,
    );
    check(
      lines.quarter !== null && lines.quarter.inside,
      `the caret finds the span's text at a quarter of the line: ${JSON.stringify(lines.quarter)}`,
    );
    check(
      lines.quarter !== null &&
        lines.quarter.offset > 0 &&
        lines.quarter.offset < lines.widest.length,
      `a quarter along the line is a character inside it: offset ${lines.quarter && lines.quarter.offset} of ${lines.widest.length}`,
    );
    check(
      lines.threeQuarters !== null && lines.quarter !== null &&
        lines.threeQuarters.offset > lines.quarter.offset,
      `further along the line is further into the text: ${lines.threeQuarters && lines.threeQuarters.offset} after ${lines.quarter && lines.quarter.offset}`,
    );
    console.log(
      `note  unscaled, the same span's last glyph sat at ${lines.unscaledLastGlyphRight.toFixed(1)}px ` +
        `against a line ending at ${lines.widest.right.toFixed(1)}px — the drift the scale removes`,
    );

    // Selecting one word of a line reads that word, not the line: the character
    // offsets a drag produces are the ones the layer now lays out.
    const oneWord = await evaluate(`
      const span = [...document.querySelectorAll('#textLayer span')].find(
        (candidate) => candidate.textContent.trim().indexOf(' ') > 0,
      );
      const text = span.textContent;
      const start = text.indexOf(' ');
      let wordStart = 0;
      while (wordStart < start && text[wordStart] === ' ') {
        wordStart += 1;
      }
      let wordEnd = wordStart;
      while (wordEnd < text.length && text[wordEnd] !== ' ') {
        wordEnd += 1;
      }
      const range = document.createRange();
      range.setStart(span.firstChild, wordStart);
      range.setEnd(span.firstChild, wordEnd);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.dispatchEvent(new Event('selectionchange'));
      return { word: text.slice(wordStart, wordEnd), info: document.querySelector('#selectionInfo').textContent };
    `);
    check(
      oneWord.word.length > 0 && oneWord.info.trim() === oneWord.word,
      `a range over one word reads that word: ${JSON.stringify(oneWord.info)} against ${JSON.stringify(oneWord.word)}`,
    );

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
        marks: document.querySelectorAll('#textLayer .find-mark').length,
        active: [...document.querySelectorAll('#textLayer .find-mark')].findIndex((mark) => mark.classList.contains('active')),
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

    // An empty box clears the counter rather than leaving the last result behind.
    const cleared = await evaluate(`
      const find = document.querySelector('#find');
      find.value = '';
      find.dispatchEvent(new Event('input', { bubbles: true }));
      await new Promise((done) => setTimeout(done, 100));
      return {
        info: document.querySelector('#findInfo').textContent,
        marks: document.querySelectorAll('#textLayer .find-mark').length,
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
