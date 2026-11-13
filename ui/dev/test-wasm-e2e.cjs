// End-to-end check of the WASM memory inspector's access path (0.19.0) in Playwright's
// Chromium, against the fixture game (scripts/vm-www/wasm-game): the same DevTools calls the
// host makes (Runtime.evaluate of WebAssembly.Memory.prototype and WebAssembly.Instance.prototype,
// Runtime.queryObjects on each, Runtime.callFunctionOn on a probe instance kept in an object
// group), then scan → next → write → freeze, as the inspector page, a userscript and the agent
// do it; the instance with its exports and globals; memory.grow from the game's button (and of
// a shared memory); the worker variant through an auto-attached session of the tab.
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
  const iproto = await send('Runtime.evaluate', { expression: 'WebAssembly.Instance.prototype', objectGroup: group });
  // `query`: look for memories and instances first (the host does on `list` and for a new probe).
  async function call(req, query) {
    const args = [{ value: null }, { value: req }, { value: null }];
    if (query !== false) {
      const found = await send('Runtime.queryObjects', { prototypeObjectId: proto.result.objectId, objectGroup: group });
      const insts = await send('Runtime.queryObjects', { prototypeObjectId: iproto.result.objectId, objectGroup: group });
      args[0] = { objectId: found.objects.objectId };
      args[2] = { objectId: insts.objects.objectId };
    }
    const r = await send('Runtime.callFunctionOn', {
      objectId: probe.result.objectId,
      functionDeclaration: 'function (found, req, more) { return this.call(found, req, more); }',
      arguments: args,
      returnByValue: true, awaitPromise: true,
    });
    if (r.exceptionDetails) throw new Error((r.exceptionDetails.exception && r.exceptionDetails.exception.description) || r.exceptionDetails.text);
    return r.result.value;
  }
  return { call, release: () => send('Runtime.releaseObjectGroup', { objectGroup: group }) };
}

