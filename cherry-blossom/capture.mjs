// Visual smoke test for the Sakura Drift scene.
// Usage: node capture.mjs   (requires google-chrome on PATH)
// Uses the DevTools protocol directly — no npm dependencies.
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { setTimeout as sleep } from 'node:timers/promises';

const PORT = 9333;
const OUT = new URL('./shots/', import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });

const chrome = spawn('google-chrome', [
  '--headless=new', '--no-sandbox', '--disable-gpu', '--hide-scrollbars',
  '--no-first-run', '--disable-features=Translate,MediaRouter',
  `--remote-debugging-port=${PORT}`, '--user-data-dir=/tmp/sakura-capture',
  'about:blank',
], { stdio: 'ignore' });

const results = [];
const log = (name, ok, detail = '') => {
  results.push({ name, ok, detail });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? '  — ' + detail : ''}`);
};

async function connect() {
  for (let i = 0; i < 50; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${PORT}/json/version`);
      const v = await r.json();
      return v.webSocketDebuggerUrl;
    } catch { await sleep(150); }
  }
  throw new Error('Chrome DevTools endpoint never came up');
}

const wsUrl = await connect();
const ws = new WebSocket(wsUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });

let seq = 0;
const pending = new Map();
const consoleErrors = [];
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
  } else if (msg.method === 'Runtime.exceptionThrown') {
    consoleErrors.push(msg.params.exceptionDetails.text + ' ' + (msg.params.exceptionDetails.exception?.description || ''));
  } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
    consoleErrors.push(msg.params.args.map((a) => a.value ?? a.description).join(' '));
  }
};
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
  const id = ++seq;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params, sessionId }));
});

// Attach to a fresh page target.
const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
const s = (m, p) => send(m, p, sessionId);

await s('Runtime.enable');
await s('Page.enable');

const pageUrl = new URL('./index.html', import.meta.url).href;

