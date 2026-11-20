// Krunk 0.26.0 page-load harness: the driver on the Windows test VM (portable node.exe, run in
// the interactive desktop session by vm-0260-load.ps1). See scripts/perf/README-load.md.
//
//   node load.mjs CONFIG.json
//
// CONFIG: { mode: "replay" | "live" | "record", out, runs, rtt, pages: [{id, url}],
//           browsers: ["krunk", "chrome", "edge", ...], scenarios: ["typed", "link"],
//           krunk: { app, base, env }, archive, settleMs }
//
// Per browser, page and run (interleaved: run → page → browser):
//   typed: fresh profile → start → /blank → Ctrl+L, URL, Enter (cold) → /blank → again (warm)
//   link:  fresh profile → start → /link?to=URL → click (link) → Alt+Left (back) → Alt+Right (fwd)
// Each navigation writes one JSON line to OUT/results.jsonl. Times are ms from the input (the
// moment kinput.exe injected the last event, on the same wall clock as performance.timeOrigin):
// nav (navigation start), ttfb, fcp, lcp, dcl, load; for a back/forward-cache restore: show and
// frame. Page-relative values (from navigation start) are kept too.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const HERE = path.dirname(new URL(import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, '$1');
const cfg = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
const OUT = cfg.out;
fs.mkdirSync(OUT, { recursive: true });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const logf = path.join(OUT, 'run.log');
const log = (...a) => {
  const s = `${new Date().toISOString()} ${a.join(' ')}`;
  console.log(s);
  fs.appendFileSync(logf, s + '\n');
};
const result = (o) => fs.appendFileSync(path.join(OUT, 'results.jsonl'), JSON.stringify(o) + '\n');
const OBSERVER = fs.readFileSync(path.join(HERE, 'observer.js'), 'utf8');
const PORT = 9222;
const AUX = 18090;
const settle = cfg.settleMs ?? 3000;

// ---- helper pages (never mapped to the replay server: a port other than 80/443) ----
const blankHtml = '<!doctype html><title>blank</title><body style="background:#fff">';
const linkHtml = (to) =>
  `<!doctype html><title>link</title><style>html,body{margin:0;height:100%}a{display:block;width:100vw;height:100vh;background:#eef}</style><a id=go href="${to.replace(/"/g, '&quot;')}">go</a>`;
http
  .createServer((req, res) => {
    const u = new URL(req.url, `http://127.0.0.1:${AUX}`);
    res.setHeader('Content-Type', 'text/html; charset=utf-8');
    if (u.pathname === '/link') res.end(linkHtml(u.searchParams.get('to') || '/blank'));
    else res.end(blankHtml);
  })
  .listen(AUX, '127.0.0.1');
const BLANK = `http://127.0.0.1:${AUX}/blank`;

// ---- processes ----
const BROWSERS = {
  chrome: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  edge: 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
};
function killAll() {
  for (const im of ['Krunk.exe', 'chrome.exe', 'msedge.exe']) {
    try {
      execFileSync('taskkill', ['/F', '/T', '/IM', im], { stdio: 'ignore' });
    } catch {}
  }
}
function rmrf(p) {
  for (let i = 0; i < 10; i++) {
    try {
      fs.rmSync(p, { recursive: true, force: true });
      return;
    } catch {
      execFileSync('cmd', ['/c', 'ping -n 2 127.0.0.1 >nul']);
    }
  }
}
const hostRules = () =>
  cfg.mode === 'live'
    ? ''
    : cfg.mode === 'record'
      ? 'MAP *:80 127.0.0.1:8080, MAP *:443 127.0.0.1:8081, EXCLUDE localhost'
      : 'MAP *:80 127.0.0.1:18080, MAP *:443 127.0.0.1:18443, EXCLUDE localhost';

// Files a session leaves in its profile (cfg.collect: paths relative to the profile, e.g.
// "Krunk\\perf.log", "trace.json") are kept as OUT\collect\<session>-<name> before the next
// session wipes the profile.
let lastSession = null;
function collect() {
  if (!lastSession || !cfg.collect) return;
  fs.mkdirSync(path.join(OUT, 'collect'), { recursive: true });
  for (const rel of cfg.collect) {
    const from = path.join(lastSession.profile, rel);
    if (fs.existsSync(from)) {
      try {
        fs.copyFileSync(from, path.join(OUT, 'collect', `${lastSession.tag}-${path.basename(rel)}`));
      } catch (e) {
        log('collect', from, String(e));
      }
    }
  }
  lastSession = null;
}

