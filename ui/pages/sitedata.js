// krunk://sitedata: cookies, local storage and session storage of the site in the tab
// this page was opened from (shield panel → Cookies and site data). The host bound the
// page to that tab when it opened it; requests carry no tab or origin.
(function () {
  'use strict';
  const el = LB.el;
  const VALUE_PREVIEW = 200;
  let state = null;

  function expiry(ms) { return ms === null || ms === undefined ? 'Session' : new Date(ms).toLocaleString(); }
  function sameSite(s) { return { unspecified: '–', none: 'None', lax: 'Lax', strict: 'Strict' }[s] || s; }
  function fail(e) { Page.toast(e.message); }

  function load() {
    return LB.request('siteDataGet').then(function (r) {
      state = r;
      document.getElementById('origin').textContent = r.origin;
      document.title = 'Site data – ' + r.origin;
      document.getElementById('private').hidden = !r.private;
      document.getElementById('gone').hidden = !!r.live;
      renderPermissions(r.permissions || {}, r.private);
      renderCookies(r.cookies);
      renderArea('local', r.local);
      renderArea('session', r.session);
    }).catch(fail);
  }

  // The site's stored permission answers (Krunk 0.5.0: local network access).
  function renderPermissions(p, isPrivate) {
    const box = document.getElementById('permissions');
    box.textContent = '';
    if (isPrivate) {
      box.appendChild(el('div', { class: 'empty', text: 'Permissions are not remembered in private tabs.' }));
      return;
    }
    if (p.localNetwork === null || p.localNetwork === undefined) {
      box.appendChild(el('div', { class: 'empty', text: 'No permissions saved for this site. Krunk asks when the site wants to reach your local network.' }));
      return;
    }
    box.appendChild(el('div', { class: 'perm-row' }, [
      el('span', { class: 'perm-name', text: 'Local network access' }),
      el('span', { class: 'perm-state ' + (p.localNetwork ? 'allowed' : 'blocked'), text: p.localNetwork ? 'Allowed' : 'Blocked' }),
      el('button', { class: 'link-btn', id: 'perm-remove', text: 'Remove', onclick: function () {
        LB.request('sitePermissionRemove', { kind: 'localNetwork' }).then(load).catch(fail);
      } }),
    ]));
  }

  function renderCookies(cookies) {
    const body = document.getElementById('cookies');
    body.textContent = '';
    if (!cookies.length) {
      body.appendChild(el('tr', {}, [el('td', { colspan: '7', class: 'muted', text: 'No cookies.' })]));
      return;
    }
    cookies.forEach(function (c) {
      const long = c.value.length > 48;
      const cell = el('td', { class: 'value' });
      const show = function (full) {
        cell.textContent = full || !long ? c.value : c.value.slice(0, 48) + '…';
        if (long) cell.appendChild(el('button', { class: 'show', text: full ? 'Hide' : 'Show', onclick: function () { show(!full); } }));
      };
      show(false);
      body.appendChild(el('tr', { 'data-name': c.name }, [
        el('td', { text: c.name, title: c.name }), cell,
        el('td', { text: c.domain }), el('td', { text: c.path }), el('td', { text: expiry(c.expires) }),
        el('td', { class: 'flags', text: [c.secure ? 'Secure' : null, c.httpOnly ? 'HttpOnly' : null, 'SameSite ' + sameSite(c.sameSite)].filter(Boolean).join(' · ') }),
        el('td', { class: 'row-actions' }, [
          Page.iconButton('edit', 'Edit cookie ' + c.name, function () { edit('cookie', c.name, c.value, c); }),
          Page.iconButton('trash', 'Delete cookie ' + c.name, function () {
            LB.request('siteCookiesDelete', { cookie: { name: c.name, domain: c.domain, path: c.path } }).then(load).catch(fail);
          }),
        ]),
      ]));
    });
  }

  function renderArea(area, items) {
    const box = document.getElementById(area);
    box.textContent = '';
    if (!items.length) {
      box.appendChild(el('div', { class: 'empty', text: 'Empty.' }));
      return;
    }
    items.forEach(function (kv) {
      const key = kv[0], value = kv[1];
      const long = value.length > VALUE_PREVIEW;
      box.appendChild(el('div', { class: 'kv', 'data-key': key }, [
        el('span', { class: 'k', text: key }),
        el('span', { class: 'v', text: long ? value.slice(0, VALUE_PREVIEW) + '… (' + value.length + ' characters)' : value }),
        el('span', { class: 'actions' }, [
          Page.iconButton('edit', 'Edit ' + key, function () { edit(area, key, value); }),
          Page.iconButton('trash', 'Delete ' + key, function () {
            LB.request('siteDataRemove', { area: area, key: key }).then(load).catch(fail);
          }),
        ]),
      ]));
    });
  }

  // ---------- the edit dialog: Text and Hex ----------
  const dialog = document.getElementById('edit');
  const $ = function (id) { return document.getElementById(id); };
  const keyInput = $('edit-key'), textArea = $('edit-value'), hexBox = $('edit-hex'), encSelect = $('edit-enc'), errorLine = $('edit-error');
  // What is being edited: area ('local', 'session', 'cookie'), the value as stored (`text`, null
  // while the bytes do not form one in the chosen encoding), the encoding, the mode.
  let ed = null;
  const hex = HexEditor.create(hexBox, function (bytes) {
    const r = HexCodec.decode(bytes, ed.enc, ed.style);
    ed.text = r.error ? null : r.text;
    showError(r.error ? r.error + ': choose another encoding or change the bytes.' : '');
  });

  function showError(text) { errorLine.textContent = text || ''; }
  // Why a cookie cannot hold `v` (as the host checks it too).
  function cookieProblem(name, v) {
    if (new TextEncoder().encode(name + v).length > 4096) return 'A cookie\'s name and value can be at most 4096 bytes together';
    if (/[\u0000-\u001F\u007F-\u009F]/.test(v)) return 'A cookie cannot hold control characters: use Percent-encoded or Base64';
    if (v.includes(';')) return 'A cookie cannot hold a semicolon: use Percent-encoded or Base64';
    if (v.trim() !== v) return 'A cookie\'s value cannot start or end with a space';
    return null;
  }
  function setMode(mode) {
    if (mode === ed.mode) return true;
    if (mode === 'hex') {
      const r = HexCodec.encode(textArea.value, ed.enc);
      if (r.error) { showError(r.error + ' (' + HexCodec.ENCODINGS[ed.enc] + '): choose another encoding.'); return false; }
      ed.text = textArea.value;
      ed.style = HexCodec.base64Style(ed.text);
      hex.setBytes(r.bytes);
    } else {
      if (ed.text === null) { showError('These bytes are not ' + HexCodec.ENCODINGS[ed.enc] + ': fix them or choose another encoding first.'); return false; }
      textArea.value = ed.text;
    }
    ed.mode = mode;
    showError('');
    $('mode-text').setAttribute('aria-selected', String(mode === 'text'));
    $('mode-hex').setAttribute('aria-selected', String(mode === 'hex'));
    textArea.hidden = mode !== 'text';
    hexBox.hidden = mode !== 'hex';
    (mode === 'hex' ? hex : textArea).focus();
    return true;
  }
  $('mode-text').addEventListener('click', function () { setMode('text'); });
  $('mode-hex').addEventListener('click', function () { setMode('hex'); });
  encSelect.addEventListener('change', function () {
    const next = encSelect.value;
    if (ed.mode === 'hex') {
      if (ed.text !== null) {
        // The same value, its bytes in the other encoding.
        const r = HexCodec.encode(ed.text, next);
        if (r.error) { showError(r.error + ' (' + HexCodec.ENCODINGS[next] + ')'); encSelect.value = ed.enc; return; }
        ed.enc = next;
        hex.setBytes(r.bytes);
      } else {
        // Bytes that were not valid: read them in the other encoding.
        ed.enc = next;
        const r = HexCodec.decode(hex.getBytes(), next, ed.style);
        ed.text = r.error ? null : r.text;
        if (r.error) { showError(r.error); return; }
      }
    } else {
      ed.enc = next;
    }
    showError('');
  });

  function edit(area, key, value, cookie) {
    const isNew = key === null;
    const isCookie = area === 'cookie';
    $('edit-title').textContent = isCookie ? 'Edit cookie' : (isNew ? 'Add to ' : 'Edit ') + (area === 'local' ? 'local storage' : 'session storage');
    $('edit-key-label').textContent = isCookie ? 'Name' : 'Key';
    keyInput.value = isNew ? '' : key;
    keyInput.readOnly = !isNew;
    textArea.value = isNew ? '' : value;
    encSelect.textContent = '';
    (isCookie ? ['utf8', 'percent', 'base64'] : ['utf8', 'latin1', 'utf16', 'base64', 'percent']).forEach(function (e) {
      encSelect.appendChild(el('option', { value: e, text: HexCodec.ENCODINGS[e] }));
    });
    ed = { area: area, cookie: cookie, mode: 'text', enc: HexCodec.detect(value, isCookie), text: value, style: HexCodec.base64Style(value) };
    encSelect.value = ed.enc;
    $('mode-text').setAttribute('aria-selected', 'true');
    $('mode-hex').setAttribute('aria-selected', 'false');
    textArea.hidden = false;
    hexBox.hidden = true;
    showError('');
    dialog.returnValue = '';
    dialog.showModal();
    // Binary values open on the Hex tab.
    if (!isNew && HexCodec.looksBinary(value, isCookie)) setMode('hex');
    else (isNew ? keyInput : textArea).focus();
  }

  // Save: the value as stored, checked before the dialog closes.
  dialog.querySelector('form').addEventListener('submit', function (e) {
    if (e.submitter && e.submitter.value !== 'ok') return;
    const value = ed.mode === 'text' ? textArea.value : ed.text;
    if (value === null) { e.preventDefault(); showError('These bytes are not ' + HexCodec.ENCODINGS[ed.enc] + ': fix them or choose another encoding.'); return; }
    if (!keyInput.value) { e.preventDefault(); showError('Enter a key'); keyInput.focus(); return; }
    if (ed.area === 'cookie') {
      const problem = cookieProblem(ed.cookie.name, value);
      if (problem) { e.preventDefault(); showError(problem); return; }
      const c = ed.cookie;
      LB.request('siteCookieSet', { cookie: { name: c.name, domain: c.domain, path: c.path }, value: value }).then(load).catch(fail);
    } else {
      LB.request('siteDataSet', { area: ed.area, key: keyInput.value, value: value }).then(load).catch(fail);
    }
  });

  document.querySelectorAll('button.add').forEach(function (b) {
    b.addEventListener('click', function () { edit(b.dataset.area, null, ''); });
  });
  document.querySelectorAll('button.clear').forEach(function (b) {
    b.addEventListener('click', function () {
      const name = b.dataset.area === 'local' ? 'local storage' : 'session storage';
      Page.confirm('Remove every item from ' + name + ' of ' + (state ? state.origin : 'this site') + '?', { ok: 'Clear', danger: true }).then(function (yes) {
        if (yes) LB.request('siteDataClear', { area: b.dataset.area }).then(load).catch(fail);
      });
    });
  });
  document.getElementById('cookies-delete-all').addEventListener('click', function () {
    Page.confirm('Delete all cookies of ' + (state ? state.origin : 'this site') + '? You may be signed out.', { ok: 'Delete', danger: true }).then(function (yes) {
      if (yes) LB.request('siteCookiesDelete', {}).then(load).catch(fail);
    });
  });
  document.getElementById('reload').addEventListener('click', load);

  LB.post('ready');
  load();
})();
