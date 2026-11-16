// History page: search, delete single visits or a time range, and clear
// browsing data. 0.23.0: per device — "This device" by default, the other synced
// devices by name, unknown (synced before devices were recorded) and all.
(function () {
  'use strict';
  const el = LB.el;
  const list = document.getElementById('list');
  const more = document.getElementById('more');
  const chips = document.getElementById('devices');
  const PAGE = 100;
  let entries = [];
  let query = '';
  let device = 'this';
  let names = {};

  function deviceName(key) {
    if (key === 'this') return 'This device';
    if (key === 'unknown') return 'Unknown device';
    if (key === 'all') return 'All devices';
    return names[key] || ('Device ' + String(key).slice(0, 6));
  }

  // A row's device key from the visit's `device` ("" this device, id, null unknown).
  function rowDevice(e) {
    if (e.device === '') return 'this';
    if (e.device == null) return 'unknown';
    return e.device;
  }

  function renderChips(devs) {
    names = {};
    const counts = {};
    devs.forEach(function (d) { counts[d.key] = d.visits; if (d.name) names[d.key] = d.name; });
    const keys = ['this'].concat(devs.map(function (d) { return d.key; }).filter(function (k) { return k !== 'this'; }), ['all']);
    if (keys.indexOf(device) < 0) device = 'this';
    chips.textContent = '';
    keys.forEach(function (k) {
      chips.appendChild(el('button', { type: 'button', class: 'chip', 'data-device': k, 'aria-pressed': String(k === device) }, [
        deviceName(k) + ' ',
        el('span', { class: 'n', text: k === 'all' ? '' : String(counts[k] || 0) }),
      ]));
    });
  }

  function loadDevices() {
    return LB.request('historyDevices', {}).then(renderChips, function () { renderChips([]); });
  }

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
      list.appendChild(el('div', { class: 'card' }, [el('div', { class: 'empty', text: query ? 'No matches.' : (device === 'all' ? 'No history yet.' : 'No history from ' + (device === 'this' ? 'this device' : deviceName(device)) + '.') })]));
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
          el('span', { class: 'sub', text: device === 'this' || device === rowDevice(e) ? Page.host(e.url) : Page.host(e.url) + ' · ' + deviceName(rowDevice(e)) }),
        ]),
        el('span', { class: 'actions' }, [
          Page.iconButton('close', 'Remove from history', function () { remove([e.id]); }),
        ]),
      ]));
    });
  }

  function load(append) {
    const before = append && entries.length ? entries[entries.length - 1].ts : null;
    LB.request('queryHistory', { text: query, before: before, limit: PAGE, device: device }).then(function (rows) {
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
  chips.addEventListener('click', function (e) {
    const b = e.target.closest('.chip');
    if (!b) return;
    device = b.getAttribute('data-device');
    chips.querySelectorAll('.chip').forEach(function (c) { c.setAttribute('aria-pressed', String(c === b)); });
    load(false);
  });

  document.getElementById('delete-range').addEventListener('click', function () {
    const sel = document.getElementById('range');
    const v = sel.value;
    const now = Date.now();
    const from = v === 'all' ? 0 : now - Number(v);
    const span = sel.options[sel.selectedIndex].text.toLowerCase();
    const text = device === 'all'
      ? 'Delete the history of all devices from ' + span + '? It is deleted on your other synced devices too.'
      : device === 'this'
        ? 'Delete this device\'s history from ' + span + '? It is deleted on your other synced devices too.'
        : 'Hide the history of ' + deviceName(device) + ' from ' + span + ' on this device? It stays on ' + deviceName(device) + '.';
    Page.confirm(text, { ok: 'Delete', danger: true }).then(function (yes) {
      if (!yes) return;
      LB.request('deleteHistoryRange', { from: from, to: now + 1, device: device === 'all' ? '' : device }).then(function () {
        Page.toast('History deleted');
        loadDevices().then(function () { load(false); });
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
      loadDevices().then(function () { load(false); });
    }, function (err) { Page.toast(err.message); });
  });

  LB.post('ready');
  loadDevices().then(function () { load(false); });
})();