function launch(name, profile) {
  rmrf(profile);
  if (name === 'krunk' || name.startsWith('krunk')) {
    const k = cfg[name] || cfg.krunk;
    try {
      execFileSync('robocopy', [k.base, profile, '/E', '/NFL', '/NDL', '/NJH', '/NJS', '/NP'], { stdio: 'ignore' });
    } catch (e) {
      if (e.status >= 8) throw new Error(`robocopy ${k.base}: ${e.status}`); // 1 = copied
    }
    const env = { ...process.env, LOCALAPPDATA: profile, KRUNK_DEVTOOLS_PORT: String(PORT), KRUNK_HOST_RULES: hostRules(), ...(k.env || {}) };
    return spawn(path.join(k.app, 'Krunk.exe'), [], { cwd: k.app, env, detached: true, stdio: 'ignore' });
  }
  const args = [
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-search-engine-choice-screen',
    '--start-maximized',
    '--disable-quic',
    `--remote-debugging-port=${PORT}`,
    ...(cfg.browserArgs || []).map((a) => a.replaceAll('{profile}', profile)),
    BLANK,
  ];
  const rules = hostRules();
  if (rules) args.unshift(`--host-resolver-rules=${rules}`);
  return spawn(BROWSERS[name], args, { detached: true, stdio: 'ignore' });
}

// ---- CDP ----
async function targets() {
  return (await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json()).filter((t) => t.type === 'page');
}
function session(t) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(t.webSocketDebuggerUrl);
    let n = 0;
    const wait = new Map();
    const events = [];
    ws.onmessage = (m) => {
      const d = JSON.parse(m.data);
      if (d.id && wait.has(d.id)) {
        wait.get(d.id)(d);
        wait.delete(d.id);
      } else if (d.method) events.push(d);
    };
    ws.onerror = reject;
    ws.onclose = () => {
      for (const r of wait.values()) r({ error: 'closed' });
      wait.clear();
    };
    ws.onopen = () =>
      resolve({
        events,
        send: (method, params = {}) =>
          new Promise((r) => {
            const id = ++n;
            wait.set(id, r);
            try {
              ws.send(JSON.stringify({ id, method, params }));
            } catch (e) {
              r({ error: String(e) });
            }
            setTimeout(() => wait.has(id) && (wait.delete(id), r({ error: 'timeout' })), 10000);
          }),
        close: () => ws.close(),
      });
  });
}
async function ready() {
  for (let i = 0; i < 120; i++) {
    try {
      const t = await targets();
      if (t.length) return true;
    } catch {}
    await sleep(500);
  }
  return false;
}
// Krunk keeps a hidden spare window and tab and its own UI pages: the focused, largest web page wins.
async function pageTarget() {
  let best = null;
  let score = -1;
  for (const t of await targets()) {
    // Krunk's own UI pages (shell, panels, the spare frame) are https://*.krunk.invalid/.
    // Tabs on www.krunk.invalid (new tab page, settings) are tabs too.
    if (!/^https?:|^about:|^chrome:|^edge:/.test(t.url) || /\/\/shell\.krunk\.invalid/.test(t.url)) continue;
    let s;
    try {
      s = await session(t);
      const r = await s.send('Runtime.evaluate', {
        // A visible page wins over Krunk's hidden made-ahead new tab page (same size, 0.25.0).
        expression: "(document.hasFocus() ? 1e9 : 0) + (document.visibilityState === 'visible' ? 1e8 : 0) + innerWidth * innerHeight",
        returnByValue: true,
      });
      const v = r.result?.result?.value ?? 0;
      if (v > score) {
        score = v;
        best = t;
      }
    } catch {
    } finally {
      s?.close();
    }
  }
  return best;
}

function kinput(...steps) {
  const out = execFileSync(path.join(HERE, 'kinput.exe'), steps, { encoding: 'utf8' });
  const lines = out.trim().split(/\r?\n/).map((l) => l.split('\t'));
  const last = lines[lines.length - 1];
  return { epoch: Number(last[1]), qpc: Number(last[2]) };
}

