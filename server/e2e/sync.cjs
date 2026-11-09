// End-to-end sync tests on a staging stack (0.9.0), with the Linux test client
// (tools/sync-cli, Krunk's own core::sync) as the devices and the website API as the user.
//   node server/e2e/sync.cjs http://dind:10017 stage-browser <krunk-sync-cli> <work dir> [part ...]
// Parts: keys (S6), records (S7: CLI ↔ CLI bookmarks, history, settings, pins, allowlist, tabs,
// device names), passwords (S8: logins and never-save, restore into a vault, a vault key without a
// PAK MAC), rotation (S9: removal, R3 confirmation, AK/PAK rotation and rekey, a removed device
// opens nothing new, Finish securing, a restorer that rotates).
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

// CLI ↔ CLI records (S7): P sets up with data; Q has data of its own (one bookmark the same) and
// joins; both sync. Then edits on both sides, a conflict, a collection off, a rename. The server
// sees only ciphertext.
async function partRecords(ctx) {
  say('== records (S7)');
  const web = await account(`records-${Date.now()}@example.org`);
  const P = 'P', Q = 'Q';
  await link(web, P);
  await link(web, Q);
  const must = async (name, ...args) => {
    const r = await cli(name, ...args);
    if (r.code !== 0) throw new Error(`${name} ${args.join(' ')}: exit ${r.code} ${r.err}`);
    return r;
  };
  const list = async (name, what, sub = 'list') => (await must(name, what, sub)).lines.filter((l) => /^[a-z_]+: /.test(l)).sort();
  // P's data before sync is on.
  await must(P, 'bookmark', 'add', '--title', 'Rust', '--url', 'https://www.rust-lang.org/');
  await must(P, 'bookmark', 'add', '--folder', 'Work', '--title', 'Work site', '--url', 'https://example.org/work');
  await must(P, 'history', 'add', '--url', 'https://example.org/visited-on-p', '--title', 'Visited on P');
  await must(P, 'settings', 'set', '--key', 'theme', '--value', '"dark"');
  await must(P, 'settings', 'set', '--key', 'tabSleepMinutes', '--value', '45');
  await must(P, 'pin', 'add', '--url', 'https://news.example/', '--title', 'News');
  await must(P, 'allow', 'add', '--host', 'shop.example');
  await must(P, 'tabs', 'set', 'https://example.org/tab-1', 'https://example.org/tab-2');
  const setup = await must(P, 'setup');
  ok(setup.one('passphrase'), 'P sets up sync');
  const s1 = await must(P, 'sync', '--name', 'Laptop P');
  ok(Number(s1.one('pushed')) >= 7 && s1.one('queue_left') === '0', `P's first sync pushes its data: pushed ${s1.one('pushed')}, left ${s1.one('queue_left')}`);
  // The server holds ciphertext only.
  const acc = psql(`SELECT id FROM accounts WHERE email = '${web.email}'`);
  const leaks = psql(`SELECT count(*) FROM records WHERE account_id = '${acc}' AND (envelope::text ILIKE '%rust-lang%' OR envelope::text ILIKE '%example.org%' OR envelope::text ILIKE '%shop.example%' OR envelope::text LIKE '%"dark"%')`);
  const n = psql(`SELECT count(*) FROM records WHERE account_id = '${acc}'`);
  ok(leaks === '0' && Number(n) >= 7, `the server stores ${n} records, none with a URL, host or setting in clear`);
  // Q's own data: the same Rust bookmark (adopted, not duplicated) and one of its own.
  await must(Q, 'bookmark', 'add', '--title', 'Rust', '--url', 'https://www.rust-lang.org/');
  await must(Q, 'bookmark', 'add', '--title', 'Only on Q', '--url', 'https://q.example/');
  await must(Q, 'history', 'add', '--url', 'https://example.org/visited-on-q', '--title', 'Visited on Q');
  await must(Q, 'tabs', 'set', 'https://example.org/q-tab');
  const ap = await approval(P, Q);
  ok(ap.j.code === 0 && ap.ap.code === 0, `Q joins (SAS ${ap.sas}) ${ap.j.err} ${ap.ap.err}`);
  const q1 = await must(Q, 'sync', '--name', 'Desktop Q');
  ok(Number(q1.one('pulled')) >= 7, `Q's first sync pulls P's records (${q1.one('pulled')}) and pushes its own (${q1.one('pushed')})`);
  const p2 = await must(P, 'sync');
  ok(Number(p2.one('pulled')) >= 1, `P pulls Q's records (${p2.one('pulled')})`);
  await must(Q, 'sync');
  const bp = await list(P, 'bookmark'), bq = await list(Q, 'bookmark');
  ok(JSON.stringify(bp) === JSON.stringify(bq), `bookmarks equal on P and Q:\n    ${bp.join('\n    ')}`);
  ok(bp.filter((l) => l.includes('rust-lang')).length === 1, 'the bookmark both had is there once (first sync adopts, no duplicate)');
  ok(bq.some((l) => l.includes('Work/Work site')) && bp.some((l) => l.includes('q.example')), 'P\'s folder reached Q, Q\'s bookmark reached P');
  const hq = await list(Q, 'history'), hp = await list(P, 'history');
  ok(hq.some((l) => l.includes('visited-on-p')) && hp.some((l) => l.includes('visited-on-q')), 'history: the union on both');
  ok((await must(Q, 'settings', 'get', '--key', 'theme')).lines[0] === '"dark"', 'a synced setting (theme) reached Q');
  ok((await must(Q, 'settings', 'get', '--key', 'tabSleepMinutes')).lines[0] !== '45', 'a device-local setting (tabSleepMinutes) did not');
  ok((await list(Q, 'pin')).some((l) => l.includes('news.example')), 'the pin reached Q');
  ok((await list(Q, 'allow')).includes('allow: shop.example'), 'the ad-blocker exception reached Q');
  const rq = await list(Q, 'tabs', 'remote');
  ok(rq.some((l) => l.includes('tab-1')) && !rq.some((l) => l.includes('q-tab')), `Q sees P's open tabs (read-only), not its own as remote: ${rq.join(', ')}`);
  ok((await list(P, 'tabs', 'remote')).some((l) => l.includes('q-tab')), 'P sees Q\'s open tabs');

  // Edits on both sides, then a conflict on the same bookmark.
  await must(P, 'bookmark', 'edit', '--url', 'https://www.rust-lang.org/', '--title', 'Rust (P)');
  await must(Q, 'bookmark', 'delete', '--url', 'https://q.example/');
  await must(Q, 'bookmark', 'add', '--folder', 'Work', '--title', 'Added on Q', '--url', 'https://example.org/q-added');
  await must(P, 'sync'); await must(Q, 'sync'); await must(P, 'sync');
  const ep = await list(P, 'bookmark'), eq = await list(Q, 'bookmark');
  ok(JSON.stringify(ep) === JSON.stringify(eq) && ep.some((l) => l.includes('Rust (P)')) && !ep.some((l) => l.includes('q.example'))
    && ep.some((l) => l.includes('Work/Added on Q')), 'edits on both sides merged (rename, delete, add to a folder)');
  await must(P, 'bookmark', 'edit', '--url', 'https://example.org/work', '--title', 'Work from P');
  await sleep(20);
  await must(Q, 'bookmark', 'edit', '--url', 'https://example.org/work', '--title', 'Work from Q');
  await must(P, 'sync');
  const cq = await must(Q, 'sync');
  await must(P, 'sync');
  const fp = await list(P, 'bookmark'), fq = await list(Q, 'bookmark');
  ok(JSON.stringify(fp) === JSON.stringify(fq) && fp.filter((l) => /Work from [PQ]/.test(l)).length === 1,
    `the same bookmark edited on both: one title wins on both (conflicts ${cq.one('conflicts')}): ${fp.find((l) => /Work from/.test(l))}`);

  // A collection off on Q: P's new history stays away from Q; Q's other collections still sync.
  await must(P, 'history', 'add', '--url', 'https://example.org/while-q-off', '--title', 'x');
  await must(P, 'bookmark', 'add', '--title', 'After off', '--url', 'https://after.example/');
  await must(P, 'sync');
  await must(Q, 'sync', '--off', 'history');
  ok(!(await list(Q, 'history')).some((l) => l.includes('while-q-off')) && (await list(Q, 'bookmark')).some((l) => l.includes('after.example')),
    'history switched off on Q: not pulled; bookmarks still sync');
  // A rename through device_meta.
  const qid = JSON.parse(fs.readFileSync(path.join(dirOf(Q), 'cli.json'), 'utf8')).device_id;
  await must(P, 'rename', qid, '--name', 'Living room PC');
  await must(P, 'sync'); await must(Q, 'sync');
  ok((await list(Q, 'names')).some((l) => l.includes(`${qid} | Living room PC`)), 'a rename on P reaches Q (encrypted device_meta)');
  // Nothing left queued, and a run with nothing to do sends nothing.
  const idle = await must(P, 'sync');
  ok(idle.one('pushed') === '0' && idle.one('queue_left') === '0', `an idle run pushes nothing (pulled ${idle.one('pulled')})`);
  ctx.records = { acc };
}

