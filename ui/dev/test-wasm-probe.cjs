// Tests of the WASM memory inspector's probe (ui/inject/wasm-probe.js, 0.19.0) under Node, with
// real WebAssembly.Memory objects. Runs alone (`node ui/dev/test-wasm-probe.cjs`) and from
// ui/dev/test-ui.cjs (`ONLY=WasmProbe`).
'use strict';
const fs = require('fs');
const path = require('path');

function make(scope) {
  const src = fs.readFileSync(path.join(__dirname, '../inject/wasm-probe.js'), 'utf8').trim().replace(/;$/, '');
  // eslint-disable-next-line no-new-func
  return new Function('scope', 'return (' + src + ')(scope);')(scope || globalThis);
}

// GM.wasm in the userscript loader (ui/inject/userscript-loader.js): the call a script makes,
// what goes to the host through the binding, and how the host's answer settles the promise.
async function runLoader(check) {
  const vm = require('vm');
  const src = fs.readFileSync(path.join(__dirname, '../inject/userscript-loader.js'), 'utf8').trim().replace(/;$/, '');
  const BINDING = '__krunkUS_test', NONCE = 'nonce-1';
  const sent = [];
  const window = { setTimeout: setTimeout, requestIdleCallback: undefined };
  window.top = window;
  window[BINDING] = (t) => { sent.push(JSON.parse(t)); };
  const sandbox = { window: window, location: { href: 'https://game.example/play', protocol: 'https:', hostname: 'game.example' },
    document: { readyState: 'complete' }, console: console, URL: URL, JSON: JSON, Object: Object, Reflect: Reflect, Promise: Promise, Error: Error };
  const reg = vm.runInNewContext('(' + src + ')', sandbox)(BINDING, NONCE, '0.19.0', ['s1', 's2']);
  const entry = (id, grants) => ({ id: id, name: id, matches: ['*://*/*'], includes: [], excludes: [], runAt: 0, noframes: false, force: false, grants: grants, info: {}, values: [] });
  let granted = null, plain = null;
  reg.add(entry('s1', ['GM.wasm']), function () { granted = arguments; });
  reg.add(entry('s2', ['GM_addStyle']), function () { plain = arguments; });
  reg.go();
  const GM = granted[9], GM_wasm = granted[11];
  check(granted.length === 12 && GM.wasm === GM_wasm && typeof GM_wasm.read === 'function', 'loader: a script with @grant GM.wasm gets GM.wasm (and GM_wasm, the last argument)');
  check(plain[9].wasm === undefined && plain[11] === undefined, 'loader: a script without the grant gets neither');
  check(['memories', 'read', 'write', 'scan', 'next', 'results', 'freeze', 'unfreeze'].every((n) => typeof GM_wasm[n] === 'function'), 'loader: memories, read, write, scan, next, results, freeze, unfreeze');
  const key = BINDING + 'R';
  const d = Object.getOwnPropertyDescriptor(window, key);
  check(d && !d.enumerable && !d.writable && !d.configurable && window[BINDING] === undefined, 'loader: the reply function can be neither listed nor replaced; the binding is gone from window');

  const read = GM_wasm.read({ mem: 0, addr: 1024, type: 'i32', junk: 1 });
  const call = sent.find((m) => m.op === 'wasm');
  const body = JSON.parse(call.body);
  check(call.nonce === NONCE && call.script === 's1' && call.id === '1', 'loader: the call carries the nonce, the script and a promise number');
  check(JSON.stringify(body) === JSON.stringify({ ctx: 0, req: { op: 'read', mem: 0, type: 'i32', where: 1024 } }), 'loader: read {mem, addr, type} goes out as the probe\'s request, unknown fields dropped (' + call.body + ')');
  let settled = false;
  read.then(() => { settled = true; });
  window[key]('wrong', 1, true, { addr: 1024, value: 666 });
  await new Promise((r) => setTimeout(r, 5));
  check(!settled, 'loader: an answer without the nonce (page code) settles nothing');
  window[key](NONCE, 1, true, { addr: 1024, value: 100 });
  check((await read) === 100, 'loader: the host\'s answer resolves read() with the value');
  const bad = GM_wasm.write({ mem: 9, where: { base: 300, offsets: [8] }, type: 'i32', value: 1, ctx: 1000001 });
  const call2 = sent.filter((m) => m.op === 'wasm')[1];
  check(JSON.parse(call2.body).ctx === 1000001 && JSON.parse(call2.body).req.where.base === 300, 'loader: ctx and a pointer path pass through');
  window[key](NONCE, 2, false, 'no memory 9');
  check((await bad.then(() => '', (e) => e.message)) === 'no memory 9', 'loader: an error rejects the promise');
  const un = GM_wasm.unfreeze(3);
  check(JSON.parse(sent.filter((m) => m.op === 'wasm')[2].body).req.freeze === 3, 'loader: unfreeze takes the freeze\'s number');
  window[key](NONCE, 3, true, { unfrozen: true });
  check((await un) === true, 'loader: unfreeze resolves true');
  const mems = GM_wasm.memories();
  window[key](NONCE, 4, true, { contexts: [{ ctx: 0, kind: 'page', url: 'u', memories: [{ mem: 0, bytes: 65536, pages: 1, shared: false }] }, { ctx: 7, kind: 'frame', url: 'f', memories: [] }] });
  const list = await mems;
  check(list.length === 1 && list[0].ctx === 0 && list[0].mem === 0 && list[0].bytes === 65536 && list[0].kind === 'page', 'loader: memories() is a flat list');
}

