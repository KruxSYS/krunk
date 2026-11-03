// Message bridge between LightBrowser's built-in UI and the Rust host.
// Requests: LB.post(cmd, args) fire-and-forget, LB.request(cmd, args) -> Promise.
// Events: LB.on(eventName, handler).
(function () {
  'use strict';
  const wv = window.chrome && window.chrome.webview;
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
      if (msg.ok) p.resolve(msg.data); else p.reject(new Error(msg.error || 'request failed'));
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

  window.LB = { post: post, request: request, on: on, el: el, _dispatch: dispatch };
})();
