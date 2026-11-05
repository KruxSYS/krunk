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

// Tab strip animations (open/close, ~160 ms) finished: no growing or shrinking tab left.
async function settleTabs(page) {
  await page.waitForFunction(() => !document.querySelector('.tab.opening, .tab.closing'));
  await page.waitForTimeout(250);
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
  check(sp && sp.popup === 'menu' && sp.rect.width === 280 && sp.rect.height === 542 && sp.rect.x >= 0 && sp.rect.y > 60, 'menu sends showPopup with a rect below the toolbar');
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
// 0.4.0: closing and opening tabs animate; closing with the mouse keeps the other widths
// while the pointer stays on the strip.
async function testTabAnimations(browser) {
  console.log('tab close/open animation');
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
  const setTabs = (n, active) => page.evaluate(([n, active]) => {
    const h = window.__host;
    h.state.tabs = [];
    for (let i = 1; i <= n; i++) h.state.tabs.push({ id: i, title: 'Tab number ' + i, url: 'https://t' + i + '.example/', favicon: null, loading: false, private: false, sleeping: false });
    h.state.nextId = n + 1;
    h.state.active = active;
    h.tabsEvent();
    h.navEvent();
  }, [n, active]);
  const widths = () => page.$$eval('.tab:not(.closing)', (ns) => ns.map((n) => Math.round(n.getBoundingClientRect().width * 10) / 10));
  await setTabs(8, 1);
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 8);
  await settleTabs(page);
  const before = await widths();
  check(before[0] < 256 && before[0] > 72, '8 tabs share the row (' + before[0] + ')');

  // Close tab 3 with its close button: the host is told at once, the tab shrinks away.
  const close3 = await page.locator('.tab[data-id="3"] .close').boundingBox();
  const cx = close3.x + close3.width / 2, cy = close3.y + close3.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.click(cx, cy);
  const sentClose = await lastSent(page, 'closeTab');
  check(sentClose && sentClose.id === 3, 'the close button sends closeTab at once');
  await page.waitForSelector('.tab.closing', { timeout: 500 });
  check(await page.locator('.tab.closing').count() === 1, 'the closed tab shrinks (one closing tab in the strip)');
  check(await page.locator('.tab.closing').evaluate((n) => getComputedStyle(n).pointerEvents === 'none'), 'a closing tab takes no clicks');
  await page.waitForFunction(() => !document.querySelector('.tab.closing'), null, { timeout: 2000 });
  check(true, 'and is gone after the animation');
  await page.waitForTimeout(200);
  const frozen = await widths();
  check(frozen.length === 7 && frozen.every((w) => Math.abs(w - before[0]) < 0.6),
    'with the pointer on the strip the others keep their width (' + frozen[0] + ' = ' + before[0] + ')');
  const under = await page.evaluate(([x, y]) => {
    const n = document.elementFromPoint(x, y);
    const tab = n && n.closest('.tab');
    return { close: !!(n && n.closest('.close')), id: tab ? Number(tab.dataset.id) : null };
  }, [cx, cy]);
  check(under.close && under.id === 4, 'the next tab\'s close button is now under the pointer (' + JSON.stringify(under) + ')');

  // Quick closes at the same spot (a click every 200 ms) close the following tabs, one each.
  await page.mouse.click(cx, cy);
  await page.waitForTimeout(200);
  await page.mouse.click(cx, cy);
  await page.waitForTimeout(200);
  const closes = (await sent(page)).filter((m) => m.cmd === 'closeTab').map((m) => m.id);
  check(JSON.stringify(closes.slice(-3)) === JSON.stringify([3, 4, 5]), 'three quick clicks close tabs 3, 4, 5 (' + closes.join(',') + ')');
  await settleTabs(page);
  const lastTab = await page.locator('.tab:not(.closing)').last().boundingBox();
  const plusBox = await page.locator('#newtab').boundingBox();
  check(plusBox.x - (lastTab.x + lastTab.width) < 12, '"+" follows the last tab while widths are frozen (gap ' + Math.round(plusBox.x - lastTab.x - lastTab.width) + ')');
  await shot(page, 'shell-tabs-closed-frozen.png');

  // Pointer leaves the strip: the tabs take their new (wider) widths, and the hit map follows.
  await page.mouse.move(640, 70);
  await page.evaluate(() => document.getElementById('tabstrip').dispatchEvent(new PointerEvent('pointerleave')));
  await settleTabs(page);
  const thawed = await widths();
  check(thawed.length === 5 && thawed[0] > before[0] + 1 && Math.max(...thawed) - Math.min(...thawed) < 0.6,
    'after the pointer leaves, 5 tabs share the row again (' + thawed[0] + ' > ' + before[0] + ')');
  await page.waitForTimeout(60);
  const hm = (await lastSent(page, 'hitMap')).map;
  check(hm.tabCenters.length === 5, 'hit map: 5 tab centres once the strip settled (' + hm.tabCenters.length + ')');
  await shot(page, 'shell-tabs-thawed.png');

  // A new tab grows in.
  await page.evaluate(() => window.__host.emit({ event: 'tabs', tabs: window.__host.state.tabs.concat([{ id: 99, title: 'New tab', url: '', favicon: null, loading: false, private: false, sleeping: false }]), active: 99 }));
  check(await page.locator('.tab[data-id="99"].opening').count() === 1 || await page.locator('.tab[data-id="99"]').evaluate((n) => n.getBoundingClientRect().width < 200),
    'a new tab starts small');
  await settleTabs(page);
  check(await page.locator('.tab[data-id="99"]').evaluate((n) => !n.classList.contains('opening') && n.getBoundingClientRect().width > 72), 'and grows to its width');

  // Ctrl+W-style close (no pointer): no frozen widths.
  await page.evaluate(() => { const h = window.__host; h.state.tabs = h.state.tabs.filter((t) => t.id !== 6); h.tabsEvent(); });
  await settleTabs(page);
  const after = await widths();
  check(Math.max(...after) - Math.min(...after) < 0.6, 'a close from the keyboard re-lays the strip out at once');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.close();

  // Reduced motion: no animation, the closed tab goes at once.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1));
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [1, 2, 3].map((i) => ({ id: i, title: 'T' + i, url: 'https://t' + i + '.example/', favicon: null, loading: false, private: false, sleeping: false }));
    h.state.active = 1; h.tabsEvent();
  });
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 3);
  await page.locator('.tab[data-id="2"] .close').click();
  await page.waitForTimeout(20);
  check(await page.locator('.tab').count() === 2 && await page.locator('.tab.closing').count() === 0, 'reduced motion: the tab goes at once');
  check(errors.length === 0, 'reduced motion: no console errors (' + errors.join(' | ') + ')');
}

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
  await settleTabs(page);
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
  await settleTabs(page);
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

  // The update prompt.
  ({ page, errors } = await make(360, 150));
  const upd = { kind: 'update', version: '0.2.1', current: '0.2.0', install: true, hasNotes: true, state: { step: 'offered' } };
  await emit(page, { event: 'popup', content: upd });
  await page.waitForSelector('.panel');
  await shot(page, 'popup-update.png');
  check((await page.textContent('h2')) === 'Krunk 0.2.1 is available', 'update prompt names the version');
  check(await page.evaluate(() => document.activeElement && document.activeElement.textContent) === 'Update now', 'Update now focused');
  await page.locator('button', { hasText: 'Release notes' }).click();
  check((await lastSent(page, 'updateReply')).choice === 'notes', 'Release notes sends updateReply{notes}');
  await page.locator('button', { hasText: 'Later' }).click();
  check((await lastSent(page, 'updateReply')).choice === 'later', 'Later sends updateReply{later}');
  await page.locator('button', { hasText: 'Update now' }).click();
  const ui = await lastSent(page, 'updateReply');
  check(ui.choice === 'install' && !('url' in ui) && !('version' in ui), 'Update now sends only the choice');
  await emit(page, { event: 'popup', content: Object.assign({}, upd, { state: { step: 'downloading', percent: 42 } }) });
  await page.waitForSelector('progress');
  check(await page.evaluate(() => document.querySelector('progress').value) === 42, 'download progress shown');
  await shot(page, 'popup-update-downloading.png');
  await emit(page, { event: 'popup', content: Object.assign({}, upd, { state: { step: 'failed', error: 'the download\'s SHA-256 does not match the manifest' } }) });
  await page.waitForSelector('.error');
  check((await page.textContent('.error')).includes('SHA-256'), 'a failed update says why');
  check(await page.locator('button', { hasText: 'Try again' }).count() === 1, 'failed: Try again');
  await emit(page, { event: 'popup', content: Object.assign({}, upd, { install: false, hasNotes: false }) });
  await page.waitForTimeout(20);
  check(await page.locator('button', { hasText: 'Download' }).count() === 1 && await page.locator('button', { hasText: 'Release notes' }).count() === 0, 'portable copy: Download, no notes button without notes');
  check(errors.length === 0, 'update prompt: no console errors (' + errors.join(' | ') + ')');
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

