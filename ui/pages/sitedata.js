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
      renderCookies(r.cookies);
      renderArea('local', r.local);
      renderArea('session', r.session);
    }).catch(fail);
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
        el('td', {}, [Page.iconButton('trash', 'Delete cookie ' + c.name, function () {
          LB.request('siteCookiesDelete', { cookie: { name: c.name, domain: c.domain, path: c.path } }).then(load).catch(fail);
        })]),
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

  const dialog = document.getElementById('edit');
  function edit(area, key, value) {
    const isNew = key === null;
    document.getElementById('edit-title').textContent = (isNew ? 'Add to ' : 'Edit ') + (area === 'local' ? 'local storage' : 'session storage');
    const k = document.getElementById('edit-key'), v = document.getElementById('edit-value');
    k.value = isNew ? '' : key;
    k.readOnly = !isNew;
    v.value = isNew ? '' : value;
    dialog.returnValue = '';
    dialog.showModal();
    (isNew ? k : v).focus();
    dialog.onclose = function () {
      if (dialog.returnValue !== 'ok' || !k.value) return;
      LB.request('siteDataSet', { area: area, key: k.value, value: v.value }).then(load).catch(fail);
    };
  }

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
