// Krunk popup: main menu, shield panel and password prompts.
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
  function item(label, kbd, onclick, disabled) {
    return el('button', { class: 'item', onclick: onclick, disabled: disabled ? '' : null }, [
      el('span', { class: 'label', text: label }),
      kbd ? el('span', { class: 'kbd', text: kbd }) : null,
    ]);
  }
  function openPage(page) { LB.post('openPage', { page: page }); close(); }
  const sep = () => el('div', { class: 'sep' });

  function renderMenu(c) {
    return [
      item('New tab', 'Ctrl+T', () => command('newTab')),
      item('New window', 'Ctrl+N', () => command('newWindow')),
      item('New private tab', 'Ctrl+Shift+N', () => command('newPrivateTab')),
      item('Reopen closed tab', 'Ctrl+Shift+T', () => command('reopenClosedTab')),
      sep(),
      item('History', 'Ctrl+H', () => command('history')),
      item('Bookmarks', 'Ctrl+Shift+O', () => command('bookmarks')),
      item('Downloads', 'Ctrl+J', () => command('downloads')),
      item('Passwords', '', () => openPage('passwords')),
      item('Cookies and site data', '', () => openPage('storage')),
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
      item('About Krunk', '', () => openPage('about')),
      item('Exit', '', () => { LB.post('quit'); }),
    ];
  }

  // Right-click on a tab.
  function renderTabMenu(c) {
    const act = (action) => { LB.post('tabMenuAction', { id: c.tabId, action: action }); close(); };
    return [
      item('Reload', 'Ctrl+R', () => act('reload')),
      item('Duplicate', '', () => act('duplicate')),
      item('Move to new window', '', () => act('moveToNewWindow'), !c.canMove),
      sep(),
      item('New window', 'Ctrl+N', () => command('newWindow')),
      sep(),
      item('Close tab', 'Ctrl+W', () => act('close')),
      // "Other tabs" exist exactly when the tab could move to a window of its own.
      item('Close other tabs', '', () => act('closeOthers'), !c.canMove),
      item('Close tabs to the right', '', () => act('closeRight'), !c.canCloseRight),
    ];
  }

  // Short names for request types (the blocked list's badges).
  const KIND_LABELS = { xmlhttprequest: 'xhr', sub_frame: 'frame', main_frame: 'page', stylesheet: 'css', websocket: 'socket', image: 'image', script: 'script', ping: 'ping', media: 'media', font: 'font', object: 'object', other: 'other', document: 'page' };
  const kindLabel = (k) => KIND_LABELS[k] || k;

  // Right-click on the bookmarks bar: on a bookmark, a folder, or the bar itself.
  function renderBookmarkMenu(c) {
    const act = (action) => LB.post('bookmarkMenuAction', { id: c.id, action: action });
    const mode = (label, value, action) => {
      const b = item(label, '', () => act(action));
      b.setAttribute('role', 'menuitemradio');
      b.setAttribute('aria-checked', c.mode === value ? 'true' : 'false');
      b.classList.add('radio');
      if (c.mode === value) b.classList.add('checked');
      return b;
    };
    const modes = [
      mode('Always show the bar', 'always', 'showAlways'),
      mode('Show it on new tabs only', 'newTab', 'showOnNewTab'),
      mode('Hide the bar', 'never', 'showNever'),
    ];
    const add = [
      item('Add this page', '', () => act('addPage'), !c.canAdd),
      item('New folder…', '', () => act('newFolder')),
    ];
    if (c.id === null || c.id === undefined) {
      return add.concat([sep()], modes, [sep(), item('Bookmark manager', 'Ctrl+Shift+O', () => command('bookmarks'))]);
    }
    if (c.isFolder) {
      return [
        item(c.children ? 'Open all (' + c.children + ')' : 'Open all', '', () => act('openAll'), !c.children),
        sep(),
        item('Rename…', '', () => act('edit')),
        item(c.children ? 'Delete folder and its ' + c.children + ' bookmarks' : 'Delete folder', '', () => act('delete')),
        sep(),
      ].concat(add, [sep()], modes);
    }
    return [
      item('Open in new tab', '', () => act('openNewTab')),
      item('Open in new window', '', () => act('openNewWindow')),
      item('Open in private tab', '', () => act('openPrivate')),
      sep(),
      item('Edit…', '', () => act('edit')),
      item('Delete', '', () => act('delete')),
      sep(),
    ].concat(add, [sep()], modes);
  }

  // Edit a bookmark or folder, or name a new folder.
  function renderBookmarkEdit(c) {
    const name = el('input', { type: 'text', id: 'bm-name', value: c.title || '', 'aria-label': 'Name', maxlength: '300', autocomplete: 'off' });
    const url = c.isFolder ? null : el('input', { type: 'url', id: 'bm-url', value: c.url || '', 'aria-label': 'Address', autocomplete: 'off', spellcheck: 'false' });
    const err = el('p', { class: 'error', role: 'alert' });
    const save = el('button', { class: 'btn primary', type: 'submit', text: 'Save' });
    const form = el('form', {
      onsubmit: (e) => {
        e.preventDefault();
        const title = name.value.trim();
        const address = url ? url.value.trim() : null;
        if (url && !/^(https?|file):\/\/\S+$/i.test(address)) { err.textContent = 'Enter a web address (https://…).'; url.focus(); return; }
        LB.post('saveBookmark', { id: c.id === undefined ? null : c.id, parent: c.parent, title: title, url: address });
      },
    }, [
      el('label', { class: 'field' }, [el('span', { text: 'Name' }), name]),
      url ? el('label', { class: 'field' }, [el('span', { text: 'Address' }), url]) : null,
      err,
      el('div', { class: 'row' }, [el('button', { class: 'btn', type: 'button', onclick: close, text: 'Cancel' }), save]),
    ]);
    setTimeout(() => { name.focus(); name.select(); }, 0);
    const heading = c.id === null || c.id === undefined ? 'New folder' : c.isFolder ? 'Rename folder' : 'Edit bookmark';
    return [el('div', { class: 'panel bookmark-edit' }, [el('h2', { text: heading }), form])];
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
      c.host ? el('div', { class: 'row' }, [
        el('button', { class: 'btn', id: 'site-data', onclick: () => { LB.post('openSiteData'); close(); }, text: 'Cookies and site data' }),
      ]) : null,
      (c.recent && c.recent.length) ? el('div', { class: 'blocked-list' }, c.recent.map(function (b) {
        const path = b.url.replace(/^https?:\/\/[^/]+/, '') || '/';
        return el('div', { class: 'blocked', title: b.url }, [
          el('span', { class: 'kind', text: kindLabel(b.kind) }),
          el('span', { class: 'addr' }, [el('strong', { text: b.host }), el('span', { text: path })]),
          b.count > 1 ? el('span', { class: 'count', text: '×' + b.count }) : null,
        ]);
      }).concat([el('button', { class: 'link', id: 'show-blocked', onclick: () => { LB.post('openBlockedPage'); close(); },
        text: 'Show all ' + c.distinct + ' blocked addresses' })])) : null,
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
    if (!c.entries.length) {
      return [el('div', { class: 'panel' }, [
        el('h2', { text: 'No saved logins' }),
        el('p', {}, ['Nothing saved for ', el('span', { class: 'origin', text: c.origin }), '.']),
        el('div', { class: 'row' }, [
          el('button', { class: 'btn', onclick: () => { LB.post('openPage', { page: 'passwords' }); close(); }, text: 'Open passwords' }),
          el('button', { class: 'btn primary', id: 'default', onclick: () => reply(null), text: 'OK' }),
        ]),
      ])];
    }
    return [el('div', { class: 'panel' }, [
      el('h2', { text: 'Fill saved login?' }),
      el('p', { class: 'origin', text: c.origin }),
    ].concat(c.entries.map((e) => el('button', { class: 'choice', onclick: () => reply(e.id), text: e.username || '(no username)' })))
      .concat([el('div', { class: 'row' }, [el('button', { class: 'btn', onclick: () => reply(null), text: 'Cancel' })])]))];
  }

  // Master-password prompt before saving or filling. The host enforces the
  // retry delay; the countdown here is only a hint.
  let countdown = null;
  function renderUnlock(c) {
    clearInterval(countdown);
    const input = el('input', { type: 'password', id: 'master', autocomplete: 'off', 'aria-label': 'Master password', placeholder: 'Master password' });
    const button = el('button', { class: 'btn primary', id: 'unlock', type: 'submit', text: 'Unlock' });
    const err = el('p', { class: 'error', role: 'alert', text: c.error || '' });
    const form = el('form', {
      onsubmit: (e) => {
        e.preventDefault();
        if (!input.value || button.disabled) return;
        button.disabled = true;
        button.textContent = 'Unlocking…';
        LB.post('vaultUnlock', { password: input.value });
        input.value = '';
      },
    }, [input, el('div', { class: 'row' }, [
      el('button', { class: 'btn', type: 'button', onclick: close, text: 'Cancel' }),
      button,
    ])]);
    let wait = c.waitMs || 0;
    if (wait > 0) {
      button.disabled = true;
      const tick = () => {
        wait -= 1000;
        if (wait <= 0) { clearInterval(countdown); button.disabled = false; err.textContent = c.error ? c.error.replace(/ Try again in \d+ s\./, '') : ''; }
        else err.textContent = 'Too many attempts. Try again in ' + Math.ceil(wait / 1000) + ' s.';
      };
      countdown = setInterval(tick, 1000);
    }
    setTimeout(() => input.focus(), 0);
    return [el('div', { class: 'panel' }, [
      el('h2', { text: c.purpose === 'save' ? 'Unlock to save this password' : 'Unlock to fill this login' }),
      el('p', { class: 'origin', text: c.origin }),
      form,
      err,
    ])];
  }

  // A folder dropdown: its items open, subfolders open in place, a right-click shows the
  // bar's menu for the item, and an item can be dragged: within the list to reorder it or
  // into a subfolder (the same marks as on the bar), or out of the popup, where the host
  // follows the mouse and the bar takes the drop.
  const DRAG_THRESHOLD = 5;
  const folderDrag = { item: null, id: 0, pointerId: null, startX: 0, startY: 0, moved: false, target: null, suppressClick: false };
  function renderFolder(c) {
    if (!c.items.length) return [el('div', { class: 'panel muted', text: '(empty)' })];
    const rows = c.items.map(function (b) {
      const icon = b.isFolder ? el('span', { class: 'folder-glyph', text: '▸' })
        : b.favicon ? el('img', { class: 'fav', src: b.favicon, alt: '' }) : el('span', { class: 'fav dot' });
      const btn = el('button', { class: 'item', title: b.url || b.title, 'data-id': b.id }, [icon, el('span', { class: 'label', text: b.title })]);
      if (b.isFolder) btn.classList.add('folder');
      btn.addEventListener('click', function (e) {
        if (folderDrag.suppressClick) return;
        if (b.isFolder) { LB.post('showBookmarkFolder', { id: b.id }); return; }
        LB.post('openUrl', { url: b.url, newTab: e.ctrlKey });
        close();
      });
      btn.addEventListener('auxclick', function (e) {
        if (e.button === 1 && b.url) { LB.post('openUrl', { url: b.url, newTab: true }); close(); }
      });
      btn.addEventListener('contextmenu', function (e) {
        e.preventDefault();
        LB.post('showFolderItemMenu', { id: b.id, x: e.clientX, y: e.clientY });
      });
      btn.addEventListener('pointerdown', function (e) {
        if (e.button !== 0) return;
        folderDrag.item = btn; folderDrag.id = b.id; folderDrag.pointerId = e.pointerId;
        folderDrag.startX = e.clientX; folderDrag.startY = e.clientY; folderDrag.moved = false; folderDrag.target = null;
      });
      return btn;
    });
    return rows;
  }
  function folderRows() { return Array.prototype.slice.call(root.querySelectorAll('.item[data-id]')); }
  function clearFolderMarks() {
    folderRows().forEach(function (n) { n.classList.remove('drop-before', 'drop-after', 'drop-into', 'dragging'); });
  }
  // Where a drop at y goes: { into: folderId } (the middle half of a folder row) or { index }
  // among the other rows.
  function folderTarget(y) {
    const others = folderRows().filter(function (n) { return n !== folderDrag.item; });
    let index = 0;
    for (let i = 0; i < others.length; i++) {
      const r = others[i].getBoundingClientRect();
      if (others[i].classList.contains('folder') && y >= r.top + r.height / 4 && y <= r.bottom - r.height / 4) {
        return { into: Number(others[i].dataset.id), node: others[i] };
      }
      if (y > r.top + r.height / 2) index = i + 1;
    }
    return { index: index, others: others };
  }
  root.addEventListener('pointermove', function (e) {
    if (!folderDrag.item || e.pointerId !== folderDrag.pointerId) return;
    if (!folderDrag.moved) {
      if (Math.abs(e.clientX - folderDrag.startX) < DRAG_THRESHOLD && Math.abs(e.clientY - folderDrag.startY) < DRAG_THRESHOLD) return;
      folderDrag.moved = true;
      root.setPointerCapture(e.pointerId);
    }
    if (e.clientX < 0 || e.clientY < 0 || e.clientX > window.innerWidth || e.clientY > window.innerHeight) {
      // Out of the dropdown: the host follows the mouse from here.
      const id = folderDrag.id;
      clearFolderMarks();
      folderDrag.item = null;
      try { root.releasePointerCapture(e.pointerId); } catch (err) { /* already released */ }
      LB.post('folderItemDragOut', { id: id });
      return;
    }
    clearFolderMarks();
    folderDrag.item.classList.add('dragging');
    const t = folderTarget(e.clientY);
    folderDrag.target = t;
    if (t.into !== undefined) t.node.classList.add('drop-into');
    else if (t.others.length) {
      if (t.index < t.others.length) t.others[t.index].classList.add('drop-before');
      else t.others[t.others.length - 1].classList.add('drop-after');
    }
  });
  function endFolderDrag(e, commit) {
    if (!folderDrag.item || e.pointerId !== folderDrag.pointerId) return;
    const id = folderDrag.id, t = folderDrag.target, moved = folderDrag.moved;
    folderDrag.item = null;
    clearFolderMarks();
    if (!moved) return;
    folderDrag.suppressClick = true;
    setTimeout(function () { folderDrag.suppressClick = false; }, 0);
    if (!commit || !t || !current || current.kind !== 'bookmarkFolder') return;
    if (t.into !== undefined) {
      if (t.into !== id) LB.post('moveBookmarkInFolder', { id: id, parent: t.into, index: 1000000 });
    } else {
      LB.post('moveBookmarkInFolder', { id: id, parent: current.id, index: t.index });
    }
  }
  root.addEventListener('pointerup', function (e) { endFolderDrag(e, true); });
  root.addEventListener('pointercancel', function (e) { endFolderDrag(e, false); });

  // "Krunk X is available" and the download that follows "Update now".
  function renderUpdate(c) {
    const reply = (choice) => LB.post('updateReply', { choice: choice });
    const s = c.state || { step: 'offered' };
    const body = [el('h2', { text: 'Krunk ' + c.version + ' is available' })];
    if (s.step === 'downloading') {
      body.push(el('p', { text: 'Downloading… ' + s.percent + '%' }));
      body.push(el('progress', { max: '100', value: String(s.percent) }));
      body.push(el('div', { class: 'row' }, [el('button', { class: 'btn', id: 'default', onclick: close, text: 'Hide' })]));
    } else if (s.step === 'installing') {
      body.push(el('p', { text: 'Installing. Krunk closes now and starts again when the update is done.' }));
    } else {
      body.push(s.step === 'failed'
        ? el('p', { class: 'error', text: 'The update failed: ' + s.error })
        : el('p', { text: c.install ? 'You have ' + c.current + '. Krunk restarts to finish the update; your tabs are restored.'
          : 'You have ' + c.current + '. Download the new version from the Krunk website.' }));
      body.push(el('div', { class: 'row' }, [
        c.hasNotes ? el('button', { class: 'btn', onclick: () => { reply('notes'); }, text: 'Release notes' }) : null,
        el('button', { class: 'btn', onclick: () => { reply('later'); }, text: 'Later' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply('install'),
          text: !c.install ? 'Download' : s.step === 'failed' ? 'Try again' : 'Update now' }),
      ]));
    }
    return [el('div', { class: 'panel' }, body)];
  }

  // A notice that takes no focus ("Press Esc to show your cursor").
  function renderHint(c) {
    return [el('div', { class: 'hint', role: 'status' }, [el('span', { text: c.text })])];
  }

  let current = null;
  LB.on('popup', function (m) {
    current = m.content;
    const c = m.content;
    const render = { menu: renderMenu, shield: renderShield, savePassword: renderSave, fill: renderFill, bookmarkFolder: renderFolder, unlock: renderUnlock, hint: renderHint, update: renderUpdate, tabMenu: renderTabMenu, bookmarkMenu: renderBookmarkMenu, bookmarkEdit: renderBookmarkEdit }[c.kind];
    root.textContent = '';
    root.dataset.kind = c.kind;
    (render ? render(c) : []).forEach((n) => n && root.appendChild(n));
    const first = document.getElementById('master') || document.getElementById('bm-name') || document.getElementById('default') || root.querySelector('button:not([disabled])');
    if (first && c.kind !== 'hint') first.focus();
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
