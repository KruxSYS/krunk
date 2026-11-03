// UI tests for the shell, popup and new-tab page, run in Chromium with the
// mock host (ui/dev/mock-host.js). These test the web UI only; they do not
// run LightBrowser.exe.
//
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node ui/dev/test-ui.cjs [screenshot-dir]
//
// Files are served under the real internal origins with the same CSP the
// host sends (core/src/assets.rs), so CSP violations show up here too.
'use strict';
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const UI = path.resolve(__dirname, '..');
const SHOTS = process.argv[2] || null;
const CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; " +
  "connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";
const MIME = { html: 'text/html', css: 'text/css', js: 'text/javascript', svg: 'image/svg+xml', png: 'image/png' };

let failures = 0;
let passes = 0;
function check(cond, name) {
  if (cond) { passes++; console.log('  ok   ' + name); } else { failures++; console.log('  FAIL ' + name); }
}

async function route(context) {
  const handler = (dir) => async (r) => {
    const url = new URL(r.request().url());
    let p = url.pathname.replace(/^\//, '');
    const file = p.startsWith('shared/') ? path.join(UI, p) : path.join(UI, dir, p);
    if (!file.startsWith(UI) || !fs.existsSync(file)) return r.fulfill({ status: 404, body: 'not found' });
    const ext = file.split('.').pop();
    await r.fulfill({
      status: 200,
      headers: { 'content-type': MIME[ext] || 'application/octet-stream', 'content-security-policy': CSP },
      body: fs.readFileSync(file),
    });
  };
  await context.route('https://shell.lightbrowser.invalid/**', handler('shell'));
  await context.route('https://lightbrowser.invalid/**', handler('pages'));
}

async function openPage(browser, url, viewport, scale) {
  const context = await browser.newContext({ viewport: viewport, deviceScaleFactor: scale || 1 });
  await route(context);
  await context.addInitScript({ path: path.join(__dirname, 'mock-host.js') });
  const page = await context.newPage();
  const errors = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(url);
  return { page, context, errors };
}

const sent = (page) => page.evaluate(() => window.__host.sent);
const lastSent = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).pop();
const emit = (page, ev) => page.evaluate((e) => window.__host.emit(e), ev);
const shot = async (page, name) => { if (SHOTS) await page.screenshot({ path: path.join(SHOTS, name) }); };

