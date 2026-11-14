// Krunk 0.16.0 UI tests: a private tab's title reads on every tab strip style. The shell is
// drawn with what the host sends for each look (ui/dev/private-tab-colors.json, written by core:
// color::tests::private_tab_contrast_table_is_current), and the contrast is measured from the
// pixels: the title's computed colour against a screenshot of what is behind it. Hooked into
// test-ui.cjs:
//   ONLY=PrivateTabContrast,PrivateTabColours node ui/dev/test-ui.cjs [shots]
'use strict';
const fs = require('fs');
const path = require('path');

// ---- colour helpers (WCAG 2; only this test does contrast maths, the shell applies variables) ----
const hex = (s) => { const v = parseInt(s.slice(1), 16); return [(v >> 16) & 255, (v >> 8) & 255, v & 255]; };
const toHex = (c) => '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('');
const rgbOf = (s) => (s.match(/[\d.]+/g) || []).slice(0, 3).map(Number);
const cssOf = (h) => 'rgb(' + hex(h).join(', ') + ')';
function lum(c) {
  const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
}
const contrast = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const AA = 4.5;
// A drawn colour may be one step per channel off the computed one; the model in core must not
// promise more than the pixels show beyond that.
const SLACK = 0.1;

const TABS = [
  { id: 1, title: 'Rust Programming Language', url: 'https://www.rust-lang.org/', private: false },
  { id: 2, title: 'Hacker News', url: 'https://news.ycombinator.com/', private: true },
  { id: 3, title: 'Wikipedia, the free encyclopedia', url: 'https://en.wikipedia.org/', private: false },
  { id: 4, title: 'GitHub: Let’s build from here', url: 'https://github.com/', private: true },
  { id: 5, title: 'Docs.rs', url: 'https://docs.rs/', private: false },
].map((t) => Object.assign({ favicon: '/tab-icon?id=' + t.id + '&v=1', loading: false, sleeping: false }, t));
const tabSel = (id) => '#tabs .tab[data-id="' + id + '"]';

