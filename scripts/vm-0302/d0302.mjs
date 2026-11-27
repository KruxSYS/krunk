// Krunk 0.30.2 download smoke: the shell's tabs and address bar (CDP on 127.0.0.1:19870 = the VM's 9222).
//   node d0302.mjs state
const base = 'http://127.0.0.1:19870';
const targets = async () => (await (await fetch(base + '/json/list')).json()).filter((t) => t.type === 'page');
function evalIn(t, expression) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(t.webSocketDebuggerUrl);
    ws.onerror = reject;
    ws.onopen = () => ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression, returnByValue: true } }));
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id === 1) { ws.close(); resolve(d.result?.result?.value); } };
  });
}
const expr = `JSON.stringify({ focus: document.hasFocus(), address: document.getElementById('address').value,
  tabs: [...document.querySelectorAll('#tabs .tab')].map((n) => (n.classList.contains('active') ? '*' : '') + (n.title || n.textContent.trim()).slice(0, 80)) })`;
const all = (await targets()).filter((t) => /shell\.krunk\.invalid\/shell/.test(t.url));
for (const t of all) console.log(await evalIn(t, expr));
console.log('pages:', (await targets()).map((t) => t.url).filter((u) => !/krunk\.invalid/.test(u)).join(' | '));
