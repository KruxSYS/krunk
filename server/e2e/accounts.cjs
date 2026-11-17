// End-to-end test of the account pages on a staging stack (milestone S2): every page under the
// real CSP, Cloudflare's always-pass Turnstile test widget, mail read from Mailpit.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/accounts.cjs http://dind:10017 stage-browser <screenshot dir>
// Staging: test values in the gitignored staging.env (ACCOUNTS_TEST_MODE=1, PUBLIC_BASE_URL=
// https://krunk.test, Turnstile test keys, SMTP_HOST=mailpit) and server/e2e/mailsink.sh up.
// The session cookie is __Host- and Secure, which a browser keeps only on a secure origin. So the
// browser sees the site as https://krunk.test, and every request to it is passed to the staging
// stack (PUBLIC_BASE_URL=https://krunk.test there, so the Origin check and mail links match).
'use strict';
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const crypto = require('crypto');
const path = require('path');

const UPSTREAM = process.argv[2] || 'http://dind:10017';
const BASE = 'https://krunk.test';
const PROJECT = process.argv[3] || 'stage-browser';
const SHOTS = process.argv[4] || '/tmp/krunk-e2e';
const EMAIL = `e2e-${Date.now()}@example.org`;
const PW1 = 'purple monkey dishwasher 7';
const PW2 = 'seven silent lighthouses';
const PW3 = 'quiet orange typewriter';

const problems = [];
let step = 0;
function ok(cond, what) {
  if (!cond) { problems.push(what); console.log('FAIL', what); } else console.log('ok  ', what);
}