const READ = `(async () => {
  const lp = window.__lp || {};
  const n = performance.getEntriesByType('navigation')[0];
  const wall = lp.now ? lp.now() : Date.now();
  // A document the observer missed (a session attached after it started): the buffered entries.
  let fcp = lp.fcp, lcp = lp.lcp, lcpWhat = lp.lcpWhat;
  if (!window.__lp) {
    const p = performance.getEntriesByName('first-contentful-paint')[0];
    fcp = p ? p.startTime : null;
    const e = await new Promise((r) => {
      try {
        new PerformanceObserver((l) => r(l.getEntries().at(-1))).observe({ type: 'largest-contentful-paint', buffered: true });
      } catch { r(null); }
      setTimeout(() => r(null), 300);
    });
    lcp = e ? e.startTime : null;
    lcpWhat = e ? (e.url || (e.element && e.element.tagName) || '') : '';
  }
  return JSON.stringify({ url: location.href, ready: document.readyState, to: performance.timeOrigin,
    skew: wall - (performance.timeOrigin + performance.now()),
    nav: n && { type: n.type, act: n.activationStart || 0, req: n.requestStart, ttfb: n.responseStart,
      dcl: n.domContentLoadedEventEnd, load: n.loadEventEnd, proto: n.nextHopProtocol },
    fcp, lcp, lcpSize: lp.lcpSize, lcpWhat, shows: lp.shows || [],
    res: performance.getEntriesByType('resource').length, observed: !!window.__lp });
})()`;
async function read(s) {
  const r = await s.send('Runtime.evaluate', { expression: READ, returnByValue: true, awaitPromise: true });
  try {
    return JSON.parse(r.result.result.value);
  } catch {
    return null;
  }
}
const hostOf = (u) => {
  try {
    return new URL(u).host;
  } catch {
    return '';
  }
};

// Waits for the navigation that the input started: a new document on the expected host that has
// loaded (then `settle` ms more for LCP), or a back/forward-cache restore (a new pageshow).
async function measure(s, before, expectHost, timeoutMs = 45000) {
  // A document is known by its timeOrigin; one seen before in this session that shows up again
  // came back from the back/forward cache.
  s.seen ??= new Set();
  if (before) s.seen.add(before.to);
  const t0 = Date.now();
  let st = null;
  let other = null;
  while (Date.now() - t0 < timeoutMs) {
    await sleep(200);
    st = await read(other || s);
    if (!other && Date.now() - t0 > 3000 && (!st || hostOf(st.url) !== expectHost)) {
      // The load went to another browser of the engine (not the tab the session watches).
      const t = (await targets()).find((x) => hostOf(x.url) === expectHost);
      if (t) {
        log('measure: the load is in another target', t.url, '; watched', st?.url);
        other = await session(t);
        continue;
      }
    }
    if (!st || hostOf(st.url) !== expectHost) continue;
    if (!s.seen.has(st.to)) {
      if (st.nav && st.nav.load > 0) {
        await sleep(settle);
        st = (await read(s)) || st;
        s.seen.add(st.to);
        other?.close();
        return { kind: 'load', st, followed: !!other };
      }
    } else if (st.shows.length > (st.to === before?.to ? before.shows.length : 0) || (st.to !== before?.to && st.shows.length)) {
      await sleep(300);
      return { kind: 'bfcache', st: (await read(s)) || st };
    }
  }
  try {
    log('measure: timeout; targets:', (await targets()).map((x) => x.url.slice(0, 80)).join(' | '));
  } catch {}
  return { kind: 'timeout', st };
}

function row(base, inputEpoch, m) {
  const st = m.st || {};
  const rel = (v) => (v == null || !(v > 0) ? null : +(st.to + v + (st.skew || 0) - inputEpoch).toFixed(1));
  const o = { ...base, kind: m.kind, url: st.url, res: st.res, observed: st.observed, followed: m.followed || undefined };
  if (m.kind === 'load') {
    const n = st.nav || {};
    Object.assign(o, {
      nav: +(st.to + (st.skew || 0) - inputEpoch).toFixed(1),
      act: n.act,
      ttfb: rel(n.ttfb),
      fcp: rel(st.fcp),
      lcp: rel(st.lcp),
      dcl: rel(n.dcl),
      load: rel(n.load),
      type: n.type,
      proto: n.proto,
      lcpWhat: st.lcpWhat,
      rel: { ttfb: n.ttfb, fcp: st.fcp, lcp: st.lcp, dcl: n.dcl, load: n.load },
    });
  } else if (m.kind === 'bfcache') {
    const sh = st.shows[st.shows.length - 1];
    Object.assign(o, { show: rel(sh.t), frame: rel(sh.frame), type: 'back_forward_cache' });
  }
  return o;
}

