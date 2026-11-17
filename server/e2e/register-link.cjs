// End-to-end test of one-step registration (service 2026-11-17) on a staging stack: register →
// the mail link → signed in, and the Krunk that asked (the /link tab) linked by itself.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/register-link.cjs http://dind:10017 stage-browser <screenshot dir>
// Staging as for link.cjs (staging.env test values, server/e2e/mailsink.sh up). The browser sees
// the site as https://krunk.test (routed to the upstream, so the __Host- cookies work); "Krunk"
// is this script calling link/authorize and link/token with the user agent Krunk/0.23.0. Both go
// through the same upstream from this machine, so the server sees one client IP, as for a Krunk
// whose own client and /link tab run on one PC. The browser keeps Playwright's own user agent,
// as Krunk's tabs send the engine's.
//
// Prints the password entries per path: a password typed into a form counts once per form sent
// (the register form's "again" field is part of the same entry).
'use strict';
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const path = require('path');
const fs = require('fs');

const UPSTREAM = process.argv[2] || 'http://dind:10017';
const PROJECT = process.argv[3] || 'stage-browser';
const SHOTS = process.argv[4] || '/tmp/krunk-register-link';
const BASE = 'https://krunk.test';
const RUN = Date.now();
const EMAIL1 = `reg-krunk-${RUN}@example.org`;
const EMAIL2 = `reg-other-${RUN}@example.org`;
const PW = 'amber lantern orchard 7';
const KRUNK_UA = 'Krunk/0.23.0 (register-link.cjs)';

