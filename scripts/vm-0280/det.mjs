// Krunk 0.28.0: reads the verdicts of a bot-detection page in the VM's browser (CDP on
// 127.0.0.1:19870 = the VM's 9222 through ssh -L). One Runtime.evaluate after the page ran.
//   node det.mjs <url> [waitMs]  → JSON summary on stdout
const [url, waitMs = '20000'] = process.argv.slice(2);
const base = 'http://127.0.0.1:19870';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const pages = async () => (await (await fetch(base + '/json/list')).json()).filter((t) => t.type === 'page' && !/krunk\.invalid/.test(t.url));
function session(t) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(t.webSocketDebuggerUrl); let n = 0; const wait = new Map();
    ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && wait.has(d.id)) { wait.get(d.id)(d); wait.delete(d.id); } };
    ws.onerror = reject;
    ws.onopen = () => resolve({ send: (method, params = {}) => new Promise((r) => { const id = ++n; wait.set(id, r); ws.send(JSON.stringify({ id, method, params })); }), close: () => ws.close() });
  });
}
const host = new URL(url).hostname;
let t;
for (let i = 0; i < 40 && !t; i++) { t = (await pages()).find((p) => p.url.includes(host)); if (!t) await sleep(500); }
if (!t) { console.log(JSON.stringify({ error: 'no tab for ' + host })); process.exit(1); }
await sleep(+waitMs);
const s = await session(t);
const expr = `(() => {
  const h = location.hostname, txt = document.body.innerText;
  const grep = (re, n = 40) => txt.split('\\n').map(l => l.trim()).filter(l => l && re.test(l)).slice(0, n);
  if (/sannysoft/.test(h)) {
    const bad = [...document.querySelectorAll('td.failed, td.warn')].map(td => { const tr = td.closest('tr'); return td.className + ': ' + (tr ? tr.cells[0].innerText.trim() : '?'); });
    return { site: 'sannysoft', failedOrWarn: bad, passedCells: document.querySelectorAll('td.passed').length };
  }
  if (/creepjs/.test(h)) return { site: 'creepjs', text: txt.slice(0, 80000), lines: grep(/lie|trash|bot|headless|stealth|errors|fingerprint|crowd|resistance|webdriver|like headless|chromium|version/i, 60) };
  return { site: h, text: txt.slice(0, 80000), lines: grep(/bot|automat|webdriver|headless|lie|spoof|mask|consisten|proxy|incognito|cdp|selenium|chrome|version|score|trust/i, 60) };
})()`;
const r = await s.send('Runtime.evaluate', { expression: process.env.EXPR || expr, awaitPromise: true, returnByValue: true });
s.close();
console.log(JSON.stringify(r.result?.result?.value ?? r, null, 1));
