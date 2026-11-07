// Downloads page: live list with progress and actions.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');

  function size(n) {
    if (n === null || n === undefined) return '';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(1) : n) + ' ' + u[i];
  }

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

  function left(s) { return s >= 3600 ? Math.round(s / 3600) + ' h left' : s >= 60 ? Math.round(s / 60) + ' min left' : s + ' s left'; }
  // One bar segment per connection for a download in parts (each filled to its own progress).
  function bar(d) {
    if (d.partProgress && d.partProgress.length > 1) {
      const total = d.partProgress.reduce((a, p) => a + p.len, 0) || 1;
      // (Widths through the CSSOM: the pages' CSP has no inline styles.)
      return el('div', { class: 'segments', title: d.partProgress.length + ' connections' }, d.partProgress.map((p) => {
        const fill = el('span', { class: 'fill' });
        fill.style.width = Math.min(100, 100 * p.done / Math.max(1, p.len)) + '%';
        const seg = el('span', { class: 'seg' }, [fill]);
        seg.style.flexBasis = (100 * p.len / total) + '%';
        return seg;
      }));
    }
    return el('progress', d.total ? { max: d.total, value: d.received } : {});
  }

  let items = [];
  function render(list_) {
    items = list_;
    list.textContent = '';
    if (!items.length) {
      list.appendChild(el('div', { class: 'empty', text: 'Files you download will appear here.' }));
      return;
    }
    items.forEach(function (d) {
      const running = d.state === 'inProgress' || d.state === 'paused';
      const failed = d.state === 'interrupted' || d.state === 'cancelled';
      const ext = (d.fileName.split('.').pop() || '').slice(0, 4).toUpperCase();
      let status;
      if (d.state === 'completed') status = size(d.total || d.received);
      else if (d.state === 'cancelled') status = 'Cancelled';
      else if (d.state === 'interrupted') status = 'Failed' + (d.reason ? ' – ' + d.reason : '');
      else {
        status = size(d.received) + (d.total ? ' of ' + size(d.total) : '') + (d.state === 'paused' ? ' – paused' : '');
        // 0.6.0: the smoothed speed and the time left, and the connections of a download in parts.
        if (d.state === 'inProgress' && d.speed) status += ' · ' + size(d.speed) + '/s';
        if (d.state === 'inProgress' && d.eta !== null && d.eta !== undefined) status += ' · ' + left(d.eta);
        if (d.connections && d.state === 'inProgress') status += ' · ' + d.connections + ' connections';
      }
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
      const row = el('div', { class: 'row dl' + (failed ? ' failed' : ''), 'data-id': d.id, tabindex: '0' }, [
        el('span', { class: 'file-icon', text: ext || 'FILE' }),
        el('div', { class: 'info' }, [
          el('div', { class: 'name', title: d.path, text: d.fileName }),
          el('div', { class: 'sub', text: status }),
          el('div', { class: 'url', title: d.url, text: d.url }),
          el('div', { class: 'when', text: when(d) }),
          running ? bar(d) : null,
          el('div', { class: 'links' }, links),
        ]),
      ]);
      row.addEventListener('contextmenu', function (e) { e.preventDefault(); showMenu(d, e.clientX, e.clientY); });
      row.addEventListener('keydown', function (e) {
        if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
          e.preventDefault();
          const r = row.getBoundingClientRect();
          showMenu(d, r.left + 40, r.top + 24);
        }
      });
      list.appendChild(row);
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