async function setViewport(width, height, mobile = false) {
  await s('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile });
  await s('Emulation.setVisibleSize', { width, height });
}

async function reload() {
  await s('Page.navigate', { url: pageUrl });
  await sleep(1800);
}

const evaluate = async (expression) => {
  const r = await s('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text);
  return r.result.value;
};

async function shot(name) {
  const { data } = await s('Page.captureScreenshot', { format: 'png' });
  writeFileSync(OUT + name + '.png', Buffer.from(data, 'base64'));
  log(`screenshot ${name}`, true, `${Buffer.from(data, 'base64').length} bytes`);
}

// --- Wide day ---------------------------------------------------------------
await setViewport(1440, 900);
await reload();

const boot = await evaluate(`(() => {
  const k = window.__sakura;
  if (!k) return { ok: false };
  return {
    ok: true,
    trees: k.world.trees.length,
    blossoms: k.world.blossoms.length,
    petals: k.petals.length,
    canvas: [document.getElementById('scene').width, document.getElementById('scene').height],
    theme: document.body.dataset.theme,
  };
})()`);
log('scene boots with trees, blossoms and petals', boot.ok && boot.trees >= 3 && boot.blossoms > 200 && boot.petals > 0, JSON.stringify(boot));
await sleep(600);
await shot('wide-day');

// Animation actually advances: petal positions change over time.
const moving = await evaluate(`(async () => {
  const p = window.__sakura.petals[0];
  const a = { x: p.x, y: p.y };
  await new Promise(r => setTimeout(r, 400));
  return Math.hypot(p.x - a.x, p.y - a.y) > 1;
})()`);
log('petals animate over time', moving === true);

// --- Gust -------------------------------------------------------------------
const gust = await evaluate(`(async () => {
  const k = window.__sakura;
  const before = k.petals.length;
  document.getElementById('btn-wind').click();
  await new Promise(r => setTimeout(r, 250));
  return { gust: k.state.gust, grew: k.petals.length > before, pressed: document.getElementById('btn-wind').getAttribute('aria-pressed') };
})()`);
log('gust button throws petals and sets state', gust.gust > 0.2 && gust.grew, JSON.stringify(gust));

// --- Night ------------------------------------------------------------------
const night = await evaluate(`(async () => {
  document.getElementById('btn-theme').click();
  await new Promise(r => setTimeout(r, 2200));
  return { body: document.body.dataset.theme, label: document.querySelector('#btn-theme .btn__label--long').textContent, pressed: document.getElementById('btn-theme').getAttribute('aria-pressed') };
})()`);
log('night toggle switches theme + label', night.body === 'night' && night.label === 'Day' && night.pressed === 'true', JSON.stringify(night));
await shot('wide-night');

// --- Wind radio -------------------------------------------------------------
const wind = await evaluate(`(async () => {
  document.querySelector('[data-wind="storm"]').click();
  await new Promise(r => setTimeout(r, 300));
  const segs = [...document.querySelectorAll('.wind__seg')].map(b => b.dataset.wind + ':' + b.getAttribute('aria-checked'));
  return { key: window.__sakura.state.windKey, segs };
})()`);
log('storm wind selected via radiogroup', wind.key === 'storm' && wind.segs.includes('storm:true') && wind.segs.filter(s => s.endsWith('true')).length === 1, JSON.stringify(wind));

// --- Pause ------------------------------------------------------------------
const pause = await evaluate(`(async () => {
  document.getElementById('btn-motion').click();
  const p = window.__sakura.petals[0];
  const a = { x: p.x, y: p.y };
  const t0 = window.__sakura.state.time;
  await new Promise(r => setTimeout(r, 400));
  return { moved: Math.hypot(p.x - a.x, p.y - a.y), timeAdvanced: window.__sakura.state.time - t0, label: document.getElementById('motion-label').textContent, pressed: document.getElementById('btn-motion').getAttribute('aria-pressed') };
})()`);
log('pause freezes petals and time', pause.moved === 0 && pause.timeAdvanced === 0 && pause.label === 'Resume' && pause.pressed === 'true', JSON.stringify(pause));
await evaluate(`document.getElementById('btn-motion').click()`);

// --- Keyboard shortcuts -----------------------------------------------------
const keys = await evaluate(`(async () => {
  const fire = (key) => window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }));
  fire('n');
  fire('1');
  fire('g');
  await new Promise(r => setTimeout(r, 120));
  return { theme: document.body.dataset.theme, wind: window.__sakura.state.windKey, gust: window.__sakura.state.gust > 0.1 };
})()`);
log('keyboard shortcuts N / 1 / G work', keys.theme === 'day' && keys.wind === 'calm' && keys.gust, JSON.stringify(keys));

// --- No horizontal overflow, UI in viewport ---------------------------------
const layout = await evaluate(`(() => {
  const dock = document.querySelector('.dock').getBoundingClientRect();
  return {
    scrollX: document.documentElement.scrollWidth > window.innerWidth,
    scrollY: document.documentElement.scrollHeight > window.innerHeight,
    dockInside: dock.left >= 0 && dock.right <= window.innerWidth && dock.bottom <= window.innerHeight,
  };
})()`);
log('no page overflow; dock within viewport (wide)', !layout.scrollX && !layout.scrollY && layout.dockInside, JSON.stringify(layout));

// --- Narrow / mobile --------------------------------------------------------
await setViewport(390, 844, true);
await reload();
await sleep(400);
const narrow = await evaluate(`(() => {
  const dock = document.querySelector('.dock').getBoundingClientRect();
  const title = document.querySelector('.title').getBoundingClientRect();
  const counter = document.querySelector('.counter').getBoundingClientRect();
  const overlap = !(dock.top > title.bottom) && !(title.bottom < dock.top);
  return {
    scrollX: document.documentElement.scrollWidth > window.innerWidth,
    dockInside: dock.left >= 0 && dock.right <= window.innerWidth,
    dockOverlapsTitle: overlap,
    counterRect: [Math.round(counter.top), Math.round(counter.bottom)],
    dockTop: Math.round(dock.top),
    counterDisplay: getComputedStyle(document.querySelector('.counter')).display,
    counterAboveDock: counter.bottom <= dock.top || getComputedStyle(document.querySelector('.counter')).display === 'none',
    trees: window.__sakura.world.trees.length,
  };
})()`);
log('narrow layout fits and keeps controls clear of title', !narrow.scrollX && narrow.dockInside && !narrow.dockOverlapsTitle && narrow.counterAboveDock && narrow.trees === 2, JSON.stringify(narrow));
await shot('narrow-day');

// --- Reduced motion ---------------------------------------------------------
// Must be set before the document is created: the media query is read at boot.
await s('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
await reload();
const reduced = await evaluate(`(async () => {
  const k = window.__sakura;
  await new Promise(r => setTimeout(r, 150));
  return { flag: k.state.reducedMotion, petals: k.petals.length, paused: k.state.paused, pausedLabel: document.getElementById('motion-label').textContent };
})()`);
log('reduced motion respected (paused, no new petals)', reduced.flag === true && reduced.paused === true && reduced.petals <= 30, JSON.stringify(reduced));
await s('Emulation.setEmulatedMedia', { features: [] });

log('no console errors or uncaught exceptions', consoleErrors.length === 0, consoleErrors.slice(0, 3).join(' | '));

ws.close();
chrome.kill('SIGKILL');

const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} checks passed. Screenshots in shots/`);
process.exit(failed ? 1 : 0);
