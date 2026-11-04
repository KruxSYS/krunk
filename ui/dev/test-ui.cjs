// UI tests for the shell, popup and new-tab page, run in Chromium with the
// mock host (ui/dev/mock-host.js). These test the web UI only; they do not
// run Krunk.exe.
//
//   NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules \
//     node ui/dev/test-ui.cjs [screenshot-dir]
//
// Files are served under the real internal origins with the same CSP the
// host sends (core/src/assets.rs), so CSP violations show up here too.
'use strict';
const { chromium } = require('playwright');
const fs = require('fs');
const zlib = require('zlib');
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

// A 16x16 PNG circle in a colour derived from the host (stands in for
// cached favicons served by the host at /favicon?host=).
function faviconPng(host) {
  let h = 0;
  for (const c of host) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const rgb = [60 + (h & 0x7f), 60 + ((h >> 7) & 0x7f), 60 + ((h >> 14) & 0x7f)];
  const rows = [];
  for (let y = 0; y < 16; y++) {
    const row = [0];
    for (let x = 0; x < 16; x++) {
      const inside = (x - 7.5) ** 2 + (y - 7.5) ** 2 < 56;
      row.push(...(inside ? [...rgb, 255] : [0, 0, 0, 0]));
    }
    rows.push(Buffer.from(row));
  }
  const crcTable = [];
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; crcTable[n] = c >>> 0; }
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(16, 0); ihdr.writeUInt32BE(16, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(Buffer.concat(rows))), chunk('IEND', Buffer.alloc(0))]);
}

async function route(context, external) {
  // Anything not on our internal origins is a network request the built-in
  // UI must never make: record it and abort.
  await context.route('**/*', (r) => { external.push(r.request().url()); return r.abort(); });
  const handler = (dir) => async (r) => {
    const url = new URL(r.request().url());
    if (url.pathname === '/favicon') {
      // Like the host: no cached favicon for this one -> 404 -> letter badge.
      if (url.searchParams.get('host') === '192.168.1.1') return r.fulfill({ status: 404, body: '' });
      return r.fulfill({ status: 200, headers: { 'content-type': 'image/png' }, body: faviconPng(url.searchParams.get('host') || '') });
    }
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
  await context.route('https://shell.krunk.invalid/**', handler('shell'));
  await context.route('https://krunk.invalid/**', handler('pages'));
}

async function openPage(browser, url, viewport, scale, setup, setupArg) {
  const context = await browser.newContext({ viewport: viewport, deviceScaleFactor: scale || 1 });
  const external = [];
  await route(context, external);
  await context.addInitScript({ path: path.join(__dirname, 'mock-host.js') });
  if (setup) await context.addInitScript(setup, setupArg === undefined ? null : setupArg);
  const page = await context.newPage();
  const errors = [];
  // Built-in pages must never use native dialogs (they show the internal origin).
  page.on('dialog', (d) => { errors.push('native ' + d.type() + ' dialog: ' + d.message()); d.dismiss(); });
  page.on('console', (m) => {
    // A favicon the host has not cached is a 404 by design (the page shows a letter instead).
    if (m.type() === 'error' && !(m.text().includes('404') && (m.location().url || '').includes('/favicon?host='))) errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(url);
  return { page, context, errors, external };
}

async function acceptInPage(page) {
  await page.waitForSelector('#lb-dialog-ok');
  await page.click('#lb-dialog-ok');
  await page.waitForTimeout(50);
}

const sent = (page) => page.evaluate(() => window.__host.sent);
const lastSent = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).pop();
const emit = (page, ev) => page.evaluate((e) => window.__host.emit(e), ev);
const shot = async (page, name) => { if (SHOTS) await page.screenshot({ path: path.join(SHOTS, name) }); };

async function testShell(browser) {
  console.log('shell');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 76 }, 1);
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

  // Dropping on the last tab's close button (right half) still means "after".
  await page.waitForTimeout(50);
  const count = await page.locator('.tab').count();
  await page.locator('.tab[data-id="1"]').dragTo(page.locator('.tab').last().locator('.close'), { force: true });
  const mv3 = await lastSent(page, 'moveTab');
  check(mv3 && mv3.id === 1 && mv3.index === count - 1, 'drop over a child of the right half moves after it (got ' + JSON.stringify(mv3) + ')');

  // Double-click on an inactive tab duplicates it, although the first press
  // activates the tab and re-renders the strip.
  await page.waitForTimeout(50);
  const inactive = await page.$eval('.tab:not(.active)', (n) => +n.dataset.id);
  const dupBefore = (await sent(page)).filter((m) => m.cmd === 'duplicateTab').length;
  await page.locator('.tab[data-id="' + inactive + '"] .title').dblclick();
  await page.waitForTimeout(50);
  const dups = (await sent(page)).filter((m) => m.cmd === 'duplicateTab');
  check(dups.length === dupBefore + 1 && dups[dups.length - 1].id === inactive, 'double-click on an inactive tab sends one duplicateTab{id:' + inactive + '}');

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
  check(sp && sp.popup === 'menu' && sp.rect.width === 280 && sp.rect.height === 512 && sp.rect.x >= 0 && sp.rect.y > 60, 'menu sends showPopup with a rect below the toolbar');
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

// Frameless window: caption buttons, hit-test map, tab widths, new-tab focus.
async function testShellFrame(browser) {
  console.log('shell frame + tab widths');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
  const setTabs = (n, active) => page.evaluate(([n, active]) => {
    const h = window.__host;
    h.state.tabs = [];
    for (let i = 1; i <= n; i++) h.state.tabs.push({ id: i, title: 'Tab number ' + i, url: 'https://t' + i + '.example/', favicon: null, loading: false, private: false, sleeping: false });
    h.state.active = active;
    h.tabsEvent();
    h.navEvent();
  }, [n, active]);
  await setTabs(3, 1);
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 3);
  const widths3 = await page.$$eval('.tab', (ns) => ns.map((n) => n.getBoundingClientRect().width));
  check(widths3.every((w) => Math.abs(w - 256) < 0.5), 'with 3 tabs each is 256 DIP wide (' + widths3.join(',') + ')');
  await shot(page, 'shell-frame-3-tabs.png');

  // Hit-test map: the tabs band, "+" and the caption buttons at the right edge.
  await page.waitForTimeout(50);
  const hm = (await lastSent(page, 'hitMap')).map;
  check(hm.stripHeight === 36, 'hit map: tab row height 36');
  const band = hm.interactive.find((r) => r.width > 700);
  check(band && Math.abs(band.x + band.width - (6 + 3 * 256 + 2 * 2)) < 2, 'hit map: tabs band ends after the last tab (' + JSON.stringify(band) + ')');
  check(Math.abs(hm.close.x + hm.close.width - 1280) < 0.5 && hm.close.width === 46 && hm.close.y === 0, 'hit map: close button at the right edge, 46 wide');
  check(hm.maximize.x + hm.maximize.width === hm.close.x && hm.minimize.x + hm.minimize.width === hm.maximize.x, 'hit map: minimize, maximize, close side by side');
  const plus = await page.locator('#newtab').boundingBox();
  check(hm.interactive.some((r) => Math.abs(r.x - plus.x) < 0.5 && r.width === plus.width), 'hit map: the + button is interactive');

  // 20 tabs: they shrink evenly, keep 72 DIP and the active close button.
  await setTabs(20, 20);
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 20);
  await page.waitForTimeout(50);
  const widths20 = await page.$$eval('.tab', (ns) => ns.map((n) => n.getBoundingClientRect().width));
  const minW = Math.min(...widths20), maxW = Math.max(...widths20);
  check(maxW - minW < 0.5, 'with 20 tabs all have the same width (' + minW.toFixed(1) + '–' + maxW.toFixed(1) + ')');
  check(minW >= 72 - 0.01, 'tabs never get narrower than 72 DIP (' + minW.toFixed(1) + ')');
  check(await page.locator('#tabs').evaluate((n) => n.classList.contains('narrow')), 'narrow tabs mode');
  check(await page.locator('.tab.active .close').isVisible(), 'the active tab keeps its close button');
  check(!(await page.locator('.tab:not(.active) .close').first().isVisible()), 'inactive narrow tabs hide theirs');
  const scrolled = await page.locator('#tabs').evaluate((n) => ({ sw: n.scrollWidth, cw: n.clientWidth, sl: n.scrollLeft }));
  check(scrolled.sw > scrolled.cw && scrolled.sl > 0, 'the row scrolls and the active (last) tab is scrolled into view');
  const activeBox = await page.locator('.tab.active').boundingBox();
  const rowBox = await page.locator('#tabs').boundingBox();
  check(activeBox.x >= rowBox.x - 0.5 && activeBox.x + activeBox.width <= rowBox.x + rowBox.width + 0.5, 'active tab fully visible');
  await shot(page, 'shell-frame-20-tabs.png');
  await setTabs(12, 3);
  await page.waitForTimeout(50);
  const widths12 = await page.$$eval('.tab', (ns) => ns.map((n) => n.getBoundingClientRect().width));
  check(Math.max(...widths12) - Math.min(...widths12) < 0.5 && widths12[0] < 256 && widths12[0] > 72, '12 tabs shrink evenly between 72 and 256 (' + widths12[0].toFixed(1) + ')');
  await shot(page, 'shell-frame-12-tabs.png');

  // Caption buttons: hover and pressed states come from the host.
  await emit(page, { event: 'captionState', hover: 'close', pressed: false });
  check(await page.locator('#win-close.hover').count() === 1, 'captionState hover highlights close');
  await shot(page, 'shell-frame-close-hover.png');
  await emit(page, { event: 'captionState', hover: 'maximize', pressed: true });
  check(await page.locator('#win-max.hover.pressed').count() === 1 && await page.locator('#win-close.hover').count() === 0, 'pressed maximize, close no longer hovered');
  await emit(page, { event: 'captionState', hover: null, pressed: false });
  check(await page.locator('.cap.hover').count() === 0, 'hover cleared');
  await emit(page, { event: 'windowState', maximized: true, active: true, fullscreen: false });
  check(await page.locator('#win-max-use').getAttribute('href') === '#i-win-restore' && await page.locator('#win-max').getAttribute('title') === 'Restore', 'maximized: restore glyph');
  await emit(page, { event: 'windowState', maximized: false, active: false, fullscreen: false });
  check(await page.locator('#win-max-use').getAttribute('href') === '#i-win-max', 'restored: maximize glyph');
  check(await page.evaluate(() => document.documentElement.classList.contains('inactive')), 'inactive window state');
  await page.click('#win-max');
  check((await lastSent(page, 'windowControl')).action === 'maximize', 'maximize button (keyboard/click fallback) sends windowControl');
  await page.click('#win-min');
  check((await lastSent(page, 'windowControl')).action === 'minimize', 'minimize sends windowControl');

  // Double-click on empty tab-row space no longer opens a tab (Windows maximizes there).
  const before = (await sent(page)).filter((m) => m.cmd === 'newTab').length;
  await page.dblclick('#drag-space');
  check((await sent(page)).filter((m) => m.cmd === 'newTab').length === before, 'double-click on the tab row background opens no tab');

  // New tab: the host focuses the address bar; the caret is there and typing goes in.
  await page.evaluate(() => { document.getElementById('address').value = 'https://t3.example/'; });
  await emit(page, { event: 'focusAddressBar' });
  await page.waitForFunction(() => document.activeElement === document.getElementById('address'), null, { timeout: 2000 }).catch(() => {});
  const focus = await page.evaluate(() => { const a = document.getElementById('address'); return { active: document.activeElement === a, all: a.selectionStart === 0 && a.selectionEnd === a.value.length }; });
  check(focus.active && focus.all, 'focusAddressBar focuses the address bar with its text selected');
  await page.keyboard.type('kru');
  check(await page.locator('#address').inputValue() === 'kru', 'typing replaces the address bar text');

  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testShellHiDpi(browser) {
  console.log('shell @150%');
  const { page } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 900, height: 76 }, 1.5);
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
  const make = async (w, h) => openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: w, height: h }, 1);

  let { page, errors } = await make(280, 512);
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

  ({ page, errors } = await make(300, 226));
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

