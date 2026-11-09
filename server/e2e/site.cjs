// The website's acceptance test (account pages: 0.9.0 restyle; home, /privacy, error pages: 0.10.0)
// on a staging stack:
//  A. every page × light/dark × 360/900/1280 under the real CSP: 0 console errors, 0 CSP
//     violations, [hidden] is never displayed, no sideways scrolling, 44 px tap targets at 360 px,
//     Cloudflare's Turnstile script loads on exactly the 4 Turnstile pages;
//  B. axe-core (WCAG 2.1 A/AA) on every page in both schemes at 360 and 1280 px and on the open
//     panels: 0 serious or critical issues (a separate pass with CSP bypassed, only to inject axe);
//  C. a keyboard-only walkthrough of /account/ (Tab order, visible focus, the section nav, rename,
//     "Sign out everywhere else" with the re-auth form);
//  D. inline validation, two-step setup with Copy key, backup codes Copy and Download .txt, the
//     sync state with web/me.sync absent and present, reduced motion, long names at 360 px and
//     "This device";
//  E. (0.10.0) the public site: HTML 404 for site paths while /api, /updates and /download answer
//     as before; the download box with a release, without sizes, with none, with unsafe links and
//     for a Krunk visitor that is outdated, current or ahead.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/site.cjs http://dind:10017 stage-browser <seed state.json> <out dir>
// AXE_JS: axe.min.js (default: .pipeline/tools/axe, axe-core 4.14.0 from npm). Staging only.
'use strict';
const { chromium } = require('playwright');
const { execFileSync } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const [UPSTREAM, PROJECT, STATE, OUT] = process.argv.slice(2);
const AXE = process.env.AXE_JS || '/work/projects/browser/.pipeline/tools/axe/node_modules/axe-core/axe.min.js';
const BASE = 'https://krunk.test';
const TS = 'XXXX.DUMMY.TOKEN.XXXX';
const THEMES = ['light', 'dark'];
const WIDTHS = [360, 900, 1280];
const TURNSTILE_PAGES = ['login', 'register', 'reset-request', 'resend'];

const lines = [];
const problems = [];
function log(s) { lines.push(s); console.log(s); }
function ok(cond, what) {
  if (cond) log('ok   ' + what); else { problems.push(what); log('FAIL ' + what); }
  return cond;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- Accounts --------------------------------------------------------------------------------
function b32decode(s) {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = 0, buf = 0; const out = [];
  for (const c of s.replace(/[\s=]/g, '')) { buf = (buf << 5) | A.indexOf(c); bits += 5; if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); } }
  return Buffer.from(out);
}
// A TOTP step is accepted once per account: `holder.last_step` remembers the last one used.
async function freshTotp(holder) {
  let step = Math.floor(Date.now() / 1000 / 30);
  while (step <= (holder.last_step || 0)) { await sleep(1000); step = Math.floor(Date.now() / 1000 / 30); }
  holder.last_step = step;
  const msg = Buffer.alloc(8); msg.writeBigUInt64BE(BigInt(step));
  const h = crypto.createHmac('sha1', b32decode(holder.totp_secret)).update(msg).digest();
  const o = h[h.length - 1] & 15;
  return String(((h.readUInt32BE(o) & 0x7fffffff) % 1000000)).padStart(6, '0');
}
async function web(method, p, body, cookie) {
  const headers = { Origin: BASE };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (cookie) headers.Cookie = cookie;
  const r = await fetch(`${UPSTREAM}/api/v1/web/${p}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const set = r.headers.get('set-cookie');
  let j = {};
  try { j = await r.json(); } catch (e) { j = {}; }
  return { status: r.status, body: j, cookie: set ? set.split(';')[0] : null };
}
function mailpit(p) {
  return JSON.parse(execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${p}`]).toString());
}
async function newAccount() {
  const a = { email: `site-${Date.now()}@example.org`, password: 'site test password 9' };
  await web('POST', 'register', { email: a.email, password: a.password, turnstile: TS });
  let token = null;
  for (let i = 0; i < 40 && !token; i++) {
    const m = (mailpit('/api/v1/messages?limit=50').messages || []).find((x) => (x.To || []).some((t) => t.Address === a.email));
    if (m) token = /token=([A-Za-z0-9_-]{43})/.exec(mailpit(`/api/v1/message/${m.ID}`).Text)[1];
    else await sleep(500);
  }
  if ((await web('POST', 'verify-email', { token, password: a.password })).status !== 200) throw new Error('verify');
  return a;
}
async function loginCookie(a) {
  const body = { email: a.email, password: a.password, turnstile: TS };
  if (a.totp_secret) body.totp = await freshTotp(a);
  const r = await web('POST', 'login', body);
  if (r.status !== 200) throw new Error(`login ${r.status} ${JSON.stringify(r.body)}`);
  return r.cookie;
}
async function newCode() {
  const r = await fetch(`${UPSTREAM}/api/v1/link/authorize`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'User-Agent': 'Krunk/0.9.0 (site.cjs)' },
    body: JSON.stringify({ platform: 'windows', label: 'Krunk 0.9.0 on Windows 11', client_version: '0.9.0' }),
  });
  return (await r.json()).user_code;
}
function psql(sql) {
  return execFileSync('docker', ['exec', `${PROJECT}-db-1`, 'psql', '-U', 'krunk', '-d', 'krunk', '-tAc', sql]).toString().trim();
}

