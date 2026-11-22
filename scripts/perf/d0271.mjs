// Krunk 0.27.1 VM check driver (CDP on 127.0.0.1:19870 = the VM's 9222 through ssh -L).
//   node d0271.mjs scope   VPN "Use for" survives a save from an already open Settings page
const PW = 'Vault-pass-0253!';
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
// Krunk keeps a hidden spare window and tab, and every view says "visible": the matching
// target with the focus wins, else the largest one (the spare tab is 14x14).
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
// The tab: the page target that is not the shell, a popup or the agent panel.
const isTab = (t) => !/shell\.krunk\.invalid|popup|agent\.html|runner/.test(t.url);
const isSettings = (t) => /settings\.html/.test(t.url);
const step = process.argv[2];
if (step === 'scope') {
  await evalIn((t) => /shell\.krunk\.invalid\/shell/.test(t.url), `LB.post('navigate', { input: 'krunk://settings' }), 'ok'`);
  await sleep(3000);
  const get = `LB.request('getSettings').then((r) => (r.settings || r).vpn.scope)`;
  console.log('scope at start:', await evalIn(isSettings, get));
  // What the VPN popup sends, while this Settings page stays open with what it loaded.
  console.log('vpnScope private:', await evalIn(isSettings, `LB.request('vpnScope', { scope: 'private' }).then(() => 'ok', (e) => 'ERR ' + e.message)`));
  console.log('scope after popup:', await evalIn(isSettings, get));
  // A change on the open page (its own checkbox), saved the way the page saves.
  console.log('toggle showButton:', await evalIn(isSettings, `(() => { const c = document.getElementById('vpn.showButton'); c.click(); return 'clicked ' + c.checked; })()`));
  await sleep(1500);
  console.log('scope after page save:', await evalIn(isSettings, get));
}
