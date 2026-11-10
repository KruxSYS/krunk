// krunk://scripts (0.12.0): userscripts and macros, with Krunk's own code editor.
//
// The address picks the view: #userscripts (or nothing) and #macros show the lists, #<id> opens
// that script in the editor, #new-userscript / #new-macro start a new one. Saving first asks the
// host to compile the source (scriptCheck: the engine parses it, nothing runs); a syntax error
// is marked at its line instead of saving. Macro hotkeys are recorded by the host, which also
// says when a chord is taken (by Krunk or by another macro).
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);

  const USERSCRIPT_TEMPLATE = [
    '// ==UserScript==',
    '// @name         New script',
    '// @namespace    krunk',
    '// @version      0.1',
    '// @description  What this script does',
    '// @match        https://example.com/*',
    '// @grant        none',
    '// ==/UserScript==',
    '',
    '(function () {',
    "  'use strict';",
    '',
    '})();',
    '',
  ].join('\n');
  const MACRO_TEMPLATE = "await key.press('a');\n";
  const MAX_IMPORT = 200000;

  let data = { userscripts: [], macros: [], running: null, enabled: { userscripts: true, macros: true } };
  let loaded = false;
  // The script in the editor: {kind, id (null while new), name, hotkey}.
  let ed = null;
  let savedName = '';
  let savedHotkey = null;
  let editor = null;
  let recording = false;
  let saving = false;
  let currentHash = location.hash;

  // ---------------------------------------------------------------- helpers
  function when(ms) {
    if (!ms) return 'Never ran';
    if (ms < 1e11) ms *= 1000; // seconds
    const d = Date.now() - ms;
    if (d < 60000) return 'Ran just now';
    if (d < 3600000) return 'Ran ' + Math.floor(d / 60000) + ' min ago';
    if (d < 86400000) return 'Ran ' + Math.floor(d / 3600000) + ' h ago';
    return 'Ran ' + new Date(ms).toLocaleDateString();
  }
  // "Ctrl+Alt+K" → keycaps (a "+" key is written "Ctrl++").
  function keycaps(chord) {
    let parts = chord.split('+');
    if (chord.endsWith('++')) parts = parts.filter(Boolean).concat('+');
    const box = el('span', { class: 'keycaps', 'aria-label': chord });
    parts.filter(Boolean).forEach(function (p, i) {
      if (i) box.appendChild(el('span', { class: 'plus', 'aria-hidden': 'true', text: '+' }));
      box.appendChild(el('kbd', { text: p }));
    });
    return box;
  }
  function toggle(on, label, onchange) {
    return el('label', { class: 'switch', title: on ? 'Turn off' : 'Turn on' }, [
      el('input', { type: 'checkbox', role: 'switch', 'aria-label': label, checked: on ? true : null, onchange: function (e) { onchange(e.target.checked); } }),
      el('span', { class: 'track', 'aria-hidden': 'true' }),
    ]);
  }
  function button(text, cls, onclick, title) {
    return el('button', { class: 'btn small' + (cls ? ' ' + cls : ''), text: text, title: title || null, onclick: onclick });
  }
  function status(kind, text, error) {
    const s = $(kind === 'macro' ? 'macros-status' : 'userscripts-status');
    s.textContent = text || '';
    s.classList.toggle('error', !!error);
  }
  function failed(kind) { return function (e) { status(kind, e.message, true); }; }

  // ---------------------------------------------------------------- lists
  function load() {
    return LB.request('scriptsList').then(function (r) {
      r = r || {};
      data = {
        userscripts: Array.isArray(r.userscripts) ? r.userscripts : [],
        macros: Array.isArray(r.macros) ? r.macros : [],
        running: r.running || null,
        enabled: Object.assign({ userscripts: true, macros: true }, r.enabled || {}),
      };
      loaded = true;
      renderLists();
    }, failed('userscript'));
  }

  function renderLists() {
    $('userscripts-off').hidden = data.enabled.userscripts !== false;
    $('macros-off').hidden = data.enabled.macros !== false;
    renderUserscripts();
    renderMacros();
    updateRunButtons();
  }

  function renderUserscripts() {
    const box = $('userscripts-list');
    box.textContent = '';
    const list = data.userscripts;
    box.hidden = !list.length;
    $('userscripts-empty').hidden = !loaded || list.length > 0;
    list.forEach(function (s) {
      const matches = s.matches || [];
      const shown = matches.slice(0, 2).join(', ') + (matches.length > 2 ? ' +' + (matches.length - 2) : '');
      const warnings = s.warnings || [];
      box.appendChild(el('div', { class: 'row item' + (s.enabled ? '' : ' off'), 'data-id': s.id }, [
        toggle(s.enabled, (s.enabled ? 'Turn off ' : 'Turn on ') + s.name, function (on) {
          LB.request('scriptToggle', { id: s.id, enabled: on }).then(load, failed('userscript'));
        }),
        el('div', { class: 'info' }, [
          el('div', { class: 'line1' }, [
            el('span', { class: 'name', text: s.name || 'Unnamed script' }),
            s.version ? el('span', { class: 'version', text: 'v' + s.version }) : null,
            s.createdByAi ? el('span', { class: 'tag ai', text: 'created by AI' }) : null,
            warnings.length ? el('span', { class: 'tag warn', title: warnings.join('\n'), text: 'uses features Krunk lacks' }) : null,
          ]),
          el('div', { class: 'sub matches', title: matches.join('\n'), text: matches.length ? 'Runs on ' + shown : 'Runs on no site' }),
        ]),
        el('span', { class: 'when', text: when(s.lastRun) }),
        el('div', { class: 'acts' }, [
          button('Edit', 'edit', function () { location.hash = '#' + s.id; }),
          button('Export', 'export', function () {
            LB.request('scriptExport', { id: s.id }).then(function (r) { if (r && r.saved) Page.toast('Exported'); }, failed('userscript'));
          }, 'Save it as a .user.js file'),
          button('Delete', 'danger delete', function () { remove('userscript', s); }),
        ]),
      ]));
    });
  }

  function renderMacros() {
    const box = $('macros-list');
    box.textContent = '';
    const list = data.macros;
    box.hidden = !list.length;
    $('macros-empty').hidden = !loaded || list.length > 0;
    list.forEach(function (m) {
      const running = data.running === m.id;
      box.appendChild(el('div', { class: 'row item' + (m.enabled ? '' : ' off') + (running ? ' running' : ''), 'data-id': m.id }, [
        toggle(m.enabled, (m.enabled ? 'Turn off ' : 'Turn on ') + m.name, function (on) {
          LB.request('macroToggle', { id: m.id, enabled: on }).then(load, failed('macro'));
        }),
        el('div', { class: 'info' }, [
          el('div', { class: 'line1' }, [
            el('span', { class: 'name', text: m.name || 'Unnamed macro' }),
            m.createdByAi ? el('span', { class: 'tag ai', text: 'created by AI' }) : null,
            running ? el('span', { class: 'tag live', text: 'running' }) : null,
          ]),
        ]),
        el('span', { class: 'hotkey' }, [m.hotkey ? keycaps(m.hotkey) : el('span', { class: 'muted', text: 'No hotkey' })]),
        el('span', { class: 'when', text: when(m.lastRun) }),
        el('div', { class: 'acts' }, [
          running
            ? button('Stop', 'stop', function () { LB.request('macroStop').catch(failed('macro')); })
            : button('Run', 'run', function () { runMacro(m.id); }),
          button('Edit', 'edit', function () { location.hash = '#' + m.id; }),
          button('Delete', 'danger delete', function () { remove('macro', m); }),
        ]),
      ]));
    });
  }

  function runMacro(id) {
    return LB.request('macroRun', { id: id }).catch(function (e) { Page.toast(e.message); });
  }

  function remove(kind, item) {
    const what = kind === 'macro' ? 'macro' : 'userscript';
    Page.confirm('Delete the ' + what + ' "' + (item.name || 'Unnamed') + '"? This cannot be undone.', { ok: 'Delete', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request(kind === 'macro' ? 'macroDelete' : 'scriptDelete', { id: item.id }).then(function () {
        Page.toast('Deleted');
        load();
      }, failed(kind));
    });
  }

  // ---------------------------------------------------------------- import
  $('import-userscript').addEventListener('click', function () { $('import-file').click(); });
  $('import-file').addEventListener('change', function (e) {
    const file = e.target.files && e.target.files[0];
    if (!file) return;
    status('userscript', '');
    if (file.size > MAX_IMPORT) { e.target.value = ''; status('userscript', file.name + ' is too large for a userscript (at most 200 KB).', true); return; }
    file.text().then(function (text) {
      e.target.value = '';
      if (!/\/\/\s*==UserScript==/.test(text)) {
        status('userscript', file.name + ' is not a userscript: it has no "// ==UserScript==" block at the top.', true);
        return;
      }
      return LB.request('scriptSave', { source: text }).then(function (r) {
        r = r || {};
        const warnings = r.warnings || [];
        status('userscript', 'Imported "' + (r.name || file.name) + '".' + (warnings.length ? ' Not supported in Krunk: ' + warnings.join('; ') : ''), warnings.length > 0);
        load();
      });
    }).catch(function (err) { status('userscript', 'Could not import ' + file.name + ': ' + err.message, true); });
  });

  // ---------------------------------------------------------------- views and the address
  function showList(kind) {
    stopRecording();
    ed = null;
    document.body.classList.remove('editing');
    $('editor-view').hidden = true;
    $('list-view').hidden = false;
    $('userscripts-panel').hidden = kind !== 'userscript';
    $('macros-panel').hidden = kind !== 'macro';
    document.querySelectorAll('.kinds a').forEach(function (a) {
      const on = a.dataset.kind === kind;
      a.classList.toggle('active', on);
      if (on) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
    });
    document.title = kind === 'macro' ? 'Macros' : 'Userscripts';
  }

  function route() {
    const h = decodeURIComponent((location.hash || '').replace(/^#/, ''));
    if (!h || h === 'userscripts') return showList('userscript');
    if (h === 'macros') return showList('macro');
    if (h === 'new-userscript') return openEditor({ kind: 'userscript', id: null, name: '', source: USERSCRIPT_TEMPLATE });
    if (h === 'new-macro') return openEditor({ kind: 'macro', id: null, name: '', source: MACRO_TEMPLATE });
    LB.request('scriptGet', { id: h }).then(function (r) {
      if (!r || !r.kind) throw new Error('not found');
      openEditor(r);
    }, function () {
      setHash('userscripts');
      showList('userscript');
      status('userscript', 'That script does not exist any more.', true);
    });
  }

  // Changes the address without opening anything.
  function setHash(h) {
    history.replaceState(null, '', '#' + h);
    currentHash = location.hash;
  }

  window.addEventListener('hashchange', function () {
    if (location.hash === currentHash) return;
    const target = location.hash;
    if (ed && dirty()) {
      // Ask first; the address stays on the editor while the question is open.
      history.replaceState(null, '', currentHash);
      Page.confirm('Leave without saving? Your changes to this script will be lost.', { ok: 'Leave', cancel: 'Keep editing', danger: true }).then(function (yes) {
        if (!yes) { if (editor) editor.focus(); return; }
        history.replaceState(null, '', target);
        currentHash = target;
        route();
      });
      return;
    }
    currentHash = target;
    route();
  });

  $('new-userscript').addEventListener('click', function () { location.hash = '#new-userscript'; });
  $('new-macro').addEventListener('click', function () { location.hash = '#new-macro'; });
  $('ed-back').addEventListener('click', function () { location.hash = ed && ed.kind === 'macro' ? '#macros' : '#userscripts'; });
  document.querySelectorAll('.open-settings').forEach(function (a) {
    a.addEventListener('click', function (e) { e.preventDefault(); LB.post('openPage', { page: 'settings', section: 'ai' }); });
  });

  // ---------------------------------------------------------------- the editor
  function dirty() {
    if (!ed || !editor) return false;
    if (editor.isDirty()) return true;
    return ed.kind === 'macro' && ($('ed-name').value.trim() !== savedName || (ed.hotkey || null) !== savedHotkey);
  }
  function updateDirty() { $('ed-dirty').hidden = !dirty(); }

  function openEditor(r) {
    stopRecording();
    ed = { kind: r.kind === 'macro' ? 'macro' : 'userscript', id: r.id || null, name: r.name || '', hotkey: r.hotkey || null };
    const macro = ed.kind === 'macro';
    savedName = ed.name;
    savedHotkey = ed.hotkey;
    document.body.classList.add('editing');
    $('list-view').hidden = true;
    $('editor-view').hidden = false;
    $('ed-name').hidden = !macro;
    $('ed-name').value = ed.name;
    $('ed-script-name').hidden = macro;
    $('ed-script-name').textContent = ed.name || 'New userscript';
    $('hotkey-row').hidden = !macro;
    $('ed-run-now').hidden = macro;
    $('ed-run').hidden = !macro;
    $('ed-help-userscript').hidden = macro;
    $('ed-help-macro').hidden = !macro;
    setHotkeyStatus('');
    showSaveError('');
    showWarnings(r.warnings || []);
    if (!editor) editor = window.KrunkEditor.create($('ed-box'), { label: 'Script source', onSave: save, onChange: updateDirty });
    editor.setValue(r.source || '');
    renderHotkey();
    updateRunButtons();
    updateDirty();
    document.title = (ed.name || (macro ? 'New macro' : 'New userscript')) + ' · Scripts';
    if (macro && !ed.id) $('ed-name').focus(); else editor.focus();
  }

  function showWarnings(list) {
    const ul = $('ed-warnings');
    ul.textContent = '';
    (list || []).forEach(function (w) { ul.appendChild(el('li', { text: w })); });
    ul.hidden = !(list && list.length);
  }
  function showSaveError(text) { $('ed-save-error').textContent = text || ''; $('ed-save-error').hidden = !text; }

  function updateRunButtons() {
    if (!ed) return;
    const saved = !!ed.id;
    $('ed-run-now').disabled = !saved;
    const run = $('ed-run');
    const running = saved && data.running === ed.id;
    run.disabled = !saved;
    run.textContent = running ? 'Stop' : 'Run';
    run.classList.toggle('running', running);
  }

  // Saves the editor's script: the parse check first, then (macros) the hotkey, then the save.
  function save() {
    if (saving || !ed) return Promise.resolve(false);
    const target = ed;
    const source = editor.getValue();
    const name = $('ed-name').value.trim();
    if (target.kind === 'macro' && !name) {
      showSaveError('Give the macro a name.');
      $('ed-name').focus();
      return Promise.resolve(false);
    }
    saving = true;
    $('ed-save').disabled = true;
    showSaveError('');
    return LB.request('scriptCheck', { source: source, kind: target.kind }).then(function (check) {
      if (ed !== target) return false;
      if (check && check.ok === false) {
        editor.setError(check.line, check.column, check.message);
        return false;
      }
      if (check && Array.isArray(check.warnings)) showWarnings(check.warnings);
      if (target.kind === 'macro') {
        const hotkey = target.hotkey || null;
        const checked = hotkey ? LB.request('hotkeyCheck', { chord: hotkey, id: target.id }) : Promise.resolve(null);
        return checked.then(function (hk) {
          if (hk && hk.conflict) { setHotkeyStatus(hk.conflict, true); return false; }
          const chord = (hk && hk.chord) || hotkey;
          return LB.request('macroSave', { id: target.id, name: name, source: source, hotkey: chord }).then(function (r) {
            target.id = (r && r.id) || target.id;
            target.name = name;
            target.hotkey = chord;
            savedName = name;
            savedHotkey = chord;
            return true;
          });
        });
      }
      return LB.request('scriptSave', { id: target.id, source: source }).then(function (r) {
        r = r || {};
        target.id = r.id || target.id;
        target.name = r.name || target.name;
        if (Array.isArray(r.warnings)) showWarnings(r.warnings);
        $('ed-script-name').textContent = target.name || 'New userscript';
        return true;
      });
    }).then(function (ok) {
      if (!ok || ed !== target) return false;
      editor.markClean();
      if (target.id) setHash(target.id);
      document.title = target.name + ' · Scripts';
      renderHotkey();
      updateDirty();
      updateRunButtons();
      Page.toast('Saved');
      return true;
    }, function (e) {
      showSaveError(e.message);
      return false;
    }).finally(function () {
      saving = false;
      $('ed-save').disabled = false;
    });
  }
  $('ed-save').addEventListener('click', function () { save(); });
  $('ed-name').addEventListener('input', updateDirty);
  document.addEventListener('keydown', function (e) {
    if (!ed || e.defaultPrevented) return;
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 's') { e.preventDefault(); save(); }
  });

  // Unsaved changes are saved first: these run what is stored.
  function savedFirst() { return dirty() || !ed.id ? save() : Promise.resolve(true); }
  $('ed-run-now').addEventListener('click', function () {
    if (!ed) return;
    savedFirst().then(function (ok) {
      if (!ok) return;
      LB.request('scriptRunNow', { id: ed.id }).then(function (r) {
        r = r || {};
        Page.toast(r.ran ? 'Ran on "' + (r.title || 'the page') + '"' : 'It did not run on "' + (r.title || 'the page') + '"');
      }, function (err) { Page.toast(err.message); });
    });
  });
  $('ed-run').addEventListener('click', function () {
    if (!ed) return;
    if (ed.id && data.running === ed.id) { LB.request('macroStop').catch(function (err) { Page.toast(err.message); }); return; }
    savedFirst().then(function (ok) { if (ok) runMacro(ed.id); });
  });

  // ---------------------------------------------------------------- hotkey recording
  function setHotkeyStatus(text, error) {
    const s = $('hotkey-status');
    s.textContent = text || '';
    s.classList.toggle('error', !!error);
  }
  function renderHotkey() {
    if (!ed) return;
    const box = $('hotkey-value');
    box.textContent = '';
    if (recording) box.appendChild(el('span', { class: 'recording', text: 'Press the keys…' }));
    else if (ed.hotkey) box.appendChild(keycaps(ed.hotkey));
    else box.appendChild(el('span', { class: 'muted', text: 'None' }));
    $('hotkey-record').textContent = recording ? 'Stop recording' : 'Record';
    $('hotkey-record').classList.toggle('recording', recording);
    $('hotkey-clear').disabled = recording || !ed.hotkey;
  }
  function startRecording() {
    if (!ed || recording) return;
    recording = true;
    setHotkeyStatus('Hold Ctrl, Alt or Shift and press a key. Esc stops.');
    LB.request('hotkeyRecord', { on: true, id: ed.id || null }).catch(function (e) { stopRecording(); setHotkeyStatus(e.message, true); });
    renderHotkey();
  }
  function stopRecording() {
    if (!recording) return;
    recording = false;
    LB.request('hotkeyRecord', { on: false }).catch(function () { /* the host stops on its own when the page goes */ });
    if (!$('hotkey-status').classList.contains('error')) setHotkeyStatus('');
    renderHotkey();
  }
  $('hotkey-record').addEventListener('click', function () { if (recording) stopRecording(); else startRecording(); });
  $('hotkey-clear').addEventListener('click', function () {
    if (!ed) return;
    ed.hotkey = null;
    setHotkeyStatus('');
    renderHotkey();
    updateDirty();
  });
  LB.on('hotkeyRecorded', function (m) {
    if (!recording || !ed) return;
    if (m.conflict) {
      // Not taken: the recorder keeps listening for another chord.
      setHotkeyStatus(m.conflict, true);
      return;
    }
    if (!m.chord) return;
    ed.hotkey = m.chord;
    setHotkeyStatus('');
    stopRecording();
    updateDirty();
  });
  // Esc, or a click anywhere else, stops recording.
  document.addEventListener('keydown', function (e) {
    if (recording && e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setHotkeyStatus(''); stopRecording(); }
  }, true);
  document.addEventListener('mousedown', function (e) {
    if (recording && !e.target.closest('#hotkey-record')) stopRecording();
  }, true);
  window.addEventListener('blur', stopRecording);

  // ---------------------------------------------------------------- host events
  LB.on('scriptsChanged', Page.debounce(load, 50));
  LB.on('macroState', function (m) {
    data.running = m.running || null;
    renderMacros();
    updateRunButtons();
  });

  route();
  load();
  LB.post('ready');
})();
