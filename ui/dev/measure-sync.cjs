// Measures where krunk://sync's controls are, in CSS px of the page, for the VM scripts (the VM
// can't query the DOM: it clicks at coordinates). The page is laid out at the VM's page size
// (window 1280 × 760 minus the shell and the bookmarks bar) for each state the 090 flows reach.
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/measure-sync.cjs [shots dir]
'use strict';
const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const UI = path.resolve(__dirname, '..');
const SHOTS = process.argv[2];
const MIME = { html: 'text/html', css: 'text/css', js: 'text/javascript', svg: 'image/svg+xml', png: 'image/png' };
const W = Number(process.env.W || 1280);
const H = Number(process.env.H || 656);

const me = { id: 'd1', name: 'Krunk on Windows (26200)', platform: 'windows', keys: true, me: true, lastSync: null };
const cli = { id: 'd2', name: 'Linux test client', platform: 'linux', keys: true, me: false, lastSync: null };
const cols = ['Bookmarks', 'Passwords (while the vault is unlocked)', 'History', 'Settings', 'New tab pins', 'Ad-blocker exceptions', 'Open tabs']
  .map((l, i) => ({ name: 'c' + i, label: l, on: true }));
const on = (extra) => Object.assign({
  state: 'on', status: 'Up to date', lastSyncMs: Date.now() - 60000, passwords: true, passwordsLocked: false, rotationPending: false,
  message: null, syncing: false, collections: cols, remoteTabs: [], confirmDevices: [], secureNeeded: false, devices: [me], approvals: [], approving: null,
}, extra);

const STATES = {
  off: [{ state: 'off', message: null }, ['#off-passwords', '#turn-on']],
  setup: [{ state: 'setup', passphrase: 'aaaa-bbbb-cccc-dddd-eeee-ffff-gggg', recoveryKey: 'KRNK-AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG-HHHH-Z', passwords: true, message: null },
    ['#setup-copy', '#setup-save', '#setup-group', '#setup-understood', '#setup-confirm', '#setup-cancel']],
  join: [{ state: 'join', message: null, passwords: true }, ['#join-passwords', '#join-ask', 'input[name=kind][value=recovery]', '#restore-secret', '#restore-go']],
  restore: [{ state: 'restoreConfirm', devices: ['Krunk on Windows (26200)', 'Linux test client'], revoked: 0 }, ['#restore-confirm', '#restore-cancel']],
  'on-new': [on({ message: 'Sync is set up. Keep your passphrase and recovery key safe.' }), ['#sync-now', '#turn-off']],
  'on-approval': [on({ message: 'Sync is set up. Keep your passphrase and recovery key safe.', approvals: [{ id: 'a1', label: 'Linux test client', platform: 'linux', country: null }] }),
    ['#approval-list button', '#share-passwords', '#sync-now']],
  'on-approving': [on({ message: 'Sync is set up. Keep your passphrase and recovery key safe.', approving: { id: 'a1', label: 'Linux test client', code: '482 917', done: false } }),
    ['#approving-code', '#approving-cancel']],
  'on-two': [on({ devices: [me, cli] }), ['#sync-now', '#device-list li:nth-child(2) button:text("Remove")', '#device-list li:nth-child(1) button:text("Rename")']],
  'on-four-msg': [on({ message: 'Sync is restored on this device.', devices: [me,
    { id: 'q', name: 'CLI Q', platform: 'linux', keys: true, me: false, lastSync: null },
    { id: 'r', name: 'CLI R', platform: 'linux', keys: true, me: false, lastSync: null },
    { id: 'k', name: 'Krunk on Windows (26200)', platform: 'windows', keys: true, me: false, lastSync: null }] }),
    ['#sync-now', '#device-list li:nth-child(3) button:text("Remove")', '#device-list li:nth-child(2) button:text("Remove")']],
  'on-two-msg': [on({ message: 'This device is approved and syncs now.', devices: [me, cli] }), ['#sync-now', '#device-list li:nth-child(2) button:text("Remove")']],
};

(async () => {
  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: W, height: H } });
  await ctx.route('**/*', (r) => r.abort());
  await ctx.route('https://krunk.invalid/**', async (r) => {
    const p = new URL(r.request().url()).pathname.replace(/^\//, '');
    const file = p.startsWith('shared/') ? path.join(UI, p) : path.join(UI, 'pages', p);
    if (!fs.existsSync(file)) return r.fulfill({ status: 404, body: '' });
    await r.fulfill({ status: 200, headers: { 'content-type': MIME[file.split('.').pop()] || 'text/plain' }, body: fs.readFileSync(file) });
  });
  await ctx.addInitScript({ path: path.join(__dirname, 'mock-host.js') });
  await ctx.addInitScript(() => { window.__host.handlers.vaultStatus = () => ({ exists: true, locked: false, busy: false, waitMs: 0 }); });
  const page = await ctx.newPage();
  await page.goto('https://krunk.invalid/sync.html');
  await page.waitForTimeout(100);
  const out = {};
  for (const [name, [view, sels]] of Object.entries(STATES)) {
    await page.evaluate((v) => window.__host.emit({ event: 'sync', view: v }), view);
    await page.waitForTimeout(80);
    out[name] = {};
    for (const s of sels) {
      const b = await page.locator(s).first().boundingBox();
      out[name][s] = b ? [Math.round(b.x + b.width / 2), Math.round(b.y + b.height / 2)] : null;
    }
    if (SHOTS) await page.screenshot({ path: path.join(SHOTS, `measure-${name}.png`) });
  }
  // The in-page confirmation of Remove.
  await page.evaluate((v) => window.__host.emit({ event: 'sync', view: v }), STATES['on-two'][0]);
  await page.waitForTimeout(80);
  await page.locator('#device-list li:nth-child(2) button:text("Remove")').click();
  await page.waitForSelector('#lb-dialog-ok');
  const ok = await page.locator('#lb-dialog-ok').boundingBox();
  out['remove-dialog'] = { '#lb-dialog-ok': [Math.round(ok.x + ok.width / 2), Math.round(ok.y + ok.height / 2)] };
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, 'measure-remove-dialog.png') });
  console.log(JSON.stringify(out, null, 1));
  await browser.close();
})();