// 0.4.0: the bookmarks bar's visibility, context menu, editor and drag.
async function testBookmarksBar(browser) {
  console.log('bookmarks bar: menu, editor, drag');
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 104 }, 1);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [{ id: 1, title: 'Rust', url: 'https://www.rust-lang.org/', favicon: null, loading: false, private: false, sleeping: false }];
    h.state.active = 1;
    h.tabsEvent();
    h.navEvent({ bookmarksBar: false });
    h.emit({ event: 'bookmarksBar', items: [
      { id: 10, title: 'Rust', url: 'https://www.rust-lang.org/', isFolder: false, favicon: null },
      { id: 11, title: 'News', url: null, isFolder: true, favicon: null },
      { id: 19, title: 'Wikipedia', url: 'https://en.wikipedia.org/', isFolder: false, favicon: null },
    ] });
  });
  await page.waitForTimeout(30);
  check(await page.locator('#bookmarks-bar').isHidden(), 'nav says no bar: hidden (never, or not a new tab)');
  await page.evaluate(() => window.__host.navEvent({ bookmarksBar: true }));
  await page.waitForSelector('#bookmarks-bar .bm');
  check(await page.locator('#bookmarks-bar').isVisible(), 'nav says bar: shown');

  // Right-click on a bookmark, on a folder, on the bar.
  await page.locator('.bm[data-id="10"]').click({ button: 'right' });
  let m = await lastSent(page, 'showBookmarkMenu');
  check(m && m.id === 10 && m.rect.x > 0, 'right-click on a bookmark: showBookmarkMenu{id:10}');
  await page.locator('.bm[data-id="11"]').click({ button: 'right' });
  m = await lastSent(page, 'showBookmarkMenu');
  check(m && m.id === 11, 'right-click on a folder: showBookmarkMenu{id:11}');
  const barBox = await page.locator('#bookmarks-bar').boundingBox();
  await page.mouse.click(barBox.x + barBox.width - 20, barBox.y + barBox.height / 2, { button: 'right' });
  m = await lastSent(page, 'showBookmarkMenu');
  check(m && m.id === null, 'right-click on empty bar space: showBookmarkMenu{id:null}');

  // A click opens the bookmark.
  await page.locator('.bm[data-id="10"]').click();
  let o = await lastSent(page, 'openUrl');
  check(o && o.url === 'https://www.rust-lang.org/', 'a click opens the bookmark');
  const opened = (await sent(page)).filter((x) => x.cmd === 'openUrl').length;

  // Drag Wikipedia to the front: moveBookmark{parent: bar, index: 0}; the drop opens nothing.
  const box = async (id) => page.locator('.bm[data-id="' + id + '"]').boundingBox();
  let from = await box(19), to = await box(10);
  await page.mouse.move(from.x + 10, from.y + 10);
  await page.mouse.down();
  await page.mouse.move(from.x - 30, from.y + 10, { steps: 4 });
  await page.mouse.move(to.x + 3, to.y + 10, { steps: 6 });
  check(await page.locator('.bm[data-id="10"].drop-before').count() === 1, 'a mark shows where the item goes');
  await shot(page, 'shell-bar-drag-reorder.png');
  await page.mouse.up();
  let mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 19 && mv.parent === 1 && mv.index === 0, 'dropped first: moveBookmark{id:19, parent:1, index:0} (' + JSON.stringify(mv) + ')');
  await page.waitForTimeout(30);
  check((await sent(page)).filter((x) => x.cmd === 'openUrl').length === opened, 'the drop does not open the bookmark');
  check(await page.locator('.bm.dragging, .bm.drop-before, .bm.drop-after').count() === 0, 'no marks left');

  // Drag Rust onto the middle of the News folder: into the folder.
  from = await box(10); to = await box(11);
  await page.mouse.move(from.x + 10, from.y + 10);
  await page.mouse.down();
  await page.mouse.move(from.x + 30, from.y + 10, { steps: 4 });
  await page.mouse.move(to.x + to.width / 2, to.y + 10, { steps: 6 });
  check(await page.locator('.bm[data-id="11"].drop-into').count() === 1, 'over the folder\'s middle: the folder lights up');
  await shot(page, 'shell-bar-drag-into-folder.png');
  await page.mouse.up();
  mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 10 && mv.parent === 11, 'dropped on the folder: moveBookmark{id:10, parent:11} (' + JSON.stringify(mv) + ')');
  check((await lastSent(page, 'showBookmarkFolder')) === undefined, 'the drop does not open the folder');
  check(errors.length === 0, 'shell: no console errors (' + errors.join(' | ') + ')');
  await page.close();

  // The popup: the three menus.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 240, height: 352 }, 1));
  await page.evaluate(() => { window.__host.autoSimulate = false; });
  const labels = () => page.$$eval('.item .label', (ns) => ns.map((n) => n.textContent));
  await emit(page, { event: 'popup', content: { kind: 'bookmarkMenu', id: 10, isFolder: false, title: 'Rust', url: 'https://www.rust-lang.org/', children: 0, mode: 'newTab', canAdd: true } });
  await page.waitForSelector('.item');
  check(JSON.stringify(await labels()) === JSON.stringify(['Open in new tab', 'Open in new window', 'Open in private tab', 'Edit…', 'Delete', 'Add this page', 'New folder…', 'Always show the bar', 'Show it on new tabs only', 'Hide the bar']), 'bookmark menu items (' + (await labels()).join(', ') + ')');
  check(await page.locator('.item.radio.checked').textContent() === 'Show it on new tabs only', 'the current mode is checked');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom <= 352), 'the bookmark menu fits 240x352 (' + await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom) + ')');
  await shot(page, 'popup-bookmark-menu.png');
  await page.locator('.item', { hasText: 'Open in new window' }).click();
  let a = await lastSent(page, 'bookmarkMenuAction');
  check(a && a.id === 10 && a.action === 'openNewWindow', 'Open in new window sends bookmarkMenuAction{openNewWindow}');
  await page.locator('.item', { hasText: 'Hide the bar' }).click();
  a = await lastSent(page, 'bookmarkMenuAction');
  check(a && a.action === 'showNever', 'Hide sends bookmarkMenuAction{showNever}');
  await emit(page, { event: 'popup', content: { kind: 'bookmarkMenu', id: 11, isFolder: true, title: 'News', url: null, children: 3, mode: 'always', canAdd: false } });
  await page.waitForTimeout(20);
  check((await labels())[0] === 'Open all (3)' && (await labels()).includes('Delete folder and its 3 bookmarks') && (await labels()).includes('Rename…'), 'folder menu: Open all (3), Rename, Delete folder and its 3 bookmarks');
  check(await page.locator('.item', { hasText: 'Add this page' }).isDisabled(), 'Add this page disabled when the page cannot be bookmarked');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom <= 292), 'the folder menu fits 240x292 (' + await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom) + ')');
  await emit(page, { event: 'popup', content: { kind: 'bookmarkMenu', id: 12, isFolder: true, title: 'Empty', url: null, children: 0, mode: 'always', canAdd: true } });
  await page.waitForTimeout(20);
  check(await page.locator('.item', { hasText: 'Open all' }).isDisabled(), 'Open all disabled for an empty folder');
  await emit(page, { event: 'popup', content: { kind: 'bookmarkMenu', id: null, isFolder: false, title: '', url: null, children: 0, mode: 'never', canAdd: true } });
  await page.waitForTimeout(20);
  check(JSON.stringify(await labels()) === JSON.stringify(['Add this page', 'New folder…', 'Always show the bar', 'Show it on new tabs only', 'Hide the bar', 'Bookmark manager']), 'bar menu items (' + (await labels()).join(', ') + ')');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom <= 222), 'the bar menu fits 240x222 (' + await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom) + ')');
  await page.locator('.item', { hasText: 'New folder' }).click();
  a = await lastSent(page, 'bookmarkMenuAction');
  check(a && a.id === null && a.action === 'newFolder', 'New folder sends bookmarkMenuAction{id:null, newFolder}');

  // The editor.
  await page.setViewportSize({ width: 340, height: 214 });
  await emit(page, { event: 'popup', content: { kind: 'bookmarkEdit', id: 10, parent: 1, isFolder: false, title: 'Rust', url: 'https://www.rust-lang.org/' } });
  await page.waitForSelector('#bm-url');
  check(await page.evaluate(() => document.activeElement.id) === 'bm-name', 'the name field has the focus');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom <= 214), 'the editor fits 340x214');
  await shot(page, 'popup-bookmark-edit.png');
  await page.fill('#bm-url', 'javascript:alert(1)');
  await page.click('button[type=submit]');
  check((await lastSent(page, 'saveBookmark')) === undefined && (await page.textContent('.error')).includes('web address'), 'a non-web address is refused in the editor');
  await page.fill('#bm-name', 'Rust lang');
  await page.fill('#bm-url', 'https://www.rust-lang.org/learn');
  await page.click('button[type=submit]');
  let sv = await lastSent(page, 'saveBookmark');
  check(sv && sv.id === 10 && sv.parent === 1 && sv.title === 'Rust lang' && sv.url === 'https://www.rust-lang.org/learn', 'Save sends saveBookmark{id, parent, title, url}');
  await emit(page, { event: 'popup', content: { kind: 'bookmarkEdit', id: null, parent: 11, isFolder: true, title: 'New folder', url: null } });
  await page.waitForSelector('#bm-name');
  check(await page.locator('#bm-url').count() === 0 && (await page.textContent('h2')) === 'New folder', 'a new folder has a name only');
  await page.fill('#bm-name', 'Tools');
  await page.press('#bm-name', 'Enter');
  sv = await lastSent(page, 'saveBookmark');
  check(sv && sv.id === null && sv.parent === 11 && sv.title === 'Tools' && sv.url === null, 'Enter saves the new folder: saveBookmark{id:null, parent:11, title}');
  check(errors.length === 0, 'popup: no console errors (' + errors.join(' | ') + ')');
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
    h.navEvent({ zoom: 1.25, bookmarked: true, bookmarksBar: true });
    h.emit({ event: 'shellConfig', showHomeButton: true, theme: 'system' });
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
  const upd = { version: '0.2.0', server: 'https://krunk.example/', configError: null, installed: true, lastCheck: 0,
    lastResult: '', lastOk: true, checking: false, available: null, downloading: null, failed: null };
  window.__updates = upd;
  h.getUpdateStatus = () => upd;
  h.checkForUpdates = () => {
    upd.lastCheck = Date.now(); upd.lastResult = 'Krunk 0.2.1 is available';
    upd.available = { version: '0.2.1', released: '2026-11-05', install: true, notes: true };
    return upd;
  };
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
  check(await page.inputValue('#bookmarks-bar') === 'always', 'defaults: bookmarks bar always shown');
  await page.selectOption('#bookmarks-bar', 'newTab');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.bookmarksBar) === 'newTab', 'bookmarks bar: "only on new tabs" saved');
  await page.selectOption('#bookmarks-bar', 'always');
  await page.waitForTimeout(80);
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

  // Settings → About: version, update check, the toggle (on by default).
  check(await page.isChecked('[data-key=checkUpdatesAutomatically]'), 'defaults: automatic update checks on');
  check((await page.textContent('#update-status')) === 'Not checked yet', 'update status before the first check');
  check((await page.textContent('#update-server')) === 'https://krunk.example/', 'the update server is named');
  check(await page.locator('#install-update').isHidden(), 'no Update now without an update');
  await page.click('#check-updates');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'checkForUpdates')) !== undefined, 'Check for updates sends checkForUpdates');
  check((await page.textContent('#update-status')).includes('Krunk 0.2.1 is available'), 'an available update is shown');
  check(await page.locator('#install-update').isVisible() && (await page.textContent('#install-update')) === 'Update now', 'Update now appears');
  await page.click('#install-update');
  const ur = await lastSent(page, 'updateReply');
  check(ur && ur.choice === 'install' && Object.keys(ur).filter((k) => k !== 'cmd' && k !== 'reqId').length === 1, 'Update now sends only updateReply{choice:install}');
  await page.uncheck('[data-key=checkUpdatesAutomatically]');
  await page.waitForTimeout(60);
  check(await page.evaluate(() => window.__settings.settings.checkUpdatesAutomatically) === false, 'the automatic-check toggle is saved');

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

