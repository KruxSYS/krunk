// Krunk 0.18.0: measures how one hung request holds up others in Chromium (docs/NETWORK.md).
// Starts hangserve.py, runs each scenario in its own fresh profile and prints a Markdown table.
//
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node scripts/vm-www/hang-measure.cjs [only,...]
//
// Times are seconds after tab A started. "at the server" is when hangserve logged the request
// (its access log, /log); "never" means not within the scenario's watch time (WATCH seconds).
'use strict';
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const http = require('http');

const PORT = +(process.env.HANG_PORT || 8120);
const H = 'http://127.0.0.1:' + PORT;
const OTHER = 'http://localhost:' + PORT; // another host: its own connection pool group
const WATCH = 30;
const only = process.argv[2] ? process.argv[2].split(',') : null;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const get = (url) => new Promise((resolve, reject) => {
  http.get(url, (res) => { let b = ''; res.on('data', (d) => { b += d; }); res.on('end', () => resolve(b)); }).on('error', reject);
});
const events = async () => JSON.parse(await get(H + '/log'));
const r1 = (x) => (typeof x === 'number' ? x.toFixed(1) : x);

async function scenario(name, fn) {
  if (only && !only.includes(name)) return [];
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'hang-' + name + '-'));
  const ctx = await chromium.launchPersistentContext(dir, { headless: true });
  const rows = [];
  try {
    // A first load, so the browser's start-up is not in the numbers.
    const warm = await ctx.newPage();
    await warm.goto(H + '/nocache?warm=' + name);
    await warm.close();
    let zero = 0, t0 = Date.now(), starts = 0;
    const api = {
      // Time zero: call it once the tabs exist, right before tab A starts.
      start: async () => {
        const mark = 'zero=' + name + '~' + (++starts);
        await get(H + '/nocache?' + mark);
        zero = (await events()).filter((e) => e.path.includes(mark))[0].t;
        t0 = Date.now();
      },
      m: name + '~',
      ctx,
      now: () => (Date.now() - t0) / 1000,
      until: async (s) => { const left = s * 1000 - (Date.now() - t0); if (left > 0) await sleep(left); },
      // Arrival times at the server of requests whose path contains `part`.
      arrivals: async (part) => (await events()).filter((e) => e.ev === '+' && e.path.includes(part)).map((e) => e.t - zero),
      // Starts a navigation; resolves to the seconds when it finished (or how it failed).
      go: (page, url, timeout) => page.goto(url, { timeout: (timeout || WATCH) * 1000, waitUntil: 'load' })
        .then(() => (Date.now() - t0) / 1000, (e) => (/Timeout/.test(e.message) ? 'never' : 'failed: ' + e.message.split('\n')[0].slice(0, 60))),
      row: (what, value, note) => rows.push({ name, what, value: r1(value), note: note || '' }),
    };
    await fn(api);
  } finally {
    await ctx.close().catch(() => {});
    fs.rmSync(dir, { recursive: true, force: true });
  }
  return rows;
}
const nth = (list, i) => (list.length > i ? list[i] : 'never');

