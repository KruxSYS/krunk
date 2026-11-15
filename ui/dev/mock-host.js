// Mock of the Rust host for testing the built-in UI in a normal browser
// (Playwright). NOT embedded in the exe: core/build.rs skips ui/dev.
//
// Provides window.__lbHost (postMessage/addEventListener, as installed by
// the browser's renderer bridge) and
// window.__host with:
//   sent         - every message the page posted, in order
//   emit(event)  - deliver a host event to the page
//   state        - a tiny simulated tab model for the shell
(function () {
  'use strict';
  const listeners = [];
  const host = {
    sent: [],
    state: { tabs: [], active: null, nextId: 100 },
    emit: function (event) {
      const e = { data: JSON.parse(JSON.stringify(event)) };
      setTimeout(function () { listeners.forEach(function (l) { l(e); }); }, 0);
    },
    tabsEvent: function () {
      host.emit({ event: 'tabs', tabs: host.state.tabs, active: host.state.active });
    },
    navEvent: function (extra) {
      const t = host.state.tabs.find(function (x) { return x.id === host.state.active; }) || {};
      host.emit({
        event: 'nav',
        state: Object.assign({
          tabId: t.id || 0, url: t.url || '', canGoBack: true, canGoForward: false, loading: !!t.loading,
          secure: (t.url || '').startsWith('https://'), bookmarked: false, blocked: 12, siteAllowed: false,
          zoom: 1, private: !!t.private,
        }, extra || {}),
      });
    },
  };

  // Simulated shell behaviour, just enough to exercise the UI.
  function simulate(msg) {
    const s = host.state;
    switch (msg.cmd) {
      case 'newTab': {
        const id = s.nextId++;
        s.tabs.push({ id: id, title: 'New tab', url: '', favicon: null, loading: false, private: !!msg.private, sleeping: false });
        s.active = id;
        host.tabsEvent(); host.navEvent();
        break;
      }
      case 'closeTab': {
        const i = s.tabs.findIndex(function (t) { return t.id === msg.id; });
        if (i < 0) break;
        s.tabs.splice(i, 1);
        if (s.active === msg.id && s.tabs.length) s.active = s.tabs[Math.min(i, s.tabs.length - 1)].id;
        host.tabsEvent(); host.navEvent();
        break;
      }
      case 'activateTab': s.active = msg.id; host.tabsEvent(); host.navEvent(); break;
      case 'moveTab': {
        const i = s.tabs.findIndex(function (t) { return t.id === msg.id; });
        const t = s.tabs.splice(i, 1)[0];
        s.tabs.splice(msg.index, 0, t);
        host.tabsEvent();
        break;
      }
      // 0.15.0: address-bar suggestions, when a test sets `host.suggest = { rows(text),
      // more(text), delayMs }`: the local rows at once, the search engine's later and only for
      // the newest text (as app/src/browser/omnibox.rs does).
      case 'addressSuggest': {
        const sg = host.suggest;
        if (!sg) break;
        const gen = sg.gen = (sg.gen || 0) + 1;
        const rows = sg.rows(msg.text);
        host.emit({ event: 'addressSuggestions', text: msg.text, entries: rows });
        const more = sg.more ? sg.more(msg.text) : [];
        if (more.length) {
          setTimeout(function () {
            if (sg.gen === gen) host.emit({ event: 'addressSearchSuggestions', text: msg.text, entries: rows.concat(more) });
          }, sg.delayMs || 150);
        }
        break;
      }
      case 'navigate': {
        const t = s.tabs.find(function (x) { return x.id === s.active; });
        if (t) { t.url = 'https://www.google.com/search?q=' + encodeURIComponent(msg.input); t.loading = true; }
        host.tabsEvent(); host.navEvent();
        break;
      }
    }
  }

  // Request handlers: host.handlers[cmd] = function (msg) { return data; }.
  // Requests with a reqId get a reply (ok + data, or the thrown error).
  host.handlers = {};

  // 0.15.0: the downloads list as the host keeps it (core/src/downloads.rs): `queryDownloads`
  // pages newest first by search text and filter, with counts; `clearDownloads` and the
  // `remove` action change it. `list` is the items, or how many realistic ones to make.
  // host.dl.queries counts the requests (the page must fetch pages, not everything).
  host.mockDownloads = function (list, now) {
    now = now || Date.now();
    if (typeof list === 'number') {
      const names = ['report.pdf', 'setup-x64.msi', 'photos.zip', 'talk.mp4', 'song.mp3', 'data.csv', 'ubuntu.iso', 'notes.txt', 'logo.png', 'app.js'];
      const sites = ['github.com', 'releases.example.org', 'cdn.example.net', 'arxiv.org', 'files.example.com', 'drive.example.io'];
      const n = list;
      list = [];
      for (let k = 0; k < n; k++) {
        const name = names[k % names.length].replace('.', '-' + (n - k) + '.');
        const site = sites[k % sites.length];
        // Three a day or so, newest first; every 13th failed, every 29th cancelled.
        const started = now - Math.floor(k * 8.3 * 3600000 / 3) - 60000;
        const state = k % 29 === 0 ? 'cancelled' : k % 13 === 0 ? 'interrupted' : 'completed';
        const size = 1000 + ((k * 7919) % 900) * 1048576 / 8;
        list.push({ id: n - k, url: 'https://' + site + '/dl/' + name, fileName: name, path: 'C:\\Users\\me\\Downloads\\' + name,
          state: state, received: state === 'completed' ? size : Math.floor(size / 3), total: size, started: started,
          finished: started + 30000, reason: state === 'interrupted' ? 'Network error' : null, pageUrl: 'https://' + site + '/releases', private: false });
      }
    }
    // `now`: the time the list was made at (tests add newer downloads after it, whatever the clock says).
    const dl = { items: list.slice(), queries: 0, now: now };
    host.dl = dl;
    const order = function (a, b) { return b.started - a.started || b.id - a.id; };
    const running = function (s) { return s === 'inProgress' || s === 'paused'; };
    const pass = { all: function () { return true; }, active: running, done: function (s) { return s === 'completed'; },
      failed: function (s) { return s === 'interrupted' || s === 'cancelled'; } };
    host.handlers.queryDownloads = function (m) {
      dl.queries++;
      const text = (m.text || '').toLowerCase();
      const hits = dl.items.filter(function (d) { return (d.fileName + '\n' + d.url).toLowerCase().indexOf(text) >= 0; }).sort(order);
      const shown = hits.filter(function (d) { return pass[m.filter || 'all'](d.state); });
      const out = { offset: m.offset || 0, items: shown.slice(m.offset || 0, (m.offset || 0) + Math.min(m.limit || 100, 500)) };
      if (m.counts) {
        const count = function (f) { return hits.filter(function (d) { return pass[f](d.state); }).length; };
        out.counts = { all: hits.length, active: count('active'), done: count('done'), failed: count('failed'), filtered: shown.length,
          since: (m.since || []).map(function (t) { return shown.filter(function (d) { return d.started >= t; }).length; }) };
      }
      return out;
    };
    host.handlers.clearDownloads = function () {
      dl.items = dl.items.filter(function (d) { return running(d.state); });
      return null;
    };
    host.handlers.downloadAction = function (m) {
      if (m.action === 'remove') dl.items = dl.items.filter(function (d) { return d.id !== m.id; });
      return null;
    };
    return dl;
  };
  function answer(msg) {
    if (msg.reqId === undefined) return;
    const h = host.handlers[msg.cmd];
    try {
      const data = h ? h(msg) : null;
      if (data && typeof data.then === 'function') {
        // Asynchronous handler (tests simulating a slow host).
        data.then((d) => host.emit({ event: 'reply', reqId: msg.reqId, ok: true, data: d === undefined ? null : d }),
          (e) => host.emit({ event: 'reply', reqId: msg.reqId, ok: false, error: String(e.message || e), data: e.data }));
        return;
      }
      host.emit({ event: 'reply', reqId: msg.reqId, ok: true, data: data === undefined ? null : data });
    } catch (e) {
      host.emit({ event: 'reply', reqId: msg.reqId, ok: false, error: String(e.message || e), data: e.data });
    }
  }

  window.__lbHost = {
    postMessage: function (msg) {
      host.sent.push(JSON.parse(JSON.stringify(msg)));
      if (host.autoSimulate) simulate(msg);
      answer(msg);
    },
    addEventListener: function (type, fn) { if (type === 'message') listeners.push(fn); },
    removeEventListener: function () {},
  };
  host.autoSimulate = true;
  window.__host = host;
})();
