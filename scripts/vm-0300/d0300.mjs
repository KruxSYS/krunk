// Krunk 0.30.0 cast smoke driver (CDP on 127.0.0.1:19870 = the VM's 9222 through ssh -L).
//   node d0300.mjs api          what the probe page sees (Presentation / Remote Playback APIs)
//   node d0300.mjs menu-cast    ⋮ menu (the shell's own button handler) → "Cast…" in the popup
//   node d0300.mjs menu-items   ⋮ menu: is "Cast…" there? (then closes it)
//   node d0300.mjs popup        the open popup's kind and text
//   node d0300.mjs hide         close the popup
const base = 'http://127.0.0.1:19870';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function targets() { return (await (await fetch(base + '/json/list')).json()).filter((t) => t.type === 'page'); }
function session(t) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(t.webSocketDebuggerUrl); let n = 0; const wait = new Map();
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && wait.has(d.id)) { wait.get(d.id)(d); wait.delete(d.id); } };
    ws.onerror = reject;
    ws.onopen = () => resolve({ send: (method, params = {}) => new Promise((r) => { const id = ++n; wait.set(id, r); ws.send(JSON.stringify({ id, method, params })); }), close: () => ws.close() });
  });
}
// Krunk keeps a hidden spare window and tab: the matching target with the focus wins, else the largest.
async function find(pred) {
  for (let i = 0; i < 40; i++) {
    const all = (await targets()).filter(pred);
    let best = null, score = -1;
    for (const t of all) {
      const s = await session(t);
      const r = await s.send('Runtime.evaluate', { expression: '(document.hasFocus() ? 1e9 : 0) + innerWidth * innerHeight', returnByValue: true });
      s.close();
      const v = r.result?.result?.value ?? 0;
      if (v > score) { score = v; best = t; }
    }
    if (best) return best;
    await sleep(250);
  }
  throw new Error('no target');
}
async function evalIn(pred, expr) {
  const s = await session(await find(pred));
  const r = await s.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  s.close();
  return r.result?.exceptionDetails ? 'EXC ' + (r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text) : r.result?.result?.value;
}
const isShell = (t) => /shell\.krunk\.invalid\/shell/.test(t.url);
const isPopup = (t) => /shell\.krunk\.invalid\/popup/.test(t.url);
const isProbe = (t) => /18112\/cast\.html/.test(t.url);
const popupState = `JSON.stringify({ kind: document.getElementById('root').dataset.kind || null, text: document.getElementById('root').innerText.slice(0, 600) })`;
const step = process.argv[2];
if (step === 'api') {
  console.log('probe:', JSON.stringify(await evalIn(isProbe, 'window.__cast || null')));
} else if (step === 'menu-cast' || step === 'menu-items') {
  console.log('open menu:', await evalIn(isShell, `(document.getElementById('menu').click(), 'clicked')`));
  await sleep(1200);
  const has = await evalIn(isPopup, `[...document.querySelectorAll('.item')].map((b) => b.textContent).filter((t) => /Cast/.test(t)).join('|') || 'none'`);
  console.log('menu Cast items:', has);
  if (step === 'menu-cast') {
    console.log('click Cast…:', await evalIn(isPopup, `(() => { const b = [...document.querySelectorAll('.item')].find((x) => x.textContent.startsWith('Cast')); if (!b) return 'missing'; b.click(); return 'clicked'; })()`));
    await sleep(2500);
    console.log('popup:', await evalIn(isPopup, popupState));
  } else {
    console.log('hide:', await evalIn(isPopup, `(LB.post('hidePopup'), 'ok')`));
  }
} else if (step === 'popup') {
  console.log('popup:', await evalIn(isPopup, popupState));
} else if (step === 'hide') {
  console.log('hide:', await evalIn(isPopup, `(LB.post('hidePopup'), 'ok')`));
}
