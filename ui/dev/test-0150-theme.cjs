// Krunk 0.15.0 (WP-C) UI tests: the AI panel in light and dark (and a live switch), the shell's
// bar styles (gradient, ombre, glass; tab shapes; active highlight; glow), the Settings style
// card (presets mirror core/src/style.rs, rows follow the style) and the new tab page's tile
// actions at 100 % and 125 %. Hooked into test-ui.cjs:
//   ONLY=ThemeAgentPanel,ThemeShellStyles,ThemeSettingsStyle,ThemeNtpTiles node ui/dev/test-ui.cjs [shots]
'use strict';
const fs = require('fs');
const path = require('path');
const mockAi = require('./mock-ai.js');

// ---- colour helpers (WCAG, as core/src/color.rs) ----
const hex = (s) => { const v = parseInt(s.slice(1), 16); return [(v >> 16) & 255, (v >> 8) & 255, v & 255]; };
const toHex = (c) => '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('');
function lum(c) {
  const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
  return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
}
const contrast = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
// What the host sends in `colors.gradient` (ThemeColors::from_appearance, explicit stops).
function gradientColors(stops) {
  const cs = stops.map(hex);
  const pick = (list) => {
    const worst = (t) => Math.min(...list.map((c) => contrast(c, hex(t))));
    return worst('#111827') >= worst('#ffffff') ? '#111827' : '#ffffff';
  };
  const veiled = cs.flatMap((c) => [mix(c, [0, 0, 0], 0.22), mix(c, [0, 0, 0], 0.11)]);
  return { gradient: { stops: stops, fg: pick(cs), ombreFg: pick(veiled) } };
}

const TABS = [
  { id: 1, title: 'Rust Programming Language', url: 'https://www.rust-lang.org/' },
  { id: 2, title: 'Hacker News', url: 'https://news.ycombinator.com/' },
  { id: 3, title: 'Wikipedia, the free encyclopedia', url: 'https://en.wikipedia.org/' },
  { id: 4, title: 'GitHub: Let’s build from here', url: 'https://github.com/' },
  { id: 5, title: 'Docs.rs', url: 'https://docs.rs/' },
].map((t) => Object.assign({ favicon: '/tab-icon?id=' + t.id + '&v=1', loading: false, private: false, sleeping: false }, t));

const APPEARANCE = { accent: '', toolbarColor: '', stripColor: '', backgroundColor: '', density: 'normal', uiFontPx: 13, roundedTabs: true,
  showDownloadsButton: true, showZoomChip: true, showExtensionsButton: true, tabStyle: 'solid', toolbarStyle: 'solid', gradientFrom: '',
  gradientVia: '', gradientTo: '', gradientAngle: 90, preset: '', tabShape: 'rounded', tabRadius: 8, activeHighlight: 'fill', accentGlow: false };