async function run(check) {
  await runLoader(check);
  const P = make();
  const mem = new WebAssembly.Memory({ initial: 2, maximum: 64 });
  const dv = new DataView(mem.buffer);
  dv.setInt32(1024, 100, true);
  dv.setInt32(2048, 100, true);
  dv.setInt32(4096, 77, true);
  dv.setFloat32(8192, 3.5, true);
  dv.setBigInt64(9000, -5n, true);
  new Uint8Array(mem.buffer).set(new TextEncoder().encode('hello\0'), 12000);
  // A pointer at 300 to a struct at 16000 whose field +8 holds 4242.
  dv.setUint32(300, 16000, true);
  dv.setInt32(16008, 4242, true);

  const ls = await P.call([mem, mem], { op: 'list' });
  check(ls.length === 1 && ls[0].bytes === 131072 && ls[0].pages === 2 && ls[0].shared === false, 'list: one memory (deduplicated), 2 pages');

  check((await P.call(null, { op: 'read', mem: 0, addr: 1024, type: 'i32' })).value === 100, 'read i32');
  check((await P.call(null, { op: 'read', mem: 0, addr: 9000, type: 'i64' })).value === '-5', 'read i64 as a decimal string');
  check((await P.call(null, { op: 'read', mem: 0, addr: 12000, type: 'utf8' })).value === 'hello', 'read utf8 up to the NUL');
  check(Math.abs((await P.call(null, { op: 'read', mem: 0, addr: 8192, type: 'f32' })).value - 3.5) < 1e-6, 'read f32');
  await P.call(null, { op: 'write', mem: 0, addr: 9000, type: 'u64', value: '18446744073709551615' });
  check(dv.getBigUint64(9000, true) === 18446744073709551615n, 'write u64 from a string');
  await P.call(null, { op: 'write', mem: 0, addr: 12000, type: 'utf16', value: 'hé' });
  check(dv.getUint16(12000, true) === 104 && dv.getUint16(12002, true) === 233, 'write utf16');
  let err = '';
  try { await P.call(null, { op: 'read', mem: 0, addr: 131070, type: 'i32' }); } catch (e) { err = e.message; }
  check(/past the end/.test(err), 'a read past the end is refused (' + err + ')');
  err = '';
  try { await P.call(null, { op: 'write', mem: 0, addr: 0, type: 'i32', value: 1.5 }); } catch (e) { err = e.message; }
  check(/whole number/.test(err), 'a fraction for an integer type is refused');

  // Pointer path.
  const r = await P.call(null, { op: 'read', mem: 0, where: { base: 300, offsets: [8] }, type: 'i32' });
  check(r.addr === 16008 && r.value === 4242, 'pointer path [300]+8 resolves to 16008');

  // Scan: exact 100 → two addresses; change one; next "decreased".
  let s = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 100 });
  check(s.count === 2, 'first scan exact i32 100 finds 2 (' + s.count + ')');
  dv.setInt32(2048, 90, true);
  s = await P.call(null, { op: 'next', scan: s.scan, kind: 'decreased' });
  let rows = (await P.call(null, { op: 'results', scan: s.scan })).rows;
  check(s.count === 1 && rows[0].addr === 2048 && rows[0].value === 90, 'next scan "decreased" keeps 2048');
  dv.setInt32(2048, 80, true);
  s = await P.call(null, { op: 'next', scan: s.scan, kind: 'decreasedBy', value: 10 });
  check(s.count === 1, 'decreasedBy 10');

  // Unknown initial value, then changed / unchanged.
  let u = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'unknown' });
  check(u.snapshot && u.count === 131072 / 4, 'unknown scan takes a snapshot of every aligned i32');
  dv.setInt32(4096, 78, true);
  u = await P.call(null, { op: 'next', scan: u.scan, kind: 'increased' });
  rows = (await P.call(null, { op: 'results', scan: u.scan })).rows;
  check(u.count === 1 && rows[0].addr === 4096 && rows[0].previous === 78, 'then "increased" finds the one value that grew');

  // Range scan of floats, unaligned scan.
  const f = await P.call(null, { op: 'scan', mem: 0, type: 'f32', kind: 'range', value: 3.4, to: 3.6 });
  check(f.count === 1, 'f32 range scan');
  const una = await P.call(null, { op: 'scan', mem: 0, type: 'u8', kind: 'exact', value: 0x92 });
  check(una.count >= 1, 'u8 scan (alignment 1)');

  // memory.grow: the buffer is replaced; reads and scans use the new one.
  mem.grow(1);
  const dv2 = new DataView(mem.buffer);
  dv2.setInt32(140000, 100, true);
  check((await P.call(null, { op: 'list' }))[0].pages === 3, 'grow: list shows 3 pages');
  const g = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 100 });
  check(g.count === 2, 'grow: a scan sees the new page (' + g.count + ')');

  // Freeze rewrites the value.
  const fr = await P.call(null, { op: 'freeze', mem: 0, addr: 1024, type: 'i32', value: 999 });
  new DataView(mem.buffer).setInt32(1024, 1, true);
  await new Promise((ok) => setTimeout(ok, 120));
  check(new DataView(mem.buffer).getInt32(1024, true) === 999, 'freeze: the value is written back');
  check((await P.call(null, { op: 'frozen' })).length === 1, 'frozen lists it');
  await P.call(null, { op: 'unfreeze', freeze: fr.freeze });
  new DataView(mem.buffer).setInt32(1024, 1, true);
  await new Promise((ok) => setTimeout(ok, 120));
  check(new DataView(mem.buffer).getInt32(1024, true) === 1, 'unfreeze: the value stays as set');

  // Watch list.
  const w = await P.call(null, { op: 'watch', entries: [{ mem: 0, addr: 4096, type: 'i32' }, { mem: 0, where: { base: 300, offsets: [8] }, type: 'i32' }, { mem: 5, addr: 0, type: 'i32' }] });
  check(w[0].value === 78 && w[1].value === 4242 && /no memory/.test(w[2].error), 'watch reads entries and reports bad ones');

  // Shared memory.
  const sm = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  new DataView(sm.buffer).setInt16(10, -7, true);
  const l2 = await P.call([sm], { op: 'list' });
  check(l2[1] && l2[1].shared === true, 'shared memory listed as shared');
  check((await P.call(null, { op: 'scan', mem: 1, type: 'i16', kind: 'exact', value: -7 })).count === 1, 'shared memory scan');

  // A large memory scans in slices without blocking: 64 MiB.
  const big = new WebAssembly.Memory({ initial: 1024 });
  new DataView(big.buffer).setInt32(64 * 1024 * 1024 - 8, 123456789, true);
  let ticks = 0;
  const timer = setInterval(() => ticks++, 1);
  const t0 = Date.now();
  const b = await P.call([big], { op: 'scan', mem: 2, type: 'i32', kind: 'exact', value: 123456789 });
  clearInterval(timer);
  check(b.count === 1, '64 MiB scan finds the value (' + (Date.now() - t0) + ' ms, the event loop ran ' + ticks + ' times)');

  // The hook finds memories made after it.
  const scope = { WebAssembly: { Memory: WebAssembly.Memory, Instance: WebAssembly.Instance, instantiate: WebAssembly.instantiate }, Promise, Uint8Array, Uint32Array, Float64Array, Int8Array, Int16Array, Uint16Array, Int32Array, Float32Array, DataView, BigInt, BigInt64Array, BigUint64Array, setTimeout, setInterval, clearInterval, performance, TextEncoder, TextDecoder, SharedArrayBuffer };
  const H = make(scope);
  H.hook();
  const made = new scope.WebAssembly.Memory({ initial: 1 });
  check(made instanceof WebAssembly.Memory, 'hooked Memory still makes real memories');
  check((await H.call(null, { op: 'list' })).length === 1, 'the hook found the new memory');
  // A module that exports its memory.
  const wasm = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 5, 3, 1, 0, 1, 7, 7, 1, 3, 109, 101, 109, 2, 0]);
  await scope.WebAssembly.instantiate(wasm);
  check((await H.call(null, { op: 'list' })).length === 2, 'instantiate: an exported memory is found');
  await runInstances(check);
  await runGrow(check, false);
  await runGrow(check, true);
  await runGrowDuringScan(check);
}