// ---- Browser ---------------------------------------------------------------------------------
let browser;
async function context(cookie, extra) {
  const ctx = await browser.newContext(Object.assign({ viewport: { width: 1280, height: 900 } }, extra || {}));
  await ctx.route(`${BASE}/**`, async (route) => {
    const u = new URL(route.request().url());
    const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
    await route.fulfill({ response });
  });
  if (cookie) {
    const [name, value] = cookie.split('=');
    await ctx.addCookies([{ name, value, url: `${BASE}/`, secure: true, httpOnly: true, sameSite: 'Strict' }]);
  }
  return ctx;
}
// A page that records console errors, page errors, CSP violations and Turnstile requests.
async function watched(ctx, label) {
  const page = await ctx.newPage();
  page.krunk = { errors: [], turnstile: 0 };
  page.on('console', (m) => {
    // The 404 page's own status is reported as a failed load; that is the point of the page.
    if (m.type() === 'error' && !(label.startsWith('404') && /status of 404/.test(m.text()))) page.krunk.errors.push(`${label}: ${m.text()}`);
  });
  page.on('pageerror', (e) => page.krunk.errors.push(`${label}: pageerror ${e.message}`));
  page.on('request', (r) => { if (new URL(r.url()).host === 'challenges.cloudflare.com') page.krunk.turnstile++; });
  await page.addInitScript(() => {
    document.addEventListener('securitypolicyviolation', (e) => console.error(`CSP violation: ${e.violatedDirective} ${e.blockedURI}`));
  });
  return page;
}
async function layoutChecks(page, label, width) {
  const r = await page.evaluate((w) => {
    const hiddenShown = [...document.querySelectorAll('[hidden]')].filter((e) => getComputedStyle(e).display !== 'none').map((e) => e.id || e.className || e.tagName);
    const overflow = document.documentElement.scrollWidth - window.innerWidth;
    const small = w <= 360 ? [...document.querySelectorAll('.secnav a, .items .actions button, details.removed summary, .site-bar nav a, .site-foot nav a, .toc a, .faq summary, .btn, .checksums summary')]
      .filter((e) => e.offsetParent !== null && e.getBoundingClientRect().height < 44).map((e) => e.textContent.trim()) : [];
    return { hiddenShown, overflow, small };
  }, width);
  ok(r.hiddenShown.length === 0, `${label}: [hidden] never displayed ${r.hiddenShown.join(',')}`);
  ok(r.overflow <= 1, `${label}: no sideways scrolling (${r.overflow} px)`);
  if (width <= 360) ok(r.small.length === 0, `${label}: tap targets ≥ 44 px ${r.small.join('|')}`);
}
const AXE_SRC = () => fs.readFileSync(AXE, 'utf8');
const axeSummary = [];
async function axe(page, label) {
  if (!(await page.evaluate(() => !!window.axe))) await page.addScriptTag({ content: AXE_SRC() });
  const res = await page.evaluate(async () => {
    const r = await window.axe.run({ exclude: [['.turnstile']] }, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] } });
    return r.violations.map((v) => ({ id: v.id, impact: v.impact, n: v.nodes.length, where: v.nodes.slice(0, 3).map((n) => n.target.join(' ')) }));
  });
  const bad = res.filter((v) => v.impact === 'serious' || v.impact === 'critical');
  axeSummary.push({ page: label, violations: res });
  ok(bad.length === 0, `axe ${label}: 0 serious/critical (${res.length} total${res.length ? ': ' + res.map((v) => `${v.id}/${v.impact}`).join(', ') : ''})`);
  for (const v of bad) log(`       ${v.id} ${v.impact} ×${v.n} ${v.where.join(' ; ')}`);
}

