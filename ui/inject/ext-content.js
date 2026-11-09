// Krunk 0.6.0 extensions (R26): the content-script shim. Installed by the host into the
// isolated world of every web tab together with the enabled extensions' content scripts
// (`core::ext::content_bundle`). Since 0.11.7 (audit C4) each extension has its own world,
// "krunk-ext-<id>", and its own DevTools binding `__krunkExt_<id>(json)`: the host knows the
// caller from the binding, and one extension can't reach another's globals. The host answers
// with `__krunkExtDeliver(json)` evaluated in this world. Nothing here is visible to the
// page's own scripts (the isolated world has its own globals).
function (extId, manifest, messages) {
  'use strict';
  var pending = {}, nextId = 1, listeners = { message: [], storage: [] };
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
      var sendResponse = function (r) { if (responded) return; responded = true; call('respond', { id: m.id, data: r === undefined ? null : r }); };
      listeners.message.forEach(function (l) { try { if (l(m.data, { id: extId, url: m.from || '' }, sendResponse) === true) async = true; } catch (e) { report(e); } });
      if (!async && !responded) sendResponse(null);
    } else if (m.event === 'storageChanged') {
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
  function report(e) { send('log', { level: 'error', text: String(e && e.stack || e) }); }
  function cb(promise, callback) {
    if (typeof callback === 'function') { promise.then(function (v) { callback(v); }, function (e) { chrome.runtime.lastError = { message: String(e.message || e) }; try { callback(); } finally { chrome.runtime.lastError = undefined; } }); }
    return promise;
  }
  function event(kind) {
    return {
      addListener: function (fn) { if (typeof fn === 'function') listeners[kind].push(fn); },
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
    connect: function () { send('unsupported', { api: 'runtime.connect' }); throw new Error('Krunk: chrome.runtime.connect (ports) is not provided; use sendMessage'); },
    onMessage: event('message'),
    onConnect: noop(),
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
  var proxied = typeof Proxy === 'function' ? new Proxy(chrome, { get: function (t, k) { if (k in t) return t[k]; if (typeof k === 'string' && !reported[k]) { reported[k] = true; send('unsupported', { api: k }); } return undefined; } }) : chrome;
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
        try { body(proxied, proxied); } catch (e) { report(e); }
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
