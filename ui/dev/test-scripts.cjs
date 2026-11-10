// krunk://scripts (0.12.0, WP-G): the lists, the editor (ui/shared/editor.js), the parse check
// before saving, import, the hotkey recorder and Run/Stop, against a mock of the host's scripts
// commands. Run by test-ui.cjs (ONLY=ScriptsPage,ScriptEditor,JsTokenizer).
'use strict';

// Installed before the page's scripts: answers the Page::Scripts commands like the host does.
function mockScripts(seed) {
  const h = window.__host;
  const st = {
    userscripts: seed.userscripts.slice(), macros: seed.macros.slice(), running: null,
    enabled: Object.assign({ userscripts: true, macros: true }, seed.enabled || {}), next: 1,
  };
  window.__scripts = st;
  const changed = () => h.emit({ event: 'scriptsChanged' });
  const metaName = (src) => { const m = /@name\s+(.+)/.exec(src); return m ? m[1].trim() : ''; };
  const warningsOf = (src) => {
    const w = [];
    if (/GM_xmlhttpRequest/.test(src)) w.push('GM_xmlhttpRequest is not supported in Krunk');
    if (/@require\b/.test(src)) w.push('@require is not supported in Krunk');
    return w;
  };
  const matchesOf = (src) => { const out = []; const re = /@(?:match|include)\s+(\S+)/g; let m; while ((m = re.exec(src))) out.push(m[1]); return out; };
  const BUILTIN = { 'Ctrl+T': 'New tab', 'Ctrl+W': 'Close tab' };
  const conflict = (chord, id) => {
    if (BUILTIN[chord]) return chord + " is Krunk's shortcut for " + BUILTIN[chord];
    const other = st.macros.find((m) => m.hotkey === chord && m.id !== id);
    return other ? chord + ' is the hotkey of the macro "' + other.name + '"' : null;
  };
  Object.assign(h.handlers, {
    scriptsList: () => ({
      userscripts: st.userscripts.map((s) => ({ id: s.id, name: s.name, namespace: '', version: s.version || '', enabled: s.enabled, createdByAi: !!s.createdByAi,
        matches: matchesOf(s.source), lastRun: s.lastRun || null, updated: 1, warnings: warningsOf(s.source) })),
      macros: st.macros.map((m) => ({ id: m.id, name: m.name, hotkey: m.hotkey || null, enabled: m.enabled, createdByAi: !!m.createdByAi, lastRun: m.lastRun || null })),
      running: st.running, enabled: st.enabled,
    }),
    scriptGet: (m) => {
      const u = st.userscripts.find((s) => s.id === m.id);
      if (u) return { kind: 'userscript', id: u.id, name: u.name, source: u.source, enabled: u.enabled, warnings: warningsOf(u.source) };
      const mc = st.macros.find((s) => s.id === m.id);
      if (mc) return { kind: 'macro', id: mc.id, name: mc.name, source: mc.source, hotkey: mc.hotkey || null, enabled: mc.enabled };
      throw new Error('No such script');
    },
    // "let = " stands in for a syntax error: V8's message, 1-based line and column.
    scriptCheck: (m) => {
      const at = m.source.indexOf('let = ');
      if (at >= 0) {
        const before = m.source.slice(0, at + 4);
        const line = before.split('\n').length;
        const column = at + 4 - before.lastIndexOf('\n');
        return { ok: false, line, column, message: "SyntaxError: Unexpected token '='" };
      }
      if (m.kind === 'userscript' && !/==UserScript==/.test(m.source)) return { ok: false, line: 1, column: 1, message: 'No ==UserScript== block' };
      return { ok: true, warnings: m.kind === 'userscript' ? warningsOf(m.source) : [] };
    },
    scriptSave: (m) => {
      if (!/==UserScript==/.test(m.source)) throw new Error('This is not a userscript: the ==UserScript== block is missing');
      let s = m.id && st.userscripts.find((x) => x.id === m.id);
      if (!s) { s = { id: 'new' + (st.next++), enabled: true }; st.userscripts.push(s); }
      s.source = m.source;
      s.name = metaName(m.source) || 'Unnamed';
      s.version = (/@version\s+(\S+)/.exec(m.source) || [])[1] || '';
      changed();
      return { id: s.id, name: s.name, warnings: warningsOf(m.source) };
    },
    scriptToggle: (m) => { st.userscripts.find((s) => s.id === m.id).enabled = m.enabled; changed(); return { id: m.id, enabled: m.enabled }; },
    scriptDelete: (m) => { st.userscripts = st.userscripts.filter((s) => s.id !== m.id); changed(); return null; },
    scriptExport: () => ({ saved: true }),
    scriptRunNow: () => ({ ran: true, title: 'Example Domain' }),
    macroSave: (m) => {
      if (m.hotkey && conflict(m.hotkey, m.id)) throw new Error(conflict(m.hotkey, m.id));
      let s = m.id && st.macros.find((x) => x.id === m.id);
      if (!s) { s = { id: 'mac' + (st.next++), enabled: true }; st.macros.push(s); }
      Object.assign(s, { name: m.name, source: m.source, hotkey: m.hotkey || null });
      changed();
      return { id: s.id };
    },
    macroDelete: (m) => { st.macros = st.macros.filter((s) => s.id !== m.id); changed(); return null; },
    macroToggle: (m) => { st.macros.find((s) => s.id === m.id).enabled = m.enabled; changed(); return null; },
    macroRun: (m) => { st.running = m.id; h.emit({ event: 'macroState', running: m.id }); return null; },
    macroStop: () => { st.running = null; h.emit({ event: 'macroState', running: null }); return null; },
    hotkeyCheck: (m) => ({ chord: m.chord, conflict: conflict(m.chord, m.id) }),
    hotkeyRecord: () => null,
  });
}

