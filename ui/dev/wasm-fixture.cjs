// A hand-assembled WebAssembly module for the WASM inspector's tests (no toolchain needed):
// a memory (1 page, at most 4) exported as "memory"; mutable globals g_i32 = 7, g_i64 = -5,
// g_f32 = 1.5, g_f64 = 2.25; an immutable global c_i32 = 1024; a function "f"; a table "tbl".
'use strict';
function section(id, bytes) { return [id, bytes.length].concat(bytes); }
function name(s) { return [s.length].concat(Array.from(Buffer.from(s, 'latin1'))); }
function fixtureBytes() {
  const globals = [5,
    0x7f, 1, 0x41, 7, 0x0b,
    0x7e, 1, 0x42, 0x7b, 0x0b,
    0x7d, 1, 0x43, 0, 0, 0xc0, 0x3f, 0x0b,
    0x7c, 1, 0x44, 0, 0, 0, 0, 0, 0, 2, 0x40, 0x0b,
    0x7f, 0, 0x41, 0x80, 0x08, 0x0b];
  const exports = [8]
    .concat(name('memory'), [2, 0], name('g_i32'), [3, 0], name('g_i64'), [3, 1], name('g_f32'), [3, 2])
    .concat(name('g_f64'), [3, 3], name('c_i32'), [3, 4], name('f'), [0, 0], name('tbl'), [1, 0]);
  return [0, 97, 115, 109, 1, 0, 0, 0]
    .concat(section(1, [1, 0x60, 0, 0]), section(3, [1, 0]), section(4, [1, 0x70, 0, 1]), section(5, [1, 1, 1, 4]))
    .concat(section(6, globals), section(7, exports), section(10, [1, 2, 0, 0x0b]));
}
module.exports = { fixtureBytes };
