// Krunk 0.16.0: "Session usage" in the ⋮ menu and the Home button (shell and Settings →
// Appearance). Hooked into test-ui.cjs (ONLY=SessionUsage,HomeButton,HomeSettings).
//
// Where Home goes is the host's business (Windows-only); core has its unit tests
// (settings::home_target, protocol's permissions).
'use strict';
const fs = require('fs');
const path = require('path');

module.exports = function ({ openPage, check, sent, lastSent, emit, shot }) {
  const APPEARANCE = { accent: '', toolbarColor: '', stripColor: '', backgroundColor: '', density: 'normal', uiFontPx: 13, roundedTabs: true,
    showDownloadsButton: true, showZoomChip: true, showExtensionsButton: true, showHomeButton: false };
  const TABS = [1, 2, 3].map((i) => ({ id: i, title: 'Tab ' + i, url: 'https://example.com/' + i, favicon: null, loading: false, private: false, sleeping: false }));
  const count = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).length;

  async function shell(browser, width, appearance) {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: width, height: 104 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', tabHoverPreview: true, appearance: appearance });
    await emit(page, { event: 'tabs', tabs: TABS, active: 2 });
    await page.evaluate(() => window.__host.navEvent({ tabId: 2, url: 'https://example.com/2', canGoBack: true, canGoForward: true, blocked: 3, zoom: 1 }));
    await page.waitForSelector('.tab[data-id="3"]');
    await page.waitForTimeout(300); // the strip's opening animation, fitToolbar's frame
    return { page, errors };
  }
  async function popup(browser, w, h, content) {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: w, height: h }, 1);
    await emit(page, { event: 'popup', content: content });
    await page.waitForSelector('#root > *');
    await page.waitForTimeout(40);
    return { page, errors };
  }

  // ---------- 1. Session usage ----------
  async function testSessionUsage(browser) {
    console.log('0.16.0: Session usage in the ⋮ menu');
    // (With the update item, as the bound always was: 670 before this row, test-ui's TabWindows.)
    const { page, errors } = await popup(browser, 280, 700, { kind: 'menu', zoom: 1, canReopen: true, hasVideo: true, update: '0.16.1' });
    const labels = await page.$$eval('.item .label', (ns) => ns.map((n) => n.textContent));
    const at = labels.indexOf('Session usage');
    check(at > 0 && labels[at - 1] === 'Cookies and site data' && labels[at + 1] === 'Extensions', '"Session usage" comes after "Cookies and site data" (' + labels.slice(at - 1, at + 2).join(' | ') + ')');
    check(labels.filter((l) => l === 'Session usage').length === 1, 'once');
    const fits = await page.evaluate(() => ({ bottom: document.getElementById('root').getBoundingClientRect().bottom, scroll: document.documentElement.scrollHeight }));
    check(fits.bottom <= 700 && fits.scroll <= 700, 'the menu with the update item fits its 280x700 bound (' + fits.bottom + ' px)');
    const resize = await lastSent(page, 'popupResize');
    check(resize && resize.height <= 700 && resize.height > 600, 'it asks the host for its own height (' + (resize && resize.height) + ' px)');
    await shot(page, '0160-menu-session-usage.png');
    await page.locator('.item', { hasText: 'Session usage' }).click();
    const open = await lastSent(page, 'openPage');
    check(open && open.page === 'sessions' && open.section === undefined, 'a click opens krunk://sessions (openPage{page:"sessions"})');
    check((await count(page, 'hidePopup')) === 1, 'and closes the menu');
    check(errors.length === 0, 'no errors (' + errors.join('; ') + ')');
    await page.context().close();

    // The shell asks for the menu with the same bound.
    const s = await shell(browser, 1100);
    await s.page.click('#menu');
    const sp = await lastSent(s.page, 'showPopup');
    check(sp && sp.popup === 'menu' && sp.rect.width === 280 && sp.rect.height === 700, 'the shell asks for the menu with 280x700 (' + JSON.stringify(sp && sp.rect) + ')');
    check(s.errors.length === 0, 'shell: no errors (' + s.errors.join('; ') + ')');
    await s.page.context().close();
  }

  // ---------- 2. Home button ----------
  const box = (page, id) => page.evaluate((i) => {
    const e = document.getElementById(i); const r = e.getBoundingClientRect();
    return { hidden: e.hidden, shown: getComputedStyle(e).display !== 'none' && r.width > 0, left: r.left, right: r.right, top: r.top, bottom: r.bottom };
  }, id);

  async function testHomeButton(browser) {
    console.log('0.16.0: the Home button in the toolbar');
    // Off by default: a host that sends no appearance, and one that sends the default.
    let { page, errors } = await shell(browser, 1100);
    check(!(await box(page, 'home')).shown, 'hidden before any appearance arrives');
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', appearance: APPEARANCE });
    await page.waitForTimeout(60);
    check(!(await box(page, 'home')).shown, 'hidden with showHomeButton off (the default)');
    const older = Object.assign({}, APPEARANCE);
    delete older.showHomeButton;
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', appearance: older });
    await page.waitForTimeout(60);
    check(!(await box(page, 'home')).shown, 'hidden when the host sends no such key');
    const before = await box(page, 'address-wrap');

    // Turned on: it appears at once (the same shellConfig path as the downloads button).
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', appearance: Object.assign({}, APPEARANCE, { showHomeButton: true }) });
    await page.waitForTimeout(80);
    const home = await box(page, 'home'), reload = await box(page, 'reload'), omni = await box(page, 'omnibox');
    check(home.shown && !home.hidden, 'shown when the setting is on, without a reload of the shell');
    check(await page.evaluate(() => document.getElementById('reload').nextElementSibling.id === 'home'), 'it is the element right after Reload');
    check(home.left >= reload.right - 0.5 && omni.left >= home.right - 0.5 && Math.abs(home.top - reload.top) < 0.5 && Math.abs((home.right - home.left) - (reload.right - reload.left)) < 0.5,
      'between Reload and the address bar, the same size as Reload (' + JSON.stringify(home) + ')');
    check((await box(page, 'address-wrap')).right - (await box(page, 'address-wrap')).left < before.right - before.left, 'the address field gave it the room');
    check((await page.getAttribute('#home', 'title')) === 'Home (Alt+Home)' && (await page.getAttribute('#home', 'aria-label')) === 'Home', 'tooltip "Home (Alt+Home)", labelled Home');
    check(await page.evaluate(() => { const u = document.querySelector('#home use'); const s = document.querySelector(u.getAttribute('href')); return !!s && s.querySelectorAll('path').length > 0 && getComputedStyle(document.querySelector('#home svg')).fill === 'none'; }),
      'a house icon from the sprite, drawn as strokes like the others');
    await shot(page, '0160-home-button.png');

    // Click = Alt+Home; Ctrl+click and middle-click = a new tab. The shell never names an address.
    await page.click('#home');
    let h = await lastSent(page, 'home');
    check(h && h.newTab === false && Object.keys(h).sort().join() === 'cmd,newTab' && (await count(page, 'home')) === 1, 'a click sends home{newTab:false} and nothing else (' + JSON.stringify(h) + ')');
    await page.click('#home', { modifiers: ['Control'] });
    h = await lastSent(page, 'home');
    check(h && h.newTab === true && (await count(page, 'home')) === 2, 'Ctrl+click sends home{newTab:true}');
    await page.click('#home', { button: 'middle' });
    h = await lastSent(page, 'home');
    check(h && h.newTab === true && (await count(page, 'home')) === 3, 'a middle-click sends home{newTab:true}, once');
    await page.click('#home', { button: 'right' });
    check((await count(page, 'home')) === 3 && (await count(page, 'showPopup')) === 0, 'a right-click does nothing');
    check(!(await sent(page)).some((m) => m.cmd === 'navigate' || m.cmd === 'openUrl' || m.cmd === 'newTab'), 'no address comes from the shell');

    // Dark theme: the icon follows the toolbar's text colour.
    await emit(page, { event: 'shellConfig', theme: 'dark', searchName: 'DuckDuckGo', appearance: Object.assign({}, APPEARANCE, { showHomeButton: true }) });
    await page.waitForTimeout(60);
    check(await page.evaluate(() => getComputedStyle(document.querySelector('#home svg')).stroke === getComputedStyle(document.querySelector('#reload svg')).stroke), 'dark: the same stroke colour as Reload');
    await shot(page, '0160-home-button-dark.png');

    // Turned off again: gone at once.
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', appearance: APPEARANCE });
    await page.waitForTimeout(80);
    check(!(await box(page, 'home')).shown, 'hidden again when the setting goes off');
    check(errors.length === 0, 'no errors (' + errors.join('; ') + ')');
    await page.context().close();

    // The narrow toolbar (0.15.4): Home is an optional button of the last step, with downloads,
    // VPN and AI; Back, Forward and Reload stay.
    for (const w of [1280, 640, 560, 520, 480, 360]) {
      ({ page, errors } = await shell(browser, w, Object.assign({}, APPEARANCE, { showHomeButton: true })));
      await emit(page, { event: 'agentButton', visible: true, open: false });
      await emit(page, { event: 'downloadsChanged', active: 1 });
      await emit(page, { event: 'vpn', enabled: true, showButton: true, status: { state: 'connected' } });
      await emit(page, { event: 'extActions', actions: [1, 2, 3].map((n) => ({ id: 'ext' + n, title: 'Extension ' + n, icon: null, badge: '', badgeColor: null })) });
      await page.waitForTimeout(350);
      const r = await page.evaluate(() => {
        const vis = (id) => { const e = document.getElementById(id); const b = e.getBoundingClientRect(); return getComputedStyle(e).display !== 'none' && b.width > 0 && b.left >= -0.5 && b.right <= window.innerWidth + 0.5; };
        const tb = document.getElementById('toolbar');
        const kids = Array.from(tb.children).filter((e) => getComputedStyle(e).display !== 'none' && e.getBoundingClientRect().width > 0).map((e) => ({ id: e.id, l: e.getBoundingClientRect().left, r: e.getBoundingClientRect().right }));
        const overlaps = [];
        for (let i = 0; i < kids.length; i++) for (let j = i + 1; j < kids.length; j++) if (Math.min(kids[i].r, kids[j].r) - Math.max(kids[i].l, kids[j].l) > 0.5) overlaps.push(kids[i].id + '/' + kids[j].id);
        return {
          tight: ['tight-1', 'tight-2', 'tight-3'].filter((c) => tb.classList.contains(c)).length,
          home: vis('home'), downloads: vis('downloads'), agent: vis('agent'), ext: vis('ext-actions'),
          always: ['back', 'forward', 'reload', 'shield', 'menu'].every(vis),
          address: Math.round(document.getElementById('address-wrap').getBoundingClientRect().width), overlaps: overlaps,
          sideways: document.documentElement.scrollWidth > window.innerWidth,
        };
      });
      const tag = w + ' px';
      check(r.always && r.overlaps.length === 0 && !r.sideways, tag + ': Back, Forward, Reload, the shield and the menu are there; nothing overlaps (' + r.overlaps.join(' ') + ')');
      if (w === 1280) check(r.tight === 0 && r.home && r.downloads && r.ext, tag + ': a wide window shows Home with everything else');
      else check(r.home === (r.tight < 3) && r.home === r.downloads && r.home === r.agent, tag + ': Home stays until the last step and goes with downloads and AI (step ' + r.tight + ', home ' + r.home + ')');
      // (360 px is narrower than a Krunk window gets: there the last step is certain.)
      if (w === 360) check(r.tight === 3 && !r.home, tag + ': with no room left Home is hidden, and Back, Forward and Reload are not (address ' + r.address + ' px)');
      if (r.tight > 0 && r.tight < 3) check(!r.ext && r.home, tag + ': the extension icons went before Home (step ' + r.tight + ')');
      check(errors.length === 0, tag + ': no errors (' + errors.join('; ') + ')');
      if (w === 480) await shot(page, '0160-home-narrow.png');
      await page.context().close();
    }
  }

  // The settings page with a host that validates the home page as core does (an http(s) URL or
  // empty).
  function homeSettingsMock(defaults) {
    const h = window.__host.handlers;
    const st = { settings: JSON.parse(JSON.stringify(defaults)), saves: 0 };
    window.__st = st;
    h.getSettings = () => ({ settings: st.settings, restartRequired: false, dnsPresets: [], wallpapers: [], userAgents: [], defaultUserAgent: 'Mozilla/5.0',
      about: { version: '0.16.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
    h.saveSettings = (m) => {
      const s = m.settings;
      if (s.homepage && !/^https?:\/\/[^\s]+$/.test(s.homepage)) { const e = new Error('Some settings are not valid'); e.data = [{ field: 'homepage', message: 'Must be an http:// or https:// URL' }]; throw e; }
      st.settings = s;
      st.saves++;
      return { settings: st.settings, restartRequired: false };
    };
    h.getAllowlist = () => [];
    h.bounceStatus = () => ({ watched: 0, cleanedTotal: 0, lastCleanupMs: 0 });
    h.permissionExceptions = () => [];
    h.importDetect = () => [];
    h.dnsStatus = () => ({ route: 'system' });
    h.getFilterStatus = () => ({ enabled: true, lastCheck: 0, lastError: null, busy: false, userRules: 0, lists: [] });
    h.getUpdateStatus = () => ({ checking: false, available: null, downloading: null });
    h.defaultBrowserStatus = () => ({ installed: true, isDefault: true });
    h.vpnState = () => ({ status: { state: 'stopped', error: null, sinceMs: 0, rxBytes: 0, txBytes: 0, tunnelIp: null, server: '' }, profiles: [], active: '', showButton: false });
  }

  async function testHomeSettings(browser) {
    console.log('0.16.0: Settings → Appearance: the Home button and its address');
    const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
    check(defaults.appearance.showHomeButton === false && defaults.homepage === 'https://duckduckgo.com/' && !('showHomeButton' in defaults),
      'defaults: appearance.showHomeButton false, the homepage unchanged, no top-level key');
    const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#appearance', { width: 1000, height: 1500 }, 1, homeSettingsMock, defaults);
    await page.waitForSelector('#style-card');
    await page.waitForTimeout(150);
    const saved = () => page.evaluate(() => ({ on: window.__st.settings.appearance.showHomeButton, homepage: window.__st.settings.homepage, saves: window.__st.saves }));
    const toggle = page.locator('[data-key="appearance.showHomeButton"]');
    check(await toggle.isVisible() && !(await toggle.isChecked()), 'Appearance has "Show the Home button", off by default');
    check(await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('.panel[data-tab=appearance] .row.setting'));
      const i = rows.findIndex((r) => r.querySelector('[data-key="appearance.showHomeButton"]'));
      const near = (r) => !!r && !!r.querySelector('[data-key="appearance.showDownloadsButton"], [data-key="appearance.showZoomChip"], [data-key="appearance.showExtensionsButton"]');
      return i > 0 && near(rows[i - 1]) && !!rows[i + 1].querySelector('#home-address[data-key="homepage"]') && near(rows[i + 2]);
    }), 'it sits with the other toolbar items, its home page field right under it');
    check(await page.inputValue('#home-address') === 'https://duckduckgo.com/', 'the field shows the homepage setting (' + (await page.inputValue('#home-address')) + ')');
    await shot(page.locator('.panel[data-tab=appearance] .card.group', { has: toggle }), '0160-settings-home.png');

    // The switch saves appearance.showHomeButton and nothing else changes.
    await toggle.check();
    await page.waitForFunction(() => window.__st.saves === 1);
    let s = await saved();
    check(s.on === true && s.homepage === 'https://duckduckgo.com/', 'turning it on saves appearance.showHomeButton = true; the homepage is untouched');
    const diff = await page.evaluate((d) => { const a = JSON.parse(JSON.stringify(window.__st.settings)); a.appearance.showHomeButton = false; return JSON.stringify(a) === JSON.stringify(d); }, defaults);
    check(diff, 'nothing else in the settings changed');

    // The field here and General's Homepage are one setting: typing in one shows in the other
    // and is what gets saved, whichever was edited.
    await page.fill('#home-address', 'https://start.example/');
    check(await page.inputValue('#homepage') === 'https://start.example/', 'typing in Appearance shows in General\'s Homepage field at once');
    await page.press('#home-address', 'Tab');
    await page.waitForFunction(() => window.__st.saves === 2);
    s = await saved();
    check(s.homepage === 'https://start.example/' && s.on === true, 'and saves homepage (' + s.homepage + ')');
    await page.click('#tabs a[data-tab="general"]');
    await page.waitForTimeout(40);
    check(await page.inputValue('#homepage') === 'https://start.example/', 'General → On startup shows the new home page');
    await page.fill('#homepage', 'https://other.example/home');
    check(await page.inputValue('#home-address') === 'https://other.example/home', 'typing in General shows in the Appearance field');
    await page.press('#homepage', 'Tab');
    await page.waitForFunction(() => window.__st.saves === 3);
    s = await saved();
    check(s.homepage === 'https://other.example/home', 'an edit in General is what is saved, not the other field\'s old text (' + s.homepage + ')');

    // Empty is allowed (Home then opens the new tab page); a bad address is refused at both fields.
    await page.fill('#homepage', '');
    await page.press('#homepage', 'Tab');
    await page.waitForFunction(() => window.__st.saves === 4);
    check((await saved()).homepage === '' && await page.inputValue('#home-address') === '', 'an empty home page is saved and shown in both');
    await page.click('#tabs a[data-tab="appearance"]');
    await page.waitForTimeout(40);
    await page.fill('#home-address', 'javascript:alert(1)');
    await page.press('#home-address', 'Tab');
    await page.waitForFunction(() => Array.from(document.querySelectorAll('.field-error[data-error-for="homepage"]')).every((p) => p.textContent));
    const errs = await page.evaluate(() => ({
      texts: Array.from(document.querySelectorAll('.field-error[data-error-for="homepage"]')).map((p) => p.textContent),
      invalid: Array.from(document.querySelectorAll('[data-key="homepage"]')).map((i) => i.classList.contains('invalid')),
      values: Array.from(document.querySelectorAll('[data-key="homepage"]')).map((i) => i.value),
    }));
    s = await saved();
    check(s.saves === 4 && s.homepage === '', 'a javascript: address is not saved');
    check(errs.texts.length === 2 && errs.texts.every((t) => /http/.test(t)) && errs.invalid.every(Boolean), 'the error shows at both fields (' + errs.texts.join(' | ') + ')');
    check(errs.values.every((v) => v === 'javascript:alert(1)'), 'what was typed stays to be fixed');
    await page.fill('#home-address', 'https://duckduckgo.com/');
    await page.press('#home-address', 'Tab');
    await page.waitForFunction(() => window.__st.saves === 5);
    // (The page clears them when the reply arrives, a moment after the host stored it.)
    const cleared = await page.waitForFunction(() => Array.from(document.querySelectorAll('.field-error[data-error-for="homepage"]')).every((p) => !p.textContent) && !document.querySelector('[data-key="homepage"].invalid'), null, { timeout: 2000 }).then(() => true, () => false);
    check(cleared, 'a good address clears the error at both');

    // Search finds both rows.
    for (const q of ['home button', 'toolbar', 'homepage']) {
      await page.fill('#filter', q);
      await page.waitForTimeout(80);
      check(await toggle.isVisible() && await page.locator('#home-address').isVisible(), 'search "' + q + '" finds the switch and its address field');
    }
    await page.fill('#filter', '');
    await page.waitForTimeout(80);

    // Turning it off again.
    await toggle.uncheck();
    await page.waitForFunction(() => window.__st.saves === 6);
    s = await saved();
    check(s.on === false && s.homepage === 'https://duckduckgo.com/', 'turning it off saves false and keeps the home page');
    check(external.length === 0, 'no external requests (' + external.join(', ') + ')');
    check(errors.filter((e) => !/Some settings are not valid/.test(e)).length === 0, 'no errors (' + errors.join('; ') + ')');
    await page.context().close();
  }

  return { testSessionUsage, testHomeButton, testHomeSettings };
};
