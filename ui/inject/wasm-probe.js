// Krunk's WASM memory inspector, the part that runs next to the memories (0.19.0).
//
// A function expression: `(<this file>)(scope)` makes one inspector instance. `scope` is
// `globalThis` in a page (the host calls the expression with `Runtime.callFunctionOn`; the
// instance stays an object of that call, which no page script can reach), or the userscript
// loader's scope. Built-ins are taken when the instance is made.
//
// The instance has one entry point, `call(found, req)`, which always returns a Promise of
// something JSON can carry:
// - `found`: `WebAssembly.Memory` objects found since the last call (from `Runtime.queryObjects`,
//   or from the document-start hook below). They join the instance's registry, where each
//   memory gets a stable number; requests name memories by that number.
//   `WebAssembly.Instance` objects (the third argument `more`, or mixed into `found`) join a
//   second registry the same way; the memories an instance exports are adopted with it.
// - `req.op`: list, instances, survey (both), globals, setGlobal, read, write, bytes,
//   writeBytes, scan, next, results, discard, freeze, unfreeze, frozen, watch, resolve.
//
// Values: i8 u8 i16 u16 i32 u32 f32 f64 as numbers; i64 u64 as decimal strings (JSON has no
// 64-bit integers); bytes as arrays of 0..255; utf8 and utf16 as strings. Little-endian, as
// WebAssembly is. `memory.grow` replaces a memory's buffer: every operation reads `mem.buffer`
// again, and a scan that gave the page time in between takes the buffer again before it goes on.
// Shared memories (SharedArrayBuffer) work the same way.
//
// Instances: each export has a name, a kind (function, memory, table, global, tag) and its place
// in the exports (`index`; requests name an exported global by instance number and index, never
// by a property name). An exported memory says which listed memory it is (`mem`). An exported
// global has its type (i32 i64 f32 f64 v128 funcref externref), whether it is mutable, and its
// value (i64 as a decimal string). JavaScript does not say a global's type: the probe finds it
// by linking the global into one-import modules it compiles itself (a wrong type is a
// LinkError); nothing is written to the global for that.
//
// `hook()` (userscripts; the host's fallback): wraps WebAssembly.Memory, instantiate,
// instantiateStreaming and Instance so memories and instances made from then on are found
// without DevTools.
(function (scope) {
  'use strict';
  const G = scope || globalThis;
  const WA = G.WebAssembly;
  const P = G.Promise, U8 = G.Uint8Array, U32 = G.Uint32Array, F64 = G.Float64Array, DV = G.DataView;
  const BI = G.BigInt, BI64 = G.BigInt64Array, BU64 = G.BigUint64Array;
  const later = G.setTimeout.bind(G), every = G.setInterval.bind(G), stop = G.clearInterval.bind(G);
  const now = G.performance && G.performance.now ? G.performance.now.bind(G.performance) : Date.now;
  const TE = G.TextEncoder ? new G.TextEncoder() : null;
  const TD8 = G.TextDecoder ? new G.TextDecoder('utf-8', { fatal: false }) : null;
  const TD16 = G.TextDecoder ? new G.TextDecoder('utf-16le', { fatal: false }) : null;
  const isArr = Array.isArray;

  // Limits (the host's and the agent tools' descriptions say the same).
  const LIMITS = Object.freeze({
    maxScanBytes: 512 * 1024 * 1024, // a memory larger than this is scanned in its first 512 MiB
    maxResults: 4 * 1024 * 1024, // a scan keeps at most this many addresses
    maxScans: 8, // older scans are dropped
    maxReadBytes: 64 * 1024, // bytes / utf8 / utf16 reads and writes, and the hex view
    maxPage: 1000, // results per page
    maxFreezes: 256,
    maxWatch: 256,
    maxPointerDepth: 8,
    maxInstances: 256, // instances kept (the first ones found)
    maxExports: 2000, // exports listed per instance (memories, globals, tables and tags first)
    maxNameChars: 200, // an export's name is cut to this
    sliceMs: 30, // a scan yields to the page after this much work
  });

  const TYPES = {
    i8: { size: 1, get: 'getInt8', set: 'setInt8' },
    u8: { size: 1, get: 'getUint8', set: 'setUint8' },
    i16: { size: 2, get: 'getInt16', set: 'setInt16' },
    u16: { size: 2, get: 'getUint16', set: 'setUint16' },
    i32: { size: 4, get: 'getInt32', set: 'setInt32' },
    u32: { size: 4, get: 'getUint32', set: 'setUint32' },
    i64: { size: 8, get: 'getBigInt64', set: 'setBigInt64', big: true },
    u64: { size: 8, get: 'getBigUint64', set: 'setBigUint64', big: true },
    f32: { size: 4, get: 'getFloat32', set: 'setFloat32', float: true },
    f64: { size: 8, get: 'getFloat64', set: 'setFloat64', float: true },
  };

  function fail(msg) { throw new Error(msg); }

  // ---------------------------------------------------------------- registry
  const mems = []; // index = memory number
  const insts = []; // index = instance number
  // The constructors as they were when the probe was made (the hook replaces them later).
  // (Another probe's hook may already be there: its `Instance` shares the prototype, whose
  // `constructor` is still the engine's.)
  const protoCtor = function (C) { try { const c = C.prototype.constructor; return typeof c === 'function' && c.prototype === C.prototype ? c : C; } catch (_) { return C; } };
  const RealInstance = WA && WA.Instance && protoCtor(WA.Instance), RealModule = WA && WA.Module, RealGlobal = WA && WA.Global;
  const RealMemoryC = WA && WA.Memory, RealTable = WA && WA.Table, RealTag = WA && WA.Tag;
  const own = G.WeakSet ? new G.WeakSet() : null; // the instances the probe makes itself (type checks)
  function isA(x, C) { try { return typeof C === 'function' && x instanceof C; } catch (_) { return false; } }
  function adoptMem(m) {
    if (!m || mems.indexOf(m) >= 0) return;
    try { if (!(m.buffer instanceof ArrayBuffer) && !(G.SharedArrayBuffer && m.buffer instanceof G.SharedArrayBuffer)) return; } catch (_) { return; }
    mems.push(m);
  }
  function adoptInst(x) {
    if (insts.indexOf(x) >= 0 || (own && own.has(x))) return;
    let ex;
    try { ex = x.exports; } catch (_) { return; }
    if (!ex || typeof ex !== 'object') return;
    if (insts.length < LIMITS.maxInstances) insts.push(x);
    // Its memories are listed even where the memory query found nothing (the hook).
    try { for (const k in ex) if (isA(ex[k], RealMemoryC)) adoptMem(ex[k]); } catch (_) { /* odd exports */ }
  }
  function adopt(found) {
    if (!found) return;
    const list = isArr(found) ? found : [found];
    for (let i = 0; i < list.length; i++) {
      const x = list[i];
      if (!x) continue;
      if (isA(x, RealInstance)) adoptInst(x);
      else adoptMem(x);
    }
  }
  function mem(n) {
    const m = mems[n];
    if (!m) fail('no memory ' + n + ' (list the memories first)');
    return m;
  }
  function view(m) { const b = m.buffer; return { b: b, dv: new DV(b), u8: new U8(b), len: b.byteLength }; }
  function isShared(m) { return !!(G.SharedArrayBuffer && m.buffer instanceof G.SharedArrayBuffer); }

  function list() {
    const out = [];
    for (let i = 0; i < mems.length; i++) {
      const len = mems[i].buffer.byteLength;
      out.push({ mem: i, bytes: len, pages: len / 65536, shared: isShared(mems[i]) });
    }
    return out;
  }

  // ---------------------------------------------------------------- instances
  // A global's type and mutability: the one-import module `(import "m" "g" (global <mut> <type>))`
  // links only with a global of exactly that type.
  const GLOBAL_TYPES = [['i32', 0x7f], ['i64', 0x7e], ['f32', 0x7d], ['f64', 0x7c], ['v128', 0x7b], ['funcref', 0x70], ['externref', 0x6f]];
  const typeMods = {};
  const globalInfo = G.WeakMap ? new G.WeakMap() : null;
  function links(g, code, mut) {
    const key = code * 2 + mut;
    let mod = typeMods[key];
    if (mod === undefined) {
      try { mod = new RealModule(new U8([0, 97, 115, 109, 1, 0, 0, 0, 2, 8, 1, 1, 109, 1, 103, 3, code, mut])); } catch (_) { mod = null; }
      typeMods[key] = mod;
    }
    if (!mod) return null; // WebAssembly can't be compiled here (a content security policy)
    try { const i = new RealInstance(mod, { m: { g: g } }); if (own) own.add(i); return true; } catch (_) { return false; }
  }
  function describeGlobal(g) {
    let info = globalInfo && globalInfo.get(g);
    if (info) return info;
    info = null;
    for (let t = 0; t < GLOBAL_TYPES.length && !info; t++) {
      for (let mut = 0; mut < 2; mut++) {
        const ok = links(g, GLOBAL_TYPES[t][1], mut);
        if (ok === null) { t = GLOBAL_TYPES.length; break; }
        if (ok) { info = { type: GLOBAL_TYPES[t][0], mutable: mut === 1 }; break; }
      }
    }
    if (!info) {
      // No check by linking: what the value itself says. `mutable: null` = not known.
      let v;
      try { v = g.value; } catch (_) { v = undefined; }
      info = { type: typeof v === 'bigint' ? 'i64' : typeof v === 'number' ? 'number' : 'other', mutable: null };
    }
    if (globalInfo) globalInfo.set(g, info);
    return info;
  }
  function globalValue(g, info) {
    if (info.type !== 'i32' && info.type !== 'i64' && info.type !== 'f32' && info.type !== 'f64' && info.type !== 'number') return null;
    let v;
    try { v = g.value; } catch (_) { return null; }
    if (typeof v === 'bigint') return v.toString();
    if (typeof v !== 'number') return null;
    return isFinite(v) ? v : String(v); // NaN and the infinities as text (JSON has none)
  }
  function cut(name) { const s = String(name); return s.length > LIMITS.maxNameChars ? s.slice(0, LIMITS.maxNameChars) + '…' : s; }
  function kindOf(x) {
    if (typeof x === 'function') return 'function';
    if (isA(x, RealMemoryC)) return 'memory';
    if (isA(x, RealGlobal)) return 'global';
    if (isA(x, RealTable)) return 'table';
    if (isA(x, RealTag)) return 'tag';
    return 'other';
  }
  function inst(n) {
    const x = insts[n];
    if (!x) fail('no instance ' + n + ' (list the memories first)');
    return x;
  }
  // The exports in their order: [name, value].
  function exportsOf(x) {
    const ex = x.exports, names = Object.keys(ex), list = [];
    for (let i = 0; i < names.length; i++) list.push([names[i], ex[names[i]]]);
    return list;
  }
  function describeExport(e, index) {
    const kind = kindOf(e[1]);
    const row = { index: index, name: cut(e[0]), kind: kind };
    if (kind === 'memory') {
      const n = mems.indexOf(e[1]);
      if (n >= 0) row.mem = n;
    } else if (kind === 'global') {
      const info = describeGlobal(e[1]);
      row.type = info.type; row.mutable = info.mutable; row.value = globalValue(e[1], info);
    } else if (kind === 'table') {
      try { row.length = e[1].length; } catch (_) { /* no length */ }
    }
    return row;
  }
  function instances() {
    const out = [];
    for (let n = 0; n < insts.length; n++) {
      const row = { inst: n, counts: { function: 0, memory: 0, table: 0, global: 0, tag: 0, other: 0 }, exports: [], omitted: 0 };
      try {
        const list = exportsOf(insts[n]);
        const funcs = [];
        for (let i = 0; i < list.length; i++) {
          const kind = kindOf(list[i][1]);
          row.counts[kind]++;
          if (kind === 'function') funcs.push(i);
          else if (row.exports.length < LIMITS.maxExports) row.exports.push(describeExport(list[i], i));
          else row.omitted++;
        }
        for (let j = 0; j < funcs.length; j++) {
          if (row.exports.length < LIMITS.maxExports) row.exports.push({ index: funcs[j], name: cut(list[funcs[j]][0]), kind: 'function' });
          else row.omitted++;
        }
      } catch (e) { row.error = String(e && e.message || e); }
      out.push(row);
    }
    return out;
  }
  function globals(req) {
    const list = exportsOf(inst(req.inst)), out = [];
    for (let i = 0; i < list.length && out.length < LIMITS.maxExports; i++) {
      if (kindOf(list[i][1]) === 'global') out.push(describeExport(list[i], i));
    }
    return out;
  }
  function setGlobal(req) {
    const list = exportsOf(inst(req.inst));
    const e = list[req.export];
    if (typeof req.export !== 'number' || !e || kindOf(e[1]) !== 'global') fail('export ' + req.export + ' of instance ' + req.inst + ' is not a global');
    const g = e[1], info = describeGlobal(g);
    if (info.mutable === false) fail('the global ' + cut(e[0]) + ' is not mutable');
    let raw;
    const text = typeof req.value === 'number' ? req.value : String(req.value).trim();
    if (info.type === 'i64') {
      try { raw = BI(typeof text === 'number' ? Math.trunc(text) : text); } catch (_) { fail('not a whole number: ' + req.value); }
      if (raw < -(BI(1) << BI(63)) || raw >= (BI(1) << BI(64))) fail(req.value + ' is out of range for i64');
    } else if (info.type === 'i32' || info.type === 'f32' || info.type === 'f64' || info.type === 'number') {
      raw = typeof text === 'number' ? text : (text === '' ? NaN : Number(text));
      if (info.type === 'i32') {
        if (!isFinite(raw) || Math.floor(raw) !== raw) fail('not a whole number: ' + req.value);
        if (raw < -2147483648 || raw > 4294967295) fail(req.value + ' is out of range for i32');
      } else if (raw !== raw && text !== 'NaN') fail('not a number: ' + req.value);
    } else fail('a ' + info.type + ' global can\'t be written from here');
    try { g.value = raw; } catch (err) { fail('the global ' + cut(e[0]) + ' can\'t be written: ' + String(err && err.message || err)); }
    return { inst: req.inst, export: req.export, type: info.type, value: globalValue(g, info) };
  }

  // ---------------------------------------------------------------- values
  function typeOf(t) { const d = TYPES[t]; if (!d) fail('unknown type ' + t); return d; }
  function checkAddr(v, addr, size) {
    if (typeof addr !== 'number' || !(addr >= 0) || Math.floor(addr) !== addr) fail('the address must be a whole number ≥ 0');
    if (addr + size > v.len) fail('address ' + addr + ' + ' + size + ' is past the end of the memory (' + v.len + ' bytes)');
  }
  // A request value as what the DataView setter takes.
  function toRaw(d, value) {
    if (d.big) {
      try { return BI(typeof value === 'number' ? Math.trunc(value) : String(value).trim()); } catch (_) { fail('not a whole number: ' + value); }
    }
    const n = typeof value === 'number' ? value : Number(String(value).trim());
    if (!isFinite(n) && !d.float) fail('not a number: ' + value);
    if (!d.float && Math.floor(n) !== n) fail('not a whole number: ' + value);
    return n;
  }
  function out(d, raw) { return d.big ? raw.toString() : raw; }
  function readAt(v, addr, t, length) {
    if (t === 'bytes' || t === 'utf8' || t === 'utf16') {
      const n = length === undefined ? 16 : length;
      if (typeof n !== 'number' || n < 0 || n > LIMITS.maxReadBytes) fail('length must be 0..' + LIMITS.maxReadBytes);
      checkAddr(v, addr, n);
      const s = v.u8.slice(addr, addr + n);
      if (t === 'bytes') return Array.prototype.slice.call(s);
      if (t === 'utf8') { const z = s.indexOf(0); return TD8.decode(z >= 0 && length === undefined ? s.subarray(0, z) : s); }
      return TD16.decode(s.subarray(0, s.length - (s.length & 1)));
    }
    const d = typeOf(t);
    checkAddr(v, addr, d.size);
    return out(d, v.dv[d.get](addr, true));
  }
  function writeAt(v, addr, t, value) {
    if (t === 'bytes' || t === 'utf8' || t === 'utf16') {
      let bytes;
      if (t === 'bytes') {
        if (!isArr(value)) fail('bytes must be an array of 0..255');
        bytes = new U8(value.length);
        for (let i = 0; i < value.length; i++) {
          const b = value[i];
          if (typeof b !== 'number' || b < 0 || b > 255 || Math.floor(b) !== b) fail('bytes must be an array of 0..255');
          bytes[i] = b;
        }
      } else if (t === 'utf8') {
        bytes = TE.encode(String(value));
      } else {
        const s = String(value);
        bytes = new U8(s.length * 2);
        for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); bytes[2 * i] = c & 255; bytes[2 * i + 1] = c >> 8; }
      }
      if (bytes.length > LIMITS.maxReadBytes) fail('at most ' + LIMITS.maxReadBytes + ' bytes at once');
      checkAddr(v, addr, bytes.length);
      v.u8.set(bytes, addr);
      return bytes.length;
    }
    const d = typeOf(t);
    checkAddr(v, addr, d.size);
    v.dv[d.set](addr, toRaw(d, value), true);
    return d.size;
  }

  // ---------------------------------------------------------------- pointers
  // base, then for each offset: read a 32-bit pointer at the address, add the offset.
  // `{base: 1024, offsets: [8, 4]}` = [[1024] + 8] + 4.
  function resolve(v, where) {
    if (typeof where === 'number') return where;
    if (!where || typeof where.base !== 'number') fail('an address, or {base, offsets}');
    const offs = where.offsets || [];
    if (!isArr(offs) || offs.length > LIMITS.maxPointerDepth) fail('at most ' + LIMITS.maxPointerDepth + ' offsets');
    let a = where.base;
    for (let i = 0; i < offs.length; i++) {
      checkAddr(v, a, 4);
      a = v.dv.getUint32(a, true) + (offs[i] | 0);
    }
    return a;
  }

  // ---------------------------------------------------------------- scans
  const scans = new Map();
  let scanSeq = 0;
  // Elements of type `d` read from a fresh buffer: a fast typed view when the alignment allows.
  function values(v, d, align, limit) {
    const len = Math.min(v.len, limit);
    if (align === d.size) {
      const n = Math.floor(len / d.size);
      switch (d.get) {
        case 'getInt8': return { arr: new G.Int8Array(v.b, 0, n), step: 1 };
        case 'getUint8': return { arr: new U8(v.b, 0, n), step: 1 };
        case 'getInt16': return { arr: new G.Int16Array(v.b, 0, n), step: 2 };
        case 'getUint16': return { arr: new G.Uint16Array(v.b, 0, n), step: 2 };
        case 'getInt32': return { arr: new G.Int32Array(v.b, 0, n), step: 4 };
        case 'getUint32': return { arr: new U32(v.b, 0, n), step: 4 };
        case 'getFloat32': return { arr: new G.Float32Array(v.b, 0, n), step: 4 };
        case 'getFloat64': return { arr: new F64(v.b, 0, n), step: 8 };
        case 'getBigInt64': return { arr: new BI64(v.b, 0, n), step: 8 };
        case 'getBigUint64': return { arr: new BU64(v.b, 0, n), step: 8 };
      }
    }
    return null;
  }
  function matcher(d, kind, value, value2, eps) {
    const raw = function (x) { return toRaw(d, x); };
    const near = d.float ? function (x, v) { return Math.abs(x - v) <= eps; } : function (x, v) { return x === v; };
    switch (kind) {
      case 'exact': { const v = raw(value); return function (x) { return near(x, v); }; }
      case 'range': {
        const lo = raw(value), hi = raw(value2);
        if (lo > hi) fail('the range is empty (from > to)');
        return function (x) { return x >= lo && x <= hi; };
      }
      case 'bigger': { const v = raw(value); return function (x) { return x > v; }; }
      case 'smaller': { const v = raw(value); return function (x) { return x < v; }; }
      case 'unknown': return null;
      default: fail('unknown first-scan kind ' + kind + ' (exact, range, bigger, smaller, unknown)');
    }
  }
  function epsilonFor(d, req) {
    if (!d.float) return 0;
    if (typeof req.epsilon === 'number' && req.epsilon >= 0) return req.epsilon;
    return d.get === 'getFloat32' ? 1e-3 : 1e-9;
  }
  // A scan yields through a message channel where there is one: a hidden tab's timers are
  // throttled (one per second and worse), and the inspector's own tab hides the inspected one.
  const MC = G.MessageChannel;
  function yieldNow() {
    return new P(function (ok) {
      if (!MC) return later(ok, 0);
      const ch = new MC();
      ch.port1.onmessage = function () { ch.port1.close(); ok(); };
      ch.port2.postMessage(0);
    });
  }

  async function firstScan(req) {
    const m = mem(req.mem);
    const t = req.type;
    const d = typeOf(t);
    const align = req.align === undefined ? d.size : req.align;
    if ([1, 2, 4, 8].indexOf(align) < 0 || align > d.size) fail('alignment must be 1, 2, 4 or 8 and at most the type size');
    const eps = epsilonFor(d, req);
    const test = matcher(d, req.kind || 'exact', req.value, req.to, eps);
    const id = ++scanSeq;
    const s = { id: id, mem: req.mem, type: t, align: align, eps: eps, addrs: null, prev: null, snapshot: null, count: 0, done: false, progress: 0, truncated: false, scanned: 0 };
    scans.set(id, s);
    if (scans.size > LIMITS.maxScans) scans.delete(scans.keys().next().value);
    const v = view(m);
    const limit = Math.min(v.len, LIMITS.maxScanBytes);
    s.scanned = limit;
    if (!test) {
      // Unknown initial value: a snapshot; the next scan compares against it.
      s.snapshot = v.u8.slice(0, limit);
      s.count = Math.floor((limit - d.size) / align) + 1;
      s.done = true; s.progress = 1;
      return summary(s);
    }
    const addrs = [];
    let prev = [];
    // While the scan gives the page time, the page may grow the memory: a memory that is not
    // shared then has a new buffer and the old one is detached. The scan goes on in the new
    // one, up to the size it started with.
    let fast = values(v, d, align, limit);
    let t0 = now();
    if (fast) {
      let arr = fast.arr;
      const n = arr.length;
      for (let i = 0; i < n; i++) {
        const x = arr[i];
        if (test(x)) {
          if (addrs.length >= LIMITS.maxResults) { s.truncated = true; break; }
          addrs.push(i * fast.step); prev.push(x);
        }
        if ((i & 0xfffff) === 0 && now() - t0 > LIMITS.sliceMs) {
          s.progress = i / n; await yieldNow(); t0 = now();
          if (m.buffer !== arr.buffer) { fast = values(view(m), d, align, limit); arr = fast.arr; }
        }
      }
    } else {
      let dv = v.dv;
      const get = d.get;
      const end = limit - d.size;
      for (let a = 0; a <= end; a += align) {
        const x = dv[get](a, true);
        if (test(x)) {
          if (addrs.length >= LIMITS.maxResults) { s.truncated = true; break; }
          addrs.push(a); prev.push(x);
        }
        if ((a & 0xfffff) === 0 && now() - t0 > LIMITS.sliceMs) {
          s.progress = a / end; await yieldNow(); t0 = now();
          if (m.buffer !== dv.buffer) dv = new DV(m.buffer);
        }
      }
    }
    s.addrs = new U32(addrs);
    s.prev = d.big ? prev : (d.float ? new F64(prev) : new F64(prev));
    s.count = addrs.length;
    s.done = true; s.progress = 1;
    return summary(s);
  }
  function summary(s) {
    return { scan: s.id, mem: s.mem, type: s.type, count: s.count, truncated: s.truncated, scannedBytes: s.scanned, snapshot: !!s.snapshot };
  }
  function scanOf(id) { const s = scans.get(id); if (!s) fail('no scan ' + id + ' (it ended, or more than ' + LIMITS.maxScans + ' were started since)'); return s; }

  // Next scan: keeps the addresses whose value now satisfies `kind` (against the value at the
  // previous scan for changed / unchanged / increased / decreased / increasedBy / decreasedBy).
  async function nextScan(req) {
    const s = scanOf(req.scan);
    const m = mem(s.mem);
    const d = typeOf(s.type);
    const v = view(m);
    const kind = req.kind;
    const raw = function (x) { return toRaw(d, x); };
    const eps = s.eps;
    const eq = d.float ? function (a, b) { return Math.abs(a - b) <= eps; } : function (a, b) { return a === b; };
    let test;
    switch (kind) {
      case 'exact': { const w = raw(req.value); test = function (x) { return eq(x, w); }; break; }
      case 'range': { const lo = raw(req.value), hi = raw(req.to); test = function (x) { return x >= lo && x <= hi; }; break; }
      case 'changed': test = function (x, p) { return !eq(x, p); }; break;
      case 'unchanged': test = function (x, p) { return eq(x, p); }; break;
      case 'increased': test = function (x, p) { return x > p && !eq(x, p); }; break;
      case 'decreased': test = function (x, p) { return x < p && !eq(x, p); }; break;
      case 'increasedBy': { const w = raw(req.value); test = function (x, p) { return eq(x - p, w); }; break; }
      case 'decreasedBy': { const w = raw(req.value); test = function (x, p) { return eq(p - x, w); }; break; }
      default: fail('unknown next-scan kind ' + kind + ' (exact, range, changed, unchanged, increased, decreased, increasedBy, decreasedBy)');
    }
    const addrs = [], prev = [];
    const get = d.get;
    let dv = v.dv, len = v.len;
    // After giving the page time: the buffer of now (see firstScan). A memory only grows.
    const again = function () { if (m.buffer !== dv.buffer) { dv = new DV(m.buffer); len = dv.byteLength; } };
    let t0 = now();
    if (s.snapshot) {
      const snap = new DV(s.snapshot.buffer);
      const end = Math.min(s.snapshot.length, len) - d.size;
      for (let a = 0; a <= end; a += s.align) {
        const x = dv[get](a, true);
        if (test(x, snap[get](a, true))) {
          if (addrs.length >= LIMITS.maxResults) { s.truncated = true; break; }
          addrs.push(a); prev.push(x);
        }
        if ((a & 0xfffff) === 0 && now() - t0 > LIMITS.sliceMs) { s.progress = a / end; await yieldNow(); t0 = now(); again(); }
      }
      s.snapshot = null;
    } else {
      const old = s.addrs, oldPrev = s.prev;
      for (let i = 0; i < old.length; i++) {
        const a = old[i];
        if (a + d.size > len) continue;
        const x = dv[get](a, true);
        if (test(x, oldPrev[i])) { addrs.push(a); prev.push(x); }
        if ((i & 0xfffff) === 0 && i && now() - t0 > LIMITS.sliceMs) { await yieldNow(); t0 = now(); again(); }
      }
    }
    s.addrs = new U32(addrs);
    s.prev = d.big ? prev : new F64(prev);
    s.count = addrs.length;
    s.progress = 1;
    return summary(s);
  }

  function results(req) {
    const s = scanOf(req.scan);
    const offset = req.offset | 0, limit = Math.min(req.limit === undefined ? 100 : req.limit | 0, LIMITS.maxPage);
    const d = typeOf(s.type);
    const v = view(mem(s.mem));
    const rows = [];
    if (s.snapshot) return { scan: s.id, count: s.count, rows: rows, note: 'an unknown-value scan lists its addresses after the next scan' };
    for (let i = offset; i < s.count && rows.length < limit; i++) {
      const a = s.addrs[i];
      const cur = a + d.size <= v.len ? out(d, v.dv[d.get](a, true)) : null;
      rows.push({ addr: a, value: cur, previous: out(d, s.prev[i]) });
    }
    return { scan: s.id, count: s.count, offset: offset, rows: rows };
  }

  // ---------------------------------------------------------------- freeze, watch
  const freezes = new Map();
  let freezeSeq = 0;
  let ticker = 0;
  function tick() {
    freezes.forEach(function (f) {
      try {
        const v = view(mem(f.mem));
        writeAt(v, resolve(v, f.where), f.type, f.value);
        f.errors = 0;
      } catch (e) { f.errors++; f.last = String(e && e.message || e); }
    });
  }
  function freeze(req) {
    if (freezes.size >= LIMITS.maxFreezes) fail('at most ' + LIMITS.maxFreezes + ' frozen values');
    const v = view(mem(req.mem));
    const where = req.where !== undefined ? req.where : req.addr;
    const a = resolve(v, where);
    let value = req.value;
    if (value === undefined || value === null) value = readAt(v, a, req.type);
    writeAt(v, a, req.type, value); // also checks the request
    const id = ++freezeSeq;
    freezes.set(id, { id: id, mem: req.mem, where: where, type: req.type, value: value, errors: 0, last: '' });
    if (!ticker) ticker = every(tick, 50);
    return { freeze: id, addr: a, value: value };
  }
  function unfreeze(req) {
    const had = freezes.delete(req.freeze);
    if (!freezes.size && ticker) { stop(ticker); ticker = 0; }
    return { unfrozen: had };
  }
  function frozen() {
    const out = [];
    freezes.forEach(function (f) { out.push({ freeze: f.id, mem: f.mem, where: f.where, type: f.type, value: f.value, error: f.errors ? f.last : '' }); });
    return out;
  }
  function watch(req) {
    const entries = req.entries || [];
    if (!isArr(entries) || entries.length > LIMITS.maxWatch) fail('at most ' + LIMITS.maxWatch + ' entries');
    return entries.map(function (e) {
      try {
        const v = view(mem(e.mem));
        const a = resolve(v, e.where !== undefined ? e.where : e.addr);
        return { addr: a, value: readAt(v, a, e.type, e.length) };
      } catch (err) { return { error: String(err && err.message || err) }; }
    });
  }

  // ---------------------------------------------------------------- entry point
  async function run(req) {
    if (!req || typeof req.op !== 'string') fail('a request {op, …}');
    switch (req.op) {
      case 'list': return list();
      case 'instances': return instances();
      case 'survey': return { memories: list(), instances: instances() };
      case 'globals': return globals(req);
      case 'setGlobal': return setGlobal(req);
      case 'limits': return LIMITS;
      case 'read': { const v = view(mem(req.mem)); const a = resolve(v, req.where !== undefined ? req.where : req.addr); return { addr: a, value: readAt(v, a, req.type, req.length) }; }
      case 'write': { const v = view(mem(req.mem)); const a = resolve(v, req.where !== undefined ? req.where : req.addr); return { addr: a, written: writeAt(v, a, req.type, req.value) }; }
      case 'bytes': { const v = view(mem(req.mem)); return { addr: req.addr, bytes: readAt(v, req.addr, 'bytes', Math.min(req.length === undefined ? 256 : req.length, v.len - req.addr)), size: v.len }; }
      case 'writeBytes': { const v = view(mem(req.mem)); return { written: writeAt(v, req.addr, 'bytes', req.bytes) }; }
      case 'resolve': { const v = view(mem(req.mem)); return { addr: resolve(v, req.where) }; }
      case 'scan': return firstScan(req);
      case 'next': return nextScan(req);
      case 'results': return results(req);
      case 'progress': { const s = scanOf(req.scan); return { scan: s.id, done: s.done, progress: s.progress }; }
      case 'discard': return { discarded: scans.delete(req.scan) };
      case 'freeze': return freeze(req);
      case 'unfreeze': return unfreeze(req);
      case 'frozen': return frozen();
      case 'watch': return watch(req);
      case 'reset': {
        scans.clear(); freezes.clear(); if (ticker) { stop(ticker); ticker = 0; }
        return { reset: true };
      }
      default: fail('unknown op ' + req.op);
    }
  }
  function call(found, req, more) {
    adopt(found);
    adopt(more);
    return P.resolve().then(function () { return run(req); });
  }

  // Memories and instances made from now on are found without DevTools (userscripts; the host's fallback).
  let hooked = false;
  function hook() {
    if (hooked || !WA) return;
    hooked = true;
    const RealMemory = WA.Memory;
    const keep = function (x) {
      try {
        if (x instanceof RealMemory) adoptMem(x);
        else if (isA(x, RealInstance)) adoptInst(x);
        else if (x && x.instance) keep(x.instance);
      } catch (_) { /* not ours to inspect */ }
      return x;
    };
    try {
      const Mem = function Memory(desc) {
        if (!new.target) throw new TypeError("Constructor WebAssembly.Memory requires 'new'");
        return keep(Reflect.construct(RealMemory, [desc], new.target === Mem ? RealMemory : new.target));
      };
      Mem.prototype = RealMemory.prototype;
      Object.defineProperty(RealMemory.prototype, 'constructor', { value: Mem, writable: true, configurable: true });
      WA.Memory = Mem;
      const Inst = function Instance(module, imports) {
        if (!new.target) throw new TypeError("Constructor WebAssembly.Instance requires 'new'");
        const inst = Reflect.construct(RealInstance, [module, imports], new.target === Inst ? RealInstance : new.target);
        if (imports) for (const ns in imports) for (const k in imports[ns]) keep(imports[ns][k]);
        return keep(inst);
      };
      Inst.prototype = RealInstance.prototype;
      WA.Instance = Inst;
      ['instantiate', 'instantiateStreaming'].forEach(function (name) {
        const real = WA[name];
        if (typeof real !== 'function') return;
        WA[name] = function () {
          const imports = arguments[1];
          if (imports) { try { for (const ns in imports) for (const k in imports[ns]) keep(imports[ns][k]); } catch (_) { /* odd imports */ } }
          return Reflect.apply(real, WA, arguments).then(keep);
        };
      });
    } catch (_) { /* a frozen WebAssembly: DevTools only */ }
  }

  return Object.freeze({ call: call, hook: hook, limits: LIMITS });
})