module.exports = (h) => {
  const { openPage, check, lastSent, emit, shot, SHOTS } = h;
  const settle = (page, ms) => page.waitForTimeout(ms || 60);

  // In the page: the text/background contrast of the first element matching each selector
  // (the background is the first opaque one up the tree).
  const contrasts = (page, sels) => page.evaluate((list) => {
    // rgb()/rgba() and color(srgb r g b / a) to [r, g, b, a].
    const COLOR = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)|color\(srgb ([\d.]+) ([\d.]+) ([\d.]+)(?: \/ ([\d.]+))?\)/g;
    const all = (s) => Array.from((s || '').matchAll(COLOR)).map((m) => m[1] !== undefined ? [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]]
      : [m[5] * 255, m[6] * 255, m[7] * 255, m[8] === undefined ? 1 : +m[8]]);
    const rgb = (s) => all(s)[0] || null;
    const L = (c) => { const f = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); }; return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]); };
    const ratio = (x, y) => { const a = L(x), b = L(y); return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05); };
    // A gradient background: its stops (a one-colour translucent layer is a veil over them, a
    // one-colour opaque layer is a marker such as the underline).
    const stopsOf = (img) => {
      let veil = null; const stops = [];
      for (const layer of img.split(/,\s*(?=linear-gradient|radial-gradient)/)) {
        const cs = all(layer);
        if (!cs.length) continue;
        const one = cs.every((c) => c.join() === cs[0].join());
        if (one && cs[0][3] < 1) veil = cs[0];
        else if (!one) stops.push(...cs.filter((c) => c[3] === 1));
      }
      return veil ? stops.map((s) => s.map((v, i) => i < 3 ? v + (veil[i] - v) * veil[3] : 1)) : stops;
    };
    const out = {};
    for (const sel of list) {
      const n = document.querySelector(sel);
      if (!n) { out[sel] = null; continue; }
      const fg = rgb(getComputedStyle(n).color);
      let bgs = null;
      for (let e = n; e; e = e.parentElement) {
        const s = getComputedStyle(e);
        const c = rgb(s.backgroundColor);
        if (c && c[3] > 0.5) { bgs = [c]; break; }
        if (/linear-gradient/.test(s.backgroundImage)) { const st = stopsOf(s.backgroundImage); if (st.length) { bgs = st; break; } }
      }
      bgs = bgs || [rgb(getComputedStyle(document.documentElement).backgroundColor) || [255, 255, 255]];
      out[sel] = Math.round(Math.min(...bgs.map((b) => ratio(fg, b))) * 100) / 100;
    }
    return out;
  }, sels);

  // ---------------------------------------------------------------- item 5: the AI panel
  async function testThemeAgentPanel(browser) {
    console.log('0.15.0 theme: AI panel light/dark');
    const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/agent.html', { width: 380, height: 760 }, 1, mockAi);
    await page.evaluate(() => { window.__host.autoSimulate = false; });
    const ai = (fn, ...args) => page.evaluate(([f, a]) => window.__ai[f](...a), [fn, args]);
    await emit(page, { event: 'theme', theme: 'light' });
    await ai('config');
    await page.fill('#input', 'List the links on this page and check the docs');
    await page.press('#input', 'Enter');
    await ai('state', 'running', 0);
    await ai('step', 1, 'ok', { summary: '12 links', detail: '{"limit":100}', ms: 85 });
    await ai('step', 2, 'error', { tool: 'click', label: 'Click #missing', summary: 'No element matches #missing', ms: 12 });
    await ai('step', 3, 'refused', { tool: 'type_text', label: 'Type into #password', summary: 'refused: password field' });
    await ai('stream', '## Links\n\nI found **12 links**; see [the docs](https://example.com/docs) and `code`.\n\n```js\ndocument.title = "x";\n```\n\n> A quote\n\n| A | B |\n|---|---|\n| 1 | 2 |\n', 40);
    await ai('state', 'asking', 3);
    await emit(page, { event: 'agentAsk', ask: { id: 7, question: 'Which format do you want?', options: ['CSV', 'JSON'], multi: false, allowOther: true } });
    await emit(page, { event: 'agentError', message: 'The provider refused the request (429).' });
    await settle(page, 200);
    const sels = ['.md p', '.md pre code', '.md blockquote', '.step-label', '.step.ok .step-sum', '.step.error .step-sum', '.step.refused .step-sum', '.card.ask .card-q', '.card.ask .opt', '#model-name', '#input', '.msg.error'];
    const look = async () => page.evaluate(() => ({ bg: getComputedStyle(document.body).backgroundColor, fg: getComputedStyle(document.body).color,
      pre: getComputedStyle(document.querySelector('.md pre')).backgroundColor, opt: getComputedStyle(document.querySelector('.card.ask .opt')).backgroundColor,
      dark: document.documentElement.classList.contains('dark') }));
    const light = await look();
    const cl = await contrasts(page, sels);
    check(Object.values(cl).every((v) => v !== null && v >= 4.5), 'light: text in the panel reads (WCAG AA): ' + JSON.stringify(cl));
    await shot(page, 'theme-agent-light.png');
    // The live switch: the host posts Event::Theme to the open panel (apply_theme).
    await emit(page, { event: 'theme', theme: 'dark' });
    await settle(page);
    const dark = await look();
    check(dark.dark && dark.bg !== light.bg && dark.fg !== light.fg && dark.pre !== light.pre && dark.opt !== light.opt,
      'a theme event switches the panel live: page, text, code blocks and options (' + light.bg + ' -> ' + dark.bg + ')');
    const cd = await contrasts(page, sels);
    check(Object.values(cd).every((v) => v !== null && v >= 4.5), 'dark: text in the panel reads (WCAG AA): ' + JSON.stringify(cd));
    check(lum(dark.bg.match(/\d+/g).slice(0, 3).map(Number)) < 0.05, 'dark: a dark panel background (' + dark.bg + ')');
    await shot(page, 'theme-agent-dark.png');
    // An accent from Settings → Appearance reaches the send button with its readable text.
    await emit(page, { event: 'theme', theme: 'dark', accent: '#facc15', colors: { accent: '#facc15', accentFg: '#111827' } });
    await ai('state', 'idle', 3);
    await settle(page);
    await page.fill('#input', 'x');
    await settle(page);
    const send = await page.evaluate(() => { const s = getComputedStyle(document.getElementById('send')); return [s.backgroundColor, s.color]; });
    check(send[0] === 'rgb(250, 204, 21)' && send[1] === 'rgb(17, 24, 39)', 'the accent and its text colour apply live (' + send.join(' / ') + ')');
    await emit(page, { event: 'theme', theme: 'light' });
    await settle(page);
    check(!(await page.evaluate(() => document.documentElement.classList.contains('dark'))) && (await page.evaluate(() => getComputedStyle(document.body).backgroundColor)) === light.bg, 'and back to light');
    check(external.length === 0, 'AI panel: no network requests');
    check(errors.length === 0, 'AI panel: no console errors (' + errors.join(' | ') + ')');
    await page.context().close();
  }

  // ---------------------------------------------------------------- item 6: shell styles
  async function testThemeShellStyles(browser) {
    console.log('0.15.0 theme: shell bar styles');
    const SUNSET = ['#f97316', '#ec4899', '#8b5cf6'];
    const OCEAN = ['#0ea5e9', '#14b8a6'];
    const looks = [
      { name: 'gradient', a: { tabStyle: 'gradient', toolbarStyle: 'gradient', gradientFrom: OCEAN[0], gradientTo: OCEAN[1], gradientAngle: 135, tabShape: 'pill', activeHighlight: 'outline' }, stops: OCEAN },
      { name: 'ombre', a: { tabStyle: 'ombre', toolbarStyle: 'gradient', gradientFrom: SUNSET[0], gradientVia: SUNSET[1], gradientTo: SUNSET[2], accentGlow: true }, stops: SUNSET },
      { name: 'ombre-solidbar', a: { tabStyle: 'ombre', toolbarStyle: 'solid', gradientFrom: SUNSET[0], gradientVia: SUNSET[1], gradientTo: SUNSET[2], activeHighlight: 'underline', tabRadius: 4 }, stops: SUNSET },
      { name: 'glass', a: { tabStyle: 'glass', toolbarStyle: 'glass', activeHighlight: 'underline', accentGlow: true }, stops: [] },
    ];
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, 'https://shell.krunk.invalid/shell.html', { width: 1100, height: 104 }, 1);
      await page.evaluate((tabs) => {
        const h = window.__host;
        h.autoSimulate = false;
        h.state.tabs = tabs; h.state.active = 2;
        h.tabsEvent(); h.navEvent();
        h.emit({ event: 'nav', state: { tabId: 2, url: 'https://news.ycombinator.com/', canGoBack: true, canGoForward: false, loading: false, secure: true,
          bookmarked: false, blocked: 3, siteAllowed: false, zoom: 1, private: false, bookmarksBar: true } });
      }, TABS);
      await page.waitForSelector('.tab.active');
      for (const l of looks) {
        const a = Object.assign({}, APPEARANCE, l.a);
        const colors = l.stops.length ? gradientColors(l.stops) : {};
        await emit(page, { event: 'shellConfig', theme: theme, searchName: 'DuckDuckGo', appearance: a, colors: colors });
        await settle(page, 120);
        const st = await page.evaluate(() => {
          const cs = (sel) => getComputedStyle(document.querySelector(sel));
          const strip = cs('#tabstrip'), bar = cs('#toolbar'), act = cs('.tab.active'), ina = cs('.tab:not(.active)');
          return { root: document.documentElement.className, stripImg: strip.backgroundImage, stripBg: strip.backgroundColor, stripFilter: strip.backdropFilter,
            barImg: bar.backgroundImage, barBg: bar.backgroundColor, barFg: bar.color, barFilter: bar.backdropFilter, barAttach: bar.backgroundAttachment,
            actImg: act.backgroundImage, actAttach: act.backgroundAttachment, actFg: act.color, actShadow: act.boxShadow, actRadius: act.borderRadius, actH: document.querySelector('.tab.active').getBoundingClientRect().height,
            inaImg: ina.backgroundImage, inaFg: ina.color, bodyImg: cs('body').backgroundImage };
        });
        const rgbOf = (h) => 'rgb(' + hex(h).join(', ') + ')';
        if (l.name === 'gradient') {
          check(st.stripImg.startsWith('linear-gradient(135deg') && st.barImg.startsWith('linear-gradient(135deg') && st.barAttach === 'fixed',
            theme + ': gradient: the strip and the toolbar show one window-fixed gradient');
          check(st.inaFg === rgbOf(colors.gradient.fg) && st.barFg === rgbOf(colors.gradient.fg), theme + ': gradient: inactive tabs and toolbar use the readable text (' + colors.gradient.fg + ')');
          check(st.actRadius === '13px' && st.actH === 26, theme + ': pill tabs (' + st.actRadius + ', ' + st.actH + ' px)');
          check(/inset/.test(st.actShadow) && !st.actImg.startsWith('linear-gradient(135deg'), theme + ': outline highlight: an inset accent ring on a tint (' + st.actShadow + ')');
        } else if (l.name === 'ombre') {
          check(/linear-gradient\(rgba\(0, 0, 0, 0\.22\).*linear-gradient\(90deg/.test(st.inaImg), theme + ': ombre: an inactive tab shows its veiled slice of the strip gradient');
          check(st.actImg.includes('linear-gradient(90deg') && !st.actImg.includes('rgba(0, 0, 0, 0.22)') && st.actAttach.includes('fixed') && st.actFg === rgbOf(colors.gradient.fg), theme + ': ombre: the active tab shows it at full strength, joined to the gradient toolbar');
          check(st.inaFg === rgbOf(colors.gradient.ombreFg), theme + ': ombre: inactive tab text ' + colors.gradient.ombreFg);
          check((st.actShadow.match(/rgb/g) || []).length >= 2, theme + ': accent glow adds a second shadow (' + st.actShadow + ')');
          check(st.actRadius === '8px 8px 0px 0px', theme + ': rounded tabs with the default radius');
        } else if (l.name === 'ombre-solidbar') {
          check(st.barImg === 'none' && st.actImg.includes('linear-gradient(90deg'), theme + ': ombre with a solid toolbar: the tabs keep the gradient, the toolbar is plain');
          check(/linear-gradient\(rgb\(\d+, \d+, \d+\), rgb/.test(st.actImg), theme + ': underline highlight: an accent bar layer over the active tab');
          check(st.actRadius === '4px 4px 0px 0px', theme + ': the roundness setting applies (' + st.actRadius + ')');
        } else if (l.name === 'glass') {
          const alpha = (c) => { const m = /(?:rgba\([^)]*,\s*|\/ )([\d.]+)\)/.exec(c); return m ? +m[1] : 1; };
          check(/blur/.test(st.stripFilter) && /blur/.test(st.barFilter) && alpha(st.stripBg) < 0.8 && alpha(st.barBg) < 0.8,
            theme + ': glass: translucent strip and toolbar with a backdrop blur (' + st.stripBg + ', ' + st.barBg + ')');
          check(/radial-gradient/.test(st.bodyImg), theme + ': glass: a soft accent backdrop behind the bars');
        }
        // Everything stays readable: tab titles, the toolbar buttons and the address field.
        const cr = await contrasts(page, ['.tab.active .title', '#toolbar > #back', '#address-display']);
        const inactive = l.name === 'glass' ? await contrasts(page, ['.tab:not(.active) .title']) : {};
        const ok = Object.values(cr).concat(Object.values(inactive)).every((v) => v === null || v >= 3);
        check(ok, theme + ': ' + l.name + ': readable (' + JSON.stringify(Object.assign(cr, inactive)) + ')');
        await page.locator('.tab').nth(3).hover();
        await settle(page, 40);
        await shot(page, 'theme-shell-' + l.name + '-' + theme + '.png');
        await page.mouse.move(600, 90);
      }
      // A 0.14 host sends only roundedTabs: square tabs, no style classes.
      await emit(page, { event: 'shellConfig', theme: theme, searchName: 'DuckDuckGo', appearance: { accent: '', toolbarColor: '', stripColor: '', backgroundColor: '', density: 'normal', uiFontPx: 13, roundedTabs: false, showDownloadsButton: true, showZoomChip: true, showExtensionsButton: true } });
      await settle(page);
      const old = await page.evaluate(() => ({ cls: document.documentElement.className, r: getComputedStyle(document.querySelector('.tab.active')).borderRadius,
        img: getComputedStyle(document.getElementById('tabstrip')).backgroundImage }));
      check(old.cls.includes('square-tabs') && old.r === '0px' && old.img === 'none' && old.cls.includes('tabs-solid'), theme + ': an older host\'s roundedTabs:false still gives square, solid tabs');
      check(external.length === 0, theme + ': shell: no network requests');
      check(errors.length === 0, theme + ': shell: no console errors (' + errors.join(' | ') + ')');
      await page.context().close();
    }
  }

  // ---------------------------------------------------------------- item 6: the Settings card
  function styleSettingsMock(defaults) {
    const h = window.__host.handlers;
    const st = { settings: JSON.parse(JSON.stringify(defaults)) };
    window.__st = st;
    h.getSettings = () => ({ settings: st.settings, restartRequired: false, dnsPresets: [], wallpapers: [], userAgents: [], defaultUserAgent: 'Mozilla/5.0',
      about: { version: '0.15.0', cef: '156.0.15', chromium: '156.0.8078.80', dataDir: 'C:\\Users\\me\\AppData\\Local\\Krunk' } });
    h.saveSettings = (m) => {
      const a = m.settings.appearance;
      if (a.toolbarStyle === 'ombre') { const e = new Error('Some settings are not valid'); e.data = [{ field: 'appearance.toolbarStyle', message: 'Ombre is for the tab strip' }]; throw e; }
      st.settings = m.settings;
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

  // core/src/style.rs PRESETS, read from the source (the settings page mirrors them).
  function rustPresets() {
    // Line comments out first: one inside a preset (0.15.2) made the pattern run into the next.
    const src = fs.readFileSync(path.join(__dirname, '..', '..', 'core', 'src', 'style.rs'), 'utf8').replace(/\/\/[^\n]*/g, '');
    const out = {};
    const re = /Preset \{\s*id: "(\w+)",[\s\S]*?tab_style: BarStyle::(\w+),\s*toolbar_style: BarStyle::(\w+),\s*gradient: \[([^\]]*)\],\s*gradient_angle: (\d+),\s*tab_shape: TabShape::(\w+),\s*active_highlight: ActiveHighlight::(\w+),\s*accent_glow: (true|false),/g;
    const lc = (s) => s[0].toLowerCase() + s.slice(1);
    let m;
    while ((m = re.exec(src))) {
      const g = m[4].split(',').map((s) => s.trim().replace(/"/g, ''));
      out[m[1]] = { tabStyle: lc(m[2]), toolbarStyle: lc(m[3]), gradientFrom: g[0], gradientVia: g[1], gradientTo: g[2], gradientAngle: +m[5],
        tabShape: lc(m[6]), activeHighlight: lc(m[7]), accentGlow: m[8] === 'true' };
    }
    return out;
  }

  async function testThemeSettingsStyle(browser) {
    console.log('0.15.0 theme: Settings style card');
    const defaults = JSON.parse(fs.readFileSync(path.join(__dirname, 'default-settings.json'), 'utf8'));
    const presets = rustPresets();
    check(Object.keys(presets).length === 20, 'read the twenty presets from core/src/style.rs (' + Object.keys(presets).join(', ') + ')');
    for (const theme of ['light', 'dark']) {
      const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/settings.html#appearance', { width: 1000, height: 1400 }, 1, styleSettingsMock,
        Object.assign({}, defaults, { theme: theme }));
      await page.waitForSelector('#style-card');
      await settle(page, 150);
      const vis = () => page.evaluate(() => ['gradient-colors-row', 'gradient-angle-row', 'tab-radius-row'].map((id) => !document.getElementById(id).hidden));
      check(JSON.stringify(await vis()) === '[false,false,true]', theme + ': solid styles hide the gradient rows; rounded tabs show the roundness');
      for (const [id, p] of Object.entries(presets)) {
        await page.selectOption('[data-key="appearance.preset"]', id);
        await page.waitForFunction((x) => window.__st.settings.appearance.preset === x, id);
        const a = await page.evaluate(() => window.__st.settings.appearance);
        const same = Object.entries(p).every(([k, v]) => a[k] === v) && a.roundedTabs === (p.tabShape !== 'square');
        check(same, theme + ': choosing "' + id + '" saves its values like Preset::apply (' + JSON.stringify(p) + ')');
        await settle(page, 40);
        const rows = await vis();
        const grad = ['gradient', 'ombre'].includes(p.tabStyle) || p.toolbarStyle === 'gradient';
        check(rows[0] === grad && rows[1] === grad && rows[2] === (p.tabShape === 'rounded'), theme + ': ' + id + ': the gradient and roundness rows follow (' + rows + ')');
        if (id === 'sunset') await shot(page.locator('#style-card'), 'theme-settings-style-sunset-' + theme + '.png');
      }
      // Changing one value after a preset makes it the user's own; the shape keeps roundedTabs.
      await page.selectOption('[data-key="appearance.preset"]', 'ocean');
      await page.waitForFunction(() => window.__st.settings.appearance.preset === 'ocean');
      await page.selectOption('[data-key="appearance.tabShape"]', 'square');
      await page.waitForFunction(() => window.__st.settings.appearance.tabShape === 'square');
      let a = await page.evaluate(() => window.__st.settings.appearance);
      check(a.preset === '' && a.roundedTabs === false && await page.inputValue('[data-key="appearance.preset"]') === '', theme + ': a change after a preset shows "Your own"; square tabs mirror roundedTabs=false');
      // The gradient colours carry colour pickers; the rows show at once on a style change.
      await page.selectOption('[data-key="appearance.preset"]', '');
      await page.selectOption('[data-key="appearance.tabStyle"]', 'solid');
      await page.selectOption('[data-key="appearance.toolbarStyle"]', 'solid');
      await page.waitForFunction(() => window.__st.settings.appearance.toolbarStyle === 'solid' && window.__st.settings.appearance.tabStyle === 'solid');
      await settle(page);
      check(JSON.stringify(await vis()) === '[false,false,false]', theme + ': solid and square: no gradient or roundness rows');
      await page.selectOption('[data-key="appearance.tabStyle"]', 'gradient');
      check((await vis())[0] === true, theme + ': choosing a gradient shows its rows at once');
      check(await page.locator('#gradient-colors-row .color-picker').count() === 3 && await page.locator('#gradient-colors-row input[type=color]').count() === 3,
        theme + ': each gradient colour has a colour picker');
      await page.fill('[data-key="appearance.gradientFrom"]', '#ff0000');
      await page.dispatchEvent('[data-key="appearance.gradientFrom"]', 'change');
      await page.waitForFunction(() => window.__st.settings.appearance.gradientFrom === '#ff0000');
      check(true, theme + ': a typed gradient colour saves');
      await shot(page.locator('#style-card'), 'theme-settings-style-' + theme + '.png');
      check(external.length === 0, theme + ': settings: no network requests');
      check(errors.length === 0, theme + ': settings: no console errors (' + errors.join(' | ') + ')');
      a = null;
      await page.context().close();
    }
  }

  // ---------------------------------------------------------------- item 8: NTP tile actions
  async function testThemeNtpTiles(browser) {
    console.log('0.15.0 theme: new tab page tile actions');
    const tiles = [
      ['https://www.rust-lang.org/', 'Rust Programming Language', true],
      ['https://news.ycombinator.com/', 'Hacker News'],
      ['https://en.wikipedia.org/wiki/Main_Page', 'Wikipedia', true],
      ['https://github.com/', 'GitHub'],
      ['https://192.168.1.1/', 'Router admin', true],
      ['https://docs.rs/', 'Docs.rs'],
    ].map(([url, title, pinned], i) => ({ url, title, pinned: !!pinned, favicon: i === 4 ? null : '/favicon?host=' + new URL(url).hostname }));
    const runs = [
      { scale: 1, cols: 6, width: 1100 },
      { scale: 1.25, cols: 6, width: 1100 },
      { scale: 1, cols: 10, width: 640, narrow: true },
    ];
    for (const theme of ['light', 'dark']) {
      for (const r of runs) {
        const tag = theme + ' ' + r.scale + 'x' + (r.narrow ? ' narrow' : '');
        const { page, errors, external } = await openPage(browser, 'https://krunk.invalid/newtab.html', { width: r.width, height: 600 }, r.scale, (arg) => {
          const h = window.__host.handlers;
          h.getTopSites = () => arg.tiles;
          h.getNewTabConfig = () => ({ searchName: 'DuckDuckGo', shortcuts: [], imageUrl: null,
            ntp: { background: 'none', backgroundColor: '', image: '', imageDim: 30, imageBlur: 0, tiles: 'mostVisited', columns: arg.cols, rows: 2, showShortcuts: false, showSearch: true, showTiles: true } });
          h.pinSite = () => null; h.unpinSite = () => null; h.hideSite = () => null;
        }, { tiles: tiles, cols: r.cols });
        await emit(page, { event: 'theme', theme: theme });
        await page.waitForSelector('.tile:not(.add)');
        await settle(page, 150);
        check(await page.locator('.pinned-mark').count() === 0, tag + ': no separate pinned badge');
        // Not hovered: a pinned tile shows only its filled pin, in the corner; an unpinned tile nothing.
        const idle = await page.evaluate(() => {
          const vis = (n) => getComputedStyle(n).visibility === 'visible';
          const t0 = document.querySelectorAll('.tile')[0], t1 = document.querySelectorAll('.tile')[1];
          const pin = t0.querySelector('.pin');
          const tr = t0.getBoundingClientRect(), pr = pin.getBoundingClientRect();
          return { pinVis: vis(pin), pinFill: getComputedStyle(pin.querySelector('.icon')).fill, pressed: pin.getAttribute('aria-pressed'),
            othersHidden: Array.from(t0.querySelectorAll('.tools .icon-btn:not(.pin)')).every((b) => !vis(b)),
            unpinnedHidden: Array.from(t1.querySelectorAll('.tools .icon-btn')).every((b) => !vis(b)),
            corner: [Math.round(tr.right - pr.right), Math.round(pr.top - tr.top)] };
        });
        check(idle.pinVis && idle.pinFill !== 'none' && idle.pressed === 'true' && idle.othersHidden && idle.unpinnedHidden,
          tag + ': not hovered: the pinned tile keeps a filled pin, everything else hidden (' + JSON.stringify(idle) + ')');
        check(idle.corner[0] <= 8 && idle.corner[1] <= 8, tag + ': the pin sits in the top-right corner (' + idle.corner + ')');
        // Hovered: the cluster; nothing overlaps the avatar, the label or each other.
        for (const i of [0, 1]) {
          const tile = page.locator('.tile').nth(i);
          await tile.hover();
          await settle(page, 60);
          const geo = await page.evaluate((k) => {
            const t = document.querySelectorAll('.tile')[k];
            const rect = (n) => { const b = n.getBoundingClientRect(); return [b.left, b.top, b.right, b.bottom]; };
            const btns = Array.from(t.querySelectorAll('.tools .icon-btn')).map((b) => ({ r: rect(b), vis: getComputedStyle(b).visibility }));
            return { tile: rect(t), avatar: rect(t.querySelector('.avatar')), label: rect(t.querySelector('.label')), btns: btns };
          }, i);
          const overlap = (a, b) => a[0] < b[2] - 0.5 && b[0] < a[2] - 0.5 && a[1] < b[3] - 0.5 && b[1] < a[3] - 0.5;
          const bs = geo.btns.map((b) => b.r);
          let clash = false;
          for (let x = 0; x < bs.length; x++) for (let y = x + 1; y < bs.length; y++) if (overlap(bs[x], bs[y])) clash = true;
          const inside = bs.every((b) => b[0] >= geo.tile[0] && b[2] <= geo.tile[2] + 0.5 && b[1] >= geo.tile[1] && b[3] <= geo.tile[3]);
          check(geo.btns.length === 3 && geo.btns.every((b) => b.vis === 'visible') && !clash && inside,
            tag + ': tile ' + i + ' hovered: three actions, none overlapping, inside the tile');
          check(!bs.some((b) => overlap(b, geo.avatar) || overlap(b, geo.label)), tag + ': tile ' + i + ': the actions cover neither the icon nor the title');
          if (i === 0) {
            const box = await tile.boundingBox();
            await shot(page.locator('#tiles'), 'theme-ntp-' + theme + '-' + r.scale + 'x' + (r.narrow ? '-narrow' : '') + '-hover.png');
            check(box.width > 100, tag + ': tile width ' + Math.round(box.width));
          }
        }
        await page.locator('.tile').nth(0).locator('.pin').click();
        check((await lastSent(page, 'unpinSite')).url === 'https://www.rust-lang.org/', tag + ': the filled pin unpins');
        await page.mouse.move(5, 5);
        await settle(page, 60);
        if (!r.narrow && r.scale === 1) await shot(page.locator('#tiles'), 'theme-ntp-' + theme + '-idle.png');
        check(external.length === 0, tag + ': no network requests');
        check(errors.length === 0, tag + ': no console errors (' + errors.join(' | ') + ')');
        await page.context().close();
      }
    }
    if (SHOTS) check(fs.existsSync(path.join(SHOTS, 'theme-ntp-light-1.25x-hover.png')), 'the 125 % screenshot was written');
  }

  return { testThemeAgentPanel, testThemeShellStyles, testThemeSettingsStyle, testThemeNtpTiles };
};
