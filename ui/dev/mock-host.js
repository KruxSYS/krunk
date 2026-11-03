// Mock of the Rust host for testing the built-in UI in a normal browser
// (Playwright). NOT embedded in the exe: core/build.rs skips ui/dev.
//
// Provides window.chrome.webview (postMessage/addEventListener) and
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
      case 'navigate': {
        const t = s.tabs.find(function (x) { return x.id === s.active; });
        if (t) { t.url = 'https://duckduckgo.com/?q=' + encodeURIComponent(msg.input); t.loading = true; }
        host.tabsEvent(); host.navEvent();
        break;
      }
    }
  }

  window.chrome = window.chrome || {};
  // Request handlers: host.handlers[cmd] = function (msg) { return data; }.
  // Requests with a reqId get a reply (ok + data, or the thrown error).
  host.handlers = {};
  function answer(msg) {
    if (msg.reqId === undefined) return;
    const h = host.handlers[msg.cmd];
    try {
      const data = h ? h(msg) : null;
      host.emit({ event: 'reply', reqId: msg.reqId, ok: true, data: data === undefined ? null : data });
    } catch (e) {
      host.emit({ event: 'reply', reqId: msg.reqId, ok: false, error: String(e.message || e), data: e.data });
    }
  }

  window.chrome.webview = {
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
