// Approves a Krunk device link on a staging stack from the command line (for the VM's
// 080-account flow): signs in as a test account (made and confirmed first if needed, mail read
// from Mailpit), re-authenticates and approves `code`. Prints the device id.
//   node server/e2e/approve.cjs http://dind:10017 stage-browser vm080@example.org 'pass phrase' ABCD-EFGH
// Staging only: Turnstile's always-pass test keys (any token) and Mailpit (server/e2e/mailsink.sh).
'use strict';
const { execFileSync } = require('child_process');

const [UPSTREAM, PROJECT, EMAIL, PW, CODE] = process.argv.slice(2);
const ORIGIN = 'https://krunk.test';
let cookie = '';
let csrf = '';

async function web(method, path, body) {
  const headers = { Origin: ORIGIN };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (cookie) headers.Cookie = cookie;
  if (csrf && method !== 'GET') headers['X-Krunk-CSRF'] = csrf;
  const r = await fetch(`${UPSTREAM}/api/v1/web/${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const set = r.headers.get('set-cookie');
  if (set) cookie = set.split(';')[0];
  let j = {};
  try { j = await r.json(); } catch (e) { j = {}; }
  if (j.csrf) csrf = j.csrf;
  return { status: r.status, body: j };
}
function mailpit(path) {
  return JSON.parse(execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${path}`]).toString());
}

(async () => {
  let r = await web('POST', 'login', { email: EMAIL, password: PW, turnstile: 'XXXX.DUMMY.TOKEN.XXXX' });
  if (r.status !== 200) {
    await web('POST', 'register', { email: EMAIL, password: PW, turnstile: 'XXXX.DUMMY.TOKEN.XXXX' });
    let token = null;
    for (let i = 0; i < 40 && !token; i++) {
      const m = (mailpit('/api/v1/messages?limit=50').messages || []).find((x) => (x.To || []).some((t) => t.Address === EMAIL) && x.Subject.includes('Confirm'));
      if (m) token = /token=([A-Za-z0-9_-]{43})/.exec(mailpit(`/api/v1/message/${m.ID}`).Text)[1];
      else await new Promise((res) => setTimeout(res, 500));
    }
    r = await web('POST', 'verify-email', { token, password: PW });
    if (r.status !== 200) throw new Error(`verify: ${r.status}`);
    r = await web('POST', 'login', { email: EMAIL, password: PW, turnstile: 'XXXX.DUMMY.TOKEN.XXXX' });
  }
  if (r.status !== 200) throw new Error(`login: ${r.status} ${JSON.stringify(r.body)}`);
  r = await web('POST', 'reauth', { password: PW });
  if (r.status !== 204) throw new Error(`reauth: ${r.status}`);
  r = await web('GET', `link/${encodeURIComponent(CODE)}`);
  if (r.status !== 200) throw new Error(`link view: ${r.status} ${JSON.stringify(r.body)}`);
  console.log(`device asks: ${r.body.label} (${r.body.platform})`);
  r = await web('POST', `link/${encodeURIComponent(CODE)}/approve`, { confirm_code: CODE });
  if (r.status !== 200) throw new Error(`approve: ${r.status} ${JSON.stringify(r.body)}`);
  console.log(`approved: device ${r.body.device_id}`);
})().catch((e) => { console.error(e.message); process.exit(1); });
