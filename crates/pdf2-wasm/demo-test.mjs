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
  for (let attempt = 0; attempt < 100 && port === null; attempt += 1) {
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

    // Zoom: the page must scroll in both directions, and its left edge must be
    // reachable — the bug was that centring in a scroll container hid it.
    for (let step = 0; step < 4; step += 1) {
      await evaluate(`document.querySelector('#zoomIn').click();`);
    }
    await waitFor(
      `document.querySelector('#canvasWrap').scrollWidth > document.querySelector('#canvasWrap').clientWidth`,
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
      select.value = '4';
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
    cdp.close();
    chrome.kill('SIGKILL');
    server?.close();
    await rm(profile, { recursive: true, force: true });
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

main().catch((error) => {
  console.error(`demo test failed: ${error.message}`);
  process.exitCode = 1;
});