// Instances: their exports, which listed memory an exported memory is, globals with type and value.
async function runInstances(check) {
  const { fixtureBytes } = require('./wasm-fixture.cjs');
  const P = make();
  const lone = new WebAssembly.Memory({ initial: 1 });
  const mod = new WebAssembly.Module(new Uint8Array(fixtureBytes()));
  const a = new WebAssembly.Instance(mod), b = new WebAssembly.Instance(mod);
  let sv = await P.call([lone, b.exports.memory, a.exports.memory], { op: 'survey' }, [a, b, a]);
  check(sv.memories.length === 3 && sv.instances.length === 2, 'survey: 3 memories and 2 instances (an instance found twice is one)');
  const ia = sv.instances[0], ib = sv.instances[1];
  const ex = (i, n) => i.exports.find((e) => e.name === n);
  check(ex(ia, 'memory').kind === 'memory' && ex(ia, 'memory').mem === 2 && ex(ib, 'memory').mem === 1, 'an exported memory names the listed memory it is (instance 0: memory 2, instance 1: memory 1)');
  check(!sv.instances.some((i) => i.exports.some((e) => e.mem === 0)), 'a memory no instance exports stays listed, tied to none');
  check(JSON.stringify(ia.counts) === JSON.stringify({ function: 1, memory: 1, table: 1, global: 5, tag: 0, other: 0 }) && ia.omitted === 0, 'the exports are counted by kind (' + JSON.stringify(ia.counts) + ')');
  check(ex(ia, 'f').kind === 'function' && ex(ia, 'f').index === 6 && ex(ia, 'tbl').kind === 'table' && ex(ia, 'tbl').length === 1, 'function and table exports, with their place in the exports');
  check(ia.exports[ia.exports.length - 1].kind === 'function', 'functions are listed last');
  const g = (n) => { const e = ex(ia, n); return [e.kind, e.type, e.mutable, e.value].join(' '); };
  check(g('g_i32') === 'global i32 true 7' && g('g_i64') === 'global i64 true -5' && g('g_f32') === 'global f32 true 1.5' && g('g_f64') === 'global f64 true 2.25',
    'mutable globals: exact type and value, i64 as a decimal string (' + [g('g_i32'), g('g_i64'), g('g_f32'), g('g_f64')].join(', ') + ')');
  check(g('c_i32') === 'global i32 false 1024', 'an immutable global: i32, not mutable, 1024');
  check(a.exports.g_i32.value === 7 && a.exports.g_f32.value === 1.5, 'finding the types wrote nothing');
  sv = await P.call(null, { op: 'survey' });
  check(sv.instances.length === 2, 'the modules the probe links for the type check are not listed as instances');
  check((await P.call(null, { op: 'instances' })).length === 2 && (await P.call(null, { op: 'list' })).length === 3, 'instances and list give the two halves');

  // Globals: read again, write.
  a.exports.g_i32.value = 8;
  let gl = await P.call(null, { op: 'globals', inst: 0 });
  check(gl.length === 5 && gl[0].name === 'g_i32' && gl[0].index === 1 && gl[0].value === 8, 'globals: the current values of an instance\'s globals');
  let w = await P.call(null, { op: 'setGlobal', inst: 0, export: 1, value: 42 });
  check(a.exports.g_i32.value === 42 && b.exports.g_i32.value === 7 && w.value === 42 && w.type === 'i32', 'setGlobal writes a mutable i32 global of that instance only');
  await P.call(null, { op: 'setGlobal', inst: 0, export: 1, value: '0x10' });
  check(a.exports.g_i32.value === 16, 'a value may come as text (0x10)');
  w = await P.call(null, { op: 'setGlobal', inst: 0, export: 2, value: '9007199254740993' });
  check(a.exports.g_i64.value === 9007199254740993n && w.value === '9007199254740993', 'an i64 global from a decimal string, exactly');
  w = await P.call(null, { op: 'setGlobal', inst: 0, export: 3, value: 0.1 });
  check(w.value === Math.fround(0.1) && a.exports.g_f32.value === Math.fround(0.1), 'an f32 global reads back as the f32 it holds');
  await P.call(null, { op: 'setGlobal', inst: 1, export: 4, value: -2.5 });
  check(b.exports.g_f64.value === -2.5, 'an f64 global');
  const refused = async (req) => P.call(null, req).then(() => '', (e) => e.message);
  check(/not mutable/.test(await refused({ op: 'setGlobal', inst: 0, export: 5, value: 1 })) && a.exports.c_i32.value === 1024, 'an immutable global is not written');
  check(/not a global/.test(await refused({ op: 'setGlobal', inst: 0, export: 0, value: 1 })) && /not a global/.test(await refused({ op: 'setGlobal', inst: 0, export: 6, value: 1 }))
    && /not a global/.test(await refused({ op: 'setGlobal', inst: 0, export: 99, value: 1 })), 'a memory, a function or no export at that place: refused');
  check(/whole number/.test(await refused({ op: 'setGlobal', inst: 0, export: 1, value: 1.5 })) && /out of range/.test(await refused({ op: 'setGlobal', inst: 0, export: 1, value: 4294967296 }))
    && /whole number/.test(await refused({ op: 'setGlobal', inst: 0, export: 2, value: 'abc' })) && /not a number/.test(await refused({ op: 'setGlobal', inst: 0, export: 4, value: 'abc' })), 'values that do not fit the global\'s type are refused');
  check(/no instance 9/.test(await refused({ op: 'globals', inst: 9 })), 'an instance that is not listed: no instance 9');

  // Caps: many exports, long names.
  const big = { exports: {} };
  Object.setPrototypeOf(big, WebAssembly.Instance.prototype);
  for (let i = 0; i < 2500; i++) big.exports['fn' + i] = function () {};
  big.exports['x'.repeat(500)] = a.exports.g_i32;
  const capped = (await P.call(null, { op: 'instances' }, [big]))[2];
  check(capped.exports.length === 2000 && capped.omitted === 501 && capped.counts.function === 2500 && capped.exports[0].kind === 'global' && capped.exports[0].name.length === 201,
    'at most 2000 exports are listed per instance (globals first, the rest counted), names cut to 200 characters');

  // The hook: instances made after it, by the constructor and by instantiate.
  const scope = { WebAssembly: { Memory: WebAssembly.Memory, Instance: WebAssembly.Instance, Module: WebAssembly.Module, Global: WebAssembly.Global, Table: WebAssembly.Table, Tag: WebAssembly.Tag,
    instantiate: WebAssembly.instantiate }, Promise, Uint8Array, Uint32Array, Float64Array, Int8Array, Int16Array, Uint16Array, Int32Array, Float32Array, DataView, BigInt, BigInt64Array, BigUint64Array,
    setTimeout, setInterval, clearInterval, performance, TextEncoder, TextDecoder, SharedArrayBuffer, WeakSet, WeakMap };
  const H = make(scope);
  H.hook();
  const viaNew = new scope.WebAssembly.Instance(mod);
  check(viaNew instanceof WebAssembly.Instance, 'hooked Instance still makes real instances');
  await scope.WebAssembly.instantiate(new Uint8Array(fixtureBytes()));
  await scope.WebAssembly.instantiate(mod);
  const hs = await H.call(null, { op: 'survey' });
  check(hs.instances.length === 3 && hs.memories.length === 3 && hs.instances.every((i, n) => i.exports.find((e) => e.kind === 'memory').mem === n),
    'the hook finds instances from new Instance, instantiate(bytes) and instantiate(module), each with its memory');
  check(hs.instances[0].exports.find((e) => e.name === 'g_f64').type === 'f64', 'and their globals');
  // A second probe in a hooked scope (the host's, after a userscript's): still the engine's constructor.
  const H2 = make(scope);
  const h2 = await H2.call(null, { op: 'instances' }, [viaNew]);
  check(h2[0].exports.find((e) => e.name === 'g_i64').type === 'i64' && (await H.call(null, { op: 'instances' })).length === 3, 'a second probe\'s type checks do not show up as instances of the hooked one');
  // Where WebAssembly can't be compiled (a content security policy): what the values say.
  const noCompile = Object.assign({}, scope, { WebAssembly: Object.assign({}, scope.WebAssembly, { Module: function () { throw new Error('blocked'); } }) });
  const N = make(noCompile);
  const ng = (await N.call(null, { op: 'instances' }, [a]))[0].exports;
  check(ng.find((e) => e.name === 'g_i64').type === 'i64' && ng.find((e) => e.name === 'g_f64').type === 'number' && ng.find((e) => e.name === 'g_f64').mutable === null && ng.find((e) => e.name === 'g_f64').value === 2.25,
    'without compiling: i64 or "number", mutability unknown, the value all the same');
}

