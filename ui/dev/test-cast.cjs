// Krunk 0.30.0: casting on the mock host: the cast picker (popup), the toolbar's cast button,
// the ⋮ menu's "Cast…" and Settings → Permissions → Casting. Hooked into test-ui.cjs
// (ONLY=CastPopup,CastButton,CastMenu,CastSettings). The host side is mocked: the popup must
// send only a device and a source (never a mode), and show what the host says.
'use strict';
const fs = require('fs');
const path = require('path');

const TV = { id: 'cast:1', name: 'Living Room TV', icon: 'cast', provider: 'cast', status: 'available', statusText: 'Available',
  tab: { mode: 'tab' }, video: { mode: 'video' } };
const SPEAKER = { id: 'cast:2', name: 'Kitchen speaker', icon: 'speaker', provider: 'cast', status: 'available', statusText: 'Available',
  tab: { mode: 'tab' }, video: { mode: 'tab', notice: "Kitchen speaker can't play this video on its own — casting the whole tab instead." } };
const OLD = { id: 'dial:3', name: 'Old TV', icon: 'generic', provider: 'dial', status: 'busy', statusText: 'YouTube', site: { mode: 'presentation' } };
const GONE = { id: 'cast:4', name: 'Bedroom', icon: 'cast', provider: 'cast', status: 'unavailable', statusText: 'Not available', tab: { mode: 'tab' } };

