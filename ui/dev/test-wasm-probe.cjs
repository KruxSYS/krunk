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

async function run(check) {
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
}

module.exports = function ({ check }) {
  return { testWasmProbe: async function () { console.log('0.19.0: WASM probe (Node)'); await run(check); } };
};

if (require.main === module) {
  let pass = 0, fail = 0;
  const check = (c, n) => { if (c) { pass++; console.log('  ok   ' + n); } else { fail++; console.log('  FAIL ' + n); } };
  run(check).then(() => { console.log(`\n${pass} passed, ${fail} failed`); process.exit(fail ? 1 : 0); }, (e) => { console.error(e); process.exit(2); });
}
