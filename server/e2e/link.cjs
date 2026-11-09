// End-to-end test of device linking on a staging stack (milestone S3): a test client plays
// Krunk (the RFC 8628 device side, plain HTTP like the browser's WinHTTP client) and Playwright
// plays the user on the real /link and /account/ pages under the real CSP.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/link.cjs http://dind:10017 stage-browser <screenshot dir>
// Staging as for accounts.cjs (staging.env test values, server/e2e/mailsink.sh up). The browser
// sees the site as https://krunk.test (the __Host- cookie needs a secure origin); the device
// client talks to the upstream directly.
'use strict';
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const path = require('path');

const UPSTREAM = process.argv[2] || 'http://dind:10017';
const BASE = 'https://krunk.test';
const PROJECT = process.argv[3] || 'stage-browser';
const SHOTS = process.argv[4] || '/tmp/krunk-link';
const EMAIL = `link-${Date.now()}@example.org`;
const PW = 'violet harbour compass 9';

const problems = [];
let step = 0;
function ok(cond, what) {
  if (!cond) { problems.push(what); console.log('FAIL', what); } else console.log('ok  ', what);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function mailpit(apiPath) {
  const out = execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${apiPath}`]);
  return JSON.parse(out.toString());
}
async function waitMail(to, subjectPart) {
  for (let i = 0; i < 40; i++) {
    const list = mailpit('/api/v1/messages?limit=100').messages || [];
    const m = list.find((x) => (x.To || []).some((t) => t.Address === to) && x.Subject.includes(subjectPart));
    if (m) return mailpit(`/api/v1/message/${m.ID}`);
    await sleep(500);
  }
  return null;
}

// The device side: what Krunk sends (no Origin, no cookies).
async function dev(method, p, body, bearer) {
  const headers = { 'User-Agent': 'Krunk/0.8.0 (link.cjs)' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (bearer) headers.Authorization = `Bearer ${bearer}`;
  const r = await fetch(`${UPSTREAM}/api/v1/${p}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  let j = {};
  try { j = await r.json(); } catch (e) { j = {}; }
  return { status: r.status, body: j };
}
async function authorize(label) {
  return dev('POST', 'link/authorize', { platform: 'windows', label, client_version: '0.8.0' });
}
// Polls like Krunk: every `interval` seconds, 5 s more after slow_down, until a final answer.
async function pollUntilDone(deviceCode, interval, maxS) {
  const end = Date.now() + maxS * 1000;
  while (Date.now() < end) {
    await sleep(interval * 1000);
    const r = await dev('POST', 'link/token', { device_code: deviceCode });
    if (r.status === 200) return r;
    if (r.body.error === 'slow_down') interval += 5;
    else if (r.body.error !== 'authorization_pending') return r;
  }
  return { status: 0, body: { error: 'timeout' } };
}

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1000, height: 900 } });
  await ctx.route(`${BASE}/**`, async (route) => {
    const u = new URL(route.request().url());
    const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
    await route.fulfill({ response });
  });
  const page = await ctx.newPage();
  const errors = [];
  const violations = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`${page.url()}: ${m.text()}`); });
  page.on('pageerror', (e) => errors.push(`${page.url()}: ${e.message}`));
  await page.addInitScript(() => {
    document.addEventListener('securitypolicyviolation', (e) => console.error(`CSP violation: ${e.violatedDirective} ${e.blockedURI}`));
  });
  async function shot(name) {
    step += 1;
    await page.screenshot({ path: path.join(SHOTS, `${String(step).padStart(2, '0')}-${name}.png`), fullPage: true });
  }
  async function waitTurnstile() {
    await page.waitForFunction(() => {
      const i = document.querySelector('[data-turnstile] input[name="cf-turnstile-response"]');
      return i && i.value.length > 0;
    }, null, { timeout: 30000 });
  }

  try {
    // An account (S2 pages).
    await page.goto(`${BASE}/account/register`);
    await page.waitForSelector('#form:not([hidden])');
    ok(await page.$eval('#form', (f) => getComputedStyle(f.querySelector('label:nth-of-type(1)')).display) === 'block', 'labels still block');
    await waitTurnstile();
    await page.fill('input[name=email]', EMAIL);
    await page.fill('input[name=password]', PW);
    await page.fill('input[name=password2]', PW);
    await page.click('button[type=submit]');
    await page.waitForSelector('#done:not([hidden])');
    const vmail = await waitMail(EMAIL, 'Confirm');
    const vlink = /(http\S+token=[A-Za-z0-9_-]{43})/.exec(vmail.Text)[1];
    await page.goto(vlink);
    await page.waitForSelector('#form:not([hidden])');
    await page.fill('input[name=password]', PW);
    await page.click('button[type=submit]');
    await page.waitForSelector('#done:not([hidden])');
    ok(true, 'account registered and verified');

    // D1: hidden TOTP rows stay hidden.
    await page.goto(`${BASE}/account/login`);
    await page.waitForSelector('#form:not([hidden])');
    ok(!(await page.isVisible('#totp-row')), 'login: the TOTP row is hidden until asked for (D1)');

    // Device A asks to be linked.
    const a = await authorize('Krunk 0.8.0 on Windows 11');
    ok(a.status === 200 && /^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/.test(a.body.user_code), `authorize: user code ${a.body.user_code}`);
    ok(a.body.expires_in === 600 && a.body.interval === 5, 'authorize: 600 s, interval 5 s');
    ok(a.body.verification_uri_complete === `${BASE}/link?code=${a.body.user_code}`, 'authorize: the link to open');
    let r = await dev('POST', 'link/token', { device_code: a.body.device_code });
    ok(r.body.error === 'authorization_pending', 'poll: authorization_pending');
    r = await dev('POST', 'link/token', { device_code: a.body.device_code });
    ok(r.body.error === 'slow_down', 'poll too fast: slow_down');
    const pollA = pollUntilDone(a.body.device_code, 10, 180);

    // The user opens the link: not signed in → sign in → back to the approval page.
    await page.goto(a.body.verification_uri_complete);
    await page.waitForURL(/\/account\/login\?next=/);
    ok(true, 'link page without a session goes to sign-in with next=');
    await page.waitForSelector('#form:not([hidden])');
    await waitTurnstile();
    await page.fill('input[name=email]', EMAIL);
    await page.fill('input[name=password]', PW);
    await page.click('button[type=submit]');
    await page.waitForURL(`${BASE}/link`);
    await page.waitForSelector('#approve-form:not([hidden])');
    ok(!page.url().includes('code='), 'the code left the address bar');
    ok((await page.textContent('#d-label')) === 'Krunk 0.8.0 on Windows 11', 'approval page: the device label');
    ok((await page.textContent('#d-platform')) === 'Windows', 'approval page: the platform');
    ok((await page.textContent('#d-code')) === a.body.user_code, 'approval page: the code to confirm');
    ok(await page.isDisabled('#approve-form button[type=submit]'), 'Approve is disabled until the code is confirmed');
    await shot('link-approve');
    await page.check('#approve-form input[name=same]');
    await page.click('#approve-form button[type=submit]');
    await page.waitForSelector('#reauth:not([hidden])');
    ok(true, 'approve asks for the password again (re-auth)');
    await shot('link-reauth');
    await page.fill('#reauth input[name=password]', PW);
    await page.click('#reauth button[type=submit]');
    await page.waitForSelector('#done:not([hidden])');
    ok((await page.textContent('#done-text')).includes('Approved'), 'approved');
    await shot('link-approved');
    const ta = await pollA;
    ok(ta.status === 200 && ta.body.access_token && ta.body.refresh_token, 'device A got its tokens');
    ok(Object.keys(ta.body).sort().join() === 'access_token,account_id,device_id,expires_in,refresh_token', 'token response: no key material');
    const linkedMail = await waitMail(EMAIL, 'new device was linked');
    ok(linkedMail && linkedMail.Text.includes('Krunk 0.8.0 on Windows 11'), 'mail: a new device was linked');
    r = await dev('POST', 'link/token', { device_code: a.body.device_code });
    ok(r.body.error === 'expired_token', 'the device code is single use');
    r = await dev('GET', 'device/me', undefined, ta.body.access_token);
    ok(r.status === 200 && r.body.email === EMAIL, 'device/me: the account email');

    // Rotation and reuse.
    r = await dev('POST', 'token/refresh', { refresh_token: ta.body.refresh_token });
    ok(r.status === 200 && r.body.refresh_token !== ta.body.refresh_token, 'refresh: rotated');
    const a2 = r.body;
    r = await dev('POST', 'token/refresh', { refresh_token: ta.body.refresh_token });
    ok(r.status === 401, 'refresh with the spent token: 401');
    r = await dev('GET', 'device/me', undefined, a2.access_token);
    ok(r.status === 401, 'reuse revoked the device at once');
    ok(await waitMail(EMAIL, 'signed out a device'), 'mail: token reuse');

    // Device B: linked with the code typed by hand (re-auth still fresh).
    const b = await authorize('Krunk 0.8.0 on Windows 10');
    const pollB = pollUntilDone(b.body.device_code, 5, 120);
    await page.goto(`${BASE}/link`);
    await page.waitForSelector('#code-form:not([hidden])');
    await page.fill('#code-form input[name=code]', b.body.user_code.toLowerCase().replace('-', ' '));
    await page.click('#code-form button[type=submit]');
    await page.waitForSelector('#approve-form:not([hidden])');
    await page.check('#approve-form input[name=same]');
    await page.click('#approve-form button[type=submit]');
    await page.waitForSelector('#done:not([hidden])');
    const tb = await pollB;
    ok(tb.status === 200, 'device B linked (typed code, no second re-auth within 10 minutes)');

    // Device C: denied.
    const c = await authorize('Krunk 0.8.0 on Windows 11');
    const pollC = pollUntilDone(c.body.device_code, 5, 120);
    await page.goto(c.body.verification_uri_complete);
    await page.waitForSelector('#approve-form:not([hidden])');
    await page.click('#deny');
    await page.waitForSelector('#done:not([hidden])');
    const tc = await pollC;
    ok(tc.body.error === 'access_denied', 'denied on the website: access_denied');
    await page.goto(`${BASE}/link?code=ZZZZ-ZZZZ`);
    await page.waitForSelector('#code-form:not([hidden])');
    ok((await page.textContent('#status')).includes('not valid'), 'an unknown code says so');

    // The account page: devices (B active, A removed), rename, remove; sessions.
    await page.goto(`${BASE}/account/`);
    await page.waitForSelector('#devices li');
    const names = await page.$$eval('#devices li .name', (l) => l.map((x) => x.textContent));
    ok(names.length === 2 && names.includes('Krunk 0.8.0 on Windows 10'), `devices listed: ${names.join(' | ')}`);
    ok((await page.$$('#devices li.revoked')).length === 1, 'the reuse-revoked device shows as removed');
    ok((await page.$$eval('#sessions li .name', (l) => l.map((x) => x.textContent))).some((t) => t.includes('(this browser)')), 'sessions: this browser');
    await shot('account-devices');
    await page.click('#devices li:not(.revoked) button:text("Rename")');
    await page.fill('#devices li:not(.revoked) form input', 'Gaming PC');
    await page.click('#devices li:not(.revoked) form button[type=submit]');
    await page.waitForFunction(() => [...document.querySelectorAll('#devices li .name')].some((x) => x.textContent === 'Gaming PC'));
    r = await dev('GET', 'device/me', undefined, tb.body.access_token);
    ok(r.body.label === 'Gaming PC', 'rename reaches the device');
    await shot('account-renamed');
    await page.click('#devices li:not(.revoked) button:text("Remove")');
    await page.waitForFunction(() => document.querySelectorAll('#devices li.revoked').length === 2);
    r = await dev('GET', 'device/me', undefined, tb.body.access_token);
    ok(r.status === 401, 'removed on the website: the device is signed out at once');
    r = await dev('POST', 'token/refresh', { refresh_token: tb.body.refresh_token });
    ok(r.status === 401, 'removed: its refresh token is dead too');
    ok(await waitMail(EMAIL, 'was removed'), 'mail: device removed');
    await shot('account-removed');

    // Unlink by the device itself.
    const d = await authorize('Krunk 0.8.0 on Windows 11');
    const pollD = pollUntilDone(d.body.device_code, 5, 120);
    await page.goto(d.body.verification_uri_complete);
    await page.waitForSelector('#approve-form:not([hidden])');
    await page.check('#approve-form input[name=same]');
    await page.click('#approve-form button[type=submit]');
    await page.waitForSelector('#done:not([hidden])');
    const td = await pollD;
    r = await dev('POST', 'device/unlink', {}, td.body.access_token);
    ok(r.status === 204, 'device/unlink: 204');
    r = await dev('GET', 'device/me', undefined, td.body.access_token);
    ok(r.status === 401, 'unlinked: 401');

    // Native endpoints refuse browser pages of other sites.
    r = await fetch(`${UPSTREAM}/api/v1/link/authorize`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: 'https://evil.example' }, body: JSON.stringify({ platform: 'windows', label: 'x', client_version: '1' }) });
    ok(r.status === 403, 'a foreign Origin on link/authorize: 403');
  } catch (e) {
    ok(false, `exception: ${e.message.split('\n')[0]}`);
    await shot('exception').catch(() => {});
  }

  // 4xx answers are logged by Chromium as console errors; the flow causes some on purpose (403
  // reauth_required, 404 for the unknown code). Anything else (a CSP violation, a script error)
  // fails the run.
  const expected = errors.filter((e) => /Failed to load resource: the server responded with a status of 40[134]/.test(e));
  const real = errors.filter((e) => !expected.includes(e));
  ok(real.length === 0 && violations.length === 0, `no CSP violations or script errors (${real.length}; ${expected.length} expected 4xx)`);
  real.forEach((e) => console.log('   ', e));
  await browser.close();
  console.log(problems.length ? `${problems.length} FAILED` : 'ALL PASSED');
  process.exit(problems.length ? 1 : 0);
})();