const SCENARIOS = {
  // The same URL as a navigation in two tabs; the first never gets an answer.
  'nav-same-url-hang': async (s) => {
    const u = H + '/hang?m=' + s.m;
    const a = await s.ctx.newPage(), b = await s.ctx.newPage(), c = await s.ctx.newPage();
    await s.start();
    s.go(a, u, WATCH + 5);
    await s.until(1);
    s.go(b, u, WATCH);
    s.go(c, H + '/ok?m=' + s.m + '-c', WATCH).then((t) => s.row('tab C, another URL on that host, started at 1.0', t, 'done'));
    await s.until(WATCH);
    const at = await s.arrivals('/hang?m=' + s.m);
    s.row('tab B, same URL, started at 1.0: its request at the server', nth(at, 1), at.length + ' request(s) arrived in ' + WATCH + ' s');
  },
  // Krunk's way round it (0.18.0): tab B's request says `Cache-Control: no-cache`, as a hard
  // reload does; the cache then starts a new entry instead of waiting for tab A's.
  'nav-same-url-hang-no-cache-header': async (s) => {
    const u = H + '/hang?m=' + s.m;
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await b.setExtraHTTPHeaders({ 'Cache-Control': 'no-cache', Pragma: 'no-cache' });
    await s.start();
    s.go(a, u, 14);
    await s.until(1);
    s.go(b, u, 12);
    await s.until(12);
    const at = await s.arrivals('/hang?m=' + s.m);
    s.row('tab B, same URL with `Cache-Control: no-cache`, started at 1.0: its request at the server', nth(at, 1), 'watched until 12.0');
  },
  'nav-same-url-slow-headers-no-cache-header': async (s) => {
    const u = H + '/slow-headers?s=8&nostore=1&m=' + s.m;
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await b.setExtraHTTPHeaders({ 'Cache-Control': 'no-cache', Pragma: 'no-cache' });
    await s.start();
    const pa = s.go(a, u);
    await s.until(1);
    const tb = await s.go(b, u);
    s.row('tab A (answer after 8 s, no-store)', await pa, 'done');
    const at = await s.arrivals('m=' + s.m);
    s.row('tab B, same URL with `Cache-Control: no-cache`, started at 1.0', tb, at.length + ' requests at the server (second at ' + r1(nth(at, 1)) + ')');
  },
  // The same, with an answer after 8 s that may be cached.
  'nav-same-url-slow-headers': async (s) => {
    const u = H + '/slow-headers?s=8&cache=1&m=' + s.m;
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await s.start();
    const pa = s.go(a, u);
    await s.until(1);
    const tb = await s.go(b, u);
    s.row('tab A (answer after 8 s, cacheable)', await pa, 'done');
    const at = await s.arrivals('m=' + s.m);
    s.row('tab B, same URL, started at 1.0', tb, at.length === 1 ? 'served from A\'s cache entry: one request at the server' : at.length + ' requests at the server (second at ' + r1(nth(at, 1)) + ')');
  },
  'nav-same-url-slow-headers-nostore': async (s) => {
    const u = H + '/slow-headers?s=8&nostore=1&m=' + s.m;
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await s.start();
    const pa = s.go(a, u);
    await s.until(1);
    const tb = await s.go(b, u);
    s.row('tab A (answer after 8 s, no-store)', await pa, 'done');
    const at = await s.arrivals('m=' + s.m);
    s.row('tab B, same URL, started at 1.0', tb, at.length + ' requests at the server (second at ' + r1(nth(at, 1)) + ')');
  },
  // The same URL as a subresource (fetch) of two tabs.
  'sub-same-url-hang': async (s) => {
    const res = '/hang?m=' + s.m;
    const pageUrl = H + '/page?same=1&u=' + encodeURIComponent(res);
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await s.start();
    await s.go(a, pageUrl);
    await s.until(1);
    await s.go(b, pageUrl);
    await s.until(WATCH);
    const at = await s.arrivals(res);
    s.row('tab B\'s fetch of the URL tab A waits for (started at ~1.0): at the server', nth(at, 1), at.length + ' request(s) arrived in ' + WATCH + ' s');
  },
  // Headers at once, the body's end after 8 s.
  'nav-same-url-slow-body': async (s) => {
    for (const kind of ['cache=1', 'plain=1', 'nostore=1']) {
      const u = H + '/slow-body?s=8&' + kind + '&m=' + s.m;
      const a = await s.ctx.newPage(), b = await s.ctx.newPage();
      await s.start();
      const start = 0;
      const pa = s.go(a, u);
      await sleep(1000);
      const tb = await s.go(b, u);
      const ta = await pa;
      const at = await s.arrivals(kind + '&m=' + s.m);
      const label = { 'cache=1': 'max-age=600', 'plain=1': 'no cache headers', 'nostore=1': 'no-store' }[kind];
      s.row('body ends after 8 s (' + label + '): tab B, same URL, 1.0 after A', typeof tb === 'number' ? tb - start : tb,
        'A done at ' + r1(typeof ta === 'number' ? ta - start : ta) + '; ' + at.length + ' request(s) at the server' + (at.length > 1 ? ', second ' + r1(at[1] - at[0]) + ' after the first' : ''));
      await a.close(); await b.close();
    }
  },
  // Six requests of one tab never answered: the host's six connections are taken.
  'six-hung': async (s) => {
    const a = await s.ctx.newPage(), b = await s.ctx.newPage(), c = await s.ctx.newPage();
    await s.start();
    await s.go(a, H + '/page?n=6&u=' + encodeURIComponent('/hang?m=' + s.m) + '&then=' + encodeURIComponent('/ok?m=' + s.m + '-seventh'));
    await s.until(2);
    const pb = s.go(b, H + '/ok?m=' + s.m + '-tabB', WATCH);
    s.go(c, OTHER + '/ok?m=' + s.m + '-otherhost', WATCH).then((t) => s.row('tab C, a navigation to another host (localhost), started at 2.0', t, 'done'));
    await s.until(12);
        s.row('tab A\'s seventh request (same host): at the server', nth(await s.arrivals('/ok?m=' + s.m + '-seventh'), 0), 'watched until 12.0; hung requests at the server: ' + (await s.arrivals('/hang?m=' + s.m)).length);
    s.row('tab B, a navigation to that host, started at 2.0: at the server', nth(await s.arrivals('/ok?m=' + s.m + '-tabB'), 0), 'watched until 12.0');
    await a.close();
    const closed = s.now();
    const tb = await pb;
    s.row('tab A closed at ' + r1(closed) + ': tab B', tb, typeof tb === 'number' ? 'done ' + r1(tb - closed) + ' s after the close' : '');
  },
  // Six event streams (HTTP/1.1) do the same without anything being broken.
  'six-sse': async (s) => {
    const a = await s.ctx.newPage(), b = await s.ctx.newPage();
    await s.start();
    await s.go(a, H + '/page?n=6&u=' + encodeURIComponent('/sse?m=' + s.m));
    await s.until(2);
    const pb = s.go(b, H + '/ok?m=' + s.m + '-tabB', 14);
    await s.until(10);
    s.row('six open event streams in tab A; tab B navigates to that host at 2.0: at the server', nth(await s.arrivals(s.m + '-tabB'), 0), 'watched until 10.0');
    await a.close();
    const closed = s.now();
    const tb = await pb;
    s.row('tab A closed at ' + r1(closed) + ': tab B', tb, typeof tb === 'number' ? 'done ' + r1(tb - closed) + ' s after the close' : '');
  },
  // Six hung requests spread over two tabs: nothing in either tab is special.
  'six-hung-two-tabs': async (s) => {
    const a = await s.ctx.newPage(), b = await s.ctx.newPage(), c = await s.ctx.newPage();
    await s.start();
    await s.go(a, H + '/page?n=3&u=' + encodeURIComponent('/hang?m=' + s.m + '-a'));
    await s.go(b, H + '/page?n=3&u=' + encodeURIComponent('/hang?m=' + s.m + '-b'));
    await s.until(2);
    s.go(c, H + '/ok?m=' + s.m + '-tabC', 12);
    await s.until(10);
    s.row('three hung requests in tab A and three in tab B; tab C navigates to that host at 2.0: at the server', nth(await s.arrivals(s.m + '-tabC'), 0), 'watched until 10.0');
  },
};