// uBO scriptlets as Krunk injects them (generated by the core test
// scriptlet_fixture_is_current): run at document start, before the page's own
// inline script, they pin window.krunkTest and prune adPlacements from JSON.
async function testScriptletInjection(browser) {
  console.log('scriptlet injection');
  const context = await browser.newContext();
  const js = fs.readFileSync(path.join(__dirname, 'scriptlet-fixture.js'), 'utf8');
  await context.route('https://example.com/**', (r) => r.fulfill({ contentType: 'text/html', body: `<!doctype html><script>
    window.seenAtStart = window.krunkTest;
    window.krunkTest = 5;
    window.afterOverwrite = window.krunkTest;
    window.parsed = JSON.parse('{"adPlacements":[{"ad":1}],"videoDetails":{"title":"t"}}');
  </script>` }));
  await context.addInitScript({ content: js });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://example.com/fixture.html');
  const r = await page.evaluate(() => ({ start: window.seenAtStart, after: window.afterOverwrite, parsed: window.parsed }));
  check(r.start === 1, 'set-constant is in place before the page script runs (' + r.start + ')');
  check(r.after === 1, 'the page cannot overwrite the constant (' + r.after + ')');
  check(r.parsed && !('adPlacements' in r.parsed) && r.parsed.videoDetails.title === 't', 'json-prune removes adPlacements and keeps the rest (' + JSON.stringify(r.parsed) + ')');
  check(errors.length === 0, 'scriptlets raise no page errors (' + errors.join(' | ') + ')');
  await context.close();
}