async function attach() {
  const t = await pageTarget();
  if (!t) throw new Error('no page target: ' + (await targets()).map((x) => x.url).join(' '));
  const s = await session(t);
  s.url = t.url;
  await s.send('Page.enable');
  await s.send('Page.addScriptToEvaluateOnNewDocument', { source: OBSERVER });
  return s;
}
async function cdpGo(s, url) {
  await s.send('Page.navigate', { url });
  for (let i = 0; i < 50; i++) {
    await sleep(200);
    const st = await read(s);
    if (st && st.url.startsWith(url.split('?')[0]) && st.ready === 'complete') break;
  }
  await sleep(800);
}
function bfReasons(s) {
  const out = [];
  for (const e of s.events.splice(0)) {
    if (e.method === 'Page.backForwardCacheNotUsed') {
      const flat = (e.params.notRestoredExplanations || []).map((x) => `${x.type}:${x.reason}`);
      out.push(...flat);
    }
  }
  return out;
}

async function startBrowser(b, profile, tag) {
  killAll();
  await sleep(1500);
  collect();
  lastSession = { profile, tag: tag || b };
  launch(b, profile);
  if (!(await ready())) throw new Error(`${b}: no DevTools port`);
  await sleep(cfg.startIdleMs ?? 8000);
  const exe = b.startsWith('krunk') ? 'krunk.exe' : b === 'edge' ? 'msedge.exe' : 'chrome.exe';
  kinput(`focus:${exe}`);
  await sleep(800);
  const s = await attach();
  log(b, 'attached to', s.url, 'of', (await targets()).map((x) => x.url.slice(0, 60)).join(' | '));
  await cdpGo(s, BLANK);
  return s;
}

async function typed(b, page, run) {
  const profile = path.join(cfg.profiles || 'C:\\pipeline\\krunk\\lp', `${b}`);
  const s = await startBrowser(b, profile, `${b}-${page.id}-${run}-typed`);
  const host = hostOf(page.url);
  for (const scen of ['cold', 'warm']) {
    const before = await read(s);
    s.events.splice(0);
    const [cx, cy] = cfg.clickAt || [640, 500];
    kinput(`click:${cx},${cy}`, 'sleep:300');
    const inp = kinput('key:ctrl+l', 'sleep:150', `text:${page.url}`, 'sleep:400', 'key:enter');
    const m = await measure(s, before, host);
    const r = row({ browser: b, page: page.id, run, scenario: scen, inputQpc: inp.qpc }, inp.epoch, m);
    result(r);
    log(b, page.id, run, scen, m.kind, `fcp ${r.fcp} lcp ${r.lcp} load ${r.load}`);
    if (scen === 'cold') await cdpGo(s, BLANK);
  }
  s.close();
}

async function link(b, page, run) {
  const profile = path.join(cfg.profiles || 'C:\\pipeline\\krunk\\lp', `${b}`);
  const s = await startBrowser(b, profile, `${b}-${page.id}-${run}-link`);
  const host = hostOf(page.url);
  await cdpGo(s, `http://127.0.0.1:${AUX}/link?to=${encodeURIComponent(page.url)}`);
  const [cx, cy] = cfg.clickAt || [640, 500];
  let before = await read(s);
  let inp = kinput(`click:${cx},${cy}`);
  let m = await measure(s, before, host);
  let r = row({ browser: b, page: page.id, run, scenario: 'link', inputQpc: inp.qpc }, inp.epoch, m);
  result(r);
  log(b, page.id, run, 'link', m.kind, `fcp ${r.fcp} lcp ${r.lcp} load ${r.load}`);
  // Back to the link page, then forward to the page again.
  for (const [scen, key, expect] of [
    ['back', 'key:alt+left', `127.0.0.1:${AUX}`],
    ['forward', 'key:alt+right', host],
  ]) {
    await sleep(500);
    before = await read(s);
    s.events.splice(0);
    inp = kinput(key);
    m = await measure(s, before, expect, 30000);
    r = row({ browser: b, page: page.id, run, scenario: scen, inputQpc: inp.qpc }, inp.epoch, m);
    await sleep(300);
    r.bfcacheNotUsed = bfReasons(s);
    result(r);
    log(b, page.id, run, scen, m.kind, `show ${r.show ?? '-'} fcp ${r.fcp ?? '-'} load ${r.load ?? '-'} ${r.bfcacheNotUsed.join(' ')}`);
  }
  s.close();
}