const DAY = 86400000;

async function testNewTab(browser) {
  console.log('new tab page');
  const tiles = [
    ['https://www.rust-lang.org/', 'Rust Programming Language', true],
    ['https://news.ycombinator.com/', 'Hacker News'],
    ['https://en.wikipedia.org/wiki/Main_Page', 'Wikipedia'],
    ['https://github.com/', 'GitHub'],
    ['https://www.bbc.co.uk/news', 'BBC News'],
    ['https://docs.rs/', 'Docs.rs'],
    ['https://duckduckgo.com/', 'DuckDuckGo'],
    ['https://example.org/', 'Example Domain'],
    ['https://www.theguardian.com/', 'The Guardian'],
    ['https://192.168.1.1/', 'Router admin'],
  ].map(([url, title, pinned], i) => ({ url, title, pinned: !!pinned, favicon: i === 9 ? null : '/favicon?host=' + new URL(url).hostname }));
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/newtab.html', { width: 1100, height: 640 }, 1,
    (t) => { window.__host.handlers.getTopSites = () => t; }, tiles);
  await page.waitForSelector('.tile');
  check((await page.locator('.tile').count()) === 10, 'renders 10 tiles');
  check((await page.locator('.tile.pinned').count()) === 1, 'one pinned tile');
  // A tile without a favicon shows its letter centred in the circle.
  const off = await page.evaluate(() => {
    const letter = document.querySelector('.tile .fav.letter');
    if (!letter) return null;
    const r = document.createRange(); r.selectNodeContents(letter);
    const g = r.getBoundingClientRect(); const a = letter.closest('.avatar').getBoundingClientRect();
    return Math.max(Math.abs((g.left + g.right) / 2 - (a.left + a.right) / 2), Math.abs((g.top + g.bottom) / 2 - (a.top + a.bottom) / 2));
  });
  check(off !== null && off < 4, 'letter badge is centred in the tile circle (offset ' + off + ' px)');
  await page.waitForTimeout(150);
  await shot(page, 'newtab-tiles.png');
  await page.locator('.tile').nth(1).hover();
  await page.locator('.tile').nth(1).locator('.pin').click();
  const pin = await lastSent(page, 'pinSite');
  check(pin && pin.url === 'https://news.ycombinator.com/' && pin.title === 'Hacker News', 'pin sends pinSite{url,title}');
  await page.locator('.tile').nth(2).hover();
  await page.locator('.tile').nth(2).locator('.icon-btn[title="Remove"]').click();
  const hide = await lastSent(page, 'hideSite');
  check(hide && hide.url === 'https://en.wikipedia.org/wiki/Main_Page', 'remove sends hideSite{url}');
  await page.locator('.tile').nth(0).hover();
  await page.locator('.tile').nth(0).locator('.pin').click();
  check((await lastSent(page, 'unpinSite')).url === 'https://www.rust-lang.org/', 'unpin on a pinned tile');
  await page.fill('#q', 'weather');
  await page.keyboard.press('Enter');
  const n = await lastSent(page, 'navigate');
  check(n && n.input === 'weather', 'search box sends navigate');
  check(external.length === 0, 'new tab page makes zero network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testHistory(browser) {
  console.log('history page');
  const now = Date.now();
  const all = [
    ['https://www.rust-lang.org/learn', 'Learn Rust', 0.1],
    ['https://doc.rust-lang.org/book/', 'The Rust Programming Language book', 0.3],
    ['https://news.ycombinator.com/', 'Hacker News', 0.5],
    ['https://docs.rs/serde/latest/serde/', 'serde - Rust', 1.2],
    ['https://github.com/rust-lang/rust', 'rust-lang/rust: Empowering everyone', 1.4],
    ['https://en.wikipedia.org/wiki/Rust_(programming_language)', 'Rust (programming language) - Wikipedia', 3.2],
  ].map(([url, title, days], i) => ({ id: i + 1, url, title, ts: now - days * DAY, visitCount: 1 + i }));
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/history.html', { width: 1100, height: 700 }, 1,
    (rows) => {
      window.__host.handlers.queryHistory = (m) => rows.filter((r) => !m.text || (r.title + r.url).toLowerCase().includes(m.text.toLowerCase()));
    }, all);
  await page.waitForSelector('.row');
  check((await page.locator('.row').count()) === 6, 'lists 6 visits');
  check((await page.locator('.group-title').count()) >= 3, 'grouped by day');
  await page.fill('#q', 'rust');
  await page.waitForTimeout(300);
  const q = await lastSent(page, 'queryHistory');
  check(q && q.text === 'rust' && q.limit === 100, 'search sends queryHistory{text}');
  check((await page.locator('.row').count()) === 5, 'shows 5 results for "rust"');
  await shot(page, 'history-search.png');
  await page.locator('.row').first().hover();
  await page.locator('.row').first().locator('.icon-btn').click();
  const del = await lastSent(page, 'deleteHistory');
  check(del && del.ids.length === 1 && del.ids[0] === 1, 'remove sends deleteHistory{ids:[1]}');
  await page.waitForTimeout(50);
  check((await page.locator('.row').count()) === 4, 'row removed');
  await page.selectOption('#range', '3600000');
  await page.click('#delete-range');
  await acceptInPage(page);
  const range = await lastSent(page, 'deleteHistoryRange');
  check(range && Math.abs(range.to - range.from - 3600001) < 5000, 'delete last hour sends a 1 h range');
  await page.selectOption('#range', 'all');
  await page.click('#delete-range');
  await acceptInPage(page);
  check((await lastSent(page, 'deleteHistoryRange')).from === 0, 'delete all time starts at 0');
  await page.click('#clear');
  await page.uncheck('#c-cache');
  await page.check('#c-downloads');
  await page.selectOption('#clear-range', '86400000');
  await shot(page, 'history-clear-dialog.png');
  check(await page.evaluate(() => Array.from(document.querySelectorAll('dialog form[method="dialog"]')).every((f) => {
    const def = f.querySelector('button:not([type="button"])');
    return def && def.value === 'ok';
  })), 'history dialogs: Enter confirms (default button is not Cancel)');
  await page.click('#clear-ok');
  await page.waitForTimeout(50);
  const cbd = await lastSent(page, 'clearBrowsingData');
  check(cbd && cbd.data.history && cbd.data.cookies && cbd.data.siteData && !cbd.data.cache && cbd.data.downloads &&
    Math.abs(Date.now() - DAY - cbd.data.since) < 5000, 'clear dialog sends the chosen kinds and range');
  check(external.length === 0, 'history page makes no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

function bookmarkTree() {
  const b = (id, parent, title, url) => ({ id, parent, isFolder: false, title, url, position: 0, added: 0 });
  const f = (id, parent, title, children) => ({ id, parent, isFolder: true, title, url: null, position: 0, added: 0, children });
  return [
    f(1, null, 'Bookmarks bar', [
      b(10, 1, 'Rust', 'https://www.rust-lang.org/'),
      f(11, 1, 'News', [b(12, 11, 'BBC News', 'https://www.bbc.co.uk/news'), b(13, 11, 'Hacker News', 'https://news.ycombinator.com/'),
        f(14, 11, 'Tech', [b(15, 14, 'Ars Technica', 'https://arstechnica.com/')])]),
      f(16, 1, 'Dev', [b(17, 16, 'GitHub', 'https://github.com/'), b(18, 16, 'Docs.rs', 'https://docs.rs/')]),
      b(19, 1, 'Wikipedia', 'https://en.wikipedia.org/'),
    ]),
    f(2, null, 'Other bookmarks', [b(20, 2, 'Example', 'https://example.org/'), f(21, 2, 'Recipes', [])]),
  ];
}

async function testBookmarks(browser) {
  console.log('bookmarks manager');
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/bookmarks.html', { width: 1100, height: 600 }, 1,
    (tree) => {
      const h = window.__host.handlers;
      h.getBookmarks = () => tree;
      h.searchBookmarks = (m) => {
        const out = [];
        const walk = (ns) => ns.forEach((n) => { if (!n.isFolder && (n.title + n.url).toLowerCase().includes(m.text.toLowerCase())) out.push(n); walk(n.children || []); });
        walk(tree);
        return out;
      };
      h.importBookmarks = () => ({ bookmarks: 42, folders: 5, skipped: 1 });
      h.exportBookmarks = () => 'C:\\Users\\me\\Documents\\bookmarks.html';
    }, bookmarkTree());
  await page.waitForSelector('.folder');
  check((await page.locator('.folder').count()) === 6, 'tree shows 6 folders');
  check((await page.locator('#items .row').count()) === 4, 'bar folder lists 4 items');
  check(await page.evaluate(() => Array.from(document.querySelectorAll('dialog form[method="dialog"]')).every((f) => {
    const def = f.querySelector('button:not([type="button"])');
    return def && def.value === 'ok';
  })), 'bookmarks dialogs: Enter confirms (default button is not Cancel)');
  // A bookmark removed in another tab disappears when this tab is shown again.
  await page.evaluate(() => {
    const h = window.__host.handlers;
    window.__origGetBookmarks = h.getBookmarks;
    h.getBookmarks = () => { const t = JSON.parse(JSON.stringify(window.__origGetBookmarks())); t[0].children.shift(); return t; };
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await page.waitForFunction(() => document.querySelectorAll('#items .row').length === 3, null, { timeout: 2000 }).catch(() => {});
  check((await page.locator('#items .row').count()) === 3, 'manager re-reads bookmarks when it becomes visible again');
  await page.evaluate(() => {
    window.__host.handlers.getBookmarks = window.__origGetBookmarks;
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await page.waitForFunction(() => document.querySelectorAll('#items .row').length === 4, null, { timeout: 2000 }).catch(() => {});
  await page.locator('.folder', { hasText: 'News' }).click();
  check((await page.locator('#items .row').count()) === 3, 'News folder has 3 items');
  await page.waitForTimeout(150);
  await shot(page, 'bookmarks-manager.png');
  // Drag "BBC News" onto the "Dev" folder.
  await page.locator('#items .row', { hasText: 'BBC News' }).dragTo(page.locator('.folder', { hasText: 'Dev' }));
  const mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 12 && mv.parent === 16, 'drag onto a folder sends moveBookmark{parent}');
  // Reorder inside the folder: drop Hacker News before BBC News.
  await page.locator('#items .row', { hasText: 'Hacker News' }).dragTo(page.locator('#items .row', { hasText: 'BBC News' }));
  const re = await lastSent(page, 'moveBookmark');
  check(re && re.id === 13 && re.parent === 11 && re.index === 0, 'drag within the list reorders (index 0)');
  // Edit + move via the dialog.
  await page.locator('#items .row', { hasText: 'BBC News' }).hover();
  await page.locator('#items .row', { hasText: 'BBC News' }).locator('.icon-btn[title="Edit"]').click();
  await page.fill('#edit-name', 'BBC');
  await page.selectOption('#edit-folder', '2');
  await page.locator('#edit-dialog button[value="ok"]').click();
  await page.waitForTimeout(80);
  const ed = await lastSent(page, 'editBookmark');
  check(ed && ed.id === 12 && ed.title === 'BBC' && ed.url === 'https://www.bbc.co.uk/news', 'edit dialog sends editBookmark');
  const mv2 = await lastSent(page, 'moveBookmark');
  check(mv2 && mv2.id === 12 && mv2.parent === 2, '"Move to folder" sends moveBookmark to Other bookmarks');
  // A folder cannot be offered as its own destination.
  await page.locator('.folder', { hasText: 'Bookmarks bar' }).click();
  await page.locator('#items .row', { hasText: 'News' }).first().hover();
  await page.locator('#items .row', { hasText: 'News' }).first().locator('.icon-btn[title="Edit"]').click();
  const opts = await page.$$eval('#edit-folder option', (o) => o.map((x) => x.textContent.trim()));
  check(!opts.includes('News') && !opts.includes('Tech'), 'folder dialog excludes itself and its subfolders');
  await page.keyboard.press('Escape');
  await page.fill('#q', 'news');
  await page.waitForTimeout(300);
  check((await page.locator('#items .row').count()) === 2, 'search finds 2 bookmarks');
  await page.locator('#items .row').first().hover();
  await page.locator('#items .row').first().locator('.icon-btn[title="Delete"]').click();
  await acceptInPage(page);
  check((await lastSent(page, 'deleteBookmark')) !== undefined, 'delete (after confirm) sends deleteBookmark');
  await page.click('#import');
  await page.waitForTimeout(50);
  check((await lastSent(page, 'importBookmarks')) !== undefined, 'import asks the host (native file dialog)');
  check((await page.locator('.toast').textContent()).includes('Imported 42'), 'import result shown');
  await page.click('#export');
  check((await lastSent(page, 'exportBookmarks')) !== undefined, 'export asks the host');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testDownloads(browser) {
  console.log('downloads page');
  const now = Date.now();
  const items = [
    { id: 3, url: 'https://releases.example.org/tool-2.4.1-x64.msi', fileName: 'tool-2.4.1-x64.msi', path: 'C:\\Users\\me\\Downloads\\tool-2.4.1-x64.msi', state: 'inProgress', received: 23500000, total: 61200000, started: now, reason: null },
    { id: 2, url: 'https://arxiv.org/pdf/1234.5678', fileName: 'paper.pdf', path: 'C:\\Users\\me\\Downloads\\paper.pdf', state: 'completed', received: 2400000, total: 2400000, started: now - 60000, reason: null },
    { id: 1, url: 'https://cdn.example.com/video.mp4', fileName: 'video.mp4', path: 'C:\\Users\\me\\Downloads\\video.mp4', state: 'interrupted', received: 1000, total: null, started: now - 120000, reason: 'Network error' },
  ];
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/downloads.html', { width: 1000, height: 420 }, 1,
    (d) => { window.__host.handlers.getDownloads = () => d; }, items);
  await page.waitForSelector('.dl');
  check((await page.locator('.dl').count()) === 3, 'lists 3 downloads');
  check((await page.locator('progress').count()) === 1, 'progress bar for the running download');
  await shot(page, 'downloads.png');
  await page.locator('.dl[data-id="2"] button', { hasText: 'Show in folder' }).click();
  const a = await lastSent(page, 'downloadAction');
  check(a && a.id === 2 && a.action === 'showInFolder', 'show in folder sends downloadAction');
  await page.locator('.dl[data-id="3"] button', { hasText: 'Cancel' }).click();
  check((await lastSent(page, 'downloadAction')).action === 'cancel', 'cancel sends downloadAction{cancel}');
  items[0].state = 'completed'; items[0].received = items[0].total;
  await emit(page, { event: 'downloads', items });
  await page.waitForTimeout(50);
  check((await page.locator('progress').count()) === 0, 'pushed update re-renders the list');
  await page.click('#clear');
  check((await lastSent(page, 'clearDownloads')) !== undefined, 'clear list sends clearDownloads');
  check(external.length === 0, 'no network requests');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testStorage(browser) {
  console.log('storage page');
  const long = 'x'.repeat(120);
  const data = {
    sites: [
      { site: 'example.com', cookies: 2, origins: ['https://example.com', 'https://www.example.com'] },
      { site: 'news.example', cookies: 1, origins: [] },
      { site: 'tracker.example', cookies: 0, origins: ['https://tracker.example'] },
    ],
    cookies: {
      'example.com': [
        { name: 'session', value: long, domain: '.example.com', path: '/', expires: null, secure: true, httpOnly: true, sameSite: 'lax' },
        { name: 'theme', value: 'dark', domain: 'www.example.com', path: '/', expires: Date.UTC(2027, 0, 1), secure: false, httpOnly: false, sameSite: 'unspecified' },
      ],
    },
  };
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/storage.html', { width: 1000, height: 640 }, 1, (d) => {
    const h = window.__host.handlers;
    h.storageList = () => ({ sites: d.sites });
    h.storageUsage = (m) => ({ usage: m.origins.map((o) => ({ origin: o, usage: o === 'https://example.com' ? 52000 : 0, breakdown: o === 'https://example.com' ? [['indexeddb', 50000], ['local_storage', 2000]] : [] })) });
    h.storageCookies = (m) => ({ cookies: d.cookies[m.site] || [] });
    h.storageDeleteCookie = () => ({ deleted: 1 });
    h.storageDeleteSite = () => ({ deleted: 2 });
    h.storageDeleteAllCookies = () => ({ deleted: 3 });
    h.storageClearOrigin = () => null;
  }, data);
  await page.waitForSelector('.site');
  check((await page.locator('.site').count()) === 3, 'lists 3 sites');
  check((await page.locator('#summary').textContent()).includes('3 sites, 3 cookies'), 'summary counts sites and cookies');
  await page.waitForFunction(() => document.querySelector('.site[data-site="example.com"] .count').textContent.includes('KB'));
  check((await page.locator('.site[data-site="example.com"] .count').textContent()).includes('2 cookies · 50.8 KB'), 'site row shows cookies and storage size');
  await page.locator('.site[data-site="example.com"] > .row').click();
  await page.waitForSelector('.site.open tr[data-name="session"]');
  const valueCell = page.locator('tr[data-name="session"] td.value');
  check((await valueCell.textContent()).includes('…Show'), 'long value truncated with Show');
  await valueCell.locator('button.show').click();
  check((await valueCell.textContent()).startsWith(long), 'Show reveals the full value');
  check((await page.locator('tr[data-name="session"] .flags').textContent()) === 'Secure · HttpOnly · SameSite Lax', 'flags column');
  check((await page.locator('tr[data-name="theme"] td').nth(4).textContent()) !== 'Session', 'expiry date shown');
  check((await page.locator('.site.open .origin').count()) === 2, 'site data rows for both origins');
  check((await page.locator('.site.open .origin').first().textContent()).includes('IndexedDB'), 'usage breakdown shown');
  await shot(page, 'storage.png');
  await page.locator('tr[data-name="theme"] .icon-btn').click();
  const del = await lastSent(page, 'storageDeleteCookie');
  check(del && del.cookie.name === 'theme' && del.cookie.domain === 'www.example.com' && del.cookie.path === '/', 'delete one cookie sends its name, domain and path');
  await page.locator('.site.open .origin button').first().click();
  await acceptInPage(page);
  check((await lastSent(page, 'storageClearOrigin')).origin === 'https://example.com', 'clear site data of an origin');
  await page.locator('.site.open .site-actions button').click();
  await acceptInPage(page);
  check((await lastSent(page, 'storageDeleteSite')).site === 'example.com', 'delete all cookies of a site');
  await page.click('#delete-all');
  await acceptInPage(page);
  check((await lastSent(page, 'storageDeleteAllCookies')) !== undefined, 'delete all cookies');
  await page.fill('#search', 'tracker');
  await page.waitForTimeout(200);
  check((await page.locator('.site').count()) === 1, 'search filters sites');
  check(external.length === 0, 'no network requests');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testSiteData(browser) {
  console.log('site data page');
  const state = {
    origin: 'https://www.example.com', private: true, live: true,
    cookies: [{ name: 'sid', value: 'abc', domain: '.example.com', path: '/', expires: null, secure: true, httpOnly: false, sameSite: 'strict' }],
    local: [['cart', '[1,2,3]'], ['theme', 'dark']],
    session: [['step', '2']],
  };
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/sitedata.html', { width: 1000, height: 700 }, 1, (d) => {
    const h = window.__host.handlers;
    h.siteDataGet = () => d;
    h.siteDataSet = () => null;
    h.siteDataRemove = () => null;
    h.siteDataClear = () => null;
    h.siteCookiesDelete = () => ({ deleted: 1 });
  }, state);
  await page.waitForSelector('.kv');
  check((await page.locator('#origin').textContent()) === 'https://www.example.com', 'shows the bound origin');
  check(await page.locator('#private').isVisible(), 'private tab marked');
  check((await page.locator('#local .kv').count()) === 2 && (await page.locator('#session .kv').count()) === 1, 'local and session storage items');
  check((await page.locator('#cookies tr').count()) === 1, 'site cookies');
  await shot(page, 'sitedata.png');
  // Edit an item.
  await page.locator('#local .kv[data-key="theme"] .icon-btn').first().click();
  await page.fill('#edit-value', 'light');
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  const set = await lastSent(page, 'siteDataSet');
  check(set && set.area === 'local' && set.key === 'theme' && set.value === 'light' && Object.keys(set).length === 5, 'edit sends siteDataSet{area,key,value} only (got ' + JSON.stringify(set) + ')');
  // Add an item.
  await page.locator('h2 button.add[data-area="session"]').click();
  await page.fill('#edit-key', 'newkey');
  await page.fill('#edit-value', 'v');
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  const add = await lastSent(page, 'siteDataSet');
  check(add.area === 'session' && add.key === 'newkey', 'add item to session storage');
  // Delete an item and a cookie.
  await page.locator('#local .kv[data-key="cart"] .icon-btn').nth(1).click();
  const rm = await lastSent(page, 'siteDataRemove');
  check(rm && rm.area === 'local' && rm.key === 'cart', 'delete sends siteDataRemove');
  await page.locator('#cookies tr[data-name="sid"] .icon-btn').click();
  check((await lastSent(page, 'siteCookiesDelete')).cookie.name === 'sid', 'delete cookie');
  await page.locator('h2 button.clear[data-area="local"]').click();
  await acceptInPage(page);
  check((await lastSent(page, 'siteDataClear')).area === 'local', 'clear local storage');
  check(!(await sent(page)).some((m) => 'origin' in m || 'tabId' in m), 'no request names an origin or tab');
  // A page not opened from a site (no binding) shows the host's error.
  await page.evaluate(() => { const get = window.__host.handlers.siteDataGet; window.__host.handlers.siteDataGet = () => Object.assign({}, get(), { live: false }); });
  await page.click('#reload');
  await page.waitForTimeout(50);
  check(await page.locator('#gone').isVisible(), 'warns when the source tab left the site');
  check(external.length === 0, 'no network requests');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testShellM4(browser) {
  console.log('shell: bookmarks bar, find, zoom');
  const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 104 }, 1);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [
      { id: 1, title: 'The Rust Programming Language', url: 'https://doc.rust-lang.org/book/', favicon: null, loading: false, private: false, sleeping: false },
      { id: 2, title: 'Hacker News', url: 'https://news.ycombinator.com/', favicon: null, loading: false, private: false, sleeping: false },
    ];
    h.state.active = 1;
    h.tabsEvent();
    h.navEvent({ zoom: 1.25, bookmarked: true });
    h.emit({ event: 'shellConfig', showHomeButton: true, showBookmarksBar: true, theme: 'system' });
    h.emit({ event: 'bookmarksBar', items: [
      { id: 10, title: 'Rust', url: 'https://www.rust-lang.org/', isFolder: false, favicon: '/favicon?host=www.rust-lang.org' },
      { id: 11, title: 'News', url: null, isFolder: true, favicon: null },
      { id: 16, title: 'Dev', url: null, isFolder: true, favicon: null },
      { id: 19, title: 'Wikipedia', url: 'https://en.wikipedia.org/', isFolder: false, favicon: '/favicon?host=en.wikipedia.org' },
    ] });
  });
  await page.waitForSelector('.bm');
  check(await page.locator('#bookmarks-bar').isVisible(), 'bookmarks bar visible');
  check((await page.locator('.bm').count()) === 4, 'bar shows 4 items');
  check(await page.locator('#zoom').textContent() === '125%' && await page.locator('#zoom').isVisible(), 'zoom chip shows 125%');
  check(await page.locator('#star.bookmarked').count() === 1, 'star filled for a bookmarked page');
  await page.click('#zoom');
  check((await lastSent(page, 'zoom')).action === 'reset', 'zoom chip resets zoom');
  await page.locator('.bm', { hasText: 'Rust' }).click();
  const ou = await lastSent(page, 'openUrl');
  check(ou && ou.url === 'https://www.rust-lang.org/' && ou.newTab === false, 'bar bookmark sends openUrl');
  await page.locator('.bm', { hasText: 'Wikipedia' }).click({ button: 'middle' });
  check((await lastSent(page, 'openUrl')).newTab === true, 'middle click opens in a new tab');
  await page.locator('.bm', { hasText: 'News' }).click();
  const sf = await lastSent(page, 'showBookmarkFolder');
  check(sf && sf.id === 11 && sf.rect.y >= 76 && sf.rect.width > 10, 'folder sends showBookmarkFolder{id, rect under the button}');
  check(await page.locator('.bm.on[data-folder="11"]').count() === 1, 'open folder highlighted');
  await emit(page, { event: 'popupClosed' });
  await page.waitForTimeout(20);
  await page.waitForTimeout(150);
  await shot(page, 'shell-bookmarks-bar.png');
  // Downloads button.
  check(await page.locator('#downloads').isHidden(), 'downloads button hidden before any download');
  await emit(page, { event: 'downloadsChanged', active: 2 });
  await page.waitForTimeout(20);
  check(await page.locator('#downloads').isVisible() && await page.locator('#downloads-count').textContent() === '2', 'downloads button shows 2 active');
  await page.click('#downloads');
  const dc = await lastSent(page, 'command');
  check(dc && dc.command.command === 'downloads', 'downloads button opens the downloads page');
  // Find bar.
  await emit(page, { event: 'showFind' });
  await page.waitForSelector('#findbar:not([hidden])');
  check(await page.evaluate(() => document.activeElement.id) === 'find', 'find input focused');
  await page.keyboard.type('borrow');
  await page.waitForTimeout(250);
  const f = await lastSent(page, 'find');
  check(f && f.query === 'borrow' && f.backwards === false, 'typing sends find{query}');
  await emit(page, { event: 'findResult', active: 3, total: 12 });
  await page.waitForTimeout(20);
  check(await page.locator('#find-count').textContent() === '3/12', 'shows 3/12');
  await page.keyboard.press('Shift+Enter');
  check((await lastSent(page, 'find')).backwards === true, 'Shift+Enter searches backwards');
  await shot(page, 'shell-find-bar.png');
  await emit(page, { event: 'findResult', active: 0, total: 0 });
  await page.waitForTimeout(20);
  check(await page.locator('#findbar.nomatch').count() === 1, 'no matches shown in red');
  await page.keyboard.press('Escape');
  check((await lastSent(page, 'findClose')) !== undefined && await page.locator('#findbar').isHidden(), 'Escape closes find');
  check(external.length === 0, 'shell makes no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testShieldAndHttpsOnly(browser) {
  console.log('shield popover and HTTPS-only interstitial');
  // Shell: the counter as the host pushes it while a news page loads.
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 76 }, 1);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [{ id: 1, title: 'World news – The Daily Example', url: 'https://news.example.com/world', favicon: null, loading: false, private: false, sleeping: false }];
    h.state.active = 1;
    h.tabsEvent();
  });
  for (const n of [3, 17, 42]) {
    await page.evaluate((n) => window.__host.navEvent({ blocked: n, url: 'https://news.example.com/world' }), n);
    await page.waitForTimeout(20);
  }
  check(await page.locator('#shield-count').textContent() === '42', 'shield badge shows the latest count (42)');
  await page.evaluate(() => window.__host.navEvent({ blocked: 0, siteAllowed: true, url: 'https://news.example.com/world' }));
  await page.waitForTimeout(20);
  check(await page.locator('#shield.allowed').count() === 1 && await page.locator('#shield-count').isHidden(), 'allowed site: grey shield, no badge');
  check(errors.length === 0, 'no console errors');

  // Popup: shield popover with a count, then the allowed state.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 300, height: 226 }, 1));
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 42, allowed: false, enabled: true } });
  await page.waitForSelector('.panel');
  check((await page.locator('.big').textContent()) === '42', 'popover shows 42 blocked');
  check((await page.textContent('.origin')) === 'news.example.com', 'popover shows the site');
  await shot(page, 'popup-shield-count.png');
  await page.locator('button', { hasText: 'Allow ads on this site' }).click();
  const al = await lastSent(page, 'shieldAllowSite');
  check(al && al.allow === true, '"Allow ads on this site" sends shieldAllowSite{allow:true}');
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 0, allowed: true, enabled: true } });
  await page.waitForTimeout(30);
  await page.locator('button', { hasText: 'Block on this site again' }).click();
  check((await lastSent(page, 'shieldAllowSite')).allow === false, 'toggle back sends allow:false');

  // HTTPS-only interstitial.
  const target = 'http://legacy.example.org/page?id=7';
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/https-only.html?url=' + encodeURIComponent(target), { width: 900, height: 520 }, 1));
  await page.waitForSelector('#continue');
  check((await page.textContent('#host')) === 'legacy.example.org', 'interstitial names the host');
  await shot(page, 'https-only-interstitial.png');
  await page.click('#continue');
  const go = await lastSent(page, 'httpsOnlyProceed');
  check(go && go.url === target, 'continue sends httpsOnlyProceed{url}');
  ({ page } = await openPage(browser, 'https://krunk.invalid/https-only.html?url=' + encodeURIComponent('javascript:alert(1)'), { width: 900, height: 520 }, 1));
  check(await page.locator('#continue').isDisabled(), 'continue disabled for a non-http URL');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testPasswords(browser) {
  console.log('passwords page');
  const vaultMock = (st) => {
    const h = window.__host.handlers;
    window.__vault = st;
    h.vaultStatus = () => ({ exists: st.exists, locked: st.locked, busy: false, waitMs: st.waitMs });
    h.vaultSetup = (m) => { if (m.password.length < 8) throw new Error('too short'); st.exists = true; st.locked = false; return null; };
    h.vaultUnlock = (m) => {
      if (m.password === 'correct horse battery') { st.locked = false; st.waitMs = 0; return null; }
      st.failures = (st.failures || 0) + 1;
      st.waitMs = 1000 * Math.pow(2, st.failures - 1);
      throw new Error('Wrong master password. Try again in ' + st.waitMs / 1000 + ' s.');
    };
    h.vaultList = () => st.entries;
    h.getNeverSave = () => ['https://ads.example'];
    h.vaultReveal = (m) => { if (m.password !== 'correct horse battery') throw new Error('Wrong master password.'); return m.id === 'a2' ? { password: 'mail-pw', note: 'Recovery code 4417' } : { password: 'Tr0ub4dor&3', note: '' }; };
    h.generatePassword = () => ({ password: 'k7#Vq2!mZp9@Lr4$Wx8e' });
  };
  const entries = [
    { id: 'a1', origin: 'https://github.com', username: 'octocat', hasNote: false, updated: 1 },
    { id: 'a2', origin: 'https://mail.example.org', username: 'alice@example.org', hasNote: true, updated: 2 },
    { id: 'a3', origin: 'https://bank.example:8443', username: 'alice', hasNote: false, updated: 3 },
  ];

  // Setup.
  let { page, errors, external } = await openPage(browser, 'https://krunk.invalid/passwords.html', { width: 1000, height: 560 }, 1, vaultMock, { exists: false, locked: true, waitMs: 0, entries });
  await page.waitForSelector('#setup:not([hidden])');
  await page.fill('#setup-pw', 'correct horse battery');
  await page.fill('#setup-pw2', 'correct horse battery!');
  await page.click('#setup-btn');
  check((await page.textContent('#setup-error')).includes('do not match'), 'setup: mismatch caught in the page');
  check(!(await sent(page)).some((m) => m.cmd === 'vaultSetup'), 'mismatched passwords are not sent');
  await page.fill('#setup-pw2', 'correct horse battery');
  check((await page.textContent('#setup-error')) === '', 'error cleared while typing');
  await page.waitForTimeout(300);
  await shot(page, 'vault-setup.png');
  await page.click('#setup-btn');
  await page.waitForSelector('#list-section:not([hidden])');
  const setup = await lastSent(page, 'vaultSetup');
  check(setup && setup.password === 'correct horse battery', 'setup sends vaultSetup{password}');
  check(await page.inputValue('#setup-pw') === '', 'setup field cleared afterwards');

  // Unlock + rate limit.
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/passwords.html', { width: 1000, height: 420 }, 1, vaultMock, { exists: true, locked: true, waitMs: 0, entries }));
  await page.waitForSelector('#unlock:not([hidden])');
  await shot(page, 'vault-unlock.png');
  await page.fill('#unlock-pw', 'wrong');
  await page.click('#unlock-btn');
  await page.waitForTimeout(80);
  check((await page.textContent('#unlock-error')).includes('Try again in'), 'wrong password shows the retry delay');
  await page.waitForTimeout(1200);
  await page.fill('#unlock-pw', 'wrong again');
  await page.click('#unlock-btn');
  await page.waitForTimeout(80);
  check(await page.locator('#unlock-btn').isDisabled(), 'unlock disabled during the delay');
  check((await page.textContent('#unlock-error')).includes('2 s'), 'second failure: 2 s');
  check(await page.inputValue('#unlock-pw') === '', 'password field cleared after an attempt');
  await shot(page, 'vault-rate-limited.png');
  await page.waitForTimeout(2300);
  await page.fill('#unlock-pw', 'correct horse battery');
  await page.click('#unlock-btn');
  await page.waitForSelector('#list-section:not([hidden])');
  check(true, 'correct password unlocks');

  // List.
  await page.setViewportSize({ width: 1000, height: 560 });
  await page.waitForSelector('.pw-row');
  check((await page.locator('.pw-row').count()) === 3, 'lists 3 logins');
  check(!(await page.content()).includes('Tr0ub4dor'), 'no password in the page before reveal');
  await page.waitForTimeout(100);
  await shot(page, 'passwords-list.png');
  await page.locator('.pw-row[data-id="a1"]').hover();
  await page.locator('.pw-row[data-id="a1"] button', { hasText: 'Copy password' }).click();
  const cp = await lastSent(page, 'vaultCopy');
  check(cp && cp.id === 'a1' && cp.field === 'password', 'copy sends vaultCopy{password}');
  check((await page.locator('.toast').textContent()).includes('30 s'), 'copy toast mentions the 30 s clear');
  await page.locator('.pw-row[data-id="a1"] button', { hasText: 'Show' }).click();
  await page.fill('#reveal-pw', 'nope');
  await page.locator('#reveal-dialog button[value="ok"]').click();
  await page.waitForTimeout(60);
  check((await page.textContent('#reveal-error')).includes('Wrong'), 'reveal with a wrong master password fails');
  // Enter in the field confirms (the Cancel button must not be the form's default).
  await page.fill('#reveal-pw', 'correct horse battery');
  await page.press('#reveal-pw', 'Enter');
  await page.waitForTimeout(60);
  check((await page.textContent('.pw-row[data-id="a1"] .secret')) === 'Tr0ub4dor&3', 'reveal (Enter) shows the password after re-entering the master password');
  const rv = await lastSent(page, 'vaultReveal');
  check(rv && rv.id === 'a1' && rv.password === 'correct horse battery', 'reveal sends the master password with the entry id');
  check(await page.locator('.pw-row[data-id="a1"] .note').isHidden(), 'an entry without a note shows no note after reveal');
  // An entry with a note: the note is shown with the password after re-authentication only.
  check(await page.locator('.pw-row[data-id="a2"] .note').isHidden(), 'note hidden before reveal');
  await page.locator('.pw-row[data-id="a2"]').hover();
  await page.locator('.pw-row[data-id="a2"] button', { hasText: 'Show' }).click();
  await page.fill('#reveal-pw', 'correct horse battery');
  await page.press('#reveal-pw', 'Enter');
  await page.waitForTimeout(60);
  check((await page.textContent('.pw-row[data-id="a2"] .note')) === 'Note: Recovery code 4417', 'reveal shows the note too');
  // Cancel closes the dialog without a request.
  const revealsBefore = (await sent(page)).filter((m) => m.cmd === 'vaultReveal').length;
  await page.locator('.pw-row[data-id="a1"] button', { hasText: 'Show' }).click();
  await page.fill('#reveal-pw', 'typed then cancelled');
  await page.locator('#reveal-dialog button[value="cancel"]').click();
  await page.waitForTimeout(60);
  check(!(await page.locator('#reveal-dialog').evaluate((d) => d.open)) &&
    (await sent(page)).filter((m) => m.cmd === 'vaultReveal').length === revealsBefore, 'reveal Cancel closes without sending');
  check(await page.evaluate(() => Array.from(document.querySelectorAll('dialog form[method="dialog"]')).every((f) => {
    const def = f.querySelector('button:not([type="button"])');
    return def && def.value === 'ok';
  })), 'every passwords dialog has the confirm button as its default (Enter)');
  await page.click('#add');
  await page.fill('#e-origin', 'news.example.com');
  await page.fill('#e-user', 'reader');
  await page.click('#e-gen');
  await page.waitForTimeout(50);
  check(await page.inputValue('#e-pw') === 'k7#Vq2!mZp9@Lr4$Wx8e', 'generate fills a strong password');
  await page.locator('#entry-dialog button[value="ok"]').click();
  await page.waitForTimeout(60);
  const sv = await lastSent(page, 'vaultSave');
  check(sv && sv.entry.origin === 'https://news.example.com' && sv.entry.username === 'reader' && sv.entry.password === 'k7#Vq2!mZp9@Lr4$Wx8e', 'add sends vaultSave with https:// origin');
  // Editing an entry that has a note: the page cannot see the note, so an
  // empty field must keep it (note: null), not overwrite it with "".
  await page.locator('.pw-row[data-id="a2"]').hover();
  await page.locator('.pw-row[data-id="a2"] .icon-btn[title="Edit"]').click();
  check(await page.getAttribute('#e-note', 'placeholder') === '(unchanged)', 'edit shows "(unchanged)" for an existing note');
  await page.fill('#e-user', 'alice.renamed@example.org');
  await page.press('#e-user', 'Enter');
  await page.waitForTimeout(60);
  const ed = await lastSent(page, 'vaultSave');
  check(ed && ed.entry.id === 'a2' && ed.entry.note === null && ed.entry.password === null, 'edit keeps the stored note and password (null) (got ' + JSON.stringify(ed && ed.entry) + ')');
  await page.click('#change');
  await page.fill('#c-old', 'correct horse battery');
  await page.fill('#c-new', 'new master pass 1');
  await page.fill('#c-new2', 'new master pass 2');
  await page.locator('#change-dialog button[value="ok"]').click();
  check((await page.textContent('#c-error')).includes('do not match'), 'change: mismatch caught');
  await page.fill('#c-new', 'new master pass 1');
  await page.fill('#c-new2', 'new master pass 1');
  await page.locator('#change-dialog button[value="ok"]').click();
  const ch = await lastSent(page, 'vaultChangePassword');
  check(ch && ch.old === 'correct horse battery' && ch.new === 'new master pass 1', 'change sends old and new');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');

  // Popup unlock prompt + save prompt.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 360, height: 200 }, 1));
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://bank.example', purpose: 'fill', error: null, waitMs: 0 } });
  await page.waitForSelector('#master');
  check(await page.evaluate(() => document.activeElement.id) === 'master', 'unlock prompt focuses the password field');
  await page.fill('#master', 'correct horse battery');
  await shot(page, 'popup-unlock.png');
  await page.click('#unlock');
  const un = await lastSent(page, 'vaultUnlock');
  check(un && un.password === 'correct horse battery', 'unlock prompt sends vaultUnlock');
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://bank.example', purpose: 'fill', error: 'Wrong master password. Try again in 4 s.', waitMs: 4000 } });
  await page.waitForTimeout(60);
  check(await page.locator('#unlock').isDisabled(), 'unlock prompt disabled during the host-side delay');
  await shot(page, 'popup-unlock-rate-limited.png');
  await page.setViewportSize({ width: 360, height: 150 });
  await emit(page, { event: 'popup', content: { kind: 'savePassword', tabId: 4, origin: 'https://bank.example', username: 'alice', update: false } });
  await page.waitForTimeout(60);
  await shot(page, 'popup-save-password.png');
  await page.locator('button', { hasText: 'Never for this site' }).click();
  const nv = await lastSent(page, 'savePasswordReply');
  check(nv && nv.never === true && nv.accept === false, 'Never sends never:true');
  await page.setViewportSize({ width: 340, height: 150 });
  await emit(page, { event: 'popup', content: { kind: 'fill', tabId: 4, origin: 'https://bank.example', entries: [] } });
  await page.waitForTimeout(60);
  check((await page.textContent('h2')) === 'No saved logins', 'fill prompt with no entries');
  check(errors.length === 0, 'popup: no console errors (' + errors.join(' | ') + ')');

  // Shell key icon.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1000, height: 76 }, 1));
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [{ id: 4, title: 'Sign in – Example Bank', url: 'https://bank.example/login', favicon: null, loading: false, private: false, sleeping: false }];
    h.state.active = 4;
    h.tabsEvent();
    h.navEvent({ loginForm: false });
  });
  await page.waitForTimeout(40);
  check(await page.locator('#key').isHidden(), 'no key icon without a login form');
  await page.evaluate(() => window.__host.navEvent({ loginForm: true }));
  await page.waitForTimeout(40);
  check(await page.locator('#key').isVisible(), 'key icon shown for a login form');
  await page.click('#key');
  check((await lastSent(page, 'requestFill')) !== undefined, 'key icon sends requestFill');
  check(external.length === 0, 'no network requests');
}

