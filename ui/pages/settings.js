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
  // 0.6.0 request headers: client hints, custom rules and the read-only list.
  const READ_ONLY_HEADERS = [
    ['Host', 'set by the network stack from the address'], ['Connection', 'managed by the network stack'],
    ['Content-Length', 'computed from the request body'], ['Accept-Encoding', 'the engine negotiates compression itself'],
    ['Cookie', 'the cookie jar (see Cookies and site data)'], ['Referer', 'the tracking-protection level decides what other sites learn'],
    ['Origin', 'security-relevant: set by the engine'], ['DNT', 'Settings → Privacy → Do Not Track'], ['Sec-GPC', 'Settings → Privacy → Global Privacy Control'],
    ['Sec-Fetch-Site / -Mode / -Dest / -User', 'added by the network stack after Krunk\'s hook'], ['Priority', 'added by the network stack after Krunk\'s hook'],
    ['Upgrade-Insecure-Requests', 'added by the engine for navigations'], ['Sec-CH-UA*', 'edited through the client hints above'],
  ];
  // The engine's own values (the host reads them from the shell page; a fallback until then).
  let HINT_DEFAULTS = { brands: [{ brand: 'Not:A-Brand', version: '8' }, { brand: 'Chromium', version: '156' }], fullVersion: '', platform: 'Windows', platformVersion: '15.0.0', architecture: 'x86', bitness: '64', model: '', mobile: false, wow64: false };
  const HINT_FIELDS = ['fullVersion', 'platform', 'platformVersion', 'architecture', 'bitness', 'model'];
  function brandsText(brands) { return (brands || []).map((b) => '"' + b.brand + '";v="' + b.version + '"').join(', '); }
  function parseBrands(text) {
    return text.split(',').map((part) => {
      const m = part.match(/^\s*"?([^";]+?)"?\s*;\s*(?:v=)?"?([^"]+?)"?\s*$/);
      return m ? { brand: m[1].trim(), version: m[2].trim() } : null;
    }).filter(Boolean);
  }
  function renderHeaders() {
    const h = settings.headers || { acceptLanguage: '', hints: null, rules: [] };
    const on = !!h.hints;
    if ($('hints-on') !== document.activeElement) $('hints-on').checked = on;
    const src = h.hints || HINT_DEFAULTS;
    if (!$('hint-fields').contains(document.activeElement)) {
      $('hint-brands').value = brandsText(src.brands);
      HINT_FIELDS.forEach((k) => { $('hint-' + k).value = src[k] || ''; });
      $('hint-mobile').checked = !!src.mobile;
      $('hint-wow64').checked = !!src.wow64;
    }
    // Nothing to customise while no hints are sent (0.7.0).
    const sending = h.sendClientHints !== false;
    $('hints-on').disabled = !sending;
    stateHidden('hint-fields', !on || !sending);
    const list = $('header-rules');
    list.textContent = '';
    if (!(h.rules || []).length) list.appendChild(el('p', { class: 'muted', text: 'No custom headers.' }));
    (h.rules || []).forEach(function (r, i) {
      list.appendChild(el('div', { class: 'row rule', 'data-name': r.name }, [
        el('span', { class: 'name', text: r.name }),
        el('span', { class: 'value', text: r.value === null || r.value === undefined ? '(removed)' : r.value }),
        r.sites && r.sites.length ? el('span', { class: 'tag', text: r.sites.join(', ') }) : null,
        Page.iconButton('trash', 'Remove ' + r.name, function () {
          const s = collect();
          s.headers.rules = s.headers.rules.filter((_, j) => j !== i);
          save(s);
        }),
      ]));
    });
    if (!$('readonly-headers').childElementCount) {
      READ_ONLY_HEADERS.forEach(([n, why]) => {
        $('readonly-headers').appendChild(el('span', { class: 'name', text: n }));
        $('readonly-headers').appendChild(el('span', { class: 'why', text: why }));
      });
    }
  }
  function collectHints(s) {
    s.headers = s.headers || { acceptLanguage: '', hints: null, rules: [] };
    if (!$('hints-on').checked) { s.headers.hints = null; return; }
    const prev = (settings.headers && settings.headers.hints) || HINT_DEFAULTS;
    const hints = Object.assign({}, prev, { brands: parseBrands($('hint-brands').value), mobile: $('hint-mobile').checked, wow64: $('hint-wow64').checked });
    HINT_FIELDS.forEach((k) => { hints[k] = $('hint-' + k).value.trim(); });
    s.headers.hints = hints;
  }
  $('hints-on').addEventListener('change', function () { save(); });
  document.querySelectorAll('#hint-fields input').forEach((i) => i.addEventListener('change', function () { save(); }));
  $('header-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const name = $('header-name').value.trim();
    const value = $('header-value').value;
    const sites = $('header-sites').value.split(',').map((x) => x.trim()).filter(Boolean);
    $('header-error').textContent = '';
    if (!name) { $('header-error').textContent = 'Enter a header name'; return; }
    const s = collect();
    s.headers.rules = (s.headers.rules || []).concat([{ name: name, value: value === '' ? null : value, sites: sites }]);
    save(s).then(function (ok) {
      if (ok === false) return;
      const err = document.querySelector('[data-error-for="headers.rules"]');
      if (err && err.textContent) { $('header-error').textContent = err.textContent; return; }
      $('header-name').value = ''; $('header-value').value = ''; $('header-sites').value = '';
    });
  });
  $('headers-reset').addEventListener('click', function () {
    const s = collect();
    s.userAgent = '';
    s.headers = { acceptLanguage: '', hints: null, rules: [] };
    save(s).then(function () { Page.toast('Request headers reset'); });
  });

  // 0.6.0 custom DNS: the mode, the ordered list with its counters, presets and Test.
  let dnsPresets = [];
  let dnsStatus = null;
  let dnsTests = {};
  function renderDns() {
    const d = settings.dns || { mode: 'system', resolvers: [] };
    document.querySelectorAll('input[name=dnsmode]').forEach((r) => { r.checked = r.value === d.mode; });
    const sel = $('dns-preset');
    if (sel.options.length === 1 && dnsPresets.length) dnsPresets.forEach((p) => sel.appendChild(el('option', { value: p.id, text: p.name })));
    const list = $('dns-list');
    list.textContent = '';
    if (!d.resolvers.length) list.appendChild(el('p', { class: 'muted', text: 'No resolvers yet: pick a preset or enter an address.' }));
    let usable = 0;
    d.resolvers.forEach(function (r, i) {
      const supported = r.kind !== 'dnsCrypt';
      const idx = supported ? usable++ : -1;
      const st = dnsStatus && dnsStatus.resolvers && idx >= 0 ? dnsStatus.resolvers[idx] : null;
      const parts = [];
      if (st && st.queries) parts.push(st.queries + (st.queries === 1 ? ' query' : ' queries') + (st.failures ? ', ' + st.failures + ' failed' : '') + ', last ' + st.lastMs + ' ms');
      if (st && st.lastError) parts.push(st.lastError);
      if (dnsTests[i]) parts.push(dnsTests[i]);
      const move = (from, to) => { const s = collect(); const list = s.dns.resolvers; const [x] = list.splice(from, 1); list.splice(to, 0, x); save(s); };
      list.appendChild(el('div', { class: 'row resolver' + (supported ? '' : ' off'), 'data-index': String(i) }, [
        el('span', { class: 'tag kind', text: KIND_LABEL[r.kind] || r.kind }),
        el('div', { class: 'main' }, [
          el('span', { class: 'name', text: r.name || address(r) }),
          el('span', { class: 'sub', text: (r.name && r.name !== address(r) ? address(r) + ' · ' : '') + (r.ip ? 'via ' + r.ip : (supported && !/^\d|^\[/.test(r.host) ? 'name looked up once' : '')) }),
          parts.length ? el('span', { class: 'sub status', text: parts.join(' · ') }) : null,
        ]),
        supported ? el('button', { class: 'btn small test', text: 'Test', onclick: function () {
          dnsTests[i] = 'testing…'; renderDns();
          LB.request('dnsTest', { index: idx }).then(function (res) { dnsTests[i] = 'OK, ' + res.ms + ' ms'; renderDns(); }, function (e) { dnsTests[i] = 'failed: ' + e.message; renderDns(); });
        } }) : el('span', { class: 'sub', text: 'not supported' }),
        Page.iconButton('up', 'Move up', function () { if (i > 0) move(i, i - 1); }),
        Page.iconButton('down', 'Move down', function () { if (i + 1 < d.resolvers.length) move(i, i + 1); }),
        Page.iconButton('trash', 'Remove ' + (r.name || address(r)), function () { const s = collect(); s.dns.resolvers.splice(i, 1); save(s); }),
      ]));
    });
    const status = $('dns-status');
    if (d.mode !== 'custom') status.textContent = '';
    else if (!dnsStatus) status.textContent = '';
    else if (dnsStatus.route === 'stub') status.textContent = 'Active: the engine asks Krunk\'s loopback stub on port ' + dnsStatus.port + ', which asks the list below.';
    else if (dnsStatus.route === 'direct') status.textContent = 'Active: the engine asks these DoH resolvers directly.';
    else status.textContent = 'Not active yet.';
  }
  const KIND_LABEL = { udp: 'UDP', tcp: 'TCP', tls: 'DoT', https: 'DoH', h3: 'DoH3', quic: 'DoQ', dnsCrypt: 'DNSCrypt' };
  function address(r) {
    const port = { udp: 53, tcp: 53, tls: 853, quic: 853, https: 443, h3: 443, dnsCrypt: 443 }[r.kind];
    const host = (r.host.indexOf(':') >= 0 ? '[' + r.host + ']' : r.host) + (r.port && r.port !== port ? ':' + r.port : '');
    if (r.kind === 'udp') return host;
    if (r.kind === 'https' || r.kind === 'h3') return r.kind + '://' + host + (r.path || '/dns-query');
    return r.kind + '://' + host;
  }
  function loadDnsStatus() {
    LB.request('dnsStatus').then(function (s) { dnsStatus = s; renderDns(); }).catch(function () {});
  }
  document.querySelectorAll('input[name=dnsmode]').forEach((r) => r.addEventListener('change', function () { save().then(loadDnsStatus); }));
  $('dns-preset').addEventListener('change', function () {
    const p = dnsPresets.find((x) => x.id === $('dns-preset').value);
    $('dns-preset').value = '';
    if (!p) return;
    const s = collect();
    s.dns.resolvers = (s.dns.resolvers || []).concat([p.resolver]);
    save(s).then(loadDnsStatus);
  });
  $('dns-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const text = $('dns-address').value.trim();
    $('dns-error').textContent = '';
    if (!text) { $('dns-error').textContent = 'Enter a resolver'; return; }
    LB.request('dnsParse', { text: text }).then(function (r) {
      const s = collect();
      s.dns.resolvers = (s.dns.resolvers || []).concat([r]);
      return save(s).then(function () {
        const err = document.querySelector('[data-error-for="dns"]');
        if (err && err.textContent) { $('dns-error').textContent = err.textContent; return; }
        $('dns-address').value = '';
        loadDnsStatus();
      });
    }, function (err) { $('dns-error').textContent = err.message; });
  });

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
    pickers.forEach((p) => p.sync());
    renderWallpapers();
    stateHidden('discard-row', !settings.tabDiscardEnabled);
    $('download-dir').textContent = settings.downloadDir || 'Downloads (default)';
    $('default-dir').hidden = !settings.downloadDir;
    renderUserAgent();
    renderHeaders();
    renderDns();
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
    const dnsmode = document.querySelector('input[name=dnsmode]:checked');
    s.dns = s.dns || { mode: 'system', resolvers: [] };
    if (dnsmode) s.dns.mode = dnsmode.value;
    collectHints(s);
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

  // ---------- colours and the new tab page background (0.7.0, R2/R3) ----------
  // Real colour pickers: the dialog, the hex field, presets, reset; changes apply live.
  const PRESETS = {
    accent: ['', '#2563eb', '#7c3aed', '#db2777', '#dc2626', '#ea580c', '#16a34a', '#0d9488'],
    surface: ['', '#ffffff', '#f3f4f6', '#e0e7ff', '#1f2023', '#111827', '#0f172a', '#1e1b4b'],
  };
  const pickers = Array.from(document.querySelectorAll('input[data-picker]')).map(function (input) {
    return ColorPicker.attach(input, { presets: PRESETS[input.dataset.picker] || [], label: input.closest('.row').querySelector('label, .label').textContent });
  });
  // A colour for the new tab page also makes it the background.
  $('ntp.backgroundColor').addEventListener('change', function () {
    const next = collect();
    next.ntp.background = next.ntp.backgroundColor ? 'color' : (next.ntp.background === 'color' ? 'none' : next.ntp.background);
    save(next);
  });
  let wallpapers = [];
  function renderWallpapers() {
    const box = $('settings-wallpapers');
    box.textContent = '';
    const n = settings.ntp || {};
    wallpapers.forEach(function (w) {
      const on = n.background === 'wallpaper' && n.wallpaper === w.id;
      const b = LB.el('button', { type: 'button', class: 'wallpaper' + (on ? ' on' : ''), title: w.title, 'aria-label': w.title, 'aria-pressed': on ? 'true' : 'false', 'data-id': w.id },
        [LB.el('img', { src: w.thumb, alt: '' })]);
      b.addEventListener('click', function () {
        const next = collect();
        Object.assign(next.ntp, { background: 'wallpaper', wallpaper: w.id, imageDim: w.dim, imageBlur: 0 });
        save(next);
      });
      box.appendChild(b);
    });
    $('ntp-image-desc').textContent = n.background === 'image' ? 'In use: your picture.' : n.image ? 'Chosen before (not in use).' : 'A picture from your computer.';
  }
  $('ntp-pick-image').addEventListener('click', function () {
    LB.request('ntpPickImage').then(function () { return LB.request('getSettings'); }).then(function (r) { settings = r.settings; render(); },
      function (e) { if (e && e.message) Page.toast(e.message); });
  });
  $('ntp-no-background').addEventListener('click', function () {
    const next = collect();
    next.ntp.background = 'none';
    save(next);
  });

  // ---------- other controls ----------
  document.querySelectorAll('[data-key], input[name=startup], input[name=theme], input[name=plevel]').forEach(function (input) {
    // (Fields with their own save: the new tab page colour also switches the background.)
    if (input.dataset.ownSave !== undefined) return;
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
      // 0.7.0 (R25): why entries were skipped.
      if (c.skipped) s += ', ' + c.skipped + ' skipped' + (c.skippedReasons && c.skippedReasons.length ? ' (' + c.skippedReasons.map((x) => x.reason + ': ' + x.count).join(', ') + ')' : '');
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
    if (r.engineHints) HINT_DEFAULTS = r.engineHints;
    $('cdm-status').textContent = r.cdm ? 'Widevine module ' + r.cdm + ' is installed.' : 'Widevine module: not downloaded yet.';
    dnsPresets = r.dnsPresets || [];
    wallpapers = r.wallpapers || [];
    banner(r.restartRequired);
    $('about').textContent = 'Krunk ' + r.about.version + ' · Chromium ' + r.about.chromium + ' (CEF ' + r.about.cef + ')' + ' · data in ' + r.about.dataDir;
    render();
    loadFilterStatus();
    loadAllowlist();
    loadBounceStatus();
    loadDnsStatus();
    loadUpdates();
    loadImportProfiles();
  });
})();
