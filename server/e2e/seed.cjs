// Seeds S3-shaped data into a staging stack and checks later that it survived a re-stage or a
// migrating deploy (0.9.0: the staging survival test before each server deploy).
//   node server/e2e/seed.cjs seed   http://dind:10017 stage-browser <state.json>
//   node server/e2e/seed.cjs verify http://dind:10017 stage-browser <state.json>
// seed: an account with two-step verification on (10 backup codes), two web sessions, two linked
// devices (one of them removed). The state file keeps the test password and TOTP secret (staging
// test values only) and what verify expects. Staging only: Turnstile test keys and Mailpit.
'use strict';
const { execFileSync } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');

const [MODE, UPSTREAM, PROJECT, STATE] = process.argv.slice(2);
const ORIGIN = 'https://krunk.test';
const TS = 'XXXX.DUMMY.TOKEN.XXXX';
if (!MODE || !UPSTREAM || !PROJECT || !STATE) {
  console.error('usage: seed.cjs seed|verify <upstream> <project> <state.json>');
  process.exit(2);
}

function session() {
  const s = { cookie: '', csrf: '' };
  s.call = async (method, path, body) => {
    const headers = { Origin: ORIGIN, 'User-Agent': 'Mozilla/5.0 (X11; Linux x86_64) seed.cjs' };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (s.cookie) headers.Cookie = s.cookie;
    if (s.csrf && method !== 'GET') headers['X-Krunk-CSRF'] = s.csrf;
    const r = await fetch(`${UPSTREAM}/api/v1/web/${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    const set = r.headers.get('set-cookie');
    if (set) s.cookie = set.split(';')[0];
    let j = {};
    try { j = await r.json(); } catch (e) { j = {}; }
    if (j && j.csrf) s.csrf = j.csrf;
    return { status: r.status, body: j };
  };
  return s;
}
async function dev(method, path, body, bearer) {
  const headers = { 'User-Agent': 'Krunk/0.8.0 (seed.cjs)' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (bearer) headers.Authorization = `Bearer ${bearer}`;
  const r = await fetch(`${UPSTREAM}/api/v1/${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  let j = {};
  try { j = await r.json(); } catch (e) { j = {}; }
  return { status: r.status, body: j };
}
function mailpit(path) {
  return JSON.parse(execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${path}`]).toString());
}
function b32decode(s) {
  const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = 0, buf = 0; const out = [];
  for (const c of s.replace(/[\s=]/g, '')) { buf = (buf << 5) | A.indexOf(c); bits += 5; if (bits >= 8) { bits -= 8; out.push((buf >> bits) & 255); } }
  return Buffer.from(out);
}
function totp(secretB32, offsetSteps) {
  const step = Math.floor(Date.now() / 1000 / 30) + (offsetSteps || 0);
  const msg = Buffer.alloc(8); msg.writeBigUInt64BE(BigInt(step));
  const h = crypto.createHmac('sha1', b32decode(secretB32)).update(msg).digest();
  const o = h[h.length - 1] & 15;
  return String(((h.readUInt32BE(o) & 0x7fffffff) % 1000000)).padStart(6, '0');
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function must(r, status, what) {
  if (r.status !== status) throw new Error(`${what}: ${r.status} ${JSON.stringify(r.body)}`);
  return r.body;
}
// A TOTP step can be used once: wait for a fresh step before each use.
let lastStep = 0;
async function freshTotp(secret) {
  let step = Math.floor(Date.now() / 1000 / 30);
  while (step <= lastStep) { await sleep(1000); step = Math.floor(Date.now() / 1000 / 30); }
  lastStep = step;
  return totp(secret);
}
async function login(st) {
  const s = session();
  lastStep = Math.max(lastStep, st.last_step || 0);
  must(await s.call('POST', 'login', { email: st.email, password: st.password, turnstile: TS, totp: await freshTotp(st.totp_secret) }), 200, 'login');
  st.last_step = lastStep;
  return s;
}
async function link(s, st, label) {
  const a = must(await dev('POST', 'link/authorize', { platform: 'windows', label, client_version: '0.8.0' }), 200, 'authorize');
  must(await s.call('GET', `link/${encodeURIComponent(a.user_code)}`), 200, 'link view');
  must(await s.call('POST', `link/${encodeURIComponent(a.user_code)}/approve`, { confirm_code: a.user_code }), 200, 'approve');
  await sleep(a.interval * 1000 + 200);
  return must(await dev('POST', 'link/token', { device_code: a.device_code }), 200, 'token');
}

async function seed() {
  const st = { email: `seed-${Date.now()}@example.org`, password: 'seeded staging pass 42' };
  const s = session();
  await s.call('POST', 'register', { email: st.email, password: st.password, turnstile: TS });
  let token = null;
  for (let i = 0; i < 40 && !token; i++) {
    const m = (mailpit('/api/v1/messages?limit=50').messages || []).find((x) => (x.To || []).some((t) => t.Address === st.email));
    if (m) token = /token=([A-Za-z0-9_-]{43})/.exec(mailpit(`/api/v1/message/${m.ID}`).Text)[1];
    else await sleep(500);
  }
  if (!token) throw new Error('no verification mail');
  must(await s.call('POST', 'verify-email', { token, password: st.password }), 200, 'verify');
  must(await s.call('POST', 'login', { email: st.email, password: st.password, turnstile: TS }), 200, 'login');
  const setup = must(await s.call('POST', 'totp/setup', { password: st.password }), 200, 'totp setup');
  st.totp_secret = setup.secret_b32;
  const en = must(await s.call('POST', 'totp/enable', { code: await freshTotp(st.totp_secret) }), 200, 'totp enable');
  st.backup_codes = en.backup_codes;
  // Re-auth for the approvals (needs a fresh TOTP step too).
  must(await s.call('POST', 'reauth', { password: st.password, totp: await freshTotp(st.totp_secret) }), 204, 'reauth');
  const d1 = await link(s, st, 'Seed PC (kept)');
  const d2 = await link(s, st, 'Seed laptop (removed)');
  must(await s.call('POST', `devices/${d2.device_id}/revoke`, {}), 200, 'revoke');
  must(await s.call('POST', `devices/${d1.device_id}/rename`, { label: 'Seed PC renamed' }), 200, 'rename');
  const s2 = await login(st);
  must(await s2.call('GET', 'me'), 200, 'me (second session)');
  st.devices = { kept: d1.device_id, removed: d2.device_id };
  st.refresh_kept = d1.refresh_token;
  st.expect = await snapshot(s);
  st.last_step = lastStep;
  fs.writeFileSync(STATE, JSON.stringify(st, null, 2), { mode: 0o600 });
  console.log(`seeded ${st.email}: ${JSON.stringify(st.expect)}`);
}

// What must survive: the account, two-step verification and its codes, the devices with their
// labels and states, and the session rows.
async function snapshot(s) {
  const me = must(await s.call('GET', 'me'), 200, 'me');
  const devices = must(await s.call('GET', 'devices'), 200, 'devices');
  const sessions = must(await s.call('GET', 'sessions'), 200, 'sessions');
  const list = (Array.isArray(devices) ? devices : devices.devices || []).map((d) => ({ id: d.id, label: d.label, revoked: !!d.revoked_at }));
  list.sort((a, b) => a.id.localeCompare(b.id));
  const sl = Array.isArray(sessions) ? sessions : sessions.sessions || [];
  return {
    email_verified: me.email_verified,
    totp: me.totp,
    backup_codes_left: me.backup_codes_left,
    devices_active: me.devices,
    devices: list,
    web_sessions: sl.filter((x) => x.kind === 'web').length,
    device_sessions: sl.filter((x) => x.kind === 'device').length,
  };
}

async function verify() {
  const st = JSON.parse(fs.readFileSync(STATE, 'utf8'));
  const s = await login(st);
  const now = await snapshot(s);
  const problems = [];
  for (const k of ['email_verified', 'totp', 'backup_codes_left', 'devices_active', 'device_sessions']) {
    if (JSON.stringify(now[k]) !== JSON.stringify(st.expect[k])) problems.push(`${k}: ${JSON.stringify(st.expect[k])} -> ${JSON.stringify(now[k])}`);
  }
  if (JSON.stringify(now.devices) !== JSON.stringify(st.expect.devices)) problems.push(`devices changed: ${JSON.stringify(now.devices)}`);
  // This verify logs in once more: one more web session than seeded (or more after earlier verifies).
  if (now.web_sessions < st.expect.web_sessions + 1) problems.push(`web sessions: ${st.expect.web_sessions} seeded, ${now.web_sessions} now`);
  // The kept device still refreshes (its token family survived); the token rotates.
  const r = await dev('POST', 'token/refresh', { refresh_token: st.refresh_kept });
  if (r.status !== 200) problems.push(`kept device refresh: ${r.status}`);
  else {
    st.refresh_kept = r.body.refresh_token;
    const me = await dev('GET', 'device/me', undefined, r.body.access_token);
    if (me.status !== 200 || me.body.label !== 'Seed PC renamed') problems.push(`device/me: ${me.status} ${me.body.label}`);
    fs.writeFileSync(STATE, JSON.stringify(st, null, 2), { mode: 0o600 });
  }
  fs.writeFileSync(STATE, JSON.stringify(st, null, 2), { mode: 0o600 });
  console.log(`verify ${st.email}: ${JSON.stringify(now)}`);
  if (problems.length) { console.log('FAIL\n  ' + problems.join('\n  ')); process.exit(1); }
  console.log('ok: the seeded S3 data survived (account, two-step, codes, devices, labels, sessions, refresh)');
}

(MODE === 'seed' ? seed() : verify()).catch((e) => { console.error(e.message); process.exit(1); });