function view(extra) {
  return Object.assign({ tabId: 3, private: false, fromPage: false, site: '', offerVideo: true, source: 'video', devices: [TV, SPEAKER, OLD, GONE],
    state: { casting: false, route: '', sink: '', mode: null, connecting: false, media: {} }, searching: false, unavailable: false }, extra || {});
}

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot } = h;
  const defaults = () => JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));

  async function popupPage(browser, theme) {
    const p = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 340, height: 360 }, 1);
    await p.page.evaluate(() => {
      window.__host.autoSimulate = false;
      window.__host.handlers.castStart = (m) => { if (window.__refuse) throw new Error(window.__refuse); return null; };
      window.__host.handlers.castStop = () => null;
      window.__host.handlers.castMedia = () => null;
    });
    await emit(p.page, { event: 'theme', theme: theme });
    return p;
  }

  async function testCastPopup(browser) {
    console.log('popup: cast picker');
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await popupPage(browser, theme);
      // Before the engine answers.
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view({ searching: true, devices: [] })) });
      await page.waitForSelector('#cast-searching');
      check((await page.textContent('#cast-title')) === 'Cast', theme + ': title');
      await shot(page, 'cast-popup-searching-' + theme + '.png');
      // No devices.
      await emit(page, Object.assign({ event: 'cast' }, view({ devices: [] })));
      await page.waitForSelector('#cast-none');
      check((await page.textContent('#cast-none')).includes('allow it on private networks'), theme + ': no devices: the firewall hint');
      await shot(page, 'cast-popup-none-' + theme + '.png');
      // Devices: the video source preselected (a remotable video), unavailable ones disabled.
      await emit(page, Object.assign({ event: 'cast' }, view()));
      await page.waitForSelector('#cast-list');
      check(await page.locator('.cast-device').count() === 4, theme + ': four devices listed');
      check((await page.getAttribute('#cast-src-video', 'aria-checked')) === 'true', theme + ': "Video on this page" preselected');
      check(await page.locator('.cast-device.unavailable .cast-pick[disabled]').count() === 1, theme + ': an unavailable device is disabled');
      check((await page.textContent('.cast-device.busy .cast-dev-status')) === 'YouTube', theme + ': a busy device says what it plays');
      await shot(page, 'cast-popup-devices-' + theme + '.png');
      await page.click('.cast-device:nth-child(1) .cast-pick');
      await page.waitForTimeout(40);
      let start = await lastSent(page, 'castStart');
      check(start && start.sinkId === 'cast:1' && start.source === 'video' && !('mode' in start), theme + ': a click sends the device and the source, no mode');
      // The speaker can't play the video: the notice says the tab is cast instead.
      await page.click('.cast-device:nth-child(2) .cast-pick');
      await page.waitForTimeout(40);
      check((await page.textContent('#cast-notice')).includes('casting the whole tab instead'), theme + ': fallback notice');
      await shot(page, 'cast-popup-fallback-' + theme + '.png');
      // "This tab".
      await page.click('#cast-src-tab');
      await page.waitForTimeout(20);
      check(await page.locator('#cast-notice').count() === 0, theme + ': switching the source clears the notice');
      await page.click('.cast-device:nth-child(1) .cast-pick');
      await page.waitForTimeout(40);
      start = await lastSent(page, 'castStart');
      check(start.source === 'tab', theme + ': "This tab" sends source tab');
      // A refusal is shown.
      await page.evaluate(() => { window.__refuse = 'Casting could not start'; });
      await page.click('.cast-device:nth-child(1) .cast-pick');
      await page.waitForTimeout(60);
      check((await page.textContent('#cast-error')) === 'Casting could not start', theme + ': a refusal is shown');
      await page.evaluate(() => { window.__refuse = null; });
      // Casting: the session's controls and Stop.
      const castingTv = Object.assign({}, TV, { status: 'casting', statusText: 'Casting this tab', stop: 'r1' });
      await emit(page, Object.assign({ event: 'cast' }, view({ source: 'tab', devices: [castingTv, SPEAKER],
        state: { casting: true, route: 'r1', sink: 'Living Room TV', mode: 'video', connecting: false,
          media: { playing: true, canPlayPause: true, canSeek: true, canVolume: true, canMute: true, volume: 0.4, muted: false, time: 75, duration: 600 } } })));
      await page.waitForSelector('#cast-controls');
      check((await page.textContent('#cast-title')) === 'Casting to Living Room TV', theme + ': casting title');
      check((await page.textContent('#cast-time')) === '1:15 / 10:00', theme + ': position and length');
      check((await page.inputValue('#cast-volume')) === '40', theme + ': device volume');
      await shot(page, 'cast-popup-casting-' + theme + '.png');
      await page.click('#cast-play');
      await page.waitForTimeout(30);
      check((await lastSent(page, 'castMedia')).action === 'pause', theme + ': pause');
      await page.locator('#cast-seek').evaluate((n) => { n.value = '300'; n.dispatchEvent(new Event('change')); });
      await page.waitForTimeout(30);
      let m = await lastSent(page, 'castMedia');
      check(m.action === 'seek' && m.value === 300, theme + ': seek');
      await page.locator('#cast-volume').evaluate((n) => { n.value = '80'; n.dispatchEvent(new Event('change')); });
      await page.waitForTimeout(30);
      m = await lastSent(page, 'castMedia');
      check(m.action === 'volume' && Math.abs(m.value - 0.8) < 1e-9, theme + ': volume 0..1');
      await page.click('.cast-stop');
      await page.waitForTimeout(30);
      check((await lastSent(page, 'castStop')).routeId === 'r1', theme + ': Stop sends the route');
      // A page asked (its cast button): the site's own app is offered first.
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view({ fromPage: true, site: 'youtube.com', source: 'tab', devices: [Object.assign({}, TV, { site: { mode: 'presentation' } }), OLD] })) });
      await page.waitForSelector('#cast-src-site');
      check((await page.getAttribute('#cast-src-site', 'aria-checked')) === 'true' && (await page.textContent('#cast-src-site')) === 'youtube.com', theme + ': page request: the site\'s app first');
      check((await page.textContent('#cast-sub')) === 'youtube.com wants to cast', theme + ': page request: who asks');
      await shot(page, 'cast-popup-page-' + theme + '.png');
      // Private tab mark.
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view({ private: true })) });
      await page.waitForSelector('#cast-private');
      check((await page.textContent('#cast-private')).startsWith('Private tab'), theme + ': private tabs are marked');
      await shot(page, 'cast-popup-private-' + theme + '.png');
      // No video: only "This tab" (no source switch).
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view({ offerVideo: false, source: 'tab' })) });
      await page.waitForSelector('#cast-list');
      check(await page.locator('.cast-sources').count() === 0, theme + ': no video: no source switch');
      // Turned off (and not restarted yet).
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view({ unavailable: true, devices: [] })) });
      await page.waitForSelector('#cast-off');
      await page.click('#cast-settings');
      const op = await lastSent(page, 'openPage');
      check(op && op.page === 'settings' && op.section === 'casting', theme + ': "Casting settings" opens Settings at Casting');
      // Events for another tab are ignored.
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'cast' }, view()) });
      await page.waitForSelector('#cast-list');
      await emit(page, Object.assign({ event: 'cast' }, view({ tabId: 9, devices: [] })));
      await page.waitForTimeout(40);
      check(await page.locator('.cast-device').count() === 4, theme + ': another tab\'s event is ignored');
      const hides = (await sent(page)).filter((x) => x.cmd === 'hidePopup').length;
      await page.keyboard.press('Escape');
      check((await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hides + 1, theme + ': Esc closes');
      check(external.length === 0, theme + ': no network requests');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  async function testCastMenu(browser) {
    console.log('popup: ⋮ menu Cast…');
    const { page, errors } = await popupPage(browser, 'dark');
    const menu = { kind: 'menu', zoom: 1, canReopen: false, hasVideo: true, update: null, account: null };
    await emit(page, { event: 'popup', content: Object.assign({ cast: true }, menu) });
    await page.waitForSelector('.item');
    const item = page.locator('.item', { hasText: 'Cast…' });
    check(await item.count() === 1, 'the menu has "Cast…" while casting is on');
    await shot(page, 'cast-menu-dark.png');
    const hides = (await sent(page)).filter((x) => x.cmd === 'hidePopup').length;
    await item.click();
    await page.waitForTimeout(30);
    const c = await lastSent(page, 'command');
    check(c && c.command.command === 'cast', '"Cast…" sends the cast command');
    check((await sent(page)).filter((x) => x.cmd === 'hidePopup').length === hides, '…without closing the popup (the picker replaces the menu)');
    await emit(page, { event: 'popup', content: Object.assign({ cast: false }, menu) });
    await page.waitForTimeout(40);
    check(await page.locator('.item', { hasText: 'Cast…' }).count() === 0, 'no "Cast…" while casting is off');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testCastButton(browser) {
    console.log('shell: cast button');
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    check(await page.locator('#cast').isHidden(), 'hidden while nothing casts');
    await emit(page, { event: 'castButton', show: true, sink: 'Living Room TV', activeTab: true });
    await page.waitForTimeout(40);
    check(await page.locator('#cast').isVisible(), 'shown while a tab casts');
    check((await page.getAttribute('#cast', 'title')) === 'Casting to Living Room TV', 'its title names the device');
    await shot(page, 'cast-button.png');
    await page.click('#cast');
    await page.waitForTimeout(30);
    const op = await lastSent(page, 'showPopup');
    check(op && op.popup === 'cast' && op.rect.width === 340, 'a click opens the picker under it');
    await emit(page, { event: 'castButton', show: true, sink: 'Desk', activeTab: false });
    await page.waitForTimeout(30);
    check((await page.getAttribute('#cast', 'title')) === 'Casting to Desk (another tab)', 'another tab casting');
    await emit(page, { event: 'castButton', show: false, sink: '', activeTab: false });
    await page.waitForTimeout(30);
    check(await page.locator('#cast').isHidden(), 'hidden again when the session ends');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  function castSettingsMock(arg) {
    const hd = window.__host.handlers;
    const clone = (x) => JSON.parse(JSON.stringify(x));
    const st = { settings: clone(arg.defaults) };
    window.__castSt = st;
    hd.getSettings = () => ({ settings: st.settings, restartRequired: false, dnsPresets: [], wallpapers: [], userAgents: [], defaultUserAgent: 'Mozilla/5.0',
      about: { version: '0.30.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
    hd.saveSettings = (m) => { const restart = m.settings.casting !== arg.defaults.casting; st.settings = m.settings; return { settings: m.settings, restartRequired: restart }; };
    hd.getAllowlist = () => [];
    hd.bounceStatus = () => ({ watched: 0, cleanedTotal: 0, lastCleanupMs: 0 });
    hd.permissionExceptions = () => [];
    hd.importDetect = () => [];
    hd.dnsStatus = () => ({ route: 'system' });
    hd.getFilterStatus = () => ({ enabled: true, lastCheck: 0, lastError: null, busy: false, userRules: 0, lists: [] });
    hd.getUpdateStatus = () => ({ checking: false, available: null, downloading: null });
    hd.defaultBrowserStatus = () => ({ installed: true, isDefault: true });
    hd.vpnState = () => ({ status: { state: 'stopped' }, profiles: [], active: '', showButton: false, scope: 'everywhere' });
  }

  async function testCastSettings(browser) {
    console.log('settings: Casting');
    for (const theme of ['light', 'dark']) {
      const d = defaults();
      check(d.casting === true, theme + ': casting is on by default');
      const { page, errors } = await openPage(browser, 'https://krunk.invalid/settings.html#casting', { width: 1100, height: 760 }, 1, castSettingsMock, { defaults: d });
      await emit(page, { event: 'theme', theme: theme });
      await page.waitForSelector('#casting');
      check(await page.locator('#casting-card').isVisible(), theme + ': #casting opens Permissions at the Casting card');
      check(await page.isChecked('#casting'), theme + ': the switch is on');
      check((await page.textContent('#casting-card')).includes('restart'), theme + ': marked restart');
      await page.locator('#casting-card').scrollIntoViewIfNeeded();
      await shot(page, 'cast-settings-' + theme + '.png');
      await page.click('#casting');
      await page.waitForTimeout(400);
      const saved = await page.evaluate(() => window.__castSt.settings.casting);
      check(saved === false, theme + ': turning it off saves casting = false');
      check(await page.locator('#restart-banner').isVisible(), theme + ': the restart banner shows');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  return { testCastPopup, testCastMenu, testCastButton, testCastSettings };
};