// Mock of the settings host: simplified versions of the core rules (the real
// ones are unit-tested in core/src/settings.rs).
function settingsMock(defaults) {
  const h = window.__host.handlers;
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const running = clone(defaults);
  const st = { settings: clone(defaults), allow: ['news.example.com'] };
  window.__settings = st;
  const restart = (s) => ['hardwareAcceleration', 'proxy', 'extraFlags', 'blockThirdPartyCookies', 'engineSecurityComponents'].some((k) => s[k] !== running[k]);
  h.getSettings = () => ({ settings: st.settings, restartRequired: restart(st.settings), about: { version: '0.1.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
  h.saveSettings = (m) => {
    const s = m.settings;
    const errs = [];
    if (s.proxy && !/^((https?|socks[45]):\/\/)?[A-Za-z0-9.-]+:\d{1,5}$/.test(s.proxy)) errs.push({ field: 'proxy', message: 'Use host:port, scheme://host:port (http, https, socks4, socks5) or direct://' });
    if (s.searchEngine === 'custom' && !/%s|\{searchTerms\}/.test(s.customSearchUrl)) errs.push({ field: 'customSearchUrl', message: 'Must contain %s where the search terms go' });
    if (s.extraFlags.split(/\s+/).some((t) => t && !/^--[A-Za-z0-9][\w.-]*(=[^\s"'`;&|<>]+)?$/.test(t))) errs.push({ field: 'extraFlags', message: 'Not a valid flag (use --flag or --flag=value)' });
    if (s.tabSleepMinutes > 1440) errs.push({ field: 'tabSleepMinutes', message: 'Must be between 0 and 1440 minutes' });
    if (errs.length) { const e = new Error('Some settings are not valid'); e.data = errs; throw e; }
    st.settings = s;
    return { settings: s, restartRequired: restart(s) };
  };
  h.resetSettings = () => { st.settings = clone(defaults); return { settings: st.settings, restartRequired: false }; };
  h.getFilterStatus = () => ({ enabled: true, lastCheck: Date.now() - 3 * 3600e3, lastError: null, busy: false, lists: [
    { id: 'easylist', title: 'EasyList (ads)', enabled: true, source: 'downloaded', fetched: Date.now() - 3 * 3600e3 },
    { id: 'easyprivacy', title: 'EasyPrivacy (trackers)', enabled: true, source: 'downloaded', fetched: Date.now() - 3 * 3600e3 },
    { id: 'ublock-filters', title: 'uBlock filters', enabled: true, source: 'embedded', fetched: null },
    { id: 'ublock-privacy', title: 'uBlock filters – Privacy', enabled: true, source: 'embedded', fetched: null },
    { id: 'ublock-unbreak', title: 'uBlock filters – Unbreak', enabled: true, source: 'embedded', fetched: null },
  ] });
  h.updateFilterLists = () => h.getFilterStatus();
  h.getAllowlist = () => st.allow;
  h.allowlistAdd = (m) => { if (!/^[a-z0-9.-]+\.[a-z]{2,}$/i.test(m.host)) throw new Error('Not a valid host name'); st.allow.push(m.host.toLowerCase()); return m.host; };
  h.allowlistRemove = (m) => { st.allow = st.allow.filter((x) => x !== m.host); return true; };
  h.pickDownloadFolder = () => 'D:\\Downloads\\Browser';
}

async function testSettings(browser) {
  console.log('settings page');
  const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html', { width: 1000, height: 2900 }, 1, settingsMock, defaults);
  await page.waitForSelector('#lists .row');
  check(await page.inputValue('#searchEngine') === 'duckDuckGo', 'defaults: DuckDuckGo');
  check(await page.isChecked('input[name=startup][value=restoreSession]'), 'defaults: restore session');
  check((await page.locator('#trackingPrevention, [data-key=smartscreen]').count()) === 0, 'no Edge-only settings');
  check(!(await page.isChecked('[data-key=engineSecurityComponents]')), 'defaults: no engine downloads from Google');
  check((await page.textContent('#about')).includes('Chromium 156.0.8078.80 (CEF 156.0.15)'), 'engine versions in the footer');
  check(await page.isChecked('[data-key=hardwareAcceleration]'), 'defaults: GPU on');
  check(await page.isChecked('[data-key=blockThirdPartyCookies]'), 'defaults: third-party cookies blocked');
  check(await page.locator('#restart-banner').isHidden(), 'no restart banner at first');
  check((await page.locator('#lists .row').count()) === 5, '5 filter lists with checkboxes');
  await page.waitForTimeout(100);
  await shot(page, 'settings-light.png');

  // Inline validation: invalid proxy is rejected and shown, nothing stored.
  await page.fill('#proxy', 'not a proxy');
  await page.press('#proxy', 'Tab');
  await page.waitForTimeout(80);
  check((await page.textContent('[data-error-for=proxy]')).includes('host:port'), 'invalid proxy: inline error');
  check(await page.locator('#proxy.invalid').count() === 1, 'invalid proxy: field marked');
  check(await page.evaluate(() => window.__settings.settings.proxy) === '', 'invalid proxy not persisted');
  check(await page.locator('#restart-banner').isHidden(), 'no restart banner for a rejected value');
  await page.locator('#proxy').scrollIntoViewIfNeeded();
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, 'settings-validation-error.png'), clip: await (async () => { const b = await page.locator('section[data-section^=advanced]').boundingBox(); return { x: 0, y: b.y - 10, width: 1000, height: b.height + 20 }; })() });
  await page.fill('#proxy', '127.0.0.1:8080');
  await page.press('#proxy', 'Tab');
  await page.waitForTimeout(80);
  check((await page.textContent('[data-error-for=proxy]')) === '', 'valid proxy clears the error');
  check(await page.evaluate(() => window.__settings.settings.proxy) === '127.0.0.1:8080', 'valid proxy persisted');
  check(await page.locator('#restart-banner').isVisible(), 'restart banner after changing the proxy');

  // Custom search without %s; extra flags.
  await page.selectOption('#searchEngine', 'custom');
  await page.waitForTimeout(60);
  check(await page.locator('#custom-row').isVisible(), 'custom search URL field shown');
  await page.fill('#customSearchUrl', 'https://search.example/?q=');
  await page.press('#customSearchUrl', 'Tab');
  await page.waitForTimeout(60);
  check((await page.textContent('[data-error-for=customSearchUrl]')).includes('%s'), 'custom search needs %s');
  await page.fill('#customSearchUrl', 'https://search.example/?q=%s');
  await page.press('#customSearchUrl', 'Tab');
  await page.waitForTimeout(60);
  check(await page.evaluate(() => window.__settings.settings.customSearchUrl) === 'https://search.example/?q=%s', 'custom search saved');
  await page.fill('#extraFlags', '--force-dark-mode "--x"');
  await page.press('#extraFlags', 'Tab');
  await page.waitForTimeout(60);
  check((await page.textContent('[data-error-for=extraFlags]')).length > 0, 'malformed extra flag rejected inline');
  await page.fill('#extraFlags', '');
  await page.press('#extraFlags', 'Tab');
  await page.fill('#tabSleepMinutes', '');
  await page.press('#tabSleepMinutes', 'Tab');
  await page.waitForTimeout(60);
  check((await page.textContent('[data-error-for=tabSleepMinutes]')).includes('1440'), 'empty sleep time gets a range error');
  await page.fill('#tabSleepMinutes', '10');
  await page.press('#tabSleepMinutes', 'Tab');

  // A slow save reply for one field must not wipe what the user is typing in another.
  await page.evaluate(() => {
    const h = window.__host.handlers; const orig = h.saveSettings;
    h.saveSettings = (m) => { h.saveSettings = orig; return new Promise((res) => setTimeout(() => res(orig(m)), 200)); };
  });
  await page.fill('#userAgent', 'Slow/1.0');
  await page.press('#userAgent', 'Tab');
  await page.fill('#tabSleepMinutes', '7');
  await page.waitForTimeout(350);
  check(await page.inputValue('#tabSleepMinutes') === '7', 'a save reply does not overwrite the field being edited');
  await page.press('#tabSleepMinutes', 'Tab');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.tabSleepMinutes === 7, 'the edited field is then saved');
  await page.fill('#userAgent', '');
  await page.press('#userAgent', 'Tab');
  await page.fill('#tabSleepMinutes', '5');
  await page.press('#tabSleepMinutes', 'Tab');
  await page.waitForTimeout(60);
  // Third-party cookie blocking is restart-only too, and is saved.
  await page.uncheck('[data-key=blockThirdPartyCookies]');
  await page.waitForTimeout(60);
  const tp = await lastSent(page, 'saveSettings');
  check(tp && tp.settings.blockThirdPartyCookies === false && await page.locator('#restart-banner').isVisible(), 'third-party cookie toggle saves and shows the restart banner');
  await page.check('[data-key=blockThirdPartyCookies]');
  await page.waitForTimeout(60);
  // Engine security components: opt-in, restart-only.
  await page.check('[data-key=engineSecurityComponents]');
  await page.waitForTimeout(60);
  const ec = await lastSent(page, 'saveSettings');
  check(ec && ec.settings.engineSecurityComponents === true && await page.locator('#restart-banner').isVisible(), 'engine components toggle saves and shows the restart banner');
  await page.uncheck('[data-key=engineSecurityComponents]');
  await page.waitForTimeout(60);
  // Restart badge for the GPU toggle (restart-only) and restart button.
  await page.uncheck('[data-key=hardwareAcceleration]');
  await page.waitForTimeout(60);
  check(await page.locator('#restart-banner').isVisible(), 'GPU toggle shows the restart banner');
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(50);
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, 'settings-restart-banner.png'), clip: { x: 0, y: 0, width: 1000, height: 420 } });
  await page.click('#restart');
  check((await lastSent(page, 'restart')) !== undefined, 'Restart now sends restart');

  // Allowlist, folder picker, update now.
  await page.fill('#allow-host', 'bad host');
  await page.click('#allow-form button');
  await page.waitForTimeout(50);
  check((await page.textContent('#allow-error')).includes('valid host'), 'allowlist validation error');
  await page.fill('#allow-host', 'shop.example.org');
  await page.click('#allow-form button');
  await page.waitForTimeout(50);
  check((await page.locator('#allowlist .row').count()) === 2, 'allowlist entry added');
  await page.click('#pick-dir');
  await page.waitForTimeout(80);
  check((await page.textContent('#download-dir')) === 'D:\\Downloads\\Browser', 'download folder from the native picker');
  await page.click('#update-lists');
  check((await lastSent(page, 'updateFilterLists')) !== undefined, 'Update now sends updateFilterLists');

  // Theme switch (live) and a dark screenshot.
  await page.check('input[name=theme][value=dark]');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => document.documentElement.classList.contains('dark')), 'dark theme applied live');
  check(await page.evaluate(() => window.__settings.settings.theme) === 'dark', 'theme saved');
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(100);
  await shot(page, 'settings-dark.png');
  await emit(page, { event: 'theme', theme: 'light' });
  await page.waitForTimeout(30);
  check(await page.evaluate(() => document.documentElement.classList.contains('light')), 'host theme event switches to light');

  // Reset (confirm is accepted by the harness).
  // Cancel first: nothing happens.
  await page.click('#reset');
  await page.waitForSelector('#lb-dialog-cancel');
  check((await page.textContent('.lb-dialog-text')).includes('Reset all settings'), 'in-page reset confirmation');
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await page.waitForTimeout(50);
  await shot(page, 'settings-reset-confirm.png');
  await page.click('#lb-dialog-cancel');
  await page.waitForTimeout(50);
  check((await lastSent(page, 'resetSettings')) === undefined, 'cancel does not reset');
  await page.click('#reset');
  await acceptInPage(page);
  await page.waitForTimeout(80);
  check((await lastSent(page, 'resetSettings')) !== undefined, 'reset sends resetSettings after confirm');
  check(await page.isChecked('[data-key=hardwareAcceleration]') && await page.inputValue('#proxy') === '', 'reset restores defaults');
  check(await page.locator('#restart-banner').isHidden(), 'banner gone after reset');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testFolderPopup(browser) {
  console.log('popup: bookmark folder');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 280, height: 110 }, 1);
  await emit(page, { event: 'popup', content: { kind: 'bookmarkFolder', id: 11, title: 'News', items: [
    { id: 12, title: 'BBC News', url: 'https://www.bbc.co.uk/news', isFolder: false, favicon: '/favicon?host=www.bbc.co.uk' },
    { id: 13, title: 'Hacker News', url: 'https://news.ycombinator.com/', isFolder: false, favicon: null },
    { id: 14, title: 'Tech', url: null, isFolder: true, favicon: null },
  ] } });
  await page.waitForSelector('.item');
  await page.waitForTimeout(100);
  await shot(page, 'popup-bookmark-folder.png');
  await page.locator('.item', { hasText: 'Tech' }).click();
  const sf = await lastSent(page, 'showBookmarkFolder');
  check(sf && sf.id === 14 && sf.rect === undefined, 'subfolder opens in place (no rect)');
  await page.locator('.item', { hasText: 'BBC News' }).click();
  const ou = await lastSent(page, 'openUrl');
  check(ou && ou.url === 'https://www.bbc.co.uk/news', 'bookmark opens url');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// The injected login script on an ordinary web page. It runs at document
