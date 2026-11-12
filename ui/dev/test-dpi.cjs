// Krunk 0.15.4: the shell and the popups at Windows display scales from 100 % to 250 %, and in
// the small CSS viewports large scales produce (a 1920x1080 display at 175 % is 1097 x 617 CSS
// px, its work area 569 px high). Hooked into test-ui.cjs (ONLY=DpiPopup,DpiShell).
//
// What the host does with the popup's answer (where the window goes, flipping a context menu
// upwards, the work-area limit) is core's `layout::place_popup` with its unit tests; the real
// windows are checked on the VM by scripts/vm-dpi-shots.ps1.
'use strict';

module.exports = function ({ openPage, check, sent, lastSent, emit, shot }) {
  const SCALES = [1, 1.25, 1.5, 1.75, 2, 2.5];
  const MENU = { kind: 'menu', zoom: 1, canReopen: true, hasVideo: true };

  // The popup page in a window of `w` x `h` CSS px at device scale `scale`; `room`: the height
  // the host says the content can have at its anchor (the room under the ⋮ button, or the
  // whole work area for a window at the top of the screen), announced before the content.
  async function popup(browser, w, h, scale, content, room) {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: w, height: h }, scale);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    if (room) await emit(page, { event: 'popupRoom', max: room });
    await emit(page, { event: 'popup', content: content, seq: 7 });
    await page.waitForFunction(() => window.__host.sent.some((m) => m.cmd === 'popupShown'));
    const shown = await lastSent(page, 'popupShown');
    const state = await page.evaluate(() => {
      const root = document.getElementById('root');
      return { dense: root.classList.contains('dense'), scroll: root.classList.contains('scroll'), client: root.clientHeight, content: root.scrollHeight };
    });
    return { page, errors, shown, state };
  }
  // Is the element wholly inside the popup's viewport?
  const inView = (page, sel) => page.evaluate((s) => {
    const all = document.querySelectorAll(s);
    const r = all[all.length - 1].getBoundingClientRect();
    return r.top >= 0 && r.bottom <= window.innerHeight && r.height > 0;
  }, sel);

  async function testDpiPopup(browser) {
    console.log('0.15.4: popups at every scale and on low work areas');
    // 1. The same content has the same CSS height at every scale (the window is content x scale).
    const heights = [];
    for (const s of SCALES) {
      const { page, errors, shown, state } = await popup(browser, 280, 752, s, MENU, 752);
      heights.push(shown.height);
      check(!state.dense && !state.scroll, s * 100 + ' %: a menu that fits is neither compact nor scrolling');
      check(errors.length === 0, s * 100 + ' %: no errors (' + errors.join('; ') + ')');
      await page.context().close();
    }
    const full = heights[0];
    check(heights.every((h) => Math.abs(h - full) <= 1), 'the ⋮ menu asks for the same CSS height at every scale (' + heights.join(', ') + ')');

    // 2. A host that sends no room (and 100 % with room to spare): exactly as before.
    {
      const { page, shown, state } = await popup(browser, 280, 752, 1, MENU, 0);
      check(shown.height === full && !state.dense && !state.scroll, 'without a room announcement nothing changes (' + shown.height + ' = ' + full + ')');
      await page.context().close();
    }

    // 3. 1920x1080 at 175 %: 569 CSS px of work area, less than the menu. Compact rows fit.
    {
      const room = 569;
      const { page, shown, state } = await popup(browser, 280, room, 1.75, MENU, room);
      check(full > room, 'the menu (' + full + ' px) is higher than a 1080p work area at 175 % (' + room + ' px)');
      check(state.dense && !state.scroll && shown.height <= room, 'it becomes compact and fits without scrolling (' + shown.height + ' px)');
      // The host gives the window that height: every item is then on screen.
      await page.setViewportSize({ width: 280, height: Math.ceil(shown.height) });
      check(await inView(page, '#root .item'), 'the last item (Exit) is inside the window');
      await shot(page, 'dpi-menu-175-compact.png');
      await page.context().close();
    }

    // 4. Lower still (1280x720 at 150 %: 448 px): compact is not enough, it scrolls.
    {
      const room = 448;
      const { page, errors, shown, state } = await popup(browser, 280, room, 1.5, MENU, room);
      check(state.dense && state.scroll && shown.height > room, 'compact and scrolling; it reports its full height (' + shown.height + ' px) so the host gives it the whole work area');
      check(state.client <= room && state.content > state.client, 'the content scrolls inside the window (' + state.content + ' in ' + state.client + ' px)');
      check(!(await inView(page, '#root .item')), 'the last item starts out of view');
      // Keyboard: Tab walks the items (as before); the focused one is scrolled into view.
      const count = await page.evaluate(() => document.querySelectorAll('#root button:not([disabled])').length);
      for (let i = 1; i < count; i++) await page.keyboard.press('Tab');
      await page.waitForTimeout(50);
      const last = await page.evaluate(() => { const a = document.activeElement; const r = a.getBoundingClientRect(); return { text: a.textContent, ok: r.top >= 0 && r.bottom <= window.innerHeight }; });
      check(/Exit/.test(last.text) && last.ok, 'Tab to the last item (Exit): it is scrolled into view (' + JSON.stringify(last) + ')');
      // The wheel scrolls it too, and nothing scrolls sideways.
      await page.evaluate(() => { document.getElementById('root').scrollTop = 0; });
      await page.mouse.move(140, 200);
      await page.mouse.wheel(0, 2000);
      await page.waitForTimeout(100);
      check(await inView(page, '#root .item'), 'the wheel scrolls to the last item');
      check(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth && document.getElementById('root').scrollWidth <= document.getElementById('root').clientWidth), 'no sideways scrolling');
      check(errors.length === 0, 'no errors (' + errors.join('; ') + ')');
      await shot(page, 'dpi-menu-448-scroll.png');
      await page.context().close();
    }

    // 5. Content that grows later (fit) is squeezed by the same rule; content that shrinks
    //    again loses the classes.
    {
      const { page } = await popup(browser, 280, 448, 1.5, MENU, 448);
      await emit(page, { event: 'popupRoom', max: 900 });
      await emit(page, { event: 'popup', content: MENU, seq: 8 });
      await page.waitForFunction(() => window.__host.sent.filter((m) => m.cmd === 'popupShown').length === 2);
      const again = await page.evaluate(() => document.getElementById('root').className);
      check(again === '', 'the same menu on a higher work area is back to normal rows (class "' + again + '")');
      await page.context().close();
    }

    // 6. A bookmark folder is a list that scrolls under its button anyway: never squeezed
    //    (the host sends no room for it; even if it did, the rows keep their height).
    {
      const items = [];
      for (let i = 0; i < 40; i++) items.push({ id: 100 + i, title: 'Bookmark ' + i, url: 'https://example.com/' + i, isFolder: false, favicon: null });
      const folder = { kind: 'bookmarkFolder', id: 5, title: 'Many', items: items, back: false };
      const { page, state } = await popup(browser, 360, 448, 1.5, folder, 448);
      check(!state.dense && !state.scroll, 'a long bookmark folder keeps its rows (it scrolls by its own rule)');
      await page.context().close();
    }

    // 7. The tab menu (a short context menu) on the lowest realistic work area: untouched.
    {
      const tab = { kind: 'tabMenu', id: 1, pinned: false, muted: false, canCloseOthers: true, canCloseRight: true, canMove: true, audible: false };
      const { page, state } = await popup(browser, 240, 448, 2.5, tab, 448);
      check(!state.dense && !state.scroll, 'a menu that fits a 448 px work area stays as it is');
      await page.context().close();
    }
  }

  // Do two rectangles overlap by more than half a pixel?
  const overlapScript = () => {
    // #tabs reaches 10 px under its neighbours on purpose (room for the active tab's glow,
    // 0.15.2): what counts is its content box, where the tabs are.
    const boxes = (parent) => Array.from(parent.children)
      .filter((e) => { const cs = getComputedStyle(e); const r = e.getBoundingClientRect(); return cs.display !== 'none' && cs.visibility !== 'hidden' && r.width > 0 && r.height > 0 && cs.position !== 'absolute' && cs.position !== 'fixed'; })
      .map((e) => {
        const r = e.getBoundingClientRect(); const cs = getComputedStyle(e);
        const pad = e.id === 'tabs' ? [parseFloat(cs.paddingLeft), parseFloat(cs.paddingRight)] : [0, 0];
        return { id: e.id || e.className, l: r.left + pad[0], t: r.top, r: r.right - pad[1], b: r.bottom };
      });
    const out = [];
    for (const id of ['tabstrip', 'toolbar']) {
      const parent = document.getElementById(id);
      const pr = parent.getBoundingClientRect();
      const bs = boxes(parent);
      for (const b of bs) {
        if (b.l < pr.left - 0.5 || b.r > pr.right + 0.5 || b.t < pr.top - 0.5 || b.b > pr.bottom + 0.5) out.push(id + ': ' + b.id + ' leaves its row');
      }
      for (let i = 0; i < bs.length; i++) for (let j = i + 1; j < bs.length; j++) {
        const a = bs[i], b = bs[j];
        if (Math.min(a.r, b.r) - Math.max(a.l, b.l) > 0.5 && Math.min(a.b, b.b) - Math.max(a.t, b.t) > 0.5) out.push(id + ': ' + a.id + ' overlaps ' + b.id);
      }
    }
    if (document.documentElement.scrollWidth > window.innerWidth) out.push('the shell scrolls sideways');
    const addr = document.getElementById('address').getBoundingClientRect();
    const vis = (id) => { const e = document.getElementById(id); const r = e.getBoundingClientRect(); return r.width > 0 && r.right <= window.innerWidth + 0.5 && r.left >= -0.5; };
    const tb = document.getElementById('toolbar');
    return {
      problems: out, address: Math.round(addr.width),
      caption: Math.round(window.innerWidth - document.getElementById('win-close').getBoundingClientRect().right),
      tight: ['tight-1', 'tight-2', 'tight-3'].filter((c) => tb.classList.contains(c)).length,
      always: ['back', 'forward', 'reload', 'shield', 'menu', 'win-min', 'win-max', 'win-close', 'newtab'].every(vis),
      shown: ['ext-actions', 'zoom', 'star', 'downloads', 'vpn', 'agent'].filter(vis).join(' ') || 'none',
    };
  };

  async function testDpiShell(browser) {
    console.log('0.15.4: the shell at every scale, down to the smallest window');
    const tabs = [];
    for (let i = 1; i <= 6; i++) tabs.push({ id: i, title: 'Tab number ' + i + ' with a long title', url: 'https://example.com/' + i, favicon: null, loading: false, private: i === 6, sleeping: false });
    // CSS widths: a 1280-DIP window, 1080p at 175 % maximized, snapped halves, and the
    // minimum window (480 DIP).
    for (const [w, s] of [[1280, 1], [1280, 2.5], [1097, 1.75], [1024, 1.25], [960, 1.5], [853, 1.5], [640, 2], [548, 1.75], [480, 1], [480, 2.5]]) {
      const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: w, height: 104 }, s);
      await page.evaluate(() => { window.__host.autoSimulate = false; });
      await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', tabHoverPreview: true });
      await emit(page, { event: 'tabs', tabs: tabs, active: 2 });
      await page.evaluate(() => window.__host.navEvent({ tabId: 2, url: 'https://example.com/2', canGoBack: true, canGoForward: true, blocked: 12, zoom: 1.25 }));
      // Every optional button there is: AI, downloads, VPN, three extensions, the zoom chip.
      await emit(page, { event: 'agentButton', visible: true, open: false });
      await emit(page, { event: 'downloadsChanged', active: 1 });
      await emit(page, { event: 'vpn', enabled: true, status: { state: 'connected' } });
      await emit(page, { event: 'extActions', actions: [1, 2, 3].map((n) => ({ id: 'ext' + n, title: 'Extension ' + n, icon: null, badge: n === 2 ? '9' : '', badgeColor: null })) });
      await page.waitForSelector('.tab[data-id="6"]');
      await page.waitForTimeout(350);
      const r = await page.evaluate(overlapScript);
      const tag = w + ' CSS px at ' + s * 100 + ' %';
      check(r.problems.length === 0, tag + ': nothing in the tab row or the toolbar overlaps or leaves its row (' + r.problems.join('; ') + ')');
      check(r.caption === 0, tag + ': the close button ends at the window edge');
      check(r.address >= 100 || r.tight === 3, tag + ': the address field keeps a usable width, or every optional button is gone (' + r.address + ' px, ' + r.shown + ')');
      check(r.always, tag + ': Back, Forward, Reload, the shield and the menu are all there');
      if (w >= 1024) check(r.tight === 0, tag + ': nothing is hidden in a wide window (' + r.shown + ')');
      check(errors.length === 0, tag + ': no errors (' + errors.join('; ') + ')');
      if (w === 480 && s === 2.5) await shot(page, 'dpi-shell-480-250.png');
      if (w === 1097) await shot(page, 'dpi-shell-1097-175.png');
      await page.context().close();
    }
  }

  return { testDpiPopup, testDpiShell };
};
