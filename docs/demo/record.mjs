/**
 * Records terminal.html into docs/demo/svipall.gif.
 *
 * Frame-stepped, not screen-captured: for every frame it calls
 * `__demo.render(t)` and then takes one screenshot, so the recording does not
 * depend on how fast this machine happens to be and two runs produce the same
 * bytes. Nothing is timed, dropped or interpolated.
 *
 * No npm dependency and no ffmpeg. The one binary it needs is the Chrome
 * Playwright already unpacked, found under ms-playwright/chromium-*; gif.mjs
 * writes the file itself, and says there why.
 *
 * Usage:  node record.mjs [--fps 20] [--out svipall.gif] [--colors 128]
 *         node record.mjs --stills          one frame per scene, then stop
 */

import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { readFile, mkdir, rm, writeFile, readdir, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { homedir, tmpdir } from 'node:os';

import { decodePng, halve, buildPalette, mapper, Gif } from './gif.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));

const arg = (name, fallback) => {
  const i = process.argv.indexOf(`--${name}`);
  return i === -1 ? fallback : process.argv[i + 1];
};

const FPS = Number(arg('fps', 20));
const COLORS = Number(arg('colors', 128));
const OUT = join(HERE, arg('out', 'svipall.gif'));
const FRAMES = join(HERE, '.frames');

/* ---- the browser Playwright unpacked ------------------------------------- */

/** Playwright keeps its browsers in one place per platform. */
const browserRoots = () => [
  process.env.PLAYWRIGHT_BROWSERS_PATH,
  join(homedir(), 'AppData', 'Local', 'ms-playwright'),
  join(homedir(), '.cache', 'ms-playwright'),
  join(homedir(), 'Library', 'Caches', 'ms-playwright'),
].filter(Boolean);

/** The newest directory matching `prefix`, and inside it the first hit for one
 *  of `names`. Returns null rather than guessing at a path that is not there. */
const findUnpacked = async (prefix, names) => {
  for (const root of browserRoots()) {
    if (!existsSync(root)) continue;
    const dirs = (await readdir(root))
      .filter((d) => d.startsWith(prefix))
      .sort()
      .reverse();
    for (const d of dirs) {
      for (const name of names) {
        const p = join(root, d, name);
        if (existsSync(p)) return p;
      }
    }
  }
  return null;
};

const chromePath = async () =>
  process.env.SVIPALL_DEMO_CHROME ||
  (await findUnpacked('chromium-', [
    join('chrome-win64', 'chrome.exe'),
    join('chrome-win', 'chrome.exe'),
    join('chrome-linux', 'chrome'),
    join('chrome-mac', 'Chromium.app', 'Contents', 'MacOS', 'Chromium'),
  ]));

/* ---- a static server, so the page is a real origin and can fetch ---------- */

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.woff2': 'font/woff2',
};

const serve = () =>
  new Promise((resolve) => {
    const server = createServer(async (req, res) => {
      const path = join(HERE, decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, ''));
      try {
        const body = await readFile(path);
        res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' });
        res.end(body);
      } catch {
        res.writeHead(404).end('not found');
      }
    });
    server.listen(0, '127.0.0.1', () => resolve(server));
  });

/* ---- CDP ----------------------------------------------------------------- */

const connect = async (wsUrl) => {
  const ws = new WebSocket(wsUrl);
  await new Promise((ok, bad) => {
    ws.addEventListener('open', ok, { once: true });
    ws.addEventListener('error', bad, { once: true });
  });

  let next = 0;
  const pending = new Map();

  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data);
    const slot = pending.get(msg.id);
    if (!slot) return;
    pending.delete(msg.id);
    if (msg.error) slot.bad(new Error(`${msg.error.message} (${JSON.stringify(msg.error.data ?? null)})`));
    else slot.ok(msg.result);
  });

  const send = (method, params = {}, sessionId) =>
    new Promise((ok, bad) => {
      const id = (next += 1);
      pending.set(id, { ok, bad });
      ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });

  return { send, close: () => ws.close() };
};

const waitForEndpoint = async (port) => {
  const deadline = Date.now() + 20_000;
  for (;;) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (r.ok) return (await r.json()).webSocketDebuggerUrl;
    } catch {
      /* not up yet */
    }
    if (Date.now() > deadline) throw new Error('Chrome never opened its debugging port');
    await new Promise((ok) => setTimeout(ok, 120));
  }
};

/* ---- record -------------------------------------------------------------- */

const chrome = await chromePath();
if (!chrome) {
  console.error('No Chrome found. Run `npx playwright install chromium`, or set SVIPALL_DEMO_CHROME.');
  process.exit(1);
}

const server = await serve();
const { port } = server.address();
const profile = join(tmpdir(), `svipall-demo-${process.pid}`);
const debugPort = 9333 + (process.pid % 500);