function mailpit(apiPath) {
  const out = execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${apiPath}`]);
  return JSON.parse(out.toString());
}
function lastMail(to, subjectPart) {
  const list = mailpit('/api/v1/messages?limit=50').messages || [];
  const m = list.find((x) => (x.To || []).some((t) => t.Address === to) && x.Subject.includes(subjectPart));
  return m ? mailpit(`/api/v1/message/${m.ID}`) : null;
}
async function waitMail(to, subjectPart) {
  for (let i = 0; i < 40; i++) {
    const m = lastMail(to, subjectPart);
    if (m) return m;
    await new Promise((r) => setTimeout(r, 500));
  }
  return null;
}
function linkIn(mail) {
  const m = /(http\S+token=[A-Za-z0-9_-]{43})/.exec(mail.Text);
  return m ? m[1] : null;
}
function b32decode(s) {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = 0, buf = 0; const out = [];
  for (const c of s) { buf = (buf << 5) | A.indexOf(c); bits += 5; if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); } }
  return Buffer.from(out);
}
function totp(secret, offsetSteps) {
  const step = Math.floor(Date.now() / 1000 / 30) + (offsetSteps || 0);
  const msg = Buffer.alloc(8); msg.writeBigUInt64BE(BigInt(step));
  const h = crypto.createHmac('sha1', secret).update(msg).digest();
  const o = h[h.length - 1] & 15;
  return String(((h.readUInt32BE(o) & 0x7fffffff) % 1000000)).padStart(6, '0');
}

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1100, height: 900 } });
  await ctx.route(`${BASE}/**`, async (route) => {
    const u = new URL(route.request().url());
    const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
    await route.fulfill({ response });
  });
  const page = await ctx.newPage();
  const errors = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(`${page.url()}: ${m.text()}`); });
  page.on('pageerror', (e) => errors.push(`${page.url()}: ${e.message}`));
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
  async function status() { return (await page.textContent('#status')).trim(); }

  // Register.
  await page.goto(`${BASE}/account/register`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await shot('register');
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PW1);
  await page.fill('input[name=password2]', 'different password!');
  await page.click('button[type=submit]');
  ok((await page.textContent('#form .field-error')).includes('different'), 'register: mismatched passwords caught in the page (at the field)');
  await page.fill('input[name=password]', 'password123');
  await page.fill('input[name=password2]', 'password123');
  await page.click('button[type=submit]');
  await page.waitForFunction(() => document.getElementById('status').textContent.length > 0);
  ok((await status()).includes('too common'), 'register: a common password is refused by the API');
  await waitTurnstile();
  await page.fill('input[name=password]', PW1);
  await page.fill('input[name=password2]', PW1);
  await page.click('button[type=submit]');
  await page.waitForSelector('#check:not([hidden])');
  await shot('register-done');
  ok(true, 'register: "check your email"');

  // Verify (link from the mail; the token leaves the address bar).
  const vmail = await waitMail(EMAIL, 'Confirm');
  ok(vmail && !/<|tracking|img/i.test(vmail.Text), 'verify mail arrived, plain text');
  const vlink = vmail && linkIn(vmail);
  ok(vlink && vlink.startsWith(`${BASE}/account/verify?token=`), 'verify mail has the link');
  // One-step registration: the browser that registered confirms without the password and is
  // signed in on /account/. The sign-in form is tested below from a signed-out state.
  await page.goto(vlink);
  await page.waitForURL(`${BASE}/account/`);
  ok(!page.url().includes('token='), 'verify: token removed from the address bar');
  await page.waitForSelector('#overview:not([hidden])');
  await shot('verified');
  ok(true, 'verify: confirmed and signed in, no password');
  await ctx.clearCookies();

  // Sign in.
  await page.goto(`${BASE}/account/login`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await shot('login');
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PW1);
  await page.click('button[type=submit]');
  await page.waitForURL(`${BASE}/account/`);
  await page.waitForSelector('#overview:not([hidden])');
  ok((await page.textContent('#me-email')) === EMAIL, 'overview shows the email');
  const cookies = await ctx.cookies();
  const c = cookies.find((x) => x.name === '__Host-krunk_session');
  ok(c && c.httpOnly && c.secure && c.sameSite === 'Strict' && c.path === '/' && !c.domain.startsWith('.'), 'cookie: __Host-, HttpOnly, Secure, SameSite=Strict');
  ok(!(await page.evaluate(() => document.cookie)).includes('krunk_session'), 'cookie: not readable from script');
  await shot('overview');

  // Two-step verification on.
  await page.fill('#totp-start input[name=password]', PW1);
  await page.click('#totp-start button');
  await page.waitForSelector('#totp-confirm:not([hidden])');
  ok(await page.$('#totp-qr svg'), 'totp: QR code shown (SVG)');
  const secret = b32decode((await page.textContent('#totp-secret')).replace(/\s/g, ''));
  await shot('totp-setup');
  await page.fill('#totp-confirm input[name=code]', totp(secret));
  await page.click('#totp-confirm button[type=submit]');
  await page.waitForSelector('#totp-codes:not([hidden])');
  const codes = await page.$$eval('#totp-code-list li', (l) => l.map((x) => x.textContent));
  ok(codes.length === 10, 'totp: 10 backup codes shown');
  await shot('totp-on');
  ok(await waitMail(EMAIL, 'Two-step verification is on'), 'totp: notification mail');

  // Change password (needs a code now).
  await page.fill('#password input[name=current]', PW1);
  await page.fill('#password input[name=new]', PW2);
  await page.fill('#password input[name=new2]', PW2);
  await page.fill('#password input[name=totp]', codes[0]);
  await page.click('#password button');
  await page.waitForFunction(() => document.getElementById('status').textContent.includes('changed'));
  ok(true, 'password change with a backup code');
  ok(await waitMail(EMAIL, 'password was changed'), 'password change: notification mail');

  // Sign out, sign in again with the second step.
  await page.click('#logout');
  await page.waitForURL(`${BASE}/account/login`);
  const after = await page.evaluate(() => fetch('/api/v1/web/me', { credentials: 'same-origin' }).then((r) => r.status));
  ok(after === 401, 'signed out: web/me is 401');
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PW2);
  await page.click('button[type=submit]');
  await page.waitForSelector('#totp-row:not([hidden])');
  ok((await status()).includes('authenticator'), 'login: asks for the code');
  await shot('login-totp');
  await waitTurnstile();
  await page.fill('input[name=totp]', totp(secret, 1));
  await page.click('button[type=submit]');
  await page.waitForURL(`${BASE}/account/`);
  ok(true, 'login with password and code');

  // Turn it off again (password + code).
  await page.waitForSelector('#totp-off:not([hidden])');
  await page.fill('#totp-off input[name=password]', PW2);
  await page.fill('#totp-off input[name=code]', codes[1]);
  await page.click('#totp-off button');
  await page.waitForSelector('#totp-start:not([hidden])');
  ok(true, 'totp off');
  await page.click('#logout');
  await page.waitForURL(`${BASE}/account/login`);

  // Reset the password from the mail link.
  await page.goto(`${BASE}/account/reset`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await shot('reset-request');
  await page.fill('input[name=email]', EMAIL);
  await page.click('#form button');
  await page.waitForSelector('#done:not([hidden])');
  const rmail = await waitMail(EMAIL, 'Reset');
  const rlink = rmail && linkIn(rmail);
  ok(rlink && rlink.startsWith(`${BASE}/account/reset?token=`), 'reset mail has the link');
  await page.goto(rlink);
  await page.waitForSelector('#form-new:not([hidden])');
  await shot('reset-new');
  await page.fill('input[name=new_password]', PW3);
  await page.fill('input[name=new_password2]', PW3);
  await page.click('#form-new button');
  await page.waitForSelector('#done:not([hidden])');
  ok((await page.textContent('#done-text')).includes('changed'), 'reset: password set');

  // Resend page (nothing to send for a confirmed address, same answer).
  await page.goto(`${BASE}/account/resend`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await page.fill('input[name=email]', EMAIL);
  await page.click('#form button');
  await page.waitForSelector('#done:not([hidden])');
  await shot('resend-done');
  ok(true, 'resend: same answer');

  // Sign in with the reset password; the overview loads; privacy page.
  await page.goto(`${BASE}/account/login`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile();
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PW3);
  await page.click('button[type=submit]');
  await page.waitForURL(`${BASE}/account/`);
  ok(true, 'login with the reset password');
  await page.goto(`${BASE}/privacy`);
  ok((await page.textContent('h1')) === 'Privacy', 'privacy page');
  await shot('privacy');
  await page.goto(`${BASE}/`);
  await page.waitForSelector('header a[href="/account/"]'); // the restyled site's header (was .topnav)
  await shot('home');

  // A cross-site page can't post with the cookie (SameSite) and is refused by Origin anyway.
  const cross = await page.evaluate(async (base) => {
    const r = await fetch(base + '/api/v1/web/logout', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}', credentials: 'same-origin' });
    return r.status;
  }, BASE);
  ok(cross === 403, 'POST without the CSRF header is 403');

  // Chromium logs every 4xx/5xx fetch as a console error; the negative steps above cause a few on
  // purpose. Anything else (a CSP violation, a script error) fails the run.
  const expected = errors.filter((e) => /Failed to load resource: the server responded with a status of 4\d\d/.test(e));
  const real = errors.filter((e) => !expected.includes(e));
  ok(real.length === 0, `no CSP violations or script errors (${real.length}; ${expected.length} expected 4xx from negative steps)`);
  real.forEach((e) => console.log('   ', e));
  await browser.close();
  console.log(problems.length ? `${problems.length} FAILED` : 'ALL PASSED');
  process.exit(problems.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
