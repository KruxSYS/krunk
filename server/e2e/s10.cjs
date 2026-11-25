// Targeted end-to-end test of 0.13.0 on a staging stack: only the new flows.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node server/e2e/s10.cjs http://dind:10017 stage-browser <krunk-sync-cli> <evidence dir>
// 1 /changelog (every version, anchors, the download box's "Full changelog"); 2 "Add a device"
// with krunk-sync-cli as the Android app (approve with the matching code; deny, expiry and reuse
// refused); 3 a QR sign-in of a second browser ("Signed in by QR"); 4 usage and activity with
// the staging requests' IPs; 5 the data download (validated against docs/sync/export-schema-1.json
// with python jsonschema; nothing mailed); 6 deletion (SQL zeros, the old cookie and device token
// answer 401, the receipt in Mailpit). Same staging set-up as accounts.cjs (krunk.test routing,
// Turnstile test keys, server/e2e/mailsink.sh up).
'use strict';
const { chromium } = require('playwright');
const { execFileSync, spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const UPSTREAM = process.argv[2] || 'http://dind:10017';
const PROJECT = process.argv[3] || 'stage-browser';
const CLI = process.argv[4] || path.join(__dirname, '../../tools/sync-cli/target/debug/krunk-sync-cli');
const SHOTS = process.argv[5] || '/tmp/krunk-s10';
const BASE = 'https://krunk.test';
const EMAIL = `s10-${Date.now()}@example.org`;
const PW = 'violet harbour lanterns 9';
const SCHEMA = path.join(__dirname, '../../docs/sync/export-schema-1.json');
fs.mkdirSync(SHOTS, { recursive: true });

const problems = [];
function ok(cond, what) {
  if (!cond) { problems.push(what); console.log('FAIL', what); } else console.log('ok  ', what);
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function sql(q) {
  return execFileSync('docker', ['exec', `${PROJECT}-db-1`, 'psql', '-U', 'krunk', '-d', 'krunk', '-XAtqc', q]).toString().trim();
}
function mailpit(apiPath) {
  const out = execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${apiPath}`]);
  return JSON.parse(out.toString());
}
function mailsTo(to) {
  return (mailpit('/api/v1/messages?limit=200').messages || []).filter((x) => (x.To || []).some((t) => t.Address === to));
}
async function waitMail(to, subjectPart) {
  for (let i = 0; i < 60; i++) {
    const m = mailsTo(to).find((x) => x.Subject.includes(subjectPart));
    if (m) return mailpit(`/api/v1/message/${m.ID}`);
    await sleep(500);
  }
  return null;
}
// krunk-sync-cli pair in the background: resolves with its key: value lines and exit code;
// `onLine` sees lines as they come (the confirm code is printed before the polling).
function cliPair(arg, dir, onLine) {
  return new Promise((resolve) => {
    const p = spawn(CLI, ['pair', arg, '--server', UPSTREAM, '--dir', dir, '--timeout', '120']);
    const out = {};
    let buf = '';
    p.stdout.on('data', (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i); buf = buf.slice(i + 1);
        const m = /^([a-z_]+): (.*)$/.exec(line);
        if (m) { out[m[1]] = m[2]; if (onLine) onLine(m[1], m[2]); }
      }
    });
    p.stderr.on('data', (d) => process.stderr.write(`  cli: ${d}`));
    p.on('close', (code) => resolve({ code, out }));
  });
}

(async () => {
  const browser = await chromium.launch();
  const errors = [];
  async function context() {
    const ctx = await browser.newContext({ viewport: { width: 1100, height: 1000 }, acceptDownloads: true });
    await ctx.route(`${BASE}/**`, async (route) => {
      const u = new URL(route.request().url());
      const response = await route.fetch({ url: UPSTREAM + u.pathname + u.search, maxRedirects: 0 });
      await route.fulfill({ response });
    });
    const page = await ctx.newPage();
    page.on('console', (m) => { if (m.type() === 'error') errors.push(`${page.url()}: ${m.text()}`); });
    page.on('pageerror', (e) => errors.push(`${page.url()}: ${e.message}`));
    return { ctx, page };
  }
  let n = 0;
  async function shot(page, name) {
    n += 1;
    await page.screenshot({ path: path.join(SHOTS, `${String(n).padStart(2, '0')}-${name}.png`), fullPage: true });
  }
  const { ctx, page } = await context();

  // ---- 1. Changelog ----------------------------------------------------------------------
  await page.goto(`${BASE}/changelog`);
  const ids = await page.$$eval('main section[id]', (s) => s.map((x) => x.id));
  // Every version CHANGELOG.md has (newest first), from 0.2.0 on.
  const want = [...fs.readFileSync(path.join(__dirname, '../../CHANGELOG.md'), 'utf8').matchAll(/^## (\d+\.\d+\.\d+) — /gm)].map((m) => m[1]);
  ok(want.length >= 32 && ids[0] === want[0] && ids[ids.length - 1] === '0.2.0', `changelog: ${ids.length} versions, ${ids[0]} … ${ids[ids.length - 1]}`);
  ok(want.every((v) => ids.includes(v)) && ids.length === want.length, `changelog: every version of CHANGELOG.md (0.2.0 … ${want[0]}) has an anchor`);
  await shot(page, 'changelog');
  await page.goto(`${BASE}/changelog#0.12.4`);
  ok(await page.$eval('section[id="0.12.4"] h2', (h) => h.textContent.includes('0.12.4')), 'changelog: /changelog#0.12.4 lands on its section');
  await page.goto(`${BASE}/download`);
  await page.waitForSelector('#download a');
  const full = await page.$('#download a:has-text("Full changelog")');
  ok(full, 'download box: "Full changelog" link');
  if (full) {
    const href = await full.getAttribute('href');
    await full.click();
    await page.waitForURL(`${BASE}${href}`);
    const v = href.split('#')[1];
    ok(href.startsWith('/changelog#') && (await page.$(`section[id="${v}"]`)), `download box link ${href} opens that version`);
    await shot(page, 'changelog-from-download-box');
  }

  // ---- Account set-up (the pages, as a user would) ----------------------------------------
  async function waitTurnstile(p) {
    await p.waitForFunction(() => {
      const i = document.querySelector('[data-turnstile] input[name="cf-turnstile-response"]');
      return i && i.value.length > 0;
    }, null, { timeout: 30000 });
  }
  await page.goto(`${BASE}/account/register`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile(page);
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
  await page.goto(`${BASE}/account/login`);
  await page.waitForSelector('#form:not([hidden])');
  await waitTurnstile(page);
  await page.fill('input[name=email]', EMAIL);
  await page.fill('input[name=password]', PW);
  await page.click('button[type=submit]');
  await page.waitForURL(`${BASE}/account/`);
  await page.waitForSelector('#overview:not([hidden])');
  const accountId = sql(`SELECT id FROM accounts WHERE email = '${EMAIL}'`);
  ok(/^[0-9a-f-]{36}$/.test(accountId), 'account registered, verified and signed in');

  async function reauthIfAsked() {
    const form = await page.waitForSelector('#reauth:not([hidden]), #pair-panel #pair-qr svg, #pair-result:not([hidden]), #export-result:not([hidden])', { timeout: 15000 });
    if (await page.$('#reauth:not([hidden])')) {
      await page.fill('#reauth input[name=password]', PW);
      await page.click('#reauth button[type=submit]');
    }
    return form;
  }
  async function newCode() {
    const btn = await page.$('#pair-new:not([hidden])');
    if (btn) await btn.click(); else await page.click('#add-device-open');
    await reauthIfAsked();
    await page.waitForSelector('#pair-qr svg');
    await page.waitForFunction(() => /\/pair#[A-Za-z0-9_-]{43}$/.test(document.getElementById('pair-url').textContent));
    return (await page.textContent('#pair-url')).trim();
  }
  async function resultText() {
    await page.waitForSelector('#pair-result:not([hidden])', { timeout: 20000 });
    return (await page.textContent('#pair-result')).trim();
  }

  // ---- 2. Add a device: krunk-sync-cli as the Android app ---------------------------------
  const cliDir = fs.mkdtempSync(path.join(os.tmpdir(), 'krunk-s10-cli-'));
  let url = await newCode();
  ok(url.startsWith(`${BASE}/pair#`), `QR shown with its link (${url.slice(0, 32)}…)`);
  await shot(page, 'add-device-qr');
  let code = null;
  const run = cliPair(url, cliDir, (k, v) => { if (k === 'confirm_code') code = v; });
  await page.waitForSelector('#pair-claimed:not([hidden])', { timeout: 20000 });
  const shown = (await page.textContent('#pair-code')).replace(/\s/g, '');
  const claimant = await page.textContent('#pair-claimant');
  ok(code && shown === code, `the page shows the code the CLI prints (${code})`);
  ok(/android/i.test(claimant) && /\d+\.\d+\.\d+\.\d+|:/.test(claimant), `claimant: Android with its IP (${claimant.replace(/\s+/g, ' ').trim()})`);
  ok(await page.$eval('#pair-approve', (b) => b.disabled), 'Approve is disabled until the code is confirmed');
  await shot(page, 'add-device-claimed');
  await page.check('#pair-same');
  await page.click('#pair-approve');
  const r1 = await run;
  ok(r1.code === 0 && /^[0-9a-f-]{36}$/.test(r1.out.device_id || ''), `the CLI received device tokens (device ${r1.out.device_id})`);
  ok(sql(`SELECT platform FROM devices WHERE id = '${r1.out.device_id}'`) === 'android', 'the device is linked with platform android');
  ok(/added|approved|linked/i.test(await resultText()), 'the page reports the device added');
  await sleep(9000); // the page reloads the devices list once the tokens are collected
  await page.reload();
  await page.waitForSelector('#overview:not([hidden])');
  // The list renders after the page's own requests: wait for it rather than read it at once.
  const listed = await page.waitForFunction(() => /android/i.test(document.getElementById('devices-active').textContent), null, { timeout: 15000 }).then(() => true, () => false);
  ok(listed, 'Devices lists the Android device');
  await shot(page, 'add-device-done');

  // Reuse of the same code is refused.
  const reuse = await cliPair(url, fs.mkdtempSync(path.join(os.tmpdir(), 'krunk-s10-cli-')));
  ok(reuse.code === 3 && reuse.out.error === 'pair_used', `reusing a code is refused (${reuse.out.error})`);
  // Denied.
  url = await newCode();
  let denyCode = null;
  const deny = cliPair(url, fs.mkdtempSync(path.join(os.tmpdir(), 'krunk-s10-cli-')), (k, v) => { if (k === 'confirm_code') denyCode = v; });
  await page.waitForSelector('#pair-claimed:not([hidden])', { timeout: 20000 });
  await page.click('#pair-deny');
  const rd = await deny;
  ok(denyCode && rd.code === 3 && rd.out.error === 'access_denied', `deny: the CLI gets access_denied (${rd.out.error})`);
  const denied = await resultText();
  ok(/denied/i.test(denied), `deny: the page says so ("${denied}")`);
  await shot(page, 'add-device-denied');
  // Expired.
  url = await newCode();
  sql(`UPDATE pair_grants SET expires_at = now() - interval '1 second' WHERE account_id = '${accountId}'`);
  const ex = await cliPair(url, fs.mkdtempSync(path.join(os.tmpdir(), 'krunk-s10-cli-')));
  ok(ex.code === 3 && ex.out.error === 'expired_token', `expiry: the claim is refused (${ex.out.error})`);
  const expired = await resultText();
  ok(/expired/i.test(expired), `expiry: the page says so ("${expired}")`);
  await shot(page, 'add-device-expired');

  // ---- 3. QR sign-in of a second browser ---------------------------------------------------
  url = await newCode();
  const phone = await context();
  await phone.page.goto(url);
  await phone.page.waitForFunction(() => /^\d{3}\s?\d{3}$/.test(document.getElementById('claim-code').textContent.trim()));
  ok(!phone.page.url().includes('#'), 'phone: the token left the address bar');
  const phoneCode = (await phone.page.textContent('#claim-code')).replace(/\s/g, '');
  await phone.page.screenshot({ path: path.join(SHOTS, `${String(++n).padStart(2, '0')}-phone-code.png`), fullPage: true });
  await page.waitForSelector('#pair-claimed:not([hidden])', { timeout: 20000 });
  ok((await page.textContent('#pair-code')).replace(/\s/g, '') === phoneCode, `both screens show ${phoneCode}`);
  await shot(page, 'qr-sign-in-claimed');
  await page.check('#pair-same');
  await page.click('#pair-approve');
  await phone.page.waitForURL(`${BASE}/account/`, { timeout: 30000 });
  await phone.page.waitForSelector('#overview:not([hidden])');
  ok((await phone.page.textContent('#who')).includes(EMAIL), 'phone: signed in to the account');
  await phone.page.screenshot({ path: path.join(SHOTS, `${String(++n).padStart(2, '0')}-phone-signed-in.png`), fullPage: true });
  await page.reload();
  await page.waitForSelector('#overview:not([hidden])');
  await page.waitForFunction(() => document.getElementById('sessions').textContent.includes('by QR'));
  ok(true, 'Signed-in browsers labels the phone "Signed in by QR"');
  ok(sql(`SELECT count(*) FROM web_sessions WHERE account_id = '${accountId}' AND via = 'qr'`) === '1', 'the session is stored with via = qr');
  ok(await waitMail(EMAIL, 'by QR'), 'the "new sign-in by QR" mail arrived');
  await shot(page, 'qr-sign-in-sessions');

  // ---- 4. Usage and activity ---------------------------------------------------------------
  const ip = sql(`SELECT host(ip) FROM ip_events WHERE account_id = '${accountId}' ORDER BY id DESC LIMIT 1`);
  await page.waitForSelector('#usage-devices');
  await page.waitForFunction(() => document.querySelectorAll('#activity-rows tr').length > 3);
  const usage = await page.textContent('#usage');
  ok(/1\s*(of|\/)\s*10/.test(usage), 'Usage: devices 1 of 10');
  const activity = await page.textContent('#activity');
  ok(activity.includes(ip), `Activity shows the requests' IP ${ip}`);
  ok(/this is you/i.test(activity), 'Activity marks this session "this is you"');
  ok(/90 days/.test(activity), 'Activity states the 90-day retention');
  const country = sql(`SELECT coalesce(country, '-') FROM ip_events WHERE account_id = '${accountId}' ORDER BY id DESC LIMIT 1`);
  console.log(`     (staging has no Cloudflare in front: the stored country is "${country}")`);
  await page.$eval('#usage', (e) => e.scrollIntoView());
  await shot(page, 'usage-activity');

  // ---- 5. Data download ---------------------------------------------------------------------
  const mailsBefore = mailsTo(EMAIL).length;
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 30000 }),
    (async () => { await page.click('#export-download'); await reauthIfAsked(); })(),
  ]);
  const file = path.join(SHOTS, download.suggestedFilename());
  await download.saveAs(file);
  ok(/^krunk-account-\d{4}-\d{2}-\d{2}\.json$/.test(download.suggestedFilename()), `the download is ${download.suggestedFilename()}`);
  let valid = '';
  try {
    valid = execFileSync('python3', ['-I', '-c',
      'import json,sys,jsonschema; s=json.load(open(sys.argv[1])); d=json.load(open(sys.argv[2])); jsonschema.Draft202012Validator(s, format_checker=jsonschema.FormatChecker()).validate(d); print("valid", len(d["devices"]), len(d["web_sessions"]), len(d["activity"]))',
      SCHEMA, file]).toString().trim();
  } catch (e) { valid = String(e.stdout || e.message).slice(0, 400); }
  ok(valid.startsWith('valid'), `the export validates against export-schema-1.json (${valid})`);
  await sleep(6000); // the mail worker runs every 5 s
  ok(mailsTo(EMAIL).length === mailsBefore, 'nothing was mailed for the download');
  await shot(page, 'export');
  const exported = JSON.parse(fs.readFileSync(file, 'utf8'));
  fs.writeFileSync(file, JSON.stringify({ ...exported, account: { ...exported.account, email: '<redacted>' } }, null, 1));

  // ---- 6. Deletion ----------------------------------------------------------------------------
  const cookie = (await ctx.cookies()).find((c) => c.name === '__Host-krunk_session').value;
  const cli = JSON.parse(fs.readFileSync(path.join(cliDir, 'cli.json'), 'utf8'));
  const curl = (args) => execFileSync('curl', ['-s', '-o', '/dev/null', '-w', '%{http_code}', ...args]).toString();
  ok(curl(['-H', `Authorization: Bearer ${cli.access_token}`, `${UPSTREAM}/api/v1/device/me`]) === '200', 'before: the device token works');
  await page.click('#delete-link');
  await page.waitForURL(`${BASE}/account/delete`);
  await page.waitForSelector('#form:not([hidden])');
  ok(await page.$('#export-first[href="/account/#export"]'), 'the delete page offers "Download your data first"');
  await waitTurnstile(page);
  await page.fill('#form input[name=confirm_email]', EMAIL.toUpperCase());
  await page.fill('#form input[name=password]', PW);
  await shot(page, 'delete-form');
  await page.click('#delete-submit');
  await page.waitForSelector('#done:not([hidden])', { timeout: 20000 });
  const remaining = await page.textContent('#remaining');
  ok(!/[1-9]/.test(remaining), `the page shows what is left: ${remaining.replace(/\s+/g, ' ').trim()}`);
  await shot(page, 'delete-done');
  const tables = sql("SELECT table_name FROM information_schema.columns WHERE table_schema = 'public' AND column_name = 'account_id' AND table_name <> 'account_deletions'").split('\n');
  const left = tables.map((t) => [t, Number(sql(`SELECT count(*) FROM ${t} WHERE account_id = '${accountId}'`))]).filter(([, c]) => c > 0);
  ok(tables.length >= 15 && left.length === 0, `SQL: 0 rows in all ${tables.length} account tables${left.length ? ` (left: ${JSON.stringify(left)})` : ''}`);
  ok(sql(`SELECT count(*) FROM account_deletions WHERE account_id = '${accountId}'`) === '1', 'the deleted id is in account_deletions');
  ok(curl(['-H', `Cookie: __Host-krunk_session=${cookie}`, `${UPSTREAM}/api/v1/web/me`]) === '401', 'the old cookie answers 401');
  ok(curl(['-H', `Authorization: Bearer ${cli.access_token}`, `${UPSTREAM}/api/v1/device/me`]) === '401', 'the device access token answers 401');
  const refresh = execFileSync('curl', ['-s', '-o', '/dev/null', '-w', '%{http_code}', '-H', 'Content-Type: application/json', '-d', JSON.stringify({ refresh_token: cli.refresh_token }), `${UPSTREAM}/api/v1/token/refresh`]).toString();
  ok(refresh === '401', 'the device refresh token answers 401');
  const receipt = await waitMail(EMAIL, 'deleted');
  ok(receipt && /14 days/.test(receipt.Text), 'the deletion receipt is in the mail sink');
  // The backup service lists the id within the hour; check the table it reads from.
  fs.writeFileSync(path.join(SHOTS, 'deleted-account-id.txt'), `${accountId}\n`);

  const csp = errors.filter((x) => !/the server responded with a status of 4\d\d/.test(x));
  ok(csp.length === 0, `no console or CSP errors on the new pages${csp.length ? `: ${csp.join(' | ')}` : ''}`);
  await browser.close();
  console.log(problems.length ? `\n${problems.length} problem(s)` : '\nall ok');
  process.exit(problems.length ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
