// krunk://extensions (0.6.0): the installed extensions with what they may reach (the scope
// list), which chrome.* APIs they use and whether Krunk provides them, enable/disable, allow
// in private tabs, options, remove; install from a zip/crx file, an unpacked folder or a
// Chrome Web Store address.
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  let list = [];

  function showError(text) { $('error').textContent = text || ''; $('error').hidden = !text; }
  function showStatus(text) { $('status').textContent = text || ''; $('status').hidden = !text; }

  function sourceText(s) {
    if (s.startsWith('webstore:')) return 'Chrome Web Store';
    if (s.startsWith('folder:')) return 'unpacked folder ' + s.slice(7);
    if (s.startsWith('file:')) return 'file ' + s.slice(5);
    return s;
  }

  function render() {
    const box = $('list');
    box.textContent = '';
    $('empty').hidden = list.length > 0;
    list.forEach(function (x) {
      const scope = x.scope.length
        ? el('p', { class: 'scope' }, ['Can read and change pages on: '].concat(x.scope.map((s, i) => el('span', { class: s.allSites ? 'all' : '', text: s.text + (i < x.scope.length - 1 ? ', ' : ''), title: s.pattern }))))
        : el('p', { class: 'scope', text: 'Does not read or change web pages.' });
      const unsupported = x.apis.filter((a) => !a.supported).concat(x.runtimeUnsupported.map((a) => ({ api: a, supported: false, reason: 'reached for at runtime; not provided' })));
      const supported = x.apis.filter((a) => a.supported);
      const apis = el('p', { class: 'apis' }, [
        supported.length ? el('span', { class: 'yes', text: 'Uses: ' + supported.map((a) => 'chrome.' + a.api).join(', ') + '. ' }) : null,
        unsupported.length ? el('span', { class: 'no', text: 'Not provided by Krunk: ' + unsupported.map((a) => 'chrome.' + a.api + (a.reason ? ' (' + a.reason.replace(/^not provided: /, '') + ')' : '')).join(', ') + '.' }) : null,
      ]);
      const chips = el('div', { class: 'chips' }, [
        el('span', { class: 'chip' + (x.contentScripts ? '' : ' off'), text: x.contentScripts ? x.contentScripts + ' content script' + (x.contentScripts > 1 ? 's' : '') : 'no content scripts' }),
        el('span', { class: 'chip' + (x.hasBackground ? '' : ' off'), text: x.hasBackground ? 'background page' : 'no background' }),
        el('span', { class: 'chip' + (x.hasPopup ? '' : ' off'), text: x.hasPopup ? 'toolbar popup' : 'no popup' }),
        (x.dnrTranslated || x.dnrSkipped) ? el('span', { class: 'chip', text: x.dnrTranslated + ' network rule' + (x.dnrTranslated === 1 ? '' : 's') + (x.dnrSkipped ? ' (' + x.dnrSkipped + ' not translated)' : '') }) : null,
      ]);
      const toggle = (key, label, checked) => el('label', { class: 'toggle' }, [
        el('input', { type: 'checkbox', 'data-key': key, 'data-id': x.id, checked: checked ? true : null, onchange: (e) => {
          const body = { id: x.id };
          body[key] = e.target.checked;
          LB.request('extSet', body).then(update, (err) => showError(err.message));
        } }),
        label,
      ]);
      const row = el('div', { class: 'row' }, [
        toggle('enabled', 'Enabled', x.enabled),
        toggle('private', 'Allow in private tabs', x.allowedInPrivate),
        x.hasOptions ? el('button', { class: 'btn small', text: 'Options', onclick: () => LB.post('extOpenOptions', { id: x.id }) }) : null,
        el('button', { class: 'btn small danger', text: 'Remove', onclick: () => {
          Page.confirm('Remove ' + x.name + '? Its stored data goes too.', { ok: 'Remove', danger: true }).then((yes) => { if (yes) LB.request('extRemove', { id: x.id }).then(update, (err) => showError(err.message)); });
        } }),
      ]);
      box.appendChild(el('div', { class: 'card ext' + (x.enabled ? '' : ' disabled'), 'data-id': x.id }, [
        el('div', { class: 'icon' }, [x.icon ? el('img', { src: x.icon, alt: '' }) : el('span', { text: '🧩' })]),
        el('div', { class: 'body' }, [
          el('div', { class: 'title' }, [el('span', { class: 'name', text: x.name }), el('span', { class: 'version', text: x.version }), el('span', { class: 'source', text: '· ' + sourceText(x.source) })]),
          x.description ? el('p', { class: 'desc', text: x.description }) : null,
          scope, apis, chips,
          el('details', {}, [el('summary', { text: 'Details' }), el('p', { text: 'Id ' + x.id + ' · permissions: ' + (x.permissions.length ? x.permissions.join(', ') : 'none') + ' · pages on https://' + x.id + '.ext.krunk.invalid' })]),
          row,
        ]),
      ]));
    });
  }

  function update(l) { if (Array.isArray(l)) { list = l; render(); } else if (l && Array.isArray(l.extensions)) { list = l.extensions; render(); } else { showError('Unexpected reply: ' + JSON.stringify(l).slice(0, 200)); } }
  function load() { LB.request('extList').then(update, (err) => showError(err.message)); }

  function installed(r) {
    showStatus('');
    if (r && r.installed) { Page.toast('Installed ' + r.name); update(r.extensions); }
  }
  $('install-file').addEventListener('click', function () { showError(''); LB.request('extInstallFile').then(installed, (err) => showError(err.message)); });
  $('install-folder').addEventListener('click', function () { showError(''); LB.request('extInstallFolder').then(installed, (err) => showError(err.message)); });
  $('url-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const url = $('url').value.trim();
    if (!url) return;
    showError('');
    showStatus('Downloading from the Chrome Web Store…');
    LB.request('extInstallUrl', { url: url }).then(function (r) { $('url').value = ''; installed(r); }, function (err) { showStatus(''); showError(err.message); });
  });
  LB.on('extChanged', function (m) { update(m.extensions); });
  LB.post('ready');
  load();
})();
