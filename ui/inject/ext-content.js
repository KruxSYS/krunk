// Krunk 0.6.0 extensions (R26): the content-script shim. Installed by the host into the
// isolated world of every web tab together with the enabled extensions' content scripts
// (`core::ext::content_bundle`). Since 0.11.7 (audit C4) each extension has its own world,
// "krunk-ext-<id>", and its own DevTools binding `__krunkExt_<id>(json)`: the host knows the
// caller from the binding, and one extension can't reach another's globals. The host answers
// with `__krunkExtDeliver(json)` evaluated in this world. Nothing here is visible to the
// page's own scripts (the isolated world has its own globals).
//
// 0.17.0: `chrome` and `browser` are globals of the world (scripts used to get them as function
// arguments only, so `globalThis.chrome` and bundlers' feature tests found nothing), ports
// (`runtime.connect`, `onConnect` for `tabs.connect`), and a message is answered as "no
// listener" / "no answer" so the host can tell the sender what Chrome would.
function (extId, manifest, messages) {
  'use strict';
  var pending = {}, nextId = 1, listeners = { message: [], storage: [], connect: [] };
  var deliver = window.__krunkExtDeliver;
  if (!deliver) {
    var byExt = {};
    window.__krunkExtDeliver = function (json) {
      var m; try { m = JSON.parse(json); } catch (e) { return; }
      var target = byExt[m.ext]; if (target) target(m);
    };
    window.__krunkExtRegister = function (id, fn) { byExt[id] = fn; };
  }
  window.__krunkExtRegister(extId, function (m) {
    if (m.reply !== undefined && pending[m.reply]) { var p = pending[m.reply]; delete pending[m.reply]; if (m.error) p.reject(new Error(m.error)); else p.resolve(m.data); }
    else if (m.event === 'message') {
      // A message from the extension's background or popup to this tab.
      var responded = false, async = false;
      var done = function (how) { if (responded) return; responded = true; send('respond', how); };
      var sendResponse = function (r) { done({ id: m.id, data: r === undefined ? null : r }); };
      if (!listeners.message.length) { done({ id: m.id, none: true }); return; }
      var sender = m.sender || { id: extId, url: m.from || '' };
      listeners.message.slice().forEach(function (l) {
        try {
          var r = l(m.data, sender, sendResponse);
          if (r === true) async = true;
          else if (r && typeof r.then === 'function') { async = true; r.then(sendResponse, function (e) { report(e); done({ id: m.id, empty: true }); }); }
        } catch (e) { report(e); }
      });
      if (!async) done({ id: m.id, empty: true });
    } else if (m.event === 'portConnect') {
      if (!listeners.connect.length) { send('port.close', { id: m.port }); return; }
      var port = makePort(m.port, m.name || '', m.sender || { id: extId });
      send('port.accept', { id: m.port });
      listeners.connect.slice().forEach(function (l) { try { l(port); } catch (e) { report(e); } });
    } else if (m.event === 'portMessage') { if (ports[m.port]) ports[m.port].message(m.data); }
    else if (m.event === 'portDisconnect') { if (ports[m.port]) ports[m.port].closed(m.error); }
    else if (m.event === 'storageChanged') {
      listeners.storage.forEach(function (l) { try { l(m.changes, m.area); } catch (e) { report(e); } });
    }
  });
  var bindingName = '__krunkExt_' + extId;
  function host(json) { return window[bindingName](json); }
  function send(cmd, data) { try { host(JSON.stringify({ ext: extId, cmd: cmd, data: data })); } catch (e) { /* no binding */ } }
  function call(cmd, data) {
    return new Promise(function (resolve, reject) {
      var id = nextId++;
      pending[id] = { resolve: resolve, reject: reject };
      try { host(JSON.stringify({ ext: extId, id: id, cmd: cmd, data: data })); }
      catch (e) { delete pending[id]; reject(new Error('Krunk: the extension binding is not available')); }
    });
  }
  function report(e) { var text; try { text = String(e && (e.stack || e.message) || e); } catch (x) { text = 'error'; } send('log', { level: 'error', text: text.slice(0, 2000) }); }
  function cb(promise, callback) {
    if (typeof callback === 'function') { promise.then(function (v) { callback(v); }, function (e) { runtime.lastError = { message: String(e && e.message || e) }; try { callback(); } finally { runtime.lastError = undefined; } }); }
    return promise;
  }
  // Ports: the same as in ext-page.js (the host keeps the table).
  var ports = {}, portSeq = 0, rand = window.crypto;
  function portId() {
    var a = new Uint32Array(2); rand.getRandomValues(a);
    return 'p' + a[0].toString(36) + a[1].toString(36) + '_' + (++portSeq);
  }
  function portEvent() {
    var ls = [];
    return {
      api: {
        addListener: function (fn) { if (typeof fn === 'function' && ls.indexOf(fn) < 0) ls.push(fn); },
        removeListener: function (fn) { var i = ls.indexOf(fn); if (i >= 0) ls.splice(i, 1); },
        hasListener: function (fn) { return ls.indexOf(fn) >= 0; },
        hasListeners: function () { return ls.length > 0; }
      },
      fire: function (a, b) { ls.slice().forEach(function (l) { try { l(a, b); } catch (e) { report(e); } }); }
    };
  }
  function makePort(id, name, sender) {
    var open = true, onMessage = portEvent(), onDisconnect = portEvent();
    var port = {
      name: name,
      postMessage: function (msg) {
        if (!open) throw new Error('Attempting to use a disconnected port object');
        send('port.post', { id: id, data: msg === undefined ? null : msg });
      },
      disconnect: function () { if (!open) return; open = false; delete ports[id]; send('port.close', { id: id }); },
      onMessage: onMessage.api,
      onDisconnect: onDisconnect.api
    };
    if (sender) port.sender = sender;
    ports[id] = {
      message: function (data) { if (open) onMessage.fire(data, port); },
      closed: function (error) {
        if (!open) return; open = false; delete ports[id];
        if (error) runtime.lastError = { message: error };
        try { onDisconnect.fire(port); } finally { runtime.lastError = undefined; }
      }
    };
    return port;
  }
  function event(kind) {
    return {
      addListener: function (fn) { if (typeof fn === 'function' && listeners[kind].indexOf(fn) < 0) listeners[kind].push(fn); },
      removeListener: function (fn) { listeners[kind] = listeners[kind].filter(function (x) { return x !== fn; }); },
      hasListener: function (fn) { return listeners[kind].indexOf(fn) >= 0; },
      hasListeners: function () { return listeners[kind].length > 0; }
    };
  }
  function noop() { return { addListener: function () {}, removeListener: function () {}, hasListener: function () { return false; }, hasListeners: function () { return false; } }; }
  function area(name) {
    return {
      get: function (keys, callback) { if (typeof keys === 'function') { callback = keys; keys = null; } return cb(call('storage.get', { area: name, keys: keys === undefined ? null : keys }), callback); },
      set: function (items, callback) { return cb(call('storage.set', { area: name, items: items }), callback); },
      remove: function (keys, callback) { return cb(call('storage.remove', { area: name, keys: keys }), callback); },
      clear: function (callback) { return cb(call('storage.clear', { area: name }), callback); },
      getBytesInUse: function (keys, callback) { if (typeof keys === 'function') { callback = keys; keys = null; } return cb(call('storage.bytes', { area: name, keys: keys === undefined ? null : keys }), callback); },
      onChanged: event('storage'),
      QUOTA_BYTES: name === 'sync' ? 102400 : 10485760
    };
  }
  var runtime = {
    id: extId,
    lastError: undefined,
    getURL: function (p) { return 'https://' + extId + '.ext.krunk.invalid/' + String(p || '').replace(/^\/+/, ''); },
    getManifest: function () { return JSON.parse(JSON.stringify(manifest)); },
    sendMessage: function (a, b, c, d) {
      // (message), (message, cb), (extId, message), (message, options), (extId, message, options, cb)
      var args = Array.prototype.slice.call(arguments);
      var callback = typeof args[args.length - 1] === 'function' ? args.pop() : null;
      var message = (args.length >= 2 && typeof args[0] === 'string' && args[0].length === 32) ? args[1] : args[0];
      return cb(call('runtime.sendMessage', { message: message === undefined ? null : message }), callback);
    },
    connect: function (a, b) {
      var o = (a && typeof a === 'object') ? a : (b || {}), id = portId(), name = String(o.name || '');
      var port = makePort(id, name, null);
      send('port.open', { id: id, name: name });
      return port;
    },
    onMessage: event('message'),
    onConnect: event('connect'),
    onInstalled: noop(),
    onStartup: noop(),
    getPlatformInfo: function (callback) { return cb(Promise.resolve({ os: 'win', arch: 'x86-64', nacl_arch: 'x86-64' }), callback); }
  };
  var i18n = {
    getMessage: function (name, subs) {
      var m = messages[name] || messages[String(name).toLowerCase()];
      if (!m || typeof m.message !== 'string') return '';
      var text = m.message, list = subs === undefined ? [] : (Array.isArray(subs) ? subs : [subs]);
      if (m.placeholders) Object.keys(m.placeholders).forEach(function (ph) {
        var content = String(m.placeholders[ph].content || ''), v = content.replace(/\$(\d)/g, function (_, n) { return list[Number(n) - 1] === undefined ? '' : String(list[Number(n) - 1]); });
        text = text.split('$' + ph + '$').join(v).split('$' + ph.toUpperCase() + '$').join(v);
      });
      return text.replace(/\$(\d)/g, function (_, n) { return list[Number(n) - 1] === undefined ? '' : String(list[Number(n) - 1]); });
    },
    getUILanguage: function () { return navigator.language; },
    getAcceptLanguages: function (callback) { return cb(Promise.resolve(navigator.languages.slice()), callback); }
  };
  var chrome = {
    runtime: runtime,
    storage: { local: area('local'), sync: area('sync'), session: area('session'), managed: { get: function (k, c) { return cb(Promise.resolve({}), c); }, onChanged: noop() }, onChanged: event('storage') },
    i18n: i18n,
    extension: { getURL: runtime.getURL, inIncognitoContext: false }
  };
  // Anything else is reported once as unsupported and returns undefined (a TypeError in the
  // extension, not a silent no-op that hides the gap).
  var reported = {};
  var proxied = typeof Proxy === 'function' ? new Proxy(chrome, { get: function (t, k) { if (k in t) return t[k]; if (typeof k === 'string' && k !== 'then' && !reported[k]) { reported[k] = true; send('unsupported', { api: k }); } return undefined; } }) : chrome;
  // Globals of this world only (the page's world has its own `chrome`, if any).
  try {
    Object.defineProperty(window, 'chrome', { value: proxied, configurable: true, writable: true });
    Object.defineProperty(window, 'browser', { value: proxied, configurable: true, writable: true });
  } catch (e) { /* the scripts still get them as arguments */ }
  var runAt = { document_start: 0, document_end: 1, document_idle: 2 };
  function matches(pattern, url) {
    // The same rules as core::ext::MatchPattern (scheme, host with *. prefix, path glob).
    if (pattern === '<all_urls>') return /^https?:$/.test(location.protocol);
    var m = /^(\*|https?|file|ftp):\/\/([^/]*)(\/.*)?$/.exec(pattern); if (!m) return false;
    var u; try { u = new URL(url); } catch (e) { return false; }
    var scheme = u.protocol.replace(':', '');
    if (m[1] === '*' ? !(scheme === 'http' || scheme === 'https') : scheme !== m[1]) return false;
    var host = u.hostname.toLowerCase(), ph = m[2].toLowerCase();
    if (ph !== '*' && m[1] !== 'file') { if (ph.indexOf('*.') === 0) { var d = ph.slice(2); if (host !== d && host.slice(-d.length - 1) !== '.' + d) return false; } else if (host !== ph) return false; }
    return glob(m[3] || '/', u.pathname + u.search);
  }
  // `*` glob over a path (the same as core::ext::glob_match; no RegExp from manifest text).
  function glob(p, t) {
    var pi = 0, ti = 0, star = -1, mark = 0;
    while (ti < t.length) {
      if (pi < p.length && p[pi] === t[ti]) { pi++; ti++; }
      else if (pi < p.length && p[pi] === '*') { star = pi; mark = ti; pi++; }
      else if (star >= 0) { pi = star + 1; mark++; ti = mark; }
      else return false;
    }
    while (pi < p.length && p[pi] === '*') pi++;
    return pi === p.length;
  }
  var ext = {
    run: function (includes, excludes, allFrames, when, css, body) {
      var url = location.href;
      if (!allFrames && window.top !== window) return;
      if (!/^https?:$/.test(location.protocol) || /(^|\.)krunk\.invalid$/.test(location.hostname)) return;
      if (!includes.some(function (p) { return matches(p, url); })) return;
      if (excludes.some(function (p) { return matches(p, url); })) return;
      // At document start the document may have no element yet (the bundle runs before
      // parsing): the stylesheet links wait for one.
      var addCss = function () {
        var root = document.head || document.documentElement;
        if (!root) { setTimeout(addCss, 0); return; }
        css.forEach(function (f) { try { var l = document.createElement('link'); l.rel = 'stylesheet'; l.href = runtime.getURL(f); root.appendChild(l); } catch (e) { report(e); } });
      };
      var go = function () {
        if (css.length) addCss();
        try { body.call(window, proxied, proxied); } catch (e) { report(e); }
      };
      var stage = runAt[when] === undefined ? 2 : runAt[when];
      if (stage === 0 || document.readyState === 'complete' || (stage === 1 && document.readyState !== 'loading')) go();
      else if (stage === 1) document.addEventListener('DOMContentLoaded', go, { once: true });
      else window.addEventListener('load', go, { once: true });
    },
    report: report
  };
  return ext;
}