(async () => {
  const server = spawn('python3', [path.join(__dirname, 'hangserve.py'), '--port', String(PORT), '--bind', '127.0.0.1'], { stdio: ['ignore', 'ignore', 'inherit'] });
  try {
    for (let i = 0; i < 50; i++) { try { await get(H + '/nocache?up'); break; } catch (e) { await sleep(100); } }
    const b = await chromium.launch({ headless: true });
    const version = b.version();
    await b.close();
    // Three browsers at a time: more of them delay each other's first requests.
    const todo = Object.entries(SCENARIOS), all = new Array(todo.length);
    let next = 0;
    await Promise.all([0, 1, 2].map(async () => {
      while (next < todo.length) { const i = next++; all[i] = await scenario(todo[i][0], todo[i][1]); }
    }));
    console.log('Chromium ' + version + ' (Playwright), ' + os.platform() + ', ' + new Date().toISOString().slice(0, 10) + '\n');
    console.log('| Scenario | What | Seconds | Notes |\n|---|---|---|---|');
    if (process.env.HANG_LOG) fs.writeFileSync(process.env.HANG_LOG, (await events()).map((e) => e.t.toFixed(3) + ' ' + e.ev + ' conn ' + e.conn + ' ' + e.path + ' ' + e.note).join('\n') + '\n');
    for (const r of all.flat()) console.log('| `' + r.name + '` | ' + r.what + ' | ' + r.value + ' | ' + r.note + ' |');
  } finally {
    server.kill();
  }
})().catch((e) => { console.error(e); process.exit(1); });
