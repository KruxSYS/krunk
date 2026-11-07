// Message bridge between Krunk's built-in UI and the Rust host.
// Requests: LB.post(cmd, args) fire-and-forget, LB.request(cmd, args) -> Promise.
// Events: LB.on(eventName, handler).
(function () {
  'use strict';
  // window.__lbHost is installed by the browser before any page script runs.
  const wv = window.__lbHost;
  let nextId = 1;
  const pending = new Map();
  const listeners = new Map();

  function send(msg) {
    if (wv) wv.postMessage(msg);
  }

  function post(cmd, args) {
    send(Object.assign({ cmd: cmd }, args || {}));
  }

  function request(cmd, args) {
    return new Promise(function (resolve, reject) {
      const reqId = nextId++;
      pending.set(reqId, { resolve: resolve, reject: reject });
      send(Object.assign({ cmd: cmd, reqId: reqId }, args || {}));
    });
  }

  function on(event, handler) {
    if (!listeners.has(event)) listeners.set(event, []);
    listeners.get(event).push(handler);
  }

  function dispatch(msg) {
    if (!msg || typeof msg.event !== 'string') return;
    if (msg.event === 'reply') {
      const p = pending.get(msg.reqId);
      if (!p) return;
      pending.delete(msg.reqId);
      if (msg.ok) p.resolve(msg.data);
      else {
        const err = new Error(msg.error || 'request failed');
        err.data = msg.data;
        p.reject(err);
      }
      return;
    }
    (listeners.get(msg.event) || []).forEach(function (h) {
      try { h(msg); } catch (e) { console.error(e); }
    });
  }

  if (wv) wv.addEventListener('message', function (e) { dispatch(e.data); });

  // Small DOM helper: el('div', {class: 'x', onclick: f}, [children or text]).
  function el(tag, attrs, children) {
    const n = document.createElement(tag);
    Object.keys(attrs || {}).forEach(function (k) {
      const v = attrs[k];
      if (v === undefined || v === null || v === false) return;
      if (k.startsWith('on')) n.addEventListener(k.slice(2), v);
      else if (k === 'text') n.textContent = v;
      else n.setAttribute(k, v === true ? '' : v);
    });
    (children || []).forEach(function (c) {
      if (c === null || c === undefined) return;
      n.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
    });
    return n;
  }

  // Theme for every built-in page: "system" follows prefers-color-scheme.
  function applyTheme(theme, accent, colors) {
    const root = document.documentElement;
    root.classList.toggle('dark', theme === 'dark');
    root.classList.toggle('light', theme === 'light');
    const set = (name, v) => { if (v) root.style.setProperty(name, v); else root.style.removeProperty(name); };
    // 0.6.0: the accent colour from Settings → Appearance (empty = the theme's own); 0.7.0: with
    // its readable text colour, and the background colour with the text drawn on it (R3).
    const c = colors || {};
    set('--accent', accent || c.accent);
    set('--accent-fg', c.accentFg);
    set('--bg', c.background);
    set('--on-bg', c.backgroundFg);
  }
  // The UI font size (Settings → Appearance) for Krunk's own pages.
  function applyFontSize(px) {
    const fs = px && px !== 13 ? px + 'px' : '';
    document.documentElement.style.fontSize = fs;
    if (document.body) document.body.style.fontSize = fs;
  }
  on('theme', function (m) { applyTheme(m.theme, m.accent, m.colors); if (m.uiFontPx !== undefined) applyFontSize(m.uiFontPx); });

  window.LB = { post: post, request: request, on: on, el: el, applyTheme: applyTheme, _dispatch: dispatch };
})();
