// Downloads page: live list with progress and actions.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');

  function act(id, action) {
    return LB.request('downloadAction', { id: id, action: action }).catch(function (e) { Page.toast(e.message); throw e; });
  }

  // "5 Nov 2026, 17:20"; the finish only as a time on the same day.
  const full = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  const timeOnly = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
  function when(d) {
    const parts = [full.format(new Date(d.started))];
    if (d.finished) {
      const a = new Date(d.started), b = new Date(d.finished);
      const sameDay = a.toDateString() === b.toDateString();
      const label = d.state === 'completed' ? 'finished ' : d.state === 'cancelled' ? 'cancelled ' : 'stopped ';
      parts.push(label + (sameDay ? timeOnly.format(b) : full.format(b)));
    }
    return parts.join(' · ');
  }

  // 0.7.0 (R10, R11): rows are kept and updated in place (keyed by id) so the bars move
  // smoothly between updates (ui/shared/dlview.js) and hover and focus stay put.
  let items = [];
  const rows = new Map();   // id -> { row, sig }
  function sig(d) { return [d.state, d.fileName, d.path, d.partProgress && d.partProgress.length > 1 ? 'parts' : 'one', d.reason || '', d.pageUrl || ''].join('|'); }
  function statusText(d) {
    let s = DlView.status(d);
    if (d.connections && d.state === 'inProgress') s += ' · ' + d.connections + ' connections';
    return s;
  }
  function buildRow(d) {
    const running = d.state === 'inProgress' || d.state === 'paused';
    const failed = d.state === 'interrupted' || d.state === 'cancelled';
    const links = [];
    if (d.state === 'completed') {
      links.push(el('button', { text: 'Open file', onclick: function () { act(d.id, 'open'); } }));
      links.push(el('button', { text: 'Show in folder', onclick: function () { act(d.id, 'showInFolder'); } }));
    }
    if (d.state === 'inProgress') links.push(el('button', { text: 'Pause', onclick: function () { act(d.id, 'pause'); } }));
    if (d.state === 'paused') links.push(el('button', { text: 'Resume', onclick: function () { act(d.id, 'resume'); } }));
    if (running) links.push(el('button', { text: 'Cancel', onclick: function () { act(d.id, 'cancel'); } }));
    else {
      if (failed) links.push(el('button', { text: 'Retry', onclick: function () { act(d.id, 'retry'); } }));
      links.push(el('button', { text: 'Remove from list', onclick: function () { act(d.id, 'remove'); } }));
    }
    const row = el('div', { class: 'row dl state-' + d.state + (failed ? ' failed' : ''), 'data-id': d.id, tabindex: '0' }, [
      DlView.icon(d),
      el('div', { class: 'info' }, [
        el('div', { class: 'name', title: d.path, text: d.fileName }),
        el('div', { class: 'sub', text: statusText(d) }),
        running ? DlView.bar(d) : null,
        el('div', { class: 'url', title: d.url, text: d.url }),
        el('div', { class: 'when', text: when(d) }),
        el('div', { class: 'links' }, links),
      ]),
    ]);
    row.addEventListener('contextmenu', function (e) { e.preventDefault(); showMenu(items.find(function (x) { return x.id === d.id; }) || d, e.clientX, e.clientY); });
    row.addEventListener('keydown', function (e) {
      if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        const r = row.getBoundingClientRect();
        showMenu(items.find(function (x) { return x.id === d.id; }) || d, r.left + 40, r.top + 24);
      }
    });
    return row;
  }
  function render(list_) {
    items = list_;
    const empty = list.querySelector('.empty');
    if (!items.length) {
      rows.forEach(function (r, id) { r.row.remove(); DlView.forget(String(id)); });
      rows.clear();
      if (!empty) {
        list.appendChild(el('div', { class: 'empty' }, [
          el('span', { class: 'empty-glyph', 'aria-hidden': 'true' }),
          el('strong', { text: 'No downloads yet' }),
          el('span', { text: 'Files you download will appear here.' }),
        ]));
      }
      return;
    }
    if (empty) empty.remove();
    const seen = new Set();
    let prev = null;
    items.forEach(function (d) {
      seen.add(d.id);
      let r = rows.get(d.id);
      const k = sig(d);
      if (!r || r.sig !== k) {
        const row = buildRow(d);
        const focused = r && r.row.contains(document.activeElement);
        if (r) r.row.replaceWith(row);
        if (focused) row.focus();
        r = { row: row, sig: k };
        rows.set(d.id, r);
      } else {
        r.row.querySelector('.sub').textContent = statusText(d);
        r.row.querySelector('.when').textContent = when(d);
        const bar = r.row.querySelector('.dl-bar, .dl-segments');
        if (bar) DlView.update(bar, d);
      }
      const at = prev ? prev.nextSibling : list.firstChild;
      if (at !== r.row) list.insertBefore(r.row, at);
      prev = r.row;
    });
    rows.forEach(function (r, id) {
      if (!seen.has(id)) { r.row.remove(); rows.delete(id); DlView.forget(String(id)); }
    });
  }

  // ---------- context menu ----------
  let menu = null;
  function closeMenu() { if (menu) { menu.remove(); menu = null; } }
  function showMenu(d, x, y) {
    closeMenu();
    const running = d.state === 'inProgress' || d.state === 'paused';
    const done = d.state === 'completed';
    const entry = function (label, action, enabled, then) {
      return el('button', {
        class: 'menu-item', role: 'menuitem', text: label, disabled: enabled ? null : '',
        onclick: function () {
          closeMenu();
          if (then) then(); else act(d.id, action).then(function () { if (action === 'copyLink' || action === 'copyPageAddress') Page.toast('Copied'); }, function () {});
        },
      });
    };
    const items_ = running ? [
      d.state === 'paused' ? entry('Resume', 'resume', true) : entry('Pause', 'pause', true),
      entry('Cancel', 'cancel', true),
      el('div', { class: 'menu-sep' }),
      entry('Copy download link', 'copyLink', true),
      entry('Copy page address', 'copyPageAddress', !!d.pageUrl),
    ] : [
      entry('Open file', 'open', done),
      entry('Show in folder', 'showInFolder', done),
      el('div', { class: 'menu-sep' }),
      entry('Copy download link', 'copyLink', true),
      entry('Copy page address', 'copyPageAddress', !!d.pageUrl),
      el('div', { class: 'menu-sep' }),
      entry(done ? 'Download again' : 'Retry', 'retry', !running),
      entry('Remove from list', 'remove', !running),
      entry('Delete file…', 'deleteFile', done, function () {
        Page.confirm('Delete "' + d.fileName + '" from your computer?', { ok: 'Delete', danger: true }).then(function (yes) {
          if (yes) act(d.id, 'deleteFile').catch(function () {});
        });
      }),
    ];
    menu = el('div', { class: 'ctx-menu', role: 'menu' }, items_);
    document.body.appendChild(menu);
    const r = menu.getBoundingClientRect();
    menu.style.left = Math.max(4, Math.min(x, window.innerWidth - r.width - 4)) + 'px';
    menu.style.top = Math.max(4, Math.min(y, window.innerHeight - r.height - 4)) + 'px';
    const first = menu.querySelector('.menu-item:not([disabled])');
    if (first) first.focus();
  }
  document.addEventListener('mousedown', function (e) { if (menu && !menu.contains(e.target)) closeMenu(); });
  document.addEventListener('keydown', function (e) {
    if (!menu) return;
    if (e.key === 'Escape') { closeMenu(); return; }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const all = Array.prototype.slice.call(menu.querySelectorAll('.menu-item:not([disabled])'));
      const i = all.indexOf(document.activeElement);
      const next = all[(i + (e.key === 'ArrowDown' ? 1 : all.length - 1)) % all.length];
      if (next) next.focus();
    }
  });
  window.addEventListener('blur', closeMenu);
  window.addEventListener('scroll', closeMenu, true);

  LB.on('downloads', function (m) { render(m.items); });
  document.getElementById('clear').addEventListener('click', function () { LB.request('clearDownloads').then(load); });

  function load() { LB.request('getDownloads').then(render); }
  LB.post('ready');
  load();
})();