// Passwords and the never-save list (S8): R sets up with its vault; S restores with the recovery
// key into a vault; T restores with the passphrase but no vault (its passwords wait). Edits,
// deletions and a conflict between R and S; the server sees ciphertext only; a vault key added
// to pak_dir without a PAK MAC is refused.
async function partPasswords(ctx) {
  say('== passwords (S8)');
  const web = await account(`passwords-${Date.now()}@example.org`);
  const must = async (name, ...args) => {
    const r = await cli(name, ...args);
    if (r.code !== 0) throw new Error(`${name} ${args.join(' ')}: exit ${r.code} ${r.err}`);
    return r;
  };
  const list = async (name, what, sub = 'list') => (await must(name, what, sub)).lines.filter((l) => /^[a-z_]+: /.test(l)).sort();
  const logins = async (name) => (await list(name, 'password')).map((l) => l.replace(/ \| [0-9a-f]{32}$/, ''));
  for (const n of ['R', 'S', 'T']) await link(web, n);
  await must('R', 'password', 'add', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'mail-secret-1');
  await must('R', 'password', 'add', '--origin', 'https://bank.example', '--user', 'ann@x', '--password', 'bank-secret-1');
  await must('R', 'never', 'add', '--origin', 'https://never.example');
  const setup = await must('R', 'setup', '--vault');
  ok(setup.one('passphrase') && setup.one('recovery_key'), 'R sets up sync with its vault');
  const r1 = await must('R', 'sync', '--vault');
  const r2 = await must('R', 'sync', '--vault');
  ok(Number(r1.one('pw_pushed')) + Number(r2.one('pw_pushed')) === 3, `R pushes 2 logins and 1 never-save origin (first run pulls only: ${r1.one('pw_pushed')} + ${r2.one('pw_pushed')})`);
  const acc = psql(`SELECT id FROM accounts WHERE email = '${web.email}'`);
  const pakRows = psql(`SELECT count(*) FROM records WHERE account_id = '${acc}' AND k = 'pak'`);
  const leaks = psql(`SELECT count(*) FROM records WHERE account_id = '${acc}' AND (envelope::text ILIKE '%secret%' OR envelope::text ILIKE '%mail.example%' OR envelope::text ILIKE '%never.example%' OR envelope::text LIKE '%"ann"%')`);
  ok(pakRows === '3' && leaks === '0', `the server holds ${pakRows} records under the passwords key, none in clear`);
  // S: recovery key → vault.
  const rs = await must('S', 'restore', '--recovery', setup.one('recovery_key'), '--vault');
  ok(rs.one('passwords') === 'on', 'S restores with the recovery key and gets the passwords key into its vault');
  const s1 = await must('S', 'sync', '--vault');
  ok(Number(s1.one('pw_pulled')) === 3, `S pulls the passwords records (${s1.one('pw_pulled')})`);
  ok(JSON.stringify(await logins('S')) === JSON.stringify(await logins('R')), `S has R's logins: ${(await logins('S')).join('; ')}`);
  ok((await list('S', 'never')).includes('never: https://never.example'), 'and the never-save list');
  // T: passphrase, no vault: the AK collections sync, the passwords wait (and stay unreadable).
  const rt = await must('T', 'restore', '--passphrase', setup.one('passphrase'));
  ok(rt.one('passwords') !== 'on', 'T restores without a vault: no passwords key');
  const t1 = await must('T', 'sync');
  ok(Number(t1.one('pak_waiting')) >= 3 && !fs.existsSync(path.join(dirOf('T'), 'vault.json')), `T's run leaves ${t1.one('pak_waiting')} passwords records waiting, no vault written`);
  // Edits on both sides, a delete, and a conflict (S's later edit wins; R keeps its version as previous).
  await must('S', 'password', 'edit', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'mail-secret-2');
  await must('R', 'password', 'delete', '--origin', 'https://bank.example', '--user', 'ann@x');
  await must('S', 'never', 'remove', '--origin', 'https://never.example');
  await must('S', 'sync', '--vault'); await must('R', 'sync', '--vault'); await must('S', 'sync', '--vault');
  const lr = await logins('R'), ls = await logins('S');
  ok(JSON.stringify(lr) === JSON.stringify(ls) && lr.length === 1 && lr[0].includes('mail-secret-2'), `edit and delete merged: ${lr.join('; ')}`);
  ok((await list('R', 'never')).length === 0, 'the never-save removal reached R');
  await must('R', 'password', 'edit', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'from-r');
  await sleep(30);
  await must('S', 'password', 'edit', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'from-s');
  await must('R', 'sync', '--vault');
  const cs = await must('S', 'sync', '--vault');
  await must('R', 'sync', '--vault'); await must('S', 'sync', '--vault');
  const fr = await logins('R'), fs2 = await logins('S');
  ok(JSON.stringify(fr) === JSON.stringify(fs2) && fr[0].includes('from-s'), `a conflict: the later edit wins on both (conflicts ${cs.one('pw_conflicts')}): ${fr[0]}`);
  const prev = [...await list('R', 'password', 'previous'), ...await list('S', 'password', 'previous')];
  ok(prev.some((l) => l.includes('from-r')), 'the losing version is kept in a vault (previous)');
  // The same login saved on R and S before they synced: one login afterwards.
  await must('R', 'password', 'add', '--origin', 'https://dup.example', '--user', 'zed', '--password', 'one');
  await must('S', 'password', 'add', '--origin', 'https://dup.example', '--user', 'zed', '--password', 'two');
  for (let i = 0; i < 3; i++) { await must('R', 'sync', '--vault'); await must('S', 'sync', '--vault'); }
  const dr = (await logins('R')).filter((l) => l.includes('dup.example'));
  ok(dr.length === 1 && JSON.stringify(await logins('R')) === JSON.stringify(await logins('S')), `saved on both: one login on both (${dr.join('; ')})`);
  // A vault key added to pak_dir by the server (or by AK-only malware) without a PAK MAC.
  const ph = psql(`SELECT seq || '|' || encode(bytes, 'hex') || '|' || encode(hash, 'hex') FROM pak_dirs WHERE account_id = '${acc}' ORDER BY seq DESC LIMIT 1`).split('|');
  const pd = JSON.parse(Buffer.from(ph[1], 'hex').toString());
  pd.seq = Number(ph[0]) + 1;
  pd.prev_hash = Buffer.from(ph[2], 'hex').toString('base64url');
  const tid = JSON.parse(fs.readFileSync(path.join(dirOf('T'), 'cli.json'), 'utf8')).device_id;
  pd.vault_keys.push({ device: tid, pk: crypto.randomBytes(32).toString('base64url') });
  const pb = Buffer.from(JSON.stringify(pd));
  psql(`INSERT INTO pak_dirs (account_id, seq, epoch, bytes, hash, mac, created_by) VALUES ('${acc}', ${pd.seq}, ${pd.pak_epoch}, decode('${pb.toString('hex')}', 'hex'), decode('${crypto.createHash('sha256').update(pb).digest('hex')}', 'hex'), decode('${crypto.randomBytes(32).toString('hex')}', 'hex'), '${tid}')`);
  const ra = await cli('R', 'refresh', '--vault');
  ok(ra.code === 4, `a vault key added without a PAK MAC is refused by a PAK holder: ${ra.one('alarm')}`);
  psql(`DELETE FROM pak_dirs WHERE account_id = '${acc}' AND seq = ${pd.seq}`);
  ok((await cli('R', 'refresh', '--vault')).code === 0, 'with the row gone, R is fine again');
  ctx.passwords = { acc };
}

