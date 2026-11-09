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
  // An item with an icon column (0.7.2, R8): `icon` names a symbol in popup.html; null keeps
  // the column empty so the labels stay aligned.
  function iconItem(icon, label, kbd, onclick, disabled) {
    const b = item(label, kbd, onclick, disabled);
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'icon micon');
    svg.setAttribute('aria-hidden', 'true');
    if (icon) {
      const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
      use.setAttribute('href', '#' + icon);
      svg.appendChild(use);
    }
    b.insertBefore(svg, b.firstChild);
    b.classList.add('with-icon');
    b.setAttribute('role', 'menuitem');
    return b;
  }
  function openPage(page) { LB.post('openPage', { page: page }); close(); }
  const sep = () => el('div', { class: 'sep' });

  function renderMenu(c) {
    // 0.7.2 (R13): a newer Krunk first; it opens the update prompt (the popup shows it next).
    const update = c.update ? [
      (function () {
        const b = item('Update available – Krunk ' + c.update, '', () => LB.post('updateReply', { choice: 'show' }));
        b.classList.add('update-item');
        b.insertBefore(el('span', { class: 'update-dot', 'aria-hidden': 'true' }), b.firstChild);
        return b;
      })(),
      sep(),
    ] : [];
    // 0.8.0: the Krunk account (none in a private window): Settings → Account.
    const account = typeof c.account === 'string' ? [
      (function () {
        const b = item(c.account ? 'Signed in as ' + c.account : 'Sign in to Krunk', '', () => { LB.post('openPage', { page: 'settings', section: 'account' }); close(); });
        b.classList.add('account-item');
        b.title = c.account ? 'Signed in as ' + c.account : 'Sign in to Krunk';
        return b;
      })(),
      sep(),
    ] : [];
    return update.concat(account, [
      item('New tab', 'Ctrl+T', () => command('newTab')),
      item('New window', 'Ctrl+N', () => command('newWindow')),
      item('New private tab', 'Ctrl+Shift+N', () => command('newPrivateTab')),
      item('Reopen closed tab', 'Ctrl+Shift+T', () => command('reopenClosedTab')),
      sep(),
      item('History', 'Ctrl+H', () => command('history')),
      item('Bookmarks', 'Ctrl+Shift+O', () => command('bookmarks')),
      item('Downloads', 'Ctrl+J', () => command('downloads')),
      item('Passwords & payments', '', () => openPage('passwords')),
      item('Cookies and site data', '', () => openPage('storage')),
      item('Extensions', '', () => openPage('extensions')),
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
      item('Picture-in-Picture', 'Alt+P', () => command('pictureInPicture'), !c.hasVideo),
      sep(),
      item('Settings', '', () => openPage('settings')),
      item('About Krunk', '', () => openPage('about')),
      item('Exit', '', () => { LB.post('quit'); }),
    ]);
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
      item(c.mutedSite ? 'Unmute site' : 'Mute site', '', () => act(c.mutedSite ? 'unmuteSite' : 'muteSite'), !c.hasSite),
      sep(),
      item('Close tab', 'Ctrl+W', () => act('close')),
      // "Other tabs" exist exactly when the tab could move to a window of its own.
      item('Close other tabs', '', () => act('closeOthers'), !c.canMove),
      item('Close tabs to the right', '', () => act('closeRight'), !c.canCloseRight),
    ];
  }

  // Right-click on Back or Forward (0.8.0): the tab's earlier pages (newest first) or later ones,
  // then "Show full history". A click jumps to exactly that page; arrows, Home/End, Enter and
  // Esc work from the keyboard.
  function renderNavMenu(c) {
    const rows = c.items.map(function (it) {
      const b = el('button', { class: 'item with-icon', role: 'menuitem', title: it.label === it.url ? it.url : it.label + '\n' + it.url }, [
        el('span', { class: 'label', text: it.label }),
      ]);
      if (it.icon) {
        b.insertBefore(el('img', { class: 'fav', src: it.icon, alt: '' }), b.firstChild);
      } else {
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        svg.setAttribute('class', 'icon micon');
        svg.setAttribute('aria-hidden', 'true');
        const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
        use.setAttribute('href', '#m-globe');
        svg.appendChild(use);
        b.insertBefore(svg, b.firstChild);
      }
      b.addEventListener('click', function () { LB.post('navToEntry', { tabId: c.tabId, entryId: it.entryId }); close(); });
      return b;
    });
    return [el('div', { class: 'nav-menu', role: 'menu', 'aria-label': c.forward ? 'Forward history' : 'Back history' },
      rows.concat([sep(), iconItem('m-history', 'Show full history', 'Ctrl+H', () => command('history'))]))];
  }
  // Up/Down (wrapping), Home and End move between a menu's items.
  function menuKeys(e) {
    if (!current || (current.kind !== 'navMenu' && current.kind !== 'bookmarkFolder')) return;
    // A subfolder opened in place goes back up with Backspace or Left (0.11.0); Right opens
    // the focused subfolder.
    if (current.kind === 'bookmarkFolder') {
      if ((e.key === 'Backspace' || e.key === 'ArrowLeft') && current.back) { e.preventDefault(); LB.post('bookmarkFolderBack'); return; }
      const f = document.activeElement;
      if (e.key === 'ArrowRight' && f && f.classList.contains('folder') && f.dataset.id) { e.preventDefault(); LB.post('showBookmarkFolder', { id: Number(f.dataset.id) }); return; }
    }
    const items = Array.prototype.slice.call(root.querySelectorAll('button.item:not([disabled])'));
    if (!items.length) return;
    const at = items.indexOf(document.activeElement);
    let next = -1;
    if (e.key === 'ArrowDown') next = at < 0 ? 0 : (at + 1) % items.length;
    else if (e.key === 'ArrowUp') next = at < 0 ? items.length - 1 : (at - 1 + items.length) % items.length;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = items.length - 1;
    if (next >= 0) { e.preventDefault(); items[next].focus(); }
  }

  // The downloads button's popup (0.6.0): the newest downloads with speed, time left and their
  // progress. 0.7.0 (R10, R11): file-type icons, actions as icon buttons (on hover for finished
  // ones; a click on a finished row opens the file), rows updated in place so the bars move
  // smoothly (ui/shared/dlview.js), and an empty state.
  const dlRows = new Map();   // id -> { row, sig }
  function dlAct(id, action) { LB.post('downloadAction', { id: id, action: action }); }
  function dlButton(label, cls, onclick) {
    return el('button', { type: 'button', class: 'dl-act ' + cls, title: label, 'aria-label': label, onclick: function (e) { e.stopPropagation(); onclick(); } }, [el('span', { class: 'dl-act-glyph', 'aria-hidden': 'true' })]);
  }
  function dlSig(d) { return [d.state, d.fileName, d.partProgress && d.partProgress.length > 1 ? 'parts' : 'one', d.reason || ''].join('|'); }
  function dlRow(d) {
    const running = d.state === 'inProgress' || d.state === 'paused';
    const done = d.state === 'completed';
    const failed = d.state === 'interrupted' || d.state === 'cancelled';
    const acts = [];
    if (done) acts.push(dlButton('Show in folder', 'folder', function () { dlAct(d.id, 'showInFolder'); close(); }));
    if (d.state === 'inProgress') acts.push(dlButton('Pause', 'pause', function () { dlAct(d.id, 'pause'); }));
    if (d.state === 'paused') acts.push(dlButton('Resume', 'resume', function () { dlAct(d.id, 'resume'); }));
    if (running) acts.push(dlButton('Cancel', 'cancel', function () { dlAct(d.id, 'cancel'); }));
    if (failed) acts.push(dlButton('Retry', 'retry', function () { dlAct(d.id, 'retry'); }));
    const row = el('div', { class: 'dl-row state-' + d.state, 'data-id': String(d.id), title: done ? 'Open ' + d.fileName : d.path, tabindex: done ? '0' : null, role: done ? 'button' : null }, [
      DlView.icon(d),
      el('div', { class: 'dl-main' }, [
        el('div', { class: 'dl-name', text: d.fileName }),
        el('div', { class: 'dl-status', text: DlView.status(d) }),
        running ? DlView.bar(d) : null,
      ]),
      el('div', { class: 'dl-acts' }, acts),
    ]);
    if (done) {
      const open = function () { dlAct(d.id, 'open'); close(); };
      row.addEventListener('click', open);
      row.addEventListener('keydown', function (e) { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); } });
    }
    return row;
  }
  // Brings the list to `items`, reusing the rows that did not change kind.
  function fillDownloads(list, items) {
    const seen = new Set();
    let prev = null;
    const empty = list.querySelector('.dl-empty');
    if (empty && items.length) empty.remove();
    items.forEach(function (d) {
      seen.add(d.id);
      let r = dlRows.get(d.id);
      const sig = dlSig(d);
      if (!r || r.sig !== sig) {
        const row = dlRow(d);
        if (r) r.row.replaceWith(row);
        r = { row: row, sig: sig };
        dlRows.set(d.id, r);
      } else {
        r.row.querySelector('.dl-status').textContent = DlView.status(d);
        const bar = r.row.querySelector('.dl-bar, .dl-segments');
        if (bar) DlView.update(bar, d);
      }
      const at = prev ? prev.nextSibling : list.firstChild;
      if (at !== r.row) list.insertBefore(r.row, at);
      prev = r.row;
    });
    dlRows.forEach(function (r, id) {
      if (!seen.has(id)) { r.row.remove(); dlRows.delete(id); DlView.forget(String(id)); }
    });
    if (!items.length && !list.querySelector('.dl-empty')) {
      list.appendChild(el('div', { class: 'dl-empty' }, [
        el('span', { class: 'dl-empty-glyph', 'aria-hidden': 'true' }),
        el('strong', { text: 'No downloads yet' }),
        el('span', { text: 'Files you download appear here.' }),
      ]));
    }
  }
  function renderDownloads(c) {
    dlRows.clear();
    const list = el('div', { class: 'dl-list', id: 'dl-list' });
    fillDownloads(list, c.items);
    return [
      el('div', { class: 'panel-title dl-title', text: 'Downloads' }),
      list,
      el('div', { class: 'sep' }),
      el('button', { class: 'item', id: 'dl-all', onclick: () => openPage('downloads') }, [el('span', { class: 'label', text: 'Show all downloads' }), el('span', { class: 'kbd', text: 'Ctrl+J' })]),
    ];
  }

  // Right-click on the title bar or the empty tab strip (0.6.0): Krunk's commands plus the
  // window controls the system menu used to offer there.
  function renderCaptionMenu(c) {
    const win = (action) => { LB.post('captionWindow', { action: action }); close(); };
    return [
      item('New tab', 'Ctrl+T', () => command('newTab')),
      item('New window', 'Ctrl+N', () => command('newWindow')),
      item('New private window', '', () => command('newPrivateWindow')),
      item('Reopen closed tab', 'Ctrl+Shift+T', () => command('reopenClosedTab'), !c.canReopen),
      sep(),
      item('Bookmark all tabs…', '', () => { LB.post('bookmarkAllTabs'); close(); }, !c.canBookmarkAll),
      item('Task manager', 'Shift+Esc', () => command('taskManager')),
      sep(),
      item(c.maximized ? 'Restore' : 'Maximize', '', () => win('maximize')),
      item('Minimize', '', () => win('minimize')),
      item('Close', 'Alt+F4', () => win('close')),
    ];
  }

  // The address bar's context menu (0.6.0). The popup has no focus (the bar keeps its
  // selection); a click sends the choice, the host makes the edit through the shell.
  function renderAddressMenu(c) {
    const act = (action) => { LB.post('addressMenuAction', { action: action }); close(); };
    return [
      item('Undo', 'Ctrl+Z', () => act('undo'), !c.canUndo),
      sep(),
      item('Cut', 'Ctrl+X', () => act('cut'), !c.canCut),
      item('Copy', 'Ctrl+C', () => act('copy'), !c.canCopy),
      item('Paste', 'Ctrl+V', () => act('paste'), !c.canPaste),
      item(c.pasteIsUrl ? 'Paste and go' : 'Paste and search', '', () => act('pasteAndGo'), !c.canPaste),
      item('Delete', 'Del', () => act('delete'), !c.canDelete),
      sep(),
      item('Select all', 'Ctrl+A', () => act('selectAll'), !c.canSelectAll),
    ];
  }

  // Short names for request types (the blocked list's badges).
  const KIND_LABELS = { xmlhttprequest: 'xhr', sub_frame: 'frame', main_frame: 'page', stylesheet: 'css', websocket: 'socket', image: 'image', script: 'script', ping: 'ping', media: 'media', font: 'font', object: 'object', other: 'other', document: 'page' };
  const kindLabel = (k) => KIND_LABELS[k] || k;

  // Right-click on the bookmarks bar: on a bookmark, a folder, or the bar itself.
  function renderBookmarkMenu(c) {
    const act = (action) => LB.post('bookmarkMenuAction', { id: c.id, action: action });
    const mode = (label, value, action) => {
      const b = iconItem(c.mode === value ? 'm-check' : null, label, '', () => act(action));
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
      iconItem('m-star', 'Add this page', '', () => act('addPage'), !c.canAdd),
      iconItem('m-folder-add', 'New folder…', '', () => act('newFolder')),
    ];
    const danger = (b) => { b.classList.add('danger'); return b; };
    if (c.id === null || c.id === undefined) {
      return add.concat([sep()], modes, [sep(), iconItem('m-book', 'Bookmark manager', 'Ctrl+Shift+O', () => command('bookmarks'))]);
    }
    if (c.isFolder) {
      return [
        iconItem('m-open-all', c.children ? 'Open all (' + c.children + ')' : 'Open all', '', () => act('openAll'), !c.children),
        sep(),
        iconItem('m-edit', 'Rename…', '', () => act('edit')),
        danger(iconItem('m-delete', c.children ? 'Delete folder and its ' + c.children + ' bookmarks' : 'Delete folder', '', () => act('delete'))),
        sep(),
      ].concat(add, [sep()], modes);
    }
    return [
      iconItem('m-tab', 'Open in new tab', '', () => act('openNewTab')),
      iconItem('m-window', 'Open in new window', '', () => act('openNewWindow')),
      iconItem('m-private', 'Open in private tab', '', () => act('openPrivate')),
      sep(),
      iconItem('m-edit', 'Edit…', '', () => act('edit')),
      danger(iconItem('m-delete', 'Delete', '', () => act('delete'))),
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

  // The shield: the site and its on/off switch, the count, the newest blocked addresses with
  // a type badge each, "Show all" (krunk://adblock), and the site's cookies and data.
  function renderShield(c) {
    const off = !c.enabled;
    const head = el('div', { class: 'pop-head' }, [
      el('div', { class: 'pop-icon shield' }, [el('span', { text: off ? '○' : (c.allowed ? '◌' : '●') })]),
      el('div', { class: 'pop-titles' }, [
        el('h2', { class: 'pop-title', text: off ? 'Ad blocking is off' : (c.allowed ? 'Ads allowed on this site' : 'Blocking ads and trackers') }),
        el('div', { class: 'pop-sub', text: c.host || 'No site' }),
      ]),
      (!off && c.host) ? el('button', {
        class: 'switch' + (c.allowed ? '' : ' on'), id: 'site-toggle', role: 'switch', 'aria-checked': c.allowed ? 'false' : 'true',
        title: c.allowed ? 'Block on this site again' : 'Allow ads on this site',
        'aria-label': c.allowed ? 'Block on this site again' : 'Allow ads on this site',
        onclick: () => { LB.post('shieldAllowSite', { allow: !c.allowed }); close(); },
      }, [el('span', { class: 'knob' })]) : null,
    ]);
    const count = el('div', { class: 'pop-card count-card' }, [
      el('span', { class: 'big', text: String(c.blocked) }),
      el('span', { class: 'count-label', text: c.blocked === 1 ? 'request blocked on this page' : 'requests blocked on this page' }),
    ]);
    const list = (c.recent && c.recent.length) ? el('div', { class: 'pop-card blocked-list' }, c.recent.map(function (b) {
      const path = b.url.replace(/^https?:\/\/[^/]+/, '') || '/';
      return el('div', { class: 'blocked', title: b.url }, [
        el('span', { class: 'badge kind-' + b.kind, text: kindLabel(b.kind) }),
        el('span', { class: 'addr' }, [el('strong', { text: b.host }), el('span', { text: path })]),
        b.count > 1 ? el('span', { class: 'count', text: '×' + b.count }) : null,
      ]);
    }).concat([el('button', { class: 'link', id: 'show-blocked', onclick: () => { LB.post('openBlockedPage'); close(); },
      text: 'Show all ' + c.distinct + ' blocked addresses' })])) : null;
    const foot = c.host ? el('div', { class: 'pop-row' }, [
      el('button', { class: 'btn', id: 'site-data', onclick: () => { LB.post('openSiteData'); close(); }, text: 'Cookies and site data' }),
    ]) : null;
    return [el('div', { class: 'panel pop' }, [head, count, list, foot])];
  }

  // The lock popup: the connection, a warning when something is wrong, the certificate.
  function dateText(unix) {
    const d = new Date(unix * 1000);
    return isNaN(d.getTime()) ? '?' : d.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
  }
  function certCard(cert, first) {
    const kv = (k, v) => el('div', { class: 'kv' }, [el('span', { class: 'k', text: k }), el('span', { class: 'v', text: v })]);
    const expired = cert.notAfter * 1000 < Date.now();
    return el('div', { class: 'pop-card cert' + (first ? ' leaf' : '') }, [
      el('div', { class: 'cert-name', text: cert.subject || '(no name)' }),
      cert.subjectOrg ? kv('Organisation', cert.subjectOrg) : null,
      cert.sans && cert.sans.length ? kv('Names', cert.sans.join(', ')) : null,
      kv('Issued by', cert.issuer + (cert.issuerOrg ? ' (' + cert.issuerOrg + ')' : '')),
      kv('Valid', dateText(cert.notBefore) + ' – ' + dateText(cert.notAfter) + (expired ? ' (expired)' : '')),
      kv('Key', cert.keyAlgorithm),
      kv('Signature', cert.signatureAlgorithm),
      kv('SHA-256', cert.sha256),
    ]);
  }
  function renderSiteInfo(c) {
    const i = c.info;
    let title, sub, icon;
    if (i.kind === 'internal') { title = 'Krunk page'; sub = 'A built-in page of Krunk'; icon = 'K'; }
    else if (i.kind === 'file') { title = 'Local file'; sub = 'A file on this computer'; icon = '▤'; }
    else if (i.kind === 'http') { title = 'Connection is not secure'; sub = 'No certificate. Anything you send can be read on the way.'; icon = '!'; }
    else if (i.kind !== 'https') { title = 'No connection information'; sub = ''; icon = '?'; }
    else if (i.warnings.length) { title = 'Connection is not secure'; sub = i.host; icon = '!'; }
    else if (i.secure) { title = 'Connection is secure'; sub = i.host; icon = '🔒'; }
    else { title = 'Connection information unavailable'; sub = i.host; icon = '?'; }
    const state = i.kind !== 'https' ? 'plain' : (i.warnings.length || !i.secure ? 'warn' : 'ok');
    const head = el('div', { class: 'pop-head' }, [
      el('div', { class: 'pop-icon ' + state }, [el('span', { text: icon })]),
      el('div', { class: 'pop-titles' }, [el('h2', { class: 'pop-title', text: title }), el('div', { class: 'pop-sub', text: sub })]),
    ]);
    const parts = [head];
    (i.warnings || []).forEach((w) => parts.push(el('div', { class: 'pop-card warning', text: w })));
    if (i.kind === 'http') parts.push(el('div', { class: 'pop-card plain-note', text: 'Connection is not secure. No certificate.' }));
    if (i.kind === 'https') {
      const kv = (k, v) => el('div', { class: 'kv' }, [el('span', { class: 'k', text: k }), el('span', { class: 'v', text: v })]);
      parts.push(el('div', { class: 'pop-card' }, [
        kv('Protocol', i.protocol || 'not reported by the engine'),
        kv('Cipher', i.cipher || 'not reported by the engine'),
        kv('Certificate', i.certs.length ? (i.certs.length === 1 ? '1 certificate' : i.certs.length + ' certificates in the chain') : 'none'),
      ]));
      if (i.certs.length) {
        parts.push(certCard(i.certs[0], true));
        if (i.certs.length > 1) {
          const chain = el('div', { id: 'cert-chain', hidden: true }, i.certs.slice(1).map((cert) => certCard(cert, false)));
          const toggle = el('button', { class: 'link', id: 'cert-details', text: 'Certificate details (' + (i.certs.length - 1) + ' more)' });
          toggle.addEventListener('click', () => {
            chain.hidden = !chain.hidden;
            toggle.textContent = chain.hidden ? 'Certificate details (' + (i.certs.length - 1) + ' more)' : 'Hide certificate details';
            fit();
          });
          parts.push(toggle, chain);
        }
      }
    }
    // Permissions (0.6.0): every kind with its state for this site; a change is stored.
    if (i.origin && i.permissions && i.permissions.length) {
      const rows = i.permissions.map((p) => {
        const sel = el('select', { class: 'perm-select', 'data-kind': p.kind }, ['ask', 'allow', 'block'].map((v) => el('option', { value: v, text: v === 'ask' ? 'Ask' : v === 'allow' ? 'Allow' : 'Block' })));
        sel.value = p.state;
        sel.addEventListener('change', () => { LB.post('permissionSet', { origin: i.origin, kind: p.kind, state: sel.value }); });
        return el('div', { class: 'kv perm' }, [el('span', { class: 'k', text: PERM_LABEL[p.kind] || p.kind }), sel]);
      });
      parts.push(el('div', { class: 'pop-card', id: 'site-permissions' }, [el('div', { class: 'pop-sub', text: 'Permissions' })].concat(rows)));
    }
    return [el('div', { class: 'panel pop' }, parts)];
  }
  // The popup asks the host for the height its content needs: #root's height (the body is
  // pinned to the window's height, so it cannot shrink) plus the body's 1px borders.
  function fit() { LB.post('popupResize', { height: Math.max(root.offsetHeight, root.scrollHeight) + 2 }); }
  // Content whose height depends on its items: the window is opened with an upper bound
  // and fitted right after the render (the ⋮ menu used to leave a gap at the bottom).
  const FITS = { extInstall: true, suggest: true, cardFill: true, menu: true, tabMenu: true, navMenu: true, httpAuth: true, bookmarkMenu: true, siteInfo: true, addressMenu: true, captionMenu: true, downloads: true, permission: true, passwordSaved: true, pip: true, bookmarkFolder: true };


  // Login suggestions under a focused field: the matching usernames (with the host for a
  // sister site of the same domain), or "Unlock Krunk passwords" while the vault is locked.
  // The popup never has the focus here (the field keeps it): plain clicks only.
  function renderSuggest(c) {
    const pick = (id) => { LB.post('suggestPick', { tabId: c.tabId, frameId: c.frameId, entryId: id }); close(); };
    const rows = [];
    if (c.locked) {
      rows.push(el('button', { class: 'suggest locked', id: 'suggest-unlock', onclick: () => LB.post('suggestUnlock', { tabId: c.tabId, frameId: c.frameId }) }, [
        el('span', { class: 'key-glyph', text: '🔒' }), el('span', { class: 'label', text: 'Unlock Krunk passwords' }),
      ]));
    } else {
      c.entries.forEach((e) => rows.push(el('button', { class: 'suggest', onclick: () => pick(e.id) }, [
        el('span', { class: 'key-glyph', text: '🔑' }),
        el('span', { class: 'label' }, [el('span', { class: 'user', text: e.username || '(no username)' }), e.host ? el('span', { class: 'host', text: e.host }) : null]),
      ])));
      // 0.6.0: a generated password for a new-password field; the host fills what it offered.
      if (c.generated) {
        rows.push(el('button', { class: 'suggest generated', id: 'suggest-generate', onclick: () => { LB.post('suggestGenerate', { tabId: c.tabId, frameId: c.frameId }); close(); } }, [
          el('span', { class: 'key-glyph', text: '✨' }),
          el('span', { class: 'label' }, [el('span', { class: 'user', text: 'Use a strong password' }), el('span', { class: 'host mono', text: c.generated })]),
        ]));
      }
    }
    rows.push(el('div', { class: 'suggest-foot', text: 'Krunk passwords · ' + c.origin.replace(/^https?:\/\//, '') }));
    return [el('div', { class: 'suggest-list' }, rows)];
  }

  // The Picture-in-Picture chooser (0.6.0): the page's videos; the host asks the page.
  function renderPip(c) {
    const rows = c.videos.map((v) => el('button', { class: 'item pip-row', 'data-index': String(v.index), onclick: () => { LB.post('pipPick', { tabId: c.tabId, index: v.index }); close(); } }, [
      el('span', { class: 'label' }, [
        el('span', { class: 'pip-name', text: v.label || 'Video ' + (v.index + 1) }),
        el('span', { class: 'pip-meta', text: (v.width && v.height ? v.width + '×' + v.height + ' · ' : '') + (v.pip ? 'in Picture-in-Picture' : v.playing ? 'playing' : 'paused') }),
      ]),
    ]));
    return [el('div', { class: 'panel-title', text: 'Which video?' })].concat(rows);
  }

  // The volume slider of the floating video window (0.7.0, R8; 0.11.0: inside the player, next
  // to the engine's own mute button, which stays the mute). The host shows and places it; it
  // never takes the keyboard. A change goes to the page at most every 50 ms; the page's own
  // changes come back as a new `pipVolume` content, applied unless the slider is being dragged.
  const pv = { tabId: 0, volume: 100, muted: false, dragging: false, timer: null, sentAt: 0 };
  function pvSend() {
    pv.sentAt = Date.now();
    if (pv.timer) return;
    pv.timer = setTimeout(function () {
      pv.timer = null;
      LB.post('pipVolume', { tabId: pv.tabId, volume: pv.volume, muted: pv.muted });
    }, 50);
  }
  function pvPaint() {
    const slider = document.getElementById('pv-slider');
    if (!slider) return;
    slider.value = String(pv.muted ? 0 : pv.volume);
    slider.style.setProperty('--fill', (pv.muted ? 0 : pv.volume) + '%');
    slider.setAttribute('aria-valuetext', pv.muted ? 'Muted' : pv.volume + '%');
    document.getElementById('pv-pct').textContent = pv.muted ? '—' : String(pv.volume);
  }
  function pvSet(volume, muted) {
    pv.volume = Math.max(0, Math.min(100, Math.round(volume)));
    pv.muted = muted;
    pvPaint();
    pvSend();
  }
  function renderPipVolume(c) {
    pv.tabId = c.tabId; pv.volume = c.volume; pv.muted = c.muted;
    const slider = el('input', { type: 'range', id: 'pv-slider', min: '0', max: '100', step: '1', 'aria-label': 'Volume', tabindex: '-1' });
    slider.addEventListener('input', function () { pvSet(Number(slider.value), false); });
    slider.addEventListener('pointerdown', function () { pv.dragging = true; });
    const done = function () { pv.dragging = false; };
    slider.addEventListener('pointerup', done);
    slider.addEventListener('pointercancel', done);
    const bar = el('div', { class: 'pv-bar' }, [slider, el('span', { id: 'pv-pct', class: 'pv-pct', 'aria-hidden': 'true' })]);
    bar.addEventListener('wheel', function (e) {
      e.preventDefault();
      pvSet((pv.muted ? 0 : pv.volume) + (e.deltaY < 0 ? 5 : -5), false);
    }, { passive: false });
    requestAnimationFrame(pvPaint);
    return [bar];
  }

  // "Add this extension?" (0.7.0, R20): every install path asks first, with what it may do.
  function renderExtInstall(c) {
    const reply = (accept) => { LB.post('extInstallReply', { requestId: c.requestId, accept: accept }); };
    const list = c.warnings.length
      ? el('ul', { class: 'ext-warnings' }, c.warnings.map((w) => el('li', { text: w })))
      : el('p', { class: 'muted', text: 'It needs no special permissions.' });
    return [el('div', { class: 'panel ext-install' }, [
      el('h2', { text: (c.previous ? 'Update “' : 'Add “') + c.name + '”?' }),
      el('p', { class: 'muted ext-meta', text: 'Version ' + c.version + (c.previous ? ' (installed: ' + c.previous + ')' : '') + ' · ' + c.source }),
      c.warnings.length ? el('p', { class: 'ext-can', text: 'It can:' }) : null,
      list,
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', id: 'ext-cancel', onclick: () => reply(false), text: 'Cancel' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true), text: c.previous ? 'Update' : 'Add extension' }),
      ]),
    ])];
  }

  // Saved cards under a card field (0.6.0): masked labels; the host fills what was chosen.
  function renderCardFill(c) {
    const rows = [];
    if (c.locked) {
      rows.push(el('button', { class: 'suggest locked', id: 'suggest-unlock', onclick: () => LB.post('suggestUnlock', { tabId: c.tabId, frameId: c.frameId }) }, [
        el('span', { class: 'key-glyph', text: '🔒' }), el('span', { class: 'label', text: 'Unlock Krunk passwords' }),
      ]));
    } else {
      c.cards.forEach((k) => rows.push(el('button', { class: 'suggest card', 'data-id': k.id, onclick: () => { LB.post('cardFillPick', { tabId: c.tabId, frameId: c.frameId, cardId: k.id }); close(); } }, [
        el('span', { class: 'key-glyph', text: '💳' }),
        el('span', { class: 'label' }, [el('span', { class: 'user', text: k.label }), k.name ? el('span', { class: 'host', text: k.name }) : null]),
      ])));
    }
    rows.push(el('div', { class: 'suggest-foot', text: 'Krunk payment methods · ' + c.origin.replace(/^https?:\/\//, '') }));
    return [el('div', { class: 'suggest-list' }, rows)];
  }

  // "Save this card?" after a checkout (0.6.0): the brand and last four digits only.
  function renderSaveCard(c) {
    const reply = (accept) => { LB.post('saveCardReply', { tabId: c.tabId, origin: c.origin, accept: accept }); close(); };
    return [el('div', { class: 'panel' }, [
      el('h2', { text: 'Save this card?' }),
      el('p', {}, [el('strong', { text: c.brand + ' •••• ' + c.last4 }), ' used on ', el('span', { class: 'origin', text: c.origin.replace(/^https?:\/\//, '') }), '. The security code is never stored.']),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', id: 'card-not-now', onclick: () => reply(false), text: 'Not now' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true), text: 'Save card' }),
      ]),
    ])];
  }

  // "Password saved" after a generated password was used (0.6.0): Undo puts things back,
  // Edit opens the entry; it goes away by itself.
  function renderPasswordSaved(c) {
    const act = (cmd) => { LB.post(cmd, { entryId: c.entryId }); close(); };
    setTimeout(function () { if (current && current.kind === 'passwordSaved' && current.entryId === c.entryId) close(); }, 8000);
    return [el('div', { class: 'panel toast' }, [
      el('p', {}, [el('strong', { text: c.update ? 'Password updated' : 'Password saved' }), c.username ? ' for ' + c.username : '', ' on ', el('span', { class: 'origin', text: c.origin.replace(/^https?:\/\//, '') })]),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', id: 'saved-undo', onclick: () => act('passwordSavedUndo'), text: 'Undo' }),
        el('button', { class: 'btn', id: 'saved-edit', onclick: () => act('passwordSavedEdit'), text: 'Edit' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => close(), text: 'OK' }),
      ]),
    ])];
  }

  function renderSave(c) {
    // 0.11.2: the account the password is for, shown and editable before saving.
    const user = el('input', { id: 'save-username', type: 'text', value: c.username || '', placeholder: 'Username or email',
      autocomplete: 'off', spellcheck: 'false', maxlength: '256', 'aria-label': 'Username' });
    const reply = (accept, never) => {
      LB.post('savePasswordReply', { tabId: c.tabId, origin: c.origin, accept: accept, never: never, username: user.value });
      close();
    };
    user.addEventListener('keydown', function (e) { if (e.key === 'Enter') { e.preventDefault(); reply(true, false); } });
    return [el('div', { class: 'panel save-login' }, [
      el('h2', { text: c.update ? 'Update saved password?' : 'Save password?' }),
      el('p', {}, ['On ', el('span', { class: 'origin', text: c.origin })]),
      el('label', { class: 'field' }, [el('span', { text: 'Username' }), user]),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', onclick: () => reply(false, true), text: 'Never for this site' }),
        el('button', { class: 'btn', onclick: () => reply(false, false), text: 'Not now' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true, false), text: c.update ? 'Update' : 'Save' }),
      ]),
    ])];
  }

  // Address-bar suggestions (0.5.1): the popup never has the focus (the address bar keeps
  // it); the shell's keyboard selection arrives as `selected`. A click goes to the row.
  function renderAddressSuggest(c) {
    const glyph = { search: '🔍', url: '🌐', history: '🕘', bookmark: '★' };
    const label = { search: 'Search', url: 'Go to', history: '', bookmark: '' };
    const rows = c.entries.map(function (s, i) {
      const btn = el('button', { class: 'suggest addr' + (c.selected === i ? ' selected' : ''), 'data-index': String(i), title: s.url }, [
        el('span', { class: 'glyph', text: glyph[s.kind] || '' }),
        el('span', { class: 'title', text: (label[s.kind] ? label[s.kind] + ' ' : '') + s.title }),
        el('span', { class: 'url', text: s.kind === 'search' || s.kind === 'url' ? '' : s.url }),
      ]);
      btn.addEventListener('click', function () { LB.post('addressSuggestPick', { url: s.url }); });
      return btn;
    });
    return [el('div', { class: 'suggest-list addr-list' }, rows)];
  }

  // "Connect to devices on your local network?" (Krunk 0.5.0): the engine's local network
  // access check, answered by the user; remembered for the site unless the tab is private.
  // What each permission kind asks for (0.6.0; the keys match core::permissions::Kind).
  const PERM = {
    camera: ['Use your camera?', 'use your camera'],
    microphone: ['Use your microphone?', 'use your microphone'],
    geolocation: ['Know your location?', 'know your location'],
    notifications: ['Show notifications?', 'show notifications'],
    clipboardRead: ['Read your clipboard?', 'read text and images you copied'],
    midi: ['Use your MIDI devices?', 'control your MIDI devices (including system-exclusive messages)'],
    screenShare: ['Share your screen?', 'see your screen'],
    localNetwork: ['Connect to devices on your local network?', 'connect to a device on your local network (a NAS, a media server, a printer…)'],
  };
  const PERM_LABEL = { camera: 'Camera', microphone: 'Microphone', geolocation: 'Location', notifications: 'Notifications', clipboardRead: 'Clipboard (read)', midi: 'MIDI devices', screenShare: 'Screen sharing', localNetwork: 'Local network' };
  function renderPermission(c) {
    const remember = el('input', { type: 'checkbox', id: 'remember', checked: !c.private });
    const reply = (allow) => {
      LB.post('permissionReply', { promptId: c.promptId, allow: allow, remember: !c.private && remember.checked });
      close();
    };
    let host = c.origin;
    try { host = new URL(c.origin).host; } catch (e) { /* keep the origin */ }
    const kinds = (c.kinds && c.kinds.length) ? c.kinds : ['localNetwork'];
    const one = kinds.length === 1 ? kinds[0] : null;
    const title = one === 'localNetwork' && c.loopback ? 'Connect to this computer?' : one ? PERM[one][0] : 'Allow ' + kinds.map((k) => PERM_LABEL[k] || k).join(' and ').toLowerCase() + '?';
    const wants = kinds.map((k) => (k === 'localNetwork' && c.loopback) ? 'connect to a program on this computer' : (PERM[k] ? PERM[k][1] : k)).join(' and ');
    return [el('div', { class: 'panel', 'data-kinds': kinds.join(' ') }, [
      el('h2', { text: title }),
      el('p', {}, [el('span', { class: 'origin', text: host }), ' wants to ' + wants + '.']),
      c.private
        ? el('p', { class: 'muted', text: 'Private tab: this answer is not remembered.' })
        : el('label', { class: 'check' }, [remember, ' Remember for this site']),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', onclick: () => reply(false), text: 'Block' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true), text: 'Allow' }),
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
    // 0.11.0: checked (the remembered default), the vault stays open until Krunk quits.
    const remain = el('input', { type: 'checkbox', id: 'remain' });
    remain.checked = c.remain !== false;
    const remainRow = el('label', { class: 'check remain', title: 'Unchecked, the vault locks itself after the time set in Settings → Passwords' },
      [remain, el('span', { text: 'Remain unlocked this session' })]);
    const form = el('form', {
      onsubmit: (e) => {
        e.preventDefault();
        if (!input.value || button.disabled) return;
        button.disabled = true;
        button.textContent = 'Unlocking…';
        LB.post('vaultUnlock', { password: input.value, remain: remain.checked });
        input.value = '';
      },
    }, [input, remainRow, el('div', { class: 'row' }, [
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
      el('h2', { text: c.purpose === 'save' ? 'Unlock to save this password' : c.purpose === 'saveCard' ? 'Unlock to save this card' : c.purpose === 'cardFill' ? 'Unlock to fill this card' : c.purpose === 'import' ? 'Unlock to import passwords' : c.purpose === 'httpAuth' ? 'Unlock to use a saved login' : 'Unlock to fill this login' }),
      el('p', { class: 'origin', text: c.origin }),
      form,
      err,
      // 0.7.0 (R13): the passwords page opens the vault with the recovery phrase.
      el('button', { type: 'button', class: 'link forgot', id: 'forgot', text: 'Forgot master password?', onclick: () => { LB.post('openPage', { page: 'passwords', section: 'recover' }); close(); } }),
    ])];
  }

  // A site's HTTP sign-in (0.8.0). One saved login of this origin fills the fields (its
  // password stays in the vault: the host sends it after the click), several are offered as a
  // list, a locked vault offers to unlock. Nothing is sent without a click on Sign in.
  function renderHttpAuth(c) {
    let chosen = null;
    const user = el('input', { type: 'text', id: 'auth-user', autocomplete: 'off', spellcheck: 'false', 'aria-label': 'User name', placeholder: 'User name' });
    const pass = el('input', { type: 'password', id: 'auth-pass', autocomplete: 'off', 'aria-label': 'Password', placeholder: 'Password' });
    const picks = [];
    function choose(l, btn) {
      chosen = l.entryId;
      user.value = l.username;
      pass.value = '';
      pass.placeholder = 'Saved password';
      picks.forEach((b) => b.classList.toggle('chosen', b === btn));
    }
    function typed() {
      if (!chosen) return;
      chosen = null;
      pass.placeholder = 'Password';
      picks.forEach((b) => b.classList.remove('chosen'));
    }
    user.addEventListener('input', typed);
    pass.addEventListener('input', typed);
    const form = el('form', {
      class: 'auth-form',
      onsubmit: (e) => {
        e.preventDefault();
        if (chosen) LB.post('httpAuthReply', { tabId: c.tabId, entryId: chosen });
        else LB.post('httpAuthReply', { tabId: c.tabId, username: user.value, password: pass.value });
        pass.value = '';
      },
    }, [
      el('label', { class: 'auth-label', for: 'auth-user', text: 'User name' }), user,
      el('label', { class: 'auth-label', for: 'auth-pass', text: 'Password' }), pass,
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', type: 'button', id: 'auth-cancel', text: 'Cancel', onclick: () => LB.post('httpAuthCancel', { tabId: c.tabId }) }),
        el('button', { class: 'btn primary', type: 'submit', id: 'auth-ok', text: 'Sign in' }),
      ]),
    ]);
    const saved = c.logins.length > 1 ? el('div', { class: 'auth-saved', role: 'group', 'aria-label': 'Saved logins' },
      [el('p', { class: 'muted', text: 'Saved logins for this site:' })].concat(c.logins.map(function (l) {
        const b = el('button', { type: 'button', class: 'choice auth-pick', text: l.username || '(no user name)' });
        b.addEventListener('click', () => { choose(l, b); document.getElementById('auth-ok').focus(); });
        picks.push(b);
        return b;
      }))) : null;
    if (c.logins.length === 1) choose(c.logins[0], null);
    setTimeout(() => (chosen ? document.getElementById('auth-ok') : user).focus(), 0);
    return [el('div', { class: 'panel http-auth' }, [
      el('h2', { text: 'Sign in' }),
      el('p', { class: 'origin', text: c.origin }),
      c.realm ? el('p', { class: 'muted auth-realm', text: 'The site says: ' + c.realm }) : null,
      c.insecure ? el('p', { class: 'auth-insecure', text: 'Not secure: this site uses http, so the password is sent unencrypted.' }) : null,
      c.failed ? el('p', { class: 'error', role: 'alert', text: 'The user name or password was not accepted. Try again.' }) : null,
      saved,
      c.locked && !c.logins.length ? el('button', { type: 'button', class: 'link', id: 'auth-unlock', text: 'Unlock Krunk passwords to use a saved login', onclick: () => LB.post('httpAuthUnlock', { tabId: c.tabId }) }) : null,
      form,
    ])];
  }

  // A folder dropdown: its items open, subfolders open in place, a right-click shows the
  // bar's menu for the item, and an item can be dragged: within the list to reorder it or
  // into a subfolder (the same marks as on the bar), or out of the popup, where the host
  // follows the mouse and the bar takes the drop.
  const DRAG_THRESHOLD = 5;
  const folderDrag = { item: null, id: 0, pointerId: null, startX: 0, startY: 0, moved: false, target: null, suppressClick: false };
  function renderFolder(c) {
    // A subfolder opened in place has a way back up (0.11.0): "‹ <parent>" as the first row.
    const back = c.back ? el('button', { class: 'item folder-back', id: 'folder-back', title: 'Back to ' + c.back, onclick: () => LB.post('bookmarkFolderBack') }, [
      el('span', { class: 'folder-glyph', text: '‹' }), el('span', { class: 'label', text: c.back }),
    ]) : null;
    if (!c.items.length) return [back, el('div', { class: 'panel muted', text: '(empty)' })];
    const rows = c.items.map(function (b) {
      const icon = b.isFolder ? el('span', { class: 'folder-glyph', text: '▸' })
        : b.favicon ? el('img', { class: 'fav', src: b.favicon, alt: '' }) : el('span', { class: 'fav dot' });
      // One line, shortened with "…"; the tooltip has the whole title and the address (R9).
      const btn = el('button', { class: 'item', title: b.url ? (b.title ? b.title + '\n' + b.url : b.url) : b.title, 'data-id': b.id }, [icon, el('span', { class: 'label', text: b.title || b.url || '' })]);
      if (b.isFolder) btn.classList.add('folder');
      btn.addEventListener('click', function (e) {
        if (folderDrag.suppressClick) return;
        if (b.isFolder) { LB.post('showBookmarkFolder', { id: b.id }); return; }
        LB.post('openUrl', { url: b.url, newTab: e.ctrlKey });
        close();
      });
      // Middle-click: a new tab behind this one, and the dropdown stays open (0.11.0), as on
      // the bar. A folder has no middle-click (the bar has none either).
      btn.addEventListener('auxclick', function (e) {
        if (e.button === 1) { e.preventDefault(); if (b.url) LB.post('openBookmarkBackground', { id: b.id }); }
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
    return back ? [back].concat(rows) : rows;
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
      // The bar's overflow lists the bar's folder from `offset` on (0.7.2).
      LB.post('moveBookmarkInFolder', { id: id, parent: current.id, index: t.index + (current.offset || 0) });
    }
  }
  root.addEventListener('pointerup', function (e) { endFolderDrag(e, true); });
  root.addEventListener('pointercancel', function (e) { endFolderDrag(e, false); });

  // The tab list (0.11.0): every tab of the window, filtered by the search field, with ×.
  // Only the rows in view exist (a window can have hundreds of tabs).
  const TL_ROW = 34;
  const tl = { tabs: [], active: null, shown: [], sel: 0 };
  function renderTabList(c) {
    tl.tabs = c.tabs; tl.active = c.active; tl.sel = -1;
    const search = el('input', { id: 'tl-search', class: 'tl-search', type: 'search', placeholder: 'Search tabs', 'aria-label': 'Search tabs', spellcheck: 'false', autocomplete: 'off' });
    const space = el('div', { id: 'tl-space', class: 'tl-space' });
    const list = el('div', { id: 'tl-list', class: 'tl-list', role: 'listbox', 'aria-label': 'Tabs' }, [space]);
    search.addEventListener('input', function () { tl.sel = 0; tlFilter(); list.scrollTop = 0; });
    search.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (!tl.shown.length) return;
        tl.sel = (tl.sel + (e.key === 'ArrowDown' ? 1 : -1) + tl.shown.length) % tl.shown.length;
        const top = tl.sel * TL_ROW;
        if (top < list.scrollTop) list.scrollTop = top;
        else if (top + TL_ROW > list.scrollTop + list.clientHeight) list.scrollTop = top + TL_ROW - list.clientHeight;
        tlRows();
      } else if (e.key === 'Enter') {
        const t = tl.shown[Math.max(0, tl.sel)];
        if (t) { LB.post('activateTab', { id: t.id }); close(); }
      }
    });
    list.addEventListener('scroll', tlRows);
    // After the first layout: the list's height is known, the current tab in view.
    requestAnimationFrame(function () {
      tlFilter();
      const at = tl.shown.findIndex(function (t) { return t.id === tl.active; });
      if (at >= 0) { list.scrollTop = Math.max(0, at * TL_ROW - list.clientHeight / 2 + TL_ROW / 2); tlRows(); }
    });
    return [el('div', { class: 'tl' }, [search, list])];
  }
  function tlFilter() {
    const search = document.getElementById('tl-search');
    const q = search ? search.value.trim().toLowerCase() : '';
    tl.shown = tl.tabs.filter(function (t) { return !q || (t.title || '').toLowerCase().indexOf(q) >= 0 || (t.url || '').toLowerCase().indexOf(q) >= 0; });
    if (tl.sel >= tl.shown.length) tl.sel = tl.shown.length - 1;
    const space = document.getElementById('tl-space');
    if (space) space.style.height = Math.max(TL_ROW, tl.shown.length * TL_ROW) + 'px';
    tlRows();
  }
  function tlRows() {
    const list = document.getElementById('tl-list'), space = document.getElementById('tl-space');
    if (!list || !space) return;
    if (!tl.shown.length) { space.replaceChildren(el('div', { class: 'tl-empty', text: 'No tab matches.' })); return; }
    const first = Math.max(0, Math.floor(list.scrollTop / TL_ROW) - 4);
    const last = Math.min(tl.shown.length, Math.ceil((list.scrollTop + (list.clientHeight || 480)) / TL_ROW) + 4);
    const rows = [];
    for (let i = first; i < last; i++) rows.push(tlRow(tl.shown[i], i));
    space.replaceChildren.apply(space, rows);
  }
  function tlRow(t, i) {
    const icon = t.favicon ? el('img', { class: 'fav', src: t.favicon, alt: '' }) : el('span', { class: 'fav dot' });
    const x = el('button', { class: 'tl-close', title: 'Close tab', 'aria-label': 'Close ' + (t.title || 'tab'), tabindex: '-1',
      onclick: function (e) { e.stopPropagation(); LB.post('closeTab', { id: t.id }); } }, [el('span', { text: '×', 'aria-hidden': 'true' })]);
    const row = el('div', { class: 'item tl-row' + (t.id === tl.active ? ' current' : '') + (i === tl.sel ? ' sel' : '') + (t.private ? ' private' : ''),
      role: 'option', 'aria-selected': i === tl.sel ? 'true' : 'false', title: (t.title || '') + (t.url ? '\n' + t.url : ''), 'data-id': t.id },
      [icon, el('span', { class: 'label', text: t.title || t.url || 'New tab' }), t.audible ? el('span', { class: 'tl-audio', text: t.muted ? '🔇' : '🔊', 'aria-hidden': 'true' }) : null, x]);
    row.style.top = (i * TL_ROW) + 'px';
    row.addEventListener('click', function () { LB.post('activateTab', { id: t.id }); close(); });
    // Middle-click closes, as on the strip.
    row.addEventListener('auxclick', function (e) { if (e.button === 1) { e.preventDefault(); LB.post('closeTab', { id: t.id }); } });
    return row;
  }

  // A page that stopped responding (0.11.0): wait for it, or end it (the tab then shows the
  // crash page with Reload). Esc and a click elsewhere just close the prompt (waiting).
  function renderUnresponsive(c) {
    const reply = (wait) => { LB.post('hangReply', { tabId: c.tabId, wait: wait }); close(); };
    return [el('div', { class: 'panel hang' }, [
      el('h2', { text: 'This page isn\'t responding' }),
      el('p', { text: (c.host ? c.host + ' is' : 'The page is') + ' busy or stuck. The rest of Krunk keeps working. You can wait for it, or close the page; you can reload it afterwards.' }),
      el('div', { class: 'row' }, [
        el('button', { class: 'btn', id: 'hang-close', onclick: () => reply(false), text: 'Close page' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply(true), text: 'Wait' }),
      ]),
    ])];
  }

  // "Krunk X is available" and the download that follows "Update now".
  function renderUpdate(c) {
    const reply = (choice) => LB.post('updateReply', { choice: choice });
    const s = c.state || { step: 'offered' };
    // A repair (0.7.0): the installed files are not this release's; the same steps reinstall it.
    const body = [el('h2', { text: c.repair ? 'Repair Krunk ' + c.version : 'Krunk ' + c.version + ' is available' })];
    if (s.step === 'downloading') {
      body.push(el('p', { text: 'Downloading… ' + s.percent + '%' }));
      body.push(el('progress', { max: '100', value: String(s.percent) }));
      body.push(el('div', { class: 'row' }, [el('button', { class: 'btn', id: 'default', onclick: close, text: 'Hide' })]));
    } else if (s.step === 'installing') {
      body.push(el('p', { text: 'Installing. Krunk closes now and starts again when the update is done.' }));
    } else {
      body.push(s.step === 'failed'
        ? el('p', { class: 'error', text: 'The update failed: ' + s.error })
        : el('p', { text: c.repair ? c.repair + '. ' + (c.install ? 'Krunk reinstalls this version and restarts; your tabs and data are kept.' : 'Download Krunk again from the Krunk website.')
          : c.install ? 'You have ' + c.current + '. Krunk restarts to finish the update; your tabs are restored.'
          : 'You have ' + c.current + '. Download the new version from the Krunk website.' }));
      body.push(el('div', { class: 'row' }, [
        c.hasNotes ? el('button', { class: 'btn', onclick: () => { reply('notes'); }, text: 'Release notes' }) : null,
        el('button', { class: 'btn', onclick: () => { reply('later'); }, text: 'Later' }),
        el('button', { class: 'btn primary', id: 'default', onclick: () => reply('install'),
          text: !c.install ? 'Download' : s.step === 'failed' ? 'Try again' : c.repair ? 'Repair' : 'Update now' }),
      ]));
    }
    return [el('div', { class: 'panel' }, body)];
  }

  // A notice that takes no focus ("Press Esc to show your cursor").


  let current = null;
  // The host shows the window once content numbered `seq` is painted (0.11.0): two frames,
  // so the first one with it is on screen when the window opens. The height comes with it.
  function answerShown(m, c) {
    if (m.seq === undefined || m.seq === null) return false;
    const seq = m.seq;
    requestAnimationFrame(function () {
      // The content's own height, not the scrolling box's (it is capped at the window).
      const height = FITS[c.kind] ? Math.max(root.offsetHeight, root.scrollHeight) + 2 : null;
      requestAnimationFrame(function () { LB.post('popupShown', { seq: seq, height: height }); });
    });
    return true;
  }
  LB.on('popup', function (m) {
    const c = m.content;
    // The downloads list is updated in place (the bars keep moving; hover stays).
    if (c.kind === 'downloads' && current && current.kind === 'downloads' && document.getElementById('dl-list')) {
      const before = dlRows.size;
      current = c;
      fillDownloads(document.getElementById('dl-list'), c.items);
      if (!answerShown(m, c) && dlRows.size !== before) requestAnimationFrame(fit);
      return;
    }
    // The volume bar is updated in place (a re-render would drop a drag in progress).
    if (c.kind === 'pipVolume' && current && current.kind === 'pipVolume' && document.getElementById('pv-slider')) {
      current = c;
      pv.tabId = c.tabId;
      if (!pv.dragging && !pv.timer && Date.now() - pv.sentAt > 300) { pv.volume = c.volume; pv.muted = c.muted; pvPaint(); }
      answerShown(m, c);
      return;
    }
    // The tab list keeps its search and scroll position (0.11.0).
    if (c.kind === 'tabList' && current && current.kind === 'tabList' && document.getElementById('tl-list')) {
      current = c;
      tl.tabs = c.tabs;
      tl.active = c.active;
      tlFilter();
      answerShown(m, c);
      return;
    }
    current = m.content;
    const render = { menu: renderMenu, shield: renderShield, siteInfo: renderSiteInfo, suggest: renderSuggest, savePassword: renderSave, passwordSaved: renderPasswordSaved, cardFill: renderCardFill, saveCard: renderSaveCard, pip: renderPip, pipVolume: renderPipVolume, extInstall: renderExtInstall, fill: renderFill, permission: renderPermission, addressSuggest: renderAddressSuggest, addressMenu: renderAddressMenu, captionMenu: renderCaptionMenu, downloads: renderDownloads, bookmarkFolder: renderFolder, unlock: renderUnlock, update: renderUpdate, tabMenu: renderTabMenu, navMenu: renderNavMenu, httpAuth: renderHttpAuth, bookmarkMenu: renderBookmarkMenu, bookmarkEdit: renderBookmarkEdit, tabList: renderTabList, unresponsive: renderUnresponsive }[c.kind];
    root.textContent = '';
    root.dataset.kind = c.kind;
    // The volume bar matches the engine's floating window, which is always dark.
    document.body.classList.toggle('pv-dark', c.kind === 'pipVolume');
    (render ? render(c) : []).forEach((n) => n && root.appendChild(n));
    const first = document.getElementById('master') || document.getElementById('bm-name') || document.getElementById('default') || root.querySelector('button:not([disabled])');
    // The first item, never "‹ Back" (Enter would leave the folder just opened).
    const firstItem = c.kind === 'bookmarkFolder' && first && first.id === 'folder-back' ? (root.querySelector('button.item:not(#folder-back)') || first) : first;
    if (firstItem && c.kind !== 'suggest' && c.kind !== 'cardFill' && c.kind !== 'addressMenu' && c.kind !== 'addressSuggest' && c.kind !== 'pipVolume') firstItem.focus();
    if (!answerShown(m, c) && FITS[c.kind]) requestAnimationFrame(fit);
    if (c.kind === 'tabList') document.getElementById('tl-search').focus();
  });
  // The window was hidden: nothing of this content may show with the next one.
  LB.on('popupClear', function () {
    current = null;
    root.textContent = '';
    delete root.dataset.kind;
  });
  // A middle press never starts the engine's autoscroll in a popup (0.11.0).
  document.addEventListener('mousedown', function (e) { if (e.button === 1) e.preventDefault(); });

  document.addEventListener('keydown', menuKeys);
  document.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      // Dismissing a prompt is never a confirmation.
      if (current && current.kind === 'savePassword') {
        LB.post('savePasswordReply', { tabId: current.tabId, origin: current.origin, accept: false, never: false });
      } else if (current && current.kind === 'extInstall') {
        LB.post('extInstallReply', { requestId: current.requestId, accept: false });
      } else if (current && current.kind === 'httpAuth') {
        LB.post('httpAuthCancel', { tabId: current.tabId });
      } else if (current && current.kind === 'fill') {
        LB.post('fillReply', { tabId: current.tabId, origin: current.origin, entryId: null });
      }
      close();
    }
  });
  document.addEventListener('contextmenu', (e) => e.preventDefault());

  LB.post('ready');
})();
