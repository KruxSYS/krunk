// Krunk 0.16.0: "Session usage" in the ⋮ menu. Hooked into test-ui.cjs (ONLY=SessionUsage).
'use strict';

module.exports = function ({ openPage, check, sent, lastSent, emit, shot }) {
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

  return { testSessionUsage };
};