const problems = [];
function ok(cond, what) {
  if (!cond) { problems.push(what); console.log('FAIL', what); } else console.log('ok  ', what);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
fs.mkdirSync(SHOTS, { recursive: true });

function mailpit(apiPath) {
  const out = execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${apiPath}`]);
  return JSON.parse(out.toString());
}
async function waitMail(to, subjectPart) {
  for (let i = 0; i < 60; i++) {
    const list = mailpit('/api/v1/messages?limit=200').messages || [];
    const m = list.find((x) => (x.To || []).some((t) => t.Address === to) && x.Subject.includes(subjectPart));
    if (m) return mailpit(`/api/v1/message/${m.ID}`);
    await sleep(500);
  }
  return null;
}
function verifyLink(mail) {
  const m = mail && /(https:\/\/krunk\.test\/account\/verify\?token=[A-Za-z0-9_-]{43})/.exec(mail.Text);
  return m ? m[1] : null;
}

// Krunk's own client: no Origin, no cookies, the Krunk user agent.
async function krunk(p, body) {
  const r = await fetch(`${UPSTREAM}/api/v1/${p}`, {
    method: 'POST',
    headers: { 'User-Agent': KRUNK_UA, 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  let j = {};
  try { j = await r.json(); } catch (e) { j = {}; }
  return { status: r.status, body: j };
}
async function authorize() {
  return krunk('link/authorize', { platform: 'windows', label: 'Krunk 0.23.0 on Windows 11', client_version: '0.23.0' });
}
async function pollUntilDone(deviceCode, maxS) {
  const end = Date.now() + maxS * 1000;
  let interval = 5;
  while (Date.now() < end) {
    await sleep(interval * 1000);
    const r = await krunk('link/token', { device_code: deviceCode });
    if (r.status === 200) return r;
    if (r.body.error === 'slow_down') interval += 5;
    else if (r.body.error !== 'authorization_pending') return r;
  }
  return { status: 0, body: { error: 'timeout' } };
}

(async () => {
  const browser = await chromium.launch();
  const errors = [];
  let shotN = 0;
  async function newCtx() {
    const ctx = await browser.newContext({ viewport: { width: 1000, height: 900 } });
    await ctx.route(`${BASE}/**`, async (route) => {
      const u = new URL(route.request().url());
      const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
      await route.fulfill({ response });
    });
    return ctx;
  }
  async function newPage(ctx) {
    const page = await ctx.newPage();
    // Expected, not errors: Chrome logs the API's 400/401 answers the pages handle (the poll's
    // authorization_pending, /link before signing in, verify's password_required) and
    // Turnstile's sandboxed frame.
    const expected = (t) => /Failed to load resource: the server responded with a status of 40[01]/.test(t) ||
      /frame is sandboxed and the 'allow-scripts' permission is not set/.test(t);
    page.on('console', (m) => { if (m.type() === 'error' && !expected(m.text())) errors.push(`${page.url()}: ${m.text()}`); });
    page.on('pageerror', (e) => errors.push(`${page.url()}: ${e.message}`));
    return page;
  }
  async function shot(page, name) {
    shotN += 1;
    const file = path.join(SHOTS, `${String(shotN).padStart(2, '0')}-${name}.png`);
    await page.screenshot({ path: file, fullPage: true });
    console.log('shot', file);
  }
  async function waitTurnstile(page) {
    await page.waitForFunction(() => {
      const i = document.querySelector('[data-turnstile] input[name="cf-turnstile-response"]');
      return i && i.value.length > 0;
    }, null, { timeout: 30000 });
  }
  // Password entries per path.
  const entries = { registration: 0, otherBrowser: 0, existingUser: 0 };
  // Did a password field ever show? (Watched on the verify page and the /link re-auth form.)
  async function watchPasswordPrompt(page, selector) {
    await page.addInitScript((sel) => {
      // In sessionStorage, so a prompt on a page left since still counts.
      const check = () => {
        const f = document.querySelector(sel);
        if (f && !f.hidden && getComputedStyle(f).display !== 'none') sessionStorage.setItem('pwSeen', '1');
      };
      new MutationObserver(check).observe(document, { subtree: true, attributes: true, childList: true });
      document.addEventListener('DOMContentLoaded', check);
    }, selector);
  }

  try {
    // ---- (1) /link?code= → sign-in → Create account → register → "check your email" --------
    const a = await authorize();
    ok(a.status === 200 && a.body.user_code, `Krunk asks to sign in: code ${a.body.user_code}`);
    const pollA = pollUntilDone(a.body.device_code, 300);
    const ctx1 = await newCtx();
    const tab = await newPage(ctx1);
    await tab.goto(a.body.verification_uri_complete);
    await tab.waitForURL(/\/account\/login\?next=/);
    await tab.waitForSelector('#form:not([hidden])');
    const regHref = await tab.getAttribute('#to-register', 'href');
    ok(regHref === `/account/register?next=${encodeURIComponent('/link?code=' + a.body.user_code)}`, `"Create an account" keeps next: ${regHref}`);
    await shot(tab, 'register-link-1a-login-from-link');
    await tab.click('#to-register');
    await tab.waitForURL(/\/account\/register\?next=/);
    await tab.waitForSelector('#form:not([hidden])');
    ok((await tab.getAttribute('#to-login', 'href')).includes('next='), '"Sign in" on register keeps next too');
    await waitTurnstile(tab);
    await tab.fill('input[name=email]', EMAIL1);
    await tab.fill('input[name=password]', PW);
    await tab.fill('input[name=password2]', PW);
    entries.registration += 1;
    await tab.click('button[type=submit]');
    await tab.waitForSelector('#check:not([hidden])');
    const regCookie = (await ctx1.cookies()).find((c) => c.name === '__Host-krunk_reg');
    ok(regCookie && regCookie.httpOnly && regCookie.secure && regCookie.sameSite === 'Lax', 'register set __Host-krunk_reg (HttpOnly, Secure, Lax)');
    await shot(tab, 'register-link-1b-check-email');

    // ---- (2) the mail link in the same browser → signed in, no password; the tab flips ------
    const mail1 = await waitMail(EMAIL1, 'Confirm');
    ok(mail1 && mail1.Text.includes('Click to confirm your address and sign in'), 'verify mail: new wording');
    const link1 = verifyLink(mail1);
    const mailTab = await newPage(ctx1);
    await watchPasswordPrompt(mailTab, '#form');
    await mailTab.goto(link1);
    await mailTab.waitForURL(`${BASE}/account/`);
    await mailTab.waitForSelector('#overview:not([hidden])');
    ok(!(await mailTab.evaluate(() => sessionStorage.getItem('pwSeen') === '1')), 'same browser: no password field was shown');
    ok((await mailTab.textContent('#me-email')) === EMAIL1, 'same browser: signed in on /account/');
    await shot(mailTab, 'register-link-2a-verified-same-browser-account');
    await tab.waitForSelector('#done:not([hidden])', { timeout: 15000 });
    const tabText = await tab.textContent('#done');
    ok(/signed in/i.test(tabText) && /This Krunk browser is now signed in/.test(tabText), `the /link tab flipped: ${tabText.replace(/\s+/g, ' ').trim()}`);
    ok(tab.url().startsWith(`${BASE}/account/register`), 'the tab stays (no /link)');
    await shot(tab, 'register-link-2b-krunk-tab-linked');

    // ---- (3) Krunk gets its tokens; the device is listed; the device_linked mail ------------
    const tokens = await pollA;
    ok(tokens.status === 200 && tokens.body.access_token && tokens.body.refresh_token, `link/token: tokens (${tokens.status} ${tokens.body.error || ''})`);
    await mailTab.goto(`${BASE}/account/`);
    await mailTab.waitForSelector('#overview:not([hidden])');
    await mailTab.waitForFunction(() => /Krunk on Windows 11/.test(document.body.innerText), null, { timeout: 15000 });
    ok(true, 'the device is in the account\'s device list');
    const linkedMail = await waitMail(EMAIL1, 'new device was linked');
    ok(!!linkedMail, 'device_linked mail arrived');
    await shot(mailTab, 'register-link-3-device-listed');

    // ---- (4) another registration, its link opened in a fresh browser: one password --------
    const ctx2 = await newCtx();
    const regTab = await newPage(ctx2);
    await regTab.goto(`${BASE}/account/register`);
    await regTab.waitForSelector('#form:not([hidden])');
    await waitTurnstile(regTab);
    await regTab.fill('input[name=email]', EMAIL2);
    await regTab.fill('input[name=password]', PW);
    await regTab.fill('input[name=password2]', PW);
    entries.otherBrowser += 1;
    await regTab.click('button[type=submit]');
    await regTab.waitForSelector('#check:not([hidden])');
    const link2 = verifyLink(await waitMail(EMAIL2, 'Confirm'));
    const ctx3 = await newCtx();
    const phone = await newPage(ctx3);
    await phone.goto(link2);
    await phone.waitForSelector('#form:not([hidden])');
    await shot(phone, 'register-link-4a-other-browser-asks-password');
    await phone.fill('input[name=password]', PW);
    entries.otherBrowser += 1;
    await phone.click('button[type=submit]');
    await phone.waitForURL(`${BASE}/account/`);
    await phone.waitForSelector('#overview:not([hidden])');
    ok((await phone.textContent('#me-email')) === EMAIL2, 'other browser: signed in after one password');
    await shot(phone, 'register-link-4b-other-browser-signed-in');
    await regTab.waitForURL(`${BASE}/account/`, { timeout: 15000 });
    await regTab.waitForSelector('#overview:not([hidden])');
    ok((await regTab.textContent('#me-email')) === EMAIL2, 'the registering tab signed in by itself');
    await shot(regTab, 'register-link-4c-registering-tab-signed-in');

    // ---- (5) existing user: sign in from Krunk's /link → tick + Approve, no re-auth --------
    const b = await authorize();
    const pollB = pollUntilDone(b.body.device_code, 120);
    const ctx4 = await newCtx();
    const page = await newPage(ctx4);
    await watchPasswordPrompt(page, '#reauth');
    await page.goto(b.body.verification_uri_complete);
    await page.waitForURL(/\/account\/login\?next=/);
    await page.waitForSelector('#form:not([hidden])');
    await waitTurnstile(page);
    await page.fill('input[name=email]', EMAIL2);
    await page.fill('input[name=password]', PW);
    entries.existingUser += 1;
    await page.click('button[type=submit]');
    await page.waitForURL(`${BASE}/link`);
    await page.waitForSelector('#approve-form:not([hidden])');
    await page.check('input[name=same]');
    await page.click('#approve-form button[type=submit]');
    await page.waitForSelector('#done:not([hidden])', { timeout: 15000 });
    ok(!(await page.evaluate(() => sessionStorage.getItem('pwSeen') === '1')), 'existing user: no re-auth prompt on approve');
    ok(/Approved/.test(await page.textContent('#done')), 'existing user: approved');
    await shot(page, 'register-link-5-existing-user-approved');
    const tb = await pollB;
    ok(tb.status === 200, 'existing user: Krunk got its tokens');

    ok(errors.length === 0, `no console errors${errors.length ? ': ' + errors.join(' | ') : ''}`);
  } catch (e) {
    problems.push(`exception: ${e.message}`);
    console.log('FAIL exception', e);
  } finally {
    await browser.close();
  }
  console.log(`password entries: registration path ${entries.registration} (expected 1), ` +
    `other-browser verify ${entries.otherBrowser} (expected 2), existing user ${entries.existingUser} (expected 1)`);
  if (entries.registration !== 1 || entries.otherBrowser !== 2 || entries.existingUser !== 1) problems.push('password counts');
  console.log(problems.length ? `${problems.length} problem(s): ${problems.join('; ')}` : 'all ok');
  process.exit(problems.length ? 1 : 0);
})();
