// The WASM inspector's two programmable surfaces, end to end in Node (0.19.0):
//
// 1. The AI agent's wasm_* tools: the mock provider (ui/dev/mock-provider.cjs) is started and its
//    scenarios are played as Krunk plays them (chat completions with tool results). Each wasm_*
//    call is turned into the probe's request the way core::wasm::agent_request does, and run by
//    the real probe (ui/inject/wasm-probe.js) on the test game (scripts/vm-www/wasm-game/game.wasm)
//    instantiated here. This file's translation is not a second implementation to trust: it is
//    checked against ui/dev/wasm-agent-cases.json, the cases the core unit test
//    `agent_cases_from_the_shared_file` runs through the real one.
// 2. GM.wasm: user scripts run through the real loader (ui/inject/userscript-loader.js) whose
//    calls are answered by the same probe: a script that does every operation, the script the
//    mock agent saves, and the worked example of docs/WASM_INSPECTOR.md.
//
// Runs alone (`node ui/dev/test-wasm-agent.cjs`) and from test-ui.cjs (`ONLY=WasmAgent`).
'use strict';
const fs = require('fs');
const path = require('path');
const http = require('http');
const os = require('os');
const vm = require('vm');
const { spawn } = require('child_process');

const GAME = path.join(__dirname, '../../scripts/vm-www/wasm-game/game.wasm');
const CASES = JSON.parse(fs.readFileSync(path.join(__dirname, 'wasm-agent-cases.json'), 'utf8'));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function makeProbe() {
  const src = fs.readFileSync(path.join(__dirname, '../inject/wasm-probe.js'), 'utf8').trim().replace(/;$/, '');
  // eslint-disable-next-line no-new-func
  return new Function('scope', 'return (' + src + ')(scope);')(globalThis);
}

// The game, as its page runs it, and the host's part: the probe with what a query finds.
function makeWorld() {
  const instance = new WebAssembly.Instance(new WebAssembly.Module(fs.readFileSync(GAME)));
  const ex = instance.exports;
  ex.init();
  const probe = makeProbe();
  const clicks = { '#hit': () => ex.hit(10), '#heal': () => ex.heal(5), '#pts': () => ex.add_score(1.5), '#spend': () => ex.spend(20), '#grow': () => ex.grow() };
  // What the host answers `list` with (app/src/browser/wasm.rs: wasm_list).
  const list = async () => {
    const sv = await probe.call([ex.memory], { op: 'survey' }, [instance]);
    return { contexts: [{ ctx: 0, kind: 'page', url: 'http://localhost/wasm-game/index.html', memories: sv.memories, instances: sv.instances }], unreachable: 'x' };
  };
  const host = (req) => (req.op === 'list' ? list() : probe.call(null, req));
  return { ex, probe, clicks, host, end: () => probe.call(null, { op: 'reset' }) };
}

// ---- the agent's tools: core::wasm::agent_request and agent_trim_instances, mirrored ----
function agentRequest(tool, a) {
  const at = (e) => (Array.isArray(e.offsets) && e.offsets.length ? { base: e.addr, offsets: e.offsets } : e.addr);
  const clean = (o) => { Object.keys(o).forEach((k) => { if (o[k] === undefined || o[k] === null) delete o[k]; }); return o; };
  switch (tool) {
    case 'wasm_memories': return { op: 'list' };
    case 'wasm_read': return clean({ op: 'read', mem: a.mem, where: at(a), type: a.type, length: a.length });
    case 'wasm_write': return clean({ op: 'write', mem: a.mem, where: at(a), type: a.type, value: a.value });
    case 'wasm_scan': return a.mode === 'next' ? clean({ op: 'next', scan: a.scan, kind: a.kind, value: a.value, to: a.to })
      : clean({ op: 'scan', mem: a.mem, type: a.type, kind: a.kind, value: a.value, to: a.to });
    case 'wasm_results': return { op: 'results', scan: a.scan, offset: a.offset || 0, limit: Math.min(a.limit === undefined ? 50 : a.limit, 200) };
    case 'wasm_freeze': return clean({ op: 'freeze', mem: a.mem, where: at(a), type: a.type, value: a.value });
    case 'wasm_unfreeze': return { op: 'unfreeze', freeze: a.freeze };
    case 'wasm_watch': return { op: 'watch', entries: a.entries.map((e) => clean({ mem: e.mem, where: at(e), type: e.type, length: e.length })) };
    case 'wasm_global': return a.value === undefined || a.value === null ? { op: 'globals', inst: a.inst } : { op: 'setGlobal', inst: a.inst, export: a.export, value: a.value };
    default: return null;
  }
}
function trimInstances(list) {
  list.contexts.forEach((c) => {
    c.instances = (c.instances || []).map((i) => ({
      inst: i.inst, counts: i.counts,
      memories: i.exports.filter((e) => e.kind === 'memory').map((e) => ({ name: e.name, mem: e.mem })),
      globals: i.exports.filter((e) => e.kind === 'global').slice(0, 40).map((e) => ({ export: e.index, name: e.name, type: e.type, mutable: e.mutable, value: e.value })),
      functions: i.exports.filter((e) => e.kind === 'function').slice(0, 40).map((e) => e.name),
    }));
  });
  return list;
}
const canon = (v) => (Array.isArray(v) ? v.map(canon) : v && typeof v === 'object' ? Object.keys(v).sort().reduce((o, k) => { o[k] = canon(v[k]); return o; }, {}) : v);
const same = (a, b) => JSON.stringify(canon(a)) === JSON.stringify(canon(b));

