// Krunk 0.15.0, WP-A: tab hover cards (shell timing and the popup's card) and the HTTP sign-in
// badge on tabs. Hooked into test-ui.cjs (ONLY=TabHover,TabPreviewCard,AuthBadge). The host side
// (captures, the non-activating popup window, the per-tab sign-in state) is Windows-only; core
// has its unit tests (layout::tab_preview_rect, http_auth::AuthTabs, reveal::Reveals).
'use strict';

module.exports = function ({ openPage, check, sent, lastSent, emit, shot }) {
  const TABS = [
    { id: 1, title: 'Rust Programming Language', url: 'https://www.rust-lang.org/', favicon: null, loading: false, private: false, sleeping: false },
    { id: 2, title: 'Example News', url: 'https://news.example.com/', favicon: null, loading: false, private: false, sleeping: false },
    { id: 3, title: 'Docs', url: 'https://docs.rs/', favicon: null, loading: false, private: false, sleeping: false },
  ];

  async function shell(browser, theme) {
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 76 }, 1);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    await emit(page, { event: 'shellConfig', theme: theme || 'light', searchName: 'DuckDuckGo', tabHoverPreview: true });
    await emit(page, { event: 'tabs', tabs: TABS, active: 1 });
    await page.waitForSelector('.tab[data-id="3"]');
    await page.waitForTimeout(250); // the strip's opening animation
    return { page, errors };
  }
  const hovers = async (page) => (await sent(page)).filter((m) => m.cmd === 'tabHover');
  const centre = async (page, id) => { const b = await page.locator('.tab[data-id="' + id + '"]').boundingBox(); return [b.x + b.width / 2, b.y + b.height / 2]; };

  async function testTabHover(browser) {
    console.log('0.15.0: tab hover cards (shell)');
    const { page, errors } = await shell(browser);
    check((await page.locator('.tab[data-id="2"]').getAttribute('title')) === null, 'with hover cards the tabs have no tooltip');
    await page.mouse.move(...await centre(page, 2));
    await page.waitForTimeout(300);
    check((await hovers(page)).length === 0, 'nothing before the pointer rested 500 ms');
    await page.waitForTimeout(350);
    let h = await lastSent(page, 'tabHover');
    check(h && h.id === 2 && h.rect && h.rect.width > 50 && h.rect.height > 10, 'after 500 ms: tabHover for tab 2 with its rect (' + JSON.stringify(h) + ')');
    // On to the next tab: at once, without another delay.
    await page.mouse.move(...await centre(page, 3), { steps: 4 });
    await page.waitForTimeout(120);
    h = await lastSent(page, 'tabHover');
    const all = await hovers(page);
    check(h && h.id === 3 && !all.some((m) => m.id === undefined), 'moving to tab 3 updates the open card at once, without hiding it (' + JSON.stringify(all) + ')');
    // Leaving the strip hides it.
    await page.mouse.move(1000, 70);
    await page.waitForTimeout(150);
    h = await lastSent(page, 'tabHover');
    check(h && h.id === undefined, 'leaving the tabs hides the card (tabHover without id)');
    // A press hides it (and a later rest needs the delay again).
    await page.mouse.move(...await centre(page, 2));
    await page.waitForTimeout(600);
    check((await lastSent(page, 'tabHover')).id === 2, 'shown again after resting');
    await page.mouse.down();
    await page.mouse.up();
    await page.waitForTimeout(50);
    check((await lastSent(page, 'tabHover')).id === undefined, 'a press on the tab hides the card');
    // The wheel over the strip hides it too.
    await page.mouse.move(...await centre(page, 3));
    await page.waitForTimeout(600);
    await page.mouse.wheel(0, 100);
    await page.waitForTimeout(50);
    check((await lastSent(page, 'tabHover')).id === undefined, 'the wheel hides the card');
    // A short pass over a tab sends nothing.
    await page.mouse.move(1000, 70);
    await page.evaluate(() => { window.__host.sent.length = 0; });
    await page.mouse.move(...await centre(page, 1));
    await page.waitForTimeout(200);
    await page.mouse.move(1000, 70);
    await page.waitForTimeout(500);
    check((await hovers(page)).length === 0, 'passing over a tab for 200 ms shows nothing');
    // Setting off: no requests, tooltips back.
    await emit(page, { event: 'shellConfig', theme: 'light', searchName: 'DuckDuckGo', tabHoverPreview: false });
    await page.waitForTimeout(50);
    await page.mouse.move(...await centre(page, 2));
    await page.waitForTimeout(700);
    check((await hovers(page)).length === 0, 'setting off: no tabHover at all');
    check(((await page.locator('.tab[data-id="2"]').getAttribute('title')) || '').startsWith('Example News'), 'setting off: the tooltip is back');
    check(errors.length === 0, 'tab hover: no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  async function testAuthBadge(browser) {
    console.log('0.15.0: sign-in badge on tabs');
    for (const theme of ['light', 'dark']) {
      const { page, errors } = await shell(browser, theme);
      const tabs = TABS.map((t) => Object.assign({}, t, { authPending: t.id === 2 }));
      await emit(page, { event: 'tabs', tabs, active: 1 });
      await page.waitForTimeout(60);
      check((await page.locator('.tab[data-id="2"] .auth-badge').count()) === 1 && (await page.locator('.auth-badge').count()) === 1, theme + ': a key badge on the waiting tab only');
      const order = await page.$$eval('.tab[data-id="2"] > *', (n) => n.map((x) => x.className.baseVal !== undefined ? x.className.baseVal : x.className));
      check(order.indexOf('auth-badge') > order.indexOf('title'), theme + ': the badge sits after the title (' + order.join(',') + ')');
      const look = await page.evaluate(() => { const s = getComputedStyle(document.querySelector('.auth-badge')); return { bg: s.backgroundColor, fg: s.color, w: s.width }; });
      check(look.w === '18px' && look.bg !== look.fg, theme + ': badge styled (' + JSON.stringify(look) + ')');
      await shot(page, 'tabs-auth-badge-' + theme + '.png');
      // The update of one tab drops it again.
      await emit(page, { event: 'tabUpdate', tab: Object.assign({}, TABS[1], { authPending: false }) });
      await page.waitForTimeout(60);
      check((await page.locator('.auth-badge').count()) === 0, theme + ': gone with the tab update');
      // Clicking the badge activates its tab (the host shows its dialog then).
      await emit(page, { event: 'tabUpdate', tab: Object.assign({}, TABS[1], { authPending: true }) });
      await page.waitForTimeout(60);
      await page.click('.tab[data-id="2"] .auth-badge');
      check((await lastSent(page, 'activateTab') || {}).id === 2, theme + ': a click on the badge activates the tab');
      check(errors.length === 0, theme + ': no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  async function testTabPreviewCard(browser) {
    console.log('0.15.0: tab hover card (popup)');
    // 296 × 232 (core layout::tab_preview_rect with a picture).
    const { page, errors } = await openPage(browser, 'https://shell.krunk.invalid/popup.html', { width: 296, height: 232 }, 1);
    const jpeg = await page.evaluate(() => {
      const c = document.createElement('canvas'); c.width = 280; c.height = 158;
      const g = c.getContext('2d');
      g.fillStyle = '#f5f5f5'; g.fillRect(0, 0, 280, 158);
      g.fillStyle = '#2563eb'; g.fillRect(0, 0, 280, 26);
      g.fillStyle = '#333'; for (let i = 0; i < 6; i++) g.fillRect(16, 40 + i * 18, 120 + (i * 37) % 120, 8);
      return c.toDataURL('image/jpeg', 0.6);
    });
    const card = (over) => Object.assign({ kind: 'tabPreview', tabId: 2, title: 'Example News — <b>today</b>', host: 'news.example.com', url: 'https://news.example.com/', thumbnail: jpeg, active: false, private: false }, over);
    for (const theme of ['light', 'dark']) {
      await emit(page, { event: 'theme', theme });
      await emit(page, { event: 'popup', seq: theme === 'light' ? 1 : 2, content: card() });
      await page.waitForSelector('#tab-preview img');
      await page.waitForFunction((s) => window.__host.sent.some((m) => m.cmd === 'popupShown' && m.seq === s), theme === 'light' ? 1 : 2);
      const r = await page.evaluate(() => {
        const img = document.querySelector('#tab-preview img').getBoundingClientRect();
        const site = document.getElementById('tp-site').getBoundingClientRect();
        return { title: document.getElementById('tp-title').textContent, bold: document.querySelectorAll('#tab-preview b').length,
          site: document.getElementById('tp-site').textContent, img: [Math.round(img.width), Math.round(img.height)], bottom: Math.round(site.bottom),
          focus: document.activeElement === document.body || document.activeElement === null, overflowY: document.documentElement.scrollHeight <= window.innerHeight + 1 };
      });
      check(r.title === 'Example News — <b>today</b>' && r.bold === 0, theme + ': the title as text (' + r.title + ')');
      check(r.site === 'news.example.com', theme + ': the site under it');
      check(r.img[0] === 280 && r.img[1] === 158, theme + ': the picture is 280×158 (' + r.img + ')');
      check(r.bottom <= 232 && r.overflowY, theme + ': everything fits the host\'s 232 px (' + r.bottom + ')');
      check(r.focus, theme + ': nothing in the card takes the focus');
      await shot(page, 'popup-tab-preview-' + theme + '.png');
    }
    // The active tab: no picture; private: marked; a non-JPEG "picture" is not shown.
    await emit(page, { event: 'popup', content: card({ active: true, thumbnail: null }) });
    await page.waitForTimeout(50);
    check((await page.locator('#tab-preview img').count()) === 0 && (await page.locator('#tab-preview.active').count()) === 1, 'active tab: title and site only');
    await shot(page, 'popup-tab-preview-active.png');
    await emit(page, { event: 'popup', content: card({ private: true, thumbnail: 'javascript:alert(1)' }) });
    await page.waitForTimeout(50);
    check((await page.locator('#tab-preview img').count()) === 0, 'a thumbnail that is not a JPEG data URL is ignored');
    check((await page.textContent('#tp-site')) === 'Private · news.example.com', 'private tab marked');
    await emit(page, { event: 'popup', content: card({ host: '', url: 'krunk://settings', thumbnail: null }) });
    await page.waitForTimeout(50);
    check((await page.textContent('#tp-site')) === 'krunk://settings', 'a page without a site shows its address');
    // Nothing the card sends besides popupShown.
    const other = (await sent(page)).filter((m) => !['ready', 'popupShown', 'popupResize'].includes(m.cmd));
    check(other.length === 0, 'the card sends no commands (' + JSON.stringify(other) + ')');
    check(errors.length === 0, 'card: no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  return { testTabHover, testAuthBadge, testTabPreviewCard };
};
