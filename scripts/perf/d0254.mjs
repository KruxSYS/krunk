// Krunk 0.25.4 VM smoke driver: the VPN kill switch (CDP on 127.0.0.1:19870 = the VM's 9222).
//   node d0254.mjs <step>   ks on|off | page | import | connect | disconnect | remove | state
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
const isTab = (t) => !/shell\.krunk\.invalid|popup|agent\.html|runner/.test(t.url);
async function go(url) {
  await evalIn((t) => /shell\.krunk\.invalid\/shell/.test(t.url), `LB.post('navigate', { input: ${JSON.stringify(url)} }), 'ok'`);
  await sleep(5000);
}
const req = (cmd, args) => `LB.request(${JSON.stringify(cmd)}, ${JSON.stringify(args || {})}).then(r => JSON.stringify(r), e => 'ERR ' + e.message)`;
const onPage = (frag) => (t) => isTab(t) && t.url.includes(frag);
const settingsPage = onPage('settings');
async function state() {
  const s = JSON.parse(await evalIn(settingsPage, req('vpnState')));
  return s;
}
const [step, a1] = process.argv.slice(2);
if (step !== 'page') await go('krunk://settings');
if (step === 'ks') {
  const cur = JSON.parse(await evalIn(settingsPage, req('getSettings')));
  const settings = cur.settings || cur;
  settings.vpn.killSwitch = a1 === 'on';
  const r = await evalIn(settingsPage, req('saveSettings', { settings }));
  console.log('ks', a1, String(r).slice(0, 200).includes('ERR') ? r : 'saved');
  await sleep(1500);
}
if (step === 'page') {
  // A real site (loopback would skip the proxy anyway).
  await go('https://example.com/');
  const t = (await targets()).filter(isTab).find((x) => /example\.com/.test(x.url)) || await find(isTab);
  const s = await session(t);
  const r = await s.send('Runtime.evaluate', { expression: 'JSON.stringify({ url: location.href, title: document.title, text: document.body ? document.body.innerText.slice(0, 160) : "" })', returnByValue: true });
  s.close();
  console.log('page', r.result?.result?.value);
}
if (step === 'import') {
  // Opens the native dialog; the ps1 types the path into it. Not awaited here.
  console.log('import', await evalIn(settingsPage, `(LB.request('vpnImport').then(r => console.log('imported', JSON.stringify(r))), 'dialog opened')`));
}
if (step === 'connect') {
  const s = await state();
  console.log('connect', s.profiles?.[0]?.name, await evalIn(settingsPage, req('vpnConnect', { id: s.profiles[0].id })));
  await sleep(2000);
}
if (step === 'disconnect') { console.log('disconnect', await evalIn(settingsPage, req('vpnDisconnect'))); await sleep(1500); }
if (step === 'remove') {
  const s = await state();
  console.log('remove', await evalIn(settingsPage, req('vpnRemove', { id: s.profiles[0].id })));
  await sleep(1500);
}
if (step === 'state' || step !== 'page') {
  const s = await state();
  console.log('state', JSON.stringify({ state: s.status?.state, error: s.status?.error, profiles: (s.profiles || []).length }));
}