// ---- the mock provider, played as Krunk plays it ----
function startProvider() {
  return new Promise((ok, no) => {
    const probe = http.createServer().listen(0, '127.0.0.1', () => {
      const port = probe.address().port;
      probe.close(() => {
        const log = path.join(os.tmpdir(), 'krunk-wasm-agent-' + process.pid + '.jsonl');
        const child = spawn(process.execPath, [path.join(__dirname, 'mock-provider.cjs'), '--port', String(port), '--log', log], { stdio: ['ignore', 'pipe', 'inherit'] });
        child.stdout.on('data', (d) => { if (/mock provider on/.test(String(d))) ok({ port, stop: () => { child.kill(); try { fs.unlinkSync(log); } catch (_) { /* none */ } } }); });
        child.on('error', no);
      });
    });
  });
}
function complete(port, messages) {
  return new Promise((ok, no) => {
    const req = http.request({ host: '127.0.0.1', port, path: '/v1/chat/completions', method: 'POST', headers: { 'Content-Type': 'application/json' } }, (res) => {
      let text = '';
      res.on('data', (c) => { text += c; });
      res.on('end', () => {
        const out = { content: '', name: '', args: '' };
        text.split('\n\n').forEach((line) => {
          const m = /^data: (\{.*\})$/.exec(line.trim());
          if (!m) return;
          const d = JSON.parse(m[1]).choices[0].delta;
          if (d.content) out.content += d.content;
          (d.tool_calls || []).forEach((t) => { if (t.function.name) out.name = t.function.name; out.args += t.function.arguments || ''; });
        });
        ok(out);
      });
    });
    req.on('error', no);
    req.end(JSON.stringify({ model: 'mock-agent', stream: true, messages }));
  });
}
// One task: the tool calls made, the finish text, and anything save_userscript was given.
async function play(port, world, words, check) {
  const messages = [{ role: 'user', content: 'User message: ' + words }];
  const calls = [];
  const saved = [];
  for (let n = 0; n < 200; n++) {
    const out = await complete(port, messages);
    if (!out.name) return { calls, saved, answer: out.content };
    const args = JSON.parse(out.args || '{}');
    if (out.name === 'finish') return { calls, saved, answer: args.answer };
    calls.push({ name: out.name, args });
    let result;
    if (out.name === 'click') {
      world.clicks[args.selector]();
      await sleep(120); // (a frozen value is written back by a 50 ms timer)
      result = { clicked: args.selector };
    } else if (out.name === 'save_userscript') {
      saved.push(args.source);
      result = { saved: true, id: 'us1' };
    } else {
      const req = agentRequest(out.name, args);
      if (!req) { check(false, 'the scenario calls a tool this check does not know: ' + out.name); return { calls, saved, answer: '' }; }
      try {
        result = await world.host(req);
        if (out.name === 'wasm_memories') result = trimInstances(result);
      } catch (e) { result = 'Error: ' + e.message; }
    }
    messages.push({ role: 'assistant', content: null, tool_calls: [{ id: 'c' + n, type: 'function', function: { name: out.name, arguments: out.args } }] });
    messages.push({ role: 'tool', tool_call_id: 'c' + n, content: typeof result === 'string' ? result : JSON.stringify(result) });
  }
  check(false, 'the scenario did not finish in 200 steps');
  return { calls, saved, answer: '' };
}

