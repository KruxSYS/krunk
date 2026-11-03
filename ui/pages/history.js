// History page: search, delete single visits or a time range, and clear
// browsing data.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const more = document.getElementById('more');
  const PAGE = 100;
  let entries = [];
  let query = '';

  function dayLabel(ts) {
    const d = new Date(ts);
    const today = new Date();
    const start = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
    if (ts >= start) return 'Today';
    if (ts >= start - 86400000) return 'Yesterday';
    return d.toLocaleDateString(undefined, { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' });
  }

  function render() {
    list.textContent = '';
    if (!entries.length) {
      list.appendChild(el('div', { class: 'card' }, [el('div', { class: 'empty', text: query ? 'No matches.' : 'No history yet.' })]));
      return;
    }
    let card = null;
    let label = null;
    entries.forEach(function (e) {
      const l = dayLabel(e.ts);
      if (l !== label) {
        label = l;
        list.appendChild(el('h2', { class: 'group-title', text: l }));
        card = el('div', { class: 'card' });
        list.appendChild(card);
      }
      const time = new Date(e.ts).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
      card.appendChild(el('div', { class: 'row', 'data-id': e.id }, [
        el('span', { class: 'time', text: time }),
        Page.favicon(e.url),
        el('span', { class: 'main' }, [
          el('a', { href: e.url, title: e.url, text: e.title || e.url }),
          el('span', { class: 'sub', text: Page.host(e.url) }),
        ]),
        el('span', { class: 'actions' }, [
          Page.iconButton('close', 'Remove from history', function () { remove([e.id]); }),
        ]),
      ]));
    });
  }

  function load(append) {
    const before = append && entries.length ? entries[entries.length - 1].ts : null;
    LB.request('queryHistory', { text: query, before: before, limit: PAGE }).then(function (rows) {
      entries = append ? entries.concat(rows) : rows;
      more.hidden = rows.length < PAGE;
      render();
    });
  }

  function remove(ids) {
    LB.request('deleteHistory', { ids: ids }).then(function () {
      entries = entries.filter(function (e) { return ids.indexOf(e.id) < 0; });
      render();
      Page.toast('Removed from history');
    });
  }

  document.getElementById('q').addEventListener('input', Page.debounce(function (e) {
    query = e.target.value.trim();
    load(false);
  }, 150));
  more.addEventListener('click', function () { load(true); });

  document.getElementById('delete-range').addEventListener('click', function () {
    const sel = document.getElementById('range');
    const v = sel.value;
    const now = Date.now();
    const from = v === 'all' ? 0 : now - Number(v);
    Page.confirm('Delete history from ' + sel.options[sel.selectedIndex].text.toLowerCase() + '?', { ok: 'Delete', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('deleteHistoryRange', { from: from, to: now + 1 }).then(function () {
        Page.toast('History deleted');
        load(false);
      });
    });
  });

  const dialog = document.getElementById('clear-dialog');
  document.getElementById('clear').addEventListener('click', function () { dialog.showModal(); });
  dialog.addEventListener('close', function () {
    if (dialog.returnValue !== 'ok') return;
    const v = document.getElementById('clear-range').value;
    const data = {
      history: document.getElementById('c-history').checked,
      cookies: document.getElementById('c-cookies').checked,
      siteData: document.getElementById('c-site').checked,
      cache: document.getElementById('c-cache').checked,
      downloads: document.getElementById('c-downloads').checked,
      since: v === 'all' ? null : Date.now() - Number(v),
    };
    LB.request('clearBrowsingData', { data: data }).then(function () {
      Page.toast('Browsing data cleared');
      load(false);
    }, function (err) { Page.toast(err.message); });
  });

  LB.post('ready');
  load(false);
})();