// Revocation and rotation (S9) with three CLIs: X sets up, Y restores, Z joins by Y's approval
// (so Z was added by Y). Y is removed: writes freeze, a rotation waits for the device list to be
// confirmed (R3), X rotates AK (rekey, min_record_epoch) and PAK; Y's keys open nothing of the
// new epoch; Z's queued change goes up after the freeze. Then "Finish securing", and a restorer
// that rotates when no other device is online.
async function partRotation(ctx) {
  say('== rotation (S9)');
  const web = await account(`rotation-${Date.now()}@example.org`);
  const must = async (name, ...args) => {
    const r = await cli(name, ...args);
    if (r.code !== 0) throw new Error(`${name} ${args.join(' ')}: exit ${r.code} ${r.err}`);
    return r;
  };
  const list = async (name, what, sub = 'list') => (await must(name, what, sub)).lines.filter((l) => /^[a-z_]+: /.test(l)).sort();
  const idOf = (n) => JSON.parse(fs.readFileSync(path.join(dirOf(n), 'cli.json'), 'utf8')).device_id;
  for (const n of ['X', 'Y', 'Z']) await link(web, n);
  const setup = await must('X', 'setup', '--vault');
  const pass = setup.one('passphrase');
  await must('Y', 'restore', '--passphrase', pass, '--vault');
  const ap = await approval('Y', 'Z', { share: true, nVault: true });
  ok(ap.j.code === 0 && ap.ap.code === 0, `Z joins by Y's approval (added by Y) ${ap.j.err} ${ap.ap.err}`);
  await must('X', 'bookmark', 'add', '--title', 'Before', '--url', 'https://before.example/');
  await must('X', 'password', 'add', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'pw-1');
  for (const n of ['X', 'Y', 'Z', 'X', 'Y', 'Z']) await must(n, 'sync', '--vault', '--yes');
  ok((await list('Z', 'bookmark')).some((l) => l.includes('before.example')) && (await list('Z', 'password')).length === 1, 'all three sync bookmarks and passwords');
  const acc = psql(`SELECT id FROM accounts WHERE email = '${web.email}'`);
  // Y is removed (from X): its tokens die, the writes freeze.
  const rv = await must('X', 'revoke', idOf('Y'));
  ok(rv.one('revoke') === '204' || rv.one('revoke') === '200', `X removes Y (${rv.one('revoke')})`);
  ok(psql(`SELECT rotation_ak || ',' || rotation_pak FROM sync_state WHERE account_id = '${acc}'`) === 'true,true', 'the server flags both keys for rotation');
  ok((await cli('Y', 'sync')).code !== 0, 'Y can no longer reach the account');
  // Z (added by Y) adds a bookmark; its run asks for the device list first and keeps the change.
  await must('Z', 'bookmark', 'add', '--title', 'While frozen', '--url', 'https://frozen.example/');
  const z1 = await must('Z', 'sync');
  ok(z1.one('frozen') === 'true' && (z1.kv.confirm || []).includes(idOf('Z')) && Number(z1.one('queue_left')) > 0,
    `Z's run waits: Z was added by the removed Y, so the list needs confirming (R3); the change stays queued (${z1.one('queue_left')})`);
  // X confirms (it removed Y) and rotates.
  const x1 = await must('X', 'sync', '--vault', '--yes');
  ok(x1.one('rotated') === '2' && x1.one('epoch') === '2' && x1.one('min_record_epoch') === '2', `X rotates AK to epoch 2, rekeys, and raises min_record_epoch (${x1.one('epoch')}/${x1.one('min_record_epoch')})`);
  const stRow = psql(`SELECT current_epoch || ',' || pak_epoch || ',' || rotation_ak || ',' || rotation_pak FROM sync_state WHERE account_id = '${acc}'`);
  ok(stRow === '2,2,false,false', `both keys rotated on the server (epoch, pak_epoch, flags: ${stRow})`);
  const old = psql(`SELECT count(*) FROM records r JOIN sync_state s USING (account_id) WHERE r.account_id = '${acc}' AND ((r.k = 'ak' AND r.e < s.current_epoch) OR (r.k = 'pak' AND r.e < s.pak_epoch))`);
  ok(old === '0', 'no record of the old epochs is left (rekeyed)');
  const head = psql(`SELECT encode(bytes, 'escape') FROM ak_dirs WHERE account_id = '${acc}' ORDER BY seq DESC LIMIT 1`);
  const hd = JSON.parse(head);
  ok(hd.revoked.includes(idOf('Y')) && !hd.devices.some((d) => d.id === idOf('Y')) && !hd.seals.some((x) => x.to === idOf('Y')) && !hd.hist,
    'the head directory lists Y as revoked, seals nothing to it, and has no hist any more');
  // Y's keys (every AK and PAK it held) open none of the records now on the server.
  const dump = path.join(WORK, 'records-after-rotation.json');
  await must('X', 'dump-records', '--out', dump);
  const ty = await must('Y', 'try-decrypt', '--file', dump, '--vault');
  ok(Number(ty.one('records')) > 0 && ty.one('opened') === '0', `the removed Y opens ${ty.one('opened')} of ${ty.one('records')} records`);
  const tx = await must('X', 'try-decrypt', '--file', dump, '--vault');
  ok(tx.one('opened') === tx.one('records'), `X opens all ${tx.one('opened')} (control)`);
  // Z follows the new epoch (sealed to it) and its queued change goes up.
  const z2 = await must('Z', 'sync', '--vault');
  ok(z2.one('epoch') === '2' && Number(z2.one('pushed')) >= 1, `Z follows epoch 2 and pushes its queued bookmark (${z2.one('pushed')})`);
  await must('X', 'sync', '--vault');
  ok((await list('X', 'bookmark')).some((l) => l.includes('frozen.example')), 'X has the bookmark Z made while frozen');
  await must('X', 'password', 'edit', '--origin', 'https://mail.example', '--user', 'ann', '--password', 'pw-2');
  await must('X', 'sync', '--vault'); await must('Z', 'sync', '--vault');
  ok((await list('Z', 'password')).some((l) => l.includes('pw-2')), 'passwords keep syncing under the new passwords key');
  // Finish securing: a new anchor at the rotated heads.
  ok(x1.one('secure_needed') === 'true', 'X asks to finish securing');
  await must('X', 'secure', '--passphrase', pass, '--vault');
  const anchor = psql(`SELECT epoch FROM ak_dirs WHERE account_id = '${acc}' AND escrow_mac IS NOT NULL ORDER BY seq DESC LIMIT 1`);
  ok(anchor === '2', `the newest escrow anchor is at epoch ${anchor}`);
  const z3 = await must('Z', 'sync', '--vault');
  ok(z3.one('secure_needed') === 'false', 'Z sees the new anchor (no securing needed there)');
  // Z is removed too and nobody runs: W restores with the passphrase and rotates in one commit.
  await must('X', 'revoke', idOf('Z'));
  await link(web, 'W');
  const rw = await must('W', 'restore', '--passphrase', pass, '--vault');
  ok(rw.one('passwords') === 'on', 'W restores while a rotation is pending');
  const st3 = psql(`SELECT current_epoch || ',' || pak_epoch || ',' || rotation_ak || ',' || rotation_pak FROM sync_state WHERE account_id = '${acc}'`);
  ok(st3 === '3,3,false,false', `the restorer rotated both keys while joining (${st3})`);
  await must('W', 'sync', '--vault'); await must('W', 'sync', '--vault');
  const x3 = await must('X', 'sync', '--vault');
  ok(x3.one('epoch') === '3' && JSON.stringify(await list('X', 'bookmark')) === JSON.stringify(await list('W', 'bookmark')), `X follows epoch 3; X and W agree`);
  ok(JSON.stringify(await list('W', 'password')) === JSON.stringify(await list('X', 'password')), 'and the passwords too');
  await must('X', 'dump-records', '--out', dump);
  const tz = await must('Z', 'try-decrypt', '--file', dump, '--vault');
  ok(tz.one('opened') === '0', `the removed Z opens ${tz.one('opened')} of ${tz.one('records')}`);
  ctx.rotation = { acc };
}

(async () => {
  fs.mkdirSync(WORK, { recursive: true });
  psql('DELETE FROM rate_limits');
  const ctx = {};
  try {
    if (parts.includes('keys')) await partKeys(ctx);
    if (parts.includes('records')) await partRecords(ctx);
    if (parts.includes('passwords')) await partPasswords(ctx);
    if (parts.includes('rotation')) await partRotation(ctx);
  } catch (e) {
    problems.push(String(e.stack || e));
    say('FAIL ' + (e.stack || e));
  }
  fs.writeFileSync(path.join(WORK, 'sync-e2e.txt'), log.join('\n') + `\n\n${problems.length} problem(s)\n`);
  console.log(`\n${problems.length} problem(s)`);
  process.exit(problems.length ? 1 : 0);
})();