async function record() {
  const s = await startBrowser('chrome', path.join(cfg.profiles || 'C:\\pipeline\\krunk\\lp', 'record'));
  for (const p of cfg.pages) {
    // Twice: the second visit records what a warm visit asks for too.
    for (let i = 0; i < 2; i++) {
      await s.send('Page.navigate', { url: p.url });
      await sleep(cfg.recordWaitMs ?? 20000);
      const st = await read(s);
      log('record', p.id, i, st?.url, `res ${st?.res} lcp ${st?.lcp}`);
    }
  }
  s.close();
}

// Web Page Replay (record or replay; our own test CA, trusted on the VM only during a pass) and,
// for replay, the latency shim in front of it.
const TOOLS = cfg.tools || HERE;
function startServers() {
  if (cfg.mode === 'live') return;
  const cert = ['--https-cert-file', path.join(TOOLS, 'krunk-replay-ca.pem'), '--https-key-file', path.join(TOOLS, 'krunk-replay-ca.key')];
  const wlog = fs.openSync(path.join(OUT, 'wpr.log'), 'a');
  spawn(path.join(TOOLS, 'wpr.exe'), [cfg.mode, '--host=127.0.0.1', '--http-port=8080', '--https-port=8081', ...cert, cfg.archive], {
    cwd: TOOLS,
    stdio: ['ignore', wlog, wlog],
  });
  if (cfg.mode === 'replay') {
    const slog = fs.openSync(path.join(OUT, 'lshim.log'), 'a');
    spawn(path.join(TOOLS, 'lshim.exe'), ['--rtt', String(cfg.rtt ?? 40), '127.0.0.1:18080=127.0.0.1:8080', '127.0.0.1:18443=127.0.0.1:8081'], {
      stdio: ['ignore', slog, slog],
    });
  }
}
async function stopServers() {
  if (cfg.mode === 'live') return;
  // WPR writes its archive (record) and exits on this request.
  try {
    await fetch('http://127.0.0.1:8080/web-page-replay-command-exit');
  } catch {}
  await sleep(3000);
  for (const im of ['wpr.exe', 'lshim.exe']) {
    try {
      execFileSync('taskkill', ['/F', '/IM', im], { stdio: 'ignore' });
    } catch {}
  }
}

async function main() {
  log('start', JSON.stringify({ ...cfg, pages: cfg.pages.map((p) => p.id) }));
  await stopServers();
  startServers();
  await sleep(3000);
  if (cfg.mode === 'record') {
    await record();
  } else {
    // A fill pass: only the listed cells ({browser, page, scenario, run}), in the given order.
    for (const c of cfg.cells || []) {
      const page = cfg.pages.find((p) => p.id === c.page);
      try {
        if (c.scenario === 'typed') await typed(c.browser, page, c.run);
        else await link(c.browser, page, c.run);
      } catch (e) {
        log('ERROR', c.browser, c.page, c.run, c.scenario, String(e?.stack || e));
        result({ browser: c.browser, page: c.page, run: c.run, scenario: c.scenario, kind: 'error', error: String(e) });
      }
    }
    for (let run = 1; run <= (cfg.cells ? 0 : cfg.runs); run++) {
      for (const page of cfg.pages) {
        // Interleaved, with the browser order rotated every run.
        const order = cfg.browsers.map((_, i) => cfg.browsers[(i + run - 1) % cfg.browsers.length]);
        for (const b of order) {
          for (const scen of cfg.scenarios) {
            try {
              if (scen === 'typed') await typed(b, page, run);
              else if (scen === 'link') await link(b, page, run);
            } catch (e) {
              log('ERROR', b, page.id, run, scen, String(e?.stack || e));
              result({ browser: b, page: page.id, run, scenario: scen, kind: 'error', error: String(e) });
            }
          }
        }
      }
    }
  }
  killAll();
  await sleep(1500);
  collect();
  await stopServers();
  log('done');
  fs.writeFileSync(path.join(OUT, 'done'), 'ok');
  process.exit(0);
}
main().catch(async (e) => {
  log('FATAL', String(e?.stack || e));
  killAll();
  await stopServers();
  fs.writeFileSync(path.join(OUT, 'done'), 'fatal');
  process.exit(1);
});