// ---- GM.wasm: the real loader, its calls answered by the probe ----
function loaderWorld(world, grants) {
  const src = fs.readFileSync(path.join(__dirname, '../inject/userscript-loader.js'), 'utf8').trim().replace(/;$/, '');
  const BINDING = '__krunkUS_agent', NONCE = 'nonce-a';
  const sent = [];
  const window = { setTimeout: setTimeout, requestIdleCallback: undefined };
  window.top = window;
  window[BINDING] = (t) => {
    const m = JSON.parse(t);
    if (m.op !== 'wasm') return;
    const body = JSON.parse(m.body);
    sent.push(body);
    // The host: the request to the context's probe, the answer through the reply function.
    world.host(body.req).then((v) => window[BINDING + 'R'](NONCE, Number(m.id), true, v), (e) => window[BINDING + 'R'](NONCE, Number(m.id), false, e.message));
  };
  const sandbox = { window: window, location: { href: 'http://localhost/wasm-game/index.html', protocol: 'http:', hostname: 'localhost' },
    document: { readyState: 'complete', title: '' }, console: console, URL: URL, JSON: JSON, Object: Object, Reflect: Reflect, Promise: Promise, Error: Error };
  const reg = vm.runInNewContext('(' + src + ')', sandbox)(BINDING, NONCE, '0.19.0', ['s1']);
  let args = null;
  reg.add({ id: 's1', name: 's1', matches: ['*://*/*'], includes: [], excludes: [], runAt: 0, noframes: false, force: false, grants: grants, info: {}, values: [] }, function () { args = arguments; });
  reg.go();
  return { GM: args[9], GM_wasm: args[11], sent, document: sandbox.document };
}
// A user script's source run with the loader's GM (as the host wraps it: core::userscripts).
function runScript(source, lw, extra) {
  const names = ['GM', 'GM_wasm', 'document', 'setTimeout', 'console', 'done'];
  // eslint-disable-next-line no-new-func
  return new Function(names.join(','), source)(lw.GM, lw.GM_wasm, lw.document, setTimeout, console, extra);
}

const TOUR_SCRIPT = `
return (async () => {
  const out = {};
  const mems = await GM.wasm.memories();
  const insts = await GM.wasm.instances();
  const m = mems[0], mem = m.mem;
  out.memories = mems; out.instances = insts;
  let s = await GM.wasm.scan({ mem, type: 'i32', kind: 'exact', value: 100 });
  out.first = s.count;
  done.click('#hit');
  out.decreased = (await GM.wasm.next({ scan: s.scan, kind: 'decreased' })).count;
  const H = (await GM.wasm.results({ scan: s.scan })).rows[0].addr;
  out.H = H;
  out.exact = (await GM.wasm.next({ scan: s.scan, kind: 'exact', value: 90 })).count;
  out.unchanged = (await GM.wasm.next({ scan: s.scan, kind: 'unchanged' })).count;
  done.click('#heal');
  out.increased = (await GM.wasm.next({ scan: s.scan, kind: 'increased' })).count;
  done.click('#hit');
  out.changed = (await GM.wasm.next({ scan: s.scan, kind: 'changed' })).count;
  out.range = (await GM.wasm.scan({ mem, type: 'i32', kind: 'range', value: 80, to: 90 })).count;
  const u = await GM.wasm.scan({ mem, type: 'i32', kind: 'unknown' });
  out.snapshot = u.snapshot;
  done.click('#hit');
  out.unknownDecreased = (await GM.wasm.next({ scan: u.scan, kind: 'decreased' })).count;
  out.discarded = await GM.wasm.discard(u.scan);
  const g = await GM.wasm.scan({ mem, type: 'i32', kind: 'exact', value: 250 });
  const G = (await GM.wasm.results({ scan: g.scan })).rows[0].addr;
  const p = await GM.wasm.scan({ mem, type: 'u32', kind: 'exact', value: G - 8 });
  const P = (await GM.wasm.results({ scan: p.scan })).rows[0].addr;
  out.viaPointer = await GM.wasm.read({ mem, where: { base: P, offsets: [8] }, type: 'i32' });
  out.written = await GM.wasm.write({ mem, addr: H, type: 'i32', value: 999 });
  out.read = await GM.wasm.read({ mem, addr: H, type: 'i32' });
  const fz = await GM.wasm.freeze({ mem, addr: H, type: 'i32', value: 999 });
  done.click('#hit');
  await new Promise((r) => setTimeout(r, 130));
  out.frozenRead = await GM.wasm.read({ mem, addr: H, type: 'i32' });
  out.frozen = await GM.wasm.frozen();
  out.watch = await GM.wasm.watch([{ mem, addr: H, type: 'i32' }, { mem, where: { base: P, offsets: [8] }, type: 'i32' }, { mem, addr: m.bytes + 16, type: 'i32' }]);
  out.pastEnd = await GM.wasm.read({ mem, addr: m.bytes + 16, type: 'i32' }).then(() => '', (e) => e.message);
  done.click('#grow');
  out.afterGrow = await GM.wasm.watch([{ mem, addr: H, type: 'i32' }, { mem, addr: m.bytes + 16, type: 'i32' }]);
  out.oldResults = (await GM.wasm.results({ scan: s.scan })).rows;
  out.nextAfterGrow = (await GM.wasm.next({ scan: s.scan, kind: 'increased' })).count;
  out.pages = (await GM.wasm.memories())[0].pages;
  out.unfrozen = await GM.wasm.unfreeze(fz);
  const base = m.bytes + 256;
  const typed = [['i8', -5], ['u8', 250], ['i16', -3000], ['u16', 60000], ['i32', -70000], ['u32', 4000000000], ['i64', '-9007199254740993'], ['u64', '18446744073709551615'],
    ['f32', 1.5], ['f64', -2.25], ['bytes', [222, 173, 190, 239]], ['utf8', 'héllo'], ['utf16', 'hé']];
  out.typed = [];
  for (let i = 0; i < typed.length; i++) {
    const t = typed[i][0], o = { mem, addr: base + 32 * i, type: t };
    await GM.wasm.write(Object.assign({ value: typed[i][1] }, o));
    if (t === 'bytes' || t === 'utf16') o.length = 4;
    if (t === 'utf8') o.length = 6;
    out.typed.push(await GM.wasm.read(o));
  }
  out.hex = await GM.wasm.bytes({ mem, addr: base, length: 16 });
  const gl = await GM.wasm.globals({ inst: insts[0].inst });
  const sp = gl.find((x) => x.name === '__stack_pointer');
  out.globals = gl;
  out.setGlobal = await GM.wasm.setGlobal({ inst: insts[0].inst, export: sp.index, value: sp.value - 16 });
  out.refused = await GM.wasm.setGlobal({ inst: insts[0].inst, export: gl.find((x) => x.name === 'VERSION').index, value: 1 }).then(() => '', (e) => e.message);
  return out;
})();`;