async function testShell(browser) {
  console.log('shell');
  const { page, errors } = await openPage(browser, 'https://shell.lightbrowser.invalid/shell.html', { width: 1100, height: 76 }, 1);
  check((await lastSent(page, 'ready')) !== undefined, 'shell posts ready');
  const favicon = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAPElEQVR4nGNgGNTgXorcfxgmWyM6pkgzQUOI0YzXEIoMIEUzVkMG3oCBD0RSDMGpmRhDCGrGZRBJGkkBADgsHvekgD78AAAAAElFTkSuQmCC';
  await page.evaluate((fav) => {
    const h = window.__host;
    h.state.tabs = [
      { id: 1, title: 'Rust Programming Language', url: 'https://www.rust-lang.org/', favicon: fav, loading: false, private: false, sleeping: false },
      { id: 2, title: 'Loading… Example News', url: 'https://news.example.com/', favicon: null, loading: true, private: false, sleeping: false },
      { id: 3, title: 'Private: DuckDuckGo', url: 'https://duckduckgo.com/', favicon: null, loading: false, private: true, sleeping: false },
      { id: 4, title: 'Docs.rs (asleep)', url: 'https://docs.rs/', favicon: null, loading: false, private: false, sleeping: true },
    ];
    h.state.active = 1;
    h.tabsEvent();
    h.navEvent();
  }, favicon);
  await page.waitForSelector('.tab');
  check((await page.locator('.tab').count()) === 4, 'renders 4 tabs');
  check(await page.locator('.tab.active').getAttribute('data-id') === '1', 'tab 1 active');
  check((await page.locator('.tab.private').count()) === 1, 'private tab has private style');
  check((await page.locator('.tab.private .private-mark').count()) === 1, 'private tab shows mask indicator');
  check((await page.locator('.tab[data-id="2"] .spinner').count()) === 1, 'loading tab shows spinner');
  check((await page.locator('.tab.sleeping').count()) === 1, 'sleeping tab dimmed');
  check(await page.locator('#address').inputValue() === 'https://www.rust-lang.org/', 'address shows active URL');
  check(await page.locator('#shield-count').textContent() === '12', 'shield shows blocked count');
  await shot(page, 'shell-tabs.png');

  // Switch tabs.
  await page.locator('.tab[data-id="3"] .title').click();
  const act = await lastSent(page, 'activateTab');
  check(act && act.id === 3, 'clicking a tab sends activateTab');
  await page.waitForSelector('.tab.active[data-id="3"]');
  check(await page.locator('#private-badge').isVisible(), 'private badge shown for private tab');
  await shot(page, 'shell-private-active.png');

  // New tab.
  await page.click('#newtab');
  check((await lastSent(page, 'newTab')) !== undefined, 'new tab button sends newTab');
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 5);
  check((await page.locator('.tab').count()) === 5, 'new tab appears');

  // Close.
  await page.locator('.tab[data-id="2"] .close').click({ force: true });
  const close = await lastSent(page, 'closeTab');
  check(close && close.id === 2, 'close button sends closeTab{id:2}');
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 4);
  check((await page.locator('.tab[data-id="2"]').count()) === 0, 'closed tab removed');
  check(!(await sent(page)).some((m) => m.cmd === 'activateTab' && m.id === 2), 'close click does not activate the tab');

  // Middle click closes.
  await page.locator('.tab[data-id="4"] .title').click({ button: 'middle' });
  const mclose = await lastSent(page, 'closeTab');
  check(mclose && mclose.id === 4, 'middle click closes tab');
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 3);

  // Drag tab 1 to the right of the last tab: order [1,3,new] -> [3,new,1].
  const order = await page.$$eval('.tab', (ns) => ns.map((n) => +n.dataset.id));
  const lastId = order[order.length - 1];
  const target = page.locator('.tab[data-id="' + lastId + '"]');
  const box = await target.boundingBox();
  await page.locator('.tab[data-id="1"]').dragTo(target, { targetPosition: { x: box.width - 5, y: box.height / 2 } });
  const mv = await lastSent(page, 'moveTab');
  check(mv && mv.id === 1 && mv.index === 2, 'drag-reorder sends moveTab{id:1,index:2} (got ' + JSON.stringify(mv) + ')');
  await page.waitForTimeout(50);
  const after = await page.$$eval('.tab', (ns) => ns.map((n) => +n.dataset.id));
  check(after[2] === 1, 'tab 1 is now last');

  // Drag onto the left half of the first tab: index 0.
  const first = page.locator('.tab').first();
  await page.locator('.tab[data-id="1"]').dragTo(first, { targetPosition: { x: 5, y: 10 } });
  const mv2 = await lastSent(page, 'moveTab');
  check(mv2 && mv2.id === 1 && mv2.index === 0, 'drag to the front sends index 0');

  // Address bar.
  await page.click('#address');
  await page.keyboard.type('rust borrow checker');
  await page.keyboard.press('Enter');
  const nav = await lastSent(page, 'navigate');
  check(nav && nav.input === 'rust borrow checker', 'typing + Enter sends navigate{input}');
  await page.click('#address');
  await page.keyboard.type('garbage');
  await page.keyboard.press('Escape');
  check(!(await sent(page)).some((m) => m.cmd === 'navigate' && m.input.includes('garbage')), 'Escape does not navigate');

  // Menu popup open/close.
  await page.click('#menu');
  const sp = await lastSent(page, 'showPopup');
  check(sp && sp.popup === 'menu' && sp.rect.width === 280 && sp.rect.height === 482 && sp.rect.x >= 0 && sp.rect.y > 60, 'menu sends showPopup with a rect below the toolbar');
  check(await page.locator('#menu.on').count() === 1, 'menu button highlighted while open');
  await page.click('#menu');
  check((await lastSent(page, 'hidePopup')) !== undefined, 'clicking menu again sends hidePopup');
  check(await page.locator('#menu.on').count() === 0, 'menu button no longer highlighted');
  await page.click('#shield');
  const sh = await lastSent(page, 'showPopup');
  check(sh && sh.popup === 'shield', 'shield sends showPopup{shield}');
  await emit(page, { event: 'popupClosed' });
  await page.waitForTimeout(20);
  check(await page.locator('#shield.on').count() === 0, 'popupClosed from host clears highlight');

  // Toolbar buttons.
  await page.click('#back');
  check((await lastSent(page, 'back')) !== undefined, 'back button sends back');
  await page.click('#star');
  check((await lastSent(page, 'toggleBookmark')) !== undefined, 'star sends toggleBookmark');

  // Loading state shows stop.
  await page.evaluate(() => window.__host.navEvent({ loading: true }));
  await page.waitForTimeout(20);
  await page.click('#reload');
  check((await lastSent(page, 'stop')) !== undefined, 'reload button acts as stop while loading');

  check(errors.length === 0, 'no console errors / CSP violations (' + errors.join(' | ') + ')');
}

