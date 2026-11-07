// Speedometer 3.1 in Playwright's Chromium on the build host, with Krunk's per-frame page
// scripts injected the way the renderer does it (app/src/ffi/cef/renderer.rs): a quick way to
// see whether a page script breaks or slows the benchmark before a VM round.
//
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node scripts/speedometer/local.cjs [--off=media,login,...] [--none] [--iterations=N] [--suites=a,b]
//
// Needs the Speedometer copy served on 127.0.0.1:8096 (scripts/vm-www/speedometer/sp-serve.py
// started in a staged copy, see fetch.sh). Prints the score and any page errors.
'use strict';
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const root = path.join(__dirname, '..', '..');
const args = Object.fromEntries(process.argv.slice(2).map((a) => {
  const [k, v] = a.replace(/^--/, '').split('=');
  return [k, v === undefined ? true : v];
}));
const off = new Set(String(args.off || '').split(',').filter(Boolean));
const renderer = fs.readFileSync(path.join(root, 'app/src/ffi/cef/renderer.rs'), 'utf8');
const rustConst = (name) => {
  const m = new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`).exec(renderer);
  if (!m) throw new Error(`no ${name} in renderer.rs`);
  return m[1];
};
const inject = (name) => fs.readFileSync(path.join(root, 'ui/inject', name), 'utf8');
const withBridge = (code) => `(function (__lbHost) {\n${code}\n})(host);`;

function initScript() {
  const parts = ['(function () {', 'var top = window.top === window;'];
  parts.push(`var dispatch = (${rustConst('SHIM')})(function () {}, false); var host = dispatch.host;`);
  if (!off.has('gpc')) parts.push(inject('gpc.js'));
  if (!off.has('dnt')) parts.push(inject('dnt.js'));
  if (!off.has('audio')) parts.push(`(${rustConst('AUDIO_TRACKER')})(function () {});`);
  if (!off.has('pointer')) parts.push(`(${inject('pointer-lock.js').trim().replace(/;\s*$/, '')})(function () {});`);
  if (!off.has('login')) parts.push(withBridge(inject('login.js')));
  parts.push('if (top) {');
  if (!off.has('tokens')) parts.push(withBridge(inject('page-tokens.js')));
  if (!off.has('notify')) parts.push(withBridge(inject('notify.js')));
  if (!off.has('media')) parts.push(withBridge(inject('media.js')));
  parts.push('}', '})();');
  return parts.join('\n');
}

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  if (!args.none) await page.addInitScript({ content: initScript() });
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });
  for (const f of page.frames()) void f;
  page.on('frameattached', () => {});
  let url = `http://127.0.0.1:8096/index.html?startAutomatically=true&iterationCount=${args.iterations || 3}`;
  if (args.suites) url += `&suites=${args.suites}`;
  await page.goto(url);
  await page.waitForFunction(() => globalThis.benchmarkClient && benchmarkClient._hasResults, null, { timeout: 15 * 60 * 1000, polling: 1000 });
  const r = await page.evaluate(() => {
    const m = benchmarkClient._metrics || {};
    return { score: m.Score && m.Score.mean, delta: m.Score && m.Score.percentDelta, suites: Object.fromEntries(Object.entries(m).filter(([k]) => !k.includes('/') && !k.startsWith('Iteration')).map(([k, v]) => [k, Math.round(v.mean * 10) / 10])) };
  });
  console.log(JSON.stringify({ off: [...off], none: !!args.none, ...r, errors: [...new Set(errors)].slice(0, 20) }, null, 1));
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
