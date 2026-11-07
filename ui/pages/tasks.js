// krunk://tasks: the engine's tasks (browser, GPU, renderers with their tabs, utilities,
// workers) with memory, CPU, network and PID, refreshed every second; sortable columns and
// "End process" for the selected killable task.
(function () {
  'use strict';
  const el = LB.el;
  const rowsEl = document.getElementById('rows');
  const endBtn = document.getElementById('end');
  const summary = document.getElementById('summary');
  let rows = [];
  let selected = null;
  let sortKey = 'title', sortDesc = false;
  const KIND = { browser: 'Browser', gpu: 'GPU', renderer: 'Tab', utility: 'Utility', extension: 'Extension', worker: 'Worker', other: 'Other' };

  function size(n) {
    if (!n) return '–';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return (i ? n.toFixed(1) : Math.round(n)) + ' ' + u[i];
  }
  function rate(n) { return n ? size(n) + '/s' : '–'; }

  function sorted() {
    const out = rows.slice();
    out.sort(function (a, b) {
      let x = a[sortKey], y = b[sortKey];
      if (sortKey === 'title') { x = (a.tabs.length ? a.tabs[0].title : a.title).toLowerCase(); y = (b.tabs.length ? b.tabs[0].title : b.title).toLowerCase(); }
      if (x === null || x === undefined) x = -1;
      if (y === null || y === undefined) y = -1;
      const c = x < y ? -1 : x > y ? 1 : 0;
      return sortDesc ? -c : c;
    });
    return out;
  }

  function render() {
    const list = sorted();
    rowsEl.textContent = '';
    list.forEach(function (r) {
      const tabs = r.tabs.length ? r.tabs.map((t) => t.title).join(', ') : '';
      const tr = el('tr', { class: r.id === selected ? 'selected' : '', 'data-id': String(r.id), 'data-kind': r.kind }, [
        el('td', { class: 'title', title: tabs || r.title }, [
          el('span', { text: r.tabs.length ? r.tabs.map((t) => t.title).join(', ') : r.title }),
          r.tabs.length > 1 ? el('span', { class: 'tabs', text: ' (' + r.tabs.length + ' tabs)' }) : null,
        ]),
        el('td', { class: 'kind', text: KIND[r.kind] || r.kind }),
        el('td', { class: 'num', text: size(r.memory) }),
        el('td', { class: 'num', text: size(r.workingSet) }),
        el('td', { class: 'num', text: size(r.private) }),
        el('td', { class: 'num', text: r.cpu.toFixed(1) + ' %' }),
        el('td', { class: 'num', text: rate(r.network) }),
        el('td', { class: 'num', text: r.pid === null || r.pid === undefined ? '–' : String(r.pid) }),
      ]);
      tr.addEventListener('click', function () { selected = r.id; render(); });
      rowsEl.appendChild(tr);
    });
    const sel = rows.find((r) => r.id === selected);
    endBtn.disabled = !sel || !sel.killable;
    endBtn.textContent = sel && sel.kind === 'browser' ? 'End process (not the browser)' : 'End process';
    const renderers = rows.filter((r) => r.kind === 'renderer').length;
    summary.textContent = rows.length + ' tasks, ' + renderers + ' renderer' + (renderers === 1 ? '' : 's');
  }

  document.querySelectorAll('th.sortable').forEach(function (th) {
    th.addEventListener('click', function () {
      const key = th.dataset.sort;
      if (sortKey === key) sortDesc = !sortDesc; else { sortKey = key; sortDesc = ['memory', 'workingSet', 'private', 'cpu', 'network'].includes(key); }
      document.querySelectorAll('th.sortable').forEach((h) => { h.classList.toggle('sorted', h === th); h.classList.toggle('desc', h === th && sortDesc); h.classList.toggle('asc', h === th && !sortDesc); });
      render();
    });
  });

  endBtn.addEventListener('click', function () {
    const sel = rows.find((r) => r.id === selected);
    if (!sel || !sel.killable) return;
    LB.request('killTask', { id: sel.id }).then(function () { Page.toast('Process ended'); refresh(); }, function (e) { Page.toast(e.message); });
  });

  function refresh() {
    return LB.request('getTasks').then(function (r) {
      rows = Array.isArray(r) ? r : [];
      if (selected !== null && !rows.some((x) => x.id === selected)) selected = null;
      render();
    }, function () {});
  }
  refresh();
  setInterval(refresh, 1000);
})();
