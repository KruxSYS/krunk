// krunk://extensions (0.6.0; laid out as cards in 0.7.0, every install confirmed first): the installed extensions with what they may reach (the scope
// list), which chrome.* APIs they use and whether Krunk provides them, enable/disable, allow
// in private tabs, options, remove; install from a zip/crx file, an unpacked folder or a
// Chrome Web Store address.
// 0.16.0: `krunk://extensions#<id>` shows that extension: its card scrolled into view, marked,
// with its details open (the name in the menu of an extension's toolbar icon leads here).
(function () {
  'use strict';
  const el = LB.el;
  const $ = (id) => document.getElementById(id);
  let list = [];
  // The extension asked for by the address: its card is marked and its details stay open
  // across updates of the list; `reveal`: scroll to it at the next render that has it (the
  // list arrives after the page).
  let target = null;
  let reveal = false;
  let loaded = false;

  function showError(text) { $('error').textContent = text || ''; $('error').hidden = !text; }
  function showStatus(text) { $('status').textContent = text || ''; $('status').hidden = !text; }

  function sourceText(s) {
    if (s.startsWith('webstore:')) return 'Chrome Web Store';
    if (s.startsWith('folder:')) return 'unpacked folder ' + s.slice(7);
    if (s.startsWith('file:')) return 'file ' + s.slice(5);
    return s;
  }

  // 0.7.0 (R22): a grid of cards: icon, name, version and an on/off switch on top; the
  // description; what it may reach; Details (what it uses, what Krunk does not provide) and the
  // actions below.
  function render() {
    const box = $('list');
    box.textContent = '';
    $('empty').hidden = list.length > 0;
    list.forEach(function (x) {
      const allSites = x.scope.some((s) => s.allSites);
      const reach = x.scope.length
        ? el('p', { class: 'reach' + (allSites ? ' all' : '') }, [allSites ? 'Can read and change all websites' : 'Can read and change: ' + x.scope.map((s) => s.text).join(', ')])
        : el('p', { class: 'reach none', text: 'Does not read or change web pages' });
      const unsupported = x.apis.filter((a) => !a.supported).concat(x.runtimeUnsupported.map((a) => ({ api: a, supported: false, reason: 'reached for at runtime; not provided' })));
      const supported = x.apis.filter((a) => a.supported);
      const sw = el('label', { class: 'switch', title: x.enabled ? 'Turn off' : 'Turn on' }, [
        el('input', { type: 'checkbox', role: 'switch', 'data-key': 'enabled', 'data-id': x.id, 'aria-label': (x.enabled ? 'Turn off ' : 'Turn on ') + x.name, checked: x.enabled ? true : null, onchange: (e) => {
          LB.request('extSet', { id: x.id, enabled: e.target.checked }).then(update, (err) => showError(err.message));
        } }),
        el('span', { class: 'track', 'aria-hidden': 'true' }),
      ]);
      const priv = el('label', { class: 'check' }, [
        el('input', { type: 'checkbox', 'data-key': 'private', 'data-id': x.id, checked: x.allowedInPrivate ? true : null, onchange: (e) => {
          LB.request('extSet', { id: x.id, private: e.target.checked }).then(update, (err) => showError(err.message));
        } }),
        'Allow in private tabs',
      ]);
      const details = el('details', { class: 'more', open: x.id === target ? true : null }, [
        el('summary', { text: 'Details' }),
        el('dl', {}, [
          el('dt', { text: 'Reaches' }), el('dd', {}, x.scope.length ? x.scope.map((s) => el('span', { class: 'tag' + (s.allSites ? ' warn' : ''), text: s.text, title: s.pattern })) : ['No web pages']),
          el('dt', { text: 'Parts' }), el('dd', {}, [
            el('span', { class: 'tag' + (x.contentScripts ? '' : ' off'), text: x.contentScripts ? x.contentScripts + ' content script' + (x.contentScripts > 1 ? 's' : '') : 'no content scripts' }),
            el('span', { class: 'tag' + (x.hasBackground ? '' : ' off'), text: x.hasBackground ? 'background page' : 'no background' }),
            el('span', { class: 'tag' + (x.hasPopup ? '' : ' off'), text: x.hasPopup ? 'toolbar popup' : 'no popup' }),
            (x.dnrTranslated || x.dnrSkipped) ? el('span', { class: 'tag', text: x.dnrTranslated + ' network rule' + (x.dnrTranslated === 1 ? '' : 's') + (x.dnrSkipped ? ' (' + x.dnrSkipped + ' not translated)' : '') }) : null,
          ]),
          el('dt', { text: 'Uses' }), el('dd', { text: supported.length ? supported.map((a) => 'chrome.' + a.api).join(', ') : 'No chrome.* APIs' }),
          unsupported.length ? el('dt', { class: 'warn', text: 'Not in Krunk' }) : null,
          unsupported.length ? el('dd', { class: 'unsupported' }, [el('ul', {}, unsupported.map((a) => el('li', {}, [el('code', { text: 'chrome.' + a.api }), a.reason ? ' — ' + a.reason.replace(/^not provided: /, '') : ''])))]) : null,
          el('dt', { text: 'Source' }), el('dd', { text: sourceText(x.source) }),
          el('dt', { text: 'Id' }), el('dd', { class: 'mono', text: x.id }),
          el('dt', { text: 'Permissions' }), el('dd', { text: x.permissions.length ? x.permissions.join(', ') : 'none' }),
        ]),
      ]);
      box.appendChild(el('article', { class: 'card ext' + (x.enabled ? '' : ' disabled') + (unsupported.length ? ' partial' : '') + (x.id === target ? ' target' : ''), 'data-id': x.id }, [
        el('div', { class: 'top' }, [
          el('div', { class: 'icon' }, [x.icon ? el('img', { src: x.icon, alt: '' }) : el('span', { class: 'puzzle', 'aria-hidden': 'true' })]),
          el('div', { class: 'title' }, [el('h2', { class: 'name', text: x.name }), el('span', { class: 'version', text: 'Version ' + x.version })]),
          sw,
        ]),
        x.description ? el('p', { class: 'desc', text: x.description }) : null,
        reach,
        unsupported.length ? el('p', { class: 'partial-note', text: 'Some of what it uses is not in Krunk (' + unsupported.length + '): see Details.' }) : null,
        details,
        el('div', { class: 'foot' }, [
          priv,
          el('span', { class: 'spacer' }),
          x.hasOptions ? el('button', { class: 'btn small', text: 'Options', onclick: () => LB.post('extOpenOptions', { id: x.id }) }) : null,
          el('button', { class: 'btn small danger', text: 'Remove', onclick: () => {
            Page.confirm('Remove ' + x.name + '? Its stored data goes too.', { ok: 'Remove', danger: true }).then((yes) => { if (yes) LB.request('extRemove', { id: x.id }).then(update, (err) => showError(err.message)); });
          } }),
        ]),
      ]));
    });
    const card = reveal && box.querySelector('.ext.target');
    if (card) {
      reveal = false;
      card.scrollIntoView({ block: 'center' });
    }
  }

  // The address names an extension (ids are 32 letters a–p; anything else is ignored). The
  // fragment is then dropped from the address: asking for the same extension again is a change
  // of the address again (a navigation to the address already shown would not tell the page).
  function route() {
    const id = location.hash.slice(1);
    if (!id) return;
    history.replaceState(null, '', location.pathname);
    if (!/^[a-p]{32}$/.test(id)) return;
    target = id;
    reveal = true;
    // (Before the list has arrived there is nothing to draw: not the "no extensions" card.)
    if (loaded) render();
  }

  function update(l) { if (Array.isArray(l)) { list = l; loaded = true; render(); } else if (l && Array.isArray(l.extensions)) { list = l.extensions; loaded = true; render(); } else { showError('Unexpected reply: ' + JSON.stringify(l).slice(0, 200)); } }
  function load() { LB.request('extList').then(update, (err) => showError(err.message)); }

  function installed(r) {
    showStatus('');
    if (r && r.installed) { Page.toast('Installed ' + r.name); update(r.extensions); }
    else if (r && r.declined) Page.toast('Not installed');
  }
  $('install-file').addEventListener('click', function () { showError(''); LB.request('extInstallFile').then(installed, (err) => showError(err.message)); });
  $('install-folder').addEventListener('click', function () { showError(''); LB.request('extInstallFolder').then(installed, (err) => showError(err.message)); });
  $('url-form').addEventListener('submit', function (e) {
    e.preventDefault();
    const url = $('url').value.trim();
    if (!url) return;
    showError('');
    showStatus('Downloading from the Chrome Web Store and checking its signatures…');
    LB.request('extInstallUrl', { url: url }).then(function (r) { $('url').value = ''; installed(r); }, function (err) { showStatus(''); showError(err.message); });
  });
  LB.on('extChanged', function (m) { update(m.extensions); });
  window.addEventListener('hashchange', route);
  route();
  LB.post('ready');
  load();
})();
