// End-to-end sync tests on a staging stack (0.9.0), with the Linux test client
// (tools/sync-cli, Krunk's own core::sync) as the devices and the website API as the user.
//   node server/e2e/sync.cjs http://dind:10017 stage-browser <krunk-sync-cli> <work dir> [part ...]
// Parts: keys (S6). Later milestones add records, passwords and rotation.
// Staging only: Turnstile test keys, Mailpit, and direct database edits that play a malicious
// server (tampered directories, a lowered KDF).
'use strict';
const { spawn, execFileSync } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const [UPSTREAM, PROJECT, CLI, WORK, ...PARTS] = process.argv.slice(2);
const ORIGIN = 'https://krunk.test';
const TS = 'XXXX.DUMMY.TOKEN.XXXX';
const PW = 'sync e2e password 7';
const parts = PARTS.length ? PARTS : ['keys'];
const problems = [];
const log = [];
function say(s) { log.push(s); console.log(s); }
function ok(cond, what) {
  if (cond) say('ok   ' + what); else { problems.push(what); say('FAIL ' + what); }
  return cond;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ---- The website side (the user) ----------------------------------------------------------
function psql(sql) {
  return execFileSync('docker', ['exec', '-i', `${PROJECT}-db-1`, 'psql', '-U', 'krunk', '-d', 'krunk', '-tA', '-v', 'ON_ERROR_STOP=1'], { input: sql }).toString().trim();
}
function mailpit(p) {
  return JSON.parse(execFileSync('docker', ['exec', `${PROJECT}-web-1`, 'wget', '-qO-', `http://mailpit:8025${p}`]).toString());
}
function webSession() {
  const s = { cookie: '', csrf: '' };
  s.call = async (method, p, body) => {
    const headers = { Origin: ORIGIN };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (s.cookie) headers.Cookie = s.cookie;
    if (s.csrf && method !== 'GET') headers['X-Krunk-CSRF'] = s.csrf;
    const r = await fetch(`${UPSTREAM}/api/v1/web/${p}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    const set = r.headers.get('set-cookie');
    if (set) s.cookie = set.split(';')[0];
    let j = {};
    try { j = await r.json(); } catch (e) { j = {}; }
    if (j && j.csrf) s.csrf = j.csrf;
    return { status: r.status, body: j };
  };
  return s;
}
async function account(email) {
  const s = webSession();
  await s.call('POST', 'register', { email, password: PW, turnstile: TS });
  let token = null;
  for (let i = 0; i < 40 && !token; i++) {
    const m = (mailpit('/api/v1/messages?limit=100').messages || []).find((x) => (x.To || []).some((t) => t.Address === email) && x.Subject.includes('Confirm'));
    if (m) token = /token=([A-Za-z0-9_-]{43})/.exec(mailpit(`/api/v1/message/${m.ID}`).Text)[1];
    else await sleep(500);
  }
  if ((await s.call('POST', 'verify-email', { token, password: PW })).status !== 200) throw new Error('verify');
  if ((await s.call('POST', 'login', { email, password: PW, turnstile: TS })).status !== 200) throw new Error('login');
  if ((await s.call('POST', 'reauth', { password: PW })).status !== 204) throw new Error('reauth');
  s.email = email;
  return s;
}
function mails(email, subject) {
  return (mailpit('/api/v1/messages?limit=200').messages || []).filter((x) => (x.To || []).some((t) => t.Address === email) && x.Subject.includes(subject)).length;
}
// The outbox worker sends within seconds: wait for `n` such mails.
async function mailsReach(email, subject, n) {
  for (let i = 0; i < 40; i++) {
    if (mails(email, subject) >= n) return mails(email, subject);
    await sleep(500);
  }
  return mails(email, subject);
}

// ---- The devices (the test client) --------------------------------------------------------
function run(args, opts = {}) {
  return new Promise((resolve) => {
    const p = spawn(CLI, args, { env: { ...process.env, RUST_BACKTRACE: '0' } });
    const lines = [];
    let buf = '';
    p.stdout.on('data', (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        lines.push(line);
        if (opts.onLine) opts.onLine(line);
      }
    });
    let err = '';
    p.stderr.on('data', (d) => { err += d.toString(); });
    p.on('close', (code) => {
      const kv = {};
      for (const l of lines) { const m = /^([a-z_]+): (.*)$/.exec(l); if (m) (kv[m[1]] = kv[m[1]] || []).push(m[2]); }
      resolve({ code, lines, kv, err: err.trim(), one: (k) => (kv[k] || [])[0] });
    });
  });
}
const dirOf = (name) => path.join(WORK, name);
async function link(web, name) {
  fs.rmSync(dirOf(name), { recursive: true, force: true });
  // Staging only: this run links more devices than the code-lookup limit allows per session in
  // 10 minutes (the limits themselves are tested by the server's integration tests).
  psql("DELETE FROM rate_limits WHERE bucket LIKE 'link-%'");
  const r = await run(['link', '--dir', dirOf(name), '--server', UPSTREAM, '--label', `Test ${name}`, '--timeout', '120'], {
    onLine: async (line) => {
      const m = /^user_code: (.+)$/.exec(line);
      if (!m) return;
      const code = m[1];
      const v = await web.call('GET', `link/${encodeURIComponent(code)}`);
      if (v.status !== 200) say(`link view ${v.status}`);
      const a = await web.call('POST', `link/${encodeURIComponent(code)}/approve`, { confirm_code: code });
      if (a.status !== 200) say(`link approve ${a.status} ${JSON.stringify(a.body)}`);
    },
  });
  if (r.code !== 0) throw new Error(`link ${name}: ${r.err}`);
  return r.one('device_id');
}
const cli = (name, ...args) => run([args[0], '--dir', dirOf(name), ...args.slice(1)]);

// Approve N from A with the SAS passed from A's file to N's (the user typing it), or a wrong one.
async function approval(a, n, opts = {}) {
  const sasFile = path.join(WORK, `sas-${a}-${n}.txt`);
  const codeFile = path.join(WORK, `code-${a}-${n}.txt`);
  for (const f of [sasFile, codeFile]) fs.rmSync(f, { force: true });
  const joinArgs = ['join', '--dir', dirOf(n), '--code-file', codeFile, '--timeout', '120', '--name', `Device ${n}`];
  if (opts.nVault) joinArgs.push('--vault');
  const approveArgs = ['approve', '--dir', dirOf(a), '--sas-file', sasFile, '--timeout', '120'];
  if (opts.share) approveArgs.push('--share-passwords');
  const pj = run(joinArgs);
  await sleep(1500);
  const pa = run(approveArgs);
  // The user reads the code on A and types it on N.
  for (let i = 0; i < 240 && !fs.existsSync(sasFile); i++) await sleep(500);
  const sas = fs.existsSync(sasFile) ? fs.readFileSync(sasFile, 'utf8').trim() : '';
  const typed = opts.wrong ? String((Number(sas) + 1) % 1000000).padStart(6, '0') : sas;
  fs.writeFileSync(codeFile, typed);
  const [j, ap] = await Promise.all([pj, pa]);
  return { j, ap, sas, typed };
}

// ---- Parts --------------------------------------------------------------------------------
async function partKeys(ctx) {
  say('== keys (S6)');
  const web = await account(`sync-${Date.now()}@example.org`);
  ctx.web = web;
  const st0 = await (async () => { await link(web, 'A'); return cli('A', 'state'); })();
  ok(st0.code === 0 && JSON.parse(st0.lines[0]).keyset_gen === null, 'A linked, no keyset yet');
  // First device: setup with a vault.
  const setup = await cli('A', 'setup', '--vault');
  ok(setup.code === 0 && /^[a-z]+(-[a-z]+){6}$/.test(setup.one('passphrase') || '') && /^KRNK-/.test(setup.one('recovery_key') || ''),
    `A sets up sync: 7-word passphrase, recovery key, epoch ${setup.one('epoch')}, passwords ${setup.one('passwords')} ${setup.err}`);
  ctx.passphrase = setup.one('passphrase');
  ctx.recovery = setup.one('recovery_key');
  const me = (await web.call('GET', 'me')).body;
  ok(me.sync && me.sync.has_keyset === true, 'the website sees sync on (has_keyset)');
  const again = await cli('A', 'setup', '--vault');
  ok(again.code === 3 && /conflict/i.test(again.err), `a second setup is refused (409): ${again.err}`);

  // Approval: B joins with the code shown on A; A shares passwords.
  await link(web, 'B');
  const before = mails(web.email, 'asks to join');
  const ap = await approval('A', 'B', { share: true, nVault: true });
  ok(ap.ap.code === 0 && ap.j.code === 0 && /^\d{6}$/.test(ap.sas), `B joins by SAS ${ap.sas}: approve exit ${ap.ap.code}, join exit ${ap.j.code} ${ap.ap.err} ${ap.j.err}`);
  ok(ap.j.one('passwords') === 'on' && ap.ap.one('shared_passwords') === 'true', 'B also got the passwords key (shared on A)');
  ok((await mailsReach(web.email, 'asks to join', before + 1)) === before + 1, 'the approval request was mailed');
  const refreshA = await cli('A', 'refresh', '--vault');
  ok(refreshA.code === 0, `A follows the directories after B's vault key was added: ${refreshA.one('refresh')} ${refreshA.err}`);
  const devs = await cli('A', 'devices');
  ok(devs.lines.filter((l) => /keys=true/.test(l)).length === 2, 'A and B hold keys (has_keys)');

  // A wrong code: aborted, the approval is gone, N must ask again.
  await link(web, 'E');
  const wrong = await approval('A', 'E', { wrong: true });
  ok(wrong.j.code === 3 && wrong.j.one('code') === 'mismatch', `E typed a wrong code (${wrong.typed} for ${wrong.sas}): refused`);
  ok(wrong.ap.code === 3, `A's side ends too (the approval is gone): ${wrong.ap.err}`);
  ok(psql("SELECT count(*) FROM approvals") === '0', 'no approval left (single attempt)');
  const eState = await cli('E', 'refresh');
  ok(eState.code === 3 && /no sync\.json/.test(eState.err), 'E got no keys');

  // Restore with the recovery key (all devices lost) and with the passphrase.
  await link(web, 'C');
  const rc = await cli('C', 'restore', '--recovery', ctx.recovery, '--vault');
  ok(rc.code === 0 && (rc.kv.keeps_access || []).length === 2 && rc.one('passwords') === 'on',
    `C restores with the recovery key; the directory lists ${(rc.kv.keeps_access || []).length} devices; passwords ${rc.one('passwords')} ${rc.err}`);
  await link(web, 'D');
  const rd = await cli('D', 'restore', '--passphrase', ctx.passphrase.replace(/-/g, ' ').toUpperCase());
  ok(rd.code === 0, `D restores with the passphrase (typed with spaces, upper case) ${rd.err}`);
  ok((await mailsReach(web.email, 'recovery data was downloaded', 2)) === 2, 'both escrow fetches were mailed');
  for (const n of ['A', 'B']) {
    const r = await cli(n, 'refresh', '--vault');
    ok(r.code === 0, `${n} accepts C's and D's additions: ${r.one('refresh')} seq ${r.one('ak_seq')} ${r.err}`);
  }
  // A wrong passphrase.
  await link(web, 'F');
  const rf = await cli('F', 'restore', '--passphrase', 'wrong words entirely here now please ok');
  ok(rf.code === 3 && /wrong/.test(rf.err), `a wrong passphrase: ${rf.err}`);

  // The server lowers the passphrase KDF in the escrow: refused before any Argon2 run.
  const acc = psql(`SELECT id FROM accounts WHERE email = '${web.email}'`);
  const kdf = psql(`SELECT pp_kdf::text FROM escrow WHERE account_id = '${acc}'`);
  psql(`UPDATE escrow SET pp_kdf = jsonb_set(pp_kdf, '{m_kib}', '1024') WHERE account_id = '${acc}'`);
  const rw = await cli('F', 'restore', '--passphrase', ctx.passphrase);
  ok(rw.code === 3 && /minimum|weak/i.test(rw.err), `a lowered KDF is refused: ${rw.err}`);
  psql(`UPDATE escrow SET pp_kdf = '${kdf}'::jsonb WHERE account_id = '${acc}'`);

  // The server injects a device into the next ak_dir (a forged MAC): every device raises the alarm.
  const head = psql(`SELECT seq || '|' || encode(bytes, 'base64') || '|' || encode(hash, 'hex') FROM ak_dirs WHERE account_id = '${acc}' ORDER BY seq DESC LIMIT 1`);
  const [seq, b64, hashHex] = head.split('|');
  const dir = JSON.parse(Buffer.from(b64.replace(/\s/g, ''), 'base64').toString());
  dir.seq = Number(seq) + 1;
  dir.prev_hash = Buffer.from(hashHex, 'hex').toString('base64url');
  dir.devices.push({ id: dir.devices[0].id, short: 99, pk: crypto.randomBytes(32).toString('base64url'), added_by: dir.devices[0].id, added_seq: dir.seq });
  const forged = Buffer.from(JSON.stringify(dir));
  const fhash = crypto.createHash('sha256').update(forged).digest('hex');
  psql(`INSERT INTO ak_dirs (account_id, seq, epoch, bytes, hash, mac, created_by) VALUES ('${acc}', ${dir.seq}, ${dir.epoch}, decode('${forged.toString('hex')}', 'hex'), decode('${fhash}', 'hex'), decode('${crypto.randomBytes(32).toString('hex')}', 'hex'), '${dir.devices[0].id}')`);
  const ra = await cli('B', 'refresh', '--vault');
  ok(ra.code === 4 && ra.one('alarm'), `B raises the alarm on a server-tampered directory: ${ra.one('alarm')}`);
  // Different bytes at the seq B already pinned (a fork) are caught too.
  psql(`DELETE FROM ak_dirs WHERE account_id = '${acc}' AND seq = ${dir.seq}`);
  const pinned = psql(`SELECT seq FROM ak_dirs WHERE account_id = '${acc}' ORDER BY seq DESC LIMIT 1`);
  const orig = psql(`SELECT encode(bytes, 'hex') FROM ak_dirs WHERE account_id = '${acc}' AND seq = ${pinned}`);
  const d2 = JSON.parse(Buffer.from(orig, 'hex').toString());
  d2.created_ms += 1;
  const b2 = Buffer.from(JSON.stringify(d2));
  psql(`UPDATE ak_dirs SET bytes = decode('${b2.toString('hex')}', 'hex'), hash = decode('${crypto.createHash('sha256').update(b2).digest('hex')}', 'hex') WHERE account_id = '${acc}' AND seq = ${pinned}`);
  const rb = await cli('A', 'refresh', '--vault');
  ok(rb.code === 4, `A raises the alarm on different bytes at a pinned seq (fork): ${rb.one('alarm')}`);
  psql(`UPDATE ak_dirs SET bytes = decode('${orig}', 'hex'), hash = decode('${crypto.createHash('sha256').update(Buffer.from(orig, 'hex')).digest('hex')}', 'hex') WHERE account_id = '${acc}' AND seq = ${pinned}`);
  const rok = await cli('A', 'refresh', '--vault');
  ok(rok.code === 0, `with the server's data put back, A is fine again: ${rok.one('refresh')}`);
  // A swapped escrow key in pak_dir (a new rr_gen with a forged MAC) is caught by PAK holders.
  const ph = psql(`SELECT seq || '|' || encode(bytes, 'hex') || '|' || encode(hash, 'hex') FROM pak_dirs WHERE account_id = '${acc}' ORDER BY seq DESC LIMIT 1`).split('|');
  const pd = JSON.parse(Buffer.from(ph[1], 'hex').toString());
  pd.seq = Number(ph[0]) + 1;
  pd.prev_hash = Buffer.from(ph[2], 'hex').toString('base64url');
  pd.esc_pk = crypto.randomBytes(32).toString('base64url');
  pd.rr_gen += 1;
  const pb = Buffer.from(JSON.stringify(pd));
  psql(`INSERT INTO pak_dirs (account_id, seq, epoch, bytes, hash, mac, created_by) VALUES ('${acc}', ${pd.seq}, ${pd.pak_epoch}, decode('${pb.toString('hex')}', 'hex'), decode('${crypto.createHash('sha256').update(pb).digest('hex')}', 'hex'), decode('${crypto.randomBytes(32).toString('hex')}', 'hex'), '${pd.vault_keys[0].device}')`);
  const rp = await cli('B', 'refresh', '--vault');
  ok(rp.code === 4, `an esc_pk swap in pak_dir is refused by a PAK holder: ${rp.one('alarm')}`);
  psql(`DELETE FROM pak_dirs WHERE account_id = '${acc}' AND seq = ${pd.seq}`);
  ctx.acc = acc;
}

(async () => {
  fs.mkdirSync(WORK, { recursive: true });
  psql('DELETE FROM rate_limits');
  const ctx = {};
  try {
    if (parts.includes('keys')) await partKeys(ctx);
  } catch (e) {
    problems.push(String(e.stack || e));
    say('FAIL ' + (e.stack || e));
  }
  fs.writeFileSync(path.join(WORK, 'sync-e2e.txt'), log.join('\n') + `\n\n${problems.length} problem(s)\n`);
  console.log(`\n${problems.length} problem(s)`);
  process.exit(problems.length ? 1 : 0);
})();