// The page of the worker variant has no memory of its own.
async function ins0Empty(page) {
  const c = await page.context().newCDPSession(page);
  const i = await inspector((m, p) => c.send(m, p));
  const sv = await i.call({ op: 'survey' });
  await i.release();
  await c.detach();
  return sv.memories.length === 0 && sv.instances.length === 0;
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

    // The instance: its exports, which memory is its, its globals.
    console.log('0.19.0: the instance, its exports and globals (page)');
    const sv = await ins.call({ op: 'survey' });
    const inst = sv.instances[0];
    const ex = (n) => inst.exports.find((e) => e.name === n);
    check(sv.instances.length === 1 && sv.memories.length === 1, 'queryObjects finds the game\'s instance: 1 instance, 1 memory');
    check(ex('memory').kind === 'memory' && ex('memory').mem === m, 'its export "memory" is the listed memory ' + m);
    check(inst.counts.function === 14 && inst.counts.global === 2 && ex('hit').kind === 'function', 'its exports by kind (' + JSON.stringify(inst.counts) + ')');
    check(ex('__stack_pointer').type === 'i32' && ex('__stack_pointer').mutable === true && ex('__stack_pointer').value === 1048576, '__stack_pointer: a mutable i32 global, 1048576');
    const ver = ex('VERSION');
    check(ver.type === 'i32' && ver.mutable === false && (await ins.call({ op: 'read', mem: m, addr: ver.value, type: 'i32' }, false)).value === 19, 'VERSION: an immutable i32 global, the address of an i32 that is 19');
    const set = await ins.call({ op: 'setGlobal', inst: inst.inst, export: ex('__stack_pointer').index, value: 1048560 }, false);
    await page.waitForFunction(() => /sp=1048560/.test(document.title), null, { timeout: 2000 });
    check(set.value === 1048560, 'setGlobal: the page reads the new value of its mutable global');
    check(/not mutable/.test(await ins.call({ op: 'setGlobal', inst: inst.inst, export: ver.index, value: 1 }, false).then(() => '', (e) => e.message)), 'setGlobal on the immutable one is refused');
    const gl = await ins.call({ op: 'globals', inst: inst.inst }, false);
    check(gl.length === 2 && gl.find((g) => g.name === '__stack_pointer').value === 1048560, 'globals: the values of now');
    check((await ins.call({ op: 'survey' })).instances.length === 1, 'a second query finds the same instance, not the probe\'s own type checks');

    // memory.grow from the game's button: the held memory, the old results and the freeze go on.
    console.log('0.19.0: memory.grow (page)');
    const MARK = 17 * 65536 + 16;
    check(/pages=17 marker=0/.test(await page.title()) && sv.memories[0].pages === 17, 'before: 17 pages');
    check(/past the end/.test(await ins.call({ op: 'read', mem: m, addr: MARK, type: 'i32' }, false).then(() => '', (e) => e.message)), 'before: 0x' + MARK.toString(16) + ' is past the end');
    const unk = await ins.call({ op: 'scan', mem: m, type: 'i32', kind: 'unknown' }, false);
    await page.click('#grow');
    await page.waitForFunction(() => /pages=18 marker=4242/.test(document.title));
    // (No query from here on: the probe holds the memory object and takes its buffer again.)
    const after = await ins.call({ op: 'list' }, false);
    check(after.length === 1 && after[0].mem === m && after[0].pages === 18, 'after Grow: the same memory number, 18 pages, without looking again');
    check((await ins.call({ op: 'read', mem: m, addr: MARK, type: 'i32' }, false)).value === 4242, 'read: the address past the old size is valid and holds 4242');
    await ins.call({ op: 'write', mem: m, addr: MARK, type: 'i32', value: 7 }, false);
    await page.waitForFunction(() => /marker=7\b/.test(document.title), null, { timeout: 2000 });
    check(true, 'write: the game reads 7 in its new page');
    const old = (await ins.call({ op: 'results', scan: s.scan }, false)).rows;
    check(old.length === 1 && old[0].addr === rows[0].addr && old[0].value === 500, 'the result list from before the growth still shows the health address and its value');
    await page.click('#hit');
    await page.waitForTimeout(300);
    check(/health=500/.test(await page.title()), 'the freeze from before still holds the value (' + await page.title() + ')');
    const nx = await ins.call({ op: 'next', scan: s.scan, kind: 'increased' }, false);
    check(nx.count === 1, 'a next scan on the old results (90 → 500: increased)');
    const un = await ins.call({ op: 'next', scan: unk.scan, kind: 'unchanged' }, false);
    check(un.count > 1000, 'a snapshot from before, compared after the growth (' + un.count + ' unchanged)');
    const seven = await ins.call({ op: 'scan', mem: m, type: 'i32', kind: 'exact', value: 7 }, false);
    const sevenAt = (await ins.call({ op: 'results', scan: seven.scan, limit: 1000 }, false)).rows.map((r) => r.addr);
    check(seven.scannedBytes === 18 * 65536 && sevenAt.indexOf(MARK) >= 0, 'a new scan covers the new page and finds the 7');
    const fz = await ins.call({ op: 'freeze', mem: m, addr: MARK, type: 'i32', value: 31337 }, false);
    await page.click('#grow');
    await page.waitForFunction(() => /pages=19/.test(document.title));
    const hx = await ins.call({ op: 'bytes', mem: m, addr: MARK, length: 16 }, false);
    check(hx.size === 19 * 65536 && hx.bytes[0] === 0x69 && hx.bytes[1] === 0x7a, 'hex window after a second growth: the frozen 31337 at the old marker, size 19 pages');
    await ins.call({ op: 'unfreeze', freeze: fz.freeze }, false);
    check((await ins.call({ op: 'read', mem: m, addr: 18 * 65536 + 16, type: 'i32' }, false)).value === 4242, 'and the new marker one page on');

    // A shared memory that grows.
    const sharedOk = await page.evaluate(() => { try { window.shm = new WebAssembly.Memory({ initial: 1, maximum: 4, shared: true }); new DataView(window.shm.buffer).setInt32(64, 2024, true); return true; } catch (e) { return String(e); } });
    if (sharedOk === true) {
      const l2 = await ins.call({ op: 'list' });
      const sm = l2.find((x) => x.shared);
      check(!!sm && sm.pages === 1, 'a shared memory is found (memory ' + (sm && sm.mem) + ')');
      const ss = await ins.call({ op: 'scan', mem: sm.mem, type: 'i32', kind: 'exact', value: 2024 }, false);
      await page.evaluate(() => { window.shm.grow(2); new DataView(window.shm.buffer).setInt32(65536 + 64, 2025, true); });
      const sr = (await ins.call({ op: 'results', scan: ss.scan }, false)).rows;
      const far = await ins.call({ op: 'read', mem: sm.mem, addr: 65536 + 64, type: 'i32' }, false);
      await ins.call({ op: 'write', mem: sm.mem, addr: 3 * 65536 - 4, type: 'i32', value: -1 }, false);
      check(sr.length === 1 && sr[0].addr === 64 && sr[0].value === 2024 && far.value === 2025 && await page.evaluate(() => new DataView(window.shm.buffer).getInt32(3 * 65536 - 4, true) === -1),
        'shared memory after grow: old results, a read and a write past the old size');
    } else {
      console.log('  (no shared memory in this page: ' + sharedOk + ')');
    }
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
      const i = await window.__probe.call(null, { op: 'instances' });
      return { l, s, i };
    });
    check(hooked.l.length === 1 && hooked.s.count >= 1, 'hook: memory listed and scannable (' + JSON.stringify(hooked.l) + ')');
    check(hooked.i.length === 1 && hooked.i[0].exports.find((e) => e.name === 'memory').mem === 0 && hooked.i[0].exports.find((e) => e.name === '__stack_pointer').mutable === true,
      'hook: the instance is caught too, with its memory and its globals');

    // The host's own strings (core::wasm::make_expression / hook_script and the GM.wasm
    // reply), written by `KRUNK_WASM_EXPR_OUT=<dir> cargo test -p core --lib wasm::tests::pages`.
    const EXPR = process.env.KRUNK_WASM_EXPR_OUT;
    if (EXPR && fs.existsSync(path.join(EXPR, 'make.js'))) {
      console.log('0.19.0: the host\'s expressions, as core builds them');
      const read = (f) => fs.readFileSync(path.join(EXPR, f), 'utf8');
      const fn = 'function (found, req, more) { return this.call(found, req, more); }';
      const callOn = async (c, objectId, found, req, more) => {
        const r = await c.send('Runtime.callFunctionOn', { objectId, functionDeclaration: fn,
          arguments: [found ? { objectId: found } : { value: null }, { value: req }, more ? { objectId: more } : { value: null }], returnByValue: true, awaitPromise: true, silent: true });
        if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception.description);
        return r.result.value;
      };
      // Without the hook: make.js, then queryObjects.
      const p4 = await browser.newPage();
      await p4.goto(base + 'index.html');
      await p4.waitForFunction(() => /health=100/.test(document.title));
      const c4 = await p4.context().newCDPSession(p4);
      const made = await c4.send('Runtime.evaluate', { expression: read('make.js'), objectGroup: 'krunk-wasm', returnByValue: false, silent: true });
      check(!made.exceptionDetails && !!made.result.objectId, 'make_expression(None) gives a probe handle');
      check((await callOn(c4, made.result.objectId, null, { op: 'list' })).length === 0, 'a new probe knows no memory before the query');
      const proto = await c4.send('Runtime.evaluate', { expression: 'WebAssembly.Memory.prototype', objectGroup: 'krunk-wasm-found', returnByValue: false, silent: true });
      const found = await c4.send('Runtime.queryObjects', { prototypeObjectId: proto.result.objectId, objectGroup: 'krunk-wasm-found' });
      check((await callOn(c4, made.result.objectId, found.objects.objectId, { op: 'list' })).length === 1, 'with the queried memories: listed');
      const iproto = await c4.send('Runtime.evaluate', { expression: 'WebAssembly.Instance.prototype', objectGroup: 'krunk-wasm-found', returnByValue: false, silent: true });
      const ifound = await c4.send('Runtime.queryObjects', { prototypeObjectId: iproto.result.objectId, objectGroup: 'krunk-wasm-found' });
      const survey = await callOn(c4, made.result.objectId, found.objects.objectId, { op: 'survey' }, ifound.objects.objectId);
      check(survey.memories.length === 1 && survey.instances.length === 1 && survey.instances[0].exports.some((e) => e.kind === 'memory' && e.mem === 0), 'the host\'s survey with both queries: the memory and its instance');
      check((await callOn(c4, made.result.objectId, null, { op: 'read', mem: 0, where: 0, type: 'u8' })).addr === 0, 'later calls need no query');
      // A navigation: the handle is gone, and the engine's words are the ones core looks for.
      await p4.reload();
      const lost = await callOn(c4, made.result.objectId, null, { op: 'list' }).then(() => '', (e) => String(e.message));
      check(/could not find object|cannot find context|execution context was destroyed|invalid remote object/i.test(lost), 'after a reload the handle is lost, in words handle_lost() knows (' + lost.slice(0, 80) + ')');
      // With the hook: hook.js at document start, make-hooked.js takes its instance over.
      const p5 = await browser.newPage();
      const c5 = await p5.context().newCDPSession(p5);
      await c5.send('Page.enable');
      await c5.send('Page.addScriptToEvaluateOnNewDocument', { source: read('hook.js') });
      await p5.goto(base + 'index.html');
      await p5.waitForFunction(() => /health=100/.test(document.title));
      check(await p5.evaluate(() => Object.keys(window).indexOf('k1') < 0 && Object.getOwnPropertyNames(window).indexOf('k1') >= 0), 'the hook leaves its probe under a non-enumerable property');
      const taken = await c5.send('Runtime.evaluate', { expression: read('make-hooked.js'), objectGroup: 'krunk-wasm', returnByValue: false, silent: true });
      const hl = await callOn(c5, taken.result.objectId, null, { op: 'list' });
      check(hl.length === 1, 'the host takes the hooked probe over: its memory is listed without a query (' + JSON.stringify(hl) + ')');
      check((await callOn(c5, taken.result.objectId, null, { op: 'instances' })).length === 1, 'and its instance');
      check(await p5.evaluate(() => Object.getOwnPropertyNames(window).indexOf('k1') < 0), 'the property is gone once taken');
      // The GM.wasm reply expression runs and does nothing where no loader waits.
      const rep = await c5.send('Runtime.evaluate', { expression: read('reply.js'), silent: true, returnByValue: true });
      check(!rep.exceptionDetails, 'gm_reply() is a valid expression');
    } else {
      console.log('(the host\'s expressions were not given: set KRUNK_WASM_EXPR_OUT)');
    }

    // Worker: the tab's auto-attached session of the dedicated worker (the host uses flattened
    // sessions; Playwright's client can only reach a child session through
    // Target.sendMessageToTarget: the same DevTools calls, in the worker's own context).
    console.log('0.19.0: the worker variant (auto-attach session)');
    const p3 = await browser.newPage();
    const c3 = await p3.context().newCDPSession(p3);
    const attached = new Promise((ok) => c3.on('Target.attachedToTarget', ok));
    await c3.send('Target.setAutoAttach', { autoAttach: true, waitForDebuggerOnStart: false, flatten: false });
    await p3.goto(base + 'index.html?worker');
    const ev = await attached;
    check(ev.targetInfo.type === 'worker', 'a worker target attached (' + ev.targetInfo.type + ')');
    await p3.waitForFunction(() => /health=100/.test(document.title));
    let seq = 0;
    const waiting = new Map();
    c3.on('Target.receivedMessageFromTarget', (msg) => { const d = JSON.parse(msg.message); const f = waiting.get(d.id); if (f) { waiting.delete(d.id); f(d); } });
    const wsend = (method, params) => new Promise((ok, no) => {
      const id = ++seq;
      waiting.set(id, (d) => (d.error ? no(new Error(d.error.message)) : ok(d.result)));
      c3.send('Target.sendMessageToTarget', { sessionId: ev.sessionId, message: JSON.stringify({ id, method, params }) }).catch(no);
    });
    const win = await inspector(wsend);
    const wsv = await win.call({ op: 'survey' });
    const wi = wsv.instances[0];
    check(wsv.memories.length === 1 && wsv.instances.length === 1 && wi.exports.find((e) => e.name === 'memory').mem === 0, 'queryObjects in the worker\'s session finds its memory and its instance');
    check((await win.call({ op: 'survey' })).memories.length === 1 && (await ins0Empty(p3)), 'the page itself has none (the module runs in the worker)');
    let ws = await win.call({ op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 100 }, false);
    await p3.click('#hit');
    await p3.waitForFunction(() => /health=90/.test(document.title));
    ws = await win.call({ op: 'next', scan: ws.scan, kind: 'decreased' }, false);
    const wrows = (await win.call({ op: 'results', scan: ws.scan }, false)).rows;
    check(ws.count === 1 && wrows[0].value === 90, 'worker: scan, hit, next scan "decreased" leaves the health address');
    await win.call({ op: 'freeze', mem: 0, addr: wrows[0].addr, type: 'i32', value: 777 }, false);
    await p3.click('#hit');
    await p3.waitForFunction(() => /health=777/.test(document.title), null, { timeout: 3000 });
    check(true, 'worker: a frozen value holds (the worker\'s own timer writes it)');
    const wsp = wi.exports.find((e) => e.name === '__stack_pointer');
    await win.call({ op: 'setGlobal', inst: 0, export: wsp.index, value: 1048544 }, false);
    await p3.waitForFunction(() => /sp=1048544/.test(document.title), null, { timeout: 3000 });
    check(wsp.mutable === true && wsp.type === 'i32', 'worker: its mutable global is written');
    await p3.click('#grow');
    await p3.waitForFunction(() => /pages=18 marker=4242/.test(document.title));
    const wm = await win.call({ op: 'read', mem: 0, addr: 17 * 65536 + 16, type: 'i32' }, false);
    const wold = (await win.call({ op: 'results', scan: ws.scan }, false)).rows;
    check(wm.value === 4242 && wold.length === 1 && wold[0].value === 777, 'worker: after memory.grow the new page is read and the old results still stand');
    await win.release();
  } catch (e) {
    fail++; console.log('  FAIL ' + (e && e.stack || e));
  } finally {
    await browser.close();
    server.close();
  }
  console.log(`\n${pass} passed, ${fail} failed`);
  process.exit(fail ? 1 : 0);
})();