const SEED = {
  userscripts: [
    { id: 'us1', name: 'Hide banners', version: '1.2', enabled: true, createdByAi: true, lastRun: Date.now() - 5 * 60000,
      source: '// ==UserScript==\n// @name Hide banners\n// @version 1.2\n// @match https://a.example/*\n// @match https://b.example/*\n// @match https://c.example/*\n// @match https://d.example/*\n// @grant none\n// ==/UserScript==\n\ndocument.querySelector(".banner")?.remove();\n' },
    { id: 'us2', name: 'Old helper', version: '0.3', enabled: false,
      source: '// ==UserScript==\n// @name Old helper\n// @version 0.3\n// @match https://news.example/*\n// ==/UserScript==\n\nconsole.log(1);\n' },
  ],
  macros: [
    { id: 'm1', name: 'Hold A', hotkey: 'Ctrl+Alt+J', enabled: true, createdByAi: true, source: "await key.down('a', { repeat: true });\n" },
  ],
};
const EMPTY = { userscripts: [], macros: [] };

module.exports = (h) => {
  const { openPage, check, sent, lastSent, emit, shot, acceptInPage } = h;
  const URL = 'https://krunk.invalid/scripts.html';
  const count = async (page, cmd) => (await sent(page)).filter((m) => m.cmd === cmd).length;
  const cmds = async (page) => (await sent(page)).map((m) => m.cmd);
  const ta = '#ed-box textarea';
  // Puts text into the editor as typing would (one input event, through the editing commands).
  const insert = (page, text) => page.evaluate((t) => document.execCommand('insertText', false, t), text);
  const caretTo = (page, a, b) => page.evaluate(([x, y]) => {
    const t = document.querySelector('#ed-box textarea');
    t.focus();
    t.setSelectionRange(x, y === undefined ? x : y);
  }, [a, b]);
  const value = (page) => page.$eval(ta, (t) => t.value);
  // A screenshot without the last toast over it.
  const still = async (page, name) => {
    await page.evaluate(() => { const t = document.querySelector('.toast'); if (t) t.classList.remove('show'); });
    await page.waitForTimeout(250);
    await shot(page, name);
  };

  async function testScriptsPage(browser) {
    console.log('scripts page (lists)');
    const { page, errors, external, context } = await openPage(browser, URL, { width: 1100, height: 760 }, 1, mockScripts, SEED);
    await page.waitForSelector('#userscripts-list .row.item');
    const rows = await page.$$eval('#userscripts-list .row.item', (r) => r.map((x) => ({
      name: x.querySelector('.name').textContent, sub: x.querySelector('.sub').textContent, ai: !!x.querySelector('.tag.ai'),
      when: x.querySelector('.when').textContent, off: x.classList.contains('off'), on: x.querySelector('input[role=switch]').checked,
      version: (x.querySelector('.version') || {}).textContent,
    })));
    check(rows.length === 2 && rows[0].name === 'Hide banners' && rows[1].name === 'Old helper', 'the userscripts are listed (' + rows.map((r) => r.name).join(', ') + ')');
    check(rows[0].sub === 'Runs on https://a.example/*, https://b.example/* +2', 'matches: the first two and +N (' + rows[0].sub + ')');
    check(rows[0].ai && !rows[1].ai, '"created by AI" tag');
    check(rows[0].version === 'v1.2', 'version shown');
    check(rows[0].when === 'Ran 5 min ago' && rows[1].when === 'Never ran', 'last run (' + rows[0].when + ' / ' + rows[1].when + ')');
    check(rows[1].off && !rows[1].on && rows[0].on, 'a turned-off script is dimmed with its switch off');
    check(await page.$eval('#tab-userscripts', (a) => a.classList.contains('active')), 'the Userscripts tab is active');
    await shot(page, 'scripts-list-light.png');

    // Toggle.
    await page.click('#userscripts-list .row.item[data-id="us1"] .switch');
    await page.waitForFunction(() => document.querySelector('#userscripts-list .row.item[data-id="us1"]').classList.contains('off'));
    const tog = await lastSent(page, 'scriptToggle');
    check(tog && tog.id === 'us1' && tog.enabled === false, 'the switch sends scriptToggle and the list follows');

    // Export.
    await page.click('#userscripts-list .row.item[data-id="us1"] .export');
    await page.waitForTimeout(80);
    check((await lastSent(page, 'scriptExport') || {}).id === 'us1', 'Export sends scriptExport');

    // Delete, asked in the page first; Cancel keeps it.
    await page.click('#userscripts-list .row.item[data-id="us2"] .delete');
    await page.waitForSelector('#lb-dialog-cancel');
    check((await page.textContent('.lb-dialog-text')).includes('Old helper'), 'Delete asks in the page, naming the script');
    await page.click('#lb-dialog-cancel');
    await page.waitForTimeout(60);
    check(await count(page, 'scriptDelete') === 0, 'Cancel keeps it');
    await page.click('#userscripts-list .row.item[data-id="us2"] .delete');
    await acceptInPage(page);
    await page.waitForFunction(() => document.querySelectorAll('#userscripts-list .row.item').length === 1);
    check((await lastSent(page, 'scriptDelete') || {}).id === 'us2', 'Delete sends scriptDelete; the list reloads');

    // Import a .user.js that uses an unsupported API.
    const userJs = '// ==UserScript==\n// @name Imported one\n// @version 2.0\n// @match https://x.example/*\n// @grant GM_xmlhttpRequest\n// ==/UserScript==\nGM_xmlhttpRequest({ url: "https://x.example/" });\n';
    await page.setInputFiles('#import-file', { name: 'imported.user.js', mimeType: 'text/javascript', buffer: Buffer.from(userJs) });
    await page.waitForFunction(() => document.querySelectorAll('#userscripts-list .row.item').length === 2);
    const imp = await lastSent(page, 'scriptSave');
    check(imp && imp.source === userJs && !imp.id, 'Import reads the file and sends scriptSave with its source');
    const st1 = await page.textContent('#userscripts-status');
    check(st1.includes('Imported "Imported one"') && st1.includes('GM_xmlhttpRequest'), 'the import says what is not supported (' + st1 + ')');
    check(await page.$eval('#userscripts-list .row.item:last-child', (r) => !!r.querySelector('.tag.warn')), 'the imported script is tagged for its unsupported features');
    const saves = await count(page, 'scriptSave');
    await page.setInputFiles('#import-file', { name: 'notes.js', mimeType: 'text/javascript', buffer: Buffer.from('console.log("hi");\n') });
    await page.waitForFunction(() => /not a userscript/.test(document.getElementById('userscripts-status').textContent));
    check(await count(page, 'scriptSave') === saves, 'a file without a ==UserScript== block is not imported, and the page says why');

    // Macros tab, Run and Stop from macroState.
    await page.click('#tab-macros');
    await page.waitForSelector('#macros-list .row.item');
    check(page.url().endsWith('#macros') && !(await page.isVisible('#userscripts-panel')), 'the Macros tab (#macros)');
    const mrow = await page.$eval('#macros-list .row.item', (x) => ({ keys: [...x.querySelectorAll('kbd')].map((k) => k.textContent), ai: !!x.querySelector('.tag.ai'), run: x.querySelector('.run') && x.querySelector('.run').textContent }));
    check(mrow.keys.join('|') === 'Ctrl|Alt|J', 'the hotkey as keycaps (' + mrow.keys.join('+') + ')');
    check(mrow.ai && mrow.run === 'Run', 'macro row: AI tag, Run');
    await page.click('#macros-list .row.item .run');
    await page.waitForSelector('#macros-list .row.item .stop');
    check((await lastSent(page, 'macroRun') || {}).id === 'm1', 'Run sends macroRun; macroState shows Stop');
    await shot(page, 'scripts-macros-light.png');
    await page.click('#macros-list .row.item .stop');
    await page.waitForSelector('#macros-list .row.item .run');
    check(await count(page, 'macroStop') === 1, 'Stop sends macroStop; Run is back');
    await emit(page, { event: 'macroState', running: 'm1' });
    await page.waitForSelector('#macros-list .row.item .stop');
    await emit(page, { event: 'macroState', running: null });
    await page.waitForSelector('#macros-list .row.item .run');
    check(true, 'macroState from the host (hotkey or toolbar) switches Run and Stop');
    await page.click('#macros-list .row.item .switch');
    await page.waitForTimeout(80);
    check((await lastSent(page, 'macroToggle') || {}).enabled === false, 'the macro switch sends macroToggle');

    // Scripts turned off in Settings: a notice with a link to the section.
    await page.evaluate(() => { window.__scripts.enabled = { userscripts: false, macros: false }; window.__host.emit({ event: 'scriptsChanged' }); });
    await page.waitForSelector('#macros-off:not([hidden])');
    await page.click('#macros-off a');
    const op = await lastSent(page, 'openPage');
    check(op && op.page === 'settings' && op.section === 'ai', 'turned off: a notice links to Settings → AI and scripts');

    // Dark theme.
    await emit(page, { event: 'theme', theme: 'dark' });
    await page.click('#tab-userscripts');
    await page.waitForTimeout(150);
    await shot(page, 'scripts-list-dark.png');
    const dark = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    check(dark === 'rgb(31, 32, 35)', 'dark theme applies (' + dark + ')');
    check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await context.close();

    // Nothing yet: what userscripts and macros are.
    const e = await openPage(browser, URL, { width: 1000, height: 600 }, 1, mockScripts, EMPTY);
    await e.page.waitForSelector('#userscripts-empty:not([hidden])');
    check((await e.page.textContent('#userscripts-empty')).includes('runs on the websites you choose'), 'empty userscripts: what they are');
    await shot(e.page, 'scripts-empty-light.png');
    await e.page.click('#tab-macros');
    await e.page.waitForSelector('#macros-empty:not([hidden])');
    check((await e.page.textContent('#macros-empty')).includes('presses keys'), 'empty macros: what they are');
    check(await e.page.isHidden('#macros-off'), 'no "turned off" notice while on');
    check(e.errors.length === 0, 'no console errors (' + e.errors.join(' | ') + ')');
    await e.context.close();
  }

  async function testScriptEditor(browser) {
    console.log('scripts page (editor)');
    const { page, errors, external, context } = await openPage(browser, URL, { width: 1100, height: 760 }, 1, mockScripts, SEED);
    await page.waitForSelector('#userscripts-list .row.item');
    await page.click('#new-userscript');
    await page.waitForSelector('#editor-view:not([hidden])');
    const tpl = await value(page);
    check(page.url().endsWith('#new-userscript') && /\/\/ @name\s+New script/.test(tpl) && /@match\s+https:\/\/example.com\/\*/.test(tpl) && /@grant\s+none/.test(tpl) && /@version\s+0.1/.test(tpl),
      'New: the template with its metadata block');
    check(await page.isVisible('#ed-run-now') && await page.isHidden('#hotkey-row') && await page.isHidden('#ed-name'), 'userscript editor: Run on current tab now, no hotkey or name field');
    const hl = await page.evaluate(() => ({
      com: [...document.querySelectorAll('.ed-hl .tk-com')].map((s) => s.textContent),
      kw: [...document.querySelectorAll('.ed-hl .tk-kw')].map((s) => s.textContent),
      str: [...document.querySelectorAll('.ed-hl .tk-str')].map((s) => s.textContent),
      nums: [...document.querySelectorAll('.ed-num')].length,
    }));
    check(hl.com.includes('// ==UserScript==') && hl.kw.includes('function') && hl.str.includes("'use strict'"), 'the template is coloured (comments, keywords, strings)');
    check(hl.nums === tpl.split('\n').length, 'a line number per line (' + hl.nums + ')');

    // Typing: highlighted as it goes.
    const lines = tpl.split('\n');
    const inside = tpl.indexOf("'use strict';") + "'use strict';".length;
    await caretTo(page, inside);
    await page.keyboard.press('Enter');
    await page.keyboard.type('const re = /a+b/g; const n = 42; const t = `x ${n} y`;');
    await page.waitForTimeout(50);
    const typed = await page.evaluate(() => ({
      kw: [...document.querySelectorAll('.ed-hl .tk-kw')].map((s) => s.textContent),
      num: [...document.querySelectorAll('.ed-hl .tk-num')].map((s) => s.textContent),
      re: [...document.querySelectorAll('.ed-hl .tk-re')].map((s) => s.textContent),
      tpl: [...document.querySelectorAll('.ed-hl .tk-tpl')].map((s) => s.textContent),
      dirty: !document.getElementById('ed-dirty').hidden,
    }));
    check(typed.kw.filter((k) => k === 'const').length === 3 && typed.num.includes('42'), 'typed code is coloured: keywords and numbers');
    check(typed.re.includes('/a+b/g') && typed.tpl.includes('`x ${') && typed.tpl.includes('} y`'), 'regular expressions and template literals (with ${…} as code)');
    check(typed.dirty, '"Not saved" shows after a change');
    const v1 = await value(page);
    const newLine = v1.split('\n')[lines.indexOf("  'use strict';") + 1];
    check(newLine.startsWith('  const re'), 'Enter keeps the indentation (' + JSON.stringify(newLine.slice(0, 12)) + ')');

    // Enter after "{" indents one more step; between braces the closing one moves down.
    await caretTo(page, v1.length);
    await page.keyboard.type('if (n) {');
    await page.keyboard.press('Enter');
    await page.keyboard.type('n++;');
    const v2 = await value(page);
    check(v2.endsWith('if (n) {\n  n++;'), 'Enter after { indents the next line');

    // Tab / Shift+Tab on selected lines.
    const a = v2.indexOf('const re'), b = v2.indexOf('if (n)') + 2;
    await caretTo(page, a, b);
    await page.keyboard.press('Tab');
    const v3 = await value(page);
    check(v3.includes('\n    const re') && v3.includes('\n  if (n) {'), 'Tab indents every selected line by 2 spaces');
    await page.keyboard.press('Shift+Tab');
    await page.keyboard.press('Shift+Tab');
    const v4 = await value(page);
    check(v4.includes('\nconst re') && v4.includes('\nif (n) {'), 'Shift+Tab outdents them');
    await page.keyboard.press('Control+Z');
    await page.keyboard.press('Control+Z');
    check((await value(page)) === v3, 'Ctrl+Z undoes the outdents');
    await caretTo(page, v3.indexOf('n++') );
    await page.keyboard.press('Tab');
    check((await value(page)).includes('    n++'), 'Tab with no selection inserts 2 spaces');

    // Find.
    await page.keyboard.press('Control+F');
    await page.waitForSelector('.ed-find:not([hidden])');
    await page.keyboard.type('const');
    await page.waitForTimeout(50);
    const f1 = await page.evaluate(() => ({ count: document.querySelector('.ed-find-count').textContent, marks: document.querySelectorAll('.ed-hl .ed-match').length, cur: document.querySelectorAll('.ed-hl .ed-match-cur').length }));
    check(f1.count === '1 of 3' && f1.marks === 3 && f1.cur === 1, 'Ctrl+F finds and marks every match (' + f1.count + ', ' + f1.marks + ')');
    await page.keyboard.press('Enter');
    check((await page.textContent('.ed-find-count')) === '2 of 3', 'Enter goes to the next match');
    await page.keyboard.press('Shift+Enter');
    await page.keyboard.press('Shift+Enter');
    check((await page.textContent('.ed-find-count')) === '3 of 3', 'Shift+Enter goes back (and wraps)');
    const sel = await page.$eval(ta, (t) => t.value.slice(t.selectionStart, t.selectionEnd));
    check(sel === 'const', 'the current match is selected in the text');
    await page.fill('.ed-find-input', 'nothing-like-this');
    check((await page.textContent('.ed-find-count')) === 'No results', 'no match: "No results"');
    await page.keyboard.press('Escape');
    check(await page.isHidden('.ed-find') && await page.evaluate(() => document.activeElement === document.querySelector('#ed-box textarea')), 'Esc closes the find bar, back in the text');

    // A syntax error: Ctrl+S checks, marks the line and does not save.
    const before = await count(page, 'scriptSave');
    const v5 = await value(page);
    const errLine = v5.split('\n').length;
    await caretTo(page, v5.length);
    await insert(page, '\nlet = 5;');
    await page.keyboard.press('Control+S');
    await page.waitForSelector('.ed-error:not([hidden])');
    const err = await page.evaluate(() => ({
      msg: document.querySelector('.ed-error').textContent,
      num: (document.querySelector('.ed-num-err') || {}).textContent,
      bar: !document.querySelector('.ed-errbar').hidden,
    }));
    const chk = await lastSent(page, 'scriptCheck');
    check(chk && chk.kind === 'userscript' && chk.source.endsWith('let = 5;'), 'Ctrl+S sends scriptCheck with the source');
    check(err.msg === 'Line ' + (errLine + 1) + ", column 5: SyntaxError: Unexpected token '='" && err.num === String(errLine + 1) && err.bar,
      'the parse error is shown at its line (' + err.msg + ', gutter ' + err.num + ')');
    check(await count(page, 'scriptSave') === before, 'not saved while it does not parse');
    await shot(page, 'scripts-editor-error-light.png');

    // Fixed: check, then save.
    await page.keyboard.press('Control+A');
    await insert(page, tpl.replace("'use strict';", "'use strict';\n  GM_xmlhttpRequest({ url: 'https://example.com/' });"));
    check(await page.isHidden('.ed-error'), 'editing clears the error mark');
    const n0 = (await sent(page)).length;
    await page.click('#ed-save');
    await page.waitForFunction(() => document.querySelector('.toast.show') && document.querySelector('.toast').textContent === 'Saved');
    const order = (await cmds(page)).slice(n0).filter((c) => c === 'scriptCheck' || c === 'scriptSave');
    check(order.join(',') === 'scriptCheck,scriptSave', 'Save: scriptCheck, then scriptSave (' + order.join(',') + ')');
    const saved = await lastSent(page, 'scriptSave');
    check(saved && !saved.id && saved.source.includes('GM_xmlhttpRequest'), 'a new script is saved without an id');
    check(/#new\d+$/.test(page.url()) && await page.isHidden('#ed-dirty'), 'the address becomes the new id; "Not saved" goes away');
    const warn = await page.textContent('#ed-warnings');
    check(await page.isVisible('#ed-warnings') && warn.includes('GM_xmlhttpRequest'), 'the unsupported-API warning is shown (' + warn + ')');
    await page.keyboard.press('Control+S');
    await page.waitForTimeout(100);
    check(/^new\d+$/.test((await lastSent(page, 'scriptSave')).id || ''), 'saving again updates it by id');
    await still(page, 'scripts-editor-light.png');

    // Run on the current tab now.
    await page.click('#ed-run-now');
    await page.waitForFunction(() => /Ran on "Example Domain"/.test(document.querySelector('.toast').textContent));
    check(/^new\d+$/.test((await lastSent(page, 'scriptRunNow') || {}).id || ''), 'Run on current tab now sends scriptRunNow');

    // Leaving with unsaved changes asks first.
    await page.click('#ed-box textarea');
    await page.keyboard.type('x');
    await page.click('#ed-back');
    await page.waitForSelector('#lb-dialog-cancel');
    await page.click('#lb-dialog-cancel');
    await page.waitForTimeout(80);
    check(await page.isVisible('#editor-view') && /#new\d+$/.test(page.url()), 'Back with unsaved changes asks; Keep editing stays');
    await page.click('#ed-back');
    await acceptInPage(page);
    await page.waitForSelector('#list-view:not([hidden])');
    check(page.url().endsWith('#userscripts'), 'Leave goes back to the list');

    // Edit from the list; a long script draws only the lines in view.
    await page.click('#userscripts-list .row.item[data-id="us1"] .edit');
    await page.waitForFunction(() => document.querySelector('#ed-script-name').textContent === 'Hide banners');
    check(page.url().endsWith('#us1') && (await value(page)).includes('.banner'), 'Edit opens the script (#us1)');
    const big = await page.evaluate(() => {
      const t = document.querySelector('#ed-box textarea');
      const lines = [];
      for (let i = 0; i < 6000; i++) lines.push('const v' + i + ' = "line ' + i + '"; // ' + i);
      t.focus();
      const t0 = performance.now();
      t.value = lines.join('\n'); // as a paste would
      t.dispatchEvent(new InputEvent('input', { inputType: 'insertFromPaste' }));
      const ms = performance.now() - t0;
      return { ms, drawn: document.querySelectorAll('.ed-hl .ed-line').length };
    });
    await page.$eval(ta, (t) => { t.scrollTop = t.scrollHeight; });
    await page.waitForTimeout(120);
    const end = await page.evaluate(() => {
      const nums = [...document.querySelectorAll('.ed-num')].map((n) => n.textContent);
      return { last: nums[nums.length - 1], drawn: document.querySelectorAll('.ed-hl .ed-line').length };
    });
    check(big.drawn < 200 && end.drawn < 200 && end.last === '6000', '6,000 lines: only the lines in view are drawn (' + big.drawn + ', ' + end.drawn + ' rows, last ' + end.last + ', ' + big.ms.toFixed(0) + ' ms)');
    const t1 = await page.evaluate(async () => {
      const t = document.querySelector('#ed-box textarea');
      const t0 = performance.now();
      for (let i = 0; i < 20; i++) document.execCommand('insertText', false, 'x');
      return (performance.now() - t0) / 20;
    });
    check(t1 < 40, 'typing in 6,000 lines stays quick (' + t1.toFixed(1) + ' ms a key)');
    const t2 = await page.evaluate(() => {
      const t = document.querySelector('#ed-box textarea');
      t.select();
      const t0 = performance.now();
      t.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
      return { ms: performance.now() - t0, indented: t.value.startsWith('  const v0') && t.value.includes('\n  const v5999') };
    });
    check(t2.indented && t2.ms < 500, 'Tab on all 6,000 lines indents them quickly (' + t2.ms.toFixed(0) + ' ms)');
    await page.keyboard.press('Control+F');
    await page.keyboard.type('line 5999"');
    await page.waitForTimeout(50);
    check((await page.textContent('.ed-find-count')) === '1 of 1', 'find in a long script');
    await page.keyboard.press('Escape');

    // Macros: new, name, hotkey recording, conflicts, save, Run/Stop.
    await page.evaluate(() => { location.hash = '#new-macro'; });
    await page.waitForSelector('#lb-dialog-ok');
    await acceptInPage(page);
    await page.waitForSelector('#hotkey-row:not([hidden])');
    check((await value(page)) === "await key.press('a');\n" && await page.isVisible('#ed-name') && await page.isHidden('#ed-run-now'), 'New macro: the template, a name field, the hotkey row');
    check(await page.evaluate(() => document.activeElement.id === 'ed-name'), 'the name field has focus');
    await page.keyboard.type('Press A');
    await page.click('#hotkey-record');
    check((await lastSent(page, 'hotkeyRecord') || {}).on === true && (await page.textContent('#hotkey-value')) === 'Press the keys…', 'Record: hotkeyRecord on, "Press the keys…"');
    await emit(page, { event: 'hotkeyRecorded', chord: 'Ctrl+Alt+K', conflict: null });
    await page.waitForSelector('#hotkey-value kbd');
    check((await page.$$eval('#hotkey-value kbd', (k) => k.map((x) => x.textContent))).join('+') === 'Ctrl+Alt+K', 'the recorded chord is shown as keycaps');
    check((await lastSent(page, 'hotkeyRecord')).on === false, 'recording stops after a chord (hotkeyRecord off)');
    await page.keyboard.press('Control+S');
    await page.waitForFunction(() => document.querySelector('.toast').textContent === 'Saved' && /#mac\d+$/.test(location.hash));
    const ms = await lastSent(page, 'macroSave');
    const hk = await lastSent(page, 'hotkeyCheck');
    check(hk && hk.chord === 'Ctrl+Alt+K', 'the hotkey is checked before saving');
    check(ms && ms.name === 'Press A' && ms.hotkey === 'Ctrl+Alt+K' && ms.source === "await key.press('a');\n" && (await lastSent(page, 'scriptCheck')).kind === 'macro', 'macroSave with name, source and hotkey (after a macro parse check)');

    // A built-in shortcut: shown in red and not taken.
    await page.click('#hotkey-record');
    await emit(page, { event: 'hotkeyRecorded', chord: 'Ctrl+T', conflict: "Ctrl+T is Krunk's shortcut for New tab" });
    await page.waitForFunction(() => document.getElementById('hotkey-status').classList.contains('error'));
    const conf = await page.evaluate(() => ({ text: document.getElementById('hotkey-status').textContent, color: getComputedStyle(document.getElementById('hotkey-status')).color, recording: document.getElementById('hotkey-value').textContent }));
    check(conf.text === "Ctrl+T is Krunk's shortcut for New tab" && conf.color === 'rgb(220, 38, 38)', 'a conflict is shown in red (' + conf.text + ')');
    check(conf.recording === 'Press the keys…' && await page.isHidden('#ed-dirty'), 'it is not taken; recording goes on');
    await still(page, 'scripts-macro-conflict-light.png');
    const offs = (await sent(page)).filter((m) => m.cmd === 'hotkeyRecord' && m.on === false).length;
    await page.keyboard.press('Escape');
    await page.waitForFunction(() => document.getElementById('hotkey-record').textContent === 'Record');
    check((await sent(page)).filter((m) => m.cmd === 'hotkeyRecord' && m.on === false).length === offs + 1, 'Esc stops recording (hotkeyRecord off)');
    check((await page.$$eval('#hotkey-value kbd', (k) => k.map((x) => x.textContent))).join('+') === 'Ctrl+Alt+K', 'the saved hotkey stays');
    await page.click('#hotkey-record');
    await page.click('#ed-box textarea');
    check(await page.evaluate(() => document.getElementById('hotkey-record').textContent === 'Record'), 'a click elsewhere stops recording');

    // Another macro's hotkey: the save-time check refuses it.
    await page.click('#hotkey-record');
    await emit(page, { event: 'hotkeyRecorded', chord: 'Ctrl+Alt+J', conflict: null });
    await page.waitForFunction(() => document.getElementById('hotkey-value').textContent.includes('J'));
    const saves = await count(page, 'macroSave');
    await page.click('#ed-save');
    await page.waitForFunction(() => document.getElementById('hotkey-status').classList.contains('error'));
    check((await page.textContent('#hotkey-status')).includes('the hotkey of the macro "Hold A"') && await count(page, 'macroSave') === saves, 'a hotkey another macro has is refused at saving');
    await page.click('#hotkey-clear');
    check((await page.textContent('#hotkey-value')) === 'None', 'Clear removes the hotkey');
    await page.click('#ed-save');
    await page.waitForFunction((n) => window.__host.sent.filter((m) => m.cmd === 'macroSave').length === n + 1, saves);
    check((await lastSent(page, 'macroSave')).hotkey == null, 'saved without a hotkey');

    // A macro parse error is shown at its own line (the host corrects the wrapper's offset).
    await caretTo(page, 0);
    await insert(page, 'let = 1;\n');
    await page.keyboard.press('Control+S');
    await page.waitForSelector('.ed-error:not([hidden])');
    check((await page.textContent('.ed-error')).startsWith('Line 1, column 5'), 'a macro syntax error at line 1');
    await page.keyboard.press('Control+Z');
    await page.waitForTimeout(50);

    // Run / Stop in the editor.
    await page.click('#ed-run');
    await page.waitForFunction(() => document.getElementById('ed-run').textContent === 'Stop');
    check(/^mac\d+$/.test((await lastSent(page, 'macroRun') || {}).id || ''), 'Run in the editor sends macroRun; Stop shows while it runs');
    await emit(page, { event: 'theme', theme: 'dark' });
    await page.waitForTimeout(150);
    await still(page, 'scripts-macro-dark.png');
    await page.click('#ed-run');
    await page.waitForFunction(() => document.getElementById('ed-run').textContent === 'Run');
    check(await count(page, 'macroStop') === 1, 'Stop in the editor sends macroStop');

    // Dark editor with an error.
    await page.evaluate(() => { location.hash = '#us1'; });
    await page.waitForFunction(() => document.querySelector('#ed-script-name').textContent === 'Hide banners');
    await caretTo(page, (await value(page)).length);
    await insert(page, 'let = 2;');
    await page.keyboard.press('Control+S');
    await page.waitForSelector('.ed-error:not([hidden])');
    await still(page, 'scripts-editor-dark.png');
    const colors = await page.evaluate(() => ({ kw: getComputedStyle(document.querySelector('.ed-hl .tk-kw') || document.body).color, com: getComputedStyle(document.querySelector('.ed-hl .tk-com')).color }));
    check(colors.com === 'rgb(139, 148, 158)', 'dark colours in the editor (' + colors.com + ')');
    check(external.length === 0, 'no network requests (' + external.join(', ') + ')');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await context.close();

    // A deep link opens that script's editor; an unknown id goes back to the list.
    const d = await openPage(browser, URL + '#m1', { width: 1000, height: 640 }, 1, mockScripts, SEED);
    await d.page.waitForSelector('#hotkey-row:not([hidden])');
    check((await d.page.inputValue('#ed-name')) === 'Hold A' && (await d.page.$$eval('#hotkey-value kbd', (k) => k.map((x) => x.textContent))).join('+') === 'Ctrl+Alt+J', '#<id> opens that macro in the editor');
    await d.page.evaluate(() => { location.hash = '#nope'; });
    await d.page.waitForFunction(() => /does not exist/.test(document.getElementById('userscripts-status').textContent));
    check(d.page.url().endsWith('#userscripts'), 'an unknown id: back to the list, saying so');
    check(d.errors.length === 0, 'no console errors (' + d.errors.join(' | ') + ')');
    await d.context.close();
  }

  // The shared tokenizer on its own (the source viewer's mode and the editor's).
  async function testJsTokenizer(browser) {
    console.log('shared JS tokenizer');
    const { page, errors, context } = await openPage(browser, URL, { width: 800, height: 400 }, 1, mockScripts, EMPTY);
    const r = await page.evaluate(() => {
      const T = window.KrunkJsTok;
      const show = (src, kinds) => { const out = []; T.js(src, 0, src.length, out, kinds); const res = []; for (let i = 0; i < out.length; i += 3) res.push(out[i + 2] + ':' + src.slice(out[i], out[i + 1])); return res; };
      const viewer = { com: 'c', str: 's', kw: 'k', num: 'n' };
      const editor = { com: 'c', str: 's', kw: 'k', num: 'n', re: 'r', tpl: 't' };
      return {
        viewer: show('let a = x / 2 / y; const s = `a ${b}`;', viewer),
        div: show('let a = x / 2 / y;', editor),
        re: show('if (/ab+c/i.test(s)) return /x\\/y[/]/g;', editor),
        tpl: show('const s = `a ${ {k: "v"}.k } b ${`in ${n}`}`; // end', editor),
        member: show('a.return / 2', editor),
      };
    });
    check(r.viewer.join('|') === 'k:let|n:2|k:const|s:`a ${b}`', "the viewer's mode is as before: no regexps, a template is one string (" + r.viewer.join('|') + ')');
    check(r.div.join('|') === 'k:let|n:2', 'a / after a value divides (' + r.div.join('|') + ')');
    check(r.re.join('|') === 'k:if|r:/ab+c/i|k:return|r:/x\\/y[/]/g', 'regular expressions, with escapes and classes (' + r.re.join('|') + ')');
    check(r.tpl.join('|') === 'k:const|t:`a ${|s:"v"|t:} b ${|t:`in ${|t:}`|t:}`|c:// end', 'templates nest; ${…} holds code (' + r.tpl.join('|') + ')');
    check(r.member.join('|') === 'n:2', 'a property named like a keyword is not one (' + r.member.join('|') + ')');
    check(errors.length === 0, 'no console errors (' + errors.join(' | ') + ')');
    await context.close();
  }

  return { testScriptsPage, testScriptEditor, testJsTokenizer };
};