// creation (like AddScriptToExecuteOnDocumentCreated), must not throw there,
// and must notice a login form that a script adds after load.
async function testLoginScript(browser) {
  console.log('injected login script');
  const context = await browser.newContext();
  const external = [];
  await context.route('**/*', (r) => { external.push(r.request().url()); return r.abort(); });
  await context.route('https://login.example/**', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><title>SPA</title><body><div id="app">loading</div></body>' }));
  // Minimal __lbHost stand-in (the renderer bridge) that records what the script posts.
  await context.addInitScript(() => {
    window.__posted = [];
    window.__lbHost = { postMessage: (m) => window.__posted.push(m), addEventListener: () => {} };
  });
  await context.addInitScript({ path: path.join(__dirname, '..', 'inject', 'login.js') });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://login.example/');
  await page.waitForTimeout(100);
  check(!(await page.evaluate(() => window.__posted.some((m) => m.lb === 'loginForm'))), 'no login form announced before one exists');
  await page.evaluate(() => {
    const f = document.createElement('form');
    f.innerHTML = '<input name="u" type="email"><input name="p" type="password"><button>Sign in</button>';
    document.getElementById('app').replaceChildren(f);
  });
  await page.waitForTimeout(700);
  check(await page.evaluate(() => window.__posted.some((m) => m.lb === 'loginForm')), 'a login form added after load is announced (loginForm)');
  check(errors.length === 0, 'login script raises no page errors (' + errors.join(' | ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await context.close();
}

(async () => {
  if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
  const browser = await chromium.launch();
  try {
    await testShell(browser);
    await testShellFrame(browser);
    await testShellHiDpi(browser);
    await testPopup(browser);
    await testNewTab(browser);
    await testHistory(browser);
    await testBookmarks(browser);
    await testDownloads(browser);
    await testStorage(browser);
    await testSiteData(browser);
    await testShellM4(browser);
    await testFolderPopup(browser);
    await testShieldAndHttpsOnly(browser);
    await testPasswords(browser);
    await testSettings(browser);
    await testLoginScript(browser);
  } finally {
    await browser.close();
  }
  console.log(`\n${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
