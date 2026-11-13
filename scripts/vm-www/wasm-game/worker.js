// The game module in a dedicated worker (index.html?worker).
'use strict';
const ready = fetch('game.wasm').then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, {})).then(({ instance }) => { instance.exports.init(); return instance.exports; });
onmessage = async (e) => {
  const ex = await ready;
  postMessage({ id: e.data.id, value: e.data.fn === 'init' ? 0 : ex[e.data.fn](e.data.arg) });
};