// ---- A + B: every page -------------------------------------------------------------------------
const PAGES = [
  // 0.10.0: the public pages (restyled) and the HTML 404.
  { name: 'home', url: '/', ready: '#download .btn.big, #download .none' },
  { name: 'privacy', url: '/privacy', ready: '.toc a' },
  { name: '404', url: '/no-such-page', ready: '.errcode' },
  { name: 'login', url: '/account/login', ready: '#form:not([hidden])' },
  { name: 'register', url: '/account/register', ready: '#form:not([hidden])' },
  { name: 'resend', url: '/account/resend', ready: '#form:not([hidden])' },
  { name: 'reset-request', url: '/account/reset', ready: '#form:not([hidden])' },
  { name: 'reset-new-password', url: '/account/reset?token=' + 'A'.repeat(43), ready: '#form-new:not([hidden])' },
  { name: 'verify', url: '/account/verify?token=' + 'A'.repeat(43), ready: '#form:not([hidden])' },
  { name: 'account', url: '/account/', ready: '#devices-active li', signed: true },
  { name: 'link', url: '/link?code=', ready: '#approve-form:not([hidden])', signed: true, code: true },
];

async function allPages(seed) {
  const cookie = await loginCookie(seed);
  const turnstileSeen = new Set();
  for (const p of PAGES) {
    for (const theme of THEMES) {
      // The /link code lookups are limited (5 per session in 10 minutes): one load per scheme,
      // the widths change in place.
      const widths = p.code ? [WIDTHS] : WIDTHS.map((w) => [w]);
      for (const ws of widths) {
        const ctx = await context(p.signed ? cookie : null, { colorScheme: theme, viewport: { width: ws[0], height: 900 } });
        const page = await watched(ctx, `${p.name}-${theme}`);
        await page.goto(BASE + p.url + (p.code ? await newCode() : ''), { waitUntil: 'networkidle' });
        await page.waitForSelector(p.ready, { timeout: 15000 });
        await sleep(300);
        for (const w of ws) {
          await page.setViewportSize({ width: w, height: 900 });
          await sleep(150);
          await layoutChecks(page, `${p.name} ${theme} ${w}`, w);
        }
        if (page.krunk.turnstile) turnstileSeen.add(p.name);
        ok(page.krunk.errors.length === 0, `${p.name} ${theme} ${ws.join('/')}: 0 console errors, 0 CSP violations ${page.krunk.errors.join(' | ')}`);
        await ctx.close();
      }
    }
  }
  ok(JSON.stringify([...turnstileSeen].sort()) === JSON.stringify([...TURNSTILE_PAGES].sort()),
    `Turnstile loads on exactly ${TURNSTILE_PAGES.join(', ')} (seen: ${[...turnstileSeen].join(', ')})`);

  // B: axe, with CSP bypassed only so that the axe script can be injected.
  for (const p of PAGES) {
    for (const theme of THEMES) {
      const ctx = await context(p.signed ? cookie : null, { colorScheme: theme, bypassCSP: true, viewport: { width: 1280, height: 900 } });
      const page = await ctx.newPage();
      await page.goto(BASE + p.url + (p.code ? await newCode() : ''), { waitUntil: 'networkidle' });
      await page.waitForSelector(p.ready, { timeout: 15000 });
      await sleep(300);
      for (const w of [1280, 360]) {
        await page.setViewportSize({ width: w, height: 900 });
        await sleep(150);
        await axe(page, `${p.name} ${theme} ${w}`);
      }
      await ctx.close();
    }
  }
}

