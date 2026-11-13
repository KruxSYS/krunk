// The game module in a dedicated worker (index.html?worker).
'use strict';
const ready = fetch('game.wasm').then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, {})).then(({ instance }) => { instance.exports.init(); return instance.exports; });
onmessage = async (e) => {
  const ex = await ready;
  const fn = e.data.fn;
  // `sp` reads the exported mutable global (not a function of the module).
  postMessage({ id: e.data.id, value: fn === 'init' ? 0 : fn === 'sp' ? ex.__stack_pointer.value : ex[fn](e.data.arg) });
};
