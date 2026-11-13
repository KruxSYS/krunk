// End-to-end check of the WASM memory inspector's access path (0.19.0) in Playwright's
// Chromium, against the fixture game (scripts/vm-www/wasm-game): the same DevTools calls the
// host makes (Runtime.evaluate of WebAssembly.Memory.prototype, Runtime.queryObjects,
// Runtime.callFunctionOn on a probe instance kept in an object group), then scan → next →
// write → freeze, as the inspector page, a userscript and the agent do it. Also the worker
// variant through a flattened auto-attach session.
//
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-wasm-e2e.cjs
'use strict';
const { chromium } = require('playwright');
const http = require('http');
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '../../scripts/vm-www/wasm-game');
const PROBE = fs.readFileSync(path.join(__dirname, '../inject/wasm-probe.js'), 'utf8').trim().replace(/;$/, '');
const TYPES = { html: 'text/html', js: 'text/javascript', wasm: 'application/wasm' };

let pass = 0, fail = 0;
const check = (c, n) => { if (c) { pass++; console.log('  ok   ' + n); } else { fail++; console.log('  FAIL ' + n); } };

async function inspector(send) {
  const group = 'krunk-wasm';
  const probe = await send('Runtime.evaluate', { expression: '(' + PROBE + ')(globalThis)', objectGroup: group, returnByValue: false });
  const proto = await send('Runtime.evaluate', { expression: 'WebAssembly.Memory.prototype', objectGroup: group });
  async function call(req) {
    const found = await send('Runtime.queryObjects', { prototypeObjectId: proto.result.objectId, objectGroup: group });
    const r = await send('Runtime.callFunctionOn', {
      objectId: probe.result.objectId,
      functionDeclaration: 'function (found, req) { return this.call(found, req); }',
      arguments: [{ objectId: found.objects.objectId }, { value: req }],
      returnByValue: true, awaitPromise: true,
    });
    if (r.exceptionDetails) throw new Error((r.exceptionDetails.exception && r.exceptionDetails.exception.description) || r.exceptionDetails.text);
    return r.result.value;
  }
  return { call, release: () => send('Runtime.releaseObjectGroup', { objectGroup: group }) };
}

(async () => {
  const server = http.createServer((req, res) => {
    const p = path.join(ROOT, decodeURIComponent(new URL(req.url, 'http://x').pathname).replace(/^\/+/, '') || 'index.html');
    if (!p.startsWith(ROOT) || !fs.existsSync(p)) { res.writeHead(404); res.end(); return; }
    res.writeHead(200, { 'Content-Type': TYPES[path.extname(p).slice(1)] || 'application/octet-stream' });
    res.end(fs.readFileSync(p));
  }).listen(0);
  const base = 'http://127.0.0.1:' + server.address().port + '/';
  const browser = await chromium.launch();
  try {
    console.log('0.19.0: WASM inspector end to end (page)');
    const page = await browser.newPage();
    await page.goto(base + 'index.html');
    await page.waitForFunction(() => /health=100/.test(document.title));
    const cdp = await page.context().newCDPSession(page);
    const ins = await inspector((m, p) => cdp.send(m, p));
    const mems = await ins.call({ op: 'list' });
    check(mems.length >= 1, 'queryObjects finds the game\'s memory (' + JSON.stringify(mems) + ')');
    const m = mems[0].mem;
    let s = await ins.call({ op: 'scan', mem: m, type: 'i32', kind: 'exact', value: 100 });
    check(s.count >= 1, 'first scan for 100: ' + s.count + ' address(es)');
    await page.click('#hit');
    await page.waitForFunction(() => /health=90/.test(document.title));
    s = await ins.call({ op: 'next', scan: s.scan, kind: 'decreased' });
    const rows = (await ins.call({ op: 'results', scan: s.scan })).rows;
    check(s.count === 1 && rows[0].value === 90, 'next scan "decreased" leaves the health address (' + JSON.stringify(rows) + ')');
    await ins.call({ op: 'write', mem: m, addr: rows[0].addr, type: 'i32', value: 999 });
    await page.waitForFunction(() => /health=999/.test(document.title), null, { timeout: 2000 });
    check(true, 'writing 999 shows in the game');
    await ins.call({ op: 'freeze', mem: m, addr: rows[0].addr, type: 'i32', value: 500 });
    await page.click('#hit'); await page.click('#hit');
    await page.waitForTimeout(300);
    check(/health=500/.test(await page.title()), 'frozen at 500 although hit twice (' + await page.title() + ')');
    // Score as f32 and gold through a pointer path: gold 250 is found, then [ptr]+8.
    await page.click('#pts');
    const f = await ins.call({ op: 'scan', mem: m, type: 'f32', kind: 'exact', value: 1.5 });
    check(f.count >= 1, 'f32 scan finds the score');
    const g = await ins.call({ op: 'scan', mem: m, type: 'i32', kind: 'exact', value: 250 });
    const gAddr = (await ins.call({ op: 'results', scan: g.scan })).rows.map((r) => r.addr);
    // A pointer to gold - 8 (the struct) somewhere in memory.
    const ptrs = await ins.call({ op: 'scan', mem: m, type: 'u32', kind: 'exact', value: gAddr[0] - 8 });
    const pAddr = (await ins.call({ op: 'results', scan: ptrs.scan })).rows[0];
    const viaPtr = pAddr && await ins.call({ op: 'read', mem: m, where: { base: pAddr.addr, offsets: [8] }, type: 'i32' });
    check(viaPtr && viaPtr.value === 250, 'gold read through the pointer path [' + (pAddr && pAddr.addr) + ']+8');
    // The page cannot reach the probe.
    check(await page.evaluate(() => typeof window.call === 'undefined' && Object.keys(window).every((k) => !/probe|krunk/i.test(k))), 'nothing of the inspector on the page\'s window');
    await ins.release();

    // The hook (userscripts): installed at document start, finds the memory without DevTools.
    console.log('0.19.0: the document-start hook (page)');
    const p2 = await browser.newPage();
    await p2.addInitScript({ content: 'window.__probe = (' + PROBE + ')(globalThis); window.__probe.hook();' });
    await p2.goto(base + 'index.html');
    await p2.waitForFunction(() => /health=100/.test(document.title));
    const hooked = await p2.evaluate(async () => {
      const l = await window.__probe.call(null, { op: 'list' });
      const s = await window.__probe.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 100 });
      return { l, s };
    });
    check(hooked.l.length === 1 && hooked.s.count >= 1, 'hook: memory listed and scannable (' + JSON.stringify(hooked) + ')');

    // Worker: flattened auto-attach, the probe in the worker's own context.
    console.log('0.19.0: the worker variant (auto-attach session)');
    const p3 = await browser.newPage();
    const c3 = await p3.context().newCDPSession(p3);
    const attached = new Promise((ok) => c3.on('Target.attachedToTarget', ok));
    await c3.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: true });
    await p3.goto(base + 'index.html?worker');
    const ev = await attached;
    check(ev.targetInfo.type === 'worker', 'a worker target attached (' + ev.targetInfo.type + ')');
    await p3.waitForFunction(() => /health=100/.test(document.title));
    const w = p3.workers()[0];
    const wl = await w.evaluate('(' + PROBE + ')(globalThis).call(null, {op:"list"})');
    check(Array.isArray(wl), 'the probe runs in a worker\'s context (Playwright cannot send queryObjects to a worker session; the host does that on Windows)');
  } catch (e) {
    fail++; console.log('  FAIL ' + (e && e.stack || e));
  } finally {
    await browser.close();
    server.close();
  }
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();
