// Settings page. Every change is sent to the host, which validates it with
// core `Settings::validate`; invalid values are never stored and the field
// errors come back and are shown next to the inputs. The page is a set of
// tabs (General, Privacy & security, …); the search box shows the matching
// rows of every tab; `#section` in the address picks a tab (deep links).
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  let settings = null;
  let userAgents = [];
  let defaultUserAgent = '';

  // ---------- tabs and deep links ----------
  const TABS = ['general', 'privacy', 'permissions', 'passwords', 'downloads', 'appearance', 'advanced'];
  function tabFromHash() {
    const h = (location.hash || '').replace(/^#/, '').toLowerCase();
    return TABS.includes(h) ? h : 'general';
  }
  function showTab(name) {
    document.querySelectorAll('.panel').forEach((p) => p.classList.toggle('active', p.dataset.tab === name));
    document.querySelectorAll('#tabs a').forEach((a) => {
      a.classList.toggle('active', a.dataset.tab === name);
      if (a.dataset.tab === name) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
    });
    window.scrollTo(0, 0);
    // The per-site answers change while the page is open (prompts in other tabs).
    if (name === 'permissions' && typeof loadPermissions === 'function') loadPermissions();
  }
  window.addEventListener('hashchange', function () { if (!$('filter').value.trim()) showTab(tabFromHash()); });
  showTab(tabFromHash());

  // ---------- search across every tab ----------
  function applyFilter() {
    const q = $('filter').value.trim().toLowerCase();
    const panels = document.querySelector('.panels');
    panels.classList.toggle('searching', !!q);
    let any = false;
    document.querySelectorAll('.panel').forEach(function (panel) {
      let hits = 0;
      panel.querySelectorAll('.row.setting').forEach(function (row) {
        const hay = (row.textContent + ' ' + (row.dataset.keywords || '')).toLowerCase();
        const hit = !q || hay.includes(q);
        row.classList.toggle('filtered-out', !!q && !hit);
        row.hidden = (!!q && !hit) || row.dataset.hiddenByState === '1';
        if (hit) hits++;
      });
      panel.querySelectorAll('.card.group').forEach(function (g) {
        g.hidden = !!q && !g.querySelector('.row.setting:not(.filtered-out)');
      });
      panel.classList.toggle('has-matches', !!q && hits > 0);
      if (hits) any = true;
    });
    $('no-matches').hidden = !q || any;
    if (!q) showTab(tabFromHash());
  }
  // Rows hidden by state (custom search URL, unload minutes, custom UA) stay hidden in a search.
  function stateHidden(id, hidden) {
    const row = $(id);
    row.dataset.hiddenByState = hidden ? '1' : '';
    row.hidden = hidden || row.classList.contains('filtered-out');
  }
  $('filter').addEventListener('input', applyFilter);

  // ---------- reading / writing nested keys ----------
  function getPath(obj, path) {
    return path.split('.').reduce((o, k) => (o == null ? undefined : o[k]), obj);
  }
  function setPath(obj, path, value) {
    const keys = path.split('.');
    let o = obj;
    keys.slice(0, -1).forEach((k) => { o = o[k]; });
    o[keys[keys.length - 1]] = value;
  }

  // The user-agent picker: the saved string is one of the presets, empty (Krunk's own), or
  // custom. Choosing a preset stores its string; "Custom…" keeps the free-text field.
  function presetFor(ua) {
    if (!ua) return '';
    const p = userAgents.find((u) => u.userAgent === ua);
    return p ? p.id : 'custom';
  }
  function renderUserAgent() {
    const sel = $('userAgentPreset');
    if (sel.options.length === 2 && userAgents.length) {
      userAgents.forEach((u) => sel.insertBefore(el('option', { value: u.id, text: u.label }), sel.lastElementChild));
    }
    if (sel !== document.activeElement) sel.value = presetFor(settings.userAgent);
    stateHidden('custom-ua-row', sel.value !== 'custom');
    $('default-ua').textContent = defaultUserAgent;
  }

  // Tracking protection (0.6.0): what Standard and Strict stand for (mirrors core::protection).
  const PROTECTION_PRESETS = {
    standard: { fingerprinting: false, cryptomining: true, thirdPartyCookies: 'trackers', stripParams: true, referrer: 'origin', bounceCleanup: true },
    strict: { fingerprinting: true, cryptomining: true, thirdPartyCookies: 'all', stripParams: true, referrer: 'none', bounceCleanup: true },
  };
  function effectiveProtection() {
    const p = settings.protection || { level: 'standard' };
    return p.level === 'custom' ? p : Object.assign({ level: p.level }, PROTECTION_PRESETS[p.level] || PROTECTION_PRESETS.standard);
  }
  function renderProtection() {
    const p = settings.protection || { level: 'standard' };
    const eff = effectiveProtection();
    document.querySelectorAll('input[name=plevel]').forEach((r) => { r.checked = r.value === p.level; });
    const custom = p.level === 'custom';
    document.querySelectorAll('.protection-part [data-key]').forEach(function (input) {
      const k = input.dataset.key.replace(/^protection\./, '');
      if (input.type === 'checkbox') input.checked = !!eff[k]; else input.value = eff[k];
      input.disabled = !custom;
    });
    document.querySelectorAll('.protection-part').forEach((row) => row.classList.toggle('preset', !custom));
  }
  function loadBounceStatus() {
    LB.request('bounceStatus').then(function (b) {
      const last = b.lastCleanupMs ? new Date(b.lastCleanupMs).toLocaleString() : 'never';
      $('bounce-status').textContent = b.watched + (b.watched === 1 ? ' site' : ' sites') + ' seen only as a redirect · ' + b.cleanedTotal + ' cleared so far · last cleanup ' + last;
    }).catch(function () {});
  }
  $('bounce-clean').addEventListener('click', function () {
    LB.request('bounceCleanNow').then(function (r) {
      Page.toast(r.cleaned ? r.cleaned + (r.cleaned === 1 ? ' site cleared' : ' sites cleared') : 'Nothing to clear');
      loadBounceStatus();
    }, function (e) { Page.toast(e.message); });
  });

  function render() {
    renderProtection();
    document.querySelectorAll('[data-key]').forEach(function (input) {
      // A preset's values are shown but not the user's: they stay as saved.
      if (input.disabled && input.closest('.protection-part')) return;
      // A save reply for another field must not overwrite what the user is
      // typing here; this field is saved on its own change event.
      if (input === document.activeElement) return;
      const v = getPath(settings, input.dataset.key);
      if (input.type === 'checkbox') input.checked = !!v;
      else if (input.tagName === 'SELECT' && input.dataset.type === 'number') {
        // Pick the closest zoom option.
        let best = null;
        Array.from(input.options).forEach((o) => { if (best === null || Math.abs(o.value - v) < Math.abs(best.value - v)) best = o; });
        if (best) input.value = best.value;
      } else input.value = v == null ? '' : v;
    });
    document.querySelectorAll('input[name=startup]').forEach((r) => { r.checked = r.value === settings.startup; });
    document.querySelectorAll('input[name=theme]').forEach((r) => { r.checked = r.value === settings.theme; });
    stateHidden('custom-row', settings.searchEngine !== 'custom');
    stateHidden('new-tab-url-row', settings.newTabPage !== 'custom');
    document.querySelectorAll('#accent-swatches .swatch').forEach((b) => {
      b.classList.toggle('on', (settings.appearance ? settings.appearance.accent : '') === b.dataset.color);
      if (b.dataset.color) b.style.setProperty('--c', b.dataset.color);
    });
    stateHidden('discard-row', !settings.tabDiscardEnabled);
    $('download-dir').textContent = settings.downloadDir || 'Downloads (default)';
    $('default-dir').hidden = !settings.downloadDir;
    renderUserAgent();
    LB.applyTheme(settings.theme, settings.appearance && settings.appearance.accent);
    renderLists();
  }

  function collect() {
    const s = JSON.parse(JSON.stringify(settings));
    document.querySelectorAll('[data-key]').forEach(function (input) {
      // A disabled protection control shows the preset, not a choice to keep.
      if (input.disabled && input.closest('.protection-part')) return;
      let v;
      if (input.type === 'checkbox') v = input.checked;
      else if (input.dataset.type === 'number') v = input.value === '' ? NaN : Number(input.value);
      else v = input.value.trim();
      setPath(s, input.dataset.key, v);
    });
    const startup = document.querySelector('input[name=startup]:checked');
    if (startup) s.startup = startup.value;
    const theme = document.querySelector('input[name=theme]:checked');
    if (theme) s.theme = theme.value;
    const plevel = document.querySelector('input[name=plevel]:checked');
    if (plevel) { s.protection = s.protection || {}; s.protection.level = plevel.value; }
    // Something that is not a whole number becomes an out-of-range value, so
    // the host's validation reports the field (instead of the message failing to parse).
    ['tabSleepMinutes', 'tabDiscardMinutes', 'vaultAutolockMinutes'].forEach((k) => {
      if (!Number.isFinite(s[k]) || s[k] < 0 || !Number.isInteger(s[k]) || s[k] > 99999999) s[k] = 99999999;
    });
    return s;
  }

  function showErrors(errors) {
    document.querySelectorAll('.field-error[data-error-for]').forEach((p) => { p.textContent = ''; });
    document.querySelectorAll('.invalid').forEach((i) => i.classList.remove('invalid'));
    (errors || []).forEach(function (e) {
      const p = document.querySelector('.field-error[data-error-for="' + e.field + '"]');
      if (p) p.textContent = e.message;
      const input = document.querySelector('[data-key="' + e.field + '"]');
      if (input) input.classList.add('invalid');
    });
  }

  function banner(restartRequired) {
    $('restart-banner').hidden = !restartRequired;
  }

  function save(next) {
    const candidate = next || collect();
    return LB.request('saveSettings', { settings: candidate }).then(function (r) {
      settings = r.settings;
      showErrors([]);
      $('save-error').hidden = true;
      banner(r.restartRequired);
      render();
    }, function (err) {
      if (Array.isArray(err.data)) {
        showErrors(err.data);
        // Keep what the user typed so they can fix it; nothing was stored.
        stateHidden('custom-row', candidate.searchEngine !== 'custom');
        stateHidden('new-tab-url-row', candidate.newTabPage !== 'custom');
      } else {
        $('save-error').textContent = err.message;
        $('save-error').hidden = false;
      }
    });
  }

  // ---------- user agent ----------
  $('userAgentPreset').addEventListener('change', function () {
    const v = this.value;
    if (v === 'custom') {
      stateHidden('custom-ua-row', false);
      $('userAgent').focus();
      return;
    }
    const s = collect();
    s.userAgent = v ? (userAgents.find((u) => u.id === v) || {}).userAgent || '' : '';
    stateHidden('custom-ua-row', true);
    save(s);
  });
  $('copy-ua').addEventListener('click', function () {
    const text = defaultUserAgent;
    const done = () => Page.toast('User agent copied');
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, done);
    else done();
  });

  // ---------- filter lists and allowlist ----------
  let filterStatus = null;
  function renderLists() {
    if (!filterStatus) return;
    const on = filterStatus.lists.filter(function (l) { return l.enabled; });
    const added = on.filter(function (l) { return !l.builtin; }).length;
    let text = on.length + ' of ' + filterStatus.lists.length + ' filter lists on';
    if (added) text += ' (' + added + ' added by you)';
    if (filterStatus.userRules) text += ' · ' + filterStatus.userRules + (filterStatus.userRules === 1 ? ' filter of your own' : ' filters of your own');
    $('lists-summary').textContent = text;
    let status = filterStatus.lastCheck ? 'Last checked ' + new Date(filterStatus.lastCheck).toLocaleString() : 'Not updated yet (using the built-in lists)';
    if (filterStatus.lastError) status += ' · last problem: ' + filterStatus.lastError;
    if (filterStatus.busy) status = 'Updating…';
    $('lists-status').textContent = status;
    $('update-lists').disabled = !settings.adblockEnabled || filterStatus.busy;
  }

  function loadFilterStatus() {
    return LB.request('getFilterStatus').then((s) => { filterStatus = s; if (settings) renderLists(); });
  }

  function renderAllowlist(hosts) {
    const box = $('allowlist');
    box.textContent = '';
    if (!hosts.length) box.appendChild(el('p', { class: 'desc', text: 'None. Use the shield in the toolbar or add a site here.' }));
    hosts.forEach(function (h) {
      box.appendChild(el('div', { class: 'row' }, [
        el('span', { class: 'main' }, [el('span', { class: 'name', text: h })]),
        el('span', { class: 'actions' }, [Page.iconButton('close', 'Remove ' + h, function () {
          LB.request('allowlistRemove', { host: h }).then(loadAllowlist);
        })]),
      ]));
    });
  }
  function loadAllowlist() { return LB.request('getAllowlist').then(renderAllowlist); }

  $('allow-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const host = $('allow-host').value.trim();
    if (!host) return;
    LB.request('allowlistAdd', { host: host }).then(function () {
      $('allow-host').value = '';
      $('allow-error').textContent = '';
      loadAllowlist();
    }, function (err) { $('allow-error').textContent = err.message; });
  });

  $('update-lists').addEventListener('click', function () {
    $('update-lists').disabled = true;
    $('lists-status').textContent = 'Updating…';
    LB.request('updateFilterLists').then(function (s) { filterStatus = s; renderLists(); },
      function (err) { $('lists-status').textContent = err.message; $('update-lists').disabled = false; });
  });

  // ---------- other controls ----------
  document.querySelectorAll('#accent-swatches .swatch').forEach(function (b) {
    b.addEventListener('click', function () {
      const next = collect();
      next.appearance.accent = b.dataset.color;
      save(next);
    });
  });
  document.querySelectorAll('[data-key], input[name=startup], input[name=theme], input[name=plevel]').forEach(function (input) {
    input.addEventListener('change', function () { save(); });
  });
  $('pick-dir').addEventListener('click', function () {
    LB.request('pickDownloadFolder').then(function (dir) {
      if (!dir) return;
      const s = collect();
      s.downloadDir = dir;
      save(s);
    });
  });
  $('default-dir').addEventListener('click', function () {
    const s = collect();
    s.downloadDir = '';
    save(s);
  });
  $('restart').addEventListener('click', function () { LB.post('restart'); });
  $('reset').addEventListener('click', function () {
    Page.confirm('Reset all settings to their defaults? History, bookmarks and passwords are kept.', { ok: 'Reset', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('resetSettings').then(function (r) {
        settings = r.settings;
        showErrors([]);
        banner(r.restartRequired);
        render();
        Page.toast('Settings reset');
      });
    });
  });

  // ---------- updates (Advanced → Updates) ----------
  let updateStatus = null;
  function renderUpdates() {
    const u = updateStatus;
    if (!u) return;
    if (u.server) $('update-server').textContent = u.server;
    let text;
    if (u.configError) text = 'Update checks are not available in this build: ' + u.configError;
    else if (u.checking) text = 'Checking…';
    else if (u.downloading !== null && u.downloading !== undefined) text = 'Downloading Krunk ' + u.available.version + '… ' + u.downloading + '%';
    else if (u.failed) text = 'The update failed: ' + u.failed;
    else if (u.available) text = 'Krunk ' + u.available.version + ' is available' + (u.available.install ? '' : ' (download it from the website)');
    else if (u.lastCheck) text = u.lastResult + ' · last checked ' + new Date(u.lastCheck).toLocaleString();
    else text = 'Not checked yet';
    $('update-status').textContent = text;
    $('check-updates').disabled = !!u.checking || !!u.configError;
    const install = $('install-update');
    install.hidden = !u.available || (u.downloading !== null && u.downloading !== undefined);
    if (u.available) install.textContent = u.available.install ? (u.failed ? 'Try again' : 'Update now') : 'Download';
  }
  function loadUpdates() {
    return LB.request('getUpdateStatus').then(function (u) { updateStatus = u; renderUpdates(); });
  }
  $('check-updates').addEventListener('click', function () {
    if (updateStatus) { updateStatus.checking = true; renderUpdates(); }
    LB.request('checkForUpdates').then(function (u) { updateStatus = u; renderUpdates(); },
      function (err) { $('update-status').textContent = err.message; loadUpdates(); });
  });
  $('install-update').addEventListener('click', function () {
    LB.post('updateReply', { choice: 'install' });
    setTimeout(loadUpdates, 300);
  });
  // A check or download in progress (also one started from the prompt): keep the line current.
  let updatePoll = 0;
  setInterval(function () {
    const u = updateStatus;
    if (!u) return;
    const busy = u.checking || (u.downloading !== null && u.downloading !== undefined);
    if (busy || (u.available && ++updatePoll % 3 === 0)) loadUpdates();
  }, 1000);

  // ---------- import (Passwords & import) ----------
  // Filled in by the host's import support; without it the section explains itself.
  function renderImportProfiles(profiles) {
    const box = $('import-profiles');
    box.textContent = '';
    if (!profiles || !profiles.length) {
      box.appendChild(el('p', { class: 'desc', text: 'No Firefox, Chrome, Edge or Brave profiles were found for this Windows user.' }));
      return;
    }
    profiles.forEach(function (p) {
      // A type the profile has no file for is offered unchecked and greyed.
      const has = { history: p.hasHistory !== false, bookmarks: p.hasBookmarks !== false, passwords: p.hasPasswords !== false };
      const types = el('span', { class: 'types' }, ['history', 'bookmarks', 'passwords'].map((t) => el('label', { class: has[t] ? '' : 'off' }, [
        el('input', { type: 'checkbox', checked: has[t], 'data-type': t }), t,
      ])));
      const pw = p.browser === 'firefox' ? el('input', { type: 'password', placeholder: 'Primary password (if set)', 'aria-label': 'Firefox primary password' }) : null;
      const btn = el('button', { class: 'btn', text: 'Import' });
      const row = el('div', { class: 'row', 'data-profile': p.id }, [
        el('span', { class: 'main' }, [el('span', { class: 'name', text: p.label + (p.isDefault ? ' (default)' : '') }), el('span', { class: 'sub', text: p.path, title: p.path })]),
        types, pw, btn,
      ]);
      btn.addEventListener('click', function () {
        const chosen = Array.from(types.querySelectorAll('input:checked')).map((i) => i.dataset.type);
        btn.disabled = true;
        $('import-result').hidden = true;
        LB.request('importRun', { profile: p.id, types: chosen, primaryPassword: pw ? pw.value : null }).then(function (r) {
          btn.disabled = false;
          if (pw) pw.value = '';
          showImportResult(r);
        }, function (err) { btn.disabled = false; showImportResult(null, err.message); });
      });
      box.appendChild(row);
    });
  }
  function showImportResult(r, error) {
    const p = $('import-result');
    p.hidden = false;
    if (error) { p.textContent = 'Import failed: ' + error; return; }
    const parts = [];
    ['history', 'bookmarks', 'passwords'].forEach(function (t) {
      const c = r[t];
      if (!c) return;
      if (c.error) { parts.push(t + ': failed (' + c.error + ')'); return; }
      let s = t + ': ' + c.imported + ' imported';
      if (c.updated) s += ', ' + c.updated + ' updated';
      if (c.merged) s += ', ' + c.merged + ' already there';
      if (c.skipped) s += ', ' + c.skipped + ' skipped';
      if (c.appBound) s += ' (' + c.appBound + ' protected by Chrome, export a CSV for those)';
      parts.push(s);
    });
    p.textContent = parts.length ? parts.join(' · ') : 'Nothing to import.';
  }
  $('import-csv').addEventListener('click', function () {
    $('import-result').hidden = true;
    LB.request('importCsv').then(function (r) { if (r) showImportResult(r); }, function (err) { showImportResult(null, err.message); });
  });
  // 0.6.0: the per-site permission answers.
  const PERM_LABEL = { camera: 'Camera', microphone: 'Microphone', geolocation: 'Location', notifications: 'Notifications', clipboardRead: 'Clipboard (read)', midi: 'MIDI devices', screenShare: 'Screen sharing', localNetwork: 'Local network' };
  function loadPermissions() {
    return LB.request('permissionExceptions').then(function (list) {
      const body = $('perm-rows');
      body.textContent = '';
      if (!list.length) { body.appendChild(el('tr', {}, [el('td', { colspan: '4', class: 'muted', text: 'No sites yet.' })])); return; }
      list.forEach(function (p) {
        const sel = el('select', {}, [el('option', { value: 'allow', text: 'Allow' }), el('option', { value: 'block', text: 'Block' })]);
        sel.value = p.allow ? 'allow' : 'block';
        sel.addEventListener('change', function () { LB.request('permissionSet', { origin: p.origin, kind: p.kind, state: sel.value }).then(loadPermissions); });
        body.appendChild(el('tr', { 'data-origin': p.origin, 'data-kind': p.kind }, [
          el('td', { text: p.origin }), el('td', { text: PERM_LABEL[p.kind] || p.kind }), el('td', {}, [sel]),
          el('td', {}, [el('button', { class: 'btn small', text: 'Remove', onclick: function () { LB.request('permissionSet', { origin: p.origin, kind: p.kind, state: 'ask' }).then(loadPermissions); } })]),
        ]));
      });
    }).catch(function () {});
  }
  if (tabFromHash() !== 'permissions') loadPermissions(); // else showTab() loaded them

  function loadImportProfiles() {
    LB.request('importDetect').then(renderImportProfiles, function () { renderImportProfiles([]); });
  }

  LB.post('ready');
  LB.request('getSettings').then(function (r) {
    settings = r.settings;
    userAgents = r.userAgents || [];
    defaultUserAgent = r.defaultUserAgent || '';
    banner(r.restartRequired);
    $('about').textContent = 'Krunk ' + r.about.version + ' · Chromium ' + r.about.chromium + ' (CEF ' + r.about.cef + ')' + ' · data in ' + r.about.dataDir;
    render();
    loadFilterStatus();
    loadAllowlist();
    loadBounceStatus();
    loadUpdates();
    loadImportProfiles();
  });
})();
