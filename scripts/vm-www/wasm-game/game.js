// The game page's loader (see index.html). In a worker (`?worker`), worker.js runs the module
// and answers the page's messages; otherwise it runs here.
'use strict';
(async function () {
  const inWorker = /[?&]worker\b/.test(location.search);
  const $ = (id) => document.getElementById(id);
  document.getElementById('where').textContent = inWorker ? 'The module runs in a dedicated worker.' : 'The module runs in the page.';
  let call;
  if (inWorker) {
    const w = new Worker('worker.js');
    let seq = 0;
    const waiting = new Map();
    w.onmessage = (e) => { const f = waiting.get(e.data.id); if (f) { waiting.delete(e.data.id); f(e.data.value); } };
    call = (fn, arg) => new Promise((ok) => { const id = ++seq; waiting.set(id, ok); w.postMessage({ id, fn, arg }); });
  } else {
    const bytes = await (await fetch('game.wasm')).arrayBuffer();
    const { instance } = await WebAssembly.instantiate(bytes, {});
    instance.exports.init();
    // `sp` reads the exported mutable global (not a function of the module).
    call = async (fn, arg) => (fn === 'sp' ? instance.exports.__stack_pointer.value : instance.exports[fn](arg));
  }
  await call('init');
  async function show() {
    const h = await call('health'), s = await call('score'), g = await call('gold');
    const pages = await call('pages'), at = await call('marker_addr'), m = await call('marker'), sp = await call('sp');
    $('health').textContent = h; $('score').textContent = Math.round(s * 100) / 100; $('gold').textContent = g;
    $('pages').textContent = pages; $('sp').textContent = sp;
    $('marker').textContent = at ? m + ' at 0x' + at.toString(16).toUpperCase() : 'none yet';
    document.title = 'WASM health=' + h + ' score=' + Math.round(s * 100) / 100 + ' gold=' + g + ' pages=' + pages + ' marker=' + m + ' sp=' + sp;
  }
  $('hit').onclick = () => call('hit', 10).then(show);
  $('heal').onclick = () => call('heal', 5).then(show);
  $('pts').onclick = () => call('add_score', 1.5).then(show);
  $('spend').onclick = () => call('spend', 20).then(show);
  $('grow').onclick = () => call('grow').then(show);
  // Keys for scripted checks: h hit, e heal, g grow.
  addEventListener('keydown', (e) => { if (e.key === 'h') $('hit').click(); if (e.key === 'e') $('heal').click(); if (e.key === 'g') $('grow').click(); });
  setInterval(show, 100);
  show();
})();
