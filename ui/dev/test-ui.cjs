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
// ONLY=NewTab,Settings runs those sections alone (a comma list of the test function names without 'test').
const ONLY = process.env.ONLY ? process.env.ONLY.split(',') : null;
const CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https://*.ext.krunk.invalid; font-src 'self'; " +
  "connect-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";
const MIME = { html: 'text/html', css: 'text/css', js: 'text/javascript', svg: 'image/svg+xml', png: 'image/png', webp: 'image/webp', json: 'application/json' };

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
    if (url.pathname === '/tab-icon') {
      return r.fulfill({ status: 200, headers: { 'content-type': 'image/png' }, body: faviconPng('tab' + url.searchParams.get('id')) });
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

// Krunk 0.10.0: what one background tab's title change costs the tab strip with 60 tabs: nodes
// added (MutationObserver) and main-thread time over 50 such changes, sent as the full list
// (`tabs`) and as a per-tab `tabUpdate`. 0.9.0 (P1 baseline): 180 nodes and 11.3 ms per event.
async function testTabStripCost(browser) {
  console.log('tab strip cost');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 76 }, 1);
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = Array.from({ length: 60 }, (_, i) => ({ id: i + 1, title: 'A page title of ordinary length ' + i, url: 'https://site' + i + '.example/', favicon: '/tab-icon?id=' + (i + 1) + '&v=1', loading: false, private: false, sleeping: false }));
    h.state.active = 1;
    h.tabsEvent();
    h.navEvent();
  });
  await page.waitForFunction(() => document.querySelectorAll('.tab').length === 60);
  await settleTabs(page);
  const measure = (kind) => page.evaluate(async (kind) => {
    const h = window.__host;
    let added = 0;
    const mo = new MutationObserver((recs) => recs.forEach((m) => { added += m.addedNodes.length; }));
    mo.observe(document.getElementById('tabs'), { childList: true, subtree: true });
    const t0 = performance.now();
    for (let k = 0; k < 50; k++) {
      h.state.tabs[30].title = kind + ' title ' + k;
      if (kind === 'none') h.emit({ event: 'noSuchEvent' });
      else if (kind === 'update') h.emit({ event: 'tabUpdate', tab: h.state.tabs[30] }); else h.tabsEvent();
      await new Promise((res) => setTimeout(res, 0));
      await new Promise((res) => setTimeout(res, 0));
    }
    await new Promise((res) => requestAnimationFrame(() => res()));
    const ms = performance.now() - t0;
    mo.disconnect();
    return { added: added / 50, ms: ms / 50, title: document.querySelector('.tab[data-id="31"] .title').textContent };
  }, kind);
  // The loop's own waits (timer clamping) are measured alone and taken off.
  const none = await measure('none');
  const full = await measure('full');
  const one = await measure('update');
  const net = (r) => Math.max(0, r.ms - none.ms).toFixed(2);
  console.log(`  info 60 tabs, one title change: full list ${full.added.toFixed(1)} nodes, ${net(full)} ms; tabUpdate ${one.added.toFixed(1)} nodes, ${net(one)} ms per event (loop ${none.ms.toFixed(2)} ms)`);
  check(full.title === 'full title 49' && one.title === 'update title 49', 'the changed tab shows its new title');
  check(full.added <= 2 && one.added <= 2, 'a title change replaces no tab contents (' + full.added + ', ' + one.added + ' nodes)');
  // An icon change replaces that tab's icon only; loading shows the spinner and back.
  const icon = await page.evaluate(async () => {
    const h = window.__host;
    const t = h.state.tabs[30];
    const node = document.querySelector('.tab[data-id="31"]');
    const close = node.querySelector('.close');
    t.favicon = '/tab-icon?id=31&v=2';
    h.emit({ event: 'tabUpdate', tab: t });
    await new Promise((res) => setTimeout(res, 20));
    const src = node.querySelector('img.favicon').getAttribute('src');
    t.loading = true;
    h.emit({ event: 'tabUpdate', tab: t });
    await new Promise((res) => setTimeout(res, 20));
    const spinner = !!node.querySelector('.spinner') && !node.querySelector('img.favicon');
    t.loading = false;
    t.audible = true;
    h.emit({ event: 'tabUpdate', tab: t });
    await new Promise((res) => setTimeout(res, 20));
    return { src, spinner, back: !!node.querySelector('img.favicon'), audio: !!node.querySelector('.audio'), sameClose: node.querySelector('.close') === close,
      others: document.querySelectorAll('.tab .favicon').length };
  });
  check(icon.src === '/tab-icon?id=31&v=2', 'a new icon address replaces the image');
  check(icon.spinner && icon.back, 'loading shows the spinner, then the icon again');
  check(icon.audio, 'a sound update shows the speaker');
  check(icon.sameClose, 'the close button is kept across updates');
  check(icon.others === 60, 'the other tabs keep their icons');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
}

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
  // 0.6.0: a speaker on a tab that plays sound; a click mutes it (tabMute); muted shows crossed out.
  await page.evaluate(() => { const h = window.__host; h.state.tabs[1].audible = true; h.state.tabs[3].muted = true; h.tabsEvent(); });
  await page.waitForTimeout(30);
  check((await page.locator('.tab[data-id="2"] .audio:not(.muted)').count()) === 1 && (await page.locator('.tab[data-id="4"] .audio.muted').count()) === 1 && (await page.locator('.tab[data-id="1"] .audio').count()) === 0, 'speaker on the audible tab, crossed out on the muted one, none elsewhere');
  await page.click('.tab[data-id="2"] .audio');
  await page.waitForTimeout(20);
  {
    const m = await lastSent(page, 'tabMute');
    check(m && m.id === 2 && m.muted === true, 'clicking the speaker sends tabMute{id, muted:true}');
    check(!(await sent(page)).some((x) => x.cmd === 'activateTab' && x.id === 2), 'the click does not activate the tab');
  }
  await page.click('.tab[data-id="4"] .audio');
  await page.waitForTimeout(20);
  {
    const m = await lastSent(page, 'tabMute');
    check(m && m.id === 4 && m.muted === false, 'clicking the crossed-out speaker unmutes');
  }
  await page.evaluate(() => { const h = window.__host; delete h.state.tabs[1].audible; delete h.state.tabs[3].muted; h.tabsEvent(); });
  await page.waitForTimeout(30);
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
  // A real drag: press, a few moves, release (dragTo would skip the moves in between).
  const dragTab = async (id, x, y) => {
    const b = await page.locator('.tab[data-id="' + id + '"]').boundingBox();
    await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
    await page.mouse.down();
    await page.mouse.move(x, y, { steps: 5 });
    await page.mouse.up();
  };
  await dragTab(1, box.x + box.width - 5, box.y + box.height / 2);
  const mv = await lastSent(page, 'moveTab');
  check(mv && mv.id === 1 && mv.index === 2, 'drag-reorder sends moveTab{id:1,index:2} (got ' + JSON.stringify(mv) + ')');
  // The dropped tab slides into its slot (about 170 ms), then the strip shows the new order.
  check(await page.locator('.tab[data-id="1"].settling').count() === 1, 'the dropped tab settles into its slot');
  await page.waitForFunction(() => !document.querySelector('.tab.settling'), null, { timeout: 2000 });
  const after = await page.$$eval('.tab', (ns) => ns.map((n) => +n.dataset.id));
  check(after[2] === 1, 'tab 1 is now last');
  check(await page.$$eval('.tab', (ns) => ns.every((n) => !n.style.transform)), 'no transforms left after the settle');

  // Drag onto the left half of the first tab: index 0.
  const first = await page.locator('.tab').first().boundingBox();
  await dragTab(1, first.x + 5, first.y + 10);
  const mv2 = await lastSent(page, 'moveTab');
  check(mv2 && mv2.id === 1 && mv2.index === 0, 'drag to the front sends index 0');
  await page.waitForFunction(() => !document.querySelector('.tab.settling'), null, { timeout: 2000 });

  // Dropping on the last tab's close button (right half) still means "after".
  await page.waitForTimeout(50);
  const count = await page.locator('.tab').count();
  const lastClose = await page.locator('.tab').last().locator('.close').boundingBox();
  await dragTab(1, lastClose.x + lastClose.width / 2, lastClose.y + lastClose.height / 2);
  const mv3 = await lastSent(page, 'moveTab');
  check(mv3 && mv3.id === 1 && mv3.index === count - 1, 'drop over a child of the right half moves after it (got ' + JSON.stringify(mv3) + ')');
  await page.waitForFunction(() => !document.querySelector('.tab.settling'), null, { timeout: 2000 });

  // Double-click on an inactive tab duplicates it, although the first press
  // activates the tab and re-renders the strip.
  await page.waitForTimeout(50);
  const inactive = await page.$eval('.tab:not(.active)', (n) => +n.dataset.id);
  const dupBefore = (await sent(page)).filter((m) => m.cmd === 'duplicateTab').length;
  await page.locator('.tab[data-id="' + inactive + '"] .title').dblclick();
  await page.waitForTimeout(50);
  const dups = (await sent(page)).filter((m) => m.cmd === 'duplicateTab');
  check(dups.length === dupBefore + 1 && dups[dups.length - 1].id === inactive, 'double-click on an inactive tab sends one duplicateTab{id:' + inactive + '}');

  // Address bar (a click selects the address once the click is handled, before anyone types).
  await page.click('#address');
  await page.waitForTimeout(30);
  await page.keyboard.type('rust borrow checker');
  await page.keyboard.press('Enter');
  const nav = await lastSent(page, 'navigate');
  check(nav && nav.input === 'rust borrow checker', 'typing + Enter sends navigate{input} (' + JSON.stringify(nav) + ')');
  await page.click('#address');
  await page.keyboard.type('garbage');
  await page.keyboard.press('Escape');
  check(!(await sent(page)).some((m) => m.cmd === 'navigate' && m.input.includes('garbage')), 'Escape does not navigate');

  // The address bar's placeholder names the default search engine and follows the setting.
  await emit(page, { event: 'shellConfig', theme: 'system', searchName: 'DuckDuckGo' });
  await page.waitForTimeout(20);
  check((await page.getAttribute('#address', 'placeholder')) === 'Search with DuckDuckGo or enter address', 'placeholder names DuckDuckGo');
  await emit(page, { event: 'shellConfig', theme: 'system', searchName: 'Brave Search' });
  await page.waitForTimeout(20);
  check((await page.getAttribute('#address', 'placeholder')) === 'Search with Brave Search or enter address', 'placeholder follows a search-engine change at once');
  // 0.6.0 appearance through shellConfig: variables, density, tab shape, hidden items.
  await emit(page, { event: 'shellConfig', theme: 'system', searchName: 'DuckDuckGo', appearance: { accent: '#16a34a', toolbarColor: '#fff7ed', stripColor: '', backgroundColor: '', density: 'compact', uiFontPx: 15, roundedTabs: false, showDownloadsButton: true, showZoomChip: false, showExtensionsButton: true } });
  await page.waitForTimeout(30);
  {
    const st = await page.evaluate(() => { const r = document.documentElement; const cs = getComputedStyle(r); return { accent: cs.getPropertyValue('--accent').trim(), toolbar: getComputedStyle(document.getElementById('toolbar')).backgroundColor, font: r.style.fontSize, compact: r.classList.contains('compact'), square: r.classList.contains('square-tabs'), strip: document.getElementById('tabstrip').getBoundingClientRect().height, zoom: getComputedStyle(document.getElementById('zoom')).display }; });
    check(st.accent === '#16a34a' && st.toolbar === 'rgb(255, 247, 237)' && st.font === '15px' && st.compact && st.square && st.strip === 30 && st.zoom === 'none', 'appearance applies live: accent, toolbar colour, font size, compact strip (30px), square tabs, zoom chip hidden (' + JSON.stringify(st) + ')');
  }
  await emit(page, { event: 'shellConfig', theme: 'system', searchName: 'DuckDuckGo', appearance: { accent: '', toolbarColor: '', stripColor: '', backgroundColor: '', density: 'normal', uiFontPx: 13, roundedTabs: true, showDownloadsButton: true, showZoomChip: true, showExtensionsButton: true } });
  await page.waitForTimeout(30);
  check(!(await page.evaluate(() => document.documentElement.classList.contains('compact'))) && (await page.evaluate(() => document.documentElement.style.fontSize)) === '', 'defaults restore the normal look');
  // 0.6.0: the camera/microphone chip follows the nav state.
  await page.evaluate(() => { window.__host.navEvent({ camera: true, microphone: false }); });
  await page.waitForTimeout(30);
  {
    const st = await page.evaluate(() => { const g = (id) => { const e = document.getElementById(id); const r = e.getBoundingClientRect(); return { hidden: e.hidden, w: r.width, h: r.height, d: getComputedStyle(e).display }; }; return { chip: g('media-chip'), cam: g('media-cam'), mic: g('media-mic'), nav: window.__host && window.__host.state.active }; });
    check(!st.chip.hidden && st.chip.w > 0 && !st.cam.hidden && st.cam.w > 0 && st.mic.hidden, 'camera in use: the chip shows the camera only (' + JSON.stringify(st) + ')');
  }
  await page.evaluate(() => { window.__host.navEvent({ camera: false, microphone: false }); });
  await page.waitForTimeout(30);
  check(await page.locator('#media-chip').isHidden(), 'no media in use: no chip');

  // Menu popup open/close.
  await page.click('#menu');
  const sp = await lastSent(page, 'showPopup');
  check(sp && sp.popup === 'menu' && sp.rect.width === 280 && sp.rect.height === 670 && sp.rect.x >= 0 && sp.rect.y > 60, 'menu sends showPopup with a rect below the toolbar (670 is the upper bound with the 0.7.2 update item; the popup fits itself)');
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

  // The press on ⋮ while its menu is open deactivates the popup: the host sends popupClosed
  // before the click arrives. The click must close, not reopen (3 clicks: open, closed, open).
  check(await page.locator('#home').count() === 0, 'the Home button is gone (0.5)');
  // The site icon opens the lock popup under itself; a second click closes it.
  await page.click('#site-icon');
  const si = await lastSent(page, 'showPopup');
  check(si && si.popup === 'siteInfo' && si.rect.width === 360 && si.rect.y > 40 && si.rect.x < 200, 'the site icon sends showPopup{siteInfo} under the icon (' + JSON.stringify(si && si.rect) + ')');
  check(await page.locator('#site-icon.on').count() === 1, 'site icon highlighted while its popup is open');
  await page.click('#site-icon');
  check((await lastSent(page, 'hidePopup')) !== undefined && await page.locator('#site-icon.on').count() === 0, 'clicking it again closes the popup');
  const countShows = async () => (await sent(page)).filter((m) => m.cmd === 'showPopup' && m.popup === 'menu').length;
  await page.click('#menu');
  const shows1 = await countShows();
  check(await page.locator('#menu.on').count() === 1, 'toggle: click 1 opens the menu');
  const mb = await page.locator('#menu').boundingBox();
  await page.mouse.move(mb.x + mb.width / 2, mb.y + mb.height / 2);
  await page.mouse.down();
  await emit(page, { event: 'popupClosed' });
  await page.waitForTimeout(20);
  await page.mouse.up();
  await page.waitForTimeout(20);
  check((await countShows()) === shows1, 'toggle: click 2 (popup closed by the press) does not reopen it');
  check(await page.locator('#menu.on').count() === 0, 'toggle: menu button not highlighted after click 2');
  await page.click('#menu');
  check((await countShows()) === shows1 + 1 && await page.locator('#menu.on').count() === 1, 'toggle: click 3 opens the menu again');
  await page.click('#menu');
  check(await page.locator('#menu.on').count() === 0, 'toggle: click 4 closes it (no stale press record)');

  // The address bar keeps the user's text only after typing: a focused but untouched field
  // follows a navigation (a bookmark click), and blurAddressBar ends editing.
  await page.click('#address');
  const h2 = await page.evaluate(() => window.__host);
  await page.evaluate(() => { const h = window.__host; h.state.tabs.find((t) => t.id === h.state.active).url = 'https://moved.example/'; h.navEvent({}); });
  await page.waitForTimeout(20);
  check(await page.locator('#address').inputValue() === 'https://moved.example/', 'a nav while the field is focused but untouched shows the new URL');
  await page.keyboard.type('typed text');
  await page.evaluate(() => { const h = window.__host; h.state.tabs.find((t) => t.id === h.state.active).url = 'https://moved2.example/'; h.navEvent({}); });
  await page.waitForTimeout(20);
  check((await page.locator('#address').inputValue()).endsWith('typed text'), 'a nav while the user typed keeps the typed text');
  await emit(page, { event: 'blurAddressBar' });
  await page.waitForTimeout(20);
  const afterBlur = await page.evaluate(() => ({ active: document.activeElement === document.getElementById('address'), value: document.getElementById('address').value }));
  // 0.11.6: unfinished text stays with its tab (as in Chrome) until the tab's page changes.
  check(!afterBlur.active && afterBlur.value.endsWith('typed text'), 'blurAddressBar leaves the field; the unfinished text stays (' + afterBlur.value + ')');
  void h2;
  {
    const val = () => page.locator('#address').inputValue();
    const ids = await page.$$eval('#tabs .tab', (ns) => ns.map((n) => +n.dataset.id));
    const here = await page.evaluate(() => window.__host.state.active);
    const other = ids.find((id) => id !== here);
    const urlOf = (id) => page.evaluate((id) => window.__host.state.tabs.find((t) => t.id === id).url, id);
    await page.locator('#tabs .tab[data-id="' + other + '"] .title').click();
    await page.waitForTimeout(60);
    check(await val() === await urlOf(other), 'another tab shows its own address (' + await val() + ')');
    await page.locator('#tabs .tab[data-id="' + here + '"] .title').click();
    await page.waitForTimeout(60);
    check((await val()).endsWith('typed text'), 'back on the tab, its unfinished text is there again (' + await val() + ')');
    // Typing, then switching with the keyboard (the field keeps the focus): the text stays behind.
    await page.click('#address');
    await page.keyboard.press('Control+a');
    await page.keyboard.type('half typed');
    await page.evaluate((id) => { const h = window.__host; h.state.active = id; h.tabsEvent(); h.navEvent({}); }, other);
    await page.waitForTimeout(60);
    check(await val() === await urlOf(other), 'Ctrl+Tab while typing: the other tab shows its address (' + await val() + ')');
    await page.evaluate((id) => { const h = window.__host; h.state.active = id; h.tabsEvent(); h.navEvent({}); }, here);
    await page.waitForTimeout(60);
    check(await val() === 'half typed', 'and the text is back with its tab (' + await val() + ')');
    // Esc goes back to the page address and forgets the text.
    await page.click('#address');
    await page.keyboard.press('Escape');
    await page.waitForTimeout(30);
    await page.evaluate(() => window.__host.navEvent({}));
    await page.waitForTimeout(40);
    check(await val() === await urlOf(here), 'Esc drops the unfinished text (' + await val() + ')');
    // The tab's page changing drops it too; and addressNavigated (a suggestion, a bookmark opened).
    await page.click('#address');
    await page.keyboard.type('again');
    await emit(page, { event: 'blurAddressBar' });
    await page.waitForTimeout(30);
    await page.evaluate(() => { const h = window.__host; h.state.tabs.find((t) => t.id === h.state.active).url = 'https://moved3.example/'; h.navEvent({}); });
    await page.waitForTimeout(40);
    check(await val() === 'https://moved3.example/', 'a new page in the tab drops the unfinished text (' + await val() + ')');
    await page.click('#address');
    await page.keyboard.type('picked');
    await emit(page, { event: 'addressNavigated' });
    await emit(page, { event: 'blurAddressBar' });
    await page.waitForTimeout(40);
    check(await val() === 'https://moved3.example/', 'addressNavigated drops it (' + await val() + ')');
  }

  // 0.5.1: suggestions while typing. The host answers addressSuggestions; Down/Up select
  // (the bar shows the row's fill), Enter picks, Esc closes without leaving the field.
  await page.click('#address');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('ru');
  await page.waitForTimeout(120);
  const asked = await lastSent(page, 'addressSuggest');
  check(asked && asked.text === 'ru' && asked.rect && asked.rect.width > 100 && asked.rect.y > 0, 'typing asks the host for suggestions with the bar\'s rect (' + JSON.stringify(asked) + ')');
  const rows = [
    { url: 'https://duckduckgo.com/?q=ru', title: 'ru', kind: 'search', fill: 'ru' },
    { url: 'https://www.rust-lang.org/', title: 'Rust', kind: 'bookmark', fill: 'https://www.rust-lang.org/' },
    { url: 'https://docs.rs/', title: 'Docs.rs', kind: 'history', fill: 'https://docs.rs/' },
  ];
  await emit(page, { event: 'addressSuggestions', text: 'ru', entries: rows });
  await page.waitForTimeout(20);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowDown');
  await page.waitForTimeout(20);
  let sel = await lastSent(page, 'addressSuggestSelect');
  check(sel && sel.selected === 1 && await page.locator('#address').inputValue() === 'https://www.rust-lang.org/', 'Down twice selects the second row and shows its address');
  await page.keyboard.press('ArrowUp');
  await page.keyboard.press('ArrowUp');
  await page.waitForTimeout(20);
  sel = await lastSent(page, 'addressSuggestSelect');
  check(sel && sel.selected === null && await page.locator('#address').inputValue() === 'ru', 'Up past the first row restores the typed text');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(30);
  const picked = await lastSent(page, 'addressSuggestPick');
  check(picked && picked.url === 'https://duckduckgo.com/?q=ru' && (await lastSent(page, 'hidePopup')) !== undefined, 'Enter on a selected row picks it and closes the suggestions');
  await page.click('#address');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('rus');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows });
  await page.waitForTimeout(20);
  const before = (await sent(page)).length;
  await page.keyboard.press('Escape');
  await page.waitForTimeout(20);
  check(await page.evaluate(() => document.activeElement === document.getElementById('address')) && await page.locator('#address').inputValue() === 'rus' && (await sent(page)).slice(before).some((m) => m.cmd === 'hidePopup'), 'Esc with suggestions open closes them and keeps the field');
  await emit(page, { event: 'addressSuggestions', text: 'zzz', entries: rows });
  await page.waitForTimeout(20);
  await page.keyboard.press('ArrowDown');
  await page.waitForTimeout(20);
  check(await page.locator('#address').inputValue() === 'rus', 'a stale answer (other text) is ignored');

  // 0.6.0: inline autocomplete. The host's answer carries `inline`; the bar shows the
  // completed text with the tail selected and the row highlighted; Enter goes there.
  const inl = { text: 'rust-lang.org', url: 'https://www.rust-lang.org/', index: 1 };
  await page.click('#address');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('rus');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  let selRange = await page.evaluate(() => { const a = document.getElementById('address'); return [a.value, a.selectionStart, a.selectionEnd]; });
  check(selRange[0] === 'rust-lang.org' && selRange[1] === 3 && selRange[2] === 13, 'inline completion fills the rest and selects it (' + JSON.stringify(selRange) + ')');
  sel = await lastSent(page, 'addressSuggestSelect');
  check(sel && sel.selected === 1, 'the completed row is highlighted in the popup');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(30);
  let pick = await lastSent(page, 'addressSuggestPick');
  check(pick && pick.url === 'https://www.rust-lang.org/', 'Enter goes to the completed entry');
  // Typing on replaces the selection and asks again.
  await page.click('#address');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('rus');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  await page.keyboard.type('x');
  await page.waitForTimeout(120);
  check(await page.locator('#address').inputValue() === 'rusx' && (await lastSent(page, 'addressSuggest')).text === 'rusx', 'typing on replaces the completion and continues');
  // Backspace removes the completion and it is not put back for that keystroke.
  await page.keyboard.press('Control+A');
  await page.keyboard.type('rus');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  await page.keyboard.press('Backspace');
  await page.waitForTimeout(120);
  check(await page.locator('#address').inputValue() === 'rus', 'Backspace removes the completion (' + await page.locator('#address').inputValue() + ')');
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  check(await page.locator('#address').inputValue() === 'rus', 'and the completion is not re-added after a deletion');
  // Arrows move the highlight; Esc reverts to the typed text.
  await page.keyboard.type('t');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rust', entries: rows, inline: { text: 'rust-lang.org', url: 'https://www.rust-lang.org/', index: 1 } });
  await page.waitForTimeout(30);
  await page.keyboard.press('ArrowDown');
  await page.waitForTimeout(20);
  sel = await lastSent(page, 'addressSuggestSelect');
  check(sel && sel.selected === 2 && await page.locator('#address').inputValue() === 'https://docs.rs/', 'Down from the completed row moves to the next one');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(20);
  check(await page.locator('#address').inputValue() === 'rust', 'Esc reverts to the typed text');
  // A completion never lands when the caret is not at the end (an edit in the middle).
  await page.keyboard.press('Control+A');
  await page.keyboard.type('rus');
  await page.evaluate(() => { const a = document.getElementById('address'); a.setSelectionRange(1, 1); });
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'rus', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  check(await page.locator('#address').inputValue() === 'rus', 'no completion while the caret is inside the text');

  // 0.7.2 (R3): typing the next letters of the completion keeps it in place in the same frame;
  // the bar never shows the typed text alone or empties between keystrokes.
  await page.keyboard.press('Control+A');
  await page.keyboard.type('r');
  await page.waitForTimeout(120);
  await emit(page, { event: 'addressSuggestions', text: 'r', entries: rows, inline: inl });
  await page.waitForTimeout(30);
  await page.evaluate(() => {
    const a = document.getElementById('address');
    window.__barValues = [];
    a.addEventListener('input', () => window.__barValues.push(a.value));
    (function frame() { window.__barValues.push(a.value); window.__barRaf = requestAnimationFrame(frame); })();
  });
  for (const ch of ['u', 's', 't', '-']) {
    await page.keyboard.type(ch);
    const typed = await page.evaluate(() => { const a = document.getElementById('address'); return a.value.slice(0, a.selectionStart); });
    await page.waitForTimeout(90);
    await emit(page, { event: 'addressSuggestions', text: typed, entries: rows, inline: inl });
    await page.waitForTimeout(20);
  }
  const seen = await page.evaluate(() => { cancelAnimationFrame(window.__barRaf); return window.__barValues; });
  selRange = await page.evaluate(() => { const a = document.getElementById('address'); return [a.value, a.selectionStart, a.selectionEnd]; });
  check(seen.length > 4 && seen.every((v) => v === 'rust-lang.org'), 'five letters typed along the completion: every frame shows the whole completion (' + [...new Set(seen)].join(',') + ')');
  check(selRange[0] === 'rust-lang.org' && selRange[1] === 5 && selRange[2] === 13, 'the completion after the typed letters stays selected (' + JSON.stringify(selRange) + ')');
  check((await lastSent(page, 'addressSuggest')).text === 'rust-', 'the host is asked for the typed text, not the completion');
  // A host request to focus the bar while typing keeps the text and the caret.
  await emit(page, { event: 'focusAddressBar' });
  await page.waitForTimeout(20);
  selRange = await page.evaluate(() => { const a = document.getElementById('address'); return [a.value, a.selectionStart, a.selectionEnd]; });
  check(selRange[0] === 'rust-lang.org' && selRange[1] === 5, 'focusAddressBar while typing selects nothing new (' + JSON.stringify(selRange) + ')');
  await page.keyboard.type('l');
  await page.waitForTimeout(90);
  // The host no longer offers it: back to the typed text, caret at the end.
  await emit(page, { event: 'addressSuggestions', text: 'rust-l', entries: rows });
  await page.waitForTimeout(20);
  selRange = await page.evaluate(() => { const a = document.getElementById('address'); return [a.value, a.selectionStart, a.selectionEnd]; });
  check(selRange[0] === 'rust-l' && selRange[1] === 6 && selRange[2] === 6, 'a completion no longer offered goes away (' + JSON.stringify(selRange) + ')');
  // Paste never predicts; Delete drops the completion.
  await page.keyboard.press('Control+A');
  await page.keyboard.type('r');
  await page.waitForTimeout(90);
  await emit(page, { event: 'addressSuggestions', text: 'r', entries: rows, inline: inl });
  await page.waitForTimeout(20);
  await page.keyboard.press('Delete');
  await page.waitForTimeout(20);
  check(await page.locator('#address').inputValue() === 'r', 'Delete removes the completion');

  // 0.6.0: the address bar's context menu. The shell asks the host with the selection and
  // the edit state; the host's addressEdit events make the edits.
  await page.click('#address');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('hello world');
  // (A right-click inside the selection keeps it; the event is dispatched directly so the
  // test does not depend on where the glyphs fall.)
  const rightClick = () => page.evaluate(() => { const a = document.getElementById('address'); a.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 40, clientY: 10 })); });
  await page.evaluate(() => { const a = document.getElementById('address'); a.setSelectionRange(0, 5); });
  await rightClick();
  await page.waitForTimeout(30);
  const cm = await lastSent(page, 'showAddressMenu');
  check(cm && cm.selection === 'hello' && cm.hasText === true && cm.dirty === true && cm.x > 0 && cm.y > 0, 'right-click sends showAddressMenu with the selection and edit state (' + JSON.stringify(cm) + ')');
  await emit(page, { event: 'addressEdit', edit: { insert: { text: 'bye' } } });
  await page.waitForTimeout(130);
  check(await page.locator('#address').inputValue() === 'bye world', 'addressEdit insert replaces the selection with the clipboard text');
  check((await lastSent(page, 'addressSuggest')).text === 'bye world', 'and asks for suggestions again');
  await page.evaluate(() => { const a = document.getElementById('address'); a.setSelectionRange(0, 4); });
  await emit(page, { event: 'addressEdit', edit: 'deleteSelection' });
  await page.waitForTimeout(30);
  check(await page.locator('#address').inputValue() === 'world', 'addressEdit deleteSelection removes the selection (Cut/Delete)');
  await emit(page, { event: 'addressEdit', edit: 'selectAll' });
  await page.waitForTimeout(30);
  selRange = await page.evaluate(() => { const a = document.getElementById('address'); return [a.selectionStart, a.selectionEnd]; });
  check(selRange[0] === 0 && selRange[1] === 5, 'addressEdit selectAll selects everything');
  await rightClick();
  await page.waitForTimeout(30);
  const beforeEsc = (await sent(page)).length;
  await page.keyboard.press('Escape');
  await page.waitForTimeout(20);
  check((await sent(page)).slice(beforeEsc).some((m) => m.cmd === 'hidePopup') && await page.evaluate(() => document.activeElement === document.getElementById('address')), 'Esc closes the menu and keeps the field');
  // Copying the whole unedited address copies the full URL.
  await page.keyboard.press('Escape');
  await page.waitForTimeout(30);
  await page.click('#address');
  await page.waitForTimeout(30);
  const copied = await page.evaluate(() => {
    const a = document.getElementById('address'); a.select();
    let got = null;
    const e = new ClipboardEvent('copy', { clipboardData: new DataTransfer(), bubbles: true, cancelable: true });
    a.dispatchEvent(e);
    got = e.defaultPrevented ? e.clipboardData.getData('text/plain') : null;
    return got;
  });
  check(copied !== null && copied.startsWith('http'), 'copy of the whole unedited address gives the full URL (' + copied + ')');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(20);

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
  // A reorder under reduced motion: the tab still follows the cursor, the others do not slide,
  // and the drop re-lays the strip out at once.
  const r1 = await page.locator('.tab[data-id="1"]').boundingBox();
  const r3 = await page.locator('.tab[data-id="3"]').boundingBox();
  await page.mouse.move(r1.x + 20, r1.y + 10);
  await page.mouse.down();
  await page.mouse.move(r3.x + r3.width - 4, r3.y + 10, { steps: 6 });
  const rm = await page.$$eval('.tab', (ns) => ns.map((n) => ({ id: +n.dataset.id, t: n.style.transform, dragging: n.classList.contains('dragging') })));
  check(rm.find((t) => t.id === 1).dragging && rm.find((t) => t.id === 1).t !== '', 'reduced motion: the dragged tab still follows the cursor');
  await page.mouse.up();
  await page.waitForTimeout(20);
  const rmOrder = await page.$$eval('.tab', (ns) => ns.map((n) => +n.dataset.id));
  check(rmOrder[1] === 1 && await page.locator('.tab.settling').count() === 0, 'reduced motion: the drop re-lays the strip out at once (' + rmOrder.join(',') + ')');
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
  await page.waitForTimeout(400);
  const widths20 = await page.$$eval('.tab:not(.active)', (ns) => ns.map((n) => n.getBoundingClientRect().width));
  const minW = Math.min(...widths20), maxW = Math.max(...widths20);
  check(maxW - minW < 0.5, 'with 20 tabs the inactive ones have the same width (' + minW.toFixed(1) + '–' + maxW.toFixed(1) + ')');
  const activeW = await page.locator('.tab.active').evaluate((n) => n.getBoundingClientRect().width);
  check(activeW >= 119.5, '0.11.0: the active narrow tab keeps 120 DIP (' + activeW.toFixed(1) + ')');
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
  await page.waitForTimeout(300);
  const widths12 = await page.$$eval('.tab:not(.active)', (ns) => ns.map((n) => n.getBoundingClientRect().width));
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

  // 0.7.2 (R13): a newer Krunk puts a dot on ⋮; none removes it.
  await emit(page, { event: 'updateAvailable', version: '0.7.9' });
  await page.waitForTimeout(20);
  const dot = await page.evaluate(() => { const b = document.getElementById('menu'); return [b.classList.contains('has-update'), b.title, getComputedStyle(b, '::after').width]; });
  check(dot[0] && dot[1].includes('0.7.9') && dot[2] === '8px', 'updateAvailable shows a dot on the menu button (' + JSON.stringify(dot) + ')');
  await shot(page, 'shell-update-dot.png');
  await emit(page, { event: 'updateAvailable', version: null });
  await page.waitForTimeout(20);
  check(!(await page.evaluate(() => document.getElementById('menu').classList.contains('has-update'))), 'no update: no dot');

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
  await page.waitForTimeout(50);
  {
    const fitMenu = await lastSent(page, 'popupResize');
    const want = await page.evaluate(() => document.getElementById('root').offsetHeight + 2);
    check(fitMenu && fitMenu.height === want && want < 620 && want > 400, 'the menu asks for exactly its content height (' + (fitMenu && fitMenu.height) + ' = #root + borders, below the 620 bound)');
  }
  await page.waitForSelector('.item');
  check(await page.locator('#zoom-value').textContent() === '125%', 'menu shows zoom');
  // 0.6.0: Picture-in-Picture in the menu, greyed without a video on the page.
  check(await page.locator('.item', { hasText: 'Picture-in-Picture' }).isDisabled(), 'Picture-in-Picture is disabled without a video');
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1.25, canReopen: true, hasVideo: true } });
  await page.waitForTimeout(50);
  await page.locator('.item', { hasText: 'Picture-in-Picture' }).click();
  check((await lastSent(page, 'command')).command.command === 'pictureInPicture', 'the item sends command pictureInPicture');
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1.25, canReopen: true } });
  await page.waitForTimeout(50);
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
  check(await page.locator('#site-toggle.on[aria-checked="true"]').count() === 1 && (await page.textContent('.pop-title')) === 'Blocking ads and trackers', 'shield header: the site switch is on');
  await page.click('#site-toggle');
  const al = await lastSent(page, 'shieldAllowSite');
  check(al && al.allow === true, 'the switch sends shieldAllowSite{allow:true}');
  // Dark theme.
  await emit(page, { event: 'theme', theme: 'dark' });
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 37, allowed: true, enabled: true, recent: [
    { url: 'https://ads.example/a.js', host: 'ads.example', kind: 'script', count: 3, redirected: false },
    { url: 'https://px.example/p.gif', host: 'px.example', kind: 'image', count: 1, redirected: false },
  ], distinct: 2 } });
  await page.waitForTimeout(40);
  check(await page.locator('#site-toggle:not(.on)[aria-checked="false"]').count() === 1 && (await page.textContent('.pop-title')) === 'Ads allowed on this site', 'allowed site: switch off, title says so');
  check(await page.locator('.badge.kind-script').count() === 1 && await page.locator('.badge.kind-image').count() === 1, 'type badges per blocked address');
  await shot(page, 'popup-shield-dark.png');
  await emit(page, { event: 'theme', theme: 'light' });

  // The lock popup (site info), every state.
  const leafCert = { subject: 'example.com', subjectOrg: 'Example Org', issuer: 'DigiCert TLS RSA SHA256 2020 CA1', issuerOrg: 'DigiCert Inc', notBefore: 1735689600, notAfter: 1798761600, sans: ['example.com', 'www.example.com'], keyAlgorithm: 'RSA 2048 bits', signatureAlgorithm: 'SHA-256 with RSA', sha256: 'AA:BB:CC', serial: '01' };
  const caCert = { subject: 'DigiCert TLS RSA SHA256 2020 CA1', subjectOrg: 'DigiCert Inc', issuer: 'DigiCert Global Root CA', issuerOrg: 'DigiCert Inc', notBefore: 1600000000, notAfter: 1900000000, sans: [], keyAlgorithm: 'RSA 2048 bits', signatureAlgorithm: 'SHA-256 with RSA', sha256: 'DD:EE:FF', serial: '02' };
  ({ page, errors } = await make(360, 260));
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'example.com', kind: 'https', secure: true, warnings: [], protocol: 'TLS 1.3', cipher: null, certs: [leafCert, caCert], origin: 'https://example.com', permissions: [{ kind: 'camera', state: 'allow' }, { kind: 'microphone', state: 'ask' }, { kind: 'geolocation', state: 'block' }] } } });
  await page.waitForSelector('#site-permissions');
  check(await page.locator('#site-permissions .perm-select').count() === 3 && await page.inputValue('#site-permissions .perm-select[data-kind=camera]') === 'allow' && await page.inputValue('#site-permissions .perm-select[data-kind=geolocation]') === 'block', 'the lock popup lists the site\'s permissions with their states');
  await page.selectOption('#site-permissions .perm-select[data-kind=microphone]', 'block');
  await page.waitForTimeout(20);
  {
    const ps = await lastSent(page, 'permissionSet');
    check(ps && ps.origin === 'https://example.com' && ps.kind === 'microphone' && ps.state === 'block', 'changing one sends permissionSet{origin, kind, state}');
  }
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'example.com', kind: 'https', secure: true, warnings: [], protocol: 'TLS 1.3', cipher: null, certs: [leafCert, caCert] } } });
  await page.waitForSelector('.pop-title');
  check((await page.textContent('.pop-title')) === 'Connection is secure' && await page.locator('.pop-icon.ok').count() === 1, 'https: "Connection is secure"');
  check((await page.textContent('.panel')).includes('TLS 1.3') && (await page.textContent('.panel')).includes('not reported by the engine'), 'protocol shown, cipher "not reported by the engine"');
  check((await page.textContent('.cert.leaf')).includes('example.com, www.example.com') && (await page.textContent('.cert.leaf')).includes('AA:BB:CC'), 'leaf: SANs and fingerprint');
  let fitReq = await lastSent(page, 'popupResize');
  check(fitReq && fitReq.height > 100, 'the popup asks the host for its height (' + (fitReq && fitReq.height) + ')');
  check(await page.locator('#cert-chain').isHidden(), 'the chain is folded');
  await page.click('#cert-details');
  await page.waitForTimeout(20);
  check(await page.locator('#cert-chain').isVisible() && (await page.textContent('#cert-chain')).includes('DigiCert Global Root CA'), '"Certificate details" shows the issuer chain');
  const fitReq2 = await lastSent(page, 'popupResize');
  check(fitReq2 && fitReq2.height > fitReq.height, 'and asks for more height (' + fitReq2.height + ' > ' + fitReq.height + ')');
  await shot(page, 'popup-siteinfo-https.png');
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'expired.badssl.com', kind: 'https', secure: false, warnings: ['Certificate error: the certificate has expired or is not yet valid.'], protocol: 'TLS 1.2', cipher: null, certs: [Object.assign({}, leafCert, { subject: '*.badssl.com', notAfter: 1430000000 })] } } });
  await page.waitForSelector('.pop-card.warning');
  check((await page.textContent('.pop-title')) === 'Connection is not secure' && (await page.textContent('.pop-card.warning')).includes('expired') && (await page.textContent('.cert.leaf')).includes('(expired)'), 'certificate error: warning card and "(expired)"');
  await shot(page, 'popup-siteinfo-expired.png');
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'mixed.badssl.com', kind: 'https', secure: true, warnings: ['This page shows insecure images or media (mixed content).'], protocol: 'TLS 1.2', cipher: null, certs: [leafCert] } } });
  await page.waitForSelector('.pop-card.warning');
  check((await page.textContent('.pop-card.warning')).includes('mixed content') && await page.locator('.pop-icon.warn').count() === 1, 'mixed content: a warning');
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'neverssl.com', kind: 'http', secure: false, warnings: [], protocol: null, cipher: null, certs: [] } } });
  await page.waitForSelector('.plain-note');
  check((await page.textContent('.plain-note')) === 'Connection is not secure. No certificate.' && await page.locator('.cert').count() === 0, 'http: "Connection is not secure. No certificate."');
  await shot(page, 'popup-siteinfo-http.png');
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: '', kind: 'internal', secure: false, warnings: [], protocol: null, cipher: null, certs: [] } } });
  await page.waitForTimeout(20);
  check((await page.textContent('.pop-title')) === 'Krunk page', 'internal page: "Krunk page"');
  await emit(page, { event: 'theme', theme: 'dark' });
  await emit(page, { event: 'popup', content: { kind: 'siteInfo', info: { host: 'example.com', kind: 'https', secure: true, warnings: [], protocol: 'TLS 1.3', cipher: null, certs: [leafCert, caCert] } } });
  await page.waitForSelector('.pop-title');
  await shot(page, 'popup-siteinfo-dark.png');
  await emit(page, { event: 'theme', theme: 'light' });
  check(errors.length === 0, 'site info: no console errors (' + errors.join(' | ') + ')');

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
  // 0.11.2: the username is shown in a field and can be corrected before saving.
  ({ page, errors } = await make(360, 196));
  await emit(page, { event: 'popup', content: Object.assign({}, save, { update: true }) });
  await page.waitForSelector('#save-username');
  check(await page.inputValue('#save-username') === 'alice', 'the prompt shows the username in a field');
  await page.fill('#save-username', 'alice@bank.example');
  await shot(page, 'popup-update-password-username.png');
  await page.locator('button', { hasText: /^Update$/ }).click();
  r = await lastSent(page, 'savePasswordReply');
  check(r && r.accept === true && r.username === 'alice@bank.example', 'Update sends the edited username (' + JSON.stringify(r) + ')');
  check(errors.length === 0, 'save prompt: no console errors (' + errors.join(' | ') + ')');

  // 0.5: the local network access prompt (host-opened; Allow/Block, remembered unless private).
  ({ page, errors } = await make(380, 196));
  const perm = { kind: 'permission', tabId: 3, promptId: 41, origin: 'https://app.plex.tv', loopback: false, private: false };
  // 0.6.0: other kinds get their own wording; several kinds are listed together.
  await emit(page, { event: 'popup', content: Object.assign({}, perm, { kinds: ['camera', 'microphone'], promptId: 42 }) });
  await page.waitForTimeout(30);
  check((await page.textContent('h2')) === 'Allow camera and microphone?' && (await page.textContent('.panel p')).includes('use your camera and use your microphone'), 'camera+microphone prompt wording');
  await page.click('#default');
  await page.waitForTimeout(20);
  {
    const r = await lastSent(page, 'permissionReply');
    check(r && r.promptId === 42 && r.allow === true && r.remember === true, 'Allow replies with the prompt id and remember');
  }
  await emit(page, { event: 'popup', content: Object.assign({}, perm, { kinds: ['geolocation'], promptId: 43 }) });
  await page.waitForTimeout(30);
  check((await page.textContent('h2')) === 'Know your location?', 'geolocation prompt wording');
  await emit(page, { event: 'popup', content: Object.assign({}, perm, { kinds: ['notifications'], promptId: 44, private: true }) });
  await page.waitForTimeout(30);
  check((await page.textContent('h2')) === 'Show notifications?' && (await page.textContent('.panel')).includes('not remembered'), 'notifications prompt, private tab note');
  await emit(page, { event: 'popup', content: perm });
  await page.waitForSelector('.panel');
  await shot(page, 'popup-permission.png');
  check((await page.textContent('.panel h2')) === 'Connect to devices on your local network?' && (await page.textContent('.panel p')).startsWith('app.plex.tv wants'), 'local network prompt names the host');
  check(await page.isChecked('#remember') && await page.evaluate(() => document.activeElement && document.activeElement.textContent) === 'Allow', 'Remember is on and Allow has the focus');
  await page.locator('button', { hasText: /^Allow$/ }).click();
  let pr = await lastSent(page, 'permissionReply');
  check(pr && pr.promptId === 41 && pr.allow === true && pr.remember === true, 'Allow sends permissionReply{promptId, allow, remember}');
  await emit(page, { event: 'popup', content: perm });
  await page.waitForTimeout(20);
  await page.uncheck('#remember');
  await page.locator('button', { hasText: /^Block$/ }).click();
  pr = await lastSent(page, 'permissionReply');
  check(pr && pr.allow === false && pr.remember === false, 'Block without Remember');
  await emit(page, { event: 'popup', content: Object.assign({}, perm, { loopback: true, private: true }) });
  await page.waitForTimeout(20);
  check((await page.textContent('.panel h2')) === 'Connect to this computer?' && await page.locator('#remember').count() === 0 && (await page.textContent('.panel p.muted')).includes('not remembered'), 'loopback wording; a private tab has no Remember');
  await page.locator('button', { hasText: /^Allow$/ }).click();
  pr = await lastSent(page, 'permissionReply');
  check(pr && pr.allow === true && pr.remember === false, 'private: never remembered');

  // 0.5.1: address-bar suggestions (never focused; a click picks).
  ({ page, errors } = await make(560, 122));
  const addr = { kind: 'addressSuggest', selected: 1, entries: [
    { url: 'https://duckduckgo.com/?q=ru', title: 'ru', kind: 'search', fill: 'ru' },
    { url: 'https://www.rust-lang.org/', title: 'Rust', kind: 'bookmark', fill: 'https://www.rust-lang.org/' },
    { url: 'https://docs.rs/', title: 'Docs.rs', kind: 'history', fill: 'https://docs.rs/' },
  ] };
  await emit(page, { event: 'popup', content: addr });
  await page.waitForSelector('.suggest.addr');
  await shot(page, 'popup-address-suggest.png');
  check(await page.locator('.suggest.addr').count() === 3 && (await page.textContent('.suggest.addr:nth-child(1) .title')) === 'Search ru' && (await page.textContent('.suggest.addr:nth-child(3) .url')) === 'https://docs.rs/', 'three rows: search first, then the pages with their addresses');
  check(await page.locator('.suggest.addr.selected').count() === 1 && (await page.getAttribute('.suggest.addr.selected', 'data-index')) === '1', 'the shell\'s selection is highlighted');
  await page.locator('.suggest.addr').nth(2).click();
  const ap = await lastSent(page, 'addressSuggestPick');
  check(ap && ap.url === 'https://docs.rs/', 'a click picks the row');

  ({ page, errors } = await make(340, 236));
  await emit(page, { event: 'popup', content: { kind: 'fill', tabId: 9, origin: 'https://mail.example', entries: [{ id: 'e1', username: 'alice@mail.example' }, { id: 'e2', username: 'bob' }] } });
  await page.waitForSelector('.choice');
  await shot(page, 'popup-fill.png');
  await page.locator('.choice', { hasText: 'bob' }).click();
  const f = await lastSent(page, 'fillReply');
  check(f && f.entryId === 'e2' && f.tabId === 9 && f.origin === 'https://mail.example', 'choosing an entry sends fillReply{entryId,tabId,origin}');

  // 0.5: the suggestion dropdown under a login field: the usernames (a sister host named),
  // no password anywhere, and "Unlock Krunk passwords" while the vault is locked.
  await emit(page, { event: 'popup', content: { kind: 'suggest', tabId: 9, frameId: 'frame-7', origin: 'https://mail.example', locked: false, entries: [
    { id: 'e1', username: 'alice@mail.example' }, { id: 'e3', username: 'bob', host: 'accounts.mail.example' },
  ] } });
  await page.waitForSelector('.suggest');
  check(await page.locator('.suggest').count() === 2 && (await page.textContent('.suggest:nth-child(2) .host')) === 'accounts.mail.example', 'two suggestions, the domain match shows its host');
  check(!(await page.content()).includes('password') || !(await page.textContent('body')).toLowerCase().includes('secret'), 'no password in the dropdown');
  check(await page.evaluate(() => document.activeElement === document.body || document.activeElement === null), 'the dropdown takes no focus');
  await shot(page, 'popup-suggest.png');
  {
    // R18: the footer keeps space below it, and the popup asks for its full height.
    await page.waitForTimeout(40);
    const gap = await page.evaluate(() => { const f = document.querySelector('.suggest-foot').getBoundingClientRect(); const r = document.getElementById('root').getBoundingClientRect(); return Math.round(r.bottom - f.bottom); });
    const fit = await lastSent(page, 'popupResize');
    const need = await page.evaluate(() => document.getElementById('root').offsetHeight);
    check(gap >= 6 && fit && fit.height >= need, 'the suggestion list has room below its footer (' + gap + ' px) and fits its content (' + (fit && fit.height) + ' >= ' + need + ')');
  }
  {
    // 0.11.3: every login of the site, not only 8; past 8 rows the list scrolls (the host sizes
    // the window to 8.5 rows and the footer).
    const many = Array.from({ length: 14 }, (_, i) => ({ id: 'm' + i, username: 'user' + i + '@mail.example' }));
    await page.setViewportSize({ width: 320, height: Math.round(8.5 * 38 + 42) });
    await emit(page, { event: 'popup', content: { kind: 'suggest', tabId: 9, frameId: 'frame-7', origin: 'https://mail.example', locked: false, entries: many } });
    await page.waitForTimeout(60);
    const box = await page.evaluate(() => { const r = document.querySelector('.suggest-rows'); const f = document.querySelector('.suggest-foot').getBoundingClientRect(); return { n: r.querySelectorAll('.suggest').length, scrolls: r.scrollHeight > r.clientHeight + 4, footIn: f.bottom <= innerHeight + 1 }; });
    check(box.n === 14 && box.scrolls && box.footIn, 'all 14 logins listed, the rows scroll, the footer stays in view (' + JSON.stringify(box) + ')');
    await shot(page, 'popup-suggest-many.png');
    await emit(page, { event: 'popup', content: { kind: 'suggest', tabId: 9, frameId: 'frame-7', origin: 'https://mail.example', locked: false, entries: [
      { id: 'e1', username: 'alice@mail.example' }, { id: 'e3', username: 'bob', host: 'accounts.mail.example' },
    ] } });
    await page.waitForTimeout(40);
  }
  await page.locator('.suggest').first().click();
  const sp = await lastSent(page, 'suggestPick');
  check(sp && sp.entryId === 'e1' && sp.tabId === 9 && sp.frameId === 'frame-7', 'choosing sends suggestPick{entryId,tabId,frameId}');
  await emit(page, { event: 'popup', content: { kind: 'suggest', tabId: 9, frameId: 'frame-7', origin: 'https://mail.example', locked: true, entries: [] } });
  await page.waitForSelector('#suggest-unlock');
  check((await page.textContent('#suggest-unlock')).includes('Unlock Krunk passwords'), 'locked vault: one Unlock entry');
  await shot(page, 'popup-suggest-locked.png');
  await page.click('#suggest-unlock');
  const su = await lastSent(page, 'suggestUnlock');
  check(su && su.tabId === 9 && su.frameId === 'frame-7', 'Unlock sends suggestUnlock{tabId,frameId}');
  // 0.6.0: a generated password for a new-password field, and the "saved" toast after it.
  await emit(page, { event: 'popup', content: { kind: 'suggest', tabId: 9, frameId: 'frame-7', origin: 'https://mail.example', locked: false, entries: [{ id: 'e1', username: 'alice@mail.example' }], generated: 'Xq7#vLm2pR9wZk4tB8nH' } });
  await page.waitForSelector('#suggest-generate');
  check(await page.locator('.suggest').count() === 2 && (await page.textContent('#suggest-generate')).includes('Use a strong password') && (await page.textContent('#suggest-generate .mono')) === 'Xq7#vLm2pR9wZk4tB8nH', 'the dropdown offers the generated password after the logins');
  await shot(page, 'popup-suggest-generated.png');
  await page.click('#suggest-generate');
  const sg = await lastSent(page, 'suggestGenerate');
  check(sg && sg.tabId === 9 && sg.frameId === 'frame-7', 'choosing it sends suggestGenerate{tabId,frameId}');
  await emit(page, { event: 'popup', content: { kind: 'passwordSaved', tabId: 9, entryId: 'e9', origin: 'https://mail.example', username: 'newuser', update: false } });
  await page.waitForSelector('#saved-undo');
  check((await page.textContent('.panel.toast')).includes('Password saved for newuser on mail.example') && await page.locator('#saved-edit').count() === 1, 'the saved toast names the login with Undo and Edit');
  await shot(page, 'popup-password-saved.png');
  await page.click('#saved-undo');
  const un = await lastSent(page, 'passwordSavedUndo');
  check(un && un.entryId === 'e9', 'Undo sends passwordSavedUndo{entryId}');
  await emit(page, { event: 'popup', content: { kind: 'passwordSaved', tabId: 9, entryId: 'e9', origin: 'https://mail.example', username: 'newuser', update: true } });
  await page.waitForFunction(() => (document.querySelector('.panel.toast') || {}).textContent?.includes('Password updated'));
  check((await page.textContent('.panel.toast')).includes('Password updated'), 'an updated login says so');
  await page.click('#saved-edit');
  const ed = await lastSent(page, 'passwordSavedEdit');
  check(ed && ed.entryId === 'e9', 'Edit sends passwordSavedEdit{entryId}');
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
  // 0.7.0: the same prompt repairs a copy whose files are not this release's.
  await emit(page, { event: 'popup', content: { kind: 'update', version: '0.7.0', current: '0.7.0', install: true, hasNotes: false, state: { step: 'offered' },
    repair: 'libcef.dll is the old engine without H.264/AAC (Krunk 0.5.1 or earlier): an update did not replace it' } });
  await page.waitForTimeout(20);
  await shot(page, 'popup-repair.png');
  check((await page.textContent('h2')) === 'Repair Krunk 0.7.0' && (await page.textContent('.panel p')).includes('without H.264/AAC') && (await page.textContent('.panel p')).includes('reinstalls this version'), 'repair prompt names the problem and what Repair does');
  await page.locator('button', { hasText: 'Repair' }).click();
  check((await lastSent(page, 'updateReply')).choice === 'install', 'Repair sends updateReply{install}');
  check(errors.length === 0, 'update prompt: no console errors (' + errors.join(' | ') + ')');
}

async function testAbout(browser) {
  console.log('krunk://about (0.7.1: the engine command line)');
  const about = { version: '0.7.1', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk',
    arguments: '--no-pings --disable-features=AutofillServerCommunication',
    commandLine: '"C:\\Krunk\\Krunk.exe" --disable-features=GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders,AutofillServerCommunication --no-pings',
    integrity: { checked: true, healthy: true, summary: 'The installed files match this release', libcef: 'c4e38bbf9f097016' } };
  const { page, errors } = await openPage(browser, 'https://krunk.invalid/about.html', { width: 900, height: 700 }, 1,
    (a) => { window.__host.handlers.getAbout = () => a; }, about);
  await page.waitForFunction(() => document.getElementById('arguments').textContent.length > 0);
  check((await page.textContent('#arguments')) === about.commandLine, 'About shows the engine command line (CEF and Krunk features merged)');
  check((await page.textContent('table')).includes('Engine command line'), 'labelled as the engine command line');
  check(errors.length === 0, 'about: no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
}

async function testGpu(browser) {
  console.log('krunk://gpu (0.7.0: engine identity, generated codec text, Repair)');
  const base = { hardwareAcceleration: true, runningWithGpu: true, arguments: '--no-pings --disable-features=AutofillServerCommunication', gpuProcess: true,
    commandLine: '"C:\\Krunk\\Krunk.exe" --disable-features=GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders,AutofillServerCommunication --no-pings',
    cef: '156.0.15', chromium: '156.0.8078.80', widevine: null, protectedContent: false };
  // Playwright's Chromium has no H.264/AAC: the probe answers like Krunk's engine (or like the old one).
  const codecs = (h264) => { const real = MediaSource.isTypeSupported.bind(MediaSource);
    MediaSource.isTypeSupported = (t) => /avc1|mp4a/.test(t) ? h264 : /hev1|ac-3|ec-3/.test(t) ? false : real(t); };
  const healthy = Object.assign({}, base, { integrity: { checked: true, flavor: 'release', libcef: 'c4e38bbf9f097016', engineFiles: 232, problems: [], releaseChecked: true, healthy: true, summary: 'The installed files match this release', installed: true } });
  let { page, errors, external } = await openPage(browser, 'https://krunk.invalid/gpu.html', { width: 900, height: 1200 }, 1,
    (a) => { window.__host.handlers.getGpuInfo = () => a.info; (new Function('h', a.codecs))(true); }, { info: healthy, codecs: '(' + codecs.toString() + ')(h)' });
  await page.waitForFunction(() => document.getElementById('engine-build').textContent.includes('c4e38bbf'));
  const text = await page.textContent('#codec-text');
  check(text.startsWith("This is Krunk's own engine build with the proprietary codecs") && text.includes('H.264') && text.includes('AAC') && !text.includes('has no H.264'), 'codec text comes from the probe and the engine (no stale "no H.264" text)');
  check(text.includes('HEVC') && text.includes('Dolby'), 'HEVC and AC-3 explained as Windows matters');
  check(await page.locator('#codec-warning').isHidden() && await page.locator('#repair').isHidden(), 'healthy engine: no warning, no Repair');
  check((await page.textContent('#command-line')).includes('GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders,AutofillServerCommunication'), 'the real engine command line (one merged --disable-features) is shown');
  check((await page.textContent('#engine-files')).includes('match this release') && (await page.textContent('#engine-files')).includes('232 engine files'), 'engine files line');
  check(await page.locator('td.yes', { hasText: 'yes' }).count() >= 2 && await page.locator('#codecs th', { hasText: 'E-AC-3' }).count() === 1, 'codec table with E-AC-3');
  await shot(page, 'gpu-healthy.png');
  check(errors.length === 0, 'healthy: no console errors (' + errors.join(' | ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await page.context().close();
  // The operator's case: the old engine left behind by an update.
  const minimal = Object.assign({}, base, { integrity: { checked: true, flavor: 'minimal', libcef: '07168eacd81bc09a', engineFiles: 232, problems: [{ path: 'libcef.dll', kind: 'changed' }], releaseChecked: false, healthy: false,
    summary: 'libcef.dll is the old engine without H.264/AAC (Krunk 0.5.1 or earlier): an update did not replace it', installed: true } });
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/gpu.html', { width: 900, height: 1200 }, 1,
    (a) => { window.__host.handlers.getGpuInfo = () => a.info; (new Function('h', a.codecs))(false); }, { info: minimal, codecs: '(' + codecs.toString() + ')(h)' }));
  await page.waitForFunction(() => document.getElementById('engine-build').textContent.includes('07168eac'));
  check((await page.textContent('#codec-text')).includes('old engine without H.264') && await page.locator('#codec-warning').isVisible(), 'old engine: says so and warns');
  check(await page.locator('#repair').isVisible() && (await page.textContent('#repair')) === 'Repair', 'Repair button shown');
  await page.click('#repair');
  await page.waitForTimeout(30);
  check(!!(await lastSent(page, 'repairEngine')) && await page.locator('#repair').isDisabled(), 'Repair sends repairEngine once');
  await shot(page, 'gpu-minimal.png');
  check(errors.length === 0, 'minimal: no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
  // Not checked yet (the check runs 20 s after start).
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/gpu.html', { width: 900, height: 1200 }, 1,
    (a) => { window.__host.handlers.getGpuInfo = () => a; }, Object.assign({}, base, { integrity: { checked: false } })));
  await page.waitForFunction(() => document.getElementById('engine-build').textContent.includes('checking'));
  check(await page.locator('#repair').isHidden(), 'not checked yet: no Repair');
  check(errors.length === 0, 'unchecked: no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
}

const DAY = 86400000;

// 0.7.0 (R2/R3): the Customize panel's wallpapers and colour picker; light wallpapers get dark text.
async function testNewTabBackgrounds(browser) {
  console.log('new tab page: wallpapers and colour (R2, R3)');
  const setup = () => {
    const h = window.__host.handlers;
    const wallpapers = [['aurora', 'Aurora', 'dark', 25], ['dunes', 'Dunes', 'light', 0]].map(([id, title, look, dim]) => ({ id, title, look, dim, thumb: 'https://krunk.invalid/shared/wallpapers/' + id + '-thumb.webp' }));
    window.__ntp = { background: 'none', backgroundColor: '', image: '', imageDim: 30, imageBlur: 0, wallpaper: '', tiles: 'mostVisited', columns: 6, rows: 2, showShortcuts: true, showSearch: true, showTiles: true };
    const cfg = () => {
      const w = window.__ntp.background === 'wallpaper' ? wallpapers.find((x) => x.id === window.__ntp.wallpaper) : null;
      return { searchName: 'Google', shortcuts: [['New tab', 'Ctrl+T']], ntp: window.__ntp, imageUrl: null, wallpapers,
        wallpaperUrl: w ? 'https://krunk.invalid/shared/wallpapers/' + w.id + '.webp' : null, wallpaperLook: w ? w.look : null };
    };
    h.getTopSites = () => [{ url: 'https://example.org/', title: 'Example', pinned: false, favicon: null }];
    h.getNewTabConfig = cfg;
    h.setNtpOptions = (m) => { window.__ntp = m.ntp; return cfg(); };
  };
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/newtab.html', { width: 1100, height: 700 }, 1, setup);
  await page.waitForSelector('.tile:not(.add)');
  await page.click('#customize');
  check(await page.locator('#wallpapers .wallpaper').count() === 2, 'the Customize panel shows the wallpaper thumbnails');
  await page.click('#wallpapers .wallpaper[data-id="aurora"]');
  await page.waitForTimeout(60);
  let o = await lastSent(page, 'setNtpOptions');
  check(o && o.ntp.background === 'wallpaper' && o.ntp.wallpaper === 'aurora' && o.ntp.imageDim === 25, 'a thumbnail chooses the wallpaper with its darkening');
  let st = await page.evaluate(() => ({ img: getComputedStyle(document.getElementById('bg')).backgroundImage, has: document.body.classList.contains('has-image'), light: document.body.classList.contains('image-light'), dim: getComputedStyle(document.getElementById('dim')).opacity }));
  check(st.img.includes('/shared/wallpapers/aurora.webp') && st.has && !st.light && st.dim === '0.25', 'the page shows it, darkened, with light text (' + JSON.stringify(st) + ')');
  const darkBtn = await page.evaluate(() => getComputedStyle(document.getElementById('customize')).color);
  check(darkBtn === 'rgb(255, 255, 255)', 'the Customize button has light text on a dark wallpaper (' + darkBtn + ')');
  await shot(page, 'newtab-wallpaper-dark.png');
  await page.click('#wallpapers .wallpaper[data-id="dunes"]');
  await page.waitForTimeout(60);
  st = await page.evaluate(() => ({ light: document.body.classList.contains('image-light'), color: getComputedStyle(document.querySelector('.tile')).color }));
  check(st.light && st.color === 'rgb(31, 41, 55)', 'a light wallpaper gets dark text (' + JSON.stringify(st) + ')');
  const btn = () => page.evaluate(() => { const c = getComputedStyle(document.getElementById('customize')); return c.color + ' on ' + c.backgroundColor; });
  check(await btn() === 'rgb(31, 41, 55) on rgba(255, 255, 255, 0.85)', 'the Customize button stays readable on a light wallpaper (' + await btn() + ')');
  await shot(page, 'newtab-wallpaper-light.png');
  // The colour picker: a swatch and the dialog.
  await page.click('.color-row .swatch[data-color="#1e1b4b"]');
  await page.waitForTimeout(60);
  o = await lastSent(page, 'setNtpOptions');
  check(o.ntp.background === 'color' && o.ntp.backgroundColor === '#1e1b4b', 'a colour swatch makes a colour background');
  check(await page.evaluate(() => getComputedStyle(document.getElementById('bg')).backgroundColor) === 'rgb(30, 27, 75)', 'and the page shows it');
  await page.click('.color-row .reset-color');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'setNtpOptions')).ntp.background === 'none', 'Reset leaves no background');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await page.context().close();
}

// 0.7.0 (R1): the operator's long tile titles never widen the grid or the page.
async function testNewTabLongTitles(browser) {
  console.log('new tab page: long titles (R1)');
  const titles = [
    ['https://plique.example/', 'Home — PLIQUE DMA'], ['https://www.youtube.com/', 'YouTube'],
    ['https://perfectaim.example/', 'PerfectAim - Undetected Game Cheats'], ['https://azeroth.example/', 'Azeroth Survivors'],
    ['https://login.example/', 'Login'], ['https://app.plex.tv/', 'Plex'],
    ['https://duckduckgo.com/?q=perfectaim', 'perfectaim at DuckDuckGo'], ['https://canyoublockit.com/', 'Can You Block It ? - A Simple Ad Block Tester'],
    ['https://www.youtube.com/feed', 'YouTube'], ['https://adblock-tester.com/', 'Ad Blocker Test | Check if Your Ad Blocker Works'],
    ['https://rabbitrun.example/', 'RabbitRun'],
    ['https://forums.plex.tv/t/solved', 'SOLVED New Media Server or New Firefox version causing Error Playing Video in Browser | Plex Forum'],
  ].map(([url, title]) => ({ url, title, pinned: false, favicon: null }));
  const setup = (t) => {
    const h = window.__host.handlers;
    h.getTopSites = () => t;
    h.getNewTabConfig = () => ({ searchName: 'Google', shortcuts: [['New tab', 'Ctrl+T']], ntp: { background: 'none', backgroundColor: '', image: '', imageDim: 30, imageBlur: 0, tiles: 'mostVisited', columns: 6, rows: 2, showShortcuts: true, showSearch: true, showTiles: true }, imageUrl: null });
  };
  let worst = [];
  const matrix = [];
  for (const width of [800, 1024, 1280, 1920, 2560]) {
    for (const zoom of [1, 1.25, 1.5]) {
      const { page, errors } = await openPage(browser, 'https://krunk.invalid/newtab.html', { width: Math.round(width / zoom), height: 900 }, zoom, setup, titles);
      await page.waitForSelector('.tile:not(.add)');
      const r = await page.evaluate(() => {
        const doc = document.documentElement, main = document.querySelector('main').getBoundingClientRect();
        const tiles = [...document.querySelectorAll('.tile')].map((t) => t.getBoundingClientRect());
        const labels = [...document.querySelectorAll('.tile .label')];
        const lineH = parseFloat(getComputedStyle(labels[0]).lineHeight);
        return {
          hscroll: doc.scrollWidth > doc.clientWidth,
          outside: tiles.filter((b) => b.left < main.left - 0.5 || b.right > main.right + 0.5 || b.right > doc.clientWidth).length,
          widths: [...new Set(tiles.map((b) => Math.round(b.width)))],
          maxLines: Math.max(...labels.map((l) => Math.round(l.getBoundingClientRect().height / lineH))),
          longTip: document.querySelector('.tile[data-url="https://forums.plex.tv/t/solved"]').title,
        };
      });
      const ok = !r.hscroll && r.outside === 0 && r.widths.length === 1 && r.maxLines <= 2 && r.longTip.startsWith('SOLVED New Media Server or New Firefox version causing Error Playing Video');
      if (!ok) worst.push(width + 'px@' + zoom + ': ' + JSON.stringify(r));
      matrix.push(Object.assign({ width: width, zoom: zoom, ok: ok }, r));
      if (SHOTS && [800, 1280, 2560].includes(width)) await shot(page, 'newtab-long-titles-' + width + '-' + Math.round(zoom * 100) + '.png');
      if (errors.length) worst.push('errors: ' + errors.join(' | '));
      await page.context().close();
    }
  }
  if (SHOTS) fs.writeFileSync(path.join(SHOTS, 'newtab-long-titles.json'), JSON.stringify(matrix, null, 1));
  check(worst.length === 0, 'long titles at 800–2560 px and 100/125/150 %: no horizontal scrollbar, every tile inside the column, one tile width, at most 2 lines, the full title in the tooltip' + (worst.length ? ' (' + worst.join('; ') + ')' : ''));
}

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
    (t) => {
      const h = window.__host.handlers;
      h.getTopSites = () => t;
      const cfg = () => ({ searchName: 'Ecosia', shortcuts: [['New tab', 'Ctrl+T'], ['Task manager', 'Shift+Esc']], ntp: window.__ntp || { background: 'none', backgroundColor: '', image: '', imageDim: 30, imageBlur: 0, tiles: 'mostVisited', columns: 6, rows: 2, showShortcuts: true, showSearch: true, showTiles: true }, imageUrl: null });
      h.getNewTabConfig = cfg;
      h.setNtpOptions = (m) => { window.__ntp = m.ntp; return cfg(); };
      h.reorderTiles = () => null;
      h.addTile = (m) => { if (!/^https?:/.test(m.url)) throw new Error('Enter a web address'); return null; };
      h.editTile = () => true;
    }, tiles);
  await page.waitForSelector('.tile:not(.add)');
  await page.waitForFunction(() => document.getElementById('q').placeholder.includes('Ecosia'));
  check((await page.getAttribute('#q', 'placeholder')) === 'Search with Ecosia or enter address', 'the search box names the default engine (getNewTabConfig)');
  // 0.6.0: the shortcut-keys section, hideable.
  check(await page.locator('#keys').isVisible() && await page.locator('#keys-list li').count() === 2 && (await page.textContent('#keys-list')).includes('Shift+Esc'), 'the keyboard-shortcut section lists the host\'s shortcuts');
  await page.click('#keys-hide');
  await page.waitForTimeout(40);
  {
    const o = await lastSent(page, 'setNtpOptions');
    check(o && o.ntp.showShortcuts === false && await page.locator('#keys').isHidden(), '"Hide" saves showShortcuts=false and hides the section');
  }
  // The customize panel: background colour, grid, tile source, show/hide.
  await page.click('#customize');
  await page.waitForTimeout(40);
  check(await page.locator('#panel').evaluate((d) => d.open), 'Customize opens the panel');
  await page.fill('#bg-color', '#112233');
  await page.dispatchEvent('#bg-color', 'change');
  await page.waitForTimeout(40);
  {
    const o = await lastSent(page, 'setNtpOptions');
    check(o && o.ntp.background === 'color' && o.ntp.backgroundColor === '#112233', 'a colour saves background=color');
    const bg = await page.evaluate(() => getComputedStyle(document.getElementById('bg')).backgroundColor);
    check(bg === 'rgb(17, 34, 51)', 'and the page shows it (' + bg + ')');
  }
  await page.waitForTimeout(150); // the colour's reply re-fills the panel; let it land first
  await page.fill('#cols', '4');
  await page.dispatchEvent('#cols', 'change');
  await page.waitForTimeout(40);
  {
    const o = await lastSent(page, 'setNtpOptions');
    const v = await page.evaluate(() => document.getElementById('tiles').style.getPropertyValue('--cols'));
    check(o.ntp.columns === 4 && v === '4', 'columns change the grid variable (' + JSON.stringify(o.ntp) + ' --cols=' + v + ')');
  }
  await page.check('input[name=tiles][value=custom]');
  await page.waitForTimeout(40);
  check((await lastSent(page, 'setNtpOptions')).ntp.tiles === 'custom', 'the tile source is saved');
  await page.uncheck('#show-search');
  await page.waitForTimeout(40);
  check((await lastSent(page, 'setNtpOptions')).ntp.showSearch === false && await page.locator('#search').isHidden(), 'the search box can be hidden');
  await page.check('#show-search');
  await page.waitForTimeout(40);
  await page.click('#panel-close');
  await page.waitForTimeout(40);
  // Add a shortcut by hand (force add: the host only parses the address).
  await page.click('#add-tile');
  await page.waitForTimeout(40);
  await page.fill('#tile-name', 'My site');
  await page.fill('#tile-url', 'https://my.example/');
  await page.click('#tile-save');
  await page.waitForTimeout(60);
  {
    const a = await lastSent(page, 'addTile');
    check(a && a.url === 'https://my.example/' && a.title === 'My site', 'the Add dialog sends addTile{url,title}');
  }
  await page.click('#add-tile');
  await page.waitForTimeout(40);
  await page.fill('#tile-url', 'not a url');
  await page.click('#tile-save');
  await page.waitForTimeout(60);
  check((await page.textContent('#tile-error')).includes('web address'), 'a refused address shows the host\'s message');
  await page.click('#tile-cancel');
  // Edit: the pencil on a tile.
  await page.hover('.tile:not(.add) >> nth=0');
  await page.click('.tile:not(.add) >> nth=0 >> .icon-btn[title=Edit]');
  await page.waitForTimeout(40);
  await page.fill('#tile-name', 'Renamed');
  await page.click('#tile-save');
  await page.waitForTimeout(60);
  {
    const e = await lastSent(page, 'editTile');
    check(e && e.url === 'https://www.rust-lang.org/' && e.title === 'Renamed' && e.newUrl === 'https://www.rust-lang.org/', 'Edit sends editTile');
  }
  // Drag the third tile before the first: the new order goes to the host.
  await page.evaluate(() => {
    const t = document.querySelectorAll('.tile:not(.add)');
    const dt = new DataTransfer();
    t[2].dispatchEvent(new DragEvent('dragstart', { bubbles: true, dataTransfer: dt }));
    const r = t[0].getBoundingClientRect();
    t[0].dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: dt, clientX: r.left + 2, clientY: r.top + 2 }));
    t[0].dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt, clientX: r.left + 2, clientY: r.top + 2 }));
    t[2].dispatchEvent(new DragEvent('dragend', { bubbles: true, dataTransfer: dt }));
  });
  await page.waitForTimeout(60);
  {
    const o = await lastSent(page, 'reorderTiles');
    check(o && o.urls[0] === 'https://en.wikipedia.org/wiki/Main_Page' && o.urls[1] === 'https://www.rust-lang.org/', 'dropping a tile before the first sends the new order (' + (o && o.urls.slice(0, 3).join(', ')) + ')');
  }
  check((await page.locator('.tile:not(.add)').count()) === 10, 'renders 10 tiles (plus the Add tile)');
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
  // 0.7.2 (R11): a 300-character address and a long title stay on one line, shortened, with the
  // whole text in the tooltip; the page never scrolls sideways and the rows keep their height.
  const longUrl = 'https://very-long.example.org/' + 'path/'.repeat(54) + '?q=end';
  const longTitle = 'A very long bookmark title that goes on and on '.repeat(4).trim();
  await page.evaluate((x) => {
    const h = window.__host.handlers;
    const base = h.getBookmarks;
    h.getBookmarks = () => { const t = JSON.parse(JSON.stringify(base())); t[1].children.push({ id: 99, parent: 2, isFolder: false, title: x.t, url: x.u, position: 9, added: 0 }); return t; };
    document.dispatchEvent(new Event('visibilitychange'));
  }, { t: longTitle, u: longUrl });
  await page.waitForTimeout(150);
  await page.locator('.folder', { hasText: 'Other bookmarks' }).click();
  await page.waitForSelector('#items .row[data-id="99"]');
  for (const w of [1100, 760]) {
    await page.setViewportSize({ width: w, height: 600 });
    await page.waitForTimeout(60);
    const m = await page.evaluate(() => {
      const r = document.querySelector('#items .row[data-id="99"]');
      const other = document.querySelector('#items .row[data-id="21"]');
      const a = r.querySelector('a'), sub = r.querySelector('.sub');
      return { scroll: document.scrollingElement.scrollWidth - document.scrollingElement.clientWidth, h: r.offsetHeight, h0: other.offsetHeight,
        rowRight: r.getBoundingClientRect().right, card: document.getElementById('items').getBoundingClientRect().right,
        titleOne: a.scrollWidth > a.clientWidth && a.offsetHeight < 30, subOne: sub.scrollWidth > sub.clientWidth && sub.offsetHeight < 30,
        tip: a.title, subTip: sub.title };
    });
    check(m.scroll <= 0 && m.h === m.h0 && m.rowRight <= m.card + 0.5, 'bookmarks page at ' + w + ' px with a 300-character address: no sideways scroll, same row height (' + JSON.stringify([m.scroll, m.h, m.h0]) + ')');
    check(m.titleOne && m.subOne && m.tip === longTitle + '\n' + longUrl && m.subTip === longUrl, 'title and address are one line each, shortened, with the whole text in the tooltip (' + w + ' px)');
  }
  await shot(page, 'bookmarks-long-url.png');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testDownloads(browser) {
  console.log('downloads page');
  const now = Date.now();
  const items = [
    { id: 3, url: 'https://releases.example.org/tool-2.4.1-x64.msi', fileName: 'tool-2.4.1-x64.msi', path: 'C:\\Users\\me\\Downloads\\tool-2.4.1-x64.msi', state: 'inProgress', received: 23500000, total: 61200000, started: now, reason: null },
    { id: 2, url: 'https://arxiv.org/pdf/1234.5678?download=1&token=abc', fileName: 'paper.pdf', path: 'C:\\Users\\me\\Downloads\\paper.pdf', state: 'completed', received: 2400000, total: 2400000, started: Date.UTC(2026, 10, 5, 9, 15), finished: Date.UTC(2026, 10, 5, 9, 16), pageUrl: 'https://arxiv.org/abs/1234.5678', reason: null },
    { id: 1, url: 'https://cdn.example.com/video.mp4', fileName: 'video.mp4', path: 'C:\\Users\\me\\Downloads\\video.mp4', state: 'interrupted', received: 1000, total: null, started: now - 120000, reason: 'Network error' },
  ];
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/downloads.html', { width: 1000, height: 420 }, 1,
    (d) => { window.__host.handlers.getDownloads = () => d; }, items);
  await page.waitForSelector('.dl');
  check((await page.locator('.dl').count()) === 3, 'lists 3 downloads');
  check((await page.locator('.dl-bar').count()) === 1, 'progress bar for the running download');
  const kinds = await page.$$eval('.dl .dl-icon', (ns) => ns.map((n) => n.dataset.kind));
  check(JSON.stringify(kinds) === JSON.stringify(['app', 'pdf', 'video']), 'file-type icons by extension (' + kinds.join(', ') + ')');
  check(await page.locator('.dl[data-id="2"] .url').textContent() === 'https://arxiv.org/pdf/1234.5678?download=1&token=abc', 'the full download address is shown');
  const when2 = await page.locator('.dl[data-id="2"] .when').textContent();
  check(/2026/.test(when2) && /finished/.test(when2), 'date and time it started and finished (' + when2 + ')');
  check(!/finished/.test(await page.locator('.dl[data-id="1"] .when').textContent()), 'an old entry without a finish time shows the start only');
  check(await page.locator('.dl[data-id="1"] button', { hasText: 'Retry' }).count() === 1, 'a failed download offers Retry');
  await shot(page, 'downloads.png');
  // Right-click: the row's menu.
  await page.locator('.dl[data-id="2"] .name').click({ button: 'right' });
  await page.waitForSelector('.ctx-menu');
  const mlabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
  check(JSON.stringify(mlabels) === JSON.stringify(['Open file', 'Show in folder', 'Copy download link', 'Copy page address', 'Download again', 'Remove from list', 'Delete file…']), 'context menu of a finished download (' + mlabels.join(', ') + ')');
  await shot(page, 'downloads-context-menu.png');
  await page.locator('.ctx-menu .menu-item', { hasText: 'Copy download link' }).click();
  check((await lastSent(page, 'downloadAction')).action === 'copyLink' && await page.locator('.ctx-menu').count() === 0, 'Copy download link sends downloadAction{copyLink} and closes the menu');
  await page.locator('.dl[data-id="2"] .name').click({ button: 'right' });
  await page.locator('.ctx-menu .menu-item', { hasText: 'Delete file' }).click();
  await page.waitForSelector('#lb-dialog-ok');
  check((await page.textContent('.lb-dialog-text')).includes('paper.pdf'), 'Delete file asks first');
  await page.click('#lb-dialog-ok');
  await page.waitForTimeout(50);
  check((await lastSent(page, 'downloadAction')).action === 'deleteFile', 'confirmed: downloadAction{deleteFile}');
  // A failed one: no file actions, no page address known; keyboard opens the menu too.
  await page.locator('.dl[data-id="1"]').focus();
  await page.waitForTimeout(100); // focusing scrolls the row into view (a scroll closes menus)
  await page.keyboard.press('Shift+F10');
  await page.waitForSelector('.ctx-menu');
  const flabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
  check(JSON.stringify(flabels) === JSON.stringify(['Open file(off)', 'Show in folder(off)', 'Copy download link', 'Copy page address(off)', 'Retry', 'Remove from list', 'Delete file…(off)']), 'context menu of a failed download (' + flabels.join(', ') + ')');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Escape');
  check(await page.locator('.ctx-menu').count() === 0, 'Esc closes the menu');
  // A running one: pause (or resume) and cancel first; no file actions.
  const rowBefore = await page.$('.dl[data-id="3"]');
  await emit(page, { event: 'downloads', items: [Object.assign({}, items[0], { state: 'inProgress', received: 100, total: 1000, connections: 4, speed: 2048, eta: 75, partProgress: [{ done: 100, len: 250, start: 0, conn: 0 }, { done: 0, len: 250, start: 250, conn: 1 }, { done: 0, len: 250, start: 500, conn: 2 }, { done: 0, len: 250, start: 750, conn: 3 }] })].concat(items.slice(1)) });
  await page.waitForTimeout(40);
  check((await page.textContent('.row.dl .sub')).includes('2.0 KB/s') && (await page.textContent('.row.dl .sub')).includes('1 min left') && (await page.textContent('.row.dl .sub')).includes('4 connections'), 'the page shows speed, time left and the connections (' + await page.textContent('.row.dl .sub') + ')');
  const segs = await page.$$eval('.row.dl .dl-segments .seg', (ns) => ns.map((n) => n.style.left + '/' + n.style.width + '/' + n.className + '/' + Number(/scaleX\(([\d.]+)\)/.exec(n.firstChild.style.transform)[1])));
  check(segs.length === 4 && segs[0] === '0%/25%/seg c0/0.4' && segs[2] === '50%/25%/seg c2/0', 'the page places each part at its offset, coloured by connection, filled per part (' + segs.join(' ; ') + ')');
  // A part split while it runs: the new layout shows where the pieces are.
  await emit(page, { event: 'downloads', items: [Object.assign({}, items[0], { state: 'inProgress', received: 400, total: 1000, connections: 4, partProgress: [{ done: 250, len: 250, start: 0, conn: 0 }, { done: 100, len: 120, start: 250, conn: 1 }, { done: 0, len: 130, start: 370, conn: 0 }, { done: 50, len: 250, start: 500, conn: 2 }, { done: 0, len: 250, start: 750, conn: 3 }] })].concat(items.slice(1)) });
  await page.waitForTimeout(40);
  const split = await page.$$eval('.row.dl .dl-segments .seg', (ns) => ns.map((n) => n.style.left + '/' + n.style.width));
  check(split.length === 5 && split[2] === '37%/13%', 'a split part shows as two pieces at their offsets (' + split.join(' ; ') + ')');
  await emit(page, { event: 'downloads', items: [Object.assign({}, items[0], { state: 'inProgress', received: 100, connections: 4 })].concat(items.slice(1)) });
  await page.waitForTimeout(50);
  check((await page.locator('.dl[data-id="3"] .sub').textContent()).includes('4 connections'), 'a download in parts shows its connections');
  await page.locator('.dl[data-id="3"] .name').click({ button: 'right' });
  const rlabels = await page.$$eval('.ctx-menu .menu-item', (ns) => ns.map((n) => n.textContent + (n.disabled ? '(off)' : '')));
  check(JSON.stringify(rlabels) === JSON.stringify(['Pause', 'Cancel', 'Copy download link', 'Copy page address(off)']), 'context menu of a running download (' + rlabels.join(', ') + ')');
  await page.locator('.ctx-menu .menu-item', { hasText: 'Pause' }).click();
  check((await lastSent(page, 'downloadAction')).action === 'pause', 'Pause from the menu');
  await page.mouse.click(5, 5);
  check(await page.locator('.ctx-menu').count() === 0, 'a click elsewhere closes the menu');
  await page.locator('.dl[data-id="2"] button', { hasText: 'Show in folder' }).click();
  const a = await lastSent(page, 'downloadAction');
  check(a && a.id === 2 && a.action === 'showInFolder', 'show in folder sends downloadAction');
  await page.locator('.dl[data-id="3"] button', { hasText: 'Cancel' }).click();
  check((await lastSent(page, 'downloadAction')).action === 'cancel', 'cancel sends downloadAction{cancel}');
  items[0].state = 'completed'; items[0].received = items[0].total;
  await emit(page, { event: 'downloads', items });
  await page.waitForTimeout(50);
  check((await page.locator('.dl-bar, .dl-segments').count()) === 0, 'pushed update re-renders the list');
  check(rowBefore && await rowBefore.evaluate((n) => n.isConnected && n.dataset.id === '3') === false, 'a row that changes state is rebuilt');
  // Rows that only progress stay the same element (no flicker, hover kept).
  const keep = await page.$('.dl[data-id="2"]');
  await emit(page, { event: 'downloads', items });
  await page.waitForTimeout(30);
  check(await keep.evaluate((n) => n.isConnected), 'an unchanged row is kept in place');
  await emit(page, { event: 'downloads', items: [] });
  await page.waitForTimeout(30);
  check((await page.textContent('.empty strong')) === 'No downloads yet' && await page.locator('.dl').count() === 0, 'an empty list says so');
  await shot(page, 'downloads-empty.png');
  await emit(page, { event: 'downloads', items });
  await page.waitForTimeout(30);
  await page.click('#clear');
  check((await lastSent(page, 'clearDownloads')) !== undefined, 'clear list sends clearDownloads');
  check(external.length === 0, 'no network requests');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testTasks(browser) {
  console.log('task manager page');
  const rows = [
    { id: 1, kind: 'browser', title: 'Browser', pid: 4321, cpu: 1.5, memory: 150e6, workingSet: 160e6, private: 140e6, network: 0, killable: false, tabs: [] },
    { id: 2, kind: 'gpu', title: 'GPU Process', pid: 4400, cpu: 0.2, memory: 60e6, workingSet: 70e6, private: 50e6, network: 0, killable: true, tabs: [] },
    { id: 3, kind: 'renderer', title: 'Tab: Example Domain', pid: 5000, cpu: 12.25, memory: 90e6, workingSet: 95e6, private: 80e6, network: 20480, killable: true, tabs: [{ id: 7, title: 'Example Domain' }, { id: 8, title: 'Docs' }] },
    { id: 4, kind: 'utility', title: 'Utility: Network Service', pid: 4500, cpu: 0.0, memory: 30e6, workingSet: 32e6, private: 25e6, network: 0, killable: true, tabs: [] },
  ];
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/tasks.html', { width: 1000, height: 600 }, 1,
    (r) => { window.__host.handlers.getTasks = () => r; window.__host.handlers.killTask = (m) => { window.__host.killed = m.id; return { ended: true }; }; }, rows);
  await page.waitForSelector('#rows tr');
  check(await page.locator('#rows tr').count() === 4, 'renders the 4 tasks');
  check((await page.textContent('#summary')).includes('4 tasks') && (await page.textContent('#summary')).includes('1 renderer'), 'summary counts tasks and renderers');
  const rendererRow = page.locator('#rows tr[data-kind=renderer]');
  check((await rendererRow.textContent()).includes('Example Domain, Docs') && (await rendererRow.textContent()).includes('(2 tabs)') && (await rendererRow.textContent()).includes('20.0 KB/s') && (await rendererRow.textContent()).includes('12.3 %') && (await rendererRow.textContent()).includes('5000'), 'a renderer row lists its tabs, network rate, CPU and PID');
  check(await page.locator('#end').isDisabled(), 'End process is disabled with nothing selected');
  await page.click('#rows tr[data-kind=browser]');
  await page.waitForTimeout(20);
  check(await page.locator('#end').isDisabled() && (await page.textContent('#end')).includes('not the browser'), 'the browser process cannot be ended');
  await page.click('#rows tr[data-kind=renderer]');
  await page.waitForTimeout(20);
  check(!(await page.locator('#end').isDisabled()), 'a renderer can be ended');
  // R19: the selected row is readable in both themes (WCAG AA for its text, the muted columns too).
  for (const theme of ['light', 'dark']) {
    await page.evaluate((t) => LB.applyTheme(t, '', null), theme);
    await page.mouse.move(5, 5);
    await page.waitForTimeout(40);
    const ratios = await page.evaluate(() => {
      // (color-mix() computes to color(srgb r g b) with 0..1 channels.)
      const parse = (c) => { const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)/.exec(c); if (m) return m.slice(1, 4).map(Number); return /color\(srgb ([\d.]+) ([\d.]+) ([\d.]+)/.exec(c).slice(1, 4).map((v) => Number(v) * 255); };
      const lum = (c) => { const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]); };
      const tr = document.querySelector('#rows tr.selected');
      const bg = parse(getComputedStyle(tr).backgroundColor);
      return [...tr.querySelectorAll('td')].map((td) => {
        const st = getComputedStyle(td);
        let fg = parse(st.color);
        const o = Number(st.opacity);
        fg = fg.map((v, i) => v * o + bg[i] * (1 - o));
        const a = lum(fg), b = lum(bg);
        return Math.round((Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) * 100) / 100;
      });
    });
    check(ratios.every((r) => r >= 4.5), theme + ': the selected row reads at AA (' + ratios.join(', ') + ')');
    await shot(page, 'tasks-selected-' + theme + '.png');
  }
  await page.evaluate(() => LB.applyTheme('light', '', null));
  await page.click('#end');
  await page.waitForTimeout(50);
  const k = await lastSent(page, 'killTask');
  check(k && k.id === 3, 'End process sends killTask{id} for the selected renderer');
  // Sorting: by memory, descending first.
  await page.click('th[data-sort=memory]');
  await page.waitForTimeout(20);
  check(await page.locator('#rows tr').first().getAttribute('data-kind') === 'browser', 'sort by memory puts the largest first');
  await page.click('th[data-sort=memory]');
  await page.waitForTimeout(20);
  check(await page.locator('#rows tr').first().getAttribute('data-kind') === 'utility', 'a second click reverses the order');
  await shot(page, 'tasks.png');
  check(errors.length === 0, 'no console errors (' + errors.join('; ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await page.context().close();
}

// 0.11.0: a large cookie list (2,500 sites, 6,000 origins) stays usable: a page of rows at a
// time, sizes measured on-screen first and patched in, search and sort over all of it, and
// clearing what a search shows (after a confirmation).
async function testStorageLarge(browser) {
  console.log('storage page: 2,500 sites');
  const data = { sites: [] };
  for (let i = 0; i < 2500; i++) {
    const site = 'site' + String(i).padStart(4, '0') + (i % 10 === 0 ? '.news' : '.example');
    const origins = ['https://' + site];
    if (i % 2 === 0) origins.push('https://www.' + site);
    if (i % 5 === 0) origins.push('https://cdn.' + site);
    data.sites.push({ site, cookies: i % 7, origins, lastUsed: i % 3 ? Date.now() - i * 60000 : null });
  }
  const { page, errors } = await openPage(browser, 'https://krunk.invalid/storage.html', { width: 1000, height: 700 }, 1, (d) => {
    const h = window.__host.handlers;
    window.__usageCalls = [];
    h.storageList = () => ({ sites: d.sites });
    h.storageUsage = (m) => { window.__usageCalls.push(m.origins.length); return { usage: m.origins.map((o) => ({ origin: o, usage: o.length * 100, breakdown: [['local_storage', o.length * 100]] })) }; };
    h.storageCookies = () => ({ cookies: [] });
    h.storageDeleteSite = () => ({ deleted: 1 });
    h.storageClearOrigin = () => null;
  }, data);
  const t0 = Date.now();
  await page.waitForSelector('.site');
  const firstMs = Date.now() - t0;
  const rows = await page.locator('.site').count();
  check(rows === 150 && firstMs < 3000, 'the first page: 150 of 2,500 rows (' + rows + ', ' + firstMs + ' ms)');
  await page.waitForFunction(() => !document.getElementById('summary').textContent.includes('measuring'), null, { timeout: 15000 });
  const calls = await page.evaluate(() => window.__usageCalls);
  check(calls.length >= 20 && Math.max(...calls) <= 200, 'sizes measured 200 origins per request, one after another (' + calls.length + ' requests)');
  check((await page.locator('.site').count()) === 150, 'measuring does not add or rebuild rows');
  check((await page.locator('#summary').textContent()).startsWith('2500 sites'), 'the summary counts every site (' + await page.locator('#summary').textContent() + ')');
  check((await page.locator('.site[data-site="site0000.news"] .count').textContent()).includes('2 subdomains'), 'a site groups its subdomains (' + await page.locator('.site[data-site="site0000.news"] .count').textContent() + ')');
  // Scrolling to the end of the page of rows brings the next page.
  await page.locator('#list > .more').scrollIntoViewIfNeeded();
  await page.waitForFunction(() => document.querySelectorAll('.site').length === 300);
  check(true, 'scrolling on shows the next 150');
  // Search runs over all 2,500, not only the rows shown.
  await page.fill('#search', '.news');
  await page.waitForTimeout(250);
  const news = await page.locator('.site').count();
  check(news === 150 && (await page.locator('#summary').textContent()).includes('250 shown'), 'search ".news": 250 matches, a page shown (' + news + ')');
  check((await page.locator('#clear-shown').textContent()) === 'Clear all shown (250)', 'the button says how many it clears');
  await page.fill('#search', 'site2499');
  await page.waitForTimeout(250);
  check((await page.locator('.site').count()) === 1, 'a site far down the list is found');
  await page.fill('#search', '');
  await page.waitForTimeout(250);
  // Sort by cookies, then by size.
  await page.selectOption('#sort', 'cookies');
  const firstCookies = await page.locator('.site .count').first().textContent();
  check(firstCookies.startsWith('6 cookies'), 'sort by cookies: most first (' + firstCookies + ')');
  await page.selectOption('#sort', 'usage');
  await page.waitForTimeout(100);
  const top = await page.locator('.site').first().getAttribute('data-site');
  check(top && top.endsWith('.news') && top.length > 0, 'sort by size: the sites with the most data first (' + top + ')');
  await page.selectOption('#sort', 'name');
  // Clearing what a search shows asks first.
  await page.fill('#search', 'site000');
  await page.waitForTimeout(250);
  await page.click('#clear-shown');
  await page.waitForSelector('dialog[open]');
  check((await page.locator('dialog[open]').textContent()).includes('10 sites'), 'the confirmation names how many sites');
  await page.locator('dialog[open] button', { hasText: 'Clear' }).click();
  await page.waitForFunction(() => window.__host.sent.filter((m) => m.cmd === 'storageDeleteSite').length >= 1);
  await page.waitForTimeout(400);
  const del = await page.evaluate(() => window.__host.sent.filter((m) => m.cmd === 'storageDeleteSite').map((m) => m.site));
  check(del.length >= 8 && del.every((s) => s.startsWith('site000')), 'it clears only the shown sites (' + del.length + ')');
  await shot(page, 'storage-large.png');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testStorage(browser) {
  console.log('storage page');
  const long = 'x'.repeat(120);
  const data = {
    sites: [
      { site: 'example.com', cookies: 2, origins: ['https://example.com', 'https://www.example.com'], lastUsed: Date.now() - 3 * 3600000 },
      { site: 'news.example', cookies: 1, origins: [], lastUsed: Date.now() - 20000 },
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
  check((await page.locator('.site[data-site="example.com"] .count').textContent()) === '2 cookies · 1 subdomain · local 2.0 KB · IndexedDB 48.8 KB · 50.8 KB', 'site row shows cookies, its subdomains, the breakdown and the storage size (' + await page.locator('.site[data-site="example.com"] .count').textContent() + ')');
  check((await page.locator('#summary').textContent()).includes('3 sites, 3 cookies, 50.8 KB of site data'), 'the total at the top (' + await page.locator('#summary').textContent() + ')');
  check((await page.locator('.site[data-site="example.com"] .last').textContent()) === '3 h ago' && (await page.locator('.site[data-site="news.example"] .last').textContent()) === 'just now' && (await page.locator('.site[data-site="tracker.example"] .last').textContent()) === 'never used', 'last used per site');
  const order = async () => (await page.locator('.site').evaluateAll((l) => l.map((n) => n.dataset.site))).join(',');
  check(await order() === 'example.com,news.example,tracker.example', 'sorted by name first');
  await page.selectOption('#sort', 'usage');
  check(await order() === 'example.com,news.example,tracker.example', 'sort by usage: bytes, then cookies');
  await page.selectOption('#sort', 'lastUsed');
  check(await order() === 'news.example,example.com,tracker.example', 'sort by last used: most recent first, unknown last (' + await order() + ')');
  await page.selectOption('#sort', 'name');
  // Selection and clearing whole sites.
  check(await page.locator('#clear-selected').isHidden(), 'no Clear selected button without a selection');
  await page.locator('.site[data-site="example.com"] .pick').click();
  await page.locator('.site[data-site="news.example"] .pick').click();
  check(await page.locator('#clear-selected').textContent() === 'Clear selected (2)' && await page.locator('.site.open').count() === 0, 'selecting two sites shows Clear selected (2) and does not expand them');
  await page.locator('.site[data-site="news.example"] .pick').focus();
  await page.keyboard.press('Space');
  await page.waitForTimeout(40);
  check(await page.locator('#clear-selected').textContent() === 'Clear selected (1)' && await page.locator('.site.open').count() === 0 && await page.evaluate(() => document.activeElement.classList.contains('pick')), 'Space on a focused checkbox unticks it, keeps the focus and does not expand the row');
  await page.keyboard.press('Space');
  await page.waitForTimeout(40);
  check(await page.locator('#clear-selected').textContent() === 'Clear selected (2)', 'and ticks it again');
  await page.evaluate(() => { window.__host.sent.length = 0; });
  await page.click('#clear-selected');
  await acceptInPage(page);
  await page.waitForTimeout(100);
  {
    const sent = await page.evaluate(() => window.__host.sent.filter((m) => m.cmd === 'storageDeleteSite' || m.cmd === 'storageClearOrigin').map((m) => m.cmd + ':' + (m.site || m.origin)).join(' '));
    check(sent === 'storageDeleteSite:example.com storageClearOrigin:https://example.com storageClearOrigin:https://www.example.com storageDeleteSite:news.example', 'Clear selected deletes the cookies and clears every origin of each site (' + sent + ')');
  }
  check(await page.locator('#clear-selected').isHidden(), 'the selection is empty again');
  await page.evaluate(() => { window.__host.sent.length = 0; });
  await page.fill('#search', 'tracker');
  await page.waitForTimeout(200);
  await page.click('#clear-shown');
  await acceptInPage(page);
  await page.waitForTimeout(100);
  {
    const sent = await page.evaluate(() => window.__host.sent.filter((m) => m.cmd === 'storageDeleteSite' || m.cmd === 'storageClearOrigin').map((m) => m.cmd + ':' + (m.site || m.origin)).join(' '));
    check(sent === 'storageClearOrigin:https://tracker.example', 'Clear all shown clears only the filtered sites (' + sent + ')');
  }
  await page.fill('#search', '');
  await page.waitForTimeout(200);
  await page.locator('.site[data-site="news.example"] > .row .icon-btn').click();
  await acceptInPage(page);
  await page.waitForTimeout(60);
  check((await lastSent(page, 'storageDeleteSite')).site === 'news.example', 'the row\'s trash button clears that site');
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
  await page.locator('.site.open .site-actions button:not(.clear-site)').click();
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
    cookies: [{ name: 'sid', value: 'abc', domain: '.example.com', path: '/', expires: null, secure: true, httpOnly: false, sameSite: 'strict' },
      { name: 'blob', value: 'v1%00%01%FF', domain: 'www.example.com', path: '/', expires: null, secure: true, httpOnly: true, sameSite: 'lax' }],
    local: [['cart', '[1,2,3]'], ['theme', 'dark'], ['bin', '\u0000\u0001\u00ffabc']],
    session: [['step', '2']],
    permissions: { localNetwork: null },
  };
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/sitedata.html', { width: 1000, height: 700 }, 1, (d) => {
    const h = window.__host.handlers;
    h.siteDataGet = () => d;
    h.siteDataSet = () => null;
    h.siteDataRemove = () => null;
    h.siteDataClear = () => null;
    h.siteCookiesDelete = () => ({ deleted: 1 });
    h.siteCookieSet = () => null;
    h.sitePermissionRemove = () => true;
  }, state);
  await page.waitForSelector('.kv');
  check((await page.locator('#origin').textContent()) === 'https://www.example.com', 'shows the bound origin');
  check(await page.locator('#private').isVisible(), 'private tab marked');
  // 0.5: permissions (local network access) – none in a private tab; allowed/blocked with Remove otherwise.
  check((await page.textContent('#permissions')).includes('not remembered in private tabs'), 'private tab: permissions are not remembered');
  await page.evaluate(() => { const get = window.__host.handlers.siteDataGet; window.__host.handlers.siteDataGet = () => Object.assign({}, get(), { private: false, permissions: { localNetwork: true } }); });
  await page.click('#reload');
  await page.waitForTimeout(50);
  check((await page.textContent('.perm-name')) === 'Local network access' && (await page.textContent('.perm-state')) === 'Allowed', 'a stored local-network permission is listed');
  await page.click('.perm-remove');
  await page.waitForTimeout(50);
  check((await lastSent(page, 'sitePermissionRemove')).kind === 'localNetwork', 'Remove asks the host to forget it');
  await page.evaluate(() => { const get = window.__host.handlers.siteDataGet; window.__host.handlers.siteDataGet = () => Object.assign({}, get(), { permissions: { localNetwork: false } }); });
  await page.click('#reload');
  await page.waitForTimeout(50);
  check((await page.textContent('.perm-state')) === 'Blocked', 'a blocked permission is shown as such');
  await page.evaluate(() => { const get = window.__host.handlers.siteDataGet; window.__host.handlers.siteDataGet = () => Object.assign({}, get(), { permissions: { localNetwork: null } }); });
  await page.click('#reload');
  await page.waitForTimeout(50);
  check((await page.textContent('#permissions')).includes('No permissions saved'), 'no stored permission: an empty state');
  await page.evaluate(() => { const get = window.__host.handlers.siteDataGet; window.__host.handlers.siteDataGet = () => Object.assign({}, get(), { private: true }); });
  await page.click('#reload');
  await page.waitForTimeout(50);
  check((await page.locator('#local .kv').count()) === 3 && (await page.locator('#session .kv').count()) === 1, 'local and session storage items');
  check((await page.locator('#cookies tr').count()) === 2, 'site cookies');
  await shot(page, 'sitedata.png');
  // Edit an item.
  await page.locator('#local .kv[data-key="theme"] .icon-btn').first().click();
  await page.fill('#edit-value', 'light');
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  const set = await lastSent(page, 'siteDataSet');
  check(set && set.area === 'local' && set.key === 'theme' && set.value === 'light' && Object.keys(set).length === 5, 'edit sends siteDataSet{area,key,value} only (got ' + JSON.stringify(set) + ')');
  await page.locator('#local .kv[data-key="cart"] .icon-btn').first().click();
  check(await page.locator('#edit-value').isVisible() && await page.locator('#mode-text').getAttribute('aria-selected') === 'true', 'text values open on the Text tab');
  await page.keyboard.press('Escape');

  // The codec: round trips and the encoding a value most likely uses.
  const codec = await page.evaluate(() => {
    const C = window.HexCodec;
    const rt = (t, e) => { const b = C.encode(t, e).bytes; return C.decode(b, e, C.base64Style(t)).text === t; };
    const all = Array.from({ length: 256 }, (_, i) => String.fromCharCode(i)).join('');
    return {
      utf8: rt('héllo 🦀', 'utf8') && C.toHex(C.encode('é', 'utf8').bytes) === 'C3 A9',
      latin1: rt(all, 'latin1') && C.encode(all, 'latin1').bytes.length === 256,
      utf16: rt('a\ud800b', 'utf16') && C.encode('a\ud800', 'utf8').error !== undefined,
      base64: rt('AAEC_-8', 'base64') && rt('AAECAw==', 'base64') && C.toHex(C.encode('AAEC_-8', 'base64').bytes) === '00 01 02 FF EF',
      badBase64: C.encode('abc$', 'base64').error !== undefined,
      percent: C.toHex(C.encode('a%00%FFé', 'percent').bytes) === '61 00 FF C3 A9' && C.decode(Uint8Array.of(0x61, 0, 0x3b, 0x20), 'percent').text === 'a%00%3B%20',
      invalidUtf8: C.decode(Uint8Array.of(0xff, 0x61), 'utf8').error === 'These bytes are not valid UTF-8',
      oddUtf16: C.decode(Uint8Array.of(1), 'utf16').error !== undefined,
      bom: C.decode(Uint8Array.of(0xef, 0xbb, 0xbf, 0x61), 'utf8').text === '\ufeffa',
      detect: [C.detect('hello', false), C.detect('\u0000\u0001ÿ', false), C.detect('\u0000\u4e00', false), C.detect('a\ud800', false),
        C.detect('AAECAwQFBgcICQoLDA0ODw==', false), C.detect('v1%00%01', true), C.detect('aGVsbG8gd29ybGQgaGVsbG8=', false), C.detect('ÿØÿà', false), C.detect('café', false)].join(','),
      binary: [C.looksBinary('hello', false), C.looksBinary('{"a":1}', false), C.looksBinary('\u0000x', false), C.looksBinary('AAECAwQFBgcICQoLDA0ODw==', false),
        C.looksBinary('a%20b', true), C.looksBinary('v1%00%01', true), C.looksBinary('dGhpcyBpcyBwbGFpbiB0ZXh0', false)].join(','),
      parseHex: C.toHex(C.parseHex('de ad,0xBE-ef')) === 'DE AD BE EF' && C.parseHex('abc') === null && C.parseHex('zz') === null,
    };
  });
  for (const [k, v] of Object.entries(codec)) {
    if (k === 'detect') check(v === 'utf8,latin1,utf16,utf16,base64,percent,utf8,latin1,utf8', 'codec: detected encodings (' + v + ')');
    else if (k === 'binary') check(v === 'false,false,true,true,false,true,false', 'codec: binary values open on Hex (' + v + ')');
    else check(v === true, 'codec: ' + k);
  }

  // A binary localStorage value opens on the Hex tab, as a binary string.
  await page.locator('#local .kv[data-key="bin"] .icon-btn').first().click();
  await page.waitForSelector('#edit-hex:not([hidden]) .hx-row');
  const hexText = () => page.evaluate(() => Array.from(document.querySelectorAll('#edit-hex .hx-row')[0].querySelectorAll('.b')).map((b) => b.textContent).filter(Boolean).join(' '));
  check(await page.locator('#mode-hex').getAttribute('aria-selected') === 'true' && await page.inputValue('#edit-enc') === 'latin1', 'binary value: Hex tab, binary-string encoding');
  check(await hexText() === '00 01 FF 61 62 63', 'the bytes (' + await hexText() + ')');
  check((await page.textContent('#edit-hex .hx-row .tx')).startsWith('...abc'), 'the text column');
  await shot(page, 'sitedata-hex.png');
  // Type over the first byte, then insert, delete, paste hex, undo.
  await page.locator('#edit-hex .b[data-i="0"]').click();
  await page.keyboard.type('41');
  check(await hexText() === '41 01 FF 61 62 63', 'typing two hex digits overwrites a byte');
  await page.keyboard.press('Insert');
  await page.keyboard.type('4');
  await page.keyboard.type('2');
  check(await hexText() === '41 42 01 FF 61 62 63' && (await page.textContent('#edit-hex .hx-status')).includes('Insert'), 'Insert mode inserts');
  await page.keyboard.press('Delete');
  check(await hexText() === '41 42 FF 61 62 63', 'Delete removes the byte at the caret');
  await page.keyboard.press('Backspace');
  check(await hexText() === '41 FF 61 62 63', 'Backspace removes the byte before');
  await page.evaluate(() => {
    const dt = new DataTransfer(); dt.setData('text/plain', 'de ad');
    document.querySelector('#edit-hex .hx-view').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  check(await hexText() === '41 DE AD FF 61 62 63', 'pasting hex inserts the bytes');
  await page.keyboard.press('Control+z');
  check(await hexText() === '41 FF 61 62 63', 'Ctrl+Z undoes');
  // Select two bytes with Shift+arrows and copy them as hex.
  await page.keyboard.press('Shift+ArrowRight');
  await page.keyboard.press('Shift+ArrowRight');
  const copied = await page.evaluate(() => {
    const dt = new DataTransfer();
    document.querySelector('#edit-hex .hx-view').dispatchEvent(new ClipboardEvent('copy', { clipboardData: dt, bubbles: true, cancelable: true }));
    return dt.getData('text/plain');
  });
  check(copied === 'FF 61' && (await page.textContent('#edit-hex .hx-status')).includes('2 selected'), 'copy the selection as hex (' + copied + ')');
  // UTF-8 cannot read 0xFF: switching to Text says so; the binary string can be saved.
  await page.selectOption('#edit-enc', 'utf16');
  check(await hexText() === '41 00 FF 00 61 00 62 00 63 00', 'another encoding shows the same value\'s bytes in it');
  await page.selectOption('#edit-enc', 'latin1');
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  const binSet = await lastSent(page, 'siteDataSet');
  check(binSet.key === 'bin' && binSet.value === 'A\u00ffabc', 'Save stores the edited binary string (' + JSON.stringify(binSet.value) + ')');
  // Bytes that are not UTF-8 cannot be saved as UTF-8.
  await page.locator('#local .kv[data-key="theme"] .icon-btn').first().click();
  await page.click('#mode-hex');
  check(await hexText() === '64 61 72 6B', 'Text -> Hex: the UTF-8 bytes of "dark"');
  await page.locator('#edit-hex .b[data-i="0"]').click();
  await page.keyboard.type('FF');
  check((await page.textContent('#edit-error')).includes('not valid UTF-8'), 'invalid UTF-8 is reported');
  await page.click('#mode-text');
  check(await page.locator('#edit-hex').isVisible(), 'and keeps the Hex tab');
  const before = (await sent(page)).length;
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  check((await sent(page)).length === before && await page.locator('#edit').isVisible(), 'and is not saved');
  await page.keyboard.press('Escape');

  // A percent-encoded binary cookie: Hex tab, edits go back percent-encoded.
  await page.locator('#cookies tr[data-name="blob"] .icon-btn').first().click();
  await page.waitForSelector('#edit-hex:not([hidden]) .hx-row');
  check(await page.inputValue('#edit-enc') === 'percent' && await hexText() === '76 31 00 01 FF', 'cookie: percent-encoded bytes');
  check((await page.locator('#edit-enc option').count()) === 3, 'cookies offer UTF-8, percent and Base64 only');
  check(await page.inputValue('#edit-key') === 'blob' && await page.locator('#edit-key').evaluate((n) => n.readOnly), 'the cookie name is fixed');
  await page.locator('#edit-hex .b[data-i="2"]').click();
  await page.keyboard.type('3B');
  await page.click('#edit button[value="ok"]');
  await page.waitForTimeout(50);
  const ck = await lastSent(page, 'siteCookieSet');
  check(ck && ck.value === 'v1%3B%01%FF' && ck.cookie.name === 'blob' && ck.cookie.domain === 'www.example.com' && ck.cookie.path === '/' && Object.keys(ck).length === 4,
    'siteCookieSet {cookie, value} percent-encoded (' + JSON.stringify(ck) + ')');
  // Bytes a cookie cannot hold as UTF-8 text are refused with advice.
  await page.locator('#cookies tr[data-name="sid"] .icon-btn').first().click();
  check(await page.locator('#edit-value').isVisible(), 'a text cookie opens on Text');
  await page.fill('#edit-value', 'a;b');
  await page.click('#edit button[value="ok"]');
  check((await page.textContent('#edit-error')).includes('semicolon') && await page.locator('#edit').isVisible(), 'a semicolon is refused');
  await page.keyboard.press('Escape');

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
  await page.locator('#cookies tr[data-name="sid"] .icon-btn').nth(1).click();
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

  // 0.5: an item dragged out of a folder dropdown is driven by the host: barDragAt marks the
  // target, barDragEnd clears it, barDragDrop moves the bookmark there.
  to = await box(10);
  await emit(page, { event: 'barDragAt', x: to.x + 3, y: 90 });
  await page.waitForTimeout(20);
  check(await page.locator('.bm[data-id="10"].drop-before').count() === 1, 'barDragAt before Rust: mark on Rust');
  to = await box(11);
  await emit(page, { event: 'barDragAt', x: to.x + to.width / 2, y: 90 });
  await page.waitForTimeout(20);
  check(await page.locator('.bm[data-id="11"].drop-into').count() === 1, 'barDragAt over the folder: it lights up');
  await emit(page, { event: 'barDragEnd' });
  await page.waitForTimeout(20);
  check(await page.locator('.bm.drop-before, .bm.drop-after, .bm.drop-into').count() === 0, 'barDragEnd clears the marks');
  to = await box(19);
  await emit(page, { event: 'barDragDrop', id: 12, x: to.x + to.width - 2, y: 90 });
  await page.waitForTimeout(20);
  mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 12 && mv.parent === 1 && mv.index === 3, 'barDragDrop after Wikipedia: moveBookmark{id:12, parent:1, index:3} (' + JSON.stringify(mv) + ')');
  to = await box(11);
  await emit(page, { event: 'barDragDrop', id: 12, x: to.x + to.width / 2, y: 90 });
  await page.waitForTimeout(20);
  mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 12 && mv.parent === 11, 'barDragDrop on the folder: moveBookmark{id:12, parent:11}');

  // The page dragged from the site icon onto the bar: a chip follows, the mark shows, and the
  // drop sends bookmarkActivePage (the host takes the title and address from the tab).
  const icon = await page.locator('#site-icon').boundingBox();
  to = await box(19);
  await page.mouse.move(icon.x + icon.width / 2, icon.y + icon.height / 2);
  await page.mouse.down();
  await page.mouse.move(icon.x + 20, icon.y + 10, { steps: 3 });
  await page.mouse.move(to.x + 3, to.y + 10, { steps: 6 });
  check(await page.locator('#url-chip').count() === 1 && (await page.textContent('#url-chip')) === 'Rust', 'a chip with the page title follows the drag');
  check(await page.locator('.bm[data-id="19"].drop-before').count() === 1, 'over the bar: the mark shows before Wikipedia');
  await shot(page, 'shell-url-drag.png');
  await page.mouse.up();
  let bp = await lastSent(page, 'bookmarkActivePage');
  check(bp && bp.parent === 1 && bp.index === 2, 'dropped before Wikipedia: bookmarkActivePage{parent:1, index:2} (' + JSON.stringify(bp) + ')');
  check(await page.locator('#url-chip').count() === 0, 'the chip is gone');
  // 0.7.0 (R4): a drag in the (unfocused) address field selects text; it never drags the page.
  const ab = await page.locator('#address').boundingBox();
  const before = (await sent(page)).filter((m) => m.cmd === 'bookmarkActivePage').length;
  await page.mouse.move(ab.x + 40, ab.y + ab.height / 2);
  await page.mouse.down();
  await page.mouse.move(ab.x + 90, ab.y + ab.height / 2, { steps: 5 });
  check(await page.locator('#url-chip').count() === 0, 'a drag in the field makes no chip');
  await page.mouse.up();
  const dragSel = await page.evaluate(() => { const a = document.getElementById('address'); return { focused: document.activeElement === a, start: a.selectionStart, end: a.selectionEnd, len: a.value.length }; });
  check(dragSel.focused && dragSel.end > dragSel.start && dragSel.end - dragSel.start < dragSel.len, 'a drag in the unfocused field focuses it and selects the dragged part (' + JSON.stringify(dragSel) + ')');
  check((await sent(page)).filter((m) => m.cmd === 'bookmarkActivePage').length === before, 'and bookmarks nothing');
  // The same in the focused field, even inside the selection: a new selection, no text drag.
  await page.evaluate(() => document.getElementById('address').select());
  await page.mouse.move(ab.x + 30, ab.y + ab.height / 2);
  await page.mouse.down();
  await page.mouse.move(ab.x + 70, ab.y + ab.height / 2, { steps: 5 });
  await page.mouse.up();
  const dragSel2 = await page.evaluate(() => { const a = document.getElementById('address'); return { start: a.selectionStart, end: a.selectionEnd, len: a.value.length }; });
  check(dragSel2.end > dragSel2.start && dragSel2.end - dragSel2.start < dragSel2.len, 'a drag inside the selected address selects again (' + JSON.stringify(dragSel2) + ')');
  // Double click: a word; triple click: everything.
  await page.mouse.dblclick(ab.x + 100, ab.y + ab.height / 2);
  const word = await page.evaluate(() => { const a = document.getElementById('address'); return a.value.slice(a.selectionStart, a.selectionEnd); });
  check(word.length > 0 && !word.includes('/') && word.length < 'https://www.rust-lang.org/'.length, 'a double click selects a word (' + word + ')');
  await page.mouse.click(ab.x + 100, ab.y + ab.height / 2, { clickCount: 3 });
  check(await page.evaluate(() => { const a = document.getElementById('address'); return a.selectionStart === 0 && a.selectionEnd === a.value.length; }), 'a triple click selects all');
  // A press elsewhere in the shell blurs the bar: selection gone, formatted address back (R5).
  const back = await page.locator('#toolbar').boundingBox();
  await page.mouse.click(back.x + back.width - 5, back.y + 4);
  await page.waitForTimeout(20);
  const blurred = await page.evaluate(() => { const a = document.getElementById('address'); return { focused: document.activeElement === a, collapsed: a.selectionStart === a.selectionEnd, display: !document.getElementById('address-display').hidden, host: (document.querySelector('#address-display .host') || {}).textContent }; });
  check(!blurred.focused && blurred.collapsed && blurred.display && blurred.host === 'www.rust-lang.org', 'a click outside the bar blurs it and the formatted address returns (' + JSON.stringify(blurred) + ')');
  // A plain click on the unfocused field focuses it with the text selected.
  await page.waitForTimeout(600);
  await page.mouse.click(ab.x + 40, ab.y + ab.height / 2);
  await page.waitForTimeout(30);
  const clickSel = await page.evaluate(() => { const a = document.getElementById('address'); return { focused: document.activeElement === a, start: a.selectionStart, end: a.selectionEnd, len: a.value.length }; });
  check(clickSel.focused && clickSel.start === 0 && clickSel.end === clickSel.len, 'a click on the field focuses it and selects the address (' + JSON.stringify(clickSel) + ')');
  check(await page.locator('#address-display').isHidden(), 'the focused field shows its own text');
  // The host says the page took the focus (a click into the page).
  await emit(page, { event: 'blurAddressBar' });
  await page.waitForTimeout(20);
  check(await page.evaluate(() => document.activeElement !== document.getElementById('address') && document.getElementById('address').selectionStart === document.getElementById('address').selectionEnd), 'blurAddressBar: the bar gives up focus and selection');
  await page.click('#address');
  await page.keyboard.press('Escape');
  // Text dropped on the bar (HTML5 drag and drop): an http(s) address becomes a bookmark.
  to = await box(10);
  await page.evaluate(([x, y]) => {
    const bar = document.getElementById('bookmarks-bar');
    const dt = new DataTransfer();
    dt.setData('text/plain', 'https://dropped.example/path');
    bar.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
    bar.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt }));
    const dt2 = new DataTransfer();
    dt2.setData('text/plain', 'not an address');
    bar.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt2 }));
  }, [to.x + 3, to.y + 10]);
  await page.waitForTimeout(20);
  const adds = (await sent(page)).filter((x) => x.cmd === 'addBookmarkAt');
  check(adds.length === 1 && adds[0].url === 'https://dropped.example/path' && adds[0].parent === 1 && adds[0].index === 0, 'dropped text: addBookmarkAt{url, parent:1, index:0} once; plain text is ignored (' + JSON.stringify(adds) + ')');
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
  // 0.7.2 (R8): every item has the icon column; the labels start at the same x, the radio
  // check sits in the icon column.
  const cols = await page.evaluate(() => Array.from(document.querySelectorAll('.item')).map((i) => [!!i.querySelector('svg.micon'), Math.round(i.querySelector('.label').getBoundingClientRect().left)]));
  check(cols.every((c) => c[0]) && new Set(cols.map((c) => c[1])).size === 1, 'bookmark menu: an icon column and aligned labels (' + JSON.stringify(cols) + ')');
  check(await page.locator('.item.radio.checked svg use').getAttribute('href') === '#m-check' && await page.locator('.item.radio:not(.checked) svg use').count() === 0, 'the checked mode shows its check in the icon column');
  check(await page.locator('.item.danger', { hasText: 'Delete' }).count() === 1, 'Delete is marked as the dangerous item');
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
  await page.waitForFunction(() => document.querySelector('h2') && document.querySelector('h2').textContent === 'New folder');
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
    h.emit({ event: 'shellConfig', theme: 'system', searchName: 'DuckDuckGo' });
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
  const wikiPress = await page.evaluate(() => {
    const b = [...document.querySelectorAll('.bm')].find((x) => x.textContent.includes('Wikipedia'));
    const ev = new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true });
    b.dispatchEvent(ev);
    return { prevented: ev.defaultPrevented, id: Number(b.dataset.id) };
  });
  check(wikiPress.prevented, '0.11.0: a middle press on the bar starts no autoscroll (mousedown defaultPrevented)');
  await page.locator('.bm', { hasText: 'Wikipedia' }).click({ button: 'middle' });
  const bg = await lastSent(page, 'openBookmarkBackground');
  check(bg && bg.id === wikiPress.id, '0.11.0: middle click opens the bookmark in a background tab (openBookmarkBackground{id})');
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
  // 0.6.0: the button opens its popup (Ctrl+J keeps opening the page); a new download pulses it.
  await page.click('#downloads');
  await page.waitForTimeout(20);
  {
    const dp = await lastSent(page, 'showPopup');
    check(dp && dp.popup === 'downloads' && dp.rect.width === 380 && dp.rect.y > 40, 'downloads button opens the downloads popup under the toolbar');
    check(await page.locator('#downloads.on').count() === 1, 'the button is highlighted while its popup is open');
  }
  await emit(page, { event: 'popupClosed' });
  await page.waitForTimeout(20);
  await emit(page, { event: 'downloadStarted' });
  await page.waitForTimeout(50);
  check(await page.locator('#downloads.pulse').count() === 1, 'downloadStarted pulses the button');
  {
    const hm = await lastSent(page, 'hitMap');
    check(hm && hm.map.downloads && hm.map.downloads.width > 10 && hm.map.downloads.x > 500, 'the hit map now reports the downloads button for the flight target (' + JSON.stringify(hm && hm.map.downloads) + ')');
  }
  await page.waitForTimeout(950);
  check(await page.locator('#downloads.pulse').count() === 0, 'the pulse ends by itself');
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

// 0.4.0: what the ad blocker blocked: the shield panel's list and krunk://adblock.
async function testBlockedList(browser) {
  console.log('blocked list: shield panel and krunk://adblock');
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 360, height: 418 }, 1);
  const recent = [
    { url: 'https://securepubads.g.doubleclick.net/tag/js/gpt.js', host: 'securepubads.g.doubleclick.net', kind: 'script', count: 1, redirected: true },
    { url: 'https://www.google-analytics.com/g/collect', host: 'www.google-analytics.com', kind: 'ping', count: 7, redirected: false },
    { url: 'https://ads.example/very/long/path/to/an/advert/banner-728x90-with-a-long-name.png', host: 'ads.example', kind: 'image', count: 2, redirected: false },
  ];
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 10, allowed: false, enabled: true, recent: recent, distinct: 14 } });
  await page.waitForSelector('.blocked');
  check(await page.locator('.blocked').count() === 3 && await page.locator('.blocked .badge').count() === 3, 'the shield lists the newest blocked addresses with type badges');
  check((await page.locator('.blocked').nth(1).textContent()).includes('www.google-analytics.com') && (await page.locator('.blocked').nth(1).textContent()).includes('×7'), 'host, path and count');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom) <= 226 + 12 + 72 + 32, 'the panel fits the size the host gives it (3 rows)');
  check(await page.evaluate(() => Array.from(document.querySelectorAll('.blocked .addr')).every((n) => n.scrollWidth <= n.clientWidth + 1 || getComputedStyle(n).textOverflow === 'ellipsis')), 'long addresses are cut with an ellipsis');
  await shot(page, 'popup-shield-blocked.png');
  await page.click('#show-blocked');
  check((await lastSent(page, 'openBlockedPage')) !== undefined && (await page.textContent('#show-blocked')).includes('14'), '"Show all 14 blocked addresses" sends openBlockedPage');
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 0, allowed: false, enabled: true, recent: [], distinct: 0 } });
  await page.waitForTimeout(20);
  check(await page.locator('.blocked-list').count() === 0, 'nothing blocked: no list');
  check(errors.length === 0, 'shield: no console errors (' + errors.join(' | ') + ')');
  await page.close();

  const items = [
    { url: 'https://securepubads.g.doubleclick.net/tag/js/gpt.js', host: 'securepubads.g.doubleclick.net', kind: 'script', count: 1, redirected: true, rule: '||securepubads.g.doubleclick.net/tag/js/gpt.js$script,redirect=googletagservices_gpt.js', list: 'uBlock filters' },
    { url: 'https://www.google-analytics.com/g/collect', host: 'www.google-analytics.com', kind: 'ping', count: 7, redirected: false, rule: '||google-analytics.com^', list: 'EasyPrivacy (trackers)' },
    { url: 'https://securepubads.g.doubleclick.net/pagead/ppub_config', host: 'securepubads.g.doubleclick.net', kind: 'xmlhttprequest', count: 2, redirected: false, rule: '||doubleclick.net^', list: 'EasyList (ads)' },
  ];
  let ext;
  ({ page, errors, external: ext } = await openPage(browser, 'https://krunk.invalid/adblock.html', { width: 1000, height: 700 }, 1,
    (d) => { window.__host.handlers.getBlocked = () => d; }, { host: 'news.example.com', source: true, enabled: true, items: items }));
  await page.waitForSelector('.req');
  check(await page.locator('.host').count() === 2 && await page.locator('.req').count() === 3, 'grouped by host (2 hosts, 3 addresses)');
  check((await page.locator('.host').first().locator('h3').textContent()).includes('3 requests'), 'a host shows its total');
  check((await page.textContent('#summary')) === '10 requests to 3 addresses blocked on news.example.com', 'summary (' + await page.textContent('#summary') + ')');
  check((await page.locator('.req').first().textContent()).includes('redirect=googletagservices_gpt.js') && (await page.locator('.req').first().textContent()).includes('stand-in'), 'the filter, its list and a stand-in note');
  check(await page.locator('.req .kind', { hasText: 'xhr' }).count() === 1, 'short type names (xhr)');
  await shot(page, 'adblock-blocked.png');
  await page.fill('#search', 'easyprivacy');
  check(await page.locator('.req').count() === 1, 'search finds the list name');
  await page.fill('#search', 'nothing-like-this');
  check((await page.textContent('#list')).includes('Nothing matches'), 'search with no hit');
  await page.fill('#search', '');
  await page.click('#refresh');
  check((await sent(page)).filter((m) => m.cmd === 'getBlocked').length >= 2, 'Refresh asks again');
  check(ext.length === 0, 'adblock page: no network requests');
  check(errors.length === 0, 'adblock page: no console errors (' + errors.join(' | ') + ')');
  await page.close();
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/adblock.html', { width: 1000, height: 400 }, 1,
    () => { window.__host.handlers.getBlocked = () => ({ host: '', source: false, enabled: true, items: [] }); }));
  await page.waitForSelector('.empty');
  check((await page.textContent('#summary')).includes('Open this page from the shield'), 'opened on its own: how to get a list');
  check(errors.length === 0, 'no console errors');
}

// 0.4.0: krunk://adblock's filter lists (on/off, add, remove, restore, update) and My filters.
function filterListsMock(opts) {
  const h = window.__host.handlers;
  const now = Date.now();
  const st = {
    enabled: !(opts && opts.off), lastCheck: now - 3600e3, lastError: null, busy: false, user: '! mine\n||ads.example^\n',
    lists: [
      { id: 'easylist', title: 'EasyList (ads)', url: 'https://easylist.to/easylist/easylist.txt', builtin: true, enabled: true, source: 'downloaded', fetched: now - 3600e3, rules: 71234, error: null },
      { id: 'easyprivacy', title: 'EasyPrivacy (trackers)', url: 'https://easylist.to/easylist/easyprivacy.txt', builtin: true, enabled: true, source: 'embedded', fetched: null, rules: 52011, error: null },
      { id: 'ublock-badware', title: 'uBlock filters – Badware risks', url: 'https://ublockorigin.github.io/uAssets/filters/badware.txt', builtin: true, enabled: false, source: 'embedded', fetched: null, rules: 3100, error: 'HTTP 503' },
    ],
  };
  window.__fl = st;
  const status = () => ({ enabled: st.enabled, lastCheck: st.lastCheck, lastError: st.lastError, busy: st.busy, lists: JSON.parse(JSON.stringify(st.lists)), userRules: 1 });
  // Slow, like a rebuild.
  const later = (f) => new Promise((res, rej) => setTimeout(() => { try { res(f()); } catch (e) { rej(e); } }, 150));
  h.getBlocked = () => ({ host: '', source: false, enabled: true, items: [] });
  h.getFilterLists = status;
  h.setFilterList = (m) => later(() => { st.lists.find((l) => l.id === m.id).enabled = m.enabled; return status(); });
  h.addFilterList = (m) => later(() => {
    if (m.url.includes('missing')) throw new Error('Could not add the list: HTTP 404');
    st.lists.push({ id: 'custom-9', title: 'Annoyances', url: m.url, builtin: false, enabled: true, source: 'downloaded', fetched: Date.now(), rules: 812, error: null });
    return {};
  });
  h.removeFilterList = (m) => later(() => { st.lists = st.lists.filter((l) => l.id !== m.id); return status(); });
  h.restoreDefaultLists = () => later(() => { st.lists.forEach((l) => { if (l.builtin) l.enabled = l.id !== 'ublock-badware'; }); return status(); });
  h.updateFilterLists = () => later(() => { st.lastCheck = Date.now(); return status(); });
  h.getUserFilters = () => ({ text: st.user, report: { network: 1, cosmetic: 0, errors: [] } });
  h.saveUserFilters = (m) => later(() => {
    st.user = m.text;
    const errors = [];
    m.text.split('\n').forEach((l, i) => { if (l.includes('$nope')) errors.push({ line: i + 1, text: l, message: 'unknown option' }); });
    return { network: m.text.split('\n').filter((l) => l.startsWith('||')).length - errors.length, cosmetic: m.text.split('\n').filter((l) => l.includes('##')).length, errors: errors };
  });
}

async function testFilterLists(browser) {
  console.log('krunk://adblock: filter lists and My filters');
  let { page, errors, external } = await openPage(browser, 'https://krunk.invalid/adblock.html#lists', { width: 1000, height: 900 }, 1, filterListsMock);
  await page.waitForSelector('.list-row');
  check(await page.locator('#lists').isVisible() && await page.locator('#blocked').isHidden(), '#lists opens the Filter lists tab');
  check(await page.locator('.list-row').count() === 3, 'every list, on or off');
  const first = await page.locator('.list-row').first().textContent();
  check(first.includes('https://easylist.to/easylist/easylist.txt') && first.includes('71,234 filters') && first.includes('built-in'), 'a list shows its address, rule count and kind (' + first + ')');
  check((await page.locator('.list-row').nth(2).textContent()).includes('Last download failed: HTTP 503'), 'a failed download is shown');
  check(await page.locator('.list-row').first().locator('.icon-btn').count() === 0, 'built-in lists cannot be removed');
  check((await page.textContent('#tab-lists')) === 'Filter lists (2)', 'the tab counts the lists that are on');
  await shot(page, 'adblock-lists.png');
  // Turn one off: the controls wait for the new engine.
  await page.locator('.list-row').nth(1).locator('input[type=checkbox]').uncheck();
  await page.waitForTimeout(30);
  check(await page.locator('#update-lists').isDisabled() && (await page.textContent('#lists-status')).includes('Turning off EasyPrivacy'), 'while the engine is rebuilt the page waits');
  await page.waitForFunction(() => !document.getElementById('update-lists').disabled);
  check((await lastSent(page, 'setFilterList')).id === 'easyprivacy' && (await lastSent(page, 'setFilterList')).enabled === false, 'setFilterList {id, enabled:false}');
  check(await page.locator('.list-row.off').count() === 2 && (await page.textContent('#tab-lists')) === 'Filter lists (1)', 'the list is off after the rebuild');
  // Add by address: validation, a failed download, a good one.
  await page.fill('#add-url', 'http://lists.example/a.txt');
  await page.click('#add-btn');
  check((await page.textContent('#add-error')) === 'Enter an https:// address' && (await lastSent(page, 'addFilterList')) === undefined, 'only https:// addresses');
  await page.fill('#add-url', 'https://lists.example/missing.txt');
  await page.click('#add-btn');
  await page.waitForFunction(() => document.getElementById('add-error').textContent !== '');
  check((await page.textContent('#add-error')) === 'Could not add the list: HTTP 404' && await page.locator('.list-row').count() === 3, 'a list that cannot be downloaded is not added');
  await page.fill('#add-url', 'https://lists.example/annoyances.txt');
  await page.press('#add-url', 'Enter');
  await page.waitForFunction(() => document.querySelectorAll('.list-row').length === 4);
  const added = page.locator('.list-row').nth(3);
  check((await added.textContent()).includes('Annoyances') && (await added.textContent()).includes('https://lists.example/annoyances.txt') && (await added.textContent()).includes('added'), 'the added list with its title and address');
  check((await page.inputValue('#add-url')) === '' && (await page.textContent('#add-error')) === '', 'the form is cleared');
  // Remove it (confirmed).
  await added.locator('.icon-btn').click();
  await page.waitForSelector('#lb-dialog-ok');
  await page.click('#lb-dialog-ok');
  await page.waitForFunction(() => document.querySelectorAll('.list-row').length === 3);
  check((await lastSent(page, 'removeFilterList')).id === 'custom-9', 'removeFilterList {id}');
  await page.click('#restore-lists');
  await page.waitForFunction(() => document.querySelectorAll('.list-row.off').length === 1);
  check((await lastSent(page, 'restoreDefaultLists')) !== undefined, 'Restore default lists');
  await page.click('#update-lists');
  check((await lastSent(page, 'updateFilterLists')) !== undefined, 'Update now');
  await page.waitForFunction(() => !document.getElementById('update-lists').disabled);
  // The host rebuilds for another reason (the daily update): wait, look again, carry on.
  await page.evaluate(() => { window.__fl.busy = true; });
  await page.click('#tab-blocked');
  await page.click('#tab-lists');
  await page.waitForFunction(() => document.getElementById('update-lists').disabled);
  check((await page.textContent('#lists-status')) === 'Updating…', 'a rebuild elsewhere: Updating…, controls wait');
  await page.evaluate(() => { window.__fl.busy = false; });
  await page.waitForFunction(() => !document.getElementById('update-lists').disabled && !document.getElementById('add-url').disabled, null, { timeout: 3000 });
  check((await page.textContent('#lists-status')).startsWith('Last checked'), 'the page looks again until it is done');

  // My filters.
  await page.click('#tab-filters');
  await page.waitForFunction(() => document.getElementById('user-text').value !== '');
  check((await page.evaluate(() => location.hash)) === '#filters', 'the tab is in the address (#filters)');
  check(await page.inputValue('#user-text') === '! mine\n||ads.example^\n' && await page.locator('#user-save').isDisabled(), 'the saved filters, nothing to save yet');
  check((await page.textContent('#user-status')).includes('1 blocking filter'), 'counts');
  await page.fill('#user-text', '! mine\n||ads.example^\n||bad.example^$nope\nexample.com##.promo\n');
  check(!(await page.locator('#user-save').isDisabled()), 'an edit enables Save');
  await page.press('#user-text', 'Control+s');
  await page.waitForFunction(() => document.getElementById('user-status').textContent.startsWith('Saved'));
  check((await lastSent(page, 'saveUserFilters')).text.includes('example.com##.promo'), 'Ctrl+S saves the text');
  check((await page.textContent('#user-status')) === 'Saved and in use · 1 blocking filter · 1 element filter · 1 line not understood', 'the report (' + await page.textContent('#user-status') + ')');
  check((await page.textContent('#user-errors')).includes('Line 3: unknown option') && (await page.textContent('#user-errors')).includes('||bad.example^$nope'), 'errors per line');
  await shot(page, 'adblock-my-filters.png');
  await page.fill('#user-text', 'x');
  await page.click('#user-revert');
  await page.waitForFunction(() => document.getElementById('user-text').value !== 'x');
  check(await page.inputValue('#user-text') === '! mine\n||ads.example^\n||bad.example^$nope\nexample.com##.promo\n', 'Revert restores the saved text');
  check(external.length === 0, 'no network requests');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.close();
  // Ad blocking off: a note, editing still works.
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/adblock.html#lists', { width: 1000, height: 600 }, 1,
    filterListsMock, { off: true }));
  await page.waitForSelector('.list-row');
  check(await page.locator('#lists-off').isVisible(), 'ad blocking off: a note');
  check(errors.length === 0, 'no console errors');
  await page.close();
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
  check((await page.textContent('.pop-sub')) === 'news.example.com', 'popover shows the site');
  await shot(page, 'popup-shield-count.png');
  await page.click('#site-toggle[title="Allow ads on this site"]');
  const al = await lastSent(page, 'shieldAllowSite');
  check(al && al.allow === true, '"Allow ads on this site" (the switch) sends shieldAllowSite{allow:true}');
  await emit(page, { event: 'popup', content: { kind: 'shield', host: 'news.example.com', blocked: 0, allowed: true, enabled: true } });
  await page.waitForTimeout(30);
  await page.click('#site-toggle[title="Block on this site again"]');
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
    h.vaultStatus = () => ({ exists: st.exists, locked: st.locked, busy: false, waitMs: st.waitMs, recovery: st.recovery || null });
    // 0.7.0 (R13): the recovery phrase. A BIP39 test vector, never a real phrase.
    const TEST_WORDS = 'legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title'.split(' ');
    h.vaultRecoveryCreate = (m) => { if (m.password !== 'correct horse battery') throw new Error('Wrong master password.'); st.pending = true; return { words: TEST_WORDS, confirm: [2, 9, 17] }; };
    h.vaultRecoveryConfirm = (m) => {
      const bad = m.answers.find((a) => a.word !== TEST_WORDS[a.position - 1]);
      if (bad) throw new Error('Word ' + bad.position + ' is not right');
      st.recovery = 1794000000000; st.pending = false;
      return { created: st.recovery };
    };
    h.vaultRecoveryCancel = () => { st.cancelled = (st.cancelled || 0) + 1; st.pending = false; return null; };
    h.vaultRecoveryCopy = () => null;
    h.vaultRecoverySave = () => ({ saved: true });
    h.vaultRecoveryRemove = (m) => { if (m.password !== 'correct horse battery') throw new Error('Wrong master password.'); st.recovery = null; return null; };
    h.vaultRecoveryCheck = (m) => {
      const w = m.phrase.split(/[^A-Za-z0-9]+/).filter((x) => x && !/^\d+$/.test(x));
      if (w.length !== 24) return { ok: false, message: 'A recovery phrase has 24 words; this has ' + w.length, unknown: [] };
      const unknown = w.map((x, i) => (/^q+$/.test(x) ? i + 1 : 0)).filter(Boolean);
      return unknown.length ? { ok: false, message: 'Word' + (unknown.length > 1 ? 's ' : ' ') + unknown.join(', ') + ' not in the word list', unknown } : { ok: true };
    };
    h.vaultRecover = (m) => {
      if (m.phrase.split(/[^A-Za-z0-9]+/).filter((x) => x && !/^\d+$/.test(x)).join(' ') !== TEST_WORDS.join(' ')) throw new Error('This recovery phrase does not open the vault');
      st.locked = false; st.recoveredWith = m.new; return null;
    };
    h.vaultChangePassword = (m) => { if (m.old !== 'correct horse battery') throw new Error('Wrong master password.'); return null; };
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
    // 0.6.0 payment methods: masked summaries; the number only after the master password.
    st.cards = st.cards || [
      { id: 'k1', nickname: 'Work Visa', brand: 'Visa', last4: '1111', expMonth: 12, expYear: 2030, name: 'Alice Example', hasAddress: true },
      { id: 'k2', nickname: '', brand: 'Mastercard', last4: '4444', expMonth: 3, expYear: 2029, name: '', hasAddress: false },
    ];
    h.cardList = () => st.cards;
    h.cardSave = (m) => { if (!m.card.id && !/^\d{12,19}$/.test(m.card.number.replace(/[\s-]/g, ''))) throw new Error('That is not a card number.'); if (!m.card.id) st.cards.push({ id: 'k3', nickname: m.card.nickname, brand: 'Visa', last4: m.card.number.slice(-4), expMonth: m.card.expMonth, expYear: m.card.expYear, name: m.card.name, hasAddress: !!m.card.address }); return null; };
    h.cardDelete = (m) => { st.cards = st.cards.filter((c) => c.id !== m.id); return null; };
    h.cardReveal = (m) => { if (m.password !== 'correct horse battery') throw new Error('Wrong master password.'); return { number: '4111111111111111', address: m.id === 'k1' ? '1 Main St, Springfield' : '' }; };
    h.cardCopy = () => null;
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
  check((await page.textContent('#unlock-error')).includes('Wrong master password'), 'the countdown keeps the reason');
  await page.waitForTimeout(1200);
  check((await page.textContent('#unlock-error')).trim() === 'Wrong master password.', 'after the delay the reason stays (0.7.1)');
  check(!(await page.locator('#unlock-btn').isDisabled()), 'unlock enabled again after the delay');
  await page.fill('#unlock-pw', 'wrong again');
  await page.click('#unlock-btn');
  await page.waitForTimeout(80);
  check(await page.locator('#unlock-btn').isDisabled(), 'unlock disabled during the delay');
  check((await page.textContent('#unlock-error')).includes('2 s'), 'second failure: 2 s');
  check(await page.inputValue('#unlock-pw') === '', 'password field cleared after an attempt');
  await shot(page, 'vault-rate-limited.png');
  await page.waitForTimeout(2300);
  check((await page.textContent('#unlock-error')).includes('Wrong master password'), 'the reason is still there after the second delay');
  await page.type('#unlock-pw', 'x');
  check((await page.textContent('#unlock-error')) === '', 'typing a new attempt clears it');
  check(await page.isChecked('#unlock-remain'), '0.11.0: the page\'s "Remain unlocked this session" starts checked');
  await page.fill('#unlock-pw', 'correct horse battery');
  await page.click('#unlock-btn');
  {
    const u = await lastSent(page, 'vaultUnlock');
    check(u && u.remain === true, 'the page sends remain with the unlock (' + JSON.stringify(u && { remain: u.remain }) + ')');
  }
  await page.waitForSelector('#list-section:not([hidden])');
  check(true, 'correct password unlocks');

  // List.
  await page.setViewportSize({ width: 1000, height: 560 });
  await page.waitForSelector('.pw-row');
  check((await page.locator('.pw-row:not(.card-row)').count()) === 3, 'lists 3 logins');
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
  await page.waitForTimeout(30);
  // 0.7.2 (R5): a labelled form; the website field has the focus; an error under the field.
  const labelled = (dlg) => page.evaluate((d) => Array.from(document.querySelectorAll(d + ' input')).every((i) =>
    i.getAttribute('aria-label') || document.querySelector(d + ' label[for="' + i.id + '"]')), dlg);
  check(await labelled('#entry-dialog') && await page.evaluate(() => document.activeElement.id === 'e-origin'), 'Add login: every field is labelled and the website has the focus');
  await page.locator('#entry-dialog button[value="ok"]').click();
  await page.waitForTimeout(30);
  check((await page.textContent('#e-pw-error')).includes('Enter or generate') && await page.getAttribute('#e-pw', 'aria-invalid') === 'true' &&
    await page.evaluate(() => document.activeElement.id === 'e-pw'), 'a missing password is shown under the field, marked invalid and focused');
  await shot(page, 'passwords-add-login-error.png');
  await page.click('#e-show');
  check(await page.getAttribute('#e-pw', 'type') === 'text' && await page.getAttribute('#e-show', 'aria-pressed') === 'true', 'the eye button shows the password (aria-pressed)');
  await page.click('#e-show');
  check(await page.getAttribute('#e-pw', 'type') === 'password' && await page.getAttribute('#e-show', 'aria-pressed') === 'false', 'and hides it again');
  await page.fill('#e-origin', 'news.example.com');
  await page.fill('#e-user', 'reader');
  await page.click('#e-gen');
  await page.waitForTimeout(50);
  check(await page.inputValue('#e-pw') === 'k7#Vq2!mZp9@Lr4$Wx8e', 'generate fills a strong password');
  check(await page.getAttribute('#e-show', 'aria-pressed') === 'true' && await page.getAttribute('#e-pw', 'aria-invalid') === null && (await page.textContent('#e-pw-error')) === '', 'the generated password is shown and the error is gone');
  await shot(page, 'passwords-add-login.png');
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
  // 0.6.0 payment methods: masked list, reveal/copy/add/edit/delete, never a security code.
  await page.waitForSelector('.card-row');
  check((await page.locator('.card-row').count()) === 2 && (await page.textContent('.card-row[data-id="k1"] .secret')) === '•••• •••• •••• 1111' && (await page.textContent('.card-row[data-id="k1"] .name')) === 'Work Visa · Visa' && (await page.textContent('.card-row[data-id="k2"] .user')).includes('expires 03/2029'), 'payment methods listed masked with nickname, brand, name and expiry');
  check(!(await page.content()).includes('4111111111111111') && !(await page.content()).includes('1 Main St'), 'no card number or address in the page before reveal');
  await page.locator('.card-row[data-id="k1"]').hover();
  await page.locator('.card-row[data-id="k1"] button', { hasText: 'Copy number' }).click();
  await page.waitForTimeout(40);
  { const cc = await lastSent(page, 'cardCopy'); const toasts = await page.locator('.toast').allTextContents();
    check(cc && cc.id === 'k1' && toasts.some((t) => t.includes('Card number copied')), 'Copy number sends cardCopy{id} and mentions the 30 s clear (' + JSON.stringify(cc) + ' ' + JSON.stringify(toasts) + ')'); }
  await page.locator('.card-row[data-id="k1"] button', { hasText: 'Show' }).click();
  await page.fill('#reveal-pw', 'nope');
  await page.press('#reveal-pw', 'Enter');
  await page.waitForTimeout(60);
  check((await page.textContent('#reveal-error')).includes('Wrong'), 'card reveal with a wrong master password fails');
  await page.fill('#reveal-pw', 'correct horse battery');
  await page.press('#reveal-pw', 'Enter');
  await page.waitForTimeout(60);
  check((await page.textContent('.card-row[data-id="k1"] .secret')) === '4111 1111 1111 1111' && (await page.textContent('.card-row[data-id="k1"] .note')) === 'Address: 1 Main St, Springfield', 'reveal shows the grouped number and the address after the master password');
  const kr = await lastSent(page, 'cardReveal');
  check(kr && kr.id === 'k1' && kr.password === 'correct horse battery', 'reveal sends cardReveal{id,password}');
  await shot(page, 'passwords-cards.png');
  await page.click('#card-add');
  check((await page.textContent('#card-dialog')).includes('never stored') && (await page.locator('#card-dialog input').count()) === 6, 'the card dialog says the security code is never stored and has no CVC field');
  await page.locator('#card-dialog button[value="ok"]').click();
  check((await page.textContent('#k-number-error')).includes('Enter the card number') && await page.getAttribute('#k-number', 'aria-invalid') === 'true', 'an empty card number is refused under the field');
  check(await labelled('#card-dialog'), 'Add card: every field is labelled');
  await page.fill('#k-number', '4111');
  await page.fill('#k-month', '13');
  await page.locator('#card-dialog button[value="ok"]').click();
  await page.waitForTimeout(30);
  check((await page.textContent('#k-exp-error')).includes('1 to 12') && await page.getAttribute('#k-month', 'aria-invalid') === 'true' && (await page.textContent('#k-number-error')) === '', 'a month of 13 is refused under the expiry');
  await shot(page, 'passwords-add-card-error.png');
  const cardBox = await page.locator('#card-dialog').boundingBox();
  check(cardBox && cardBox.width >= 440 && cardBox.width <= 540, 'the card dialog has the form width (' + (cardBox && cardBox.width) + ')');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(30);
  check(!(await page.locator('#card-dialog').evaluate((d) => d.open)), 'Esc closes the card dialog');
  await page.click('#card-add');
  await page.waitForTimeout(30);
  check(await page.evaluate(() => document.activeElement.id === 'k-number') && (await page.textContent('#k-exp-error')) === '' && await page.getAttribute('#k-month', 'aria-invalid') === null, 'reopened: the number has the focus, no old errors');
  // Dark theme: the same dialog on the dark tokens.
  await emit(page, { event: 'theme', theme: 'dark' });
  await page.waitForTimeout(30);
  check(await page.evaluate(() => getComputedStyle(document.getElementById('card-dialog')).backgroundColor) === 'rgb(43, 45, 49)', 'dark theme: the dialog is on the raised dark surface');
  await shot(page, 'passwords-add-card-dark.png');
  await emit(page, { event: 'theme', theme: 'light' });
  await page.fill('#k-number', '5555 5555 5555 4444');
  await page.fill('#k-month', '7');
  await page.fill('#k-year', '2031');
  await page.fill('#k-name', 'Alice Example');
  await page.fill('#k-nick', 'Personal');
  await page.locator('#card-dialog button[value="ok"]').click();
  await page.waitForTimeout(60);
  const ks = await lastSent(page, 'cardSave');
  check(ks && ks.card.id === null && ks.card.number === '5555 5555 5555 4444' && ks.card.expMonth === 7 && ks.card.expYear === 2031 && ks.card.nickname === 'Personal' && ks.card.address === '' && Object.keys(ks.card).sort().join() === 'address,expMonth,expYear,id,name,nickname,number', 'Add sends cardSave{card} without a security code (' + JSON.stringify(ks && ks.card) + ')');
  check(await page.inputValue('#k-number') === '' && (await page.locator('.card-row').count()) === 3, 'the number field is cleared and the list reloads');
  await page.locator('.card-row[data-id="k1"]').hover();
  await page.locator('.card-row[data-id="k1"] .icon-btn[title="Edit"]').click();
  check((await page.getAttribute('#k-number', 'placeholder')).includes('1111 (unchanged)') && (await page.getAttribute('#k-address', 'placeholder')) === '(unchanged)', 'edit keeps the stored number and address unless typed');
  await page.fill('#k-nick', 'Company Visa');
  await page.press('#k-nick', 'Enter');
  await page.waitForTimeout(60);
  const ke = await lastSent(page, 'cardSave');
  check(ke && ke.card.id === 'k1' && ke.card.number === null && ke.card.address === null && ke.card.nickname === 'Company Visa', 'edit sends number:null and address:null (unchanged) (' + JSON.stringify(ke && ke.card) + ')');
  await page.locator('.card-row[data-id="k2"]').hover();
  await page.locator('.card-row[data-id="k2"] .icon-btn[title="Delete"]').click();
  await page.waitForSelector('#lb-dialog-ok');
  check((await page.textContent('.lb-dialog-text')).includes('Mastercard card ending in 4444'), 'Delete asks first, naming the card');
  await page.click('#lb-dialog-ok');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'cardDelete')).id === 'k2' && (await page.locator('.card-row').count()) === 2, 'then sends cardDelete{id} and the list reloads');
  await page.click('#change');
  await page.fill('#c-old', 'correct horse battery');
  await page.fill('#c-new', 'new master pass 1');
  await page.fill('#c-new2', 'new master pass 2');
  await page.locator('#change-dialog button[value="ok"]').click();
  check((await page.textContent('#c-new2-error')).includes('do not match') && await page.evaluate(() => document.activeElement.id) === 'c-new2', 'change: the mismatch is shown under the repeat field, which gets the focus');
  // R14: every field has its label, a show/hide button and its own error line.
  const grid = await page.$$eval('#change-dialog .field-grid > label', (ns) => ns.map((n) => n.textContent + '>' + n.htmlFor));
  check(JSON.stringify(grid) === JSON.stringify(['Current password>c-old', 'New password>c-new', 'Repeat new password>c-new2']), 'labelled fields (' + grid.join(', ') + ')');
  await page.click('#c-new + .reveal');
  check(await page.$eval('#c-new', (e) => e.type) === 'text' && await page.$eval('#c-new + .reveal', (e) => e.getAttribute('aria-pressed')) === 'true', 'the eye shows the new password');
  check((await page.textContent('#c-strength')).startsWith('Strength:'), 'a strength meter for the new password (' + await page.textContent('#c-strength') + ')');
  await shot(page, 'passwords-change-dialog.png');
  await page.fill('#c-old', '');
  await page.fill('#c-new', 'short');
  await page.locator('#change-dialog button[value="ok"]').click();
  check((await page.textContent('#c-old-error')).includes('current master password') && (await page.textContent('#c-new-error')).includes('8 characters') && await page.evaluate(() => document.activeElement.id) === 'c-old', 'empty current and short new: both said at once, focus on the first');
  await page.fill('#c-old', 'wrong password');
  await page.fill('#c-new', 'new master pass 1');
  await page.fill('#c-new2', 'new master pass 1');
  await page.locator('#change-dialog button[value="ok"]').click();
  await page.waitForTimeout(60);
  check((await page.textContent('#c-old-error')).includes('Wrong master password') && await page.inputValue('#c-new') === 'new master pass 1', 'a wrong current password is said under it and nothing typed is lost');
  await page.fill('#c-old', 'correct horse battery');
  await page.keyboard.press('Enter');
  await page.waitForTimeout(60);
  const ch = await lastSent(page, 'vaultChangePassword');
  check(ch && ch.old === 'correct horse battery' && ch.new === 'new master pass 1' && !(await page.$eval('#change-dialog', (d) => d.open)), 'Enter sends old and new; the dialog closes');
  await page.click('#change');
  await page.keyboard.press('Escape');
  check(!(await page.$eval('#change-dialog', (d) => d.open)), 'Esc closes it');

  // R13: the recovery phrase card and its three steps.
  check((await page.textContent('#recovery-state')) === 'No recovery phrase' && (await page.textContent('#recovery-make')) === 'Create recovery phrase', 'no phrase yet: the card says so');
  await page.click('#recovery-make');
  await page.waitForSelector('#rd-step1:not([hidden])');
  await page.fill('#rd-pw', 'nope');
  await page.click('#rd-next1');
  await page.waitForTimeout(40);
  check((await page.textContent('#rd-pw-error')).includes('Wrong master password'), 'the master password comes first');
  await page.fill('#rd-pw', 'correct horse battery');
  await page.click('#rd-next1');
  await page.waitForSelector('#rd-step2:not([hidden])');
  const shownWords = await page.$$eval('#rd-words li', (ns) => ns.map((n) => n.textContent));
  check(shownWords.length === 24 && shownWords[23] === 'title', 'the 24 words, numbered in order');
  await shot(page, 'passwords-recovery-words.png');
  await page.click('#rd-copy');
  await page.click('#rd-save');
  await page.waitForTimeout(40);
  check((await lastSent(page, 'vaultRecoveryCopy')) !== undefined && (await lastSent(page, 'vaultRecoverySave')) !== undefined, 'Copy and Save to file ask the host (the words are not sent back)');
  await page.click('#rd-next2');
  await page.waitForSelector('#rd-step3:not([hidden])');
  const askedLabels = await page.$$eval('#rd-asked label', (ns) => ns.map((n) => n.textContent));
  check(JSON.stringify(askedLabels) === JSON.stringify(['Word 2', 'Word 9', 'Word 17']), 'three words are asked back (' + askedLabels.join(', ') + ')');
  await page.fill('#rd-a0', 'winner');
  await page.fill('#rd-a1', 'legal');
  await page.fill('#rd-a2', 'zoo');
  await page.click('#rd-confirm');
  await page.waitForTimeout(40);
  check((await page.textContent('#rd-error')).includes('Word 17 is not right'), 'a wrong word is named');
  await page.fill('#rd-a2', 'legal');
  await page.click('#rd-confirm');
  await page.waitForTimeout(60);
  check(!(await page.$eval('#recovery-dialog', (d) => d.open)) && (await page.textContent('#recovery-state')).startsWith('Recovery phrase made on') && (await page.textContent('#recovery-make')) === 'Make a new phrase', 'confirmed: the card shows the phrase and offers a new one');
  check(await page.evaluate(() => !window.__vault.cancelled) && await page.locator('#rd-words li').count() === 0, 'the words are gone from the page; nothing cancelled');
  // Closing half way tells the host to forget the words.
  await page.click('#recovery-make');
  await page.fill('#rd-pw', 'correct horse battery');
  await page.click('#rd-next1');
  await page.waitForSelector('#rd-step2:not([hidden])');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(40);
  check(await page.evaluate(() => window.__vault.cancelled === 1) && await page.locator('#rd-words li').count() === 0, 'Esc on the words: vaultRecoveryCancel, the words are cleared');
  await page.click('#recovery-remove');
  await page.fill('#rr-pw', 'correct horse battery');
  await page.locator('#recovery-remove-dialog button[value="ok"]').click();
  await page.waitForTimeout(60);
  check((await page.textContent('#recovery-state')) === 'No recovery phrase', 'Remove takes the phrase away (after the master password)');
  await page.evaluate(() => { window.__vault.recovery = 1794000000000; });
  await page.locator('#recovery').scrollIntoViewIfNeeded();
  await shot(page, 'passwords-recovery-card.png');
  // R15: the name everywhere.
  {
    const fs = require('fs');
    const files = ['pages/settings.html', 'pages/passwords.html', 'shell/popup.js'].map((f) => fs.readFileSync(path.join(__dirname, '..', f), 'utf8')).join('\n');
    check(!/Manage passwords(?! &(amp;)? payments)/.test(files) && /Manage passwords &amp; payments/.test(files) && (await page.textContent('h1')) === 'Passwords & payments', 'R15: "Manage passwords & payments" everywhere; the page is "Passwords & payments"');
  }
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');

  // R13: a forgotten master password, from the locked page.
  await page.context().close();
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/passwords.html', { width: 1000, height: 640 }, 1, vaultMock, { exists: true, locked: true, waitMs: 0, entries, recovery: 1794000000000 }));
  await page.waitForSelector('#unlock:not([hidden])');
  await page.click('#forgot');
  await page.waitForSelector('#recover-form:not([hidden])');
  await page.waitForTimeout(30);
  check(await page.evaluate(() => document.activeElement.id) === 'r-phrase', 'Forgot master password? opens the phrase form, focused');
  const words = 'legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title'.split(' ');
  await page.fill('#r-phrase', words.slice(0, 23).join(' '));
  check((await page.textContent('#r-phrase-msg')) === '23 of 24 words', 'it counts the words');
  await page.fill('#r-phrase', words.slice(0, 4).concat(['qqqq']).concat(words.slice(5)).join(' '));
  await page.waitForTimeout(400);
  check((await page.textContent('#r-phrase-msg')).includes('Word 5 not in the word list') && await page.$eval('#r-phrase-msg', (e) => e.classList.contains('bad')), 'an unknown word is named (' + await page.textContent('#r-phrase-msg') + ')');
  // Numbered, over several lines: fine.
  await page.fill('#r-phrase', words.map((w, i) => (i + 1) + '. ' + w).join('\n'));
  await page.waitForTimeout(400);
  check(await page.$eval('#r-phrase-msg', (e) => e.classList.contains('ok')), 'a numbered phrase over several lines is accepted');
  await page.fill('#r-new', 'brand new master');
  await page.fill('#r-new2', 'brand new mastery');
  await page.click('#recover-btn');
  check((await page.textContent('#r-pw-error')).includes('do not match'), 'the new password is checked first');
  await page.fill('#r-new2', 'brand new master');
  await shot(page, 'passwords-recover.png');
  await page.click('#recover-btn');
  await page.waitForSelector('#list-section:not([hidden])');
  const rec = await lastSent(page, 'vaultRecover');
  check(rec && rec.new === 'brand new master' && rec.phrase.includes('legal') && await page.evaluate(() => window.__vault.recoveredWith) === 'brand new master', 'vaultRecover{phrase,new} opens the vault');
  check(await page.inputValue('#r-phrase') === '' && await page.inputValue('#r-new') === '', 'the phrase and the password are cleared from the page');
  check(errors.length === 0, 'recover: no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
  // Without a phrase: the page says it cannot be recovered; #recover from the unlock popup.
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/passwords.html#recover', { width: 1000, height: 560 }, 1, vaultMock, { exists: true, locked: true, waitMs: 0, entries }));
  await page.waitForSelector('#recover:not([hidden])');
  check(await page.locator('#recover-none').isVisible() && await page.locator('#recover-form').isHidden() && (await page.textContent('#recover-none')).includes('no recovery phrase'), '#recover without a phrase: it says the vault cannot be opened');
  await page.click('#recover-none-back');
  check(await page.locator('#unlock').isVisible(), 'Back goes to the unlock form');
  await page.context().close();

  // Popup unlock prompt + save prompt.
  ({ page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 360, height: 262 }, 1));
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://bank.example', purpose: 'fill', error: null, waitMs: 0 } });
  await page.waitForSelector('#master');
  check(await page.evaluate(() => document.activeElement.id) === 'master', 'unlock prompt focuses the password field');
  await page.click('#forgot');
  {
    const op = await lastSent(page, 'openPage');
    check(op && op.page === 'passwords' && op.section === 'recover', 'the unlock prompt offers "Forgot master password?" (krunk://passwords#recover)');
  }
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://bank.example', purpose: 'fill', error: null, waitMs: 0 } });
  await page.waitForTimeout(60);
  await page.waitForSelector('#master');
  await page.fill('#master', 'correct horse battery');
  await shot(page, 'popup-unlock.png');
  await page.click('#unlock');
  await page.waitForTimeout(30);
  const un = await lastSent(page, 'vaultUnlock');
  check(un && un.password === 'correct horse battery' && un.remain === true, 'unlock prompt sends vaultUnlock, "Remain unlocked this session" checked by default (' + JSON.stringify(un) + ')');
  check(await page.evaluate(() => { const r = document.getElementById('remain').closest('label').getBoundingClientRect(); return r.bottom <= window.innerHeight && r.height > 0; }), 'the checkbox fits the prompt');
  // 0.11.0: the remembered choice is the default; unchecked, the unlock says so.
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://bank.example', purpose: 'fill', error: null, waitMs: 0, remain: false } });
  await page.waitForTimeout(60);
  check(!(await page.isChecked('#remain')), 'remembered "unchecked" shows unchecked');
  await page.click('#remain');
  await page.click('#remain');
  await page.fill('#master', 'pw');
  await page.click('#unlock');
  await page.waitForTimeout(30);
  check((await lastSent(page, 'vaultUnlock')).remain === false, 'unchecked: vaultUnlock{remain:false}');
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
  // 0.6.0 payment methods: the card dropdown (locked and unlocked) and the save offer.
  await emit(page, { event: 'popup', content: { kind: 'cardFill', tabId: 4, frameId: 'f1', origin: 'https://shop.example', locked: true, cards: [] } });
  await page.waitForSelector('#suggest-unlock');
  await page.click('#suggest-unlock');
  const cu = await lastSent(page, 'suggestUnlock');
  check(cu && cu.tabId === 4 && cu.frameId === 'f1', 'a locked card dropdown offers Unlock (suggestUnlock{tabId,frameId})');
  await emit(page, { event: 'popup', content: { kind: 'cardFill', tabId: 4, frameId: 'f1', origin: 'https://shop.example', locked: false, cards: [{ id: 'k1', label: 'Work Visa · Visa •••• 1111', name: 'Alice Example' }, { id: 'k2', label: 'Mastercard •••• 4444', name: '' }] } });
  await page.waitForSelector('.suggest.card');
  check((await page.locator('.suggest.card').count()) === 2 && (await page.textContent('.suggest.card[data-id="k1"]')).includes('•••• 1111') && !(await page.content()).includes('4111'), 'the card dropdown lists masked cards only');
  check(await page.evaluate(() => document.activeElement === document.body || document.activeElement === null || document.activeElement.tagName !== 'BUTTON'), 'the card dropdown does not steal the focus from the page field');
  await shot(page, 'popup-card-fill.png');
  await page.click('.suggest.card[data-id="k2"]');
  const cf = await lastSent(page, 'cardFillPick');
  check(cf && cf.tabId === 4 && cf.frameId === 'f1' && cf.cardId === 'k2', 'choosing a card sends cardFillPick{tabId,frameId,cardId}');
  await page.setViewportSize({ width: 360, height: 150 });
  await emit(page, { event: 'popup', content: { kind: 'saveCard', tabId: 4, origin: 'https://shop.example', brand: 'Visa', last4: '1111' } });
  await page.waitForSelector('#card-not-now');
  check((await page.textContent('h2')) === 'Save this card?' && (await page.textContent('.panel')).includes('Visa •••• 1111') && (await page.textContent('.panel')).includes('never stored'), 'the save offer shows the brand and last four and says the code is never stored');
  await shot(page, 'popup-save-card.png');
  await page.click('#card-not-now');
  const sc = await lastSent(page, 'saveCardReply');
  check(sc && sc.accept === false && sc.origin === 'https://shop.example' && sc.tabId === 4, 'Not now sends saveCardReply{accept:false}');
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://shop.example', purpose: 'cardFill', error: null, waitMs: 0 } });
  await page.waitForSelector('#master');
  check((await page.textContent('h2')) === 'Unlock to fill this card', 'the unlock prompt from the card dropdown says so');
  await emit(page, { event: 'popup', content: { kind: 'unlock', tabId: 4, origin: 'https://shop.example', purpose: 'saveCard', error: null, waitMs: 0 } });
  await page.waitForFunction(() => document.querySelector('h2')?.textContent === 'Unlock to save this card');
  check(true, 'the unlock prompt for a card to save says so');
  await emit(page, { event: 'popup', content: { kind: 'saveCard', tabId: 4, origin: 'https://shop.example', brand: 'Visa', last4: '1111' } });
  await page.waitForSelector('#default');
  await page.click('#default');
  check((await lastSent(page, 'saveCardReply')).accept === true, 'Save card sends saveCardReply{accept:true}');
  // 0.6.0: the Picture-in-Picture chooser.
  await page.setViewportSize({ width: 340, height: 300 });
  await emit(page, { event: 'popup', content: { kind: 'pip', tabId: 4, videos: [{ index: 0, label: 'trailer.mp4', width: 640, height: 360, playing: true, pip: false }, { index: 1, label: '', width: 0, height: 0, playing: false, pip: false }] } });
  await page.waitForSelector('.pip-row');
  check((await page.locator('.pip-row').count()) === 2 && (await page.textContent('.pip-row[data-index="0"]')).includes('640×360 · playing') && (await page.textContent('.pip-row[data-index="1"]')).includes('Video 2'), 'the chooser lists the videos with size and state');
  await shot(page, 'popup-pip-chooser.png');
  await page.click('.pip-row[data-index="1"]');
  const pp = await lastSent(page, 'pipPick');
  check(pp && pp.tabId === 4 && pp.index === 1, 'a row sends pipPick{tabId,index}');
  // 0.7.0 (R20): the install confirmation.
  await page.setViewportSize({ width: 400, height: 260 });
  await emit(page, { event: 'popup', content: { kind: 'extInstall', requestId: 7, name: 'uBlock Origin Lite', version: '2026.11.1', source: 'Chrome Web Store, signed by the Chrome Web Store', warnings: ['Read and change all your data on all websites', 'Block content on any page'] } });
  await page.waitForSelector('.ext-install');
  check((await page.textContent('h2')) === 'Add “uBlock Origin Lite”?' && await page.locator('.ext-warnings li').count() === 2 && (await page.textContent('.ext-meta')).includes('signed by the Chrome Web Store') && await page.evaluate(() => document.activeElement.id) === 'default', 'the confirmation names the extension, its source and what it can do; Add has the focus');
  await shot(page, 'popup-ext-install.png');
  await page.keyboard.press('Escape');
  check((await lastSent(page, 'extInstallReply')).accept === false, 'Esc declines');
  await emit(page, { event: 'popup', content: { kind: 'extInstall', requestId: 8, name: 'Dark Reader', version: '4.9', source: 'The file dark.crx', warnings: [], previous: '4.8' } });
  await page.waitForFunction(() => document.querySelector('h2') && document.querySelector('h2').textContent.startsWith('Update'));
  check((await page.textContent('h2')) === 'Update “Dark Reader”?' && (await page.textContent('.ext-install')).includes('no special permissions') && (await page.textContent('#default')) === 'Update', 'an update says so; no warnings: says it needs none');
  await page.click('#default');
  {
    const r = await lastSent(page, 'extInstallReply');
    check(r.requestId === 8 && r.accept === true, 'Update sends extInstallReply{requestId, accept:true}');
  }
  // 0.11.0: the volume slider inside the floating window, next to its own mute button: a
  // compact slider and the level, no mute of its own (the engine's button mutes).
  await page.setViewportSize({ width: 116, height: 28 });
  await emit(page, { event: 'popup', content: { kind: 'pipVolume', tabId: 4, volume: 60, muted: false } });
  await page.waitForSelector('#pv-slider');
  await page.waitForTimeout(40);
  const pvState = () => page.evaluate(() => ({ v: document.getElementById('pv-slider').value, pct: document.getElementById('pv-pct').textContent,
    label: document.getElementById('pv-slider').getAttribute('aria-valuetext'), mute: !!document.getElementById('pv-mute'), focused: document.activeElement && document.activeElement.id }));
  let pvs = await pvState();
  check(pvs.v === '60' && pvs.pct === '60' && pvs.label === '60%' && !pvs.mute && pvs.focused !== 'pv-slider', 'the slider shows the video\'s volume, has no mute of its own and takes no focus (' + JSON.stringify(pvs) + ')');
  check(await page.evaluate(() => document.body.classList.contains('pv-dark') && getComputedStyle(document.body).backgroundColor === 'rgba(32, 33, 36, 0.92)'), 'the slider is dark like the floating window');
  check(await page.evaluate(() => { const s = document.getElementById('pv-slider').getBoundingClientRect(); return s.width >= 60 && s.right <= 116 && s.bottom <= 28; }), 'it fits its 116×28 popup');
  await shot(page, 'popup-pip-volume.png');
  const box = await page.locator('#pv-slider').boundingBox();
  await page.mouse.click(box.x + box.width * 0.25, box.y + box.height / 2);
  await page.waitForTimeout(120);
  let pvSent = await lastSent(page, 'pipVolume');
  check(pvSent && pvSent.tabId === 4 && pvSent.volume >= 15 && pvSent.volume <= 35 && pvSent.muted === false, 'a click on the slider sends pipVolume (' + JSON.stringify(pvSent) + ')');
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, -100);
  await page.waitForTimeout(120);
  const afterWheel = (await lastSent(page, 'pipVolume')).volume;
  check(afterWheel === pvSent.volume + 5, 'the wheel over the slider raises it by 5 (' + afterWheel + ')');
  // The engine's mute button muted the video: the page reports it and the slider follows.
  await page.waitForTimeout(320);
  await emit(page, { event: 'popup', content: { kind: 'pipVolume', tabId: 4, volume: afterWheel, muted: true } });
  await page.waitForTimeout(40);
  pvs = await pvState();
  check(pvs.v === '0' && pvs.label === 'Muted' && pvs.pct === '—', 'muted by the player\'s button: the slider shows it (' + JSON.stringify(pvs) + ')');
  // The page's own change comes back (after the slider's own change has settled).
  await emit(page, { event: 'popup', content: { kind: 'pipVolume', tabId: 4, volume: 10, muted: false } });
  await page.waitForTimeout(40);
  pvs = await pvState();
  check(pvs.v === '10' && pvs.pct === '10', 'the page\'s volume change moves the slider (' + JSON.stringify(pvs) + ')');
  check(await page.evaluate(() => !!document.getElementById('pv-slider') && document.getElementById('root').dataset.kind === 'pipVolume'), 'updated in place');
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
  // 0.6.0: the Picture-in-Picture button: hidden without videos, a command for one, the
  // chooser for several, "leave" while a video is in the floating window.
  check(await page.locator('#pip').isHidden(), 'no PiP button without a video');
  await page.evaluate(() => window.__host.navEvent({ videos: 1, pip: false }));
  await page.waitForTimeout(40);
  check(await page.locator('#pip').isVisible() && (await page.getAttribute('#pip', 'title')) === 'Picture-in-Picture (Alt+P)', 'one video shows the PiP button');
  await page.click('#pip');
  check((await lastSent(page, 'command')).command.command === 'pictureInPicture', 'one video: the button sends the command');
  await page.evaluate(() => window.__host.navEvent({ videos: 2, pip: false }));
  await page.waitForTimeout(40);
  await page.click('#pip');
  const sp = await lastSent(page, 'showPopup');
  check(sp && sp.popup === 'pip' && sp.rect.width === 340, 'two videos: the button opens the chooser popup');
  await page.evaluate(() => window.__host.navEvent({ videos: 2, pip: true }));
  await page.waitForTimeout(40);
  check((await page.getAttribute('#pip', 'class')).includes('active') && (await page.getAttribute('#pip', 'title')).startsWith('Leave'), 'a video in PiP marks the button');
  await shot(page, 'shell-pip-active.png');
  await page.click('#pip');
  check((await lastSent(page, 'command')).command.command === 'pictureInPicture', 'in PiP the button sends the command (leave)');
  check(external.length === 0, 'no network requests');
}

// Mock of the settings host: simplified versions of the core rules (the real
// ones are unit-tested in core/src/settings.rs).
function settingsMock(defaults) {
  const h = window.__host.handlers;
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const running = clone(defaults);
  window.__running = running;
  const st = { settings: clone(defaults), allow: ['news.example.com'] };
  window.__settings = st;
  // The host derives blockThirdPartyCookies from the protection level (sync_protection).
  const PRESETS = { standard: { thirdPartyCookies: 'trackers' }, strict: { thirdPartyCookies: 'all' } };
  const sync = (s) => { const p = s.protection || { level: 'standard' }; const eff = p.level === 'custom' ? p : PRESETS[p.level]; s.blockThirdPartyCookies = eff.thirdPartyCookies === 'all'; return s; };
  const restart = (s) => ['hardwareAcceleration', 'proxy', 'extraFlags', 'blockThirdPartyCookies', 'engineSecurityComponents', 'protectedContent'].some((k) => sync(s)[k] !== running[k]);
  window.__bounce = { watched: 2, cleanedTotal: 5, lastCleanupMs: Date.now() - 3600e3 };
  h.bounceStatus = () => window.__bounce;
  // 0.6.0 custom DNS: presets, parsing and the stub's status come from the host.
  window.__dnsStatus = { route: 'system', spki: 'abc=' };
  h.dnsStatus = () => window.__dnsStatus;
  h.dnsParse = (m) => {
    if (m.text === '1.1.1.1') return { kind: 'udp', name: '1.1.1.1', host: '1.1.1.1', ip: '', port: 53, path: '', stamp: '' };
    if (m.text.startsWith('sdns://Agc')) return { kind: 'https', name: 'https://dns.cloudflare.com/dns-query', host: 'dns.cloudflare.com', ip: '1.1.1.1', port: 443, path: '/dns-query', stamp: m.text };
    throw new Error('Not a host name: ' + m.text);
  };
  h.dnsTest = (m) => { if (m.index === 0) return { ms: 23 }; throw new Error('no answer within 2000 ms'); };
  h.bounceCleanNow = () => { window.__bounce = { watched: 0, cleanedTotal: 7, lastCleanupMs: Date.now() }; return { cleaned: 2 }; };
  const UA_DEFAULT = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36';
  const UAS = [
    { id: 'chrome-windows', label: 'Chrome 157 on Windows', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36' },
    { id: 'firefox-windows', label: 'Firefox 159 on Windows', userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:159.0) Gecko/20100101 Firefox/159.0' },
    { id: 'safari-iphone', label: 'Safari 27 on iPhone', userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0 Mobile/15E148 Safari/604.1' },
  ];
  window.__uas = UAS;
  const DNS_PRESETS = [
    { id: 'quad9-dot', name: 'Quad9 (DoT)', resolver: { kind: 'tls', name: 'Quad9 (DoT)', host: 'dns.quad9.net', ip: '9.9.9.9', port: 853, path: '', stamp: '' } },
    { id: 'cloudflare-doh', name: 'Cloudflare (DoH)', resolver: { kind: 'https', name: 'Cloudflare (DoH)', host: 'cloudflare-dns.com', ip: '1.1.1.1', port: 443, path: '/dns-query', stamp: '' } },
  ];
  const WALLPAPERS = [['aurora', 'Aurora', 'dark', 25], ['dunes', 'Dunes', 'light', 0], ['nebula', 'Nebula', 'dark', 15]]
    .map(([id, title, look, dim]) => ({ id, title, look, dim, thumb: 'https://krunk.invalid/shared/wallpapers/' + id + '-thumb.webp' }));
  h.getSettings = () => ({ settings: st.settings, restartRequired: restart(st.settings), dnsPresets: DNS_PRESETS, wallpapers: WALLPAPERS, about: { version: '0.1.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' }, userAgents: UAS, defaultUserAgent: UA_DEFAULT });
  h.importDetect = () => [
    { id: 'firefox:abcd.default-release', browser: 'firefox', label: 'Firefox – default-release', path: 'C:\\Users\\me\\AppData\\Roaming\\Mozilla\\Firefox\\Profiles\\abcd.default-release', isDefault: true, hasHistory: true, hasBookmarks: true, hasPasswords: true },
    { id: 'chrome:Default', browser: 'chrome', label: 'Google Chrome – Person 1 (Default)', path: 'C:\\Users\\me\\AppData\\Local\\Google\\Chrome\\User Data\\Default', isDefault: true, hasHistory: true, hasBookmarks: false, hasPasswords: true },
  ];
  h.importRun = (m) => {
    if (m.profile === 'chrome:Default') return { history: { imported: 120, merged: 3 }, passwords: { imported: 0, merged: 0, skipped: 0, appBound: 2 } };
    return { history: { imported: 9, merged: 0 }, bookmarks: { imported: 1, folders: 0, merged: 0 }, passwords: { imported: 1, updated: 1, merged: 2, skipped: 3, skippedReasons: [{ reason: 'no website address', count: 1 }, { reason: 'unreadable entry', count: 2 }], appBound: 0 } };
  };
  h.importCsv = () => ({ passwords: { imported: 2, merged: 0, skipped: 1, appBound: 0 } });
  h.saveSettings = (m) => {
    const s = m.settings;
    const errs = [];
    if (s.proxy && !/^((https?|socks[45]):\/\/)?[A-Za-z0-9.-]+:\d{1,5}$/.test(s.proxy)) errs.push({ field: 'proxy', message: 'Use host:port, scheme://host:port (http, https, socks4, socks5) or direct://' });
    if (s.searchEngine === 'custom' && !/%s|\{searchTerms\}/.test(s.customSearchUrl)) errs.push({ field: 'customSearchUrl', message: 'Must contain %s where the search terms go' });
    if (s.extraFlags.split(/\s+/).some((t) => t && !/^--[A-Za-z0-9][\w.-]*(=[^\s"'`;&|<>]+)?$/.test(t))) errs.push({ field: 'extraFlags', message: 'Not a valid flag (use --flag or --flag=value)' });
    if (s.tabSleepMinutes > 1440) errs.push({ field: 'tabSleepMinutes', message: 'Must be between 0 and 1440 minutes' });
    if (s.dns && s.dns.mode === 'custom' && !(s.dns.resolvers || []).some((r) => r.kind !== 'dnsCrypt')) errs.push({ field: 'dns', message: 'Custom DNS needs at least one supported resolver' });
    const ro = (s.headers && s.headers.rules || []).find((r) => /^(cookie|host|referer)$/i.test(r.name));
    if (ro) errs.push({ field: 'headers.rules', message: ro.name + ': ' + ro.name + ' is read-only: the cookie jar' });
    if (errs.length) { const e = new Error('Some settings are not valid'); e.data = errs; throw e; }
    st.settings = s;
    return { settings: s, restartRequired: restart(s) };
  };
  h.resetSettings = () => { st.settings = clone(defaults); return { settings: st.settings, restartRequired: false }; };
  h.getFilterStatus = () => ({ enabled: true, lastCheck: Date.now() - 3 * 3600e3, lastError: null, busy: false, userRules: 3, lists: [
    { id: 'easylist', title: 'EasyList (ads)', builtin: true, enabled: true, source: 'downloaded', fetched: Date.now() - 3 * 3600e3 },
    { id: 'easyprivacy', title: 'EasyPrivacy (trackers)', builtin: true, enabled: true, source: 'downloaded', fetched: Date.now() - 3 * 3600e3 },
    { id: 'ublock-filters', title: 'uBlock filters', builtin: true, enabled: true, source: 'embedded', fetched: null },
    { id: 'ublock-privacy', title: 'uBlock filters – Privacy', builtin: true, enabled: false, source: 'embedded', fetched: null },
    { id: 'custom-1', title: 'My list', builtin: false, enabled: true, source: 'downloaded', fetched: Date.now() },
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
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html', { width: 1000, height: 1600 }, 1, settingsMock, defaults);
  await page.waitForFunction(() => document.getElementById('lists-summary').textContent !== '');
  // 0.5: the page is tabbed; a control must be on the shown tab before it is used.
  const tab = async (name) => { await page.click('#tabs a[data-tab="' + name + '"]'); await page.waitForTimeout(20); };
  check(await page.locator('.panel.active[data-tab=general]').count() === 1 && await page.locator('#tabs a.active[data-tab=general]').count() === 1, 'the General tab is shown first');
  check(await page.locator('.panel').count() === 9 && await page.locator('.panel.active').count() === 1, 'nine tabs (Account since 0.8.0, AI and scripts since 0.12.0), one shown');
  check(await page.inputValue('#searchEngine') === 'duckDuckGo', 'defaults: DuckDuckGo');
  // 0.6.0: new-tab behaviour (defaults keep the 0.5 behaviour).
  check(await page.inputValue('#newTabPage') === 'krunk' && await page.inputValue('#newTabPosition') === 'end' && await page.inputValue('#linkTabs') === 'auto' && await page.inputValue('#ntpSearchUrl') === '', 'new-tab defaults: Krunk page, at the end, links as asked, default search');
  check(await page.locator('#new-tab-url-row').isHidden(), 'the custom address row is hidden unless "custom"');
  // 0.6.0 permissions: the defaults and the per-site table.
  await tab('permissions');
  check(await page.locator('[data-key^="permissions."]').count() === 8 && await page.inputValue('[data-key="permissions.camera"]') === 'ask', 'eight permission defaults, all Ask');
  await page.selectOption('[data-key="permissions.midi"]', 'block');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.permissions.midi === 'block', 'a default change is saved');
  await page.selectOption('[data-key="permissions.midi"]', 'ask');
  await page.waitForTimeout(60);
  await page.evaluate(() => { window.__host.handlers.permissionExceptions = () => [{ origin: 'https://a.example', kind: 'camera', allow: true }, { origin: 'https://b.example', kind: 'geolocation', allow: false }]; window.__host.handlers.permissionSet = () => null; });
  await page.evaluate(() => LB.request('permissionExceptions').then(() => {}));
  await page.waitForTimeout(60);
  await tab('general');
  await tab('permissions');
  await page.waitForFunction(() => document.querySelectorAll('#perm-rows tr[data-origin]').length === 2);
  check(await page.locator('#perm-rows tr[data-origin]').count() === 2 && (await page.textContent('#perm-rows')).includes('https://b.example'), 'the exceptions table lists the stored answers');
  await page.click('#perm-rows tr[data-origin="https://a.example"] button');
  await page.waitForTimeout(40);
  {
    const ps = await lastSent(page, 'permissionSet');
    check(ps && ps.origin === 'https://a.example' && ps.kind === 'camera' && ps.state === 'ask', 'Remove sends permissionSet{state: ask}');
  }
  await page.selectOption('#perm-rows tr[data-origin="https://b.example"] select', 'allow');
  await page.waitForTimeout(40);
  check((await lastSent(page, 'permissionSet')).state === 'allow', 'changing an answer sends permissionSet');
  await tab('general');
  // 0.6.0 appearance: the accent swatches and the toolbar-item toggles; 0.7.0: real colour pickers.
  await tab('appearance');
  const accentRow = '.row:has([data-key="appearance.accent"])';
  check(await page.locator(accentRow + ' .swatch').count() === 8 && await page.locator(accentRow + ' .swatch.on[data-color=""]').count() === 1, 'eight accent swatches, the default selected');
  check(await page.locator('input[type=color]').count() === 5 && await page.locator('.reset-color').count() === 5, 'a colour dialog and Reset for the accent, toolbar, strip, background and new tab page');
  await page.click(accentRow + ' .swatch[data-color="#16a34a"]');
  await page.waitForTimeout(60);
  {
    const saved = await lastSent(page, 'saveSettings');
    check(saved && saved.settings.appearance.accent === '#16a34a', 'a swatch saves the accent');
    const acc = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--accent').trim());
    check(acc === '#16a34a', 'and the page uses it at once (' + acc + ')');
  }
  // A custom colour from the dialog: live while it moves, saved once more at the end.
  {
    const before = (await sent(page)).filter((m) => m.cmd === 'saveSettings').length;
    await page.evaluate(() => {
      const c = document.querySelector('.row:has([data-key="appearance.toolbarColor"]) input[type=color]');
      ['#101828', '#1d2939', '#344054'].forEach((v) => { c.value = v; c.dispatchEvent(new Event('input', { bubbles: true })); });
    });
    await page.waitForTimeout(250);
    const saves = (await sent(page)).filter((m) => m.cmd === 'saveSettings').slice(before);
    check(saves.length >= 1 && saves.length <= 2 && saves[saves.length - 1].settings.appearance.toolbarColor === '#344054', 'the dialog\'s colour applies live, throttled (' + saves.length + ' saves)');
    check(await page.inputValue('[data-key="appearance.toolbarColor"]') === '#344054', 'the hex field follows the dialog');
    await page.fill('[data-key="appearance.toolbarColor"]', '#fde68a');
    await page.press('[data-key="appearance.toolbarColor"]', 'Tab');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'saveSettings')).settings.appearance.toolbarColor === '#fde68a' && await page.evaluate(() => document.querySelector('.row:has([data-key="appearance.toolbarColor"]) input[type=color]').value) === '#fde68a', 'typing a hex value saves it and moves the dialog\'s colour');
    await page.click('.row:has([data-key="appearance.toolbarColor"]) .reset-color');
    await page.waitForTimeout(60);
    check((await lastSent(page, 'saveSettings')).settings.appearance.toolbarColor === '' && await page.locator('.row:has([data-key="appearance.toolbarColor"]) .reset-color').isDisabled(), 'Reset to default clears it');
  }
  // The host's theme message carries the readable text colour for a custom colour (WCAG).
  await emit(page, { event: 'theme', theme: 'light', accent: '#ffe066', colors: { accent: '#ffe066', accentFg: '#111827', background: '#0f172a', backgroundFg: '#ffffff' } });
  await page.waitForTimeout(30);
  {
    const v = await page.evaluate(() => { const cs = getComputedStyle(document.documentElement); return [cs.getPropertyValue('--accent-fg').trim(), cs.getPropertyValue('--on-bg').trim(), getComputedStyle(document.querySelector('.panel.active .panel-title')).color]; });
    check(v[0] === '#111827' && v[1] === '#ffffff' && v[2] === 'rgb(255, 255, 255)', 'the page uses the readable text colours (' + v.join(', ') + ')');
  }
  await emit(page, { event: 'theme', theme: 'light' });
  // 0.7.0 (R2): the new tab page background: built-in wallpapers, a colour, your own image.
  check(await page.locator('#settings-wallpapers .wallpaper').count() === 3, 'the wallpaper thumbnails');
  await page.click('#settings-wallpapers .wallpaper[data-id="dunes"]');
  await page.waitForTimeout(60);
  {
    const n = (await lastSent(page, 'saveSettings')).settings.ntp;
    check(n.background === 'wallpaper' && n.wallpaper === 'dunes' && n.imageDim === 0, 'a thumbnail saves the wallpaper with its usual darkening');
    check(await page.locator('#settings-wallpapers .wallpaper.on[data-id="dunes"]').count() === 1, 'and shows it chosen');
  }
  await page.fill('[data-key="ntp.backgroundColor"]', '#123456');
  await page.press('[data-key="ntp.backgroundColor"]', 'Tab');
  await page.waitForTimeout(60);
  {
    const n = (await lastSent(page, 'saveSettings')).settings.ntp;
    check(n.background === 'color' && n.backgroundColor === '#123456', 'a new tab page colour becomes its background');
  }
  await page.click('#ntp-no-background');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.ntp.background === 'none', 'No background');
  await page.uncheck('[data-key="appearance.showZoomChip"]');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.appearance.showZoomChip === false, 'hiding the zoom indicator is saved');
  await page.check('[data-key="appearance.showZoomChip"]');
  await page.waitForTimeout(60);
  await page.click(accentRow + ' .swatch[data-color=""]');
  await page.waitForTimeout(60);
  await tab('general');
  await page.selectOption('#newTabPage', 'custom');
  await page.waitForTimeout(60);
  check(await page.locator('#new-tab-url-row').isVisible(), 'choosing "custom" shows the address row');
  {
    const saved = await lastSent(page, 'saveSettings');
    check(saved && saved.settings.newTabPage === 'custom', 'the choice is saved (newTabPage=custom)');
  }
  await page.selectOption('#newTabPage', 'krunk');
  await page.waitForTimeout(60);
  check(await page.isChecked('input[name=startup][value=restoreSession]'), 'defaults: restore session');
  check(!(await page.isChecked('#autoPip')), 'defaults: automatic Picture-in-Picture off');
  await page.click('#autoPip');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.autoPip === true, 'automatic Picture-in-Picture can be switched on');
  await page.click('#autoPip');
  await page.waitForTimeout(60);
  check((await page.locator('#trackingPrevention, [data-key=smartscreen]').count()) === 0, 'no Edge-only settings');
  check(!(await page.isChecked('[data-key=engineSecurityComponents]')), 'defaults: no engine downloads from Google');
  // 0.6.0: protected content (Widevine) off by default, needs a restart, shows the CDM status.
  await tab('privacy');
  check(!(await page.isChecked('#protectedContent')) && (await page.textContent('#cdm-status')).includes('not downloaded yet'), 'defaults: protected content off, no Widevine module');
  await page.click('#protectedContent');
  await page.waitForTimeout(60);
  {
    const sv = await lastSent(page, 'saveSettings');
    check(sv && sv.settings.protectedContent === true && (await page.locator('#restart-banner').isVisible()), 'protected content on: saved and the restart banner shows');
  }
  await page.click('#protectedContent');
  await page.waitForTimeout(60);
  await tab('general');
  check((await page.textContent('#about')).includes('Chromium 156.0.8078.80 (CEF 156.0.15)'), 'engine versions in the footer');
  check(await page.isChecked('[data-key=hardwareAcceleration]'), 'defaults: GPU on');
  await tab('passwords');
  check(await page.inputValue('[data-key="passgen.length"]') === '20' && await page.isChecked('[data-key="passgen.symbols"]'), 'generated passwords: 20 characters with symbols by default');
  check(await page.isChecked('#offerSaveCards'), 'defaults: offer to save payment cards');
  await page.click('#offerSaveCards');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.offerSaveCards === false, 'the card offer can be switched off');
  await page.click('#offerSaveCards');
  await page.waitForTimeout(60);
  await page.fill('[data-key="passgen.length"]', '32');
  await page.press('[data-key="passgen.length"]', 'Tab');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.passgen.length === 32, 'the generator length is saved');
  await page.fill('[data-key="passgen.length"]', '20');
  await page.press('[data-key="passgen.length"]', 'Tab');
  await page.waitForTimeout(60);
  await tab('advanced');
  check(await page.isChecked('input[name=plevel][value=standard]') && await page.locator('input[name=plevel]:checked').count() === 1, 'defaults: tracking protection Standard');
  check(await page.locator('#restart-banner').isHidden(), 'no restart banner at first');
  check((await page.textContent('#lists-summary')) === '4 of 5 filter lists on (1 added by you) · 3 filters of your own', 'filter lists summarized (' + await page.textContent('#lists-summary') + ')');
  check((await page.getAttribute('#manage-lists', 'href')) === 'adblock.html#lists' && (await page.getAttribute('#manage-filters', 'href')) === 'adblock.html#filters', 'links to the lists and My filters on krunk://adblock');
  check(await page.inputValue('#bookmarks-bar') === 'always', 'defaults: bookmarks bar always shown');
  check(await page.inputValue('#downloadConnections') === '4', 'defaults: 4 connections per download');
  await tab('downloads');
  await page.selectOption('#downloadConnections', '1');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.downloadConnections) === 1, 'connections per download: 1 (off) saved as a number');
  await page.selectOption('#downloadConnections', '4');
  await page.waitForTimeout(80);
  await tab('general');
  await page.selectOption('#bookmarks-bar', 'newTab');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.bookmarksBar) === 'newTab', 'bookmarks bar: "only on new tabs" saved');
  await page.selectOption('#bookmarks-bar', 'always');
  await page.waitForTimeout(80);
  await page.waitForTimeout(100);
  await shot(page, 'settings-light.png');

  // Inline validation: invalid proxy is rejected and shown, nothing stored.
  await tab('advanced');
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
  await tab('general');
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
  await tab('advanced');
  await page.fill('#extraFlags', '--force-dark-mode "--x"');
  await page.press('#extraFlags', 'Tab');
  await page.waitForTimeout(60);
  check((await page.textContent('[data-error-for=extraFlags]')).length > 0, 'malformed extra flag rejected inline');
  await page.fill('#extraFlags', '');
  await page.press('#extraFlags', 'Tab');
  await tab('general');
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
  await page.fill('#homepage', 'https://slow.example/');
  await page.press('#homepage', 'Tab');
  await page.fill('#tabSleepMinutes', '7');
  await page.waitForTimeout(350);
  check(await page.inputValue('#tabSleepMinutes') === '7', 'a save reply does not overwrite the field being edited');
  await page.press('#tabSleepMinutes', 'Tab');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'saveSettings')).settings.tabSleepMinutes === 7, 'the edited field is then saved');
  await page.fill('#homepage', 'https://duckduckgo.com/');
  await page.press('#homepage', 'Tab');
  await page.fill('#tabSleepMinutes', '5');
  await page.press('#tabSleepMinutes', 'Tab');
  await page.waitForTimeout(60);
  // Tracking protection (0.6.0): the presets show their values greyed; Strict blocks all
  // third-party cookies, which the host turns into the engine switch (restart banner).
  await tab('privacy');
  check(await page.locator('.protection-part [data-key]').count() === 6 && await page.locator('.protection-part [data-key]:disabled').count() === 6, 'the six parts are shown disabled under a preset');
  check(await page.inputValue('[data-key="protection.thirdPartyCookies"]') === 'trackers' && await page.inputValue('[data-key="protection.referrer"]') === 'origin' && !(await page.isChecked('[data-key="protection.fingerprinting"]')) && await page.isChecked('[data-key="protection.cryptomining"]'), 'Standard: trackers\' cookies, origin referrer, no fingerprinting list, NoCoin on');
  check((await page.textContent('#bounce-status')).startsWith('2 sites seen only as a redirect · 5 cleared so far · last cleanup '), 'bounce-tracker status (' + await page.textContent('#bounce-status') + ')');
  await page.check('input[name=plevel][value=strict]');
  await page.waitForTimeout(60);
  {
    const tp = await lastSent(page, 'saveSettings');
    check(tp && tp.settings.protection.level === 'strict' && await page.locator('#restart-banner').isVisible(), 'Strict saves the level and shows the restart banner (all third-party cookies need the engine switch)');
    check(await page.inputValue('[data-key="protection.thirdPartyCookies"]') === 'all' && await page.inputValue('[data-key="protection.referrer"]') === 'none' && await page.isChecked('[data-key="protection.fingerprinting"]'), 'Strict: all cookies, no referrer, fingerprinting on');
    check(tp.settings.protection.fingerprinting === false, 'the stored custom choices are untouched by a preset');
  }
  await page.check('input[name=plevel][value=custom]');
  await page.waitForTimeout(60);
  check(await page.locator('.protection-part [data-key]:disabled').count() === 0 && !(await page.isChecked('[data-key="protection.fingerprinting"]')) && await page.inputValue('[data-key="protection.thirdPartyCookies"]') === 'trackers', 'Custom enables the parts with the stored choices');
  await page.selectOption('[data-key="protection.referrer"]', 'none');
  await page.check('[data-key="protection.fingerprinting"]');
  await page.waitForTimeout(60);
  {
    const tp = await lastSent(page, 'saveSettings');
    check(tp.settings.protection.level === 'custom' && tp.settings.protection.referrer === 'none' && tp.settings.protection.fingerprinting === true && tp.settings.protection.thirdPartyCookies === 'trackers', 'Custom choices are saved');
    // (The proxy set earlier in this test keeps the banner up; the cookie policy alone would not.)
    const banner = await page.evaluate(() => !document.getElementById('restart-banner').hidden);
    const expected = await page.evaluate(() => ['hardwareAcceleration', 'proxy', 'extraFlags', 'engineSecurityComponents', 'protectedContent'].some((k) => window.__settings.settings[k] !== window.__running[k]));
    check(banner === expected && tp.settings.blockThirdPartyCookies !== true, 'Custom with trackers-only cookies does not ask for a restart by itself');
  }
  await page.click('#bounce-clean');
  await page.waitForTimeout(80);
  check((await lastSent(page, 'bounceCleanNow')) !== undefined && (await page.textContent('#bounce-status')).startsWith('0 sites seen only as a redirect · 7 cleared'), 'Clean now clears the due bounce trackers and refreshes the status');
  await page.check('input[name=plevel][value=standard]');
  await page.waitForTimeout(60);
  // 0.6.0 custom DNS: the card, presets, parsing, order, Test and the mode.
  check(await page.isChecked('input[name=dnsmode][value=system]') && (await page.textContent('#dns-list')).includes('No resolvers yet') && await page.locator('#dns-preset option').count() === 3, 'DNS: system by default, no resolvers, the presets listed');
  await page.check('input[name=dnsmode][value=custom]');
  await page.waitForTimeout(80);
  check((await page.textContent('[data-error-for=dns]')).includes('at least one') && await page.evaluate(() => window.__settings.settings.dns.mode) === 'system', 'Custom without a resolver is refused and not stored');
  await page.check('input[name=dnsmode][value=system]');
  await page.waitForTimeout(60);
  await page.selectOption('#dns-preset', 'quad9-dot');
  await page.waitForTimeout(80);
  await page.fill('#dns-address', 'sdns://AgcAAAAAAAAABzEuMS4xLjEAEmRucy5jbG91ZGZsYXJlLmNvbQovZG5zLXF1ZXJ5');
  await page.click('#dns-form button');
  await page.waitForTimeout(80);
  await page.fill('#dns-address', '1.1.1.1');
  await page.click('#dns-form button');
  await page.waitForTimeout(80);
  {
    const d = await page.evaluate(() => window.__settings.settings.dns);
    check(d.resolvers.length === 3 && d.resolvers[0].kind === 'tls' && d.resolvers[1].stamp.startsWith('sdns://') && d.resolvers[2].kind === 'udp', 'a preset, a stamp and a plain address are added in order (' + d.resolvers.map((r) => r.kind).join(',') + ')');
    check(await page.locator('#dns-list .resolver').count() === 3 && (await page.textContent('#dns-list .resolver[data-index="0"]')).includes('DoT') && (await page.textContent('#dns-list .resolver[data-index="0"]')).includes('via 9.9.9.9') && await page.inputValue('#dns-address') === '', 'the list shows the kind and the bootstrap address, the form is cleared');
  }
  await page.fill('#dns-address', 'bad host');
  await page.click('#dns-form button');
  await page.waitForTimeout(80);
  check((await page.textContent('#dns-error')).includes('Not a host name') && await page.evaluate(() => window.__settings.settings.dns.resolvers.length) === 3, 'a bad address is refused with the host\'s reason');
  await page.locator('#dns-list .resolver[data-index="2"] .icon-btn[title="Move up"]').click();
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.dns.resolvers.map((r) => r.kind).join(',')) === 'tls,udp,https', 'Move up reorders the list');
  await page.evaluate(() => { window.__dnsStatus = { route: 'stub', port: 51234, spki: 'abc=', resolvers: [{ queries: 12, failures: 1, lastMs: 31, lastError: '' }, { queries: 0, failures: 0, lastMs: 0, lastError: '' }, { queries: 0, failures: 0, lastMs: 0, lastError: '' }] }; });
  await page.check('input[name=dnsmode][value=custom]');
  await page.waitForTimeout(120);
  check(await page.evaluate(() => window.__settings.settings.dns.mode) === 'custom' && (await page.textContent('#dns-status')).includes('loopback stub on port 51234') && (await page.textContent('#dns-list .resolver[data-index="0"]')).includes('12 queries, 1 failed, last 31 ms'), 'Custom with resolvers is stored; the status names the stub and the counters show');
  await page.locator('#dns-list .resolver[data-index="0"] .test').click();
  await page.waitForTimeout(80);
  check((await page.textContent('#dns-list .resolver[data-index="0"]')).includes('OK, 23 ms') && (await lastSent(page, 'dnsTest')).index === 0, 'Test shows the latency');
  await page.locator('#dns-list .resolver[data-index="1"] .test').click();
  await page.waitForTimeout(80);
  check((await page.textContent('#dns-list .resolver[data-index="1"]')).includes('failed: no answer within 2000 ms'), 'a failing Test shows the error');
  await page.locator('#dns-list .resolver[data-index="2"] .icon-btn[title^="Remove"]').click();
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.dns.resolvers.length) === 2, 'Remove drops a resolver');
  await page.check('input[name=dnsmode][value=system]');
  await page.waitForTimeout(60);
  // Engine security components: opt-in, restart-only.
  await page.check('[data-key=engineSecurityComponents]');
  await page.waitForTimeout(60);
  const ec = await lastSent(page, 'saveSettings');
  check(ec && ec.settings.engineSecurityComponents === true && await page.locator('#restart-banner').isVisible(), 'engine components toggle saves and shows the restart banner');
  await page.uncheck('[data-key=engineSecurityComponents]');
  await page.waitForTimeout(60);
  // Restart badge for the GPU toggle (restart-only) and restart button.
  await tab('advanced');
  await page.uncheck('[data-key=hardwareAcceleration]');
  await page.waitForTimeout(60);
  check(await page.locator('#restart-banner').isVisible(), 'GPU toggle shows the restart banner');
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(50);
  if (SHOTS) await page.screenshot({ path: path.join(SHOTS, 'settings-restart-banner.png'), clip: { x: 0, y: 0, width: 1000, height: 420 } });
  await page.click('#restart');
  check((await lastSent(page, 'restart')) !== undefined, 'Restart now sends restart');

  // Allowlist, folder picker, update now.
  await tab('privacy');
  await page.fill('#allow-host', 'bad host');
  await page.click('#allow-form button');
  await page.waitForTimeout(50);
  check((await page.textContent('#allow-error')).includes('valid host'), 'allowlist validation error');
  await page.fill('#allow-host', 'shop.example.org');
  await page.click('#allow-form button');
  await page.waitForTimeout(50);
  check((await page.locator('#allowlist .row').count()) === 2, 'allowlist entry added');
  await tab('downloads');
  await page.click('#pick-dir');
  await page.waitForTimeout(80);
  check((await page.textContent('#download-dir')) === 'D:\\Downloads\\Browser', 'download folder from the native picker');
  await tab('privacy');
  await page.click('#update-lists');
  check((await lastSent(page, 'updateFilterLists')) !== undefined, 'Update now sends updateFilterLists');
  await tab('advanced');

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
  await tab('appearance');
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
  await tab('advanced');
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

  // 0.5: the user-agent picker. The default is shown read-only; a preset stores its string;
  // Custom… keeps the free-text field with the old validation.
  await tab('advanced');
  check((await page.textContent('#default-ua')).includes('Chrome/156.0.0.0') && await page.inputValue('#userAgentPreset') === '', 'the default user agent is shown, and the picker says Krunk (default)');
  check(await page.locator('#custom-ua-row').isHidden(), 'the custom field is hidden while a preset is chosen');
  check((await page.textContent('#custom-ua-row, .row[data-keywords*="picker"]')).includes('no restart'), 'the picker says no restart is needed');
  await page.selectOption('#userAgentPreset', 'firefox-windows');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.userAgent) === (await page.evaluate(() => window.__uas[1].userAgent)), 'choosing Firefox stores its user-agent string');
  check(await page.locator('#restart-banner').isHidden(), 'no restart banner for the user agent');
  await page.selectOption('#userAgentPreset', 'custom');
  await page.waitForTimeout(40);
  check(await page.locator('#custom-ua-row').isVisible() && await page.inputValue('#userAgent') === (await page.evaluate(() => window.__uas[1].userAgent)), 'Custom… shows the field with the current string');
  await page.fill('#userAgent', 'Mine/1.0 \u00e9');
  await page.press('#userAgent', 'Tab');
  await page.waitForTimeout(80);
  check((await page.textContent('[data-error-for=userAgent]')).length > 0 || await page.evaluate(() => window.__settings.settings.userAgent) === 'Mine/1.0 \u00e9', 'the custom field keeps the host validation path');
  await page.fill('#userAgent', 'Mine/1.0');
  await page.press('#userAgent', 'Tab');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.userAgent) === 'Mine/1.0' && await page.inputValue('#userAgentPreset') === 'custom', 'a custom string is saved and the picker says Custom');
  await page.selectOption('#userAgentPreset', '');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.userAgent) === '' && await page.locator('#custom-ua-row').isHidden(), 'Krunk (default) clears the string');
  // 0.6.0: request headers (Accept-Language, client hints, custom rules, read-only list, Reset).
  check(await page.locator('#hint-fields').isHidden() && !(await page.isChecked('#hints-on')) && await page.locator('#readonly-headers .name').count() === 13, 'client hints off by default, the read-only list has 13 rows');
  // 0.7.0 (R12): "Send client hints", on by default; off disables the custom hints.
  check(await page.isChecked('#sendClientHints'), 'Send client hints: on by default');
  await page.uncheck('#sendClientHints');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.headers.sendClientHints) === false && await page.isDisabled('#hints-on'), 'off is saved; custom hints cannot be set while none are sent');
  await page.check('#sendClientHints');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.headers.sendClientHints) === true && !(await page.isDisabled('#hints-on')), 'and on again');
  await page.fill('#acceptLanguage', 'de-DE, de');
  await page.press('#acceptLanguage', 'Tab');
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.headers.acceptLanguage) === 'de-DE, de', 'Accept-Language is saved');
  await page.check('#hints-on');
  await page.waitForTimeout(80);
  {
    const h = await page.evaluate(() => window.__settings.settings.headers.hints);
    check(h && h.platform === 'Windows' && h.brands.length === 2 && h.brands[1].brand === 'Chromium' && await page.locator('#hint-fields').isVisible(), 'the hints switch stores the engine\'s values and shows the fields (' + JSON.stringify(h) + ')');
  }
  await page.fill('#hint-platform', 'Krunk OS');
  await page.press('#hint-platform', 'Tab');
  await page.fill('#hint-brands', '"Chromium";v="156", "Krunk";v="0.6"');
  await page.press('#hint-brands', 'Tab');
  await page.waitForTimeout(80);
  {
    const h = await page.evaluate(() => window.__settings.settings.headers.hints);
    check(h.platform === 'Krunk OS' && h.brands.length === 2 && h.brands[1].brand === 'Krunk' && h.brands[1].version === '0.6', 'platform and brands edits are saved (' + JSON.stringify(h.brands) + ')');
  }
  await page.fill('#header-name', 'X-Krunk-Test');
  await page.fill('#header-value', '1');
  await page.click('#header-form button');
  await page.waitForTimeout(80);
  await page.fill('#header-name', 'X-Page-Header');
  await page.fill('#header-sites', 'krunk-test.test, example.com');
  await page.click('#header-form button');
  await page.waitForTimeout(80);
  {
    const r = await page.evaluate(() => window.__settings.settings.headers.rules);
    check(r.length === 2 && r[0].name === 'X-Krunk-Test' && r[0].value === '1' && r[0].sites.length === 0 && r[1].value === null && r[1].sites.join() === 'krunk-test.test,example.com', 'two rules: a set header everywhere and a removal on two sites (' + JSON.stringify(r) + ')');
    check(await page.locator('#header-rules .rule').count() === 2 && (await page.textContent('#header-rules')).includes('(removed)') && await page.inputValue('#header-name') === '', 'the rules are listed and the form cleared');
  }
  await page.fill('#header-name', 'Cookie');
  await page.click('#header-form button');
  await page.waitForTimeout(80);
  check((await page.textContent('#header-error')).includes('read-only') && await page.evaluate(() => window.__settings.settings.headers.rules.length) === 2, 'a read-only name is refused with the reason (' + await page.textContent('#header-error') + ')');
  await page.locator('#header-rules .rule[data-name="X-Krunk-Test"] .icon-btn').click();
  await page.waitForTimeout(80);
  check(await page.evaluate(() => window.__settings.settings.headers.rules.length) === 1, 'Remove drops the rule');
  await page.click('#headers-reset');
  await page.waitForTimeout(80);
  {
    const h = await page.evaluate(() => window.__settings.settings.headers);
    check(h.acceptLanguage === '' && h.hints === null && h.rules.length === 0 && await page.evaluate(() => window.__settings.settings.userAgent) === '' && await page.locator('#hint-fields').isHidden(), 'Reset clears the language, hints, rules and user agent');
  }
  check((await page.textContent('#about')).includes('Krunk 0.1.0') && await page.locator('a[href="gpu.html"]').count() >= 1, 'About line and a link to krunk://gpu in Advanced');

  // Search across every tab, and deep links.
  await page.fill('#filter', 'cookies');
  await page.waitForTimeout(40);
  const shownPanels = await page.$$eval('.panel', (ps) => ps.filter((p) => getComputedStyle(p).display !== 'none').map((p) => p.dataset.tab));
  check(shownPanels.includes('privacy') && !shownPanels.includes('appearance'), 'searching "cookies" shows the Privacy tab\'s matches and hides tabs without any (' + shownPanels.join(',') + ')');
  check(await page.locator('.row.setting:visible').count() >= 2 && await page.locator('#defaultZoom:visible').count() === 0, 'only matching rows stay (' + await page.locator('.row.setting:visible').count() + ')');
  await page.fill('#filter', 'zzzzqqq');
  await page.waitForTimeout(40);
  check(await page.locator('#no-matches').isVisible(), 'no matches: an empty state');
  await page.fill('#filter', '');
  await page.waitForTimeout(40);
  check(await page.locator('#no-matches').isHidden() && await page.locator('.panel.active').count() === 1, 'clearing the search restores the tab');
  await page.evaluate(() => { location.hash = '#downloads'; });
  await page.waitForTimeout(40);
  check(await page.locator('.panel.active[data-tab=downloads]').count() === 1 && await page.locator('#tabs a.active[data-tab=downloads]').count() === 1, 'the #downloads deep link shows the Downloads tab');
  await page.evaluate(() => { location.hash = '#nonsense'; });
  await page.waitForTimeout(40);
  check(await page.locator('.panel.active[data-tab=general]').count() === 1, 'an unknown section falls back to General');
  // 0.5: import from other browsers (Passwords & import).
  await page.evaluate(() => { location.hash = '#passwords'; });
  await page.waitForTimeout(40);
  await page.waitForFunction(() => document.querySelectorAll('#import-profiles .row').length === 2);
  check((await page.textContent('#import-profiles .row:nth-child(1) .name')) === 'Firefox – default-release (default)', 'the Firefox profile is listed with its name');
  check(await page.locator('#import-profiles .row:nth-child(1) input[type=password]').count() === 1 && await page.locator('#import-profiles .row:nth-child(2) input[type=password]').count() === 0, 'only Firefox rows ask for a primary password');
  check(!(await page.isChecked('#import-profiles .row:nth-child(2) input[data-type=bookmarks]')) && await page.locator('#import-profiles .row:nth-child(2) label.off').count() === 1, 'a type the profile has no file for is unchecked and greyed');
  await page.fill('#import-profiles .row:nth-child(1) input[type=password]', 'fx-secret');
  await page.click('#import-profiles .row:nth-child(1) button');
  await page.waitForTimeout(60);
  const run = await lastSent(page, 'importRun');
  check(run && run.profile === 'firefox:abcd.default-release' && run.types.join(',') === 'history,bookmarks,passwords' && run.primaryPassword === 'fx-secret', 'Import asks the host for the chosen profile, types and primary password');
  check((await page.textContent('#import-result')) === 'history: 9 imported · bookmarks: 1 imported · passwords: 1 imported, 1 updated, 2 already there, 3 skipped (no website address: 1, unreadable entry: 2)', 'the counts are shown (' + await page.textContent('#import-result') + ')');
  check((await page.inputValue('#import-profiles .row:nth-child(1) input[type=password]')) === '', 'the primary password field is cleared afterwards');
  await page.click('#import-profiles .row:nth-child(2) button');
  await page.waitForTimeout(60);
  const run2 = await lastSent(page, 'importRun');
  check(run2 && run2.profile === 'chrome:Default' && run2.types.join(',') === 'history,passwords' && run2.primaryPassword === null, 'Chrome: no bookmarks (unchecked), no primary password');
  check((await page.textContent('#import-result')).includes('passwords: 0 imported (2 protected by Chrome, export a CSV for those)'), 'app-bound Chrome passwords are explained (' + await page.textContent('#import-result') + ')');
  await page.click('#import-csv');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'importCsv')) !== undefined && (await page.textContent('#import-result')) === 'passwords: 2 imported, 1 skipped', 'the CSV import asks the host (native file dialog) and shows its counts');
  await shot(page, 'settings-import.png');
  // 0.7.2 (R12): a 0 always says why.
  await page.evaluate(() => { window.__host.handlers.importRun = () => ({ passwords: { imported: 0, updated: 0, merged: 0, skipped: 0, skippedReasons: [], appBound: 0, why: 'Firefox has no saved logins in this profile' } }); });
  await page.click('#import-profiles .row:nth-child(1) button');
  await page.waitForTimeout(60);
  check((await page.textContent('#import-result')) === 'passwords: 0 imported (Firefox has no saved logins in this profile)', 'an empty login store is said, not a bare 0 (' + await page.textContent('#import-result') + ')');
  await page.evaluate(() => { window.__host.handlers.importRun = () => ({ passwords: { imported: 1, updated: 0, merged: 0, skipped: 2, skippedReasons: [{ reason: 'could not be decrypted: encrypted with a key that is not in key4.db', count: 2 }], appBound: 0 } }); });
  await page.click('#import-profiles .row:nth-child(1) button');
  await page.waitForTimeout(60);
  check((await page.textContent('#import-result')).includes('2 skipped (could not be decrypted: encrypted with a key that is not in key4.db: 2)'), 'the decryption reason is shown');
  const deep = await openPage(browser, 'https://krunk.invalid/settings.html#privacy', { width: 1000, height: 1200 }, 1, settingsMock, defaults);
  await deep.page.waitForFunction(() => document.getElementById('lists-summary').textContent !== '');
  check(await deep.page.locator('.panel.active[data-tab=privacy]').count() === 1, 'opened at #privacy: the Privacy tab shows');
  await shot(deep.page, 'settings-privacy-tab.png');
  await deep.page.close();
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// 0.7.2 (R9): long titles in a dropdown stay on one line with "…"; every row keeps its height.
// (R10): the bar's overflow is the bar's folder from `offset` on: a reorder in it counts from there.
async function testFolderPopupLong(browser) {
  console.log('popup: bookmark folder, long titles and the bar overflow');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 320, height: 140 }, 1);
  const long = 'An extremely long bookmark title that would wrap onto a second line in 0.7.1 and break the rows';
  await emit(page, { event: 'popup', content: { kind: 'bookmarkFolder', id: 1, title: 'Bookmarks bar', offset: 5, items: [
    { id: 30, title: long, url: 'https://long.example.org/' + 'x'.repeat(200), isFolder: false, favicon: null },
    { id: 31, title: 'Short', url: 'https://short.example/', isFolder: false, favicon: null },
    { id: 32, title: 'A folder with a long name that does not fit either, not at all', url: null, isFolder: true, favicon: null },
  ] } });
  await page.waitForSelector('.item');
  await page.waitForTimeout(60);
  const rows = await page.evaluate(() => Array.from(document.querySelectorAll('.item')).map((i) => {
    const l = i.querySelector('.label');
    return { h: i.offsetHeight, lh: l.offsetHeight, cut: l.scrollWidth > l.clientWidth, right: i.getBoundingClientRect().right };
  }));
  check(rows.every((r) => r.h === 30 && r.lh < 24 && r.right <= 320) && rows[0].cut && rows[2].cut && !rows[1].cut, 'long titles: one line, shortened, every row 30 px (' + JSON.stringify(rows) + ')');
  check((await lastSent(page, 'popupResize')) !== undefined, 'the dropdown asks for the height of its rows');
  await shot(page, 'popup-bookmark-folder-long.png');
  const fbox = async (id) => page.locator('.item[data-id="' + id + '"]').boundingBox();
  const s = await fbox(31), t = await fbox(30);
  await page.mouse.move(s.x + 20, s.y + s.height / 2);
  await page.mouse.down();
  await page.mouse.move(s.x + 20, s.y + s.height / 2 - 8, { steps: 3 });
  await page.mouse.move(t.x + 20, t.y + 3, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(30);
  const mv = await lastSent(page, 'moveBookmarkInFolder');
  check(mv && mv.id === 31 && mv.parent === 1 && mv.index === 5, 'a reorder in the overflow is counted from its first bar item (' + JSON.stringify(mv) + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// 0.7.2 (R10): the bookmarks bar shows the items that fit completely and "»" for the rest.
async function testBarOverflow(browser) {
  console.log('bookmarks bar: overflow');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 104 }, 1);
  const items = [];
  for (let i = 0; i < 16; i++) items.push(i === 5 ? { id: 100 + i, title: 'Folder ' + i, url: null, isFolder: true, favicon: null }
    : { id: 100 + i, title: 'Bookmark number ' + i, url: 'https://b' + i + '.example/', isFolder: false, favicon: null });
  await page.evaluate((its) => {
    const h = window.__host;
    h.state.tabs = [{ id: 1, title: 'Rust', url: 'https://www.rust-lang.org/', favicon: null, loading: false, private: false, sleeping: false }];
    h.state.active = 1;
    h.tabsEvent();
    h.navEvent({ bookmarksBar: true });
    h.emit({ event: 'bookmarksBar', items: its });
  }, items);
  await page.waitForSelector('#bookmarks-bar .bm');
  await page.waitForTimeout(50);
  const state = () => page.evaluate(() => {
    const bar = document.getElementById('bookmarks-bar');
    const br = bar.getBoundingClientRect();
    const more = document.getElementById('bm-more');
    const shown = Array.from(bar.querySelectorAll('.bm')).filter((n) => getComputedStyle(n).display !== 'none');
    const last = shown[shown.length - 1];
    return { shown: shown.length, hidden: bar.querySelectorAll('.bm.overflowed').length, more: !more.hidden,
      moreRight: Math.round(more.getBoundingClientRect().right), barRight: Math.round(br.right - 8),
      lastRight: last ? last.getBoundingClientRect().right : 0, moreLeft: more.getBoundingClientRect().left,
      prefix: shown.every((n, i) => n === bar.querySelectorAll('.bm')[i]) };
  });
  let st = await state();
  check(st.more && st.hidden > 0 && st.shown + st.hidden === 16 && st.prefix && st.lastRight <= st.moreLeft && Math.abs(st.moreRight - st.barRight) <= 1,
    'at 1100 px: the items that fit, then "»" at the right end (' + JSON.stringify(st) + ')');
  await page.click('#bm-more');
  let f = await lastSent(page, 'showBookmarkFolder');
  check(f && f.id === 1 && f.from === st.shown && f.rect && f.rect.x > 900, '"»" opens the bar\'s folder from the first hidden item (' + JSON.stringify(f) + ')');
  await page.click('#bm-more');
  check((await lastSent(page, 'hidePopup')) !== undefined, 'a second click closes it');
  await shot(page, 'shell-bar-overflow.png');
  // A narrower window: fewer items; a wide one: all of them and no "»".
  await page.setViewportSize({ width: 600, height: 104 });
  await page.waitForTimeout(80);
  const narrow = await state();
  check(narrow.shown < st.shown && narrow.more && narrow.lastRight <= narrow.moreLeft, 'the bar recalculates when the window gets narrower (' + narrow.shown + ' shown)');
  await page.setViewportSize({ width: 3000, height: 104 });
  await page.waitForTimeout(80);
  st = await state();
  check(st.shown === 16 && !st.more, 'all items fit: no "»"');
  await page.setViewportSize({ width: 1100, height: 104 });
  await page.waitForTimeout(80);
  // Fewer bookmarks (one removed): recalculated.
  await emit(page, { event: 'bookmarksBar', items: items.slice(0, 4) });
  await page.waitForTimeout(50);
  st = await state();
  check(st.shown === 4 && !st.more, 'after removing bookmarks everything fits again');
  await emit(page, { event: 'bookmarksBar', items: items });
  await page.waitForTimeout(50);
  st = await state();
  // Dragging a bookmark onto "»" moves it to the end of the bar (into the overflow).
  const from = await page.locator('.bm[data-id="100"]').boundingBox();
  const more = await page.locator('#bm-more').boundingBox();
  await page.mouse.move(from.x + 10, from.y + 10);
  await page.mouse.down();
  await page.mouse.move(from.x + 40, from.y + 10, { steps: 4 });
  await page.mouse.move(more.x + more.width / 2, more.y + more.height / 2, { steps: 8 });
  check(await page.locator('#bm-more.drop-into').count() === 1, '"»" lights up as a drop target');
  await page.mouse.up();
  await page.waitForTimeout(30);
  const mv = await lastSent(page, 'moveBookmark');
  check(mv && mv.id === 100 && mv.parent === 1 && mv.index >= 16, 'a drop on "»" moves the bookmark to the end of the bar (' + JSON.stringify(mv) + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// 0.11.0: many tabs. Tabs shrink to 72 px (the active one keeps 120 and its ×), then the row
// scrolls: the wheel and ‹ › scroll it, the edges fade, "▾" lists every tab; none of it shows
// while the tabs fit. A middle press starts no autoscroll and a middle click closes the tab
// (also when the row is scrolled); widths stay as they were until the pointer leaves.
async function testTabOverflow(browser) {
  console.log('shell: tab overflow (60 tabs)');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
  const setTabs = (n, active) => page.evaluate(([n, active]) => {
    const h = window.__host;
    h.state.tabs = Array.from({ length: n }, (_, i) => ({ id: i + 1, title: 'Tab number ' + (i + 1) + ' with a long title', url: 'https://site' + (i + 1) + '.example/', favicon: null, loading: false, private: false, sleeping: false }));
    h.state.nextId = n + 1;
    h.state.active = active;
    h.tabsEvent(); h.navEvent();
  }, [n, active]);
  await setTabs(4, 1);
  await page.waitForTimeout(250);
  check(await page.locator('#tabs-left').isHidden() && await page.locator('#tabs-right').isHidden() && await page.locator('#tablist').isHidden(),
    'four tabs fit: no arrows, no tab list button');
  await setTabs(60, 30);
  await page.waitForTimeout(1000);
  const m = await page.evaluate(() => {
    const box = document.getElementById('tabs');
    const tabs = [...box.querySelectorAll('.tab')];
    const w = tabs.map((t) => t.getBoundingClientRect().width);
    const act = box.querySelector('.tab.active').getBoundingClientRect(), row = box.getBoundingClientRect();
    return { n: tabs.length, min: Math.min(...w.filter((_, i) => i !== 29)), active: w[29], over: box.scrollWidth > box.clientWidth,
      inView: act.left >= row.left && act.right <= row.right, fades: [box.classList.contains('fade-left'), box.classList.contains('fade-right')],
      close: getComputedStyle(box.querySelector('.tab.active .close')).display };
  });
  check(m.n === 60 && m.over, '60 tabs overflow the row (' + JSON.stringify(m) + ')');
  check(Math.round(m.min) === 72 && m.active >= 119.5, 'tabs shrink to 72 px, the active one keeps 120 (' + Math.round(m.min) + ', ' + Math.round(m.active) + ')');
  check(m.close !== 'none', 'the active tab keeps its close button');
  check(m.inView, 'the active tab (30 of 60) is scrolled into view (' + JSON.stringify(await page.evaluate(() => { const b = document.getElementById('tabs'); const a = b.querySelector('.tab.active').getBoundingClientRect(); const r = b.getBoundingClientRect(); return { sl: b.scrollLeft, sw: b.scrollWidth, cw: b.clientWidth, al: a.left, ar: a.right, rl: r.left, rr: r.right }; })) + ')');
  check(m.fades[0] && m.fades[1], 'both edges fade while there are tabs on both sides');
  check(await page.locator('#tabs-left').isVisible() && await page.locator('#tabs-right').isVisible() && await page.locator('#tablist').isVisible(),
    'overflowing: ‹ › and ▾ show');
  await shot(page, 'shell-tab-overflow.png');
  const hm = await lastSent(page, 'hitMap');
  check(hm && hm.map.interactive.length >= 5, 'the hit map has the arrows and the list button (no window drag there)');
  // The wheel scrolls smoothly (several frames, not one jump).
  const s0 = await page.evaluate(() => document.getElementById('tabs').scrollLeft);
  await page.mouse.move(400, 18);
  await page.mouse.wheel(0, 300);
  await page.waitForTimeout(16);
  const s1 = await page.evaluate(() => document.getElementById('tabs').scrollLeft);
  await page.waitForTimeout(500);
  const s2 = await page.evaluate(() => document.getElementById('tabs').scrollLeft);
  check(s2 - s0 > 250 && s1 < s2, 'the wheel scrolls the row smoothly (' + [s0, s1, s2].map(Math.round).join(' → ') + ')');
  await page.click('#tabs-left');
  await page.waitForTimeout(500);
  const s3 = await page.evaluate(() => document.getElementById('tabs').scrollLeft);
  check(s3 < s2 - 200, '‹ scrolls most of a row back (' + Math.round(s2) + ' → ' + Math.round(s3) + ')');
  // A title change does not scroll back to the active tab.
  await page.evaluate(() => { const h = window.__host; h.state.tabs[29].title = 'Changed'; h.tabsEvent(); });
  await page.waitForTimeout(300);
  const s4 = await page.evaluate(() => document.getElementById('tabs').scrollLeft);
  check(Math.abs(s4 - s3) < 2, 'a title change leaves the scroll position alone');
  // Dragging a tab still reorders on the scrolled strip.
  {
    const ids = await page.evaluate(() => {
      const row = document.getElementById('tabs').getBoundingClientRect();
      const vis = [...document.querySelectorAll('#tabs .tab')].filter((n) => { const r = n.getBoundingClientRect(); return r.left > row.left + 40 && r.right < row.right - 40; });
      const a = vis[1].getBoundingClientRect(), b = vis[4].getBoundingClientRect();
      return { id: Number(vis[1].dataset.id), ax: a.left + a.width / 2, ay: a.top + a.height / 2, bx: b.right - 4 };
    });
    await page.mouse.move(ids.ax, ids.ay);
    await page.mouse.down();
    await page.mouse.move(ids.bx, ids.ay, { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(300);
    const mt = await lastSent(page, 'moveTab');
    check(mt && mt.id === ids.id, 'dragging a tab on the scrolled strip reorders it (' + JSON.stringify(mt) + ')');
  }
  // Middle press: no autoscroll; middle click closes the tab under the pointer.
  const target = await page.evaluate(() => {
    const row = document.getElementById('tabs').getBoundingClientRect();
    const t = [...document.querySelectorAll('#tabs .tab')].find((n) => { const r = n.getBoundingClientRect(); return r.left > row.left + 60 && r.right < row.right - 60; });
    const r = t.getBoundingClientRect();
    const ev = new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true });
    t.dispatchEvent(ev);
    return { id: Number(t.dataset.id), x: r.left + r.width / 2, y: r.top + r.height / 2, prevented: ev.defaultPrevented };
  });
  check(target.prevented, 'a middle press on the scrolled strip is prevented (no autoscroll)');
  const before = await page.evaluate(() => [...document.querySelectorAll('#tabs .tab:not(.closing)')].map((t) => t.getBoundingClientRect().width));
  await page.mouse.click(target.x, target.y, { button: 'middle' });
  await page.waitForTimeout(60);
  const ct = await lastSent(page, 'closeTab');
  check(ct && ct.id === target.id, 'middle click closes the overflowed tab under the pointer (' + JSON.stringify(ct) + ' vs ' + target.id + ')');
  await page.waitForTimeout(300);
  const after = await page.evaluate(() => [...document.querySelectorAll('#tabs .tab:not(.closing)')].map((t) => t.getBoundingClientRect().width));
  check(after.length === 59 && Math.abs(after[0] - before[0]) < 0.5, 'the other tabs keep their widths while the pointer stays');
  // ▾ asks the host for the tab list under the button.
  await page.click('#tablist');
  const sp = await lastSent(page, 'showPopup');
  const lb = await page.locator('#tablist').boundingBox();
  check(sp && sp.popup === 'tabList' && Math.abs(sp.rect.x - lb.x) < 1 && sp.rect.width > 0, '▾ opens the tab list at its button (' + JSON.stringify(sp) + ')');
  check(await page.locator('#tablist.on').count() === 1, '▾ is highlighted while its list is open');
  // Back to a few tabs: everything goes away again.
  await page.mouse.move(600, 60);
  await setTabs(3, 1);
  await page.waitForTimeout(300);
  check(await page.locator('#tablist').isHidden() && await page.evaluate(() => !document.getElementById('tabs').classList.contains('fade-left')), 'fitting again: no list button, no fade');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

async function testTabList(browser) {
  console.log('popup: tab list');
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 380, height: 532 }, 1);
  const tabs = Array.from({ length: 300 }, (_, i) => ({ id: i + 1, title: (i % 3 ? 'News story ' : 'Rust docs ') + (i + 1), url: 'https://s' + (i + 1) + '.example/', favicon: null, loading: false, private: i === 4, sleeping: false, audible: i === 2 }));
  await emit(page, { event: 'popup', seq: 9, content: { kind: 'tabList', tabs, active: 150 } });
  await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'popupShown' && m.seq === 9));
  await page.waitForTimeout(50);
  const r = await page.evaluate(() => ({ rows: document.querySelectorAll('.tl-row').length, focus: document.activeElement && document.activeElement.id,
    current: document.querySelector('.tl-row.current') && document.querySelector('.tl-row.current').textContent }));
  check(r.rows > 10 && r.rows < 40, '300 tabs: only the rows in view exist (' + r.rows + ')');
  check(r.focus === 'tl-search', 'the search field has the focus');
  check(r.current && r.current.startsWith('Rust docs 150') === false && r.current.includes('150'), 'the current tab is in view and marked (' + r.current + ')');
  await shot(page, 'popup-tab-list.png');
  await page.keyboard.type('rust');
  await page.waitForTimeout(50);
  const f = await page.$$eval('.tl-row .label', (l) => l.map((x) => x.textContent));
  check(f.length > 0 && f.every((t) => t.startsWith('Rust docs')), 'typing filters by title (' + f.slice(0, 3).join(' | ') + ')');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  const act = await lastSent(page, 'activateTab');
  check(act && act.id === 4, 'Down + Enter switches to the second match (' + JSON.stringify(act) + ')');
  // × closes without switching; the list follows the host's update in place, keeping the search.
  await emit(page, { event: 'popup', content: { kind: 'tabList', tabs, active: 150 } });
  await page.waitForTimeout(50);
  const n0 = (await sent(page)).filter((m) => m.cmd === 'activateTab').length;
  await page.locator('.tl-row', { hasText: 'Rust docs 7' }).first().locator('.tl-close').click();
  const cl = await lastSent(page, 'closeTab');
  check(cl && cl.id === 7 && (await sent(page)).filter((m) => m.cmd === 'activateTab').length === n0, '× closes that tab and switches to nothing');
  await emit(page, { event: 'popup', content: { kind: 'tabList', tabs: tabs.filter((t) => t.id !== 7), active: 150 } });
  await page.waitForTimeout(50);
  check(await page.inputValue('#tl-search') === 'rust' && (await page.locator('.tl-row', { hasText: 'Rust docs 7' }).count()) === 0, 'the update keeps the search; the closed tab is gone');
  await page.locator('.tl-row', { hasText: 'Rust docs 10' }).first().click({ button: 'middle' });
  check((await lastSent(page, 'closeTab')).id === 10, 'middle click closes from the list too');
  await page.fill('#tl-search', 'zzz');
  await page.waitForTimeout(30);
  check(await page.locator('.tl-empty').count() === 1, 'no match: "No tab matches."');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// 0.11.0: a page that stops responding: the prompt (Wait / Close page) and the crash page.
async function testHangAndCrash(browser) {
  console.log('popup: page not responding; crash page');
  let { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 380, height: 150 }, 1);
  await emit(page, { event: 'popup', seq: 5, content: { kind: 'unresponsive', tabId: 7, host: 'busy.example' } });
  await page.waitForSelector('#hang-close');
  await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'popupShown' && m.seq === 5));
  check((await page.textContent('h2')) === 'This page isn\'t responding' && (await page.textContent('.hang p')).includes('busy.example'), 'the prompt names the site');
  check(await page.evaluate(() => document.activeElement.id) === 'default' && (await page.textContent('#default')) === 'Wait', 'Wait is the default button');
  check(await page.evaluate(() => { const r = document.querySelector('.hang .row').getBoundingClientRect(); return r.bottom <= 150; }), 'it fits its 380×150 popup');
  await shot(page, 'popup-unresponsive.png');
  await page.click('#default');
  let r = await lastSent(page, 'hangReply');
  check(r && r.tabId === 7 && r.wait === true && (await lastSent(page, 'hidePopup')), 'Wait answers wait:true and closes the prompt');
  await emit(page, { event: 'popup', content: { kind: 'unresponsive', tabId: 7, host: 'busy.example' } });
  await page.waitForSelector('#hang-close');
  await page.click('#hang-close');
  r = await lastSent(page, 'hangReply');
  check(r && r.tabId === 7 && r.wait === false, 'Close page answers wait:false');
  // The page recovered: the host clears the prompt.
  await emit(page, { event: 'popupClear' });
  await page.waitForTimeout(30);
  const left = await page.evaluate(() => document.getElementById('root').innerHTML.slice(0, 80));
  check(left === '', 'the prompt goes when the page responds again (' + left + ')');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.context().close();

  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/crashed.html?url=' + encodeURIComponent('https://busy.example/app?x=1'), { width: 900, height: 500 }, 1));
  await page.waitForSelector('#reload');
  check((await page.textContent('#where')) === 'busy.example' && !(await page.isDisabled('#reload')), 'the crash page names the site and offers Reload');
  await shot(page, 'crashed.png');
  const nav = page.waitForRequest((req) => req.url().startsWith('https://busy.example/app'), { timeout: 3000 }).catch(() => null);
  await page.click('#reload');
  check(!!(await nav), 'Reload loads the page\'s address again');
  await page.context().close();
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/crashed.html?url=' + encodeURIComponent('javascript:alert(1)'), { width: 900, height: 500 }, 1));
  await page.waitForSelector('#reload');
  check(await page.isDisabled('#reload'), 'a non-web address cannot be reloaded from it');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
}

// 0.11.0: the source viewer (krunk://source).
async function testSourceViewer(browser) {
  console.log('source viewer');
  const html = '<!DOCTYPE html>\n<html><head><title>T &amp; U</title><style>body{color:red;margin:0}</style>' +
    '<script>function f(a){if(a){return "x";}else{return 1;}} // done\n</script></head><body><div class="a"><p>Hello <b>you</b></p>' +
    '<a href="/next?x=1&amp;y=2">next</a><pre>  keep\n    this</pre><!-- note --></div></body></html>';
  const data = { url: 'https://site.example/dir/page.html', from: 'cache', status: 200, mime: 'text/html', text: html };
  let { page, errors } = await openPage(browser, 'https://krunk.invalid/source.html?url=' + encodeURIComponent(data.url), { width: 1000, height: 600 }, 1, (d) => {
    window.__host.handlers.sourceGet = () => d;
  }, data);
  await page.waitForSelector('.ln');
  await page.waitForTimeout(100);
  check((await page.textContent('#src-url')) === data.url && (await page.textContent('#src-info')).includes('from the cache'), 'the address and where the source came from');
  check(await page.locator('.ln').count() === 4, 'the original has its own 4 lines (' + await page.locator('.ln').count() + ')');
  const colours = await page.evaluate(() => ['h-tag', 'h-attr', 'h-val', 'h-com', 'h-doc', 'h-ent', 'h-kw', 'h-str', 'h-prop'].filter((c) => !document.querySelector('.' + c)));
  check(colours.length === 0, 'tags, attributes, values, comments, doctype, entities, script keywords and strings, style properties are coloured (missing: ' + colours.join(', ') + ')');
  check(await page.evaluate(() => getComputedStyle(document.querySelector('.ln'), '::before').content) === '"1"', 'line numbers');
  const link = await page.getAttribute('.h-val a', 'href');
  check(link === 'source.html?url=' + encodeURIComponent('https://site.example/next?x=1&y=2'), 'a link opens its own source (' + link + ')');
  await shot(page, 'source-plain.png');
  // Beautify: one element per line, indented; <pre> as it is; the script formatted.
  await page.check('#opt-beautify');
  await page.waitForTimeout(100);
  const lines = await page.$$eval('.ln .t', (l) => l.map((x) => x.textContent));
  check(lines.length > 15, 'beautified: many lines (' + lines.length + ')');
  check(lines.includes('    <div class="a">') || lines.some((l) => /^ +<div class="a">$/.test(l)), 'nested tags are indented');
  check(lines.some((l) => /^ +<p>Hello <b>you<\/b><\/p>$/.test(l)) || lines.some((l) => l.trim() === '<p>') , 'inline tags stay in their text');
  check(lines.some((l) => l.includes('<pre>  keep')) && lines.includes('    this</pre>'), '<pre> keeps its spaces and lines');
  check(lines.some((l) => l.trim() === 'if(a){' || l.trim() === 'if (a) {' || l.trim() === 'if(a) {') && lines.some((l) => l.trim().startsWith('return "x";')), 'the script is formatted a statement per line (' + lines.filter((l) => /return|if/.test(l)).join(' | ') + ')');
  check(lines.some((l) => l.trim() === 'color:red;'), 'the style is formatted a declaration per line');
  await shot(page, 'source-beautified.png');
  // Wrap lines; both choices are remembered.
  await page.check('#opt-wrap');
  check(await page.evaluate(() => document.getElementById('src').classList.contains('wrap')), 'wrap lines');
  await page.reload();
  await page.waitForSelector('.ln');
  check(await page.isChecked('#opt-wrap') && await page.isChecked('#opt-beautify'), 'Beautify and Wrap lines are remembered');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.context().close();

  // The fallback: the tab's own document, said so; an HTTP error says its status.
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/source.html?url=x', { width: 900, height: 400 }, 1, () => {
    window.__host.handlers.sourceGet = () => ({ url: 'https://shop.example/checkout', from: 'page', status: null, mime: 'text/html', text: '<p>done</p>' });
  }));
  await page.waitForSelector('.ln');
  check(!(await page.isHidden('#src-note')) && (await page.textContent('#src-note')).includes('could not be loaded again'), 'the fallback says what is shown');
  await page.context().close();

  // 60,000 lines: shown at once without colouring everything; the end is reachable.
  const big = Array.from({ length: 60000 }, (_, i) => '<div class="row' + i + '"><span>line ' + i + '</span></div>').join('\n');
  ({ page, errors } = await openPage(browser, 'https://krunk.invalid/source.html?url=x', { width: 900, height: 500 }, 1, (b) => {
    try { localStorage.clear(); } catch (e) { /* none */ }
    window.__host.handlers.sourceGet = () => ({ url: 'https://big.example/', from: 'network', status: 200, mime: 'text/html', text: b });
  }, big));
  const t0 = Date.now();
  await page.waitForSelector('.chunk');
  await page.waitForTimeout(50);
  const ms = Date.now() - t0;
  const r = await page.evaluate(() => ({ chunks: document.querySelectorAll('.chunk').length, lit: document.querySelectorAll('.h-tag').length, info: document.getElementById('src-info').textContent }));
  check(r.chunks === 300 && r.lit < 20000 && ms < 4000, '60,000 lines: 300 chunks, colours only near the screen, shown in ' + ms + ' ms (' + JSON.stringify(r) + ')');
  await page.evaluate(() => { const b = document.getElementById('src'); b.scrollTop = b.scrollHeight; });
  await page.waitForTimeout(300);
  check(await page.evaluate(() => [...document.querySelectorAll('.chunk')].pop().querySelector('.h-tag') !== null), 'the last chunk is coloured when it comes into view');
  check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
  await page.context().close();
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
  check((await page.locator('.item', { hasText: 'BBC News' }).getAttribute('title')) === 'BBC News\nhttps://www.bbc.co.uk/news', 'a folder row\'s tooltip has the title and the address');
  await page.locator('.item', { hasText: 'Tech' }).click();
  const sf = await lastSent(page, 'showBookmarkFolder');
  check(sf && sf.id === 14 && sf.rect === undefined, 'subfolder opens in place (no rect)');
  await page.locator('.item', { hasText: 'BBC News' }).click();
  const ou = await lastSent(page, 'openUrl');
  check(ou && ou.url === 'https://www.bbc.co.uk/news', 'bookmark opens url');

  // 0.5: right-click on an item asks the host for the bar's menu at the click.
  await page.locator('.item', { hasText: 'Hacker News' }).click({ button: 'right' });
  const fm = await lastSent(page, 'showFolderItemMenu');
  check(fm && fm.id === 13 && fm.x > 0 && fm.y > 0, 'right-click on a folder item sends showFolderItemMenu{id:13, x, y}');
  // Drag Hacker News above BBC: a mark, then moveBookmarkInFolder{parent: the folder, index 0}.
  const fbox = async (text) => page.locator('.item', { hasText: text }).boundingBox();
  let hn = await fbox('Hacker News'), bbc = await fbox('BBC News');
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2);
  await page.mouse.down();
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2 - 8, { steps: 3 });
  await page.mouse.move(bbc.x + 20, bbc.y + 3, { steps: 4 });
  check(await page.locator('.item.drop-before', { hasText: 'BBC News' }).count() === 1, 'a mark shows where the item goes in the dropdown');
  await shot(page, 'popup-folder-drag.png');
  await page.mouse.up();
  let mf = await lastSent(page, 'moveBookmarkInFolder');
  check(mf && mf.id === 13 && mf.parent === 11 && mf.index === 0, 'dropped above BBC: moveBookmarkInFolder{id:13, parent:11, index:0} (' + JSON.stringify(mf) + ')');
  const openedBefore = (await sent(page)).filter((x) => x.cmd === 'openUrl').length;
  await page.waitForTimeout(20);
  check((await sent(page)).filter((x) => x.cmd === 'openUrl').length === openedBefore, 'the drop opens nothing');
  // Onto the middle of the Tech folder: into it.
  hn = await fbox('Hacker News');
  const tech = await fbox('Tech');
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2);
  await page.mouse.down();
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2 + 8, { steps: 3 });
  await page.mouse.move(tech.x + 20, tech.y + tech.height / 2, { steps: 4 });
  check(await page.locator('.item.drop-into', { hasText: 'Tech' }).count() === 1, 'over the subfolder: it lights up');
  await page.mouse.up();
  mf = await lastSent(page, 'moveBookmarkInFolder');
  check(mf && mf.id === 13 && mf.parent === 14, 'dropped on the subfolder: moveBookmarkInFolder{id:13, parent:14}');
  // Out of the popup (below its bottom edge): the host takes over.
  hn = await fbox('Hacker News');
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2);
  await page.mouse.down();
  await page.mouse.move(hn.x + 20, hn.y + hn.height / 2 + 8, { steps: 3 });
  await page.mouse.move(hn.x + 20, 130, { steps: 4 });
  const out = await lastSent(page, 'folderItemDragOut');
  check(out && out.id === 13, 'dragged out of the dropdown: folderItemDragOut{id:13}');
  check(await page.locator('.item.dragging, .item.drop-before, .item.drop-after, .item.drop-into').count() === 0, 'no marks left after the hand-over');
  await page.mouse.up();

  // 0.11.0: middle-click on a bookmark opens it behind the current tab and the dropdown stays
  // open; on a folder it does nothing (as on the bar); a middle press starts no autoscroll.
  const before = (await sent(page)).length;
  await page.locator('.item', { hasText: 'BBC News' }).click({ button: 'middle' });
  await page.locator('.item', { hasText: 'Tech' }).click({ button: 'middle' });
  await page.waitForTimeout(30);
  const after = (await sent(page)).slice(before);
  check(after.length === 1 && after[0].cmd === 'openBookmarkBackground' && after[0].id === 12,
    'middle-click: openBookmarkBackground{id:12} only, no hidePopup, nothing for the folder (' + JSON.stringify(after) + ')');
  check(await page.evaluate(() => { const ev = new MouseEvent('mousedown', { button: 1, bubbles: true, cancelable: true }); document.querySelector('.item').dispatchEvent(ev); return ev.defaultPrevented; }),
    'a middle press in the dropdown starts no autoscroll');

  // 0.11.0: content with a number is answered with popupShown after it is painted, with the
  // fitted height; the host opens the window then.
  await emit(page, { event: 'popup', seq: 41, content: { kind: 'bookmarkFolder', id: 14, title: 'Tech', back: 'News', items: [
    { id: 15, title: 'Rust', url: 'https://www.rust-lang.org/', isFolder: false, favicon: null },
    { id: 16, title: 'Deep', url: null, isFolder: true, favicon: null },
  ] } });
  await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'popupShown' && m.seq === 41));
  const shown = await lastSent(page, 'popupShown');
  const rootH = await page.evaluate(() => document.getElementById('root').offsetHeight + 2);
  check(shown.seq === 41 && shown.height === rootH, 'popupShown{seq:41, height} once painted (' + JSON.stringify(shown) + ')');
  check((await page.locator('.item', { hasText: 'BBC News' }).count()) === 0, 'nothing of the previous folder is left');
  // "‹ Back" row first; the focus starts on the first item, not on Back.
  const rows = await page.$$eval('.item', (l) => l.map((x) => x.textContent));
  check(rows[0] === '‹News' && rows[1] === 'Rust', 'a subfolder has "‹ News" as its first row (' + rows.join(' | ') + ')');
  check(await page.evaluate(() => document.activeElement && document.activeElement.textContent === 'Rust'), 'the focus is on the first item, not on Back');
  await shot(page, 'popup-folder-back.png');
  await page.click('#folder-back');
  check(!!(await lastSent(page, 'bookmarkFolderBack')), 'Back sends bookmarkFolderBack');
  const n0 = (await sent(page)).filter((x) => x.cmd === 'bookmarkFolderBack').length;
  await page.focus('.item:not(#folder-back)');
  await page.keyboard.press('Backspace');
  await page.keyboard.press('ArrowLeft');
  check((await sent(page)).filter((x) => x.cmd === 'bookmarkFolderBack').length === n0 + 2, 'Backspace and Left go back up too');
  await page.keyboard.press('ArrowDown');
  const downAt = await page.evaluate(() => document.activeElement.textContent);
  check(downAt === '▸Deep', 'Down moves to the next item (' + downAt + ')');
  await page.keyboard.press('ArrowRight');
  const deeper = await lastSent(page, 'showBookmarkFolder');
  check(deeper && deeper.id === 16, 'Right opens the focused subfolder');
  // A dropdown at the top level has no Back, and Backspace does nothing there.
  await emit(page, { event: 'popup', content: { kind: 'bookmarkFolder', id: 1, title: 'Bookmarks bar', offset: 4, items: [
    { id: 17, title: 'Docs', url: 'https://docs.rs/', isFolder: false, favicon: null }] } });
  await page.waitForTimeout(30);
  check(await page.locator('#folder-back').count() === 0, 'the top level has no Back row');
  const n1 = (await sent(page)).filter((x) => x.cmd === 'bookmarkFolderBack').length;
  await page.keyboard.press('Backspace');
  check((await sent(page)).filter((x) => x.cmd === 'bookmarkFolderBack').length === n1, 'Backspace at the top level sends nothing');
  check(!(await sent(page)).slice(-3).some((m) => m.cmd === 'popupShown' && m.seq == null), 'content without a number gets no popupShown');
  // 0.11.1: going back to a longer list in a smaller window: the measured height is the whole
  // list's (the box scrolls in the small window, but its content height is reported).
  await page.setViewportSize({ width: 360, height: 110 });
  const many = Array.from({ length: 25 }, (_, i) => ({ id: 200 + i, title: 'Bookmark ' + i, url: 'https://b' + i + '.example/', isFolder: false, favicon: null }));
  await emit(page, { event: 'popup', seq: 77, content: { kind: 'bookmarkFolder', id: 1, title: 'Bookmarks bar', offset: 3, items: many } });
  await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'popupShown' && m.seq === 77));
  const tall = await lastSent(page, 'popupShown');
  const rowH = await page.evaluate(() => document.querySelector('.item').getBoundingClientRect().height);
  check(tall.height >= 25 * rowH && rowH === 30, 'a long list reports its full height in a small window (' + tall.height + ' px, rows ' + rowH + ' px)');
  await page.setViewportSize({ width: 280, height: 110 });
  // Hidden: the content is dropped at once.
  await emit(page, { event: 'popupClear' });
  check(await page.evaluate(() => document.getElementById('root').children.length) === 0, 'popupClear empties the popup');
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

  // 0.5: focusing the password or the username field reports it with its rect; typing,
  // Esc and leaving the field report a blur; a fill from the browser lands in that form.
  await page.focus('input[name=p]');
  await page.waitForTimeout(30);
  let ff = await page.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').pop());
  check(ff && ff.field === 'password' && ff.rect && ff.rect.width > 10 && ff.rect.height > 5, 'focusing the password field posts fieldFocus{field:password, rect}');
  await page.focus('input[name=u]');
  await page.waitForTimeout(30);
  ff = await page.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').pop());
  const blurs = await page.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldBlur').length);
  check(ff && ff.field === 'username' && blurs >= 1, 'the username field of the form posts fieldFocus{field:username} after a blur of the password');
  await page.keyboard.type('a');
  await page.waitForTimeout(30);
  check((await page.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldBlur').length)) === blurs + 1, 'typing posts fieldBlur');
  check(!(await page.evaluate(() => window.__posted.some((m) => m.password !== undefined))), 'no password left the page without a submit');
  // Fill: a page whose stand-in bridge can deliver a message.
  const ctx2 = await browser.newContext();
  await ctx2.route('**/*', (r) => r.abort());
  await ctx2.route('https://login.example/**', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><body><form id="a"><input name="u" type="text"><input name="p" type="password"></form><form id="b"><input name="u2" type="email"><input name="p2" type="password"></form><iframe src="https://frame.example/login"></iframe></body>' }));
  await ctx2.route('https://frame.example/**', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><body><form><input name="fu" type="text"><input name="fp" type="password"></form></body>' }));
  await ctx2.addInitScript(() => {
    window.__posted = []; window.__listeners = [];
    window.__lbHost = { postMessage: (m) => window.__posted.push(m), addEventListener: (t, fn) => window.__listeners.push(fn) };
    window.__deliver = (d) => window.__listeners.forEach((fn) => fn({ data: d }));
  });
  await ctx2.addInitScript({ path: path.join(__dirname, '..', 'inject', 'login.js') });
  const p2 = await ctx2.newPage();
  await p2.goto('https://login.example/');
  await p2.waitForTimeout(100);
  await p2.focus('#b input[name=u2]');
  await p2.waitForTimeout(30);
  await p2.evaluate(() => window.__deliver({ lbFill: { username: 'carol', password: 'hunter2' } }));
  await p2.waitForTimeout(30);
  const filled = await p2.evaluate(() => ({ u: document.querySelector('#a input[name=u]').value, p: document.querySelector('#a input[name=p]').value, u2: document.querySelector('#b input[name=u2]').value, p2: document.querySelector('#b input[name=p2]').value }));
  check(filled.u2 === 'carol' && filled.p2 === 'hunter2' && filled.u === '' && filled.p === '', 'the fill lands in the focused form only (' + JSON.stringify(filled) + ')');
  // The iframe runs the script too and reports its own field (its own __posted).
  const frame = p2.frames().find((f) => f.url().startsWith('https://frame.example/'));
  await frame.focus('input[name=fp]');
  await p2.waitForTimeout(30);
  const framePosted = await frame.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').map((m) => m.field));
  check(framePosted.includes('password'), 'a login field in a cross-origin iframe reports fieldFocus from the frame');
  check(!(await frame.evaluate(() => window.__posted.some((m) => m.lb === 'loginForm'))), 'an iframe does not announce loginForm (top-level only)');
  // 0.6.0: a new-password field reports its details, and a generated fill lands in every
  // password field of the form (not the username).
  await p2.evaluate(() => {
    const f = document.createElement('form');
    f.id = 'signup';
    f.innerHTML = '<input name="su" type="text"><input name="np" type="password" autocomplete="new-password"><input name="np2" type="password" autocomplete="new-password"><button>Create account</button>';
    document.body.appendChild(f);
  });
  await p2.waitForTimeout(1100); // the quiet second after the previous fill
  await p2.focus('#signup input[name=np]');
  await p2.waitForTimeout(30);
  {
    const ff = await p2.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').pop());
    check(ff && ff.field === 'newPassword' && ff.autocomplete === 'new-password' && ff.passwords === 2 && ff.formText.includes('Create account'), 'a new-password field posts fieldFocus{newPassword, autocomplete, passwords, formText} (' + JSON.stringify(ff) + ')');
  }
  await p2.evaluate(() => window.__deliver({ lbFill: { generated: true, password: 'Gen3rated#Pass' } }));
  await p2.waitForTimeout(30);
  {
    const v = await p2.evaluate(() => ({ su: document.querySelector('#signup input[name=su]').value, np: document.querySelector('#signup input[name=np]').value, np2: document.querySelector('#signup input[name=np2]').value, p: document.querySelector('#a input[name=p]').value }));
    check(v.np === 'Gen3rated#Pass' && v.np2 === 'Gen3rated#Pass' && v.su === '' && v.p === '', 'a generated fill lands in both password fields of the form only (' + JSON.stringify(v) + ')');
  }
  await p2.waitForTimeout(1100);
  await p2.focus('#a input[name=p]');
  await p2.waitForTimeout(30);
  {
    const ff = await p2.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').pop());
    check(ff && ff.field === 'password' && ff.passwords === 1, 'a plain login password field is still "password" with its details (' + JSON.stringify(ff) + ')');
  }
  // 0.6.0 payment cards: a checkout form reports its card field, a card fill lands in the
  // cc-* fields (never the security code), and the submit posts the number without the code.
  await p2.evaluate(() => {
    const f = document.createElement('form');
    f.id = 'checkout';
    f.innerHTML = '<input name="cardholder" autocomplete="cc-name"><input name="cardnumber" autocomplete="cc-number" inputmode="numeric"><input name="exp" autocomplete="cc-exp" placeholder="MM/YY"><input name="cvc" autocomplete="cc-csc" maxlength="4"><button>Pay now</button>';
    document.body.appendChild(f);
    window.__posted.length = 0;
  });
  await p2.waitForTimeout(1100);
  await p2.focus('#checkout input[name=cvc]');
  await p2.waitForTimeout(30);
  check(!(await p2.evaluate(() => window.__posted.some((m) => m.lb === 'fieldFocus'))), 'the security-code field is not a card field');
  await p2.focus('#checkout input[name=cardnumber]');
  await p2.waitForTimeout(30);
  {
    const ff = await p2.evaluate(() => window.__posted.filter((m) => m.lb === 'fieldFocus').pop());
    check(ff && ff.field === 'card' && ff.rect && ff.rect.width > 10, 'focusing the card number posts fieldFocus{field:card, rect} (' + JSON.stringify(ff) + ')');
  }
  await p2.evaluate(() => window.__deliver({ lbFill: { card: { number: '4111111111111111', expMonth: 12, expYear: 2030, name: 'Alice Example' } } }));
  await p2.waitForTimeout(30);
  {
    const v = await p2.evaluate(() => ({ n: document.querySelector('#checkout input[name=cardnumber]').value, e: document.querySelector('#checkout input[name=exp]').value, h: document.querySelector('#checkout input[name=cardholder]').value, c: document.querySelector('#checkout input[name=cvc]').value, p: document.querySelector('#a input[name=p]').value }));
    check(v.n === '4111111111111111' && v.e === '12/30' && v.h === 'Alice Example' && v.c === '' && v.p === '', 'a card fill sets number, cc-exp as MM/YY and the name; the CVC and other forms stay empty (' + JSON.stringify(v) + ')');
  }
  await p2.waitForTimeout(1100);
  await p2.fill('#checkout input[name=cvc]', '123');
  await p2.evaluate(() => { document.getElementById('checkout').addEventListener('submit', (e) => e.preventDefault()); window.__posted.length = 0; });
  await p2.click('#checkout button');
  await p2.waitForTimeout(30);
  {
    const cs = await p2.evaluate(() => window.__posted.filter((m) => m.lb === 'cardSubmit'));
    check(cs.length === 1 && cs[0].number === '4111111111111111' && cs[0].expMonth === 12 && cs[0].expYear === 2030 && cs[0].name === 'Alice Example' && Object.keys(cs[0]).sort().join() === 'expMonth,expYear,lb,name,number', 'Pay posts one cardSubmit{number,expMonth,expYear,name} and nothing else (' + JSON.stringify(cs) + ')');
    check(!(await p2.evaluate(() => JSON.stringify(window.__posted).includes('123'))), 'the security code never leaves the page');
  }
  // A field found by name only (no autocomplete) counts; a short number does not.
  await p2.evaluate(() => { const f = document.createElement('form'); f.id = 'c2'; f.innerHTML = '<input name="ccnum" value="12345"><button>Buy</button>'; document.body.appendChild(f); f.addEventListener('submit', (e) => e.preventDefault()); window.__posted.length = 0; });
  await p2.click('#c2 button');
  await p2.waitForTimeout(30);
  check(!(await p2.evaluate(() => window.__posted.some((m) => m.lb === 'cardSubmit'))), 'a 5-digit value in a card-named field is not a card');
  await p2.waitForTimeout(3100);
  await p2.fill('#c2 input', '5555 5555 5555 4444');
  await p2.click('#c2 button');
  await p2.waitForTimeout(30);
  {
    const cs = await p2.evaluate(() => window.__posted.filter((m) => m.lb === 'cardSubmit').pop());
    check(cs && cs.number === '5555555555554444' && cs.expMonth === 0, 'a card-named field without autocomplete posts the digits only (' + JSON.stringify(cs) + ')');
  }
  await ctx2.close();
  check(errors.length === 0, 'login script raises no page errors (' + errors.join(' | ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await context.close();
}

// 0.6.0: the media script lists the page's videos for Picture-in-Picture and does the
// request on the krunk-pip event (here without a gesture, so the engine refuses it).
async function testMediaScript(browser) {
  console.log('injected media script');
  const context = await browser.newContext();
  const webm = fs.readFileSync(path.join(__dirname, '..', '..', 'scripts', 'vm-www', '8094', 'test-vp9-opus.webm'));
  await context.route('**/*', (r) => r.abort());
  await context.route('https://media.example/v.webm', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'video/webm' }, body: webm }));
  await context.route('https://media.example/', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><body><video id="a" src="/v.webm" muted autoplay loop></video><video id="b" src="/v.webm" muted title="Second clip"></video><video id="c" src="/v.webm" muted disablepictureinpicture></video><iframe id="f" srcdoc="&lt;video id=fv src=https://media.example/v.webm muted&gt;&lt;/video&gt;"></iframe></body>' }));
  await context.addInitScript(() => {
    window.__posted = [];
    window.__lbHost = { postMessage: (m) => window.__posted.push(m), addEventListener: () => {} };
    // 0.7.0 (R24): the script must not watch the page's DOM.
    window.__observers = [];
    const MO = window.MutationObserver;
    window.MutationObserver = function (cb) { window.__observers.push(1); return new MO(cb); };
  });
  await context.addInitScript({ path: path.join(__dirname, '..', 'inject', 'media.js') });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://media.example/');
  await page.waitForTimeout(1500);
  // (Counted before any waitForFunction: Playwright's own polling may create one.)
  const observers = await page.evaluate(() => window.__observers.length);
  await page.waitForFunction(() => { const m = window.__posted.filter((x) => x.lb === 'media').pop(); return m && m.videos.length >= 3 && m.videos.some((v) => v.playing); }, null, { timeout: 8000 }).catch(() => {});
  const media = await page.evaluate(() => window.__posted.filter((x) => x.lb === 'media').pop());
  check(media && media.videos.length === 3, 'media lists the three eligible videos (two of the page, one in a same-origin frame; disablepictureinpicture left out) (' + JSON.stringify(media) + ')');
  if (media && media.videos.length === 3) {
    const [a, b, f] = media.videos;
    check(a.playing && a.label === 'v.webm' && a.width > 0 && a.height > 0, 'the playing video: file name, size, playing (' + JSON.stringify(a) + ')');
    check(!b.playing && b.label === 'Second clip', 'the paused video: its title');
    check(f.index === 2 && !f.pip, 'the frame video gets the next index');
  }
  const r = await page.evaluate(() => new Promise((resolve) => {
    document.dispatchEvent(new CustomEvent('krunk-pip', { detail: { index: 1, done: (p) => p.then((v) => resolve({ ok: true, v }), (e) => resolve({ ok: false, e: String(e) })) } }));
  }));
  check(r && (r.ok || /NotAllowed|gesture|user activation/i.test(r.e)), 'the krunk-pip event asks the engine for PiP (refused without a gesture here) (' + JSON.stringify(r) + ')');
  const x = await page.evaluate(() => new Promise((resolve) => {
    document.dispatchEvent(new CustomEvent('krunk-pip', { detail: { exit: true, done: (p) => p.then(resolve, (e) => resolve({ e: String(e) })) } }));
  }));
  check(x && (x.exited === false || (r && r.ok)), 'exit with nothing in PiP is a no-op (' + JSON.stringify(x) + ')');
  const missing = await page.evaluate(() => new Promise((resolve) => {
    document.dispatchEvent(new CustomEvent('krunk-pip', { detail: { index: 9, done: (p) => p.then(resolve, (e) => resolve({ e: String(e) })) } }));
  }));
  check(missing && /no such video/.test(missing.e), 'an unknown index is refused');
  await page.evaluate(() => document.getElementById('a').pause());
  await page.waitForFunction(() => { const m = window.__posted.filter((x) => x.lb === 'media').pop(); return m && !m.videos[0].playing; }, null, { timeout: 3000 }).catch(() => {});
  check(await page.evaluate(() => !window.__posted.filter((x) => x.lb === 'media').pop().videos[0].playing), 'pausing reports the change');
  // Added later: a video, and a frame with a video (found through their own events).
  await page.evaluate(() => {
    const v = document.createElement('video'); v.src = '/v.webm'; v.muted = true; v.title = 'Late clip'; document.body.appendChild(v);
    const f = document.createElement('iframe'); f.srcdoc = '<video src="https://media.example/v.webm" muted title="Late frame clip"></video>'; document.body.appendChild(f);
  });
  await page.waitForFunction(() => { const m = window.__posted.filter((x) => x.lb === 'media').pop(); return m && m.videos.length >= 5; }, null, { timeout: 8000 }).catch(() => {});
  const late = await page.evaluate(() => window.__posted.filter((x) => x.lb === 'media').pop());
  check(late && late.videos.length === 5 && late.videos.some((v) => v.label === 'Late clip') && late.videos.some((v) => v.label === 'Late frame clip'), 'a video and a frame added later are found without watching the DOM (' + (late && late.videos.map((v) => v.label).join(', ')) + ')');
  check(observers === 0, 'no MutationObserver in the page (0.7.0, R24) (' + observers + ')');
  check(!(await page.evaluate(() => Object.keys(window).some((k) => /krunk|lbHost/i.test(k) && !['__lbHost', '__observers'].includes(k)))), 'no global added to the page');
  check(errors.length === 0, 'media script raises no page errors (' + errors.join(' | ') + ')');
  await context.close();
}

// 0.7.0 (R7): painted.js says once when the top document painted its first content.
// 0.7.0 (R12): nohints.js removes navigator.userAgentData only when the engine has no
// metadata (an empty object); with the engine's own it changes nothing.
async function testNoHintsScript(browser) {
  console.log('injected nohints script');
  const src = fs.readFileSync(path.join(__dirname, '..', 'inject', 'nohints.js'), 'utf8');
  for (const empty of [true, false]) {
    const context = await browser.newContext();
    await context.route('**/*', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: '<!doctype html><p>x</p>' }));
    if (empty) {
      // What the engine gives a page under a user-agent override without metadata.
      await context.addInitScript(() => {
        const d = Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgentData');
        const fake = { brands: [], mobile: false, platform: '' };
        Object.defineProperty(Navigator.prototype, 'userAgentData', { get: () => fake, configurable: true, enumerable: d ? d.enumerable : true });
      });
    }
    // Evaluated at the page's top level, as the renderer does: it must declare nothing there.
    await context.addInitScript({ content: src });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(String(e)));
    await page.goto('https://hints.example/');
    check(await page.evaluate(() => typeof d === 'undefined' && typeof v === 'undefined'), 'nohints leaves no globals (a page may declare its own d)');
    const r = await page.evaluate(() => ({ has: 'userAgentData' in navigator, type: typeof navigator.userAgentData, brands: navigator.userAgentData ? navigator.userAgentData.brands.length : -1 }));
    if (empty) check(!r.has && r.type === 'undefined', 'no metadata: navigator.userAgentData is gone (' + JSON.stringify(r) + ')');
    else check(r.has && r.brands > 0, 'the engine\'s metadata: navigator.userAgentData stays (' + JSON.stringify(r) + ')');
    check(errors.length === 0, 'nohints raises no errors (' + errors.join(' | ') + ')');
    await context.close();
  }
}

// 0.7.0 (R16): every link on Krunk's pages is styled (never the browser's blue) and reads at
// WCAG AA on what is behind it, light and dark, with the default and a custom accent.
// 0.7.0 (R17): on the permissions page a group's description and table start where its heading does.
async function testPermissionsPadding(browser) {
  console.log('permissions padding (R17)');
  const { page, context } = await openPage(browser, 'https://krunk.invalid/settings.html#permissions', { width: 1100, height: 800 }, 1);
  await page.waitForSelector('.panel.active[data-tab=permissions]');
  const r = await page.evaluate(() => [...document.querySelectorAll('.panel[data-tab=permissions] .card.group')].map((g) => {
    const h = g.querySelector('h3').getBoundingClientRect().left;
    return [...g.querySelectorAll(':scope > .desc, :scope > table')].map((n) => {
      const x = n.tagName === 'TABLE' ? n.querySelector('th').getBoundingClientRect().left : n.getBoundingClientRect().left;
      return Math.round(x - h);
    });
  }).flat());
  check(r.length >= 3 && r.every((d) => Math.abs(d) <= 1), 'descriptions and the sites table line up with the headings (offsets ' + r.join(',') + ')');
  await shot(page, 'settings-permissions.png');
  await context.close();
}

// 0.7.0 (R20): webstore.js on a store detail page: "Add to Chrome" becomes "Add to Krunk"; the
// click goes to Krunk (without the id) and not to the store; elsewhere on the store nothing changes.
async function testWebStoreScript(browser) {
  console.log('injected webstore script (R20)');
  const src = fs.readFileSync(path.join(__dirname, '..', 'inject', 'webstore.js'), 'utf8');
  const context = await browser.newContext();
  await context.route('**/*', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><body><h1>Dark Reader</h1><button id="b" disabled aria-disabled="true"><span class="x"><span>Add to Chrome</span></span></button><p id="clicked">no</p>' +
      '<script>document.getElementById("b").addEventListener("click", () => { document.getElementById("clicked").textContent = "store"; });' +
      'setTimeout(() => { const c = document.createElement("button"); c.id = "late"; c.textContent = "Add to Chrome"; document.body.appendChild(c); }, 300);</script>' }));
  await context.addInitScript(() => {
    window.__posted = [];
    window.__listeners = [];
    window.__lbHostForTest = { postMessage: (m) => window.__posted.push(m), addEventListener: (t, f) => window.__listeners.push(f) };
  });
  await context.addInitScript({ content: '(function (__lbHost) {\n' + src + '\n})(window.__lbHostForTest);' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://chromewebstore.google.com/detail/dark-reader/eimadpbcbfnmbkopoojfekhnkhdbieeh');
  await page.waitForTimeout(700);
  check((await page.textContent('#b')) === 'Add to Krunk' && !(await page.isDisabled('#b')) && (await page.textContent('#late')) === 'Add to Krunk', 'the store button (and one added later) says Add to Krunk and is enabled');
  await page.click('#b');
  const posted = await page.evaluate(() => window.__posted);
  check(posted.length === 1 && JSON.stringify(posted[0]) === '{"lb":"extStoreClick"}' && (await page.textContent('#clicked')) === 'no', 'the click goes to Krunk only, with no id in it');
  await page.evaluate(() => window.__listeners.forEach((f) => f({ data: { event: 'krunkStore', text: 'Dark Reader was added to Krunk.' } })));
  check((await page.textContent('#krunk-store-notice')) === 'Dark Reader was added to Krunk.', 'Krunk\'s answer shows on the page');
  await page.goto('https://chromewebstore.google.com/category/extensions');
  await page.waitForTimeout(500);
  check((await page.textContent('#b')) === 'Add to Chrome', 'not a detail page: nothing changes');
  check(errors.length === 0, 'webstore script raises no errors (' + errors.join(' | ') + ')');
  await context.close();
}

async function testLinkSweep(browser) {
  console.log('link sweep (R16)');
  const pages = ['about', 'adblock', 'bookmarks', 'downloads', 'extensions', 'gpu', 'history', 'https-only', 'newtab', 'passwords', 'settings', 'sitedata', 'storage', 'tasks'];
  const measure = () => {
    const parse = (c) => { const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?/.exec(c); return m ? [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]] : null; };
    const lum = (c) => { const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]); };
    const bgOf = (n) => { for (; n; n = n.parentElement) { const b = parse(getComputedStyle(n).backgroundColor); if (b && b[3] > 0.9) return b; } return parse(getComputedStyle(document.body).backgroundColor) || [255, 255, 255, 1]; };
    const out = [];
    document.querySelectorAll('a[href]').forEach((a) => {
      const r = a.getBoundingClientRect();
      if (!r.width || !r.height || getComputedStyle(a).visibility === 'hidden') return;
      const fg = parse(getComputedStyle(a).color), bg = bgOf(a);
      const x = lum(fg), y = lum(bg);
      out.push({ text: a.textContent.trim().slice(0, 30), color: getComputedStyle(a).color, ratio: Math.round((Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05) * 100) / 100 });
    });
    return out;
  };
  for (const theme of ['light', 'dark']) {
    for (const accent of ['', '#f59e0b']) {
      const bad = [];
      let count = 0;
      for (const p of pages) {
        const { page, context } = await openPage(browser, 'https://krunk.invalid/' + p + '.html', { width: 1200, height: 900 }, 1);
        await page.waitForTimeout(250);
        // Settings: every section at once, so its links count too.
        await page.evaluate(() => document.querySelectorAll('.panel[data-tab]').forEach((n) => { n.classList.add('active'); n.hidden = false; n.style.display = 'block'; }));
        await page.evaluate(([t, a]) => LB.applyTheme(t, a, null), [theme, accent]);
        await page.waitForTimeout(80);
        for (const l of await page.evaluate(measure)) {
          count++;
          if (l.color === 'rgb(0, 0, 238)' || l.color === 'rgb(85, 26, 139)' || l.ratio < 4.5) bad.push(p + ': "' + l.text + '" ' + l.color + ' ' + l.ratio);
        }
        if (theme === 'dark' && !accent && (p === 'settings' || p === 'about')) await shot(page, 'links-' + p + '-dark.png');
        await context.close();
      }
      check(bad.length === 0 && count > 5, theme + (accent ? ', amber accent' : '') + ': ' + count + ' links, all styled and AA (' + bad.slice(0, 6).join(' | ') + ')');
    }
  }
}

async function testPaintedScript(browser) {
  console.log('injected painted script');
  const context = await browser.newContext();
  await context.route('**/*', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' },
    body: '<!doctype html><body style="background:#111;color:#eee"><h1>Painted</h1><iframe srcdoc="<p>frame</p>"></iframe></body>' }));
  await context.addInitScript(() => {
    window.__posted = [];
    const host = { postMessage: (m) => window.__posted.push(m), addEventListener: () => {} };
    window.__lbHostForTest = host;
  });
  const src = fs.readFileSync(path.join(__dirname, '..', 'inject', 'painted.js'), 'utf8');
  await context.addInitScript({ content: '(function (__lbHost) {\n' + src + '\n})(window.__lbHostForTest);' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://paint.example/');
  await page.waitForTimeout(800);
  const top = await page.evaluate(() => window.__posted.filter((m) => m.lb === 'painted').length);
  const frame = await page.frames()[1].evaluate(() => (window.__posted || []).length);
  check(top === 1 && frame === 0, 'painted: once from the top document, never from a frame (top ' + top + ', frame ' + frame + ')');
  // Krunk's own pages announce themselves with `ready` (a `painted` there is a malformed request).
  await page.goto('https://krunk.invalid/newtab.html');
  await page.waitForTimeout(800);
  const own = await page.evaluate(() => window.__posted.length);
  check(own === 0, 'painted: nothing from Krunk\'s own pages (' + own + ')');
  check(errors.length === 0, 'painted script raises no page errors (' + errors.join(' | ') + ')');
  await context.close();
}

// 0.6.0: krunk://extensions (the list, the scope and API survey, toggles, remove, the three
// install paths) and the shell's extension buttons.
async function testExtensions(browser) {
  console.log('extensions page');
  const id1 = 'a'.repeat(32), id2 = 'b'.repeat(32);
  const exts = [
    { id: id1, name: 'Krunk fixture', version: '1.0.0', description: 'Test extension', enabled: true, allowedInPrivate: false, source: 'file:krunk-fixture.zip', installedAt: 1, updatedAt: 1,
      scope: [{ pattern: 'http://localhost/*', text: 'localhost', allSites: false }, { pattern: '<all_urls>', text: 'all websites', allSites: true }],
      apis: [{ api: 'runtime', supported: true, reason: '' }, { api: 'storage', supported: true, reason: '' }, { api: 'webRequest', supported: false, reason: 'not provided: blocking network interception (use declarativeNetRequest static rules)' }],
      runtimeUnsupported: ['notifications'], dnrTranslated: 1, dnrSkipped: 1, hasOptions: true, hasPopup: true, hasBackground: true, contentScripts: 1, icon: 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', permissions: ['storage', 'tabs'] },
    { id: id2, name: 'Quiet one', version: '2.1', description: '', enabled: false, allowedInPrivate: true, source: 'webstore:' + id2, installedAt: 2, updatedAt: 2, scope: [], apis: [], runtimeUnsupported: [], dnrTranslated: 0, dnrSkipped: 0, hasOptions: false, hasPopup: false, hasBackground: false, contentScripts: 0, icon: null, permissions: [] },
  ];
  const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/extensions.html', { width: 1000, height: 700 }, 1,
    (list) => {
      const h = window.__host.handlers;
      window.__exts = list;
      h.extList = () => window.__exts;
      h.extSet = (m) => { const x = window.__exts.find((e) => e.id === m.id); if (m.enabled !== undefined) x.enabled = m.enabled; if (m.private !== undefined) x.allowedInPrivate = m.private; return window.__exts; };
      h.extRemove = (m) => { window.__exts = window.__exts.filter((e) => e.id !== m.id); return window.__exts; };
      h.extInstallFile = () => ({ installed: 'c'.repeat(32), name: 'From file', extensions: window.__exts.concat([Object.assign({}, window.__exts[0], { id: 'c'.repeat(32), name: 'From file', enabled: true, source: 'file:x.zip' })]) });
      h.extInstallFolder = () => ({ declined: true });
      h.extInstallUrl = (m) => { if (!/chromewebstore/.test(m.url)) throw new Error('That is not a Chrome Web Store extension address (…/detail/<name>/<id>).'); return { installed: 'd'.repeat(32), name: 'From the store', extensions: window.__exts }; };
    }, exts);
  await page.waitForSelector('.ext');
  check((await page.locator('.ext').count()) === 2, 'lists the 2 extensions');
  const first = page.locator('.ext[data-id="' + id1 + '"]');
  // 0.7.0 (R22): cards: what it reaches up front, the rest under Details.
  check((await first.locator('.reach').textContent()) === 'Can read and change all websites' && await first.locator('.reach.all').count() === 1, 'all websites is said up front, marked');
  check((await first.locator('.partial-note').textContent()).includes('not in Krunk (2)'), 'what Krunk does not provide is pointed out on the card');
  await first.locator('.more summary').click();
  const dl = await first.locator('.more dl').textContent();
  check(dl.includes('chrome.runtime, chrome.storage') && dl.includes('chrome.webRequest — blocking network interception') && dl.includes('chrome.notifications — reached for at runtime') && dl.includes('1 content script') && dl.includes('1 network rule (1 not translated)') && dl.includes('localhost'), 'Details: uses, not in Krunk with the reason, parts, rules, sites (' + dl.slice(0, 160) + ')');
  check((await first.locator('img').getAttribute('src')).startsWith('data:image/png') && await page.locator('.ext[data-id="' + id2 + '"] .icon .puzzle').count() === 1, 'the icon is shown when there is one, a placeholder otherwise');
  check(await page.locator('.ext[data-id="' + id2 + '"]').evaluate((e) => e.classList.contains('disabled')) && (await page.textContent('.ext[data-id="' + id2 + '"] .reach')) === 'Does not read or change web pages', 'a disabled extension is dimmed; one without sites says so');
  await shot(page, 'extensions.png');
  await page.evaluate(() => LB.applyTheme('dark', '', null));
  await shot(page, 'extensions-dark.png');
  await page.evaluate(() => LB.applyTheme('light', '', null));
  await first.locator('.switch').click();
  await page.waitForTimeout(60);
  const st = await lastSent(page, 'extSet');
  check(st && st.id === id1 && st.enabled === false, 'the Enabled toggle sends extSet{enabled:false}');
  await first.locator('input[data-key="private"]').click();
  await page.waitForTimeout(60);
  check((await lastSent(page, 'extSet')).private === true, 'the private toggle sends extSet{private:true}');
  await first.locator('button', { hasText: 'Options' }).click();
  check((await lastSent(page, 'extOpenOptions')).id === id1, 'Options sends extOpenOptions');
  await page.locator('.ext[data-id="' + id2 + '"] button', { hasText: 'Remove' }).click();
  await page.waitForSelector('#lb-dialog-ok');
  await page.click('#lb-dialog-ok');
  await page.waitForTimeout(60);
  check((await lastSent(page, 'extRemove')).id === id2 && (await page.locator('.ext').count()) === 1, 'Remove asks, sends extRemove and the list shrinks');
  await page.click('#install-file');
  await page.waitForTimeout(80);
  check((await lastSent(page, 'extInstallFile')) !== undefined && (await page.locator('.ext').count()) === 2 && (await page.locator('.toast').textContent()).includes('Installed From file'), 'Install from file: the reply refreshes the list and toasts');
  await page.click('#install-folder');
  await page.waitForTimeout(80);
  check((await page.locator('.toast').last().textContent()).includes('Not installed'), 'a declined install says Not installed');
  await page.fill('#url', 'https://example.com/not-a-store');
  await page.click('#install-url');
  await page.waitForTimeout(80);
  check((await page.textContent('#error')).includes('not a Chrome Web Store'), 'a non-store address is refused with the reason');
  await page.fill('#url', 'https://chromewebstore.google.com/detail/thing/' + 'd'.repeat(32));
  await page.click('#install-url');
  await page.waitForTimeout(80);
  check((await lastSent(page, 'extInstallUrl')).url.includes('chromewebstore') && await page.inputValue('#url') === '', 'a store address is sent and the field cleared');
  await emit(page, { event: 'extChanged', extensions: [] });
  await page.waitForTimeout(40);
  check(await page.locator('#empty').isVisible() && (await page.textContent('#empty h2')) === 'No extensions yet' && (await page.textContent('#empty')).includes('Add to Krunk'), 'extChanged with nothing installed shows the empty state, which says how to add one');
  await shot(page, 'extensions-empty.png');
  check(errors.length === 0, 'no console errors (' + errors.join('; ') + ')');
  check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
  await page.context().close();

  // The shell's extension buttons.
  const shell = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1000, height: 76 }, 1);
  await emit(shell.page, { event: 'extActions', actions: [{ id: id1, title: 'Krunk fixture', icon: null, badge: '3', badgeColor: 'rgb(0,128,0)', hasPopup: true }, { id: id2, title: 'Quiet', icon: null, badge: '', badgeColor: '', hasPopup: false }] });
  await shell.page.waitForSelector('.ext-action');
  check((await shell.page.locator('.ext-action').count()) === 2 && (await shell.page.textContent('.ext-action[data-ext="' + id1 + '"] .ext-badge')) === '3', 'the shell shows a button per action with its badge');
  await shell.page.click('.ext-action[data-ext="' + id1 + '"]');
  const act = await lastSent(shell.page, 'extAction');
  check(act && act.id === id1 && act.rect.width > 10 && act.rect.y >= 0, 'a click sends extAction{id, rect}');
  await emit(shell.page, { event: 'extActions', actions: [] });
  await shell.page.waitForTimeout(40);
  check((await shell.page.locator('.ext-action').count()) === 0, 'no actions: no buttons');
  check(shell.errors.length === 0, 'shell: no console errors (' + shell.errors.join('; ') + ')');
  await shell.page.context().close();
}

// 0.6.0: the content-script shim as the bundle installs it in a page's isolated world (here
// the page's own world, with a stand-in for the DevTools binding): matches, run_at, the
// chrome.* subset, messaging and storage through the binding, unsupported APIs reported.
async function testExtShim(browser) {
  console.log('extension content shim');
  const context = await browser.newContext();
  await context.route('**/*', (r) => r.abort());
  await context.route('https://site.example/**', (r) => r.fulfill({ status: 200, headers: { 'content-type': 'text/html' }, body: '<!doctype html><title>Site</title><body><h1>page</h1></body>' }));
  const shim = fs.readFileSync(path.join(__dirname, '..', 'inject', 'ext-content.js'), 'utf8');
  const id = 'e'.repeat(32);
  await context.addInitScript((bid) => {
    window.__calls = [];
    // 0.11.7: the extension's own binding, __krunkExt_<id>; the old shared name stays unused.
    window['__krunkExt_' + bid] = (json) => { window.__calls.push(JSON.parse(json)); };
    window.__krunkExt = () => { window.__shared = true; };
  }, id);
  await context.addInitScript({ content: '(function(){ var __krunkShim = ' + shim.trim() + ';\n' +
    'var ext = __krunkShim(' + JSON.stringify(id) + ', {name: "Fixture", version: "1"}, {greet: {message: "Hi $who$", placeholders: {who: {content: "$1"}}}});\n' +
    'ext.run(["https://site.example/*"], [], false, "document_start", ["cs.css"], function (chrome) { window.__ran = (window.__ran||0)+1; window.__id = chrome.runtime.id; window.__url = chrome.runtime.getURL("a/b.png"); window.__msg = chrome.i18n.getMessage("greet", ["Bob"]); window.__manifest = chrome.runtime.getManifest().name; chrome.storage.local.get("k", function (v) { window.__got = v; }); chrome.runtime.sendMessage({ping: 1}).then(function (r) { window.__reply = r; }); chrome.runtime.onMessage.addListener(function (m, s, send) { window.__in = m; send({pong: m.x}); }); void chrome.notifications; });\n' +
    'ext.run(["https://other.example/*"], [], false, "document_start", [], function () { window.__wrong = true; });\n' +
    'ext.run(["<all_urls>"], ["https://site.example/*"], false, "document_start", [], function () { window.__excluded = true; });\n' +
    '})();' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto('https://site.example/');
  await page.waitForTimeout(100);
  const v = await page.evaluate(() => ({ ran: window.__ran, wrong: window.__wrong, excluded: window.__excluded, id: window.__id, url: window.__url, msg: window.__msg, manifest: window.__manifest, calls: window.__calls, css: !!document.querySelector('link[href$="/cs.css"]') }));
  check(v.ran === 1 && !v.wrong && !v.excluded, 'only the script whose matches fit runs (' + JSON.stringify([v.ran, v.wrong, v.excluded]) + ')');
  check(v.id === id && v.url === 'https://' + id + '.ext.krunk.invalid/a/b.png' && v.msg === 'Hi Bob' && v.manifest === 'Fixture' && v.css, 'runtime.id/getURL/getManifest, i18n placeholders, the CSS link');
  const get = v.calls.find((c) => c.cmd === 'storage.get'), send = v.calls.find((c) => c.cmd === 'runtime.sendMessage'), unsupported = v.calls.find((c) => c.cmd === 'unsupported');
  check(get && get.ext === id && get.data.area === 'local' && get.data.keys === 'k' && typeof get.id === 'number', 'storage.get goes through the binding with the area and keys');
  check(send && send.data.message.ping === 1, 'runtime.sendMessage goes through the binding');
  check(!(await page.evaluate(() => window.__shared)), 'the shim calls its own extension\'s binding, not a shared one (0.11.7)');
  check(unsupported && unsupported.data.api === 'notifications', 'an API Krunk does not provide is reported once');
  await page.evaluate(([gid, sid, xid]) => {
    window.__krunkExtDeliver(JSON.stringify({ ext: xid, reply: gid, data: { k: 42 } }));
    window.__krunkExtDeliver(JSON.stringify({ ext: xid, reply: sid, data: { ok: true } }));
    window.__krunkExtDeliver(JSON.stringify({ ext: xid, event: 'message', id: 77, data: { x: 5 }, from: 'bg' }));
  }, [get.id, send.id, id]);
  await page.waitForTimeout(50);
  const after = await page.evaluate(() => ({ got: window.__got, reply: window.__reply, inMsg: window.__in, respond: window.__calls.find((c) => c.cmd === 'respond') }));
  check(after.got && after.got.k === 42 && after.reply && after.reply.ok === true, 'replies resolve the callback and the promise');
  check(after.inMsg && after.inMsg.x === 5 && after.respond && after.respond.data.id === 77 && after.respond.data.data.pong === 5, 'a message from the extension reaches onMessage and sendResponse goes back as respond');
  check(!(await page.evaluate(() => Object.keys(window).some((k) => /^chrome$|^browser$/.test(k)))), 'the page world has no chrome/browser global from the shim');
  check(errors.length === 0, 'shim raises no page errors (' + errors.join(' | ') + ')');
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
  // 0.7.0 (R24): the event position getters stay the engine's own until a lock is granted.
  check(await page.evaluate(() => ['clientX', 'movementX', 'screenY'].every((p) => Object.getOwnPropertyDescriptor(MouseEvent.prototype, p).get.name === 'get ' + p)), 'MouseEvent getters untouched before the first lock');
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
  // 0.11.1: and where a window's first tab starts (the strip's left padding), for the grab point
  // in the new window (it used to come from the hit map, whose second rect is ‹ on an overflowing row).
  const firstLeft = await page.$eval('.tab', (n) => n.getBoundingClientRect().left);
  check(out && typeof out.firstX === 'number' && Math.abs(out.firstX - firstLeft) < 1, 'tabDragOut.firstX is where the first tab starts (' + (out && out.firstX) + ' vs ' + firstLeft + ')');
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
  // The dragged tab follows the cursor; the others slide left by its width (no drop marks).
  const live = await page.$$eval('.tab', (ns) => ns.map((n) => ({ id: +n.dataset.id, t: n.style.transform, dragging: n.classList.contains('dragging') })));
  const dragged = live.find((t) => t.dragging);
  const slid = live.filter((t) => !t.dragging && /translateX\(-/.test(t.t));
  check(dragged && /translateX\((\d+)/.test(dragged.t) && parseFloat(dragged.t.match(/\(([-\d.]+)/)[1]) > 100, 'the dragged tab follows the cursor (transform ' + (dragged && dragged.t) + ')');
  check(slid.length === ids.length - 1 && await page.locator('.tab.drop-after, .tab.drop-before').count() === 0, 'the other tabs slide left to make room (' + slid.length + ' of ' + (ids.length - 1) + '), no drop marks');
  await shot(page, 'shell-tab-reorder-live.png');
  await page.mouse.up();
  const mv = await lastSent(page, 'moveTab');
  check(mv && mv.id === ids[0] && mv.index === ids.length - 1, 'horizontal drag reorders (got ' + JSON.stringify(mv) + ')');
  await page.waitForFunction(() => !document.querySelector('.tab.settling'), null, { timeout: 2000 });
  await page.waitForTimeout(60);
  const hmAfter = (await lastSent(page, 'hitMap')).map;
  check(hmAfter.tabCenters.length === ids.length && hmAfter.tabCenters.every((c, i) => i === 0 || c > hmAfter.tabCenters[i - 1]), 'the hit map is reported after the drop settles');

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

  // A tab from another window hovering this strip (0.11.1): a gap opens where it would land,
  // with the tab in it (a ghost); the gap follows the drop place; leaving closes it.
  const g = { id: 9, title: 'Dragged here', favicon: null, private: false };
  const order = () => page.$$eval('#tabs > .tab:not(.closing)', (ns) => ns.map((n) => (n.classList.contains('ghost') ? 'G' : n.dataset.id)).join(','));
  const maps = async () => (await sent(page)).filter((m) => m.cmd === 'hitMap').length;
  const mapsBefore = await maps();
  const now = (await order()).split(','); // after the reorder above
  await emit(page, { event: 'tabDropHint', index: 1, ghost: g });
  await page.waitForTimeout(350);
  check(await order() === [now[0], 'G', now[1], now[2], now[3]].join(','), 'tabDropHint{index:1, ghost} opens a gap before the second tab (' + await order() + ')');
  const gb = await page.locator('.tab.ghost').boundingBox();
  check(gb && gb.width > 60 && (await page.locator('.tab.ghost .title').textContent()) === 'Dragged here', 'the ghost shows the dragged tab at tab width (' + JSON.stringify(gb) + ')');
  check(await page.locator('.tab.drop-before, .tab.drop-after').count() === 0, 'no 2-px marks any more');
  await shot(page, 'shell-drop-ghost.png');
  await emit(page, { event: 'tabDropHint', index: 3, ghost: g });
  await page.waitForTimeout(350);
  check(await order() === [now[0], now[1], now[2], 'G', now[3]].join(','), 'the gap moves with the drop place (' + await order() + ')');
  check(await page.locator('.tab.ghost').count() === 1, 'one ghost (the old gap closed)');
  check(await maps() === mapsBefore, 'no hit map while the gap is open (the host keeps the drop places from before it)');
  await emit(page, { event: 'tabDropHint', index: null });
  await page.waitForTimeout(400);
  check(await page.locator('.tab.ghost').count() === 0 && await order() === now.join(','), 'tabDropHint{index:null} closes the gap (' + await order() + ')');
  const back = (await lastSent(page, 'hitMap')).map.tabCenters;
  check(JSON.stringify(back) === JSON.stringify(hmAfter.tabCenters), 'the hit map is the same as before the gap');
  // Dropped: the tab takes the ghost's place, already open (no opening animation).
  await emit(page, { event: 'tabDropHint', index: 2, ghost: g });
  await page.waitForTimeout(350);
  const ghostBox = await page.locator('.tab.ghost').boundingBox();
  await page.evaluate((id) => {
    const h = window.__host;
    h.state.tabs.splice(2, 0, { id: id, title: 'Dragged here', url: 'https://g.example/', favicon: null, loading: false, private: false, sleeping: false });
    h.tabsEvent();
  }, g.id);
  await page.waitForTimeout(30);
  const dropped = page.locator('.tab[data-id="9"]');
  const db = await dropped.boundingBox();
  check(await page.locator('.tab.ghost').count() === 0 && !(await dropped.evaluate((n) => n.classList.contains('opening'))) &&
    db && ghostBox && Math.abs(db.x - ghostBox.x) < 2, 'the dropped tab takes the ghost\'s place without opening again (' + JSON.stringify(db) + ' vs ' + JSON.stringify(ghostBox) + ')');
  await emit(page, { event: 'tabDropHint', index: null });
  await page.evaluate(() => {
    const h = window.__host;
    h.state.tabs = h.state.tabs.filter((t) => t.id !== 9);
    h.tabsEvent();
  });
  await page.waitForTimeout(400);

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
  await emit(page, { event: 'popup', content: { kind: 'downloads', items: [
    { id: 3, url: 'https://x.example/big.bin', fileName: 'big.bin', path: 'C:\\d\\big.bin', state: 'inProgress', received: 25000000, total: 64000000, started: 1, speed: 4200000, eta: 9, connections: 4, partProgress: [{ done: 16000000, len: 16000000, start: 0, conn: 0 }, { done: 6000000, len: 16000000, start: 16000000, conn: 1 }, { done: 3000000, len: 16000000, start: 32000000, conn: 2 }, { done: 0, len: 16000000, start: 48000000, conn: 3 }] },
    { id: 2, url: 'https://x.example/one.bin', fileName: 'one.bin', path: 'C:\\d\\one.bin', state: 'inProgress', received: 1000, total: 4000, started: 1, speed: 500 },
    { id: 1, url: 'https://x.example/done.zip', fileName: 'done.zip', path: 'C:\\d\\done.zip', state: 'completed', received: 999, total: 999, started: 1, finished: 2 },
  ] } });
  await page.waitForTimeout(60);
  check(await page.locator('.dl-row').count() === 3 && await page.locator('#dl-all').count() === 1, 'downloads popup: 3 rows and "Show all downloads"');
  check((await page.textContent('.dl-row[data-id="3"] .dl-status')).includes('4.0 MB/s') && (await page.textContent('.dl-row[data-id="3"] .dl-status')).includes('9 s left'), 'a running download shows its speed and time left (' + await page.textContent('.dl-row[data-id="3"] .dl-status') + ')');
  check(await page.locator('.dl-row[data-id="3"] .dl-segments .seg').count() === 4, 'four connections: four bar segments');
  {
    const fills = await page.$$eval('.dl-row[data-id="3"] .dl-segments .seg .fill', (els) => els.map((e) => Number(/scaleX\(([\d.]+)\)/.exec(e.style.transform)[1])));
    check(fills[0] === 1 && fills[1] === 0.375 && fills[3] === 0, 'each segment is filled to its part\'s progress (' + fills.join(',') + ')');
  }
  check(await page.locator('.dl-row[data-id="2"] .dl-bar').count() === 1 && await page.locator('.dl-row[data-id="2"] .dl-segments').count() === 0, 'a single-connection download shows one bar');
  check(await page.locator('.dl-row[data-id="1"] .dl-icon[data-kind="archive"]').count() === 1 && await page.locator('.dl-row[data-id="2"] .dl-icon[data-kind="disk"]').count() === 1, 'file-type icons in the popup');
  {
    // R10: between the host's updates the bar moves on at the download's speed, never backwards.
    const scale = () => page.$eval('.dl-row[data-id="2"] .dl-fill', (e) => Number(/scaleX\(([\d.]+)\)/.exec(e.style.transform)[1]));
    const row = await page.$('.dl-row[data-id="2"]');
    const s0 = await scale();
    await page.waitForTimeout(400);
    const s1 = await scale();
    check(s0 >= 0.25 && s0 < 0.3 && s1 > s0 + 0.03 && s1 <= 0.25 + 500 * 1.5 / 4000 + 0.001, 'the single bar moves on between updates (' + s0.toFixed(3) + ' -> ' + s1.toFixed(3) + ')');
    // The host reports less than was shown: the bar waits instead of going back.
    await emit(page, { event: 'popup', content: { kind: 'downloads', items: [
      { id: 2, url: 'https://x.example/one.bin', fileName: 'one.bin', path: 'C:\\d\\one.bin', state: 'inProgress', received: 1050, total: 4000, started: 1, speed: 500 },
    ] } });
    await page.waitForTimeout(20);
    const s2 = await scale();
    check(s2 >= s1 && await row.evaluate((n) => n.isConnected), 'never backwards, and the row is updated in place (' + s1.toFixed(3) + ' -> ' + s2.toFixed(3) + ')');
    // Reduced motion: the bar jumps to each reported value.
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.reload();
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'popup', content: { kind: 'downloads', items: [
      { id: 2, url: 'https://x.example/one.bin', fileName: 'one.bin', path: 'C:\\d\\one.bin', state: 'inProgress', received: 1000, total: 4000, started: 1, speed: 500 },
    ] } });
    await page.waitForTimeout(400);
    check(Math.abs(await scale() - 0.25) < 0.001, 'with reduced motion the bar stays at the reported value');
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await emit(page, { event: 'popup', content: { kind: 'downloads', items: [] } });
    await page.waitForTimeout(30);
    check((await page.textContent('.dl-empty strong')) === 'No downloads yet', 'the popup says when there are no downloads');
    await shot(page, 'popup-downloads-empty.png');
    await emit(page, { event: 'popup', content: { kind: 'downloads', items: [
      { id: 3, url: 'https://x.example/big.bin', fileName: 'big.bin', path: 'C:\\d\\big.bin', state: 'inProgress', received: 25000000, total: 64000000, started: 1, speed: 4200000, eta: 9, connections: 4, partProgress: [{ done: 16000000, len: 16000000, start: 0, conn: 0 }, { done: 6000000, len: 16000000, start: 16000000, conn: 1 }, { done: 3000000, len: 16000000, start: 32000000, conn: 2 }, { done: 0, len: 16000000, start: 48000000, conn: 3 }] },
      { id: 1, url: 'https://x.example/done.zip', fileName: 'done.zip', path: 'C:\\d\\done.zip', state: 'completed', received: 999, total: 999, started: 1, finished: 2 },
    ] } });
    await page.waitForTimeout(60);
  }
  await page.hover('.dl-row[data-id="1"]');
  await page.waitForTimeout(200);
  check(await page.$eval('.dl-row[data-id="1"] .dl-acts', (e) => getComputedStyle(e).opacity) === '1', 'a finished download shows its actions on hover');
  await shot(page, 'popup-downloads.png');
  await page.click('.dl-row[data-id="1"] .dl-act.folder');
  check((await lastSent(page, 'downloadAction')).action === 'showInFolder', 'the folder button shows the file in its folder');
  {
    const fit = await lastSent(page, 'popupResize');
    check(fit && fit.height > 150, 'the downloads popup fits its content (' + (fit && fit.height) + ')');
  }
  await emit(page, { event: 'popup', content: { kind: 'downloads', items: [
    { id: 3, url: 'https://x.example/big.bin', fileName: 'big.bin', path: 'C:\\d\\big.bin', state: 'inProgress', received: 25000000, total: 64000000, started: 1, speed: 4200000, eta: 9, connections: 4, partProgress: [{ done: 16000000, len: 16000000, start: 0, conn: 0 }, { done: 6000000, len: 16000000, start: 16000000, conn: 1 }] },
  ] } });
  await page.waitForTimeout(40);
  await page.click('.dl-row[data-id="3"] .dl-act.pause');
  await page.waitForTimeout(20);
  {
    const a = await lastSent(page, 'downloadAction');
    check(a && a.id === 3 && a.action === 'pause', 'Pause in the popup sends downloadAction{pause}');
  }
  await page.click('#dl-all');
  await page.waitForTimeout(20);
  {
    const o = await lastSent(page, 'openPage');
    check(o && o.page === 'downloads', '"Show all downloads" opens the page');
  }
  await emit(page, { event: 'popup', content: { kind: 'addressMenu', canUndo: false, canCut: true, canCopy: true, canPaste: true, pasteIsUrl: true, canDelete: true, canSelectAll: true } });
  await page.waitForTimeout(50);
  check(await page.locator('.item').count() === 7 && await page.locator('.item[disabled]').count() === 1 && (await page.textContent('#root')).includes('Paste and go'), 'address menu: 7 items, Undo disabled, "Paste and go" for an address in the clipboard');
  {
    const fitMenu = await lastSent(page, 'popupResize');
    check(fitMenu && fitMenu.height > 150 && fitMenu.height < 260, 'the address menu fits its content (' + (fitMenu && fitMenu.height) + ')');
  }
  await page.click('.item:has-text("Paste and go")');
  await page.waitForTimeout(20);
  let am = await lastSent(page, 'addressMenuAction');
  check(am && am.action === 'pasteAndGo' && (await lastSent(page, 'hidePopup')) !== undefined, '"Paste and go" sends addressMenuAction{pasteAndGo} and closes');
  await emit(page, { event: 'popup', content: { kind: 'addressMenu', canUndo: true, canCut: false, canCopy: false, canPaste: true, pasteIsUrl: false, canDelete: false, canSelectAll: true } });
  await page.waitForTimeout(50);
  check((await page.textContent('#root')).includes('Paste and search') && await page.locator('.item[disabled]').count() === 3, '"Paste and search" for words; Cut/Copy/Delete disabled without a selection');
  await page.click('.item:has-text("Undo")');
  await page.waitForTimeout(20);
  am = await lastSent(page, 'addressMenuAction');
  check(am && am.action === 'undo', 'Undo sends addressMenuAction{undo}');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 5, canMove: true, canCloseRight: true, hasSite: true, mutedSite: false } });
  await page.waitForTimeout(30);
  check((await page.textContent('#root')).includes('Mute site') && !(await page.textContent('#root')).includes('Unmute site'), 'tab menu offers "Mute site" for a site');
  await page.click('.item:has-text("Mute site")');
  await page.waitForTimeout(20);
  {
    const a = await lastSent(page, 'tabMenuAction');
    check(a && a.id === 5 && a.action === 'muteSite', '"Mute site" sends tabMenuAction{muteSite}');
  }
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 5, canMove: true, canCloseRight: true, hasSite: true, mutedSite: true } });
  await page.waitForTimeout(30);
  check((await page.textContent('#root')).includes('Unmute site'), 'a muted site gets "Unmute site"');
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 5, canMove: true, canCloseRight: true, hasSite: false } });
  await page.waitForTimeout(30);
  check(await page.locator('.item:has-text("Mute site")[disabled]').count() === 1, 'no site (a built-in page): the mute item is disabled');
  // The title bar's menu.
  await emit(page, { event: 'popup', content: { kind: 'captionMenu', maximized: false, canReopen: true, canBookmarkAll: true } });
  await page.waitForTimeout(50);
  check(await page.locator('.item').count() === 9 && (await page.textContent('#root')).includes('Maximize') && (await page.textContent('#root')).includes('Task manager'), 'caption menu: 9 items incl. Maximize and Task manager');
  {
    const fit = await lastSent(page, 'popupResize');
    check(fit && fit.height > 200 && fit.height < 352, 'the caption menu fits its content (' + (fit && fit.height) + ')');
  }
  await page.click('.item:has-text("Minimize")');
  await page.waitForTimeout(20);
  {
    const w = await lastSent(page, 'captionWindow');
    check(w && w.action === 'minimize' && (await lastSent(page, 'hidePopup')) !== undefined, 'Minimize sends captionWindow{minimize} and closes');
  }
  await emit(page, { event: 'popup', content: { kind: 'captionMenu', maximized: true, canReopen: false, canBookmarkAll: false } });
  await page.waitForTimeout(30);
  check((await page.textContent('#root')).includes('Restore') && await page.locator('.item[disabled]').count() === 2, 'maximized: "Restore"; Reopen and Bookmark all disabled when nothing to do');
  await page.click('.item:has-text("Task manager")');
  await page.waitForTimeout(20);
  {
    const c = await lastSent(page, 'command');
    check(c && c.command && c.command.command === 'taskManager', '"Task manager" sends command{taskManager}');
  }
  await emit(page, { event: 'popup', content: { kind: 'captionMenu', maximized: false, canReopen: true, canBookmarkAll: true } });
  await page.waitForTimeout(30);
  await page.click('.item:has-text("Bookmark all tabs")');
  await page.waitForTimeout(20);
  check((await lastSent(page, 'bookmarkAllTabs')) !== undefined, '"Bookmark all tabs" sends bookmarkAllTabs');
  await emit(page, { event: 'popup', content: { kind: 'captionMenu', maximized: false, canReopen: true, canBookmarkAll: true } });
  await page.waitForTimeout(30);
  await page.click('.item:has-text("New private window")');
  await page.waitForTimeout(20);
  {
    const c = await lastSent(page, 'command');
    check(c && c.command && c.command.command === 'newPrivateWindow', '"New private window" sends command{newPrivateWindow}');
  }
  await emit(page, { event: 'popup', content: { kind: 'tabMenu', tabId: 7, canMove: false, canCloseRight: false } });
  await page.waitForSelector('.item');
  const labels = await page.$$eval('.item .label', (ns) => ns.map((n) => n.textContent));
  check(JSON.stringify(labels) === JSON.stringify(['Reload', 'Duplicate', 'Move to new window', 'New window', 'Mute site', 'Close tab', 'Close other tabs', 'Close tabs to the right']), 'tab menu items (' + labels.join(', ') + ')');
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
  check(await page.locator('.item.update-item').count() === 0, 'no update item without an update');
  // 0.7.2 (R13): "Update available – Krunk X.Y.Z" first; it asks for the update prompt.
  await page.setViewportSize({ width: 280, height: 670 });
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: false, update: '0.7.9' } });
  await page.waitForTimeout(30);
  const firstItem = page.locator('.item').first();
  check((await firstItem.textContent()).includes('Update available – Krunk 0.7.9') && await page.locator('.item.update-item .update-dot').count() === 1, 'the update item is the first one, with a dot');
  check(await page.evaluate(() => { const l = document.querySelector('.item.update-item .label'); return l.scrollWidth <= l.clientWidth; }), 'the update item is not shortened');
  check(await page.evaluate(() => document.getElementById('root').getBoundingClientRect().bottom <= 670), 'the menu with the update item fits 280x670');
  await shot(page, 'popup-menu-update.png');
  const hidesBefore = (await sent(page)).filter((x) => x.cmd === 'hidePopup').length;
  await firstItem.click();
  const ur = await lastSent(page, 'updateReply');
  check(ur && ur.choice === 'show' && (await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hidesBefore, 'it sends updateReply{show} (the prompt replaces the menu)');
  check(errors.length === 0, 'popup: no console errors (' + errors.join(' | ') + ')');
}

// 0.8.0 (B): the back/forward history menus. The shell asks with showNavMenu (never for a
// disabled button); the popup lists the entries, jumps by entry id and works from the keyboard.
async function testNavMenu(browser) {
  console.log('navMenu');
  {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 120 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; window.__host.navEvent({ tabId: 7, canGoBack: false, canGoForward: true }); });
    await page.waitForTimeout(80);
    check(await page.isDisabled('#back') && !(await page.isDisabled('#forward')), 'nav state: Back disabled, Forward enabled');
    await page.click('#back', { button: 'right', force: true });
    await page.waitForTimeout(40);
    check((await lastSent(page, 'showNavMenu')) === undefined, 'a disabled Back shows no menu');
    await page.click('#forward', { button: 'right' });
    const m = await lastSent(page, 'showNavMenu');
    const r = await page.evaluate(() => document.getElementById('forward').getBoundingClientRect().toJSON());
    check(m && m.forward === true && Math.abs(m.rect.x - r.left) < 1 && Math.abs(m.rect.y - (r.bottom + 2)) < 1, 'right-click Forward: showNavMenu{forward:true} under the button');
    await page.evaluate(() => window.__host.navEvent({ tabId: 7, canGoBack: true, canGoForward: false }));
    await page.waitForTimeout(80);
    await page.click('#back', { button: 'right' });
    check((await lastSent(page, 'showNavMenu')).forward === false, 'right-click Back: showNavMenu{forward:false}');
    check((await sent(page)).filter((x) => x.cmd === 'back').length === 0, 'a right-click does not go back');
    check(errors.length === 0, 'shell: no console errors (' + errors.join(' | ') + ')');
  }
  const longTitle = 'A very long page title '.repeat(13);
  const items = [];
  for (let i = 14; i >= 0; i--) items.push({ entryId: 100 + i, label: i === 3 ? longTitle : 'Page ' + i, url: 'http://localhost:8094/p' + i + '.html', icon: i % 2 ? '/favicon?host=localhost' : null });
  items[5].label = items[5].url; // no title: the address
  for (const theme of ['light', 'dark']) {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 340, height: 520 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'theme', theme: theme });
    await emit(page, { event: 'popup', content: { kind: 'navMenu', tabId: 7, forward: false, items: items } });
    await page.waitForSelector('#root[data-kind="navMenu"] .item');
    const labels = await page.$$eval('#root .item .label', (l) => l.map((x) => x.textContent));
    check(labels.length === 16 && labels[0] === 'Page 14' && labels[14] === 'Page 0' && labels[15] === 'Show full history', theme + ': 15 entries as given (newest first), then Show full history');
    check(await page.$$eval('#root .item img.fav', (l) => l.length) === 7 && await page.$$eval('#root .item svg use[href="#m-globe"]', (l) => l.length) === 8, theme + ': favicons, and a globe without one');
    const longRow = await page.$eval('#root .item:nth-child(12) .label', (x) => ({ sw: x.scrollWidth, cw: x.clientWidth, ws: getComputedStyle(x).whiteSpace, to: getComputedStyle(x).textOverflow }));
    check(longRow.sw > longRow.cw && longRow.ws === 'nowrap' && longRow.to === 'ellipsis', theme + ': a 300-character title stays on one line with "…"');
    const width = await page.evaluate(() => document.documentElement.scrollWidth);
    check(width <= 340, theme + ': no horizontal scroll (' + width + ')');
    const fit = await lastSent(page, 'popupResize');
    check(fit && fit.height > 16 * 28 && fit.height < 520, theme + ': fitted to its rows (' + (fit && fit.height) + ')');
    check(await page.evaluate(() => document.activeElement && document.activeElement.textContent) === 'Page 14', theme + ': the first entry has the focus');
    await shot(page, 'navmenu-' + theme + '.png');
    // Keyboard: down, up (wraps to the end), home, end, Enter.
    await page.keyboard.press('ArrowDown');
    check(await page.evaluate(() => document.activeElement.textContent) === 'Page 13', theme + ': ArrowDown moves down');
    await page.keyboard.press('Home');
    await page.keyboard.press('ArrowUp');
    check(await page.evaluate(() => document.activeElement.textContent).then((t) => t.includes('Show full history')), theme + ': ArrowUp from the first wraps to the last');
    await page.keyboard.press('Home');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    const j = await lastSent(page, 'navToEntry');
    check(j && j.tabId === 7 && j.entryId === 112 && (await lastSent(page, 'hidePopup')) !== undefined, theme + ': Enter jumps to that entry (navToEntry{tabId:7, entryId:112}) and closes');
    await emit(page, { event: 'popup', content: { kind: 'navMenu', tabId: 7, forward: true, items: items.slice(0, 2) } });
    await page.waitForTimeout(60);
    await page.click('#root .item:first-child');
    check((await lastSent(page, 'navToEntry')).entryId === 114, theme + ': a click jumps too');
    await emit(page, { event: 'popup', content: { kind: 'navMenu', tabId: 7, forward: true, items: items.slice(0, 2) } });
    await page.waitForTimeout(60);
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    const h = await lastSent(page, 'command');
    check(h && h.command.command === 'history', theme + ': Show full history opens krunk://history');
    const hides = (await sent(page)).filter((x) => x.cmd === 'hidePopup').length;
    await emit(page, { event: 'popup', content: { kind: 'navMenu', tabId: 7, forward: true, items: items.slice(0, 2) } });
    await page.waitForTimeout(60);
    await page.keyboard.press('Escape');
    check((await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hides + 1, theme + ': Esc closes');
    check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
  }
}

// 0.8.0 (C): Settings → Account. The page only shows what the host says (the `account` event)
// and asks with posts; it never talks to the network itself.
// For the VM scripts (ONLY=MeasureAccount): where Settings → Account's buttons are at the VM's
// page size (1280 × 656 under the shell and the bookmarks bar; 1280 × 684 without the bar).
async function measureAccountCard(browser) {
  const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
  for (const h of [656, 684]) {
    const { page } = await openPage(browser, 'https://krunk.invalid/settings.html#account', { width: 1280, height: h }, 1, settingsMock, defaults);
    await page.waitForFunction(() => document.getElementById('lists-summary').textContent !== '');
    const at = async (id) => { const b = await page.locator('#' + id).boundingBox(); return b && [Math.round(b.x + b.width / 2), Math.round(b.y + b.height / 2)]; };
    await emit(page, { event: 'account', card: { state: 'signedOut' } });
    await emit(page, { event: 'sync', view: { state: 'noAccount' } });
    await page.waitForTimeout(60);
    const signIn = await at('account-sign-in');
    await emit(page, { event: 'account', card: { state: 'linking', userCode: 'ABCD-EFGH', expiresInS: 600 } });
    await page.waitForTimeout(60);
    const copy = await at('account-copy');
    await emit(page, { event: 'account', card: { state: 'signedIn', email: 'vm090@example.org', device: 'Krunk on Windows', lastCheckedMs: Date.now() } });
    await emit(page, { event: 'sync', view: { state: 'off', message: null } });
    await page.waitForTimeout(60);
    console.log(JSON.stringify({ height: h, signIn: signIn, copyCode: copy, signOut: await at('account-sign-out'), syncOpen: await at('sync-open') }));
    await page.context().close();
  }
}

async function testAccountCard(browser) {
  console.log('account card');
  const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
  for (const theme of ['light', 'dark']) {
    const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#account', { width: 1000, height: 700 }, 1, settingsMock, defaults);
    await page.waitForFunction(() => document.getElementById('lists-summary').textContent !== '');
    await emit(page, { event: 'theme', theme: theme });
    check(await page.locator('.panel.active[data-tab=account]').count() === 1, theme + ': #account opens the Account tab');
    const st = (await sent(page)).filter((m) => m.cmd === 'accountState');
    check(st.length >= 1 && st.some((m) => m.check === true), theme + ': the shown card asks for the state (check: true)');
    const vis = (id) => page.isVisible('#' + id);
    // Signed out.
    await emit(page, { event: 'account', card: { state: 'signedOut' } });
    await page.waitForTimeout(40);
    check((await page.textContent('#account-title')) === 'Sign in to Krunk' && await vis('account-sign-in') && !(await vis('account-sign-out')) && !(await vis('account-message')), theme + ': signed out: Sign in');
    check(!(await page.textContent('#account-row')).includes('Sync is coming'), theme + ': no "Sync is coming" line any more (0.9.0)');
    await shot(page, 'account-signed-out-' + theme + '.png');
    await page.click('#account-sign-in');
    check((await lastSent(page, 'accountSignIn')) !== undefined, theme + ': Sign in posts accountSignIn');
    // Working, then the code.
    await emit(page, { event: 'account', card: { state: 'working', message: 'Asking krunk.209.wf for a code…' } });
    await page.waitForTimeout(40);
    check((await page.textContent('#account-title')).startsWith('Asking') && !(await vis('account-sign-in')), theme + ': working: no buttons');
    await emit(page, { event: 'account', card: { state: 'linking', userCode: 'ABCD-EFGH', verifyUrl: 'https://krunk.209.wf/link?code=ABCD-EFGH', expiresAtMs: Date.now() + 9 * 60000 + 30000 } });
    await page.waitForTimeout(60);
    check((await page.textContent('#account-code')) === 'ABCD-EFGH' && await vis('account-copy') && await vis('account-cancel') && await vis('account-reopen'), theme + ': linking: the code, Copy, Open again, Cancel');
    check(/9:(2|3)\d more/.test(await page.textContent('#account-wait')), theme + ': a countdown (' + (await page.textContent('#account-wait')) + ')');
    await shot(page, 'account-linking-' + theme + '.png');
    await page.click('#account-reopen');
    check((await lastSent(page, 'accountReopen')) !== undefined, theme + ': Open the page again');
    await page.click('#account-cancel');
    check((await lastSent(page, 'accountCancel')) !== undefined, theme + ': Cancel posts accountCancel');
    // An error, then signed in.
    await emit(page, { event: 'account', card: { state: 'signedOut', message: "Krunk couldn't reach krunk.209.wf. Check your connection and try again." } });
    await page.waitForTimeout(40);
    check(await vis('account-message') && (await page.textContent('#account-message')).includes("couldn't reach"), theme + ': an error is one clear sentence');
    await shot(page, 'account-error-' + theme + '.png');
    await emit(page, { event: 'account', card: { state: 'signedIn', email: 'someone@example.org', deviceName: 'Krunk on Windows 11', version: '0.11.0' } });
    await page.waitForTimeout(40);
    check((await page.textContent('#account-title')) === 'Signed in as someone@example.org' && (await page.textContent('#account-desc')).includes('This device: Krunk on Windows 11 · version 0.11.0.'), theme + ': signed in: the address, the device name and the running version');
    check(await vis('account-manage') && await vis('account-sign-out') && !(await vis('account-sign-in')), theme + ': Manage account and Sign out on this device');
    await shot(page, 'account-signed-in-' + theme + '.png');
    await page.click('#account-manage');
    check((await lastSent(page, 'accountManage')) !== undefined, theme + ': Manage account');
    await page.click('#account-sign-out');
    check((await lastSent(page, 'accountSignOut')) !== undefined, theme + ': Sign out on this device');
    // A private window.
    await emit(page, { event: 'account', card: { state: 'private' } });
    await page.waitForTimeout(40);
    check((await page.textContent('#account-row')).includes("isn't shown in private windows") && !(await vis('account-sign-in')) && !(await vis('account-sign-out')), theme + ': private: nothing of the account');
    // Search finds it.
    await page.fill('#filter', 'sign out');
    await page.waitForTimeout(80);
    check(await page.isVisible('#account-row'), theme + ': settings search finds the account row');
    check(external.length === 0, theme + ': no network requests from the page (' + external.join(', ') + ')');
    check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
  }
  // The ⋮ menu's account item.
  const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 280, height: 700 }, 1);
  await page.evaluate(() => { window.__host.autoSimulate = false; });
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: false, account: '' } });
  await page.waitForSelector('.item');
  check((await page.locator('.item').first().textContent()) === 'Sign in to Krunk', 'menu: "Sign in to Krunk" first');
  await page.locator('.item', { hasText: 'Sign in to Krunk' }).click();
  const op = await lastSent(page, 'openPage');
  check(op && op.page === 'settings' && op.section === 'account', 'menu: it opens Settings → Account');
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: false, account: 'a-rather-long-address.for.testing@example.org' } });
  await page.waitForTimeout(60);
  const label = await page.$eval('.item.account-item .label', (x) => ({ t: x.textContent, sw: x.scrollWidth, cw: x.clientWidth }));
  check(label.t === 'Signed in as a-rather-long-address.for.testing@example.org' && label.sw >= label.cw, 'menu: "Signed in as …", shortened if long');
  await shot(page, 'menu-account.png');
  const fit = await lastSent(page, 'popupResize');
  check(fit && fit.height < 700, 'menu: still fits (' + (fit && fit.height) + ')');
  await emit(page, { event: 'popup', content: { kind: 'menu', zoom: 1, canReopen: false } });
  await page.waitForTimeout(60);
  check(await page.locator('.item.account-item').count() === 0, 'menu: no account item in a private window');
  check(errors.length === 0, 'menu: no console errors (' + errors.join(' | ') + ')');
}

// 0.9.0: krunk://sync. Every view the host sends, in both themes; each control posts what the
// host expects; the page itself never sends anything but syncState (and the vault status) on its
// own; no native dialog, no network.
function syncMock() {
  const h = window.__host.handlers;
  h.vaultStatus = () => ({ exists: true, locked: false, busy: false, waitMs: 0 });
  h.syncSetupCopy = () => ({});
  h.syncSetupSave = () => ({ saved: true });
}

async function testSyncPage(browser) {
  console.log('sync page');
  for (const theme of ['light', 'dark']) {
    const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/sync.html', { width: 900, height: 900 }, 1, syncMock);
    await page.waitForTimeout(80);
    await emit(page, { event: 'theme', theme: theme });
    const t = (x) => theme + ': ' + x;
    const cmds = (await sent(page)).map((m) => m.cmd).filter((c) => c !== 'ready');
    check(JSON.stringify(cmds.sort()) === JSON.stringify(['syncState', 'vaultStatus']), t('on load only syncState and vaultStatus (' + cmds + ')'));
    const vis = (id) => page.isVisible('#' + id);
    const view = async (v) => { await emit(page, { event: 'sync', view: v }); await page.waitForTimeout(40); };
    // Off: turn on (with passwords: the vault is unlocked).
    await view({ state: 'off', message: null });
    check(await vis('v-off') && !(await vis('v-on')), t('off view'));
    check(await page.isChecked('#off-passwords') && await page.isEnabled('#off-passwords'), t('passwords can be included (vault unlocked)'));
    await shot(page, 'sync-off-' + theme + '.png');
    const before = (await sent(page)).length;
    await page.waitForTimeout(300);
    check((await sent(page)).length === before, t('nothing is sent while the user does nothing'));
    await page.click('#turn-on');
    const on = await lastSent(page, 'syncTurnOn');
    check(on && on.passwords === true, t('Turn on sync posts syncTurnOn {passwords: true}'));
    // No account.
    await view({ state: 'noAccount' });
    check(await vis('v-noaccount'), t('no account: sign in first'));
    await page.click('#go-account');
    const op = await lastSent(page, 'openPage');
    check(op && op.page === 'settings' && op.section === 'account', t('opens Settings → Account'));
    // Working.
    await view({ state: 'working', message: 'Checking your Krunk account…' });
    check((await page.textContent('#working-text')) === 'Checking your Krunk account…', t('working message'));
    // Setup: the secrets, copy, save, confirm.
    await view({ state: 'setup', passphrase: 'apple-banana-cherry-dog-egg-fig-grape', recoveryKey: 'KRNK-AAAA-BBBB-CCCC-DDDD-EEEE-FFFF-GGGG-HHHH-Z', passwords: true, message: null });
    check((await page.textContent('#setup-passphrase')) === 'apple-banana-cherry-dog-egg-fig-grape' && (await page.textContent('#setup-recovery')).startsWith('KRNK-'), t('setup shows the passphrase and the recovery key'));
    await shot(page, 'sync-setup-' + theme + '.png');
    await page.click('#setup-copy');
    await page.waitForTimeout(60);
    check((await page.textContent('#setup-saved')).includes('Copied'), t('Copy both'));
    await page.click('#setup-save');
    await page.waitForTimeout(60);
    check((await page.textContent('#setup-saved')) === 'Saved.', t('Save to a file'));
    await page.fill('#setup-group', 'hhhh');
    await page.check('#setup-understood');
    await page.click('#setup-confirm');
    const conf = await lastSent(page, 'syncSetupConfirm');
    check(conf && conf.group === 'hhhh' && conf.understood === true, t('confirm posts the typed group and the tick'));
    await view({ state: 'setup', passphrase: 'a', recoveryKey: 'KRNK-X', passwords: true, message: 'Type the last group of 4 characters.' });
    check(await vis('setup-error') && (await page.textContent('#setup-error')).includes('last group'), t('a wrong group is explained'));
    await page.click('#setup-cancel');
    check((await lastSent(page, 'syncCancel')) !== undefined, t('cancel'));
    // Join: restore by recovery key; ask another device.
    await view({ state: 'join', message: null, passwords: true });
    check(await vis('v-join'), t('join view'));
    check(await page.evaluate(() => document.activeElement.id) === 'restore-secret', t('the restore field has the focus'));
    await shot(page, 'sync-join-' + theme + '.png');
    await page.check('input[name=kind][value=recovery]');
    await page.fill('#restore-secret', 'KRNK-1234-5678');
    await page.click('#restore-go');
    const rs = await lastSent(page, 'syncRestore');
    check(rs && rs.secret === 'KRNK-1234-5678' && rs.recovery === true && (await page.inputValue('#restore-secret')) === '', t('restore posts the secret once and clears the field'));
    await page.check('input[name=kind][value=passphrase]');
    await page.fill('#restore-secret', ' krnk-abcd-efgh');
    await page.keyboard.press('Enter');
    const rs2 = await lastSent(page, 'syncRestore');
    check(rs2 && rs2.recovery === true, t('a recovery key is recognised by its prefix'));
    await page.click('#join-ask');
    check((await lastSent(page, 'syncJoin')) !== undefined, t('ask my other devices'));
    check((await page.textContent('#restore-vault-hint')).includes('unlocked password vault'), t('restore: passwords go into the unlocked vault'));
    await view({ state: 'join', message: "That passphrase or recovery key doesn't open your sync data.", passwords: false });
    check((await page.textContent('#join-error')).includes("doesn't open"), t('a wrong secret is said'));
    // Restore confirm.
    await view({ state: 'restoreConfirm', devices: ['Krunk on DESKTOP', 'Laptop'], revoked: 1 });
    check((await page.$$eval('#restore-devices li', (l) => l.length)) === 2 && (await page.textContent('#restore-revoked')).includes('1 removed'), t('restore lists the devices that keep access'));
    check(await page.evaluate(() => document.activeElement.id) === 'restore-confirm', t('the confirm button has the focus'));
    await page.click('#restore-confirm');
    check((await lastSent(page, 'syncRestoreConfirm')) !== undefined, t('confirm the devices'));
    // Joining: waiting, then the code.
    await view({ state: 'joining', enterCode: false, message: null });
    check(!(await vis('code-form')) && await vis('joining-wait'), t('joining: waiting'));
    await view({ state: 'joining', enterCode: true, message: null });
    check(await vis('code-form'), t('joining: the code field'));
    await page.fill('#code', '482 917');
    await page.click('#code-go');
    const code = await lastSent(page, 'syncJoinCode');
    check(code && code.code === '482917', t('the 6 digits are sent without the space'));
    await shot(page, 'sync-joining-' + theme + '.png');
    // On: devices, a request to join, the approval in progress, remove, turn off.
    const onView = {
      state: 'on', status: 'Up to date', lastSyncMs: Date.now() - 120000, passwords: true, passwordsLocked: false, rotationPending: false, message: null, syncing: false,
      collections: [{ name: 'bookmarks', label: 'Bookmarks', on: true }, { name: 'history', label: 'History', on: false }, { name: 'tabs', label: 'Open tabs', on: true }],
      remoteTabs: [{ device: 'Old laptop', updatedMs: Date.now() - 60000, tabs: [['Example', 'https://example.org/'], ['', 'https://rust-lang.org/']] }],
      confirmDevices: [], secureNeeded: false,
      devices: [{ id: 'd1', name: 'Krunk on DESKTOP', platform: 'windows', keys: true, me: true, lastSync: null }, { id: 'd2', name: 'Old laptop', platform: 'windows', keys: true, me: false, lastSync: new Date(Date.now() - 3700000).toISOString() }],
      approvals: [{ id: 'ap1', label: 'New PC', platform: 'windows', country: 'NL' }], approving: null,
    };
    await view(onView);
    check(await vis('v-on') && (await page.$$eval('#device-list li', (l) => l.length)) === 2, t('on: the devices'));
    check((await page.textContent('#device-list')).includes('This device'), t('this device is marked'));
    check((await page.textContent('#device-list')).includes('last synced 1 hour ago'), t('"1 hour ago", not "1 hours"'));
    check(await vis('approvals') && await vis('share-row'), t('a device asks to join; passwords can be shared'));
    await shot(page, 'sync-on-' + theme + '.png');
    await page.check('#share-passwords');
    await page.click('#approval-list button');
    const ap = await lastSent(page, 'syncApprove');
    check(ap && ap.id === 'ap1' && ap.sharePasswords === true, t('Approve posts the id and the share choice'));
    await view(Object.assign({}, onView, { approving: { id: 'ap1', label: 'New PC', code: '482 917', done: false } }));
    check((await page.textContent('#approving-code')) === '482 917' && !(await vis('approvals')), t('the code to type on the new device'));
    await shot(page, 'sync-approving-' + theme + '.png');
    await view(Object.assign({}, onView, { approvals: [], approving: { id: '', label: 'New PC', code: null, done: true } }));
    check((await page.textContent('#approving-title')).includes('approved'), t('approved'));
    check((await page.textContent('#on-last')).startsWith('Last synced 2 minutes ago'), t('last sync time'));
    await page.click('#sync-now');
    check((await lastSent(page, 'syncNow')) !== undefined, t('Sync now'));
    check(await page.isChecked('#coll-bookmarks') && !(await page.isChecked('#coll-history')), t('collections show their state'));
    await page.check('#coll-history');
    const sc = await lastSent(page, 'syncSetCollection');
    check(sc && sc.name === 'history' && sc.on === true, t('a collection is switched on'));
    check(await vis('remote') && (await page.$$eval('#remote-list li a', (l) => l.map((a) => a.textContent))).join('|') === 'Example|https://rust-lang.org/', t('open tabs of the other device (titles, or the address)'));
    await page.click('#remote-list li a');
    const ou = await lastSent(page, 'openUrl');
    check(ou && ou.url === 'https://example.org/' && ou.newTab === true, t('an open tab opens in a new tab here'));
    await page.click('#device-list li:nth-child(1) button:text("Rename")');
    await page.waitForSelector('#lb-dialog-input');
    await page.fill('#lb-dialog-input', '  Desk PC  ');
    await acceptInPage(page);
    const rn = await lastSent(page, 'syncRenameDevice');
    check(rn && rn.id === 'd1' && rn.name === 'Desk PC', t('Rename (in-page prompt, trimmed)'));
    await view(Object.assign({}, onView, { syncing: true, status: 'Syncing…' }));
    check(!(await page.isEnabled('#sync-now')) && (await page.getAttribute('#sync-now', 'aria-busy')) === 'true', t('Sync now is busy while syncing'));
    await view(Object.assign({}, onView, { remoteTabs: [] }));
    check(!(await vis('remote')), t('no other devices\' tabs: no section'));
    check(!(await vis('confirm-rotation')) && !(await vis('secure')), t('no rotation card, no securing card by default'));
    await view(Object.assign({}, onView, { confirmDevices: ['Laptop (added by the removed PC)'] }));
    check(await vis('confirm-rotation') && (await page.textContent('#confirm-list')).includes('Laptop'), t('a rotation waits for the device list to be confirmed'));
    await shot(page, 'sync-confirm-rotation-' + theme + '.png');
    await page.click('#confirm-go');
    check((await lastSent(page, 'syncConfirmRotation')) !== undefined, t('confirm posts syncConfirmRotation'));
    await view(Object.assign({}, onView, { secureNeeded: true }));
    check(await vis('secure'), t('Finish securing is offered after a rotation'));
    await shot(page, 'sync-secure-' + theme + '.png');
    await page.click('#secure-go');
    check((await lastSent(page, 'syncFinishSecuring')) === undefined, t('an empty secret is not sent'));
    await page.check('input[name=secure-kind][value=recovery]');
    await page.fill('#secure-secret', ' KRNK-AAAA-BBBB ');
    await page.click('#secure-go');
    const fs1 = await lastSent(page, 'syncFinishSecuring');
    check(fs1 && fs1.secret === 'KRNK-AAAA-BBBB' && fs1.recovery === true && (await page.inputValue('#secure-secret')) === '', t('Finish securing posts the secret once and clears the field'));
    await page.click('#device-list li:nth-child(2) button:text("Remove")');
    await acceptInPage(page);
    const rm = await lastSent(page, 'syncRemoveDevice');
    check(rm && rm.id === 'd2', t('Remove (after the in-page confirmation)'));
    await view(Object.assign({}, onView, { rotationPending: true, approvals: [] }));
    check(await vis('on-rotation'), t('re-securing after a removal'));
    await page.click('#turn-off');
    await acceptInPage(page);
    const off = await lastSent(page, 'syncTurnOff');
    check(off && off.deleteData === false, t('Turn off (keeps data)'));
    // Alarm and private.
    await view({ state: 'alarm', message: "Sync's keys on the server don't check out." });
    check(await vis('v-alarm'), t('alarm view'));
    await shot(page, 'sync-alarm-' + theme + '.png');
    await view({ state: 'private' });
    check(await vis('v-private') && !(await vis('v-on')), t('private window: nothing of sync'));
    check(external.length === 0, t('no network requests (' + external.join(', ') + ')'));
    check(errors.length === 0, t('no console errors, no native dialogs (' + errors.join(' | ') + ')'));
  }
}

async function testSyncCard(browser) {
  console.log('settings sync card');
  const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
  for (const theme of ['light', 'dark']) {
    const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#account', { width: 1000, height: 900 }, 1, settingsMock, defaults);
    await page.waitForFunction(() => document.getElementById('lists-summary').textContent !== '');
    await emit(page, { event: 'theme', theme: theme });
    const t = (x) => theme + ': ' + x;
    check((await sent(page)).some((m) => m.cmd === 'syncState'), t('Settings asks for the sync view'));
    check(!(await sent(page)).some((m) => m.cmd.startsWith('sync') && m.cmd !== 'syncState'), t('and nothing else of sync on its own'));
    const view = async (v) => { await emit(page, { event: 'sync', view: v }); await page.waitForTimeout(40); };
    await view({ state: 'noAccount' });
    check((await page.textContent('#sync-desc')).includes('Sign in') && !(await page.isVisible('#sync-open')), t('no account: sign in first'));
    await view({ state: 'off', message: null });
    check((await page.textContent('#sync-title')) === 'Sync is off' && await page.isVisible('#sync-open'), t('off: Turn on sync…'));
    await shot(page, 'settings-sync-off-' + theme + '.png');
    await page.click('#sync-open');
    check((await lastSent(page, 'syncOpen')) !== undefined, t('opens krunk://sync'));
    await view({ state: 'on', status: 'Up to date', lastSyncMs: Date.now() - 60000, passwords: false, passwordsLocked: false, devices: [], approvals: [], approving: null, rotationPending: false, message: null, syncing: false, remoteTabs: [],
      collections: [{ name: 'bookmarks', label: 'Bookmarks', on: true }, { name: 'history', label: 'History', on: true }] });
    check((await page.textContent('#sync-title')) === 'Sync is on' && await page.isVisible('#sync-manage'), t('on: Manage sync'));
    check((await page.textContent('#sync-desc')) === 'Up to date' && (await page.textContent('#sync-last')).startsWith('Last synced'), t('on: the status and the last sync'));
    await page.click('#sync-now');
    check((await lastSent(page, 'syncNow')) !== undefined, t('Sync now from Settings'));
    await page.uncheck('#sync-coll-history');
    const sc = await lastSent(page, 'syncSetCollection');
    check(sc && sc.name === 'history' && sc.on === false, t('a collection is switched off from Settings'));
    await shot(page, 'settings-sync-on-' + theme + '.png');
    await view({ state: 'private' });
    check(!(await page.isVisible('#sync-card')), t('private window: no sync card'));
    check(external.length === 0, t('no network requests'));
    check(errors.length === 0, t('no console errors (' + errors.join(' | ') + ')'));
  }
}

// 0.8.0: a site's HTTP sign-in dialog. Typed credentials, a saved login (the password stays in
// the host), several saved logins, a locked vault, a refused answer, Cancel and Esc.
async function testHttpAuth(browser) {
  console.log('http auth dialog');
  const base = { kind: 'httpAuth', tabId: 5, origin: 'http://localhost:8094', realm: 'Krunk test area', insecure: true, logins: [], locked: false, failed: false };
  for (const theme of ['light', 'dark']) {
    const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 380, height: 420 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'theme', theme: theme });
    // Typed.
    await emit(page, { event: 'popup', content: Object.assign({}, base, { realm: '<b>Admin</b> area' }) });
    await page.waitForSelector('#auth-user');
    check((await page.textContent('.http-auth .origin')) === 'http://localhost:8094' && (await page.textContent('.auth-realm')) === 'The site says: <b>Admin</b> area' && await page.$('.auth-realm b') === null, theme + ': origin, and the realm as plain text');
    check((await page.textContent('.auth-insecure')).startsWith('Not secure'), theme + ': "Not secure" over http');
    check(await page.evaluate(() => document.activeElement.id) === 'auth-user', theme + ': the user name field has the focus');
    await shot(page, 'httpauth-typed-' + theme + '.png');
    await page.fill('#auth-user', 'tester');
    await page.fill('#auth-pass', 's3cret-pass');
    await page.keyboard.press('Enter');
    const r1 = await lastSent(page, 'httpAuthReply');
    check(r1 && r1.tabId === 5 && r1.username === 'tester' && r1.password === 's3cret-pass' && !r1.entryId, theme + ': Enter sends the typed user name and password');
    check(await page.inputValue('#auth-pass') === '', theme + ': the password field is cleared after sending');
    // One saved login: filled, its password stays in the vault.
    await emit(page, { event: 'popup', content: Object.assign({}, base, { insecure: false, origin: 'https://intranet.example', logins: [{ entryId: 'e1', username: 'alice' }] }) });
    await page.waitForTimeout(60);
    check(await page.inputValue('#auth-user') === 'alice' && (await page.getAttribute('#auth-pass', 'placeholder')) === 'Saved password' && await page.$('.auth-insecure') === null, theme + ': one saved login fills the fields (https: no warning)');
    check(await page.evaluate(() => document.activeElement.id) === 'auth-ok', theme + ': Sign in has the focus, nothing was sent yet');
    const before = (await sent(page)).filter((m) => m.cmd === 'httpAuthReply').length;
    check(before === 1, theme + ': no answer without a click');
    await shot(page, 'httpauth-saved-' + theme + '.png');
    await page.click('#auth-ok');
    const r2 = await lastSent(page, 'httpAuthReply');
    check(r2.entryId === 'e1' && !r2.password, theme + ': Sign in sends only the saved login\'s id');
    // Typing a password instead uses what was typed.
    await emit(page, { event: 'popup', content: Object.assign({}, base, { logins: [{ entryId: 'e1', username: 'alice' }] }) });
    await page.waitForTimeout(60);
    await page.fill('#auth-pass', 'other');
    await page.click('#auth-ok');
    const r3 = await lastSent(page, 'httpAuthReply');
    check(!r3.entryId && r3.username === 'alice' && r3.password === 'other', theme + ': a typed password overrides the saved one');
    // Several saved logins.
    await emit(page, { event: 'popup', content: Object.assign({}, base, { logins: [{ entryId: 'e1', username: 'alice' }, { entryId: 'e2', username: 'bob' }] }) });
    await page.waitForTimeout(60);
    check(await page.locator('.auth-pick').count() === 2 && await page.inputValue('#auth-user') === '', theme + ': several saved logins are listed, none chosen');
    await page.locator('.auth-pick', { hasText: 'bob' }).click();
    check(await page.inputValue('#auth-user') === 'bob', theme + ': a click chooses one');
    await shot(page, 'httpauth-several-' + theme + '.png');
    await page.click('#auth-ok');
    check((await lastSent(page, 'httpAuthReply')).entryId === 'e2', theme + ': and Sign in uses it');
    // Locked vault, refused answer.
    await emit(page, { event: 'popup', content: Object.assign({}, base, { locked: true, failed: true }) });
    await page.waitForTimeout(60);
    check((await page.textContent('.http-auth .error')).includes('not accepted'), theme + ': a refused answer says so');
    await page.click('#auth-unlock');
    check((await lastSent(page, 'httpAuthUnlock')).tabId === 5, theme + ': Unlock asks for the vault');
    const fit = await lastSent(page, 'popupResize');
    check(fit && fit.height > 200 && fit.height < 420, theme + ': fitted (' + (fit && fit.height) + ')');
    // Cancel and Esc.
    await page.click('#auth-cancel');
    check((await lastSent(page, 'httpAuthCancel')).tabId === 5, theme + ': Cancel');
    const cancels = (await sent(page)).filter((m) => m.cmd === 'httpAuthCancel').length;
    await emit(page, { event: 'popup', content: base });
    await page.waitForTimeout(60);
    await page.keyboard.press('Escape');
    check((await sent(page)).filter((m) => m.cmd === 'httpAuthCancel').length === cancels + 1, theme + ': Esc cancels');
    check(external.length === 0, theme + ': no network requests');
    check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
  }
}

(async () => {
  if (SHOTS) fs.mkdirSync(SHOTS, { recursive: true });
  const browser = await chromium.launch();
  try {
    if (!ONLY || ONLY.includes('Shell')) await testShell(browser);
    if (!ONLY || ONLY.includes('TabStripCost')) await testTabStripCost(browser);
    if (!ONLY || ONLY.includes('ScriptletInjection')) await testScriptletInjection(browser);
    if (!ONLY || ONLY.includes('PointerLockShim')) await testPointerLockShim(browser);
    if (!ONLY || ONLY.includes('ShellFrame')) await testShellFrame(browser);
    if (!ONLY || ONLY.includes('TabAnimations')) await testTabAnimations(browser);
    if (!ONLY || ONLY.includes('TabWindows')) await testTabWindows(browser);
    if (!ONLY || ONLY.includes('ShellHiDpi')) await testShellHiDpi(browser);
    if (!ONLY || ONLY.includes('Popup')) await testPopup(browser);
    if (!ONLY || ONLY.includes('NavMenu')) await testNavMenu(browser);
    if (!ONLY || ONLY.includes('HttpAuth')) await testHttpAuth(browser);
    if (!ONLY || ONLY.includes('NewTab')) await testNewTab(browser);
    if (!ONLY || ONLY.includes('NewTabLongTitles')) await testNewTabLongTitles(browser);
    if (!ONLY || ONLY.includes('NewTabBackgrounds')) await testNewTabBackgrounds(browser);
    if (!ONLY || ONLY.includes('History')) await testHistory(browser);
    if (!ONLY || ONLY.includes('Bookmarks')) await testBookmarks(browser);
    if (!ONLY || ONLY.includes('Downloads')) await testDownloads(browser);
    if (!ONLY || ONLY.includes('Storage')) await testStorage(browser);
    if (!ONLY || ONLY.includes('StorageLarge')) await testStorageLarge(browser);
    if (!ONLY || ONLY.includes('Tasks')) await testTasks(browser);
    if (!ONLY || ONLY.includes('Extensions')) await testExtensions(browser);
    if (!ONLY || ONLY.includes('ExtShim')) await testExtShim(browser);
    if (!ONLY || ONLY.includes('SiteData')) await testSiteData(browser);
    if (!ONLY || ONLY.includes('ShellM4')) await testShellM4(browser);
    if (!ONLY || ONLY.includes('BookmarksBar')) await testBookmarksBar(browser);
    if (!ONLY || ONLY.includes('FolderPopup')) await testFolderPopup(browser);
    if (!ONLY || ONLY.includes('FolderPopup')) await testFolderPopupLong(browser);
    if (!ONLY || ONLY.includes('BarOverflow')) await testBarOverflow(browser);
    if (!ONLY || ONLY.includes('TabOverflow')) await testTabOverflow(browser);
    if (!ONLY || ONLY.includes('TabList')) await testTabList(browser);
    if (!ONLY || ONLY.includes('HangAndCrash')) await testHangAndCrash(browser);
    if (!ONLY || ONLY.includes('SourceViewer')) await testSourceViewer(browser);
    if (!ONLY || ONLY.includes('ShieldAndHttpsOnly')) await testShieldAndHttpsOnly(browser);
    if (!ONLY || ONLY.includes('BlockedList')) await testBlockedList(browser);
    if (!ONLY || ONLY.includes('FilterLists')) await testFilterLists(browser);
    if (!ONLY || ONLY.includes('Passwords')) await testPasswords(browser);
    if (!ONLY || ONLY.includes('Settings')) await testSettings(browser);
    if (!ONLY || ONLY.includes('AccountCard')) await testAccountCard(browser);
    if (!ONLY || ONLY.includes('SyncPage')) await testSyncPage(browser);
    if (!ONLY || ONLY.includes('SyncCard')) await testSyncCard(browser);
    if (ONLY && ONLY.includes('MeasureAccount')) await measureAccountCard(browser);
    if (!ONLY || ONLY.includes('LoginScript')) await testLoginScript(browser);
    if (!ONLY || ONLY.includes('MediaScript')) await testMediaScript(browser);
    if (!ONLY || ONLY.includes('Gpu')) await testGpu(browser);
    if (!ONLY || ONLY.includes('About')) await testAbout(browser);
    if (!ONLY || ONLY.includes('PaintedScript')) await testPaintedScript(browser);
    if (!ONLY || ONLY.includes('NoHints')) await testNoHintsScript(browser);
    if (!ONLY || ONLY.includes('LinkSweep')) await testLinkSweep(browser);
    if (!ONLY || ONLY.includes('WebStore')) await testWebStoreScript(browser);
    if (!ONLY || ONLY.includes('PermissionsPadding')) await testPermissionsPadding(browser);
    // 0.12.0: the AI agent, userscripts and macros have their own files; each exports
    // `(helpers) => ({ testName: async (browser) => … })` and runs here (ONLY=Name without 'test').
    for (const f of ['test-ai-panel.cjs', 'test-ai-settings.cjs', 'test-scripts.cjs']) {
      const file = path.join(__dirname, f);
      if (!fs.existsSync(file)) continue;
      const tests = require(file)({ openPage, check, sent, lastSent, emit, shot, acceptInPage, settleTabs, SHOTS, UI });
      for (const [name, fn] of Object.entries(tests)) {
        if (!ONLY || ONLY.includes(name.replace(/^test/, ''))) await fn(browser);
      }
    }
  } finally {
    await browser.close();
  }
  console.log(`\n${passes} passed, ${failures} failed`);
  process.exit(failures ? 1 : 0);
})().catch((e) => { console.error(e); process.exit(2); });
