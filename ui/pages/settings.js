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
  const TABS = ['account', 'general', 'privacy', 'permissions', 'passwords', 'downloads', 'appearance', 'ai', 'advanced'];
  // Sections inside a tab (0.14.0: the VPN popup's "VPN settings" opens #vpn).
  const SECTIONS = { vpn: ['privacy', 'vpn-card'] };
  function tabFromHash() {
    const h = (location.hash || '').replace(/^#/, '').toLowerCase();
    if (SECTIONS[h]) return SECTIONS[h][0];
    return TABS.includes(h) ? h : 'general';
  }
  function showTab(name) {
    document.querySelectorAll('.panel').forEach((p) => p.classList.toggle('active', p.dataset.tab === name));
    document.querySelectorAll('#tabs a').forEach((a) => {
      a.classList.toggle('active', a.dataset.tab === name);
      if (a.dataset.tab === name) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
    });
    window.scrollTo(0, 0);
    const section = SECTIONS[(location.hash || '').replace(/^#/, '').toLowerCase()];
    if (section && section[0] === name) {
      const card = $(section[1]);
      if (card) requestAnimationFrame(function () { card.scrollIntoView({ block: 'start' }); });
    }
    // The per-site answers change while the page is open (prompts in other tabs).
    if (name === 'permissions' && typeof loadPermissions === 'function') loadPermissions();
    // The Account card on screen: a signed-in device may check with the server (0.8.0).
    if (name === 'account') LB.post('accountState', { check: true });
  }
  window.addEventListener('hashchange', function () { if (!$('filter').value.trim()) showTab(tabFromHash()); });
  showTab(tabFromHash());

  // ---------- search across every tab ----------
  function applyFilter() {
    const q = $('filter').value.trim().toLowerCase();
    // 0.12.0: a word of one or two letters ("AI") matches where a word starts, not inside
    // words ("email"); longer queries match anywhere, as before.
    const short = q.length > 0 && q.length < 3 ? new RegExp('(^|[^a-z0-9])' + q.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')) : null;
    const panels = document.querySelector('.panels');
    panels.classList.toggle('searching', !!q);
    let any = false;
    document.querySelectorAll('.panel').forEach(function (panel) {
      let hits = 0;
      panel.querySelectorAll('.row.setting').forEach(function (row) {
        const hay = (row.textContent + ' ' + (row.dataset.keywords || '')).toLowerCase();
        const hit = !q || (short ? short.test(hay) : hay.includes(q));
        row.classList.toggle('filtered-out', !!q && !hit);
        row.hidden = (!!q && !hit) || row.dataset.hiddenByState === '1';
        if (hit && row.dataset.hiddenByState !== '1') hits++;
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

  // ---------- VPN (0.14.0) ----------
  // The host reads the imported file itself (native dialog) and keeps the profiles; the page
  // shows the `vpn` view and sends commands. A password goes to the host once and is never
  // shown or sent back: the field is cleared as soon as it is read.
  const VPN_KIND = { wireguard: 'WireGuard', openvpn: 'OpenVPN' };
  const VPN_STATE = { connected: 'Connected', connecting: 'Connecting…', reconnecting: 'Reconnecting…', failed: 'Not connected', stopped: 'Off' };
  let vpnView = null;
  let vpnOpen = { id: '', what: '' }; // the profile whose rename or credentials form is open
  let vpnSig = '';
  function vpnError(e) { return (e && e.message) || 'Failed'; }
  function vpnBytes(n) {
    if (!(n > 0)) return '0 B';
    const u = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.min(u.length - 1, Math.floor(Math.log(n) / Math.log(1024)));
    return (i === 0 ? n : (n / Math.pow(1024, i)).toFixed(1)) + ' ' + u[i];
  }
  function vpnSet(v) { if (v && Array.isArray(v.profiles)) vpnView = v; renderVpn(); }
  function vpnAsk(cmd, args) {
    $('vpn-error').textContent = '';
    return LB.request(cmd, args).then(function (v) { vpnSet(v); return true; }, function (e) { $('vpn-error').textContent = vpnError(e); return false; });
  }
  function renderVpn() {
    const v = vpnView;
    if (!v) return;
    const st = (v.status && v.status.state) || 'stopped';
    const engaged = st !== 'stopped';
    const active = v.profiles.find((p) => p.id === v.active) || null;
    // The status line.
    let line = '';
    if (engaged) {
      line = VPN_STATE[st] + (active ? ' · ' + active.name : '');
      if (st === 'connected') line += (v.status.tunnelIp ? ' · ' + v.status.tunnelIp : '') + ' · ' + vpnBytes(v.status.rxBytes) + ' received, ' + vpnBytes(v.status.txBytes) + ' sent';
      if (v.status.error && (st === 'failed' || st === 'reconnecting')) line += ' · ' + v.status.error;
      if (st === 'failed') line += '. Krunk\'s connections are stopped until it connects or you disconnect.';
    } else line = v.profiles.length ? 'Off: Krunk connects directly.' : '';
    $('vpn-status').textContent = line;
    $('vpn-status').classList.toggle('bad', st === 'failed');
    $('vpn-auto-desc').textContent = active
      ? 'To the profile used last (' + active.name + '), before any page loads.'
      : 'To the profile used last, before any page loads. Connect to one first.';
    // The list (rebuilt only when what it shows changed: an open form keeps what is typed).
    const sig = JSON.stringify([v.profiles, v.active, st, vpnOpen]);
    if (sig === vpnSig) return;
    vpnSig = sig;
    const box = $('vpn-profiles');
    box.textContent = '';
    if (!v.profiles.length) box.appendChild(el('p', { class: 'muted', text: 'No profiles yet: import a file from your VPN provider.' }));
    v.profiles.forEach(function (p) {
      const running = engaged && p.id === v.active;
      const needs = p.needsCredentials && !p.hasCredentials;
      const sub = [VPN_KIND[p.kind] || p.kind, p.server].concat(p.needsCredentials ? [p.hasCredentials ? 'user name and password saved' : 'needs a user name and password'] : []);
      box.appendChild(el('div', { class: 'row vpn-profile' + (running ? ' running' : ''), 'data-id': p.id }, [
        el('div', { class: 'main' }, [
          el('span', { class: 'name', text: p.name }),
          el('span', { class: 'sub', text: sub.join(' · ') }),
        ]),
        running ? el('span', { class: 'vpn-badge ' + st, text: VPN_STATE[st] }) : null,
        running
          ? el('button', { type: 'button', class: 'btn small vpn-disconnect', text: 'Disconnect', onclick: function () { vpnAsk('vpnDisconnect'); } })
          : el('button', { type: 'button', class: 'btn small primary vpn-connect', text: 'Connect', disabled: needs, title: needs ? 'Enter the user name and password first' : null, 'aria-label': 'Connect to ' + p.name,
            onclick: function () { vpnAsk('vpnConnect', { id: p.id }); } }),
        el('button', { type: 'button', class: 'btn small vpn-rename', text: 'Rename', 'aria-label': 'Rename ' + p.name, onclick: function () { vpnToggle(p.id, 'rename'); } }),
        p.kind === 'openvpn' ? el('button', { type: 'button', class: 'btn small vpn-creds', text: p.hasCredentials ? 'Change password' : 'User name and password', onclick: function () { vpnToggle(p.id, 'credentials'); } }) : null,
        el('button', { type: 'button', class: 'btn small danger vpn-remove', text: 'Remove', 'aria-label': 'Remove ' + p.name, onclick: function () { vpnRemove(p); } }),
      ]));
      if (vpnOpen.id === p.id) {
        const form = vpnOpen.what === 'rename' ? vpnRenameForm(p) : vpnCredentialsForm(p);
        box.appendChild(form);
        form.querySelector('input').focus();
      }
    });
  }
  function vpnToggle(id, what) {
    vpnOpen = vpnOpen.id === id && vpnOpen.what === what ? { id: '', what: '' } : { id: id, what: what };
    renderVpn();
  }
  function vpnClose() { vpnOpen = { id: '', what: '' }; renderVpn(); }
  function vpnRenameForm(p) {
    const input = el('input', { type: 'text', class: 'vpn-name-input', maxlength: '60', spellcheck: 'false', value: p.name, 'aria-label': 'New name for ' + p.name });
    const err = el('p', { class: 'field-error' });
    const form = el('form', { class: 'inline-form vpn-form', autocomplete: 'off', 'data-for': p.id }, [
      input,
      el('button', { type: 'submit', class: 'btn small primary', text: 'Save' }),
      el('button', { type: 'button', class: 'btn small', text: 'Cancel', onclick: vpnClose }),
      err,
    ]);
    form.addEventListener('submit', function (e) {
      e.preventDefault();
      const name = input.value.trim();
      if (!name) { err.textContent = 'Enter a name'; return; }
      LB.request('vpnRename', { id: p.id, name: name }).then(function (v) { vpnOpen = { id: '', what: '' }; vpnSet(v); }, function (x) { err.textContent = vpnError(x); });
    });
    return form;
  }
  function vpnCredentialsForm(p) {
    const user = el('input', { type: 'text', class: 'vpn-user', autocomplete: 'off', spellcheck: 'false', maxlength: '256', placeholder: 'User name', 'aria-label': 'User name for ' + p.name });
    const pass = el('input', { type: 'password', class: 'vpn-pass', autocomplete: 'off', spellcheck: 'false', maxlength: '1024', placeholder: 'Password', 'aria-label': 'Password for ' + p.name });
    const err = el('p', { class: 'field-error' });
    const form = el('form', { class: 'inline-form vpn-form', autocomplete: 'off', 'data-for': p.id }, [
      user, pass,
      el('button', { type: 'submit', class: 'btn small primary', text: 'Save' }),
      el('button', { type: 'button', class: 'btn small', text: 'Cancel', onclick: function () { pass.value = ''; vpnClose(); } }),
      err,
    ]);
    form.addEventListener('submit', function (e) {
      e.preventDefault();
      const username = user.value.trim();
      const password = pass.value;
      pass.value = '';
      if (!username) { err.textContent = 'Enter the user name'; return; }
      LB.request('vpnCredentials', { id: p.id, username: username, password: password }).then(function (v) {
        vpnOpen = { id: '', what: '' }; vpnSet(v); Page.toast('User name and password saved');
      }, function (x) { err.textContent = vpnError(x); });
    });
    return form;
  }
  function vpnRemove(p) {
    Page.confirm('Remove the VPN profile ' + p.name + '? Its file and password are deleted from this computer.', { ok: 'Remove', danger: true }).then(function (yes) {
      if (!yes) return;
      if (vpnOpen.id === p.id) vpnOpen = { id: '', what: '' };
      vpnAsk('vpnRemove', { id: p.id }).then(function (ok) {
        if (!ok) return;
        // The host may have turned auto-connect off with it.
        LB.request('getSettings').then(function (r) { settings = r.settings; render(); }).catch(function () {});
      });
    });
  }
  $('vpn-import').addEventListener('click', function () {
    const btn = this;
    $('vpn-error').textContent = '';
    $('vpn-warnings').hidden = true;
    btn.disabled = true;
    LB.request('vpnImport').then(function (r) {
      btn.disabled = false;
      if (!r) return; // cancelled
      const w = $('vpn-warnings');
      w.textContent = '';
      (r.warnings || []).forEach(function (t) { w.appendChild(el('li', { text: t })); });
      w.hidden = !(r.warnings || []).length;
      Page.toast('Imported ' + (r.profile ? r.profile.name : 'the profile'));
      LB.request('vpnState').then(vpnSet).catch(function () {});
    }, function (e) { btn.disabled = false; $('vpn-error').textContent = vpnError(e); });
  });
  LB.on('vpn', function (m) { vpnSet(m); });

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

  // ---------- AI and scripts (0.12.0) ----------
  // Providers change through their own commands (the host keeps its list when the page saves).
  // An API key goes to the host once and is never kept, shown or sent back: the key fields are
  // cleared as soon as they are read, and the page only learns whether a provider has a key.
  const AI_KINDS = { openrouter: 'OpenRouter', openai: 'OpenAI', anthropic: 'Anthropic', custom: 'Custom (OpenAI-compatible)' };
  const AI_MODELS_SHOWN = 200;
  let aiKeys = {}; // provider id -> true while a key is stored
  const aiTests = {}; // provider id -> { ok, text } of its last Test
  let aiKeyOpen = ''; // the provider whose key form is open
  let aiModels = null; // { provider, models } from the last "Load models"
  let aiSig = '';
  function aiProviders() { return (settings && settings.ai && settings.ai.providers) || []; }
  function aiError(e) { return Array.isArray(e && e.data) && e.data[0] ? e.data[0].message : (e && e.message) || 'Failed'; }
  function renderAi() {
    if (!settings || !settings.ai) return;
    const on = !!settings.ai.enabled;
    $('ai-body').disabled = !on;
    $('ai-off-hint').hidden = on;
    const sel = $('ai.activeProvider');
    if (sel !== document.activeElement) {
      sel.textContent = '';
      sel.appendChild(el('option', { value: '', text: aiProviders().length ? '(choose one)' : '(add a provider first)' }));
      aiProviders().forEach((p) => sel.appendChild(el('option', { value: p.id, text: p.name })));
    }
    if (aiModels && aiModels.provider !== settings.ai.activeProvider) aiModels = null;
    aiCount();
    renderAiProviders();
    renderAiModels();
  }
  function aiCount() {
    $('ai-extra-count').textContent = $('ai.systemPromptExtra').value.length + ' / 4000';
  }
  $('ai.systemPromptExtra').addEventListener('input', aiCount);

  function renderAiProviders() {
    const on = !!settings.ai.enabled;
    const list = aiProviders();
    // Rebuilt only when something it shows changed (an open key field keeps what is typed).
    const sig = JSON.stringify([list, aiKeys, aiTests, aiKeyOpen, on]);
    if (sig === aiSig) return;
    aiSig = sig;
    const box = $('ai-providers');
    box.textContent = '';
    if (!list.length) box.appendChild(el('p', { class: 'muted', text: 'No providers yet.' }));
    list.forEach(function (p) {
      const has = !!aiKeys[p.id];
      const test = aiTests[p.id];
      box.appendChild(el('div', { class: 'row ai-provider', 'data-id': p.id }, [
        el('div', { class: 'main' }, [
          el('span', { class: 'name', text: p.name }),
          el('span', { class: 'sub', text: (AI_KINDS[p.kind] || p.kind) + (p.kind === 'custom' && p.baseUrl ? ' · ' + p.baseUrl : '') }),
          test ? el('span', { class: 'sub ai-test-result ' + (test.ok ? 'ok' : 'bad'), role: 'status', text: test.text }) : null,
          aiMaxOutput(p),
        ]),
        el('span', { class: 'key-status' + (has ? ' saved' : ''), text: has ? 'Key saved' : 'No key' }),
        el('button', { type: 'button', class: 'btn small ai-test', text: 'Test', disabled: !on, title: on ? 'Ask the provider for its model list' : 'Turn on AI features first', onclick: function () { aiTest(p); } }),
        el('button', { type: 'button', class: 'btn small ai-change-key', text: has ? 'Change key' : 'Add key', onclick: function () { aiKeyOpen = aiKeyOpen === p.id ? '' : p.id; renderAiProviders(); } }),
        el('button', { type: 'button', class: 'btn small danger ai-remove', text: 'Remove', onclick: function () { aiRemove(p); } }),
      ]));
      if (aiKeyOpen === p.id) {
        const form = aiKeyForm(p, has);
        box.appendChild(form);
        form.querySelector('input').focus();
      }
    });
  }
  // 0.15.0: the provider's output cap, saved on change (0 = auto: OpenAI-compatible providers
  // then use their own default, Anthropic the model's maximum). The host checks the range and
  // caps it by what the model is known to allow.
  const AI_MAX_OUTPUT_LIMIT = 1000000;
  function aiMaxOutput(p) {
    const was = typeof p.maxOutputTokens === 'number' ? p.maxOutputTokens : 32000;
    const input = el('input', { type: 'number', class: 'ai-max-output', min: '0', max: String(AI_MAX_OUTPUT_LIMIT), step: '1000', value: String(was),
      'aria-label': 'Max output tokens for ' + p.name, title: 'The longest answer the model may give, in tokens. 0 = auto.' });
    input.addEventListener('change', function () {
      const n = Number(input.value);
      if (input.value.trim() === '' || !Number.isInteger(n) || n < 0 || n > AI_MAX_OUTPUT_LIMIT) {
        input.value = String(was);
        Page.toast('Max output tokens is a whole number from 0 (auto) to 1,000,000');
        return;
      }
      if (n === was) return;
      LB.request('aiProviderSave', { provider: Object.assign({}, p, { maxOutputTokens: n }) }).then(function (r) {
        settings.ai.providers = (r && r.providers) || settings.ai.providers;
        Page.toast(n === 0 ? 'Max output tokens: auto' : 'Max output tokens: ' + n.toLocaleString('en-US'));
        renderAiProviders();
      }, function (e) { input.value = String(was); Page.toast(aiError(e)); });
    });
    return el('label', { class: 'sub ai-max-out' }, ['Max output tokens ', input]);
  }
  function aiKeyForm(p, has) {
    const input = el('input', { type: 'password', class: 'ai-key-input', autocomplete: 'off', spellcheck: 'false', placeholder: has ? 'New API key' : 'API key', 'aria-label': 'API key for ' + p.name });
    const err = el('p', { class: 'field-error' });
    const form = el('form', { class: 'inline-form ai-key-form', autocomplete: 'off', 'data-for': p.id }, [
      input,
      el('button', { type: 'submit', class: 'btn small primary', text: 'Save key' }),
      el('button', { type: 'button', class: 'btn small', text: 'Cancel', onclick: function () { input.value = ''; aiKeyOpen = ''; renderAiProviders(); } }),
      has ? el('button', { type: 'button', class: 'btn small danger ai-remove-key', text: 'Remove key', onclick: function () {
        input.value = '';
        LB.request('aiKeyRemove', { id: p.id }).then(function () {
          delete aiKeys[p.id]; aiKeyOpen = ''; renderAiProviders(); Page.toast('Key removed');
        }, function (e) { err.textContent = aiError(e); });
      } }) : null,
      err,
    ]);
    form.addEventListener('submit', function (e) {
      e.preventDefault();
      const key = input.value.trim();
      input.value = '';
      if (!key) { err.textContent = 'Enter the key'; return; }
      aiSetKey(p.id, key).then(function (ok) {
        if (ok === true) { aiKeyOpen = ''; renderAiProviders(); } else err.textContent = ok;
      });
    });
    return form;
  }
  // Resolves true, or the error text.
  function aiSetKey(id, key) {
    return LB.request('aiKeySet', { id: id, key: key }).then(function (r) {
      aiKeys[id] = !(r && r.hasKey === false);
      renderAiProviders();
      Page.toast('Key saved');
      return true;
    }, function (e) { return 'The key was not saved: ' + aiError(e); });
  }
  function aiTest(p) {
    aiTests[p.id] = { ok: true, text: 'Testing…' };
    renderAiProviders();
    LB.request('aiTest', { id: p.id }).then(function (r) {
      if (r && r.ok === false) aiTests[p.id] = { ok: false, text: r.message || 'Failed' };
      else {
        const n = r && typeof r.models === 'number' ? r.models : 0;
        aiTests[p.id] = { ok: true, text: 'Connected: ' + n + (n === 1 ? ' model' : ' models') };
      }
    }, function (e) { aiTests[p.id] = { ok: false, text: aiError(e) }; }).then(renderAiProviders);
  }
  function aiRemove(p) {
    Page.confirm('Remove ' + p.name + '? Its key is deleted from this computer too.', { ok: 'Remove', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('aiProviderRemove', { id: p.id }).then(function (r) {
        settings.ai.providers = (r && r.providers) || [];
        delete aiKeys[p.id]; delete aiTests[p.id];
        if (aiKeyOpen === p.id) aiKeyOpen = '';
        if (settings.ai.activeProvider === p.id) {
          const s = collect();
          s.ai.activeProvider = '';
          return save(s);
        }
        render();
      }, function (e) { Page.toast(aiError(e)); });
    });
  }
  $('ai-name').value = AI_KINDS.openrouter;
  $('ai-kind').addEventListener('change', function () {
    const kind = this.value;
    const name = $('ai-name').value.trim();
    // The name follows the kind until the user types their own.
    if (!name || Object.values(AI_KINDS).includes(name)) $('ai-name').value = AI_KINDS[kind];
    $('ai-base-url').hidden = kind !== 'custom';
    $('ai-key').placeholder = kind === 'custom' ? 'API key (optional)' : 'API key';
  });
  $('ai-add-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const err = $('ai-add-error');
    err.textContent = '';
    const kind = $('ai-kind').value;
    const name = $('ai-name').value.trim();
    const baseUrl = kind === 'custom' ? $('ai-base-url').value.trim() : '';
    if (!name) { err.textContent = 'Enter a name'; return; }
    if (kind === 'custom' && !baseUrl) { err.textContent = 'Enter the server\'s address, for example http://localhost:11434/v1'; return; }
    if (kind !== 'custom' && !$('ai-key').value.trim()) { err.textContent = AI_KINDS[kind] + ' needs an API key'; return; }
    // The key leaves the field at once; it goes to the host when the provider exists.
    let key = $('ai-key').value.trim();
    $('ai-key').value = '';
    const hadKey = !!key;
    LB.request('aiProviderSave', { provider: { id: '', kind: kind, name: name, baseUrl: baseUrl, vision: null } }).then(function (r) {
      settings.ai.providers = (r && r.providers) || [];
      const id = r && r.id;
      const keyDone = key && id ? aiSetKey(id, key) : Promise.resolve(true);
      key = '';
      return keyDone.then(function (ok) {
        if (ok !== true) err.textContent = ok + ' Use "Add key" to enter it again.';
        $('ai-name').value = AI_KINDS[$('ai-kind').value];
        $('ai-base-url').value = '';
        // The first provider becomes the agent's.
        if (id && !settings.ai.activeProvider) {
          const s = collect();
          s.ai.activeProvider = id;
          return save(s);
        }
        render();
      });
    }, function (e) {
      key = '';
      err.textContent = aiError(e) + (hadKey ? ' The key was not kept: enter it again.' : '');
    });
  });

  function renderAiModels() {
    $('ai-model-picker').hidden = !aiModels;
    $('ai-load-models').textContent = aiModels ? 'Reload models' : 'Load models';
    const box = $('ai-models');
    box.textContent = '';
    if (!aiModels) return;
    const q = $('ai-model-filter').value.trim().toLowerCase();
    const all = aiModels.models;
    const hits = all.filter((m) => !q || (m.id + ' ' + (m.name || '')).toLowerCase().includes(q));
    const cur = settings.ai.activeModel;
    hits.slice(0, AI_MODELS_SHOWN).forEach(function (m) {
      const on = m.id === cur;
      box.appendChild(el('button', { type: 'button', class: 'ai-model' + (on ? ' on' : ''), 'data-id': m.id, 'aria-pressed': on ? 'true' : 'false', onclick: function () {
        const s = collect();
        s.ai.activeModel = m.id;
        save(s);
      } }, [
        el('span', { class: 'name', text: m.name || m.id }),
        el('span', { class: 'id', text: m.id }),
        m.tools ? el('span', { class: 'cap', text: 'tools' }) : null,
        m.vision ? el('span', { class: 'cap', text: 'images' }) : null,
      ]));
    });
    if (!hits.length) box.appendChild(el('p', { class: 'muted', text: all.length ? 'No model matches.' : 'The provider listed no models.' }));
    else if (hits.length > AI_MODELS_SHOWN) box.appendChild(el('p', { class: 'muted', text: (hits.length - AI_MODELS_SHOWN) + ' more: narrow the search.' }));
  }
  $('ai-model-filter').addEventListener('input', renderAiModels);
  $('ai-load-models').addEventListener('click', function () {
    const btn = this;
    const id = $('ai.activeProvider').value;
    const status = $('ai-models-status');
    if (!id) { status.textContent = 'Choose a provider first.'; return; }
    const refresh = !!(aiModels && aiModels.provider === id);
    status.textContent = 'Loading models…';
    btn.disabled = true;
    LB.request('aiModels', { id: id, refresh: refresh }).then(function (r) {
      aiModels = { provider: id, models: (r && r.models) || [] };
      status.textContent = aiModels.models.length + (aiModels.models.length === 1 ? ' model' : ' models') + '. Click one to use it.';
    }, function (e) {
      aiModels = null;
      status.textContent = 'Could not load the models: ' + aiError(e);
    }).then(function () { btn.disabled = false; renderAiModels(); });
  });
  function loadAiStatus() {
    LB.request('aiStatus').then(function (r) { aiKeys = (r && r.keys) || {}; if (settings) renderAiProviders(); }).catch(function () {});
  }
  $('manage-scripts').addEventListener('click', function () { LB.post('openPage', { page: 'scripts' }); });
  // 0.15.0: Privacy → session logs.
  $('open-sessions').addEventListener('click', function (e) { e.preventDefault(); LB.post('openPage', { page: 'sessions' }); });

  function render() {
    renderAi();
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
    stateHidden('mail-custom-row', settings.mailHandler !== 'custom');
    pickers.forEach((p) => p.sync());
    renderStyleRows(settings.appearance || {});
    renderWallpapers();
    stateHidden('discard-row', !settings.tabDiscardEnabled);
    $('download-dir').textContent = settings.downloadDir || 'Downloads (default)';
    $('default-dir').hidden = !settings.downloadDir;
    renderUserAgent();
    renderHeaders();
    renderDns();
    renderVpn();
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
    syncStyle(s.appearance);
    // Something that is not a whole number becomes an out-of-range value, so
    // the host's validation reports the field (instead of the message failing to parse).
    ['tabSleepMinutes', 'tabDiscardMinutes', 'vaultAutolockMinutes'].forEach((k) => {
      if (!Number.isFinite(s[k]) || s[k] < 0 || !Number.isInteger(s[k]) || s[k] > 99999999) s[k] = 99999999;
    });
    // The same for the agent's step limit (0 is out of its range, so the host names the field).
    if (s.ai && (!Number.isInteger(s.ai.maxSteps) || s.ai.maxSteps < 0 || s.ai.maxSteps > 99999999)) s.ai.maxSteps = 0;
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
        stateHidden('mail-custom-row', candidate.mailHandler !== 'custom');
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
  // ---------- Account (0.8.0) ----------
  // Nothing here talks to the network: the host does, and only after Sign in (and while this
  // card is shown for a signed-in device). The page shows what the host says.
  let accountCard = null;
  let accountTimer = 0;
  function accountShow(id, on) { $(id).hidden = !on; }
  function renderAccount() {
    const c = accountCard;
    if (!c) return;
    clearInterval(accountTimer);
    const st = c.state;
    accountShow('account-linking', st === 'linking');
    accountShow('account-sign-in', st === 'signedOut');
    accountShow('account-copy', st === 'linking');
    accountShow('account-reopen', st === 'linking');
    accountShow('account-cancel', st === 'linking');
    accountShow('account-manage', st === 'signedIn');
    accountShow('account-add-device', st === 'signedIn');
    accountShow('account-sign-out', st === 'signedIn');
    const msg = c.message || '';
    $('account-message').textContent = msg;
    accountShow('account-message', !!msg);
    if (st === 'private') {
      $('account-title').textContent = 'Not available in private windows';
      $('account-desc').textContent = "Your account isn't shown in private windows. Open Settings in a normal window to sign in or out.";
    } else if (st === 'signedIn') {
      $('account-title').textContent = 'Signed in as ' + c.email;
      $('account-desc').textContent = 'This device: ' + c.deviceName + (c.version ? ' · version ' + c.version : '') + '. You can rename or remove it on your account page.';
    } else if (st === 'linking') {
      $('account-title').textContent = 'Signing in…';
      $('account-desc').textContent = 'Approve this device on krunk.209.wf. Only approve a code that you see here.';
      $('account-code').textContent = c.userCode;
      // 0.13.0: the QR of the sign-in link (checked in core; an older server sends none).
      const qr = typeof c.qr === 'string' && c.qr.startsWith('data:image/svg+xml;base64,') ? c.qr : '';
      if (qr) $('account-qr').src = qr;
      else $('account-qr').removeAttribute('src');
      accountShow('account-qr', !!qr);
      accountShow('account-qr-hint', !!qr);
      const tick = function () {
        const left = Math.max(0, Math.round((c.expiresAtMs - Date.now()) / 1000));
        $('account-wait').textContent = 'Waiting for the approval… The code works for ' + Math.floor(left / 60) + ':' + String(left % 60).padStart(2, '0') + ' more.';
      };
      tick();
      accountTimer = setInterval(tick, 1000);
    } else if (st === 'working') {
      $('account-title').textContent = c.message;
      $('account-desc').textContent = '';
      accountShow('account-message', false);
    } else {
      $('account-title').textContent = 'Sign in to Krunk';
      $('account-desc').textContent = 'Sign in with your Krunk account to see this device on your account page. Signing in opens krunk.209.wf, where you approve this device.';
    }
  }
  LB.on('account', function (m) { accountCard = m.card; renderAccount(); });
  // Settings → Sync (beta) (0.9.0): the status; everything else is in krunk://sync.
  function renderSync(v) {
    const card = $('sync-card');
    card.hidden = v.state === 'private';
    const texts = {
      noAccount: ['Sync', 'Sign in to your Krunk account above first. Then you can sync bookmarks, history, settings and passwords between your devices, end-to-end encrypted.'],
      off: ['Sync is off', (v.message ? v.message + ' ' : '') + 'Sync your bookmarks, history, settings and passwords between your devices. They are encrypted on this device before they leave it.'],
      working: ['Sync', v.message || ''],
      join: ['Finishing sync setup', 'Continue in krunk://sync.'],
      restoreConfirm: ['Finishing sync setup', 'Continue in krunk://sync.'],
      joining: ['Waiting for your other device', 'Continue in krunk://sync.'],
      alarm: ['Sync stopped', v.message || ''],
      on: ['Sync is on', (v.message || v.status || '')],
    }[v.state] || ['Sync', ''];
    $('sync-title').textContent = texts[0];
    $('sync-desc').textContent = texts[1];
    $('sync-open').hidden = v.state !== 'off';
    $('sync-manage').hidden = !['on', 'alarm', 'join', 'restoreConfirm', 'joining', 'working'].includes(v.state);
    const on = v.state === 'on';
    $('sync-now').hidden = !on;
    $('sync-now').disabled = on && !!v.syncing;
    $('sync-now').setAttribute('aria-busy', on && v.syncing ? 'true' : 'false');
    $('sync-last').hidden = !(on && v.lastSyncMs);
    $('sync-last').textContent = on && v.lastSyncMs ? 'Last synced ' + new Date(v.lastSyncMs).toLocaleString() : '';
    const cl = $('sync-collections');
    cl.hidden = !on;
    cl.textContent = '';
    (on ? v.collections : []).forEach(function (c) {
      const box = LB.el('input', { type: 'checkbox', id: 'sync-coll-' + c.name });
      box.checked = c.on;
      box.addEventListener('change', function () { LB.post('syncSetCollection', { name: c.name, on: box.checked }); });
      cl.appendChild(LB.el('label', { class: 'sync-coll' }, [box, LB.el('span', { text: c.label })]));
    });
  }
  LB.on('sync', function (m) { renderSync(m.view); });
  $('sync-open').addEventListener('click', function () { LB.post('syncOpen'); });
  $('sync-manage').addEventListener('click', function () { LB.post('syncOpen'); });
  $('sync-now').addEventListener('click', function () { LB.post('syncNow'); });
  $('account-sign-in').addEventListener('click', function () { LB.post('accountSignIn'); });
  $('account-cancel').addEventListener('click', function () { LB.post('accountCancel'); });
  $('account-reopen').addEventListener('click', function () { LB.post('accountReopen'); });
  $('account-manage').addEventListener('click', function () { LB.post('accountManage'); });
  $('account-add-device').addEventListener('click', function () { LB.post('accountAddDevice'); });
  $('account-sign-out').addEventListener('click', function () { LB.post('accountSignOut'); });
  $('account-copy').addEventListener('click', function () {
    const text = accountCard && accountCard.userCode;
    if (!text) return;
    const done = () => Page.toast('Code copied');
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, done);
    else done();
  });
  LB.post('accountState', { check: tabFromHash() === 'account' });
  LB.post('syncState');

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
  // 0.15.0: the three gradient colours share a row: no swatches, a short reset, their own names.
  const pickers = Array.from(document.querySelectorAll('input[data-picker]')).map(function (input) {
    const own = input.dataset.picker === 'gradient';
    return ColorPicker.attach(input, { presets: PRESETS[input.dataset.picker] || [], resetLabel: own ? 'Clear' : undefined,
      label: (own && input.getAttribute('aria-label')) || input.closest('.row').querySelector('label, .label').textContent });
  });

  // ---------- tab and toolbar styles (0.15.0) ----------
  // The curated looks of core/src/style.rs (PRESETS; ui/dev/test-0150-theme.cjs checks they
  // match). Choosing one copies its values; changing any of them afterwards makes it "Your own".
  const STYLE_PRESETS = {
    classic: { tabStyle: 'solid', toolbarStyle: 'solid', gradient: ['', '', ''], gradientAngle: 90, tabShape: 'rounded', activeHighlight: 'fill', accentGlow: false },
    sunset: { tabStyle: 'ombre', toolbarStyle: 'gradient', gradient: ['#f97316', '#ec4899', '#8b5cf6'], gradientAngle: 90, tabShape: 'rounded', activeHighlight: 'fill', accentGlow: true },
    ocean: { tabStyle: 'gradient', toolbarStyle: 'gradient', gradient: ['#0ea5e9', '', '#14b8a6'], gradientAngle: 135, tabShape: 'pill', activeHighlight: 'outline', accentGlow: false },
    glass: { tabStyle: 'glass', toolbarStyle: 'glass', gradient: ['', '', ''], gradientAngle: 90, tabShape: 'rounded', activeHighlight: 'underline', accentGlow: true },
    mono: { tabStyle: 'solid', toolbarStyle: 'solid', gradient: ['', '', ''], gradientAngle: 90, tabShape: 'square', activeHighlight: 'underline', accentGlow: false },
  };
  function presetValues(p) {
    return { tabStyle: p.tabStyle, toolbarStyle: p.toolbarStyle, gradientFrom: p.gradient[0], gradientVia: p.gradient[1], gradientTo: p.gradient[2],
      gradientAngle: p.gradientAngle, tabShape: p.tabShape, activeHighlight: p.activeHighlight, accentGlow: p.accentGlow };
  }
  // In collect(): the preset stays only while every value is still its own; roundedTabs
  // mirrors the shape for older versions.
  function syncStyle(a) {
    if (!a) return;
    const p = STYLE_PRESETS[a.preset];
    if (a.preset && (!p || Object.entries(presetValues(p)).some(([k, v]) => a[k] !== v))) a.preset = '';
    a.roundedTabs = a.tabShape !== 'square';
  }
  function renderStyleRows(a) {
    const grad = ['gradient', 'ombre'].includes(a.tabStyle) || a.toolbarStyle === 'gradient';
    stateHidden('gradient-colors-row', !grad);
    stateHidden('gradient-angle-row', !grad);
    stateHidden('tab-radius-row', a.tabShape !== 'rounded');
  }
  $('appearance.preset').addEventListener('change', function () {
    const next = collect();
    const p = STYLE_PRESETS[this.value];
    if (p) Object.assign(next.appearance, presetValues(p), { roundedTabs: p.tabShape !== 'square' });
    next.appearance.preset = p ? this.value : '';
    save(next);
  });
  // The rows follow a style choice at once (before the host's reply).
  ['appearance.tabStyle', 'appearance.toolbarStyle', 'appearance.tabShape'].forEach(function (id) {
    $(id).addEventListener('change', function () {
      renderStyleRows({ tabStyle: $('appearance.tabStyle').value, toolbarStyle: $('appearance.toolbarStyle').value, tabShape: $('appearance.tabShape').value });
    });
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
      // 0.7.2 (R12): a 0 always says why.
      if (c.why) s += ' (' + c.why + ')';
      // 0.7.4: the cipher and key sizes of fields that could not be decrypted (no secrets).
      if (c.detail) s += ' [' + c.detail + ']';
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

  // ---------- default browser (0.12.1) ----------
  // Windows lets no program make itself the default: "Make default" opens Windows Settings at
  // Krunk's page, and the page asks again while that may be under way and when it is back in
  // front.
  let defaultState = null;
  let defaultOpened = false;
  let defaultTimer = 0;
  function renderDefault() {
    const s = defaultState;
    const notDefault = !!(s && s.installed && s.isDefault === false);
    $('default-banner').hidden = !notDefault;
    $('default-make').hidden = !(s && s.installed && s.isDefault !== true);
    $('default-hint').hidden = !(notDefault && defaultOpened);
    let text = 'Checking…';
    if (s && !s.installed) text = 'This copy of Krunk is not installed. Install Krunk with its setup to make it your default browser.';
    else if (s && s.isDefault === true) text = 'Krunk is your default browser.';
    else if (s && s.isDefault === false) text = 'Krunk is not your default browser: links you open in other apps open in another browser.';
    else if (s) text = 'Windows did not say which browser is the default.';
    $('default-status').textContent = text;
  }
  function checkDefault() {
    return LB.request('defaultBrowserStatus').then(function (s) {
      if (!s) return;
      defaultState = s;
      if (s.isDefault === true) stopDefaultPoll();
      renderDefault();
    }, function () {});
  }
  function stopDefaultPoll() { if (defaultTimer) { clearInterval(defaultTimer); defaultTimer = 0; } }
  function makeDefault() {
    LB.request('makeDefaultBrowser').then(function () {
      defaultOpened = true;
      renderDefault();
      // Asked every 2 s for 3 minutes while the user is in Windows Settings.
      stopDefaultPoll();
      const until = Date.now() + 180e3;
      defaultTimer = setInterval(function () { if (Date.now() > until) stopDefaultPoll(); else checkDefault(); }, 2000);
    }, function (e) {
      $('default-status').textContent = e.message;
    });
  }
  $('default-make').addEventListener('click', makeDefault);
  $('default-banner-make').addEventListener('click', makeDefault);
  window.addEventListener('focus', checkDefault);
  document.addEventListener('visibilitychange', function () { if (!document.hidden) checkDefault(); });
  checkDefault();

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
    loadAiStatus();
    LB.request('vpnState').then(vpnSet).catch(function () {});
  });
})();