// ---- C: keyboard only --------------------------------------------------------------------------
async function focused(page) {
  return page.evaluate(() => {
    const e = document.activeElement;
    if (!e || e === document.body) return { d: 'body', visible: false };
    const cs = getComputedStyle(e);
    const ring = cs.outlineStyle !== 'none' && parseFloat(cs.outlineWidth) > 0;
    const name = (e.getAttribute('aria-label') || e.textContent || e.name || e.id || '').trim().replace(/\s+/g, ' ').slice(0, 40);
    return { d: `${e.tagName.toLowerCase()}${e.id ? '#' + e.id : ''} "${name}"`, visible: ring && e.matches(':focus-visible') };
  });
}
async function tabTo(page, re, max) {
  for (let i = 0; i < (max || 80); i++) {
    await page.keyboard.press('Tab');
    const f = await focused(page);
    if (re.test(f.d)) return f;
  }
  return null;
}
async function keyboard(seed) {
  const cookie = await loginCookie(seed);
  const ctx = await context(cookie);
  const page = await watched(ctx, 'keyboard');
  await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
  await page.waitForSelector('#devices-active li');
  // The whole Tab order, each stop with a visible focus ring.
  const order = [];
  let invisible = [];
  for (let i = 0; i < 70; i++) {
    await page.keyboard.press('Tab');
    const f = await focused(page);
    if (f.d === 'body' || (order.length && f.d === order[0])) break;
    order.push(f.d);
    if (!f.visible) invisible.push(f.d);
  }
  log('       Tab order: ' + order.join(' → '));
  fs.writeFileSync(path.join(OUT, 'keyboard-tab-order.txt'), order.join('\n') + '\n');
  ok(invisible.length === 0, `keyboard: every stop shows a focus ring (${order.length} stops) ${invisible.join(' | ')}`);
  const want = ['"Krunk"', '"Privacy"', '#logout', '"Overview"', '"Devices & sync"', '"Signed-in browsers"', '"Security"',
    'Rename', 'Remove', 'Removed devices', '#revoke-all-open', 'Change password'];
  let at = -1;
  const missing = [];
  for (const w of want) {
    const j = order.findIndex((d, k) => k > at && d.includes(w));
    if (j < 0) missing.push(w); else at = j;
  }
  ok(missing.length === 0, `keyboard: logical order header → section nav → devices → browsers → security ${missing.join(', ')}`);

  // The section nav by keyboard.
  await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
  await page.waitForSelector('#devices-active li');
  ok(!!(await tabTo(page, /"Security"/)), 'keyboard: reach the "Security" section link');
  await page.keyboard.press('Enter');
  await sleep(700);
  const sec = await page.evaluate(() => ({ hash: location.hash, top: document.getElementById('security').getBoundingClientRect().top,
    current: (document.querySelector('#secnav a[aria-current="true"]') || {}).textContent }));
  ok(sec.hash === '#security' && sec.top < 300 && sec.current === 'Security', `keyboard: Enter on "Security" scrolls there and marks it (${JSON.stringify(sec)})`);

  // Rename by keyboard (and back).
  for (const name of ['Seed PC (keyboard)', 'Seed PC renamed']) {
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#devices-active li');
    ok(!!(await tabTo(page, /Rename /)), 'keyboard: reach Rename');
    await page.keyboard.press('Enter');
    const f = await focused(page);
    ok(/input/.test(f.d) && f.visible, `keyboard: Rename puts the focus in the name field (${f.d})`);
    await page.keyboard.press('Control+A');
    await page.keyboard.type(name);
    await page.keyboard.press('Enter');
    await page.waitForFunction((n) => [...document.querySelectorAll('#devices li .name')].some((x) => x.textContent === n), name);
    const st = await page.textContent('#status');
    ok(st.includes('Renamed'), `keyboard: renamed to "${name}"; banner "${st}"`);
    ok((await focused(page)).d.includes('#status'), 'keyboard: the focus moves to the banner (role=status) after the list is rebuilt');
  }

  // "Sign out everywhere else" with the re-auth form, keyboard only.
  psql(`UPDATE web_sessions SET reauth_at = now() - interval '11 minutes' WHERE account_id = (SELECT id FROM accounts WHERE email = '${seed.email}')`);
  await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
  await page.waitForSelector('#sessions li');
  ok(!!(await tabTo(page, /#revoke-all-open/)), 'keyboard: reach "Sign out everywhere else"');
  await page.keyboard.press('Enter');
  let f = await focused(page);
  ok(f.d.includes('Sign out everywhere else') && f.d.startsWith('button') && !f.d.includes('#revoke-all-open'), `keyboard: the confirm panel opens with the focus on its button (${f.d})`);
  ok((await page.getAttribute('#revoke-all-open', 'aria-expanded')) === 'true', 'keyboard: aria-expanded=true');
  await page.keyboard.press('Enter');
  await page.waitForSelector('#reauth:not([hidden])');
  f = await focused(page);
  ok(f.d.startsWith('input') && f.visible, `keyboard: re-auth asks for the password, focused (${f.d})`);
  const placed = await page.evaluate(() => document.getElementById('revoke-all').nextElementSibling === document.getElementById('reauth'));
  ok(placed, 'keyboard: the re-auth form sits right under the action');
  await page.keyboard.type(seed.password);
  await page.keyboard.press('Tab');
  await page.keyboard.type(await freshTotp(seed));
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.getElementById('status').textContent.includes('signed out'), null, { timeout: 15000 });
  ok(true, `keyboard: re-auth by keyboard, then "${(await page.textContent('#status')).trim()}"`);
  const role = await page.getAttribute('#status', 'role');
  ok(role === 'status', `banner role=${role} for a success`);
  // The list is rebuilt right after the banner (me, devices and sessions reload together).
  await page.waitForFunction(() => document.querySelectorAll('#sessions li').length === 1, null, { timeout: 10000 }).catch(() => {});
  const webCount = await page.$$eval('#sessions li', (l) => l.length);
  ok(webCount === 1, `keyboard: one website session left (this one): ${webCount}`);
  ok(page.krunk.errors.length === 0, 'keyboard walkthrough: 0 console errors ' + page.krunk.errors.join(' | '));
  await ctx.close();
}

// ---- D: the rest -------------------------------------------------------------------------------
async function features(seed) {
  // Inline validation: nothing is sent while a field is wrong.
  {
    const ctx = await context(null);
    const page = await watched(ctx, 'validation');
    let posted = 0;
    page.on('request', (r) => { if (r.method() === 'POST' && r.url().includes('/api/v1/web/')) posted++; });
    await page.goto(BASE + '/account/register', { waitUntil: 'networkidle' });
    await page.waitForSelector('#form:not([hidden])');
    await page.click('#form button[type=submit]');
    const errs = await page.$$eval('#form [aria-invalid="true"]', (l) => l.map((x) => x.name));
    ok(JSON.stringify(errs) === '["email","password","password2"]', `validation: empty register form → 3 field errors (${errs})`);
    ok((await focused(page)).d.includes('input'), 'validation: the focus is on the first wrong field');
    const desc = await page.evaluate(() => {
      const i = document.querySelector('#form input[name=email]');
      return (i.getAttribute('aria-describedby') || '').split(' ').map((id) => (document.getElementById(id) || {}).textContent).join('|');
    });
    ok(desc.includes('Please fill this in.'), `validation: message tied by aria-describedby (${desc})`);
    await page.fill('input[name=email]', 'not-an-address');
    await page.fill('input[name=password]', 'short');
    await page.fill('input[name=password2]', 'short');
    await page.click('#form button[type=submit]');
    const msgs = await page.$$eval('#form .field-error', (l) => l.map((x) => x.textContent));
    ok(msgs.some((m) => m.includes('email address like')) && msgs.some((m) => m.includes('at least 10')), `validation: bad email and short password (${msgs.join(' | ')})`);
    await page.fill('input[name=email]', 'a@example.org');
    await page.fill('input[name=password]', 'long enough password');
    await page.fill('input[name=password2]', 'long enough passwort');
    await page.click('#form button[type=submit]');
    const mm = await page.$$eval('#form .field-error', (l) => l.map((x) => x.textContent));
    ok(mm.length === 1 && mm[0].includes('different'), `validation: different passwords (${mm})`);
    ok(posted === 0, `validation: nothing was sent (${posted} POSTs)`);
    await ctx.close();
  }

  // Two-step setup on a fresh account: Copy key, the code's own check, codes Copy and Download.
  const acct = await newAccount();
  const cookie = await loginCookie(acct);
  {
    const ctx = await context(cookie, { acceptDownloads: true, permissions: ['clipboard-read', 'clipboard-write'] });
    const page = await watched(ctx, 'totp');
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#totp-start:not([hidden])');
    await page.fill('#totp-start input[name=password]', acct.password);
    await page.click('#totp-start button[type=submit]');
    await page.waitForSelector('#totp-confirm:not([hidden])');
    ok(!!(await page.$('#totp-qr svg')), 'totp: QR code in the panel');
    const shown = (await page.textContent('#totp-secret')).trim();
    const key = await page.getAttribute('#totp-secret', 'data-key');
    ok(/^([A-Z2-7]{4} )+[A-Z2-7]{1,4}$/.test(shown) && shown.replace(/ /g, '') === key, `totp: the key in groups of 4 (${shown.slice(0, 14)}…)`);
    await page.click('#totp-copy');
    await sleep(300);
    ok((await page.evaluate(() => navigator.clipboard.readText())) === key, 'totp: Copy key puts the key (no spaces) on the clipboard');
    await page.fill('#totp-confirm input[name=code]', '12345');
    await page.click('#totp-confirm button[type=submit]');
    ok((await page.textContent('#totp-confirm .field-error')).includes('6 digits'), 'totp: a 5-digit code is caught in the page');
    await page.screenshot({ path: path.join(OUT, 'panel-totp-setup.png'), fullPage: false });
    acct.totp_secret = key;
    await page.fill('#totp-confirm input[name=code]', await freshTotp(acct));
    await page.click('#totp-confirm button[type=submit]');
    await page.waitForSelector('#totp-codes:not([hidden])');
    const codes = await page.$$eval('#totp-code-list li', (l) => l.map((x) => x.textContent));
    ok(codes.length === 10, 'totp: 10 backup codes');
    await page.click('#codes-copy');
    await sleep(300);
    const clip = await page.evaluate(() => navigator.clipboard.readText());
    ok(codes.every((c) => clip.includes(c)) && clip.includes(acct.email), 'codes: Copy puts all 10 codes (and the address) on the clipboard');
    const [dl] = await Promise.all([page.waitForEvent('download'), page.click('#codes-download')]);
    const file = path.join(OUT, 'backup-codes-download.txt');
    await dl.saveAs(file);
    const txt = fs.readFileSync(file, 'utf8');
    ok(dl.suggestedFilename() === 'krunk-backup-codes.txt' && codes.every((c) => txt.includes(c)), `codes: Download .txt (${dl.suggestedFilename()}, ${txt.split('\n').length} lines)`);
    fs.writeFileSync(file, txt.replace(/[a-z0-9]{4}-[a-z0-9]{4}/gi, 'xxxx-xxxx'));
    await page.screenshot({ path: path.join(OUT, 'panel-backup-codes.png'), fullPage: false });
    ok(page.krunk.errors.length === 0, 'totp + codes: 0 console errors, 0 CSP violations ' + page.krunk.errors.join(' | '));
    await ctx.close();
  }
  // axe on the open panels (two-step setup, codes, sign out everywhere, rename, re-auth), both schemes.
  const c2 = await loginCookie(acct);
  for (const theme of THEMES) {
    const ctx = await context(c2, { colorScheme: theme, bypassCSP: true });
    const page = await ctx.newPage();
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#overview:not([hidden])');
    await page.evaluate(() => {
      for (const id of ['totp-confirm', 'totp-codes', 'revoke-all', 'reauth']) document.getElementById(id).hidden = false;
      for (let i = 0; i < 10; i++) { const li = document.createElement('li'); li.textContent = 'abcd-efgh'; document.getElementById('totp-code-list').appendChild(li); }
      document.getElementById('totp-secret').textContent = 'JBSW Y3DP EHPK 3PXP';
    });
    await page.click('#password button[type=submit]');
    await axe(page, `account panels + field errors ${theme} 1280`);
    await page.setViewportSize({ width: 360, height: 900 });
    await axe(page, `account panels + field errors ${theme} 360`);
    await ctx.close();
  }

  // Sync state: web/me.sync absent (the server before migration 0004), on, and securing.
  const seedCookie = await loginCookie(seed);
  const cases = [
    { name: 'absent', sync: undefined, tile: 'Off', title: 'Sync is off — turn it on in Krunk (Settings → Sync).', badge: false },
    { name: 'off', sync: { has_keyset: false, records: 0, bytes: 0, rotation_pending: false }, tile: 'Off', title: 'Sync is off — turn it on in Krunk (Settings → Sync).', badge: false },
    { name: 'on', sync: { has_keyset: true, records: 1204, bytes: 3250585, rotation_pending: false }, tile: 'On', title: 'Sync is on', tileD: '1,204 items · 3.1 MB', badge: true },
    { name: 'securing', sync: { has_keyset: true, records: 10, bytes: 2048, rotation_pending: true }, tile: 'Securing', title: 'Securing your account…', badge: true },
  ];
  for (const c of cases) {
    const ctx = await context(seedCookie);
    await ctx.route(`${BASE}/api/v1/web/me`, async (route) => {
      const response = await route.fetch({ url: `${UPSTREAM}/api/v1/web/me` });
      const j = await response.json();
      if (c.sync === undefined) delete j.sync; else j.sync = c.sync;
      await route.fulfill({ response, json: j });
    });
    const page = await watched(ctx, 'sync-' + c.name);
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#devices-active li');
    const v = await page.evaluate(() => ({ tile: document.getElementById('tile-sync').textContent, d: document.getElementById('tile-sync-d').textContent,
      title: document.getElementById('sync-title').textContent, badge: !!document.querySelector('#devices-active .badge.warn') }));
    ok(v.tile === c.tile && v.title === c.title && (!c.tileD || v.d === c.tileD) && v.d.trim() !== '' && v.badge === c.badge,
      `sync state ${c.name}: tile "${v.tile}" / "${v.d}", "${v.title}", no-keys badge ${v.badge}`);
    if (c.name === 'absent' || c.name === 'securing') await page.screenshot({ path: path.join(OUT, `sync-${c.name}.png`), fullPage: false, clip: { x: 0, y: 0, width: 1280, height: 760 } });
    ok(page.krunk.errors.length === 0, `sync state ${c.name}: 0 console errors`);
    await ctx.close();
  }

  // Reduced motion: the busy indicator stands still; aria-busy and disabled either way.
  for (const motion of ['no-preference', 'reduce']) {
    const ctx = await context(seedCookie, { reducedMotion: motion });
    const page = await ctx.newPage();
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#password');
    let anim = '';
    await page.route(`${BASE}/api/v1/web/password/change`, async (route) => {
      anim = await page.evaluate(() => {
        const b = document.querySelector('#password button[type=submit]');
        return `${b.getAttribute('aria-busy')}/${b.disabled}/${getComputedStyle(b, '::before').animationName}`;
      });
      await route.fulfill({ status: 401, json: { error: 'unauthorized', message: 'The password is wrong.' } });
    });
    await page.fill('#password input[name=current]', 'whatever password');
    await page.fill('#password input[name=new]', 'new password long');
    await page.fill('#password input[name=new2]', 'new password long');
    await page.fill('#password input[name=totp]', '123456');
    await page.click('#password button[type=submit]');
    await page.waitForFunction(() => document.getElementById('status').textContent !== '');
    ok(anim === (motion === 'reduce' ? 'true/true/none' : 'true/true/spin'), `busy state with reduced motion ${motion}: aria-busy/disabled/animation = ${anim}`);
    ok((await page.getAttribute('#status', 'role')) === 'alert', 'an error banner has role=alert');
    await ctx.close();
  }

  // Long names at 360 px: ellipsis with the full text in a title; the header email likewise at 900.
  {
    const long = 'W'.repeat(64);
    const ctx = await context(seedCookie, { viewport: { width: 360, height: 900 } });
    await ctx.route(`${BASE}/api/v1/web/devices`, async (route) => {
      const response = await route.fetch({ url: `${UPSTREAM}/api/v1/web/devices` });
      const j = await response.json();
      j.forEach((d) => { if (!d.revoked_at) d.label = long; });
      await route.fulfill({ response, json: j });
    });
    await ctx.route(`${BASE}/api/v1/web/me`, async (route) => {
      const response = await route.fetch({ url: `${UPSTREAM}/api/v1/web/me` });
      const j = await response.json();
      j.email = 'a-very-long-address-for-the-header-check-' + 'x'.repeat(30) + '@example.org';
      await route.fulfill({ response, json: j });
    });
    const page = await watched(ctx, 'long');
    await page.goto(BASE + '/account/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#devices-active li');
    const n = await page.$eval('#devices-active .name', (e) => ({ cut: e.scrollWidth > e.clientWidth, title: e.title, ell: getComputedStyle(e).textOverflow }));
    ok(n.cut && n.title === long && n.ell === 'ellipsis', `360 px: a 64-character device name is cut with an ellipsis, full text in title (${JSON.stringify({ cut: n.cut, ell: n.ell })})`);
    await layoutChecks(page, 'long names 360', 360);
    await page.screenshot({ path: path.join(OUT, 'long-names-360.png'), fullPage: false });
    await page.setViewportSize({ width: 900, height: 900 });
    await sleep(200);
    const w = await page.$eval('#who', (e) => ({ cut: e.scrollWidth > e.clientWidth, title: e.title, ell: getComputedStyle(e).textOverflow }));
    ok(w.cut && w.title.startsWith('a-very-long') && w.ell === 'ellipsis', 'header: a long email is cut with an ellipsis, full text in title');
    await ctx.close();
  }

  // "This device": Krunk opens /account/?this=<device id>.
  {
    const ctx = await context(seedCookie);
    const page = await watched(ctx, 'this-device');
    await page.goto(BASE + '/account/?this=' + seed.devices.kept, { waitUntil: 'networkidle' });
    await page.waitForSelector('#devices-active li');
    const b = await page.$$eval('#devices-active .badge.self', (l) => l.map((x) => x.textContent));
    ok(b.length === 1 && b[0] === 'This device' && !page.url().includes('this='), `"This device" on the kept device; the address is cleaned (${page.url()})`);
    await ctx.close();
  }
}

// ---- E: the public site (0.10.0): error statuses and the download box --------------------------
async function publicSite() {
  const get = async (p) => { const r = await fetch(UPSTREAM + p, { redirect: 'manual' }); return { status: r.status, type: r.headers.get('content-type') || '', body: await r.text() }; };
  let r = await get('/no-such-page');
  ok(r.status === 404 && r.type.startsWith('text/html') && r.body.includes("This page doesn't exist"), `unknown site path: 404 with the HTML page (${r.status} ${r.type})`);
  r = await get('/api/v1/no-such-thing');
  ok(r.status === 404 && r.type.startsWith('application/json'), `unknown /api path: 404 JSON (${r.status} ${r.type} ${r.body.slice(0, 60)})`);
  r = await get('/updates/win-x64/no-such-file.json');
  ok(r.status === 404 && r.body.trim() === '404 Not Found', `missing update file: 404 plain as before (${r.status} "${r.body.trim().slice(0, 40)}")`);
  r = await get('/download/0.0.0/no-such-file.exe');
  ok(r.status === 404 && r.body.trim() === '404 Not Found', `missing download: 404 plain as before (${r.status} "${r.body.trim().slice(0, 40)}")`);
  r = await get('/healthz');
  ok(r.status === 200 && r.body === 'ok', 'healthz unchanged');

  const release = { version: '0.10.0', released: '2026-11-10', notes: '/download/0.10.0/NOTES.md', files: [
    { kind: 'installer', name: 'Krunk-Setup-0.10.0.exe', url: '/download/0.10.0/Krunk-Setup-0.10.0.exe', size: 146898835, sha256: 'a'.repeat(64) },
    { kind: 'portable', name: 'Krunk-win-x64.zip', url: '/download/0.10.0/Krunk-win-x64.zip', size: 196187572, sha256: 'b'.repeat(64) }] };
  const noSizes = JSON.parse(JSON.stringify(release)); noSizes.files.forEach((f) => { delete f.size; });
  const evil = JSON.parse(JSON.stringify(release)); evil.files[0].url = 'https://evil.example/x.exe'; evil.files[1].url = '/download/../x';
  const cases = [
    { name: 'release', body: release, krunk: null, expect: { main: 'Download for Windows', sub: 'Installer · 140 MB · 64-bit', using: null } },
    { name: 'no sizes', body: noSizes, krunk: null, expect: { main: 'Download for Windows', sub: 'Installer · 64-bit', using: null } },
    { name: 'none published', status: 404, krunk: null, expect: { none: true } },
    { name: 'unsafe links', body: evil, krunk: null, expect: { none: true } },
    { name: 'krunk outdated', body: release, krunk: '0.9.0', expect: { main: 'Download for Windows', using: 'outdated' } },
    { name: 'krunk current', body: release, krunk: '0.10.0', expect: { main: 'Download for Windows', using: 'up to date' } },
    { name: 'krunk ahead', body: release, krunk: '0.11.0', expect: { using: 'ahead' } },
  ];
  for (const c of cases) {
    const ctx = await context(null, { viewport: { width: 1280, height: 900 } });
    await ctx.route(`${BASE}/download/release.json`, (route) => c.status
      ? route.fulfill({ status: c.status, body: '404 Not Found' })
      : route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(c.body) }));
    if (c.krunk) await ctx.addInitScript((v) => { Object.defineProperty(window, 'krunk', { value: Object.freeze({ version: v }) }); }, c.krunk);
    const page = await watched(ctx, `home ${c.name}`);
    await page.goto(BASE + '/', { waitUntil: 'networkidle' });
    await page.waitForSelector('#download .btn.big, #download .none');
    const got = await page.evaluate(() => {
      const q = (s) => document.querySelector(s);
      const using = q('#using-krunk');
      return { main: q('#download .dl-main') && q('#download .dl-main').textContent, sub: q('#download .btn.big .sub') && q('#download .btn.big .sub').textContent,
        none: !!q('#download .none'), using: using ? using.className + ' ' + using.textContent : null,
        hrefs: [...document.querySelectorAll('#download a')].map((a) => a.getAttribute('href')) };
    });
    const e = c.expect;
    const fine = (e.none ? got.none : !got.none)
      && (!e.main || got.main === e.main) && (!e.sub || got.sub === e.sub)
      && (e.using === undefined || (e.using === null ? got.using === null : (got.using || '').includes(e.using)))
      && got.hrefs.every((h) => h.startsWith('/download/'));
    ok(fine, `download box, ${c.name}: ${JSON.stringify(got).slice(0, 220)}`);
    // With no release the browser itself reports release.json's 404; nothing else may appear.
    const errors = page.krunk.errors.filter((e) => !(c.status === 404 && /status of 404/.test(e)));
    ok(errors.length === 0, `download box, ${c.name}: 0 console errors ${errors.join(' | ')}`);
    await ctx.close();
  }
}

(async () => {
  fs.mkdirSync(OUT, { recursive: true });
  // Staging only: start from empty rate-limit windows (this run signs in and registers several
  // times; the limits themselves are covered by the server's integration tests).
  psql('DELETE FROM rate_limits');
  const seed = JSON.parse(fs.readFileSync(STATE, 'utf8'));
  browser = await chromium.launch();
  try {
    log('== E: public site statuses and download box');
    await publicSite();
    log('== A/B: pages, CSP, layout, axe');
    await allPages(seed);
    log('== C: keyboard');
    await keyboard(seed);
    log('== D: validation, two-step, codes, sync state, motion, long names, this device');
    await features(seed);
  } finally {
    fs.writeFileSync(STATE, JSON.stringify(seed, null, 2), { mode: 0o600 });
    await browser.close();
  }
  fs.writeFileSync(path.join(OUT, 'axe.json'), JSON.stringify({ axe: '4.14.0', runs: axeSummary }, null, 1));
  fs.writeFileSync(path.join(OUT, 'site-results.txt'), lines.join('\n') + `\n\n${problems.length} problem(s)\n`);
  console.log(`\n${problems.length} problem(s)`);
  process.exit(problems.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