// The pointer-lock shim (ui/inject/pointer-lock.js) with a fake browser on the other side.
async function testPointerLockShim(browser) {
  console.log('pointer lock shim');
  const context = await browser.newContext();
  const shim = fs.readFileSync(path.join(__dirname, '..', 'inject', 'pointer-lock.js'), 'utf8');
  await context.addInitScript({ content: 'window.__sent = []; window.__plock = ' + shim + '(function (j) { window.__sent.push(JSON.parse(j)); });' });
  await context.route('https://game.example/**', (r) => r.fulfill({ contentType: 'text/html', body: `<!doctype html>
    <div id="stage" style="width:400px;height:300px;background:#333"></div>
    <div id="hud" style="position:absolute;left:0;top:0;width:400px;height:300px"></div>
    <script>
      window.log = [];
      document.addEventListener('pointerlockchange', () => log.push('change:' + (document.pointerLockElement ? document.pointerLockElement.id : 'null')));
      document.onpointerlockerror = () => log.push('error');
      document.addEventListener('mousemove', (e) => log.push('move:' + e.movementX + ',' + e.movementY + ':' + e.isTrusted + ':' + e.buttons));
      window.addEventListener('pointermove', (e) => log.push('pmove:' + e.movementX + ',' + e.movementY));
      // At load, without any user input: must be refused.
      window.noGesture = document.getElementById('stage').requestPointerLock().then(() => 'resolved', (e) => e.name);
    </script>` }));
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://game.example/');
  check(await page.evaluate(() => Element.prototype.requestPointerLock.toString()) === 'function requestPointerLock() { [native code] }', 'requestPointerLock looks native');
  check(await page.evaluate(() => 'pointerLockElement' in Document.prototype && document.pointerLockElement === null), 'pointerLockElement is null when unlocked');
  // Without a user gesture: refused, pointerlockerror, nothing sent.
  const noGesture = await page.evaluate(() => window.noGesture);
  check(noGesture === 'NotAllowedError', 'without user activation the promise rejects (NotAllowedError)');
  check((await page.evaluate(() => window.__sent.length)) === 0 && (await page.evaluate(() => log.includes('error'))), 'nothing asked, pointerlockerror fired');
  // With a click: the browser is asked.
  await page.evaluate(() => { document.getElementById('hud').addEventListener('click', () => { window.p = document.getElementById('stage').requestPointerLock({ unadjustedMovement: true }).then(() => 'resolved', (e) => e.name); }); });
  await page.click('#hud');
  check(JSON.stringify(await page.evaluate(() => window.__sent)) === '[{"lb":"pointerLock","unadjusted":true}]', 'a click sends the request with unadjustedMovement');
  await page.evaluate(() => window.__plock('{"t":"granted"}'));
  check((await page.evaluate(() => window.p)) === 'resolved', 'granted: the promise resolves');
  check(await page.evaluate(() => document.pointerLockElement && document.pointerLockElement.id === 'stage' && log.includes('change:stage')), 'pointerLockElement set and pointerlockchange fired');
  // Real (trusted) moves carry the browser's movement and a frozen position.
  await page.evaluate(() => { log.length = 0; window.xs = []; document.addEventListener('mousemove', (e) => xs.push(e.clientX)); });
  await page.evaluate(() => window.__plock('{"t":"move","dx":7,"dy":-3,"b":1}'));
  await page.mouse.move(200, 150);
  await page.evaluate(() => window.__plock('{"t":"move","dx":-2,"dy":5,"b":0}'));
  await page.mouse.move(210, 160);
  await page.mouse.move(220, 170);
  let moves = await page.evaluate(() => log.slice());
  check(moves.includes('move:7,-3:true:0') && moves.includes('move:-2,5:true:0'), 'trusted mousemove events carry the raw movement (' + moves.join(' ') + ')');
  check(moves.includes('move:0,0:true:0'), 'a real move with no new raw input has no movement (the re-centring never shows)');
  check(moves.includes('pmove:7,-3'), 'pointermove of the same input gets the same movement');
  await page.evaluate(() => { window.targets = []; document.getElementById('stage').addEventListener('mousemove', (e) => targets.push(e.target.id + ':' + e.isTrusted)); });
  await page.mouse.move(230, 180);
  check((await page.evaluate(() => window.targets)).includes('stage:true'), 'with an overlay on top, real moves go to the locked element (' + (await page.evaluate(() => window.targets)).join(',') + ')');
  const xs = await page.evaluate(() => window.xs);
  check(xs.length > 0 && xs.every((x) => x === xs[0]), 'clientX stays frozen while locked (' + xs.join(',') + ')');
  // A page listening to pointerrawupdate too (its own timing) still sees all the movement
  // in mousemove: every stream gets the full movement since its own previous event.
  await page.evaluate(() => {
    window.sums = { mousemove: 0, pointerrawupdate: 0 };
    const stage = document.getElementById('stage');
    stage.addEventListener('pointerrawupdate', (e) => { if (e.isTrusted) sums.pointerrawupdate += e.movementX; });
    stage.addEventListener('mousemove', (e) => { sums.mousemove += e.movementX; }); // trusted or the fallback
  });
  for (let i = 0; i < 5; i++) {
    await page.evaluate(() => window.__plock('{"t":"move","dx":3,"dy":0,"b":0}'));
    await page.mouse.move(240 + i * 3, 190);
  }
  await page.waitForTimeout(120); // the last move's events, or the fallback
  const sums = await page.evaluate(() => window.sums);
  check(sums.mousemove === 15, 'with a pointerrawupdate listener too, mousemove still gets all the movement (' + JSON.stringify(sums) + ')');
  check(sums.pointerrawupdate === 0 || sums.pointerrawupdate === 15, 'pointerrawupdate gets all of it as well, when it fires (' + sums.pointerrawupdate + ')');
  // Continuous movement with real events coming: no synthetic events in between (they only
  // carry what no real event picked up).
  await page.waitForTimeout(120);
  await page.evaluate(() => { window.synth = 0; window.realSum = 0; document.getElementById('stage').addEventListener('mousemove', (e) => { if (e.isTrusted) realSum += e.movementX; else synth++; }); });
  for (let i = 0; i < 20; i++) {
    await page.evaluate(() => window.__plock('{"t":"move","dx":2,"dy":0,"b":0}'));
    await page.mouse.move(100 + (i % 2) * 5, 100);
    await page.waitForTimeout(15);
  }
  const cont = await page.evaluate(() => ({ synth: window.synth, real: window.realSum }));
  check(cont.synth === 0 && cont.real >= 36, 'while real events keep coming, no synthetic ones (' + JSON.stringify(cont) + ')');
  await page.waitForTimeout(120); // let any leftover synthetic events pass
  // Movement no real event picked up goes out as a synthetic event.
  await page.evaluate(() => { log.length = 0; window.__plock('{"t":"move","dx":4,"dy":1,"b":1}'); });
  await page.waitForTimeout(120);
  moves = await page.evaluate(() => log.slice());
  check(moves.includes('move:4,1:false:1'), 'leftover movement arrives as a synthetic event (' + moves.join(' ') + ')');
  check(await page.evaluate(() => Object.getOwnPropertyDescriptor(MouseEvent.prototype, 'movementX').get.toString()) === 'function get movementX() { [native code] }', 'the movementX getter looks native');
  // exitPointerLock asks the browser; the release arrives as an event.
  await page.evaluate(() => document.exitPointerLock());
  check((await page.evaluate(() => window.__sent.pop())).lb === 'pointerUnlock', 'exitPointerLock sends pointerUnlock');
  await page.evaluate(() => window.__plock('{"t":"released"}'));
  check(await page.evaluate(() => document.pointerLockElement === null && log.includes('change:null')), 'released: unlocked and pointerlockchange');
  check(await page.evaluate(() => getComputedStyle(document.getElementById('hud')).pointerEvents) === 'auto', 'released: the page is clickable again');
  // Denied requests reject.
  await page.click('#hud');
  await page.evaluate(() => window.__plock('{"t":"denied"}'));
  check((await page.evaluate(() => window.p)) === 'NotAllowedError', 'a denied request rejects');
  check(errors.length === 0, 'no page errors (' + errors.join(' | ') + ')');
  await context.close();
}