// memory.grow: a memory the probe holds stays usable in every operation; what was found before
// the growth stays valid; addresses past the old size become valid.
async function runGrow(check, shared) {
  const tag = shared ? 'grow (shared): ' : 'grow: ';
  const P = make();
  const mem = new WebAssembly.Memory({ initial: 1, maximum: 8, shared: shared });
  const dv = () => new DataView(mem.buffer);
  const OLD = 65536, FAR = OLD + 4096; // FAR is past the end until the memory grows
  dv().setInt32(1024, 100, true);
  dv().setInt32(2048, 100, true);
  dv().setUint32(300, FAR, true); // a pointer to where there is no memory yet
  await P.call([mem], { op: 'list' });
  const refused = async (req) => P.call(null, req).then(() => '', (e) => e.message);
  check(/past the end/.test(await refused({ op: 'read', mem: 0, addr: FAR, type: 'i32' })) && /past the end/.test(await refused({ op: 'read', mem: 0, where: { base: 300, offsets: [8] }, type: 'i32' })),
    tag + 'before: an address past the end is refused, directly and through a pointer');
  const s = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 100 });
  const u = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'unknown' });
  const fr = await P.call(null, { op: 'freeze', mem: 0, addr: 2048, type: 'i32', value: 555 });
  const old = mem.buffer;

  mem.grow(2);
  check(mem.buffer !== old && mem.buffer.byteLength === 3 * OLD && (shared || old.byteLength === 0), tag + 'the memory has a new buffer of 3 pages' + (shared ? '' : ' (the old one is detached)'));
  const l = await P.call(null, { op: 'list' });
  check(l.length === 1 && l[0].pages === 3 && l[0].bytes === 3 * OLD && l[0].shared === shared, tag + 'list: the same memory number, 3 pages');
  check((await P.call(null, { op: 'read', mem: 0, addr: 1024, type: 'i32' })).value === 100, tag + 'read: an old address holds its value');
  await P.call(null, { op: 'write', mem: 0, addr: FAR + 8, type: 'i32', value: 4242 });
  check(dv().getInt32(FAR + 8, true) === 4242 && (await P.call(null, { op: 'read', mem: 0, addr: FAR + 8, type: 'i32' })).value === 4242, tag + 'write and read past the old size');
  const viaPtr = await P.call(null, { op: 'read', mem: 0, where: { base: 300, offsets: [8] }, type: 'i32' });
  check(viaPtr.addr === FAR + 8 && viaPtr.value === 4242, tag + 'a pointer path into the new pages resolves');
  await P.call(null, { op: 'writeBytes', mem: 0, addr: 3 * OLD - 2, bytes: [0xAB, 0xCD] });
  const hx = await P.call(null, { op: 'bytes', mem: 0, addr: 3 * OLD - 16, length: 256 });
  check(hx.size === 3 * OLD && hx.bytes.length === 16 && hx.bytes[14] === 0xAB && hx.bytes[15] === 0xCD, tag + 'hex window: the last bytes of the grown memory, read and written');
  await P.call(null, { op: 'write', mem: 0, addr: 3 * OLD - 16, type: 'utf8', value: 'grown' });
  check((await P.call(null, { op: 'read', mem: 0, addr: 3 * OLD - 16, type: 'utf8', length: 5 })).value === 'grown', tag + 'text in the new pages');

  // The result list from before the growth.
  let rows = (await P.call(null, { op: 'results', scan: s.scan })).rows;
  check(rows.length === 2 && rows[0].addr === 1024 && rows[0].value === 100 && rows[1].addr === 2048 && rows[1].value === 555, tag + 'the result list from before shows the values of now');
  dv().setInt32(1024, 90, true);
  const n = await P.call(null, { op: 'next', scan: s.scan, kind: 'decreased' });
  rows = (await P.call(null, { op: 'results', scan: s.scan })).rows;
  check(n.count === 1 && rows[0].addr === 1024 && rows[0].value === 90 && rows[0].previous === 90, tag + 'a next scan on it compares against the values before the growth');
  const un = await P.call(null, { op: 'next', scan: u.scan, kind: 'changed' });
  const urows = (await P.call(null, { op: 'results', scan: u.scan })).rows.map((r) => r.addr);
  check(un.count === 2 && urows[0] === 1024 && urows[1] === 2048, tag + 'a snapshot from before, then "changed": the two values that changed (' + urows + ')');

  // Freezes: the one from before goes on in the new buffer; a new one past the old size.
  dv().setInt32(2048, 1, true);
  const far = await P.call(null, { op: 'freeze', mem: 0, where: { base: 300, offsets: [8] }, type: 'i32', value: 777 });
  dv().setInt32(FAR + 8, 2, true);
  await new Promise((ok) => setTimeout(ok, 130));
  check(dv().getInt32(2048, true) === 555, tag + 'a freeze from before is still written back');
  check(far.addr === FAR + 8 && dv().getInt32(FAR + 8, true) === 777, tag + 'a freeze through a pointer into the new pages');
  mem.grow(1);
  dv().setInt32(2048, 1, true);
  dv().setInt32(FAR + 8, 2, true);
  await new Promise((ok) => setTimeout(ok, 130));
  const fz = await P.call(null, { op: 'frozen' });
  check(dv().getInt32(2048, true) === 555 && dv().getInt32(FAR + 8, true) === 777 && fz.length === 2 && fz.every((f) => !f.error), tag + 'and both after a second growth, without errors');
  await P.call(null, { op: 'unfreeze', freeze: fr.freeze });
  await P.call(null, { op: 'unfreeze', freeze: far.freeze });

  // Watch entries and a new scan.
  const w = await P.call(null, { op: 'watch', entries: [{ mem: 0, addr: 1024, type: 'i32' }, { mem: 0, addr: FAR + 8, type: 'i32' }, { mem: 0, where: { base: 300, offsets: [8] }, type: 'i32' }, { mem: 0, addr: 4 * OLD, type: 'u8' }] });
  check(w[0].value === 90 && w[1].value === 777 && w[2].addr === FAR + 8 && /past the end/.test(w[3].error), tag + 'watch: old and new addresses, and the new end');
  const g = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 777 });
  check(g.count === 1 && g.scannedBytes === 4 * OLD, tag + 'a new scan covers all 4 pages');
}