async function testShellHiDpi(browser) {
  console.log('shell @150%');
  const { page } = await openPage(browser, 'https://shell.lightbrowser.invalid/shell.html', { width: 900, height: 76 }, 1.5);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [
      { id: 1, title: 'Example Domain', url: 'https://example.com/', favicon: null, loading: false, private: false, sleeping: false },
      { id: 2, title: 'Private tab', url: '', favicon: null, loading: true, private: true, sleeping: false },
    ];
    h.state.active = 2;
    h.tabsEvent();
    h.navEvent({ url: '', loading: true, private: true });
  });
  await page.waitForSelector('.tab');
  check(!(await page.locator('#star').isVisible()), 'no bookmark star on an empty new tab');
  await shot(page, 'shell-150pct-private-loading.png');
}

async function testPopup(browser) {
  console.log('popup');
  const make = async (w, h) => openPage(browser, 'https://shell.lightbrowser.invalid/popup.html', { width: w, height: h }, 1);

  let { page, errors } = await make(280, 482);
  await page.evaluate(() => { window.__host.autoSimulate = false; });
  check((await lastSent(page, 'ready')) !== undefined, 'popup posts ready');
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1.25, canReopen: true } });
  await page.waitForSelector('.item');
  check(await page.locator('#zoom-value').textContent() === '125%', 'menu shows zoom');
  await shot(page, 'popup-menu.png');
  await page.locator('.item', { hasText: 'New private tab' }).click();
  const cmd = await lastSent(page, 'command');
  check(cmd && cmd.command.command === 'newPrivateTab', 'menu item sends command newPrivateTab');
  check((await lastSent(page, 'hidePopup')) !== undefined, 'menu item closes popup');
  await page.locator('.zoom button', { hasText: '+' }).click();
  const z = await lastSent(page, 'zoom');
  check(z && z.action === 'in', 'zoom + sends zoom{in}');
  await page.locator('.item', { hasText: 'Settings' }).click();
  const op = await lastSent(page, 'openPage');
  check(op && op.page === 'settings', 'Settings opens settings page');
  check(errors.length === 0, 'menu: no console errors (' + errors.join(' | ') + ')');

  ({ page, errors } = await make(300, 180));
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 37, allowed: false, enabled: true } });
  await page.waitForSelector('.panel');
  await shot(page, 'popup-shield.png');
  await page.locator('button', { hasText: 'Allow ads on this site' }).click();
  const al = await lastSent(page, 'shieldAllowSite');
  check(al && al.allow === true, 'shield allow sends shieldAllowSite{allow:true}');

  ({ page, errors } = await make(360, 150));
  const save = { kind: 'savePassword', tabId: 7, origin: 'https://bank.example', username: 'alice', update: false };
  await emit(page, { event: 'popup', content: save });
  await page.waitForSelector('.panel');
  await shot(page, 'popup-save-password.png');
  check(await page.evaluate(() => document.activeElement && document.activeElement.textContent) === 'Save', 'Save button focused');
  await page.keyboard.press('Escape');
  let r = await lastSent(page, 'savePasswordReply');
  check(r && r.accept === false && r.tabId === 7 && r.origin === 'https://bank.example', 'Escape = dismiss (accept:false) with tab id and origin');
  await emit(page, { event: 'popup', content: save });
  await page.waitForTimeout(20);
  await page.locator('button', { hasText: /^Save$/ }).click();
  r = await lastSent(page, 'savePasswordReply');
  check(r && r.accept === true && r.tabId === 7 && r.origin === 'https://bank.example' && !('password' in r), 'Save sends accept with tab id/origin and no password');

  ({ page, errors } = await make(340, 236));
  await emit(page, { event: 'popup', content: { kind: 'fill', tabId: 9, origin: 'https://mail.example', entries: [{ id: 'e1', username: 'alice@mail.example' }, { id: 'e2', username: 'bob' }] } });
  await page.waitForSelector('.choice');
  await shot(page, 'popup-fill.png');
  await page.locator('.choice', { hasText: 'bob' }).click();
  const f = await lastSent(page, 'fillReply');
  check(f && f.entryId === 'e2' && f.tabId === 9 && f.origin === 'https://mail.example', 'choosing an entry sends fillReply{entryId,tabId,origin}');
  check(errors.length === 0, 'prompts: no console errors (' + errors.join(' | ') + ')');
}

async function testNewTab(browser) {
  console.log('new tab page');
  const { page, errors } = await openPage(browser, 'https://lightbrowser.invalid/newtab.html', { width: 1000, height: 600 }, 1);
  await page.fill('#q', 'weather');
  await page.keyboard.press('Enter');
  const n = await lastSent(page, 'navigate');
  check(n && n.input === 'weather', 'search box sends navigate');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

(async () => {
  if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
  const browser = await chromium.launch();
  try {
    await testShell(browser);
    await testShellHiDpi(browser);
    await testPopup(browser);
    await testNewTab(browser);
  } finally {
    await browser.close();
  }
  console.log(`\n${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
