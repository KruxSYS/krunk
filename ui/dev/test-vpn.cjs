// Krunk 0.14.0: the VPN on the mock host: the Settings card (Privacy & security → VPN), the
// toolbar button and its popup. Hooked into test-ui.cjs (ONLY=VpnSettings,VpnButton,VpnPopup;
// ONLY=MeasureVpn prints the VM coordinates). The host side is mocked here: the pages must send
// the right commands, never keep a password, and show what the host says.
'use strict';
const fs = require('fs');
const path = require('path');

const WG = { id: '0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a', name: 'Home WireGuard', kind: 'wireguard', server: 'vpn.example.org:51820', needsCredentials: false, hasCredentials: false };
const OV = { id: '0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b', name: 'Office', kind: 'openvpn', server: 'office.example.net:1194', needsCredentials: true, hasCredentials: false };
const STOPPED = { state: 'stopped', error: null, sinceMs: 0, rxBytes: 0, txBytes: 0, tunnelIp: null, server: '' };

// The settings page's host, cut down, plus the vpn* commands (a small simulated VPN).
function vpnSettingsMock(arg) {
  const h = window.__host.handlers;
  const clone = (x) => JSON.parse(JSON.stringify(x));
  const st = { settings: clone(arg.defaults), view: clone(arg.view), imports: [], failNext: null };
  window.__vpnSt = st;
  h.getSettings = () => ({ settings: st.settings, restartRequired: false, dnsPresets: [], wallpapers: [], userAgents: [], defaultUserAgent: 'Mozilla/5.0',
    about: { version: '0.14.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
  h.saveSettings = (m) => {
    const s = m.settings;
    s.vpn.activeProfile = st.settings.vpn.activeProfile;
    const errs = [];
    if (s.vpn.autoConnect && !s.vpn.activeProfile) errs.push({ field: 'vpn.autoConnect', message: 'Connect to a profile once first: Krunk connects to the one used last' });
    if (errs.length) { const e = new Error('Some settings are not valid'); e.data = errs; throw e; }
    st.settings = s;
    st.view.showButton = s.vpn.showButton;
    return { settings: s, restartRequired: false };
  };
  h.getAllowlist = () => [];
  h.bounceStatus = () => ({ watched: 0, cleanedTotal: 0, lastCleanupMs: 0 });
  h.permissionExceptions = () => [];
  h.importDetect = () => [];
  h.dnsStatus = () => ({ route: 'system' });
  h.getFilterStatus = () => ({ enabled: true, lastCheck: 0, lastError: null, busy: false, userRules: 0, lists: [] });
  h.getUpdateStatus = () => ({ checking: false, available: null, downloading: null });
  h.defaultBrowserStatus = () => ({ installed: true, isDefault: true });
  // ---- VPN ----
  const view = () => clone(st.view);
  const find = (id) => st.view.profiles.find((p) => p.id === id);
  h.vpnState = view;
  h.vpnImport = () => {
    if (st.failNext) { const e = st.failNext; st.failNext = null; throw new Error(e); }
    const p = { id: '0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c', name: 'Imported', kind: 'wireguard', server: '203.0.113.5:51820', needsCredentials: false, hasCredentials: false };
    st.view.profiles.push(p);
    return { profile: p, warnings: ['PostUp is ignored: Krunk never runs scripts'] };
  };
  h.vpnRename = (m) => { const p = find(m.id); if (!p) throw new Error('This profile is gone'); p.name = m.name; return view(); };
  h.vpnRemove = (m) => {
    st.view.profiles = st.view.profiles.filter((p) => p.id !== m.id);
    if (st.view.active === m.id) { st.view.active = ''; st.view.status = Object.assign({}, st.view.status, { state: 'stopped' }); }
    return view();
  };
  h.vpnCredentials = (m) => {
    st.creds = { id: m.id, username: m.username, password: m.password };
    find(m.id).hasCredentials = true;
    return view();
  };
  h.vpnConnect = (m) => {
    const p = find(m.id);
    if (p.needsCredentials && !p.hasCredentials) throw new Error('Enter the user name and password for this profile first (Settings → VPN)');
    st.view.active = m.id;
    st.settings.vpn.activeProfile = m.id;
    st.view.status = { state: 'connecting', error: null, sinceMs: Date.now(), rxBytes: 0, txBytes: 0, tunnelIp: null, server: p.server };
    return view();
  };
  h.vpnDisconnect = () => { st.view.status = { state: 'stopped', error: null, sinceMs: Date.now(), rxBytes: 0, txBytes: 0, tunnelIp: null, server: '' }; return view(); };
}

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot, acceptInPage } = h;
  const defaults = () => JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
  const baseView = (extra) => Object.assign({ status: STOPPED, profiles: [WG, OV], active: '', showButton: false }, extra || {});

  async function testVpnSettings(browser) {
    console.log('settings: VPN card');
    for (const theme of ['light', 'dark']) {
      const d = defaults();
      check(d.vpn && d.vpn.showButton === false && d.vpn.autoConnect === false && d.vpn.activeProfile === '', theme + ': the defaults have the VPN off (button hidden, no auto-connect)');
      const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#vpn', { width: 1000, height: 800 }, 1, vpnSettingsMock, { defaults: d, view: baseView() });
      await page.waitForSelector('#vpn-profiles .vpn-profile');
      await emit(page, { event: 'theme', theme: theme });
      await page.waitForTimeout(150);
      check(await page.locator('.panel.active[data-tab=privacy]').count() === 1, theme + ': #vpn opens Privacy & security');
      const card = await page.locator('#vpn-card').boundingBox();
      check(card && card.y >= -1 && card.y < 400, theme + ': and scrolls to the VPN card (y ' + (card && Math.round(card.y)) + ')');
      check((await sent(page)).some((m) => m.cmd === 'vpnState'), theme + ': the card asks for the state');
      check((await page.textContent('#vpn-explain')).includes('Other apps on this computer are not affected') && (await page.textContent('#vpn-explain')).includes('connections stop'), theme + ': the explanation: Krunk only, fail-closed');
      check(!(await page.isChecked('[data-key="vpn.showButton"]')) && !(await page.isChecked('[data-key="vpn.autoConnect"]')), theme + ': button and auto-connect off');
      const rows = page.locator('#vpn-profiles .vpn-profile');
      check(await rows.count() === 2, theme + ': both profiles listed');
      check((await rows.nth(0).textContent()).includes('Home WireGuard') && (await rows.nth(0).textContent()).includes('WireGuard · vpn.example.org:51820'), theme + ': name, type and server');
      check((await rows.nth(1).textContent()).includes('needs a user name and password') && await rows.nth(1).locator('.vpn-connect').isDisabled(), theme + ': OpenVPN without a password: Connect is off until one is entered');
      await shot(page, 'vpn-card-' + theme + '.png');

      // The toolbar button setting.
      await page.click('[data-key="vpn.showButton"]');
      await page.waitForTimeout(60);
      const saved = await lastSent(page, 'saveSettings');
      check(saved && saved.settings.vpn.showButton === true, theme + ': "Show the VPN button" is saved');
      // Auto-connect without a profile used last: the host refuses, the error is shown.
      await page.click('[data-key="vpn.autoConnect"]');
      await page.waitForTimeout(80);
      check((await page.textContent('[data-error-for="vpn.autoConnect"]')).includes('Connect to a profile once first'), theme + ': auto-connect needs a profile: the error is shown');

      // Credentials: sent once, the field is emptied, never shown again.
      await rows.nth(1).locator('.vpn-creds').click();
      await page.fill('.vpn-form .vpn-user', 'alice');
      await page.fill('.vpn-form .vpn-pass', 'pw-SECRET-1');
      await page.click('.vpn-form button[type=submit]');
      await page.waitForTimeout(80);
      const cr = await lastSent(page, 'vpnCredentials');
      check(cr && cr.id === OV.id && cr.username === 'alice' && cr.password === 'pw-SECRET-1', theme + ': vpnCredentials with the id, user name and password');
      check(await page.locator('.vpn-form').count() === 0 && !(await page.content()).includes('pw-SECRET-1'), theme + ': the form closes and the password is nowhere in the page');
      check((await rows.nth(1).textContent()).includes('user name and password saved') && !(await rows.nth(1).locator('.vpn-connect').isDisabled()), theme + ': saved: Connect is on');

      // Connect, then the host's events.
      await rows.nth(0).locator('.vpn-connect').click();
      await page.waitForTimeout(80);
      check((await lastSent(page, 'vpnConnect')).id === WG.id, theme + ': Connect sends vpnConnect with the profile');
      check((await rows.nth(0).textContent()).includes('Connecting') && await rows.nth(0).locator('.vpn-disconnect').count() === 1, theme + ': connecting: a badge and Disconnect');
      await emit(page, { event: 'vpn', status: { state: 'connected', error: null, sinceMs: Date.now() - 65000, rxBytes: 3 * 1024 * 1024, txBytes: 2048, tunnelIp: '10.77.0.2', server: WG.server }, profiles: [WG, Object.assign({}, OV, { hasCredentials: true })], active: WG.id, showButton: true });
      await page.waitForTimeout(60);
      const line = await page.textContent('#vpn-status');
      check(line.includes('Connected · Home WireGuard · 10.77.0.2') && line.includes('3.0 MB received'), theme + ': the status line (' + line + ')');
      check((await page.textContent('#vpn-auto-desc')).includes('(Home WireGuard)'), theme + ': auto-connect names the profile used last');
      await shot(page, 'vpn-card-connected-' + theme + '.png');
      await emit(page, { event: 'vpn', status: { state: 'failed', error: 'The username or password was refused', sinceMs: Date.now(), rxBytes: 0, txBytes: 0, tunnelIp: null, server: WG.server }, profiles: [WG, OV], active: WG.id, showButton: true });
      await page.waitForTimeout(60);
      check((await page.textContent('#vpn-status')).includes('refused') && (await page.textContent('#vpn-status')).includes('connections are stopped'), theme + ': failed: the error and that connections are stopped');
      await rows.nth(0).locator('.vpn-disconnect').click();
      await page.waitForTimeout(60);
      check((await lastSent(page, 'vpnDisconnect')) !== undefined, theme + ': Disconnect sends vpnDisconnect');

      // Rename and remove.
      await rows.nth(0).locator('.vpn-rename').click();
      await page.fill('.vpn-form .vpn-name-input', 'Home');
      await page.press('.vpn-form .vpn-name-input', 'Enter');
      await page.waitForTimeout(80);
      check((await lastSent(page, 'vpnRename')).name === 'Home' && (await rows.nth(0).textContent()).startsWith('Home'), theme + ': Rename');
      await rows.nth(1).locator('.vpn-remove').click();
      await acceptInPage(page);
      await page.waitForTimeout(80);
      check((await lastSent(page, 'vpnRemove')).id === OV.id && await rows.count() === 1, theme + ': Remove asks first, then vpnRemove');

      // Import: the host opens the dialog; the page only shows the answer.
      await page.click('#vpn-import');
      await page.waitForTimeout(120);
      const imp = await lastSent(page, 'vpnImport');
      check(imp && Object.keys(imp).sort().join(',') === 'cmd,reqId', theme + ': vpnImport carries nothing from the page');
      check(await rows.count() === 2 && (await page.textContent('#vpn-warnings')).includes('PostUp is ignored'), theme + ': the imported profile and its warnings');
      await page.evaluate(() => { window.__vpnSt.failNext = 'This is not a WireGuard or OpenVPN configuration file'; });
      await page.click('#vpn-import');
      await page.waitForTimeout(80);
      check((await page.textContent('#vpn-error')).includes('not a WireGuard or OpenVPN'), theme + ': an import error is shown');

      // Keyboard: every control is a real button or input in the tab order.
      const tabbable = await page.evaluate(() => Array.from(document.querySelectorAll('#vpn-card button, #vpn-card input')).every((n) => n.tabIndex >= 0));
      check(tabbable, theme + ': every VPN control is keyboard reachable');
      // Search finds the card.
      await page.fill('#filter', 'wireguard');
      await page.waitForTimeout(80);
      check(await page.isVisible('#vpn-profiles-row'), theme + ': settings search finds the VPN');
      check(external.length === 0, theme + ': no network requests from the page (' + external.join(', ') + ')');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  async function testVpnButton(browser) {
    console.log('shell: VPN button');
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await page.waitForTimeout(100);
    const b = page.locator('#vpn');
    check(await b.isHidden(), 'hidden before the host says anything');
    check(await page.evaluate(() => document.getElementById('vpn').nextElementSibling.id === 'shield'), 'right before the shield');
    await emit(page, Object.assign({ event: 'vpn' }, baseView({ showButton: false })));
    await page.waitForTimeout(40);
    check(await b.isHidden(), 'hidden while "Show the VPN button" is off');
    const states = [
      ['stopped', 'off', 'VPN off'],
      ['connecting', 'connecting', 'VPN connecting (Home WireGuard)'],
      ['reconnecting', 'connecting', 'VPN reconnecting (Home WireGuard)'],
      ['connected', 'on', 'VPN connected (Home WireGuard)'],
      ['failed', 'error', 'VPN error: connections are stopped (Home WireGuard)'],
    ];
    for (const [state, cls, label] of states) {
      await emit(page, Object.assign({ event: 'vpn' }, baseView({ showButton: true, active: WG.id, status: Object.assign({}, STOPPED, { state: state }) })));
      await page.waitForTimeout(40);
      const c = await b.getAttribute('class');
      const others = ['off', 'connecting', 'on', 'error'].filter((x) => x !== cls);
      check(await b.isVisible() && c.split(' ').includes(cls) && !others.some((o) => c.split(' ').includes(o)) && (await b.getAttribute('aria-label')) === label, state + ': class .' + cls + ', "' + label + '"');
      await shot(page, 'vpn-button-' + state + '.png');
    }
    await b.click();
    await page.waitForTimeout(40);
    const open = await lastSent(page, 'showPopup');
    check(open && open.popup === 'vpn' && open.rect.width === 320 && open.rect.height === 330, 'a click opens the VPN popup (320 × 330)');
    check((await b.getAttribute('class')).includes('open') && (await b.getAttribute('aria-expanded')) === 'true', 'the button shows it is open');
    await b.click();
    await page.waitForTimeout(40);
    check((await lastSent(page, 'hidePopup')) !== undefined && (await b.getAttribute('aria-expanded')) === 'false', 'a second click closes it');
    await b.focus();
    await page.keyboard.press('Enter');
    await page.waitForTimeout(40);
    check((await sent(page)).filter((m) => m.cmd === 'showPopup' && m.popup === 'vpn').length === 2, 'Enter on the focused button opens it too');
    await emit(page, Object.assign({ event: 'vpn' }, baseView({ showButton: false })));
    await page.waitForTimeout(40);
    check(await b.isHidden(), 'turned off again: hidden');
    for (const theme of ['light', 'dark']) {
      await emit(page, { event: 'theme', theme: theme });
      await emit(page, Object.assign({ event: 'vpn' }, baseView({ showButton: true, active: WG.id, status: Object.assign({}, STOPPED, { state: 'connected' }) })));
      await page.waitForTimeout(60);
      const color = await page.evaluate(() => getComputedStyle(document.querySelector('#vpn .icon')).color);
      check(color === (theme === 'dark' ? 'rgb(74, 222, 128)' : 'rgb(22, 163, 74)'), theme + ': connected is green (' + color + ')');
      await shot(page, 'vpn-button-connected-' + theme + '.png');
    }
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testVpnPopup(browser) {
    console.log('popup: VPN');
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 320, height: 330 }, 1);
      await page.evaluate(() => {
        window.__host.autoSimulate = false;
        window.__vpnReplies = [];
        window.__host.handlers.vpnConnect = (m) => { if (window.__refuse) throw new Error(window.__refuse); return {}; };
        window.__host.handlers.vpnDisconnect = () => ({});
      });
      await emit(page, { event: 'theme', theme: theme });
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'vpn' }, baseView({ active: WG.id, showButton: true })) });
      await page.waitForSelector('#vpn-toggle');
      check((await page.textContent('#vpn-title')) === 'VPN is off' && (await page.textContent('#vpn-toggle')) === 'Connect', theme + ': off: Connect');
      check(await page.evaluate(() => document.activeElement && document.activeElement.tagName !== 'BODY'), theme + ': the focus is in the popup');
      check((await page.inputValue('#vpn-profile')) === WG.id, theme + ': the profile used last is picked');
      await shot(page, 'vpn-popup-off-' + theme + '.png');
      await page.click('#vpn-toggle');
      await page.waitForTimeout(40);
      check((await lastSent(page, 'vpnConnect')).id === WG.id, theme + ': Connect sends vpnConnect');
      // The host's events update the panel in place.
      const connected = baseView({ active: WG.id, showButton: true, status: { state: 'connected', error: null, sinceMs: Date.now() - 3723000, rxBytes: 1536, txBytes: 10, tunnelIp: '10.77.0.2', server: WG.server } });
      await emit(page, Object.assign({ event: 'vpn' }, connected));
      await page.waitForTimeout(60);
      check((await page.textContent('#vpn-title')) === 'VPN connected' && (await page.textContent('#vpn-toggle')) === 'Disconnect', theme + ': connected: Disconnect');
      check((await page.textContent('#vpn-uptime')).startsWith('1:02:0') && (await page.textContent('#vpn-ip')) === '10.77.0.2' && (await page.textContent('#vpn-rx')) === '1.5 KB' && (await page.textContent('#vpn-tx')) === '10 B', theme + ': uptime, address and bytes');
      check((await page.textContent('#vpn-sub')).includes('Home WireGuard · vpn.example.org:51820'), theme + ': the profile and server');
      await shot(page, 'vpn-popup-connected-' + theme + '.png');
      await page.selectOption('#vpn-profile', OV.id);
      await page.waitForTimeout(40);
      check((await page.textContent('#vpn-connect')) === 'Connect to this one' && (await page.textContent('body')).includes('needs a user name and password'), theme + ': another profile picked: switch, and its missing password noted');
      await emit(page, Object.assign({ event: 'vpn' }, connected));
      await page.waitForTimeout(40);
      check((await page.inputValue('#vpn-profile')) === OV.id, theme + ': a counter update keeps the pick');
      await page.evaluate(() => { window.__refuse = 'Enter the user name and password for this profile first (Settings → VPN)'; });
      await page.click('#vpn-connect');
      await page.waitForTimeout(60);
      check((await page.textContent('#vpn-refused')).includes('Enter the user name and password'), theme + ': a refusal is shown in the popup');
      await page.click('#vpn-toggle');
      await page.waitForTimeout(40);
      check((await lastSent(page, 'vpnDisconnect')) !== undefined, theme + ': Disconnect sends vpnDisconnect');
      await emit(page, Object.assign({ event: 'vpn' }, baseView({ active: WG.id, showButton: true, status: { state: 'failed', error: 'The server did not answer', sinceMs: Date.now(), rxBytes: 0, txBytes: 0, tunnelIp: null, server: WG.server } })));
      await page.waitForTimeout(40);
      check((await page.textContent('#vpn-error')) === 'The server did not answer' && (await page.textContent('#vpn-connect')) === 'Try again' && (await page.textContent('#vpn-toggle')) === 'Disconnect', theme + ': failed: the error, Try again and Disconnect');
      check((await page.textContent('body')).includes("connections are stopped"), theme + ': failed: says connections are stopped');
      await shot(page, 'vpn-popup-failed-' + theme + '.png');
      await page.click('#vpn-settings');
      const op = await lastSent(page, 'openPage');
      check(op && op.page === 'settings' && op.section === 'vpn' && (await lastSent(page, 'hidePopup')) !== undefined, theme + ': "VPN settings" opens Settings at the VPN and closes');
      await emit(page, { event: 'popup', content: Object.assign({ kind: 'vpn' }, baseView({ profiles: [] })) });
      await page.waitForTimeout(40);
      check((await page.textContent('body')).includes('No VPN profiles yet') && await page.locator('#vpn-toggle').count() === 0, theme + ': no profiles: says where to import');
      const hides = (await sent(page)).filter((m) => m.cmd === 'hidePopup').length;
      await page.keyboard.press('Escape');
      check((await sent(page)).filter((m) => m.cmd === 'hidePopup').length === hides + 1, theme + ': Esc closes');
      check(external.length === 0, theme + ': no network requests');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  // For the VM scripts (ONLY=MeasureVpn): centres (CSS px) at the VM's window (1280 × 760): the
  // Settings page (1280 × 656 under the shell and the bookmarks bar; 684 without the bar), the
  // toolbar (shell 1280 wide, no other optional buttons) and the popup (its own viewport, and
  // where that is in the shell).
  async function measureVpn(browser) {
    for (const hgt of [656, 684]) {
      const { page } = await openPage(browser, 'https://krunk.invalid/settings.html#vpn', { width: 1280, height: hgt }, 1, vpnSettingsMock, { defaults: defaults(), view: baseView() });
      await page.waitForSelector('#vpn-profiles .vpn-profile .vpn-connect');
      await page.waitForTimeout(150);
      const at = async (sel) => { const b = await page.locator(sel).first().boundingBox(); return b && [Math.round(b.x + b.width / 2), Math.round(b.y + b.height / 2)]; };
      console.log(JSON.stringify({ page: [1280, hgt], showButton: await at('[data-key="vpn.showButton"]'), autoConnect: await at('[data-key="vpn.autoConnect"]'),
        importFile: await at('#vpn-import'), firstConnect: await at('#vpn-profiles .vpn-profile .vpn-connect') }));
      await page.context().close();
    }
    const { page } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1280, height: 76 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, Object.assign({ event: 'vpn' }, baseView({ showButton: true })));
    await page.waitForTimeout(60);
    const b = await page.locator('#vpn').boundingBox();
    await page.click('#vpn');
    await page.waitForTimeout(40);
    const rect = (await lastSent(page, 'showPopup')).rect;
    await page.context().close();
    const p = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 320, height: 330 }, 1);
    await p.page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(p.page, { event: 'popup', content: Object.assign({ kind: 'vpn' }, baseView({ active: WG.id, showButton: true })) });
    await p.page.waitForSelector('#vpn-toggle');
    const centre = async () => { const t = await p.page.locator('#vpn-toggle').boundingBox(); return [Math.round(t.x + t.width / 2), Math.round(t.y + t.height / 2)]; };
    const local = await centre();
    // Connected: the counters card pushes Disconnect down.
    await emit(p.page, Object.assign({ event: 'vpn' }, baseView({ active: WG.id, showButton: true, status: { state: 'connected', error: null, sinceMs: Date.now(), rxBytes: 1, txBytes: 1, tunnelIp: '10.77.0.2', server: WG.server } })));
    await p.page.waitForTimeout(60);
    const on = await centre();
    const inShell = (c) => [Math.round(rect.x + c[0]), Math.round(rect.y + c[1])];
    console.log(JSON.stringify({ shell: [1280, 76], vpnButton: [Math.round(b.x + b.width / 2), Math.round(b.y + b.height / 2)], popupRect: rect,
      connectLocal: local, connectInShell: inShell(local), disconnectLocal: on, disconnectInShell: inShell(on) }));
    await p.page.context().close();
  }

  // `MeasureVpn` (no `test` prefix) runs only when ONLY names it.
  return { testVpnSettings, testVpnButton, testVpnPopup, MeasureVpn: measureVpn };
};
