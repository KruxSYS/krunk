// Screenshots of every account page in both colour schemes at 360, 900 and 1280 px (0.9.0: the
// before/after evidence of the restyle). Signed-in pages use a seeded account (seed.cjs).
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/shots.cjs http://dind:10017 <state.json> <out dir>
// Each page is loaded once; the width and the scheme change in place (no reload), so the /link
// code is looked up only once. Console errors and CSP violations are listed in <out>/problems.txt.
'use strict';
const { chromium } = require('playwright');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const [UPSTREAM, STATE, OUT] = process.argv.slice(2);
const BASE = 'https://krunk.test';
const WIDTHS = [360, 900, 1280];
const THEMES = ['light', 'dark'];
const TS = 'XXXX.DUMMY.TOKEN.XXXX';

function b32decode(s) {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = 0, buf = 0; const out = [];
  for (const c of s.replace(/[\s=]/g, '')) { buf = (buf << 5) | A.indexOf(c); bits += 5; if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); } }
  return Buffer.from(out);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function freshTotp(st) {
  let step = Math.floor(Date.now() / 1000 / 30);
  while (step <= (st.last_step || 0)) { await sleep(1000); step = Math.floor(Date.now() / 1000 / 30); }
  st.last_step = step;
  const msg = Buffer.alloc(8); msg.writeBigUInt64BE(BigInt(step));
  const h = crypto.createHmac('sha1', b32decode(st.totp_secret)).update(msg).digest();
  const o = h[h.length - 1] & 15;
  return String(((h.readUInt32BE(o) & 0x7fffffff) % 1000000)).padStart(6, '0');
}
async function apiLogin(st) {
  const r = await fetch(`${UPSTREAM}/api/v1/web/login`, {
    method: 'POST',
    headers: { Origin: BASE, 'Content-Type': 'application/json' },
    body: JSON.stringify({ email: st.email, password: st.password, turnstile: TS, totp: await freshTotp(st) }),
  });
  if (r.status !== 200) throw new Error(`login ${r.status}`);
  const [name, value] = r.headers.get('set-cookie').split(';')[0].split('=');
  return { name, value };
}
async function newCode() {
  const r = await fetch(`${UPSTREAM}/api/v1/link/authorize`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'User-Agent': 'Krunk/0.9.0 (shots.cjs)' },
    body: JSON.stringify({ platform: 'windows', label: 'Krunk 0.9.0 on Windows 11', client_version: '0.9.0' }),
  });
  return (await r.json()).user_code;
}

(async () => {
  const st = JSON.parse(fs.readFileSync(STATE, 'utf8'));
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch();
  const problems = [];
  async function context(signedIn) {
    const ctx = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    await ctx.route(`${BASE}/**`, async (route) => {
      const u = new URL(route.request().url());
      const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
      await route.fulfill({ response });
    });
    if (signedIn) {
      const c = await apiLogin(st);
      await ctx.addCookies([{ name: c.name, value: c.value, url: `${BASE}/`, secure: true, httpOnly: true, sameSite: 'Strict' }]);
    }
    return ctx;
  }
  async function shoot(ctx, name, url, ready) {
    const page = await ctx.newPage();
    page.on('console', (m) => { if (m.type() === 'error') problems.push(`${name}: console: ${m.text()}`); });
    page.on('pageerror', (e) => problems.push(`${name}: pageerror: ${e.message}`));
    await page.addInitScript(() => {
      document.addEventListener('securitypolicyviolation', (e) => console.error(`CSP violation: ${e.violatedDirective} ${e.blockedURI}`));
    });
    await page.goto(BASE + url, { waitUntil: 'networkidle' });
    if (ready) await ready(page);
    await sleep(400);
    for (const theme of THEMES) {
      await page.emulateMedia({ colorScheme: theme });
      for (const w of WIDTHS) {
        await page.setViewportSize({ width: w, height: 900 });
        await sleep(250);
        await page.screenshot({ path: path.join(OUT, `${name}-${theme}-${w}.png`), fullPage: true });
      }
    }
    await page.close();
  }
  const out = await context(false);
  await shoot(out, 'login', '/account/login');
  await shoot(out, 'register', '/account/register');
  await shoot(out, 'resend', '/account/resend');
  await shoot(out, 'reset-request', '/account/reset');
  await shoot(out, 'reset-new-password', '/account/reset?token=' + 'A'.repeat(43));
  await shoot(out, 'verify', '/account/verify?token=' + 'A'.repeat(43));
  await out.close();
  const inn = await context(true);
  await shoot(inn, 'account', '/account/', (p) => p.waitForSelector('#devices li', { timeout: 10000 }).catch(() => {}));
  const code = await newCode();
  await shoot(inn, 'link', `/link?code=${code}`, (p) => p.waitForTimeout(1500));
  await inn.close();
  await browser.close();
  fs.writeFileSync(STATE, JSON.stringify(st, null, 2), { mode: 0o600 });
  fs.writeFileSync(path.join(OUT, 'problems.txt'), problems.length ? problems.join('\n') + '\n' : 'none\n');
  console.log(`${fs.readdirSync(OUT).filter((f) => f.endsWith('.png')).length} screenshots in ${OUT}; problems: ${problems.length}`);
  for (const p of problems) console.log('  ' + p);
})().catch((e) => { console.error(e); process.exit(1); });
