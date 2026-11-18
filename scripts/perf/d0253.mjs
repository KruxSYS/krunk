// Krunk 0.25.3 VM pass driver (CDP on 127.0.0.1:19870 = the VM's 9222 through ssh -L).
//   node d0253.mjs <step>   steps: setup | submit <user> <pw> | unlock | lock | key | popup | cards
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
async function visibleTab() {
  for (const t of (await targets()).filter(isTab)) {
    const s = await session(t);
    const r = await s.send('Runtime.evaluate', { expression: 'document.visibilityState', returnByValue: true });
    s.close();
    if (r.result?.result?.value === 'visible') return t;
  }
  throw new Error('no visible tab');
}
// Like typing in the address bar (CDP may not open krunk:// pages).
async function go(url) {
  await evalIn((t) => /shell\.krunk\.invalid\/shell/.test(t.url), `LB.post('navigate', { input: ${JSON.stringify(url)} }), 'ok'`);
  await sleep(3000);
}
const req = (cmd, args) => `LB.request(${JSON.stringify(cmd)}, ${JSON.stringify(args || {})}).then(r => JSON.stringify(r), e => 'ERR ' + e.message)`;
const onPage = (frag) => (t) => isTab(t) && t.url.includes(frag);
const [step, a1, a2] = process.argv.slice(2);
if (step === 'list') console.log((await targets()).map((t) => t.url).join('\n'));
if (step === 'setup') {
  await go('krunk://passwords');
  console.log('setup', await evalIn(onPage('passwords'), req('vaultSetup', { password: PW })));
  console.log('save', await evalIn(onPage('passwords'), req('vaultSave', { entry: { id: null, origin: 'http://localhost:8094', username: 'Ann@Example.com', password: 'login-pw-1', note: null } })));
  console.log('list', await evalIn(onPage('passwords'), req('vaultList')));
}
if (step === 'cards') {
  await go('krunk://settings');
  const det = await evalIn(onPage('settings'), req('importDetect'));
  console.log('detect', String(det).slice(0, 600));
  const ff = JSON.parse(det).find?.((p) => /firefox/i.test(p.browser || p.name || '')) || (JSON.parse(det).profiles || []).find((p) => /firefox/i.test(JSON.stringify(p)));
  console.log('firefox profile', JSON.stringify(ff).slice(0, 300));
  console.log('run', await evalIn(onPage('settings'), req('importRun', { profile: ff.id, types: ['cards'], primaryPassword: null })));
  await go('krunk://passwords');
  console.log('cardList', await evalIn(onPage('passwords'), req('cardList')));
}
if (step === 'submit') {
  await go('http://localhost:8094/login.html');
  console.log('submit', await evalIn(onPage('login.html'), `(() => { const f = document.getElementById('f'); f.action = '/log'; document.getElementById('user').value = ${JSON.stringify(a1)}; document.getElementById('pw').value = ${JSON.stringify(a2)}; f.requestSubmit(); return 'sent'; })()`));
  await sleep(2500);
}
if (step === 'unlock') {
  if (a1 !== 'here') await go('krunk://passwords');
  // From the passwords page (or, with "here", any Krunk page is not possible: use the popup's unlock).
  console.log('unlock', await evalIn(onPage('passwords'), req('vaultUnlock', { password: PW, remain: false })));
  await sleep(1500);
}
if (step === 'lock') { await go('krunk://passwords'); console.log('lock', await evalIn(onPage('passwords'), req('vaultLock'))); }
if (step === 'popup') console.log('popup', await evalIn((t) => /popup/.test(t.url), `JSON.stringify({ text: document.body.innerText.slice(0, 300), visible: document.visibilityState })`));
if (step === 'key') {
  if (a1) await go(a1);
  console.log('key', await evalIn((t) => /shell\.krunk\.invalid\/shell/.test(t.url), `(() => { const k = document.getElementById('key'); const s = { hidden: k.hidden, offer: k.classList.contains('offer'), title: k.title }; if (${JSON.stringify(a2 === 'click')}) k.click(); return JSON.stringify(s); })()`));
  await sleep(1500);
}
