// Settings page. Every change is sent to the host, which validates it with
// core `Settings::validate`; invalid values are never stored and the field
// errors come back and are shown next to the inputs.
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  let settings = null;

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

  function render() {
    document.querySelectorAll('[data-key]').forEach(function (input) {
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
    $('custom-row').hidden = settings.searchEngine !== 'custom';
    $('discard-row').hidden = !settings.tabDiscardEnabled;
    $('download-dir').textContent = settings.downloadDir || 'Downloads (default)';
    $('default-dir').hidden = !settings.downloadDir;
    LB.applyTheme(settings.theme);
    renderLists();
  }

  function collect() {
    const s = JSON.parse(JSON.stringify(settings));
    document.querySelectorAll('[data-key]').forEach(function (input) {
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
        $('custom-row').hidden = candidate.searchEngine !== 'custom';
      } else {
        $('save-error').textContent = err.message;
        $('save-error').hidden = false;
      }
    });
  }

  // ---------- filter lists and allowlist ----------
  let filterStatus = null;
  function renderLists() {
    const box = $('lists');
    box.textContent = '';
    if (!filterStatus) return;
    filterStatus.lists.forEach(function (l) {
      const cb = el('input', { type: 'checkbox', 'data-list': l.id });
      cb.checked = settings.filterLists.includes(l.id);
      cb.disabled = !settings.adblockEnabled;
      cb.addEventListener('change', function () {
        const lists = Array.from(box.querySelectorAll('input[data-list]')).filter((c) => c.checked).map((c) => c.dataset.list);
        const s = collect();
        s.filterLists = lists;
        save(s);
      });
      const meta = l.source === 'downloaded' && l.fetched ? 'updated ' + new Date(l.fetched).toLocaleString() : 'built-in copy';
      box.appendChild(el('label', { class: 'row toggle' }, [cb, el('span', { text: l.title }), el('span', { class: 'list-meta', text: meta })]));
    });
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
    if (!hosts.length) box.appendChild(el('p', { class: 'help', text: 'None. Use the shield in the toolbar or add a site here.' }));
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
  document.querySelectorAll('[data-key], input[name=startup], input[name=theme]').forEach(function (input) {
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
  $('filter').addEventListener('input', function () {
    const q = this.value.trim().toLowerCase();
    document.querySelectorAll('section.card').forEach(function (sec) {
      sec.hidden = !!q && !(sec.textContent.toLowerCase().includes(q) || (sec.dataset.section || '').includes(q));
    });
  });

  LB.post('ready');
  LB.request('getSettings').then(function (r) {
    settings = r.settings;
    banner(r.restartRequired);
    $('about').textContent = 'LightBrowser ' + r.about.version + ' · WebView2 runtime ' + r.about.runtime + ' · data in ' + r.about.dataDir;
    render();
    loadFilterStatus();
    loadAllowlist();
  });
})();