const proc = spawn(chrome, [
  '--headless=new',
  `--remote-debugging-port=${debugPort}`,
  `--user-data-dir=${profile}`,
  '--no-first-run',
  '--no-default-browser-check',
  '--hide-scrollbars',
  '--force-color-profile=srgb',
  '--disable-lcd-text',            /* greyscale AA quantises far better */
  '--font-render-hinting=none',
  '--disable-gpu',
  'about:blank',
], { stdio: 'ignore' });

let frames = 0;
try {
  const cdp = await connect(await waitForEndpoint(debugPort));
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (method, params) => cdp.send(method, params, sessionId);

  await s('Page.enable');
  await s('Runtime.enable');
  await s('Emulation.setDeviceMetricsOverride', {
    width: 960,
    height: 450,
    deviceScaleFactor: 2,
    mobile: false,
  });

  await s('Page.navigate', { url: `http://127.0.0.1:${port}/terminal.html` });

  const deadline = Date.now() + 30_000;
  for (;;) {
    const { result } = await s('Runtime.evaluate', {
      expression: 'document.documentElement.dataset.ready === "1"',
    });
    if (result.value === true) break;
    if (Date.now() > deadline) throw new Error('terminal.html never became ready');
    await new Promise((ok) => setTimeout(ok, 100));
  }

  const { result: dur } = await s('Runtime.evaluate', { expression: '__demo.duration' });
  const duration = dur.value;
  frames = Math.ceil((duration / 1000) * FPS);
  console.log(`${(duration / 1000).toFixed(1)}s at ${FPS} fps = ${frames} frames`);

  /* --stills dumps one frame per scene and stops. It is the fastest way to
     look at what the GIF will say before spending four hundred screenshots
     on it. */
  if (process.argv.includes('--stills')) {
    const { result: at } = await s('Runtime.evaluate', { expression: 'JSON.stringify(__demo.stills)' });
    const stills = JSON.parse(at.value);
    await mkdir(join(HERE, '.stills'), { recursive: true });
    for (const [i, t] of stills.entries()) {
      await s('Runtime.evaluate', { expression: `__demo.render(${t})` });
      const shot = await s('Page.captureScreenshot', { format: 'png' });
      await writeFile(join(HERE, '.stills', `step${i + 1}.png`), Buffer.from(shot.data, 'base64'));
    }
    console.log(`wrote ${stills.length} stills to .stills/`);
    cdp.close();
    process.exit(0);
  }

  await rm(FRAMES, { recursive: true, force: true });
  await mkdir(FRAMES, { recursive: true });

  for (let i = 0; i < frames; i += 1) {
    const t = Math.round((i * 1000) / FPS);
    await s('Runtime.evaluate', { expression: `__demo.render(${t})` });
    const shot = await s('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    await writeFile(join(FRAMES, `f${String(i).padStart(5, '0')}.png`), Buffer.from(shot.data, 'base64'));
    if (i % 50 === 0) process.stdout.write(`\r  frame ${i}/${frames}`);
  }
  process.stdout.write(`\r  frame ${frames}/${frames}\n`);

  cdp.close();
} finally {
  proc.kill();
  server.close();
  await rm(profile, { recursive: true, force: true }).catch(() => {});
}

/* ---- encode -------------------------------------------------------------- */

const load = async (i) =>
  halve(decodePng(await readFile(join(FRAMES, `f${String(i).padStart(5, '0')}.png`))));

/* Pass one: the histogram. Every third frame is enough to see every colour the
   film uses — a colour that appears in one frame and no other would have to be
   a single character on screen for 1/15 of a second — and anything missed is
   still mapped to its nearest neighbour rather than dropped. */
const histogram = new Map();
for (let i = 0; i < frames; i += 3) {
  const { data } = await load(i);
  for (let p = 0; p < data.length; p += 3) {
    const k = (data[p] << 16) | (data[p + 1] << 8) | data[p + 2];
    histogram.set(k, (histogram.get(k) ?? 0) + 1);
  }
}

const palette = buildPalette(histogram, COLORS);
const nearest = mapper(palette);
console.log(`  ${histogram.size} colours seen, palette of ${palette.length}`);

/* Pass two: write them. */
const delayCs = Math.max(1, Math.round(100 / FPS));
const gif = new Gif(960, 450, palette, delayCs);
const indices = new Uint8Array(960 * 450);

for (let i = 0; i < frames; i += 1) {
  const { data } = await load(i);
  for (let p = 0, q = 0; q < indices.length; p += 3, q += 1) {
    indices[q] = nearest(data[p], data[p + 1], data[p + 2]);
  }
  gif.addFrame(indices);
  if (i % 50 === 0) process.stdout.write(`\r  encode ${i}/${frames}`);
}
process.stdout.write(`\r  encode ${frames}/${frames}\n`);

await writeFile(OUT, gif.finish());
await rm(FRAMES, { recursive: true, force: true });

const { size } = await stat(OUT);
console.log(
  `${OUT}  ${(size / 1024 / 1024).toFixed(2)} MB  ${frames} frames  ${(100 / delayCs).toFixed(1)} fps`,
);