// The page grows the memory while a scan has given it time (scans work in slices).
async function runGrowDuringScan(check) {
  for (const shared of [false, true]) {
    const mem = new WebAssembly.Memory({ initial: 4, maximum: 16, shared: shared });
    let yields = 0, clock = 0;
    // A clock that makes every slice end at once, and a page that grows the memory each time it gets to run.
    const scope = Object.assign({}, globalThis, { WebAssembly, Promise, Uint8Array, Uint32Array, Float64Array, Int8Array, Int16Array, Uint16Array, Int32Array, Float32Array, DataView, BigInt,
      BigInt64Array, BigUint64Array, TextEncoder, TextDecoder, SharedArrayBuffer, WeakSet, WeakMap, setInterval, clearInterval, MessageChannel: undefined,
      performance: { now: () => (clock += 100) },
      setTimeout: (f, ms) => setTimeout(() => { yields++; if (mem.buffer.byteLength < 12 * 65536) mem.grow(1); f(); }, ms) });
    const P = make(scope);
    const END = 4 * 65536 - 8;
    new DataView(mem.buffer).setInt32(END, 123456789, true);
    new DataView(mem.buffer).setInt32(1001, 123456789, true); // not aligned
    await P.call([mem], { op: 'list' });
    const tag = 'grow during a scan' + (shared ? ' (shared): ' : ': ');
    yields = 0;
    const a = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 123456789 });
    const ar = (await P.call(null, { op: 'results', scan: a.scan })).rows;
    check(yields >= 1 && a.count === 1 && ar[0].addr === END && ar[0].value === 123456789, tag + 'the aligned scan goes on in the new buffer and finds the value at the end (' + a.count + ' found, ' + yields + ' pauses)');
    yields = 0;
    const before = mem.buffer.byteLength;
    const b = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'exact', value: 123456789, align: 1 });
    check(yields >= 1 && b.count === 2 && b.scannedBytes === before && mem.buffer.byteLength > before, tag + 'the byte-wise scan too, over the size it started with (' + b.count + ' found)');
    yields = 0;
    const u = await P.call(null, { op: 'scan', mem: 0, type: 'i32', kind: 'unknown' });
    new DataView(mem.buffer).setInt32(END, 5, true);
    const un = await P.call(null, { op: 'next', scan: u.scan, kind: 'changed' });
    check(yields >= 1 && un.count === 1, tag + 'the next scan after a snapshot too (' + un.count + ' changed, ' + yields + ' pauses)');
  }
}

module.exports = function ({ check }) {
  return { testWasmProbe: async function () { console.log('0.19.0: WASM probe (Node)'); await run(check); } };
};

if (require.main === module) {
  let pass = 0, fail = 0;
  const check = (c, n) => { if (c) { pass++; console.log('  ok   ' + n); } else { fail++; console.log('  FAIL ' + n); } };
  run(check).then(() => { console.log(`\n${pass} passed, ${fail} failed`); process.exit(fail ? 1 : 0); }, (e) => { console.error(e); process.exit(2); });
}