module.exports = (h) => {
  const { openPage, check, emit, shot } = h;
  const CASES = JSON.parse(fs.readFileSync(path.join(__dirname, 'private-tab-colors.json'), 'utf8'));
  const settle = (page, ms) => page.waitForTimeout(ms || 50);

  async function openShell(browser) {
    const o = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 104 }, 1);
    await o.page.evaluate((tabs) => {
      const host = window.__host;
      host.autoSimulate = false;
      host.state.tabs = tabs; host.state.active = 3;
      host.tabsEvent(); host.navEvent({ bookmarksBar: true });
    }, TABS);
    await o.page.waitForSelector(tabSel(3) + '.active');
    // A blank page that only decodes screenshots (the shell's own page stays as it is).
    o.px = await o.context.newPage();
    return o;
  }
  const config = (page, theme, appearance, colors) =>
    emit(page, { event: 'shellConfig', theme: theme, searchName: 'DuckDuckGo', appearance: appearance, colors: colors });
  const focus = (page, on) => emit(page, { event: 'windowState', active: on, maximized: false, fullscreen: false });
  async function activate(page, id) {
    await page.evaluate((i) => { window.__host.state.active = i; window.__host.tabsEvent(); }, id);
    await page.waitForSelector(tabSel(id) + '.active');
  }

  // The title of the tab matching `sel` as drawn: its colour, the mask icon's, the opacity it is
  // drawn with, and every colour behind it (the title is hidden for one screenshot of its box).
  async function measure(o, sel) {
    const info = await o.page.evaluate((s) => {
      const tab = document.querySelector(s);
      const t = tab.querySelector('.title');
      const r = t.getBoundingClientRect();
      let opacity = 1;
      for (let e = t; e; e = e.parentElement) opacity *= +getComputedStyle(e).opacity;
      const mark = tab.querySelector('.private-mark'), close = tab.querySelector('.close');
      const out = { rect: [r.left, r.top, r.width, r.height], color: getComputedStyle(t).color, opacity: opacity,
        mark: mark ? getComputedStyle(mark).color : null, close: close ? getComputedStyle(close).color : null,
        tabBg: getComputedStyle(tab).backgroundColor, tabImg: getComputedStyle(tab).backgroundImage };
      t.style.visibility = 'hidden';
      return out;
    }, sel);
    const [x, y, w, hgt] = info.rect;
    const clip = { x: Math.ceil(x) + 1, y: Math.ceil(y) + 1, width: Math.max(4, Math.floor(w) - 3), height: Math.max(4, Math.floor(hgt) - 3) };
    const png = await o.page.screenshot({ clip: clip });
    await o.page.evaluate((s) => { document.querySelector(s + ' .title').style.visibility = ''; }, sel);
    const behind = await o.px.evaluate(async (b64) => {
      const bytes = Uint8Array.from(atob(b64), (ch) => ch.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
      const ctx = new OffscreenCanvas(bmp.width, bmp.height).getContext('2d');
      ctx.drawImage(bmp, 0, 0);
      const d = ctx.getImageData(0, 0, bmp.width, bmp.height).data;
      const seen = new Set();
      for (let i = 0; i < d.length; i += 4) seen.add((d[i] << 16) | (d[i + 1] << 8) | d[i + 2]);
      return Array.from(seen).map((v) => [(v >> 16) & 255, (v >> 8) & 255, v & 255]);
    }, png.toString('base64'));
    const fg = rgbOf(info.color);
    let worst = behind[0];
    for (const b of behind) if (contrast(fg, b) < contrast(fg, worst)) worst = b;
    return Object.assign(info, { fg: toHex(fg), behind: behind, worst: toHex(worst), ratio: Math.floor(contrast(fg, worst) * 100) / 100 });
  }

  // ------------------------------------------------- every look, both themes, every state
  async function testPrivateTabContrast(browser) {
    console.log('0.16.0 private tabs: title contrast on every style');
    check(CASES.length >= 20 && CASES.slice(0, 20).map((c) => c.appearance.preset).join() === 'classic,sunset,ocean,glass,mono,aurora,midnight,forest,ember,candy,frost,neon,synthwave,matrix,graphite,citrus,mint,sakura,coffee,royal',
      'the fixture from core holds the twenty built-in styles and ' + (CASES.length - 20) + ' other looks');
    for (const theme of ['light', 'dark']) {
      const o = await openShell(browser);
      const page = o.page;
      let lowest = { ratio: 99 };
      for (const c of CASES) {
        const want = c.colors.private[theme], model = c.ratios[theme];
        const tag = theme + ': ' + c.name + ': ';
        const builtIn = CASES.indexOf(c) < 12;
        await config(page, theme, c.appearance, c.colors);
        const bad = [];
        // One state: the title has the colour core chose, the mask icon the same, nothing dims
        // it, and it reads on the pixels behind it at least as well as core's model says.
        const state = (name, m, colour, modelled) => {
          const need = Math.max(modelled >= AA ? AA : 0, modelled - SLACK);
          if (m.fg !== colour) bad.push(name + ': title ' + m.fg + ', not ' + colour);
          if (m.mark !== null && m.mark !== m.color) bad.push(name + ': mask icon ' + m.mark + ', title ' + m.color);
          if (m.opacity !== 1) bad.push(name + ': drawn at opacity ' + m.opacity);
          if (m.ratio < need) bad.push(name + ': ' + m.fg + ' on ' + m.worst + ' is ' + m.ratio + ' (core says ' + modelled + ', need ' + need.toFixed(2) + ')');
          if (builtIn && m.ratio < lowest.ratio) lowest = { ratio: m.ratio, where: c.name + ', ' + name + ': ' + m.fg + ' on ' + m.worst };
          return m;
        };
        const seen = {};
        for (const focused of [true, false]) {
          const win = focused ? '' : ' (window in the background)';
          await focus(page, focused);
          await settle(page);
          // Inactive: both private tabs (two places on a gradient strip).
          seen['inactive' + win] = state('inactive' + win, await measure(o, tabSel(2)), want.fg, model.inactive);
          state('inactive, second tab' + win, await measure(o, tabSel(4)), want.fg, model.inactive);
          // Under the mouse.
          await page.locator(tabSel(2) + ' .title').hover();
          await settle(page);
          seen['hover' + win] = state('hovered' + win, await measure(o, tabSel(2)), want.hoverFg, model.hover);
          await page.mouse.move(600, 95);
          // Active, on its purple fill.
          await activate(page, 2);
          await settle(page);
          seen['active' + win] = state('active' + win, await measure(o, tabSel(2)), want.activeFg, model.active);
          if (focused && (process.env.SHOT_ALL || ['Classic', 'Sunset', 'Midnight', 'Glass'].includes(c.name))) await shot(page, 'private-' + c.appearance.preset + '-' + theme + '.png');
          await activate(page, 3);
        }
        await focus(page, true);
        check(bad.length === 0, tag + 'the title reads in every state (inactive ' + seen.inactive.fg + ' ' + seen.inactive.ratio + ', hovered ' + seen.hover.fg + ' ' + seen.hover.ratio +
          ', active ' + seen.active.fg + ' ' + seen.active.ratio + '; core ' + JSON.stringify(model) + ')' + (bad.length ? ' ' + bad.join('; ') : ''));
        // A hovered tab on a gradient or glass strip is veiled, elsewhere filled with the theme's hover colour.
        const veiled = ['gradient', 'glass'].includes(c.appearance.tabStyle);
        check(veiled ? /^rgba\((0, 0, 0|255, 255, 255), 0\.15\)$/.test(seen.hover.tabBg) && seen.hover.tabBg === want.hoverBg : seen.hover.tabBg === (theme === 'dark' ? 'rgb(58, 60, 66)' : 'rgb(229, 231, 235)'),
          tag + 'hovered fill ' + seen.hover.tabBg);
        // The active private tab keeps its purple fill.
        check(seen.active.tabBg === (theme === 'dark' ? 'rgb(59, 42, 92)' : 'rgb(237, 233, 254)') && seen.active.tabImg === 'none', tag + 'the active private tab is filled with --private-bg (' + seen.active.tabBg + ')');
      }
      // A private tab dragged in from another window: its ghost (the theme's raised surface).
      for (const c of [CASES[0], CASES[6]]) {
        await config(page, theme, c.appearance, c.colors);
        for (const focused of [true, false]) {
          await focus(page, focused);
          await emit(page, { event: 'tabDropHint', index: 1, ghost: { id: 77, title: 'A private tab from another window', favicon: null, private: true } });
          await page.waitForSelector('.tab.ghost.private:not(.opening)');
          await page.waitForTimeout(260);
          const m = await measure(o, '.tab.ghost.private');
          check(m.fg === c.colors.private[theme].ghostFg && m.ratio >= AA && m.ratio >= c.ratios[theme].ghost - SLACK && m.opacity === 1,
            theme + ': ' + c.name + (focused ? '' : ' (window in the background)') + ': the ghost of a private tab reads (' + m.fg + ' on ' + m.worst + ': ' + m.ratio + ')');
          await emit(page, { event: 'tabDropHint', index: null });
          await page.waitForFunction(() => !document.querySelector('.tab.ghost, .tab.closing'));
        }
        await focus(page, true);
      }
      check(lowest.ratio >= AA, theme + ': the lowest contrast measured on a built-in style: ' + lowest.ratio + ' (' + lowest.where + ')');
      check(o.external.length === 0, theme + ': no network requests');
      check(o.errors.length === 0, theme + ': no console errors (' + o.errors.join(' | ') + ')');
      await o.context.close();
    }
  }

  // ------------------------------------------------- the user's colours, the fallbacks, normal tabs
  async function testPrivateTabColours(browser) {
    console.log('0.16.0 private tabs: the user\'s title colours, the theme, normal tabs');
    const byName = (n) => CASES.find((c) => c.name === n);
    const classic = byName('Classic'), ocean = byName('Ocean'), sunset = byName('Sunset');
    const o = await openShell(browser);
    const page = o.page;
    const look = (sel) => page.evaluate((s) => {
      const tab = document.querySelector(s), cs = getComputedStyle(tab);
      const part = (q) => { const n = tab.querySelector(q); return n ? getComputedStyle(n).color : null; };
      return { color: cs.color, title: part('.title'), mark: part('.private-mark'), close: part('.close'), bg: cs.backgroundColor, img: cs.backgroundImage, opacity: cs.opacity };
    }, sel);

    // Settings → Appearance → Tab title colours win on private tabs too, focused or not.
    for (const c of [classic, ocean]) {
      for (const theme of ['light', 'dark']) {
        const a = Object.assign({}, c.appearance, { tabTextColor: '#ff8800', activeTabTextColor: '#00ffaa' });
        await config(page, theme, a, c.colors);
        const tag = theme + ': ' + c.name + ': ';
        for (const focused of [true, false]) {
          await focus(page, focused);
          await settle(page);
          const ina = await look(tabSel(2));
          check(ina.title === 'rgb(255, 136, 0)' && ina.mark === ina.title && ina.opacity === '1', tag + 'the user\'s colour for the other tabs applies to a private tab and its mask icon' + (focused ? '' : ' (window in the background)') + ' (' + ina.title + ')');
          await page.locator(tabSel(2) + ' .title').hover();
          await settle(page);
          check((await look(tabSel(2))).title === 'rgb(255, 136, 0)', tag + 'and under the mouse' + (focused ? '' : ' (window in the background)'));
          await page.mouse.move(600, 95);
        }
        await focus(page, true);
        check((await look(tabSel(1))).title === 'rgb(255, 136, 0)' && (await look(tabSel(3))).title === 'rgb(0, 255, 170)', tag + 'normal tabs use the user\'s colours as before');
        await activate(page, 2);
        const act = await look(tabSel(2));
        check(act.title === 'rgb(0, 255, 170)' && act.mark === act.title && act.bg === (theme === 'dark' ? 'rgb(59, 42, 92)' : 'rgb(237, 233, 254)'), tag + 'the user\'s colour for the active tab applies to the active private tab, on its purple fill (' + act.title + ')');
        await activate(page, 3);
        // Cleared again: back to the colours core chose.
        await config(page, theme, c.appearance, c.colors);
        await settle(page);
        check((await look(tabSel(2))).title === cssOf(c.colors.private[theme].fg), tag + 'without them the title is the private colour again (' + c.colors.private[theme].fg + ')');
      }
    }

    // A host event without `colors.private` (and the moment before the first one): the
    // stylesheet's own fallbacks are the default look's colours, per theme.
    for (const theme of ['light', 'dark']) {
      await config(page, theme, classic.appearance, {});
      await settle(page);
      const want = classic.colors.private[theme];
      const ina = await look(tabSel(2));
      await page.locator(tabSel(2) + ' .title').hover();
      await settle(page);
      const hov = await look(tabSel(2));
      await page.mouse.move(600, 95);
      await activate(page, 2);
      const act = await look(tabSel(2));
      await activate(page, 3);
      check(ina.title === cssOf(want.fg) && hov.title === cssOf(want.hoverFg) && act.title === cssOf(want.activeFg),
        theme + ': the stylesheet\'s fallbacks are the default look\'s colours from core (' + [want.fg, want.hoverFg, want.activeFg].join(', ') + ')');
    }
    check(classic.colors.private.light.fg === '#6d28d9' && classic.colors.private.light.hoverFg === '#6d28d9' && classic.colors.private.light.activeFg === '#6d28d9',
      'Classic in the light theme keeps the private purple #6d28d9 in every state');

    // "Follow Windows": the shell picks the theme's set by itself.
    for (const scheme of ['dark', 'light']) {
      await page.emulateMedia({ colorScheme: scheme });
      await config(page, 'system', ocean.appearance, Object.assign({}, ocean.colors, { private: { light: Object.assign({}, ocean.colors.private.light, { fg: '#112233' }), dark: Object.assign({}, ocean.colors.private.dark, { fg: '#445566' }) } }));
      await settle(page);
      check((await look(tabSel(2))).title === cssOf(scheme === 'dark' ? '#445566' : '#112233'), 'theme "system", Windows ' + scheme + ': the ' + scheme + ' theme\'s private colour applies');
    }
    await page.emulateMedia({ colorScheme: 'light' });

    // On a styled strip the buttons of a private tab take the title's colour.
    await config(page, 'light', ocean.appearance, ocean.colors);
    await page.locator(tabSel(2) + ' .title').hover();
    await settle(page);
    const pv = await look(tabSel(2));
    await page.mouse.move(600, 95);
    check(pv.close === pv.title && pv.title === cssOf(ocean.colors.private.light.hoverFg), 'gradient strip: a private tab\'s close button has the title\'s colour (' + pv.close + ')');

    // Normal tabs are drawn exactly as before, with or without the private colours in the event.
    const normal = async () => {
      const ina = await look(tabSel(1)), act = await look(tabSel(3));
      await page.locator(tabSel(5) + ' .title').hover();
      await settle(page);
      const hov = await look(tabSel(5));
      await page.mouse.move(600, 95);
      await settle(page);
      return { ina: ina, act: act, hov: hov };
    };
    for (const theme of ['light', 'dark']) {
      const dark = theme === 'dark';
      for (const c of [classic, sunset, ocean, byName('Glass'), byName('Midnight')]) {
        for (const focused of [true, false]) {
          const tag = theme + ': ' + c.name + (focused ? '' : ' (window in the background)') + ': ';
          await focus(page, focused);
          await config(page, theme, c.appearance, c.colors);
          await settle(page, 220); // a normal tab dims over 160 ms when the window loses the focus
          const withPrivate = await normal();
          const without = Object.assign({}, c.colors);
          delete without.private;
          await config(page, theme, c.appearance, without);
          await settle(page);
          check(JSON.stringify(await normal()) === JSON.stringify(withPrivate), tag + 'normal tabs are the same with and without the private colours');
          const n = withPrivate;
          // What 0.15.3 drew (shell.css before 0.16.0).
          if (c === classic) {
            check(n.ina.title === (dark ? 'rgb(156, 163, 175)' : 'rgb(107, 114, 128)') && n.ina.bg === 'rgba(0, 0, 0, 0)' && n.act.title === (dark ? 'rgb(229, 231, 235)' : 'rgb(31, 41, 55)') &&
              n.act.bg === (dark ? 'rgb(43, 45, 49)' : 'rgb(255, 255, 255)') && n.hov.bg === (dark ? 'rgb(58, 60, 66)' : 'rgb(229, 231, 235)'), tag + 'normal tabs: muted titles, the raised active tab, the theme\'s hover colour');
          } else if (c === ocean || c.name === 'Midnight') {
            check(n.ina.title === cssOf(c.colors.gradient.fg) && n.ina.opacity === (focused ? '1' : '0.75') && /^color\(srgb [\d. ]+ \/ 0\.15\)$/.test(n.hov.bg),
              tag + 'normal tabs: the gradient\'s text colour, dimmed in a background window, a tint under the mouse (' + n.hov.bg + ')');
          } else if (c === sunset) {
            check(n.ina.title === cssOf(c.colors.gradient.ombreFg) && /rgba\(0, 0, 0, 0\.22\)/.test(n.ina.img) && /rgba\(0, 0, 0, 0\.11\)/.test(n.hov.img) && n.act.title === cssOf(c.colors.gradient.fg),
              tag + 'normal tabs: veiled ombre slices with their text colour');
            const p = await look(tabSel(2));
            check(p.img === 'none' && p.bg === 'rgba(0, 0, 0, 0)', tag + 'a private tab lies flat on the strip (no ombre slice)');
          }
        }
        await focus(page, true);
      }
    }
    check(o.external.length === 0, 'no network requests');
    check(o.errors.length === 0, 'no console errors (' + o.errors.join(' | ') + ')');
    await o.context.close();
  }

  return { testPrivateTabContrast, testPrivateTabColours };
};
