// Downloads page. 0.15.0: the list comes from the host in pages (`queryDownloads`: newest first,
// by search text and filter) and is drawn as a windowed list: every row has the same height and
// only the rows near the viewport are in the DOM, so the page costs the same with 20 or 20,000
// downloads. Groups (Today / Yesterday / Earlier) come from the host's counts since the local
// midnights. The host pushes the running downloads that moved (`downloads`) and a signal when the
// list changed (`downloadsChanged`): the page then fetches the rows it shows again.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const search = document.getElementById('q');
  const summary = document.getElementById('summary');
  const perf = document.getElementById('perf');

  // Row pitch (a 66 px card and its gap), group header height, rows per request, rows drawn
  // beyond the viewport on each side.
  const ROW_H = 72;
  const HEAD_H = 40;
  const PAGE = 100;
  const OVERSCAN = 6;
  // Pages kept around the one in view.
  const KEEP_PAGES = 8;

  const view = {
    text: '',
    filter: 'all',
    total: 0,          // matching downloads
    groups: [],        // [{ start, label }] of the non-empty groups
    counts: null,
    gen: 0,            // bumped by every refresh: older replies are dropped
    loaded: false,
  };
  let pages = new Map();     // page index -> items
  let stale = null;          // the pages before a refresh, shown until it answers
  const loading = new Set(); // page indexes asked for (this generation)
  const nodes = new Map();   // key (download id, or 'p' + index for a placeholder) -> { node, sig }

  function act(id, action) {
    return LB.request('downloadAction', { id: id, action: action }).catch(function (e) { Page.toast(e.message); throw e; });
  }

  // ---------- times and groups ----------
  const full = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });
  const timeOnly = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });
  const dateOnly = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
  function midnights() {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);
    return [today.getTime(), yesterday.getTime()];
  }
  // "5 Nov 2026, 17:20 · finished 17:21": the tooltip of a row's time.
  function whenFull(d) {
    const parts = [full.format(new Date(d.started))];
    if (d.finished) {
      const a = new Date(d.started), b = new Date(d.finished);
      const sameDay = a.toDateString() === b.toDateString();
      const label = d.state === 'completed' ? 'finished ' : d.state === 'cancelled' ? 'cancelled ' : 'stopped ';
      parts.push(label + (sameDay ? timeOnly.format(b) : full.format(b)));
    }
    return parts.join(' · ');
  }
  function whenShort(d) {
    return d.started >= midnights()[1] ? timeOnly.format(new Date(d.started)) : dateOnly.format(new Date(d.started));
  }
  function setGroups(since) {
    const today = Math.min(since[0] || 0, view.total);
    const upToYesterday = Math.min(Math.max(since[1] || 0, today), view.total);
    view.groups = [
      { start: 0, end: today, label: 'Today' },
      { start: today, end: upToYesterday, label: 'Yesterday' },
      { start: upToYesterday, end: view.total, label: 'Earlier' },
    ].filter(function (g) { return g.end > g.start; });
  }

  // ---------- geometry ----------
  function headersUpTo(i) {
    let n = 0;
    view.groups.forEach(function (g) { if (g.start <= i) n++; });
    return n;
  }
  function rowY(i) { return i * ROW_H + HEAD_H * headersUpTo(i); }
  function headY(k) { return view.groups[k].start * ROW_H + HEAD_H * k; }
  function height() { return view.total * ROW_H + HEAD_H * view.groups.length; }
  // The last row starting at or above `y` (list coordinates).
  function indexAt(y) {
    let lo = 0, hi = Math.max(0, view.total - 1);
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (rowY(mid) <= y) lo = mid; else hi = mid - 1;
    }
    return lo;
  }
  function range() {
    const top = -list.getBoundingClientRect().top;
    const first = Math.max(0, indexAt(Math.max(0, top)) - OVERSCAN);
    const last = Math.min(view.total - 1, indexAt(top + window.innerHeight) + OVERSCAN);
    return [first, last];
  }

  // ---------- data ----------
  function itemAt(i) {
    const p = Math.floor(i / PAGE);
    const page = pages.get(p) || (stale && stale.get(p));
    return page ? page[i - p * PAGE] || null : null;
  }
  function query(offset, counts) {
    const args = { text: view.text, filter: view.filter, offset: offset, limit: PAGE };
    if (counts) { args.counts = true; args.since = midnights(); }
    return LB.request('queryDownloads', args).then(function (r) { return r || { offset: offset, items: [] }; });
  }
  function fetchPage(p) {
    if (pages.has(p) || loading.has(p)) return;
    loading.add(p);
    const gen = view.gen;
    const t0 = performance.now();
    query(p * PAGE, false).then(function (r) {
      if (gen !== view.gen) return;
      loading.delete(p);
      pages.set(p, r.items || []);
      stat.lastPage = performance.now() - t0;
      schedule();
    }, function () { loading.delete(p); });
  }
  // The list changed (or the search or filter): counts and the rows in view again; until the
  // answer the old rows stay (no flash of placeholders).
  function refresh(toTop) {
    view.gen++;
    const gen = view.gen;
    loading.clear();
    stale = pages;
    pages = new Map();
    if (toTop) window.scrollTo(0, 0);
    const p = toTop || !view.loaded ? 0 : Math.floor(range()[0] / PAGE);
    loading.add(p);
    const t0 = performance.now();
    query(p * PAGE, true).then(function (r) {
      if (gen !== view.gen) return;
      loading.delete(p);
      const c = r.counts || { all: 0, active: 0, done: 0, failed: 0, filtered: 0, since: [0, 0] };
      view.counts = c;
      view.total = c.filtered;
      setGroups(c.since || []);
      pages.set(p, r.items || []);
      stale = null;
      stat.lastPage = performance.now() - t0;
      showCounts();
      if (!view.loaded) {
        view.loaded = true;
        stat.firstRows = Math.round(performance.now());
        document.body.dataset.firstRows = String(stat.firstRows);
      }
      render();
    }, function () { loading.delete(p); });
  }
  const refreshSoon = Page.debounce(function () { refresh(false); }, 120);

  function showCounts() {
    const c = view.counts;
    const fmt = new Intl.NumberFormat();
    document.querySelectorAll('#chips .chip').forEach(function (b) {
      b.querySelector('.n').textContent = c ? fmt.format(c[b.dataset.filter] || 0) : '';
      b.setAttribute('aria-pressed', String(b.dataset.filter === view.filter));
    });
    const n = view.total;
    summary.textContent = view.text ? fmt.format(n) + (n === 1 ? ' match' : ' matches') : '';
    document.getElementById('clear').disabled = !c || c.done + c.failed === 0;
  }

  // ---------- rows ----------
  function sig(d) { return [d.state, d.fileName, d.path, d.partProgress && d.partProgress.length > 1 ? 'parts' : 'one', d.reason || '', d.pageUrl || ''].join('|'); }
  function statusText(d) {
    // Finished: the pill says how it ended; the line has the size, or why it failed.
    if (d.state === 'completed') return DlView.size(d.total || d.received);
    if (d.state === 'interrupted') return d.reason || '';
    if (d.state === 'cancelled') return '';
    let s = DlView.status(d);
    if (d.connections && d.state === 'inProgress') s += ' · ' + d.connections + ' connections';
    return s;
  }
  const ICON = {
    open: 'M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5',
    folder: 'M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z',
    pause: 'M9 6v12M15 6v12',
    resume: 'M8 5l11 7-11 7z',
    cancel: 'M6 6l12 12M18 6L6 18',
    retry: 'M4 12a8 8 0 0 1 14-5.3L20 9M20 4v5h-5M20 12a8 8 0 0 1-14 5.3L4 15M4 20v-5h5',
    link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
    remove: 'M4 7h16M4 12h10M4 17h7M15 15l5 5M20 15l-5 5',
    more: 'M5 12h.01M12 12h.01M19 12h.01',
  };
  function actButton(icon, label, onclick) {
    return el('button', { type: 'button', class: 'act act-' + icon, title: label, 'aria-label': label,
      onclick: function (e) { e.stopPropagation(); onclick(e); } }, [Page.svg(ICON[icon])]);
  }
  function current(d) { return byId(d.id) || d; }
  function buildRow(d) {
    const running = d.state === 'inProgress' || d.state === 'paused';
    const done = d.state === 'completed';
    const failed = d.state === 'interrupted' || d.state === 'cancelled';
    const site = DlView.site(d);
    const acts = [];
    if (done) {
      acts.push(actButton('open', 'Open file', function () { act(d.id, 'open'); }));
      acts.push(actButton('folder', 'Show in folder', function () { act(d.id, 'showInFolder'); }));
    }
    if (d.state === 'inProgress') acts.push(actButton('pause', 'Pause', function () { act(d.id, 'pause'); }));
    if (d.state === 'paused') acts.push(actButton('resume', 'Resume', function () { act(d.id, 'resume'); }));
    if (running) acts.push(actButton('cancel', 'Cancel', function () { act(d.id, 'cancel'); }));
    if (failed) acts.push(actButton('retry', 'Retry', function () { act(d.id, 'retry'); }));
    acts.push(actButton('link', 'Copy download link', function () { act(d.id, 'copyLink').then(function () { Page.toast('Copied'); }, function () {}); }));
    if (!running) acts.push(actButton('remove', 'Remove from list', function () { act(d.id, 'remove'); }));
    acts.push(actButton('more', 'More actions', function (e) {
      const r = e.currentTarget.getBoundingClientRect();
      showMenu(current(d), r.left, r.bottom + 4);
    }));
    const kind = DlView.kind(d.fileName);
    const name = done
      ? el('button', { type: 'button', class: 'name', title: 'Open ' + d.path, text: d.fileName, onclick: function () { act(d.id, 'open'); } })
      : el('span', { class: 'name', title: d.path, text: d.fileName });
    const row = el('div', { class: 'dl state-' + d.state + (failed ? ' failed' : ''), role: 'listitem', 'data-id': String(d.id), tabindex: '0' }, [
      DlView.icon(d),
      el('div', { class: 'info' }, [
        el('div', { class: 'line' }, [name, failed ? el('span', { class: 'pill', text: d.state === 'cancelled' ? 'Cancelled' : 'Failed' }) : null]),
        el('div', { class: 'meta' }, [
          site ? el('span', { class: 'site', title: d.url, text: site }) : null,
          el('span', { class: 'kind', text: DlView.kindLabel(kind) }),
          el('span', { class: 'sub', text: statusText(d) }),
        ]),
        running ? DlView.bar(d) : null,
      ]),
      el('span', { class: 'when', title: whenFull(d), text: whenShort(d) }),
      el('div', { class: 'acts' }, acts),
    ]);
    row.addEventListener('contextmenu', function (e) { e.preventDefault(); showMenu(current(d), e.clientX, e.clientY); });
    row.addEventListener('keydown', function (e) {
      if (e.target !== row) return;
      if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
        e.preventDefault();
        const r = row.getBoundingClientRect();
        showMenu(current(d), r.left + 40, r.top + 24);
      } else if (e.key === 'Enter' && done) {
        act(d.id, 'open');
      } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        focusIndex(Number(row.dataset.index) + (e.key === 'ArrowDown' ? 1 : -1));
      }
    });
    return row;
  }
  function updateRow(node, d) {
    node.querySelector('.sub').textContent = statusText(d);
    const bar = node.querySelector('.dl-bar, .dl-segments');
    if (bar) DlView.update(bar, d);
  }
  function placeholder() {
    return el('div', { class: 'dl placeholder', 'aria-hidden': 'true' }, [
      el('span', { class: 'ph ph-icon' }), el('div', { class: 'info' }, [el('span', { class: 'ph ph-name' }), el('span', { class: 'ph ph-meta' })]),
    ]);
  }
  function byId(id) {
    let found = null;
    pages.forEach(function (items) { if (!found) found = items.find(function (x) { return x.id === id; }) || null; });
    return found;
  }

  // ---------- drawing ----------
  let headNodes = [];
  let headSig = '';
  let emptyNode = null;
  function render() {
    frame = 0;
    list.style.height = height() + 'px';
    // Headers (at most three), redrawn when the groups change.
    const heads = JSON.stringify(view.groups);
    if (heads !== headSig) {
      headSig = heads;
      headNodes.forEach(function (n) { n.remove(); });
      headNodes = view.groups.map(function (g, k) {
        const h = el('div', { class: 'dl-group', role: 'presentation', text: g.label });
        h.style.transform = 'translateY(' + headY(k) + 'px)';
        list.appendChild(h);
        return h;
      });
    }
    if (emptyNode) { emptyNode.remove(); emptyNode = null; }
    if (!view.total) {
      nodes.forEach(function (r, key) { r.node.remove(); if (typeof key === 'number') DlView.forget(String(key)); });
      nodes.clear();
      if (view.loaded || view.counts) showEmpty();
      return;
    }
    const [first, last] = range();
    const used = new Set();
    const focused = document.activeElement;
    let missing = false;
    for (let i = first; i <= last; i++) {
      const d = itemAt(i);
      const key = d ? d.id : 'p' + i;
      if (!d) missing = true;
      let r = nodes.get(key);
      if (d) {
        const k = sig(d);
        if (!r || r.sig !== k) {
          const node = buildRow(d);
          const hadFocus = r && r.node.contains(focused);
          if (r) r.node.replaceWith(node); else list.appendChild(node);
          r = { node: node, sig: k };
          nodes.set(key, r);
          if (hadFocus) node.focus({ preventScroll: true });
        } else {
          updateRow(r.node, d);
        }
      } else if (!r) {
        r = { node: placeholder(), sig: '' };
        list.appendChild(r.node);
        nodes.set(key, r);
      }
      r.node.style.transform = 'translateY(' + rowY(i) + 'px)';
      r.node.dataset.index = String(i);
      used.add(key);
    }
    nodes.forEach(function (r, key) {
      if (used.has(key)) return;
      r.node.remove();
      nodes.delete(key);
      if (typeof key === 'number') DlView.forget(String(key));
    });
    for (let p = Math.floor(first / PAGE); p <= Math.floor(last / PAGE); p++) fetchPage(p);
    stat.placeholders = missing;
    // Pages far from the view go.
    const mid = Math.floor(first / PAGE);
    pages.forEach(function (_, p) { if (Math.abs(p - mid) > KEEP_PAGES) pages.delete(p); });
    stat.rows = nodes.size;
    if (!perf.hidden) showPerf();
  }
  function showEmpty() {
    const filtered = view.text || view.filter !== 'all';
    emptyNode = el('div', { class: 'empty' }, [
      el('span', { class: 'empty-glyph', 'aria-hidden': 'true' }),
      el('strong', { text: filtered ? 'No matching downloads' : 'No downloads yet' }),
      el('span', { text: filtered ? 'Try other words or another filter.' : 'Files you download will appear here.' }),
    ]);
    list.appendChild(emptyNode);
    list.style.height = '';
  }
  function focusIndex(i) {
    if (i < 0 || i >= view.total) return;
    const y = rowY(i) + list.getBoundingClientRect().top;
    if (y < 0 || y + ROW_H > window.innerHeight) window.scrollBy(0, y < 0 ? y - 8 : y + ROW_H - window.innerHeight + 8);
    render();
    const n = list.querySelector('.dl[data-index="' + i + '"]:not(.placeholder)');
    if (n) n.focus({ preventScroll: true });
  }
  let frame = 0;
  function schedule() { if (!frame) frame = requestAnimationFrame(render); }
  window.addEventListener('scroll', function () { schedule(); stat.scrolled(); }, { passive: true });
  window.addEventListener('resize', schedule);

  // ---------- live updates ----------
  LB.on('downloads', function (m) {
    (m.items || []).forEach(function (d) {
      pages.forEach(function (items) {
        const i = items.findIndex(function (x) { return x.id === d.id; });
        if (i >= 0) items[i] = d;
      });
      const r = nodes.get(d.id);
      if (!r) return;
      if (r.sig === sig(d)) updateRow(r.node, d); else schedule();
    });
  });
  LB.on('downloadsChanged', refreshSoon);

  // ---------- search, filters, clear ----------
  search.addEventListener('input', Page.debounce(function () {
    if (search.value.trim() === view.text) return;
    view.text = search.value.trim();
    refresh(true);
  }, 200));
  document.getElementById('chips').addEventListener('click', function (e) {
    const b = e.target.closest('.chip');
    if (!b || b.dataset.filter === view.filter) return;
    view.filter = b.dataset.filter;
    showCounts();
    refresh(true);
  });
  document.getElementById('clear').addEventListener('click', function () {
    LB.request('clearDownloads').then(function () { refresh(false); }, function (e) { Page.toast(e.message); });
  });

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

  // ---------- krunk://downloads#perf: numbers for the VM check, without devtools ----------
  const stat = {
    firstRows: 0, lastPage: 0, rows: 0, frames: [], scrolling: 0,
    scrolled: function () {
      if (perf.hidden) return;
      stat.scrolling = performance.now();
      if (!stat.loop) stat.loop = requestAnimationFrame(stat.tick);
    },
    tick: function (t) {
      stat.frames.push(t);
      while (stat.frames.length && stat.frames[0] < t - 1000) stat.frames.shift();
      showPerf();
      stat.loop = t - stat.scrolling < 1000 ? requestAnimationFrame(stat.tick) : 0;
    },
  };
  function showPerf() {
    const f = stat.frames;
    let worst = 0;
    for (let i = 1; i < f.length; i++) worst = Math.max(worst, f[i] - f[i - 1]);
    perf.textContent = 'first rows ' + stat.firstRows + ' ms · last page ' + Math.round(stat.lastPage) + ' ms · ' +
      stat.rows + ' rows in DOM · ' + view.total + ' listed' +
      (f.length > 1 ? ' · scroll ' + (f.length - 1) + ' fps, worst frame ' + Math.round(worst) + ' ms' : '');
  }
  if (location.hash === '#perf') perf.hidden = false;

  // The groups move at midnight.
  (function atMidnight() {
    const next = midnights()[0] + 86400000 + 1000;
    setTimeout(function () { refresh(false); atMidnight(); }, Math.max(1000, next - Date.now()));
  })();

  LB.post('ready');
  refresh(true);
})();