async function run(check) {
  // The translation used below is core's: the shared cases.
  let okCases = 0;
  CASES.cases.filter((c) => !c.error).forEach((c) => { if (same(agentRequest(c.tool, c.args), c.req)) okCases++; else check(false, 'this check\'s translation differs from core\'s for ' + JSON.stringify(c.args)); });
  check(okCases === CASES.cases.filter((c) => !c.error).length && okCases >= 40, 'the tool → request translation used here matches core\'s on the ' + okCases + ' shared cases');

  const provider = await startProvider();
  try {
    // --- the tour: every operation of the agent's tools ---
    let world = makeWorld();
    const tour = await play(provider.port, world, 'wasm tools tour', check);
    check(/^TOUR \{/.test(tour.answer), 'tour: the scenario ran to its end (' + tour.calls.length + ' tool calls)' + (/^TOUR/.test(tour.answer) ? '' : ': ' + tour.answer.slice(0, 300)));
    const t = /^TOUR /.test(tour.answer) ? JSON.parse(tour.answer.slice(5)) : {};
    const names = new Set(tour.calls.map((c) => c.name));
    check(['wasm_memories', 'wasm_read', 'wasm_write', 'wasm_scan', 'wasm_results', 'wasm_freeze', 'wasm_unfreeze', 'wasm_watch', 'wasm_global'].every((n) => names.has(n)), 'tour: all nine wasm_* tools are called');
    const c0 = ((t.memories || {}).contexts || [])[0] || { memories: [{}], instances: [{}] };
    const i0 = c0.instances[0] || {};
    check(c0.memories.length === 1 && c0.memories[0].pages === 17 && i0.counts && i0.counts.function === 14 && same(i0.memories, [{ name: 'memory', mem: 0 }]) && i0.globals.length === 2 && i0.functions.indexOf('hit') >= 0 && !i0.exports,
      'wasm_memories: the memory (17 pages) and the instance in its short form (its memory, 2 globals, function names)');
    const H = t.results && t.results.rows[0].addr;
    check(t.first.count >= 1 && t.decreased.count === 1 && t.results.rows.length === 1 && t.results.rows[0].value === 90 && H === world.ex.health_addr(), 'wasm_scan first exact → next decreased → wasm_results: the health address 0x' + Number(H).toString(16));
    check(t.exact.count === 1 && t.unchanged.count === 1 && t.increased.count === 1 && t.changed.count === 1, 'wasm_scan next: exact, unchanged, increased, changed each keep the address');
    check(t.range.count >= 1 && t.unknown.snapshot === true && t.unknown.count > 1000 && t.unknownDecreased.count >= 1 && t.unknownDecreased.count < 10, 'wasm_scan first: range, and unknown initial value then decreased (' + t.unknownDecreased.count + ' left)');
    check(t.viaPointer.value === 250 && t.viaPointer.addr === t.goldAt.rows[0].addr, 'wasm_read through a pointer path (offsets [8]) reads the gold');
    check(t.write.written === 4 && t.read.value === 999 && t.freeze.freeze >= 1, 'wasm_write, wasm_read, wasm_freeze');
    check(t.watch.length === 3 && t.watch[0].value === 999 && t.watch[1].value === 250 && same(t.watch[2].value, [231, 3, 0, 0]), 'wasm_watch: the frozen health after a hit (999), the gold through the pointer, raw bytes');
    check(/past the end/.test(String(t.beforeGrow)) && t.afterGrow[0].value === 999 && t.afterGrow[1].value === 4242, 'memory.grow: an address past the end is refused before, read after (4242); the freeze holds through it');
    check(t.oldResults.rows.length === 1 && t.oldResults.rows[0].addr === H && t.oldResults.rows[0].value === 999 && t.nextAfterGrow.count === 1, 'memory.grow: the result list from before and a next scan on it');
    check(t.unfreeze.unfrozen === true, 'wasm_unfreeze');
    check(same(t.typed.map((x) => x.value), [-5, 250, -3000, 60000, -70000, 4000000000, '-9007199254740993', '18446744073709551615', 1.5, -2.25, [222, 173, 190, 239], 'héllo', 'hé']),
      'wasm_write and reads of i8 u8 i16 u16 i32 u32 i64 u64 f32 f64 bytes utf8 utf16 (' + JSON.stringify((t.typed || []).map((x) => x.value)) + ')');
    check(t.hex.value.length === 64 && t.hex.value[0] === 251 && t.hex.value[32] === 250, 'wasm_read type bytes: a 64-byte hex window');
    const sp = (t.globals || []).find((g) => g.name === '__stack_pointer') || {};
    check(sp.type === 'i32' && sp.mutable === true && t.setGlobal.value === sp.value - 16 && world.ex.__stack_pointer.value === sp.value - 16 && t.globalsAfter.find((g) => g.name === '__stack_pointer').value === sp.value - 16,
      'wasm_global: reads the globals, writes the mutable one (the module sees ' + world.ex.__stack_pointer.value + ')');
    await world.end();

    // --- the scenario used on Windows: find the health, set it, freeze it ---
    world = makeWorld();
    const health = await play(provider.port, world, 'find the health value in the wasm game and set it to 999, then freeze it', check);
    world.clicks['#hit'](); world.clicks['#hit']();
    await sleep(130);
    check(/0x100004, set to 999 and frozen/.test(health.answer) && world.ex.health() === 999, 'health scenario: found by scanning, written and frozen (two more hits, still ' + world.ex.health() + ')');
    await world.end();

    // --- the agent writes a user script that uses GM.wasm; it runs through the loader ---
    world = makeWorld();
    const gold = await play(provider.port, world, 'write a userscript that keeps the gold', check);
    check(gold.saved.length === 1 && /@grant\s+GM\.wasm/.test(gold.saved[0]), 'gold scenario: save_userscript with @grant GM.wasm');
    let lw = loaderWorld(world, ['GM.wasm']);
    runScript(gold.saved[0].replace(/^[\s\S]*==\/UserScript==/, ''), lw);
    for (let i = 0; i < 40 && world.ex.gold() !== 9999; i++) await sleep(50);
    world.clicks['#spend']();
    await sleep(130);
    check(world.ex.gold() === 9999 && lw.sent.some((b) => b.req.op === 'freeze'), 'the saved script, run by the loader: gold frozen at 9999 (after Spend: ' + world.ex.gold() + ')');
    await world.end();
  } finally {
    provider.stop();
  }

  // --- GM.wasm: every operation from a user script ---
  const world = makeWorld();
  const lw = loaderWorld(world, ['GM.wasm']);
  const o = await runScript(TOUR_SCRIPT, lw, { click: (sel) => world.clicks[sel]() });
  check(o.memories.length === 1 && o.memories[0].pages === 17 && o.instances.length === 1 && o.instances[0].exports.find((e) => e.kind === 'memory').mem === o.memories[0].mem, 'GM.wasm: memories() and instances() (the instance\'s memory is the listed one)');
  check(o.first >= 1 && o.decreased === 1 && o.H === world.ex.health_addr() && o.exact === 1 && o.unchanged === 1 && o.increased === 1 && o.changed === 1, 'GM.wasm: scan exact, next decreased / exact / unchanged / increased / changed, results');
  check(o.range >= 1 && o.snapshot === true && o.unknownDecreased >= 1 && o.discarded === true, 'GM.wasm: scan range and unknown, discard');
  check(o.viaPointer === 250 && o.written === 4 && o.read === 999 && o.frozenRead === 999 && o.frozen.length === 1 && o.frozen[0].value === 999 && o.unfrozen === true, 'GM.wasm: pointer path, write, read, freeze (holds through a hit), frozen, unfreeze');
  check(o.watch[0].value === 999 && o.watch[1].value === 250 && /past the end/.test(o.watch[2].error) && /past the end/.test(o.pastEnd), 'GM.wasm: watch (address, pointer path, and an error per entry); a read past the end rejects');
  check(o.afterGrow[0].value === 999 && o.afterGrow[1].value === 4242 && o.oldResults[0].value === 999 && o.nextAfterGrow === 1 && o.pages === 18, 'GM.wasm across memory.grow: the freeze, the new page, the old results, a next scan, 18 pages');
  check(same(o.typed, [-5, 250, -3000, 60000, -70000, 4000000000, '-9007199254740993', '18446744073709551615', 1.5, -2.25, [222, 173, 190, 239], 'héllo', 'hé']), 'GM.wasm: write and read of all thirteen types');
  check(o.hex.bytes.length === 16 && o.hex.bytes[0] === 251 && o.hex.size === 18 * 65536, 'GM.wasm: bytes() (hex read)');
  const sp = o.globals.find((g) => g.name === '__stack_pointer');
  check(sp.mutable === true && o.setGlobal === sp.value - 16 && world.ex.__stack_pointer.value === sp.value - 16 && /not mutable/.test(o.refused), 'GM.wasm: globals(), setGlobal() on the mutable one; the constant one rejects');
  await world.end();

  // --- without the grant ---
  const plain = loaderWorld(makeWorld(), ['GM_addStyle']);
  check(plain.GM.wasm === undefined && plain.GM_wasm === undefined && plain.sent.length === 0, 'a script without @grant GM.wasm has no GM.wasm and no GM_wasm');

  // --- the worked example of the docs page ---
  const doc = fs.readFileSync(path.join(__dirname, '../../docs/WASM_INSPECTOR.md'), 'utf8');
  const example = /```js\n(\/\/ ==UserScript==[\s\S]*?)```/.exec(doc);
  check(!!example && /@grant\s+GM\.wasm/.test(example[1]), 'docs/WASM_INSPECTOR.md has a worked example script');
  if (example) {
    const w2 = makeWorld();
    const l2 = loaderWorld(w2, ['GM.wasm']);
    runScript(example[1].replace(/^[\s\S]*==\/UserScript==/, ''), l2);
    await sleep(300);
    w2.clicks['#hit']();
    for (let i = 0; i < 60 && w2.ex.health() !== 500; i++) await sleep(50);
    w2.clicks['#hit'](); w2.clicks['#hit']();
    await sleep(130);
    check(w2.ex.health() === 500, 'the example script: after the first Hit it finds the health and holds it at 500 (now ' + w2.ex.health() + ')');
    await w2.end();
  }
}

module.exports = function ({ check }) {
  return { testWasmAgent: async function () { console.log('0.19.0: the agent\'s wasm_* tools and GM.wasm, end to end (Node)'); await run(check); } };
};

if (require.main === module) {
  let pass = 0, fail = 0;
  const check = (c, n) => { if (c) { pass++; console.log('  ok   ' + n); } else { fail++; console.log('  FAIL ' + n); } };
  run(check).then(() => { console.log(`\n${pass} passed, ${fail} failed`); process.exit(fail ? 1 : 0); }, (e) => { console.error(e); process.exit(2); });
}