// Krunk 0.3.0: moving tabs between windows (shell side; the host part runs on the VM).
async function testTabWindows(browser) {
  console.log('tabs and windows');
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1000, height: 120 }, 1);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = [1, 2, 3, 4].map((id) => ({ id: id, title: 'Tab ' + id, url: 'https://t' + id + '.example/', favicon: null, loading: false, private: false, sleeping: false }));
    h.state.active = 1;
    h.tabsEvent();
  });
  await page.waitForSelector('.tab');
  await page.waitForTimeout(100);
  const ids = await page.$$eval('.tab', (ns) => ns.map((n) => +n.dataset.id));

  // The hit map reports each tab's centre, left to right (where drops from other windows land).
  const map = (await sent(page)).filter((m) => m.cmd === 'hitMap').pop().map;
  const c = map.tabCenters || [];
  check(c.length === ids.length && c.every((x, i) => i === 0 || x > c[i - 1]), 'hitMap.tabCenters: one increasing centre per tab (' + JSON.stringify(c) + ')');

  // Dragging a tab down out of the strip hands the drag over to the host, with the grab point.
  const t2 = page.locator('.tab[data-id="' + ids[1] + '"]');
  const b = await t2.boundingBox();
  const before = (await sent(page)).filter((m) => m.cmd === 'moveTab').length;
  await page.mouse.move(b.x + 30, b.y + 12);
  await page.mouse.down();
  await page.mouse.move(b.x + 40, b.y + 14, { steps: 2 });
  await page.mouse.move(b.x + 60, b.y + 90, { steps: 4 });
  await page.mouse.up();
  const out = await lastSent(page, 'tabDragOut');
  check(out && out.id === ids[1] && Math.abs(out.offsetX - 30) < 2 && Math.abs(out.offsetY - (b.y + 12)) < 2,
    'dragging out of the strip sends tabDragOut{id, offsetX, offsetY} (got ' + JSON.stringify(out) + ')');
  check((await sent(page)).filter((m) => m.cmd === 'moveTab').length === before, 'a drag-out is not also a reorder');
  check(await page.locator('.tab.dragging, .tab.drop-before, .tab.drop-after').count() === 0, 'no drag marks left after the hand-over');

  // A small wiggle is a click, not a drag; a horizontal drag still reorders.
  const t1 = page.locator('.tab[data-id="' + ids[0] + '"]');
  const b1 = await t1.boundingBox();
  const outs = (await sent(page)).filter((m) => m.cmd === 'tabDragOut').length;
  await page.mouse.move(b1.x + 20, b1.y + 10);
  await page.mouse.down();
  await page.mouse.move(b1.x + 22, b1.y + 11);
  await page.mouse.up();
  check((await sent(page)).filter((m) => m.cmd === 'moveTab').length === before &&
    (await sent(page)).filter((m) => m.cmd === 'tabDragOut').length === outs, 'a 2-px wiggle neither reorders nor drags out');
  const last = await page.locator('.tab').last().boundingBox();
  await page.mouse.move(b1.x + 20, b1.y + 10);
  await page.mouse.down();
  await page.mouse.move(last.x + last.width - 4, last.y + 10, { steps: 6 });
  check(await page.locator('.tab.drop-after, .tab.drop-before').count() === 1, 'one drop mark while reordering');
  await page.mouse.up();
  const mv = await lastSent(page, 'moveTab');
  check(mv && mv.id === ids[0] && mv.index === ids.length - 1, 'horizontal drag reorders (got ' + JSON.stringify(mv) + ')');

  // The docked DevTools control appears with DevTools and switches its side.
  check(await page.locator('#devtools-ctl').isHidden(), 'no DevTools control while the tab has no DevTools');
  await emit(page, { event: 'devTools', open: true, dock: 'right' });
  await page.waitForTimeout(20);
  check(await page.locator('#devtools-ctl').isVisible(), 'DevTools control shown when DevTools is docked');
  check((await page.getAttribute('#dt-dock', 'title')) === 'Dock DevTools at the bottom', 'docked right: offers the bottom');
  await shot(page, 'shell-devtools-control.png');
  await page.click('#dt-dock');
  check((await lastSent(page, 'devToolsDock')).dock === 'bottom', 'dock button sends devToolsDock{bottom}');
  await emit(page, { event: 'devTools', open: true, dock: 'bottom' });
  await page.waitForTimeout(20);
  check((await page.getAttribute('#dt-dock', 'title')) === 'Dock DevTools on the right', 'docked at the bottom: offers the right');
  await page.click('#dt-dock');
  check((await lastSent(page, 'devToolsDock')).dock === 'right', 'and sends devToolsDock{right}');
  await page.click('#dt-close');
  check((await lastSent(page, 'devToolsClose')) !== undefined, 'close button sends devToolsClose');
  await emit(page, { event: 'devTools', open: false, dock: 'right' });
  await page.waitForTimeout(20);
  check(await page.locator('#devtools-ctl').isHidden(), 'hidden again when DevTools closes');

  // A tab from another window hovering this strip: the host says where it would land.
  await emit(page, { event: 'tabDropHint', index: 1 });
  await page.waitForTimeout(20);
  const marks = await page.$$eval('.tab', (ns) => ns.map((n) => (n.classList.contains('drop-before') ? 'B' : '') + (n.classList.contains('drop-after') ? 'A' : '')));
  check(marks[1] === 'B' && marks.filter((m) => m).length === 1, 'tabDropHint{index:1} marks before the second tab (' + marks.join(',') + ')');
  await shot(page, 'shell-drop-hint.png');
  await emit(page, { event: 'tabDropHint', index: 99 });
  await page.waitForTimeout(20);
  check(await page.locator('.tab').last().evaluate((n) => n.classList.contains('drop-after')), 'an index past the end marks after the last tab');
  await emit(page, { event: 'tabDropHint', index: null });
  await page.waitForTimeout(20);
  check(await page.locator('.tab.drop-before, .tab.drop-after').count() === 0, 'tabDropHint{index:null} clears the mark');

  // Right-click on a tab asks for its context menu at the click.
  const t3 = page.locator('.tab[data-id="' + ids[2] + '"] .title');
  await t3.click({ button: 'right' });
  const tm = await lastSent(page, 'showTabMenu');
  check(tm && tm.id === ids[2] && tm.rect.width === 240 && tm.rect.height === 256 && tm.rect.y > 0, 'right-click sends showTabMenu{id, rect} (got ' + JSON.stringify(tm) + ')');
  check(!(await sent(page)).some((m) => m.cmd === 'showPopup' && m.popup === 'tabMenu'), 'not through showPopup');
  check(errors.length === 0, 'shell: no console errors (' + errors.join(' | ') + ')');

  // The popup's tab menu.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 240, height: 256 }, 1));
  await page.evaluate(() => { window.__host.autoSimulate = false; });
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 7, canMove: false, canCloseRight: false } });
  await page.waitForSelector('.item');
  const labels = await page.$$eval('.item .label', (ns) => ns.map((n) => n.textContent));
  check(JSON.stringify(labels) === JSON.stringify(['Reload', 'Duplicate', 'Move to new window', 'New window', 'Close tab', 'Close other tabs', 'Close tabs to the right']), 'tab menu items (' + labels.join(', ') + ')');
  check(await page.locator('.item', { hasText: 'Move to new window' }).isDisabled(), '"Move to new window" is disabled for a window\'s only tab');
  check(await page.locator('.item', { hasText: 'Close other tabs' }).isDisabled(), '"Close other tabs" is disabled for a window\'s only tab');
  check(await page.locator('.item', { hasText: 'Close tabs to the right' }).isDisabled(), '"Close tabs to the right" is disabled with nothing to the right');
  const fits = await page.evaluate(() => document.documentElement.scrollHeight <= 256 && document.documentElement.scrollWidth <= 240);
  check(fits, 'the tab menu fits 240x256 (no scrolling)');
  await shot(page, 'popup-tab-menu-single.png');
  await page.locator('.item', { hasText: 'Duplicate' }).click();
  let a = await lastSent(page, 'tabMenuAction');
  check(a && a.id === 7 && a.action === 'duplicate' && (await lastSent(page, 'hidePopup')) !== undefined, 'Duplicate sends tabMenuAction{duplicate} and closes');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 8, canMove: true } });
  await page.waitForTimeout(20);
  check(!(await page.locator('.item', { hasText: 'Move to new window' }).isDisabled()), 'enabled when the window has other tabs');
  await shot(page, 'popup-tab-menu.png');
  await page.locator('.item', { hasText: 'Move to new window' }).click();
  a = await lastSent(page, 'tabMenuAction');
  check(a && a.id === 8 && a.action === 'moveToNewWindow', 'Move to new window sends tabMenuAction{moveToNewWindow}');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 8, canMove: true } });
  await page.waitForTimeout(20);
  await page.locator('.item', { hasText: 'New window' }).last().click();
  const nw = await lastSent(page, 'command');
  check(nw && nw.command.command === 'newWindow', 'New window sends command{newWindow}');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 9, canMove: true } });
  await page.waitForTimeout(20);
  await page.locator('.item', { hasText: 'Close tab' }).first().click();
  a = await lastSent(page, 'tabMenuAction');
  check(a && a.id === 9 && a.action === 'close', 'Close tab sends tabMenuAction{close}');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 10, canMove: true, canCloseRight: true } });
  await page.waitForTimeout(20);
  check(!(await page.locator('.item', { hasText: 'Close tabs to the right' }).isDisabled()), '"Close tabs to the right" is enabled with tabs to the right');
  await shot(page, 'popup-tab-menu-close-many.png');
  await page.locator('.item', { hasText: 'Close other tabs' }).click();
  a = await lastSent(page, 'tabMenuAction');
  check(a && a.id === 10 && a.action === 'closeOthers', 'Close other tabs sends tabMenuAction{closeOthers}');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 11, canMove: true, canCloseRight: true } });
  await page.waitForTimeout(20);
  await page.locator('.item', { hasText: 'Close tabs to the right' }).click();
  a = await lastSent(page, 'tabMenuAction');
  check(a && a.id === 11 && a.action === 'closeRight', 'Close tabs to the right sends tabMenuAction{closeRight}');

  // The main menu has "New window" (Ctrl+N).
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: false } });
  await page.waitForTimeout(20);
  const nwItem = page.locator('.item', { hasText: 'New window' });
  check(await nwItem.count() === 1 && (await nwItem.locator('.kbd').textContent()) === 'Ctrl+N', 'main menu: New window, Ctrl+N');
  await nwItem.click();
  check((await lastSent(page, 'command')).command.command === 'newWindow', 'main menu New window sends command{newWindow}');
  check(errors.length === 0, 'popup: no console errors (' + errors.join(' | ') + ')');
}

(async () => {
  if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
  const browser = await chromium.launch();
  try {
    await testShell(browser);
    await testScriptletInjection(browser);
    await testPointerLockShim(browser);
    await testShellFrame(browser);
    await testTabAnimations(browser);
    await testTabWindows(browser);
    await testShellHiDpi(browser);
    await testPopup(browser);
    await testNewTab(browser);
    await testHistory(browser);
    await testBookmarks(browser);
    await testDownloads(browser);
    await testStorage(browser);
    await testSiteData(browser);
    await testShellM4(browser);
    await testBookmarksBar(browser);
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
