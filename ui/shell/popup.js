// LightBrowser popup: main menu, shield panel and password prompts.
// The host decides what to show (`popup` event) and where; prompts echo the
// tab id and origin they were raised for so the host can re-check them.
(function () {
  'use strict';
  const el = LB.el;
  const root = document.getElementById('root');

  function close() { LB.post('hidePopup'); }
  function command(name, arg) {
    const c = arg === undefined ? { command: name } : { command: name, arg: arg };
    LB.post('command', { command: c });
    close();
  }
  function item(label, kbd, onclick) {
    return el('button', { class: 'item', onclick: onclick }, [
      el('span', { class: 'label', text: label }),
      kbd ? el('span', { class: 'kbd', text: kbd }) : null,
    ]);
  }
  function openPage(page) { LB.post('openPage', { page: page }); close(); }
  const sep = () => el('div', { class: 'sep' });

  function renderMenu(c) {
    return [
      item('New tab', 'Ctrl+T', () => command('newTab')),
      item('New private tab', 'Ctrl+Shift+N', () => command('newPrivateTab')),
      item('Reopen closed tab', 'Ctrl+Shift+T', () => command('reopenClosedTab')),
      sep(),
      item('History', 'Ctrl+H', () => command('history')),
      item('Bookmarks', 'Ctrl+Shift+O', () => command('bookmarks')),
      item('Downloads', 'Ctrl+J', () => command('downloads')),
      item('Passwords', '', () => openPage('passwords')),
      sep(),
      el('div', { class: 'zoom' }, [
        el('span', { class: 'label', text: 'Zoom' }),
        el('button', { title: 'Zoom out (Ctrl+-)', onclick: () => zoom('out'), text: '−' }),
        el('span', { class: 'value', id: 'zoom-value', text: Math.round(c.zoom * 100) + '%' }),
        el('button', { title: 'Zoom in (Ctrl++)', onclick: () => zoom('in'), text: '+' }),
      ]),
      item('Find in page…', 'Ctrl+F', () => command('find')),
      item('Print…', 'Ctrl+P', () => command('print')),
      item('Developer tools', 'F12', () => command('devTools')),
      sep(),
      item('Settings', '', () => openPage('settings')),
      item('About LightBrowser', '', () => openPage('about')),
      item('Exit', '', () => { LB.post('quit'); }),
    ];
  }

  function zoom(action) {
    LB.post('zoom', { action: action });
  }

  function renderShield(c) {
    return [el('div', { class: 'panel' }, [
      el('h2', { text: c.enabled ? (c.allowed ? 'Blocking is off for' : 'Blocking ads and trackers on') : 'Ad blocking is disabled' }),
      el('p', { class: 'origin', text: c.host || '' }),
      el('p', {}, [el('span', { class: 'big', text: String(c.blocked) }), ' blocked on this page']),
      c.enabled && c.host ? el('div', { class: 'row' }, [
        el('button', {
          class: 'btn', onclick: () => { LB.post('shieldAllowSite', { allow: !c.allowed }); close(); },
          text: c.allowed ? 'Block on this site again' : 'Allow ads on this site',
        }),
      ]) : null,
    ])];
  }

  function renderSave(c) {
    const reply = (accept, never) => {
      LB.post('savePasswordReply', { tabId: c.tabId, origin: c.origin, accept: accept, never: never });
      close();
    };
    return [el('div', { class: 'panel' }, [
      el('h2', { text: c.update ? 'Update saved password?' : 'Save password?' }),
      el('p', {}, [c.username ? el('strong', { text: c.username }) : 'This login', ' on ',
        el('span', { class: 'origin', text: c.origin })]),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', onclick: () => reply(false, true), text: 'Never for this site' }),
        el('button', { class: 'btn', onclick: () => reply(false, false), text: 'Not now' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true, false), text: c.update ? 'Update' : 'Save' }),
      ]),
    ])];
  }

  function renderFill(c) {
    const reply = (id) => {
      LB.post('fillReply', { tabId: c.tabId, origin: c.origin, entryId: id });
      close();
    };
    return [el('div', { class: 'panel' }, [
      el('h2', { text: 'Fill saved login?' }),
      el('p', { class: 'origin', text: c.origin }),
    ].concat(c.entries.map((e) => el('button', { class: 'choice', onclick: () => reply(e.id), text: e.username || '(no username)' })))
      .concat([el('div', { class: 'row' }, [el('button', { class: 'btn', onclick: () => reply(null), text: 'Cancel' })])]))];
  }

  function renderFolder(c) {
    if (!c.items.length) return [el('div', { class: 'panel muted', text: '(empty)' })];
    return c.items.map(function (b) {
      const icon = b.isFolder ? el('span', { class: 'folder-glyph', text: '▸' })
        : b.favicon ? el('img', { class: 'fav', src: b.favicon, alt: '' }) : el('span', { class: 'fav dot' });
      const btn = el('button', { class: 'item', title: b.url || b.title }, [icon, el('span', { class: 'label', text: b.title })]);
      btn.addEventListener('click', function (e) {
        if (b.isFolder) { LB.post('showBookmarkFolder', { id: b.id }); return; }
        LB.post('openUrl', { url: b.url, newTab: e.ctrlKey });
        close();
      });
      btn.addEventListener('auxclick', function (e) {
        if (e.button === 1 && b.url) { LB.post('openUrl', { url: b.url, newTab: true }); close(); }
      });
      return btn;
    });
  }

  let current = null;
  LB.on('popup', function (m) {
    current = m.content;
    const c = m.content;
    const render = { menu: renderMenu, shield: renderShield, savePassword: renderSave, fill: renderFill, bookmarkFolder: renderFolder }[c.kind];
    root.textContent = '';
    root.dataset.kind = c.kind;
    (render ? render(c) : []).forEach((n) => n && root.appendChild(n));
    const first = document.getElementById('default') || root.querySelector('button');
    if (first) first.focus();
  });

  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      // Dismissing a prompt is never a confirmation.
      if (current && current.kind === 'savePassword') {
        LB.post('savePasswordReply', { tabId: current.tabId, origin: current.origin, accept: false, never: false });
      } else if (current && current.kind === 'fill') {
        LB.post('fillReply', { tabId: current.tabId, origin: current.origin, entryId: null });
      }
      close();
    }
  });
  document.addEventListener('contextmenu', (e) => e.preventDefault());

  LB.post('ready');
})();
