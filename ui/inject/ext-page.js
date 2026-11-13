// Krunk 0.6.0 extensions (R26): the `chrome.*` shim for an extension's own contexts on
// https://<id>.ext.krunk.invalid: its pages (the action popup, the options page, pages in
// tabs, offscreen documents, popup windows) and, since 0.17.0, its background service worker.
// In a page it runs in the main world before the page's scripts and talks to the host through
// the page bridge (__lbHost) like Krunk's own pages, with the `ext` commands only. The host
// enforces what an extension may ask for.
//
// The background (0.17.0): the hidden page /_krunk/background.html only starts a dedicated
// Worker and relays between it and the host. The worker's bootstrap
// (core::ext_runtime::worker_bootstrap) runs this same file with a bridge to that page, then
// the extension's service worker with the engine's own importScripts. Everything this file
// needs later is taken from the global scope now: hardened extensions (SES, LavaMoat) remove
// or freeze globals once they run.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
var G = typeof globalThis === 'object' ? globalThis : self;
var loc = G.location, nav = G.navigator, XHR = G.XMLHttpRequest, timer = G.setTimeout.bind(G), rand = G.crypto;
var m = /^([a-p]{32})\.ext\.krunk\.invalid$/.exec(loc.hostname);
if (!m) return;
var extId = m[1];
var host = __lbHost;
var inWorker = typeof G.document === 'undefined';
var backgroundPage = !inWorker && loc.pathname === '/_krunk/background.html';
// The manifest, the locale messages and the background's description before the context's
// own scripts run: synchronous requests to the extension's own origin (served by the host,
// never the network).
function fetchSync(path) {
  try { var x = new XHR(); x.open('GET', path, false); x.send(); return x.status === 200 ? JSON.parse(x.responseText) : null; } catch (e) { return null; }
}
if (backgroundPage) {
  // The relay: nothing else runs in this page (no chrome.* here; the worker has it).
  var info = fetchSync('/_krunk/background.json') || {};
  var say = function (op, data) { host.postMessage({ cmd: 'ext', ext: extId, op: op, data: data }); };
  if (!info.boot) { say('log', { level: 'error', text: 'The extension has no background script.' }); say('bgReady', {}); return; }
  var worker;
  try { worker = new G.Worker(info.boot, { type: info.module ? 'module' : 'classic', name: info.install ? 'krunk-sw:install' : 'krunk-sw' }); }
  catch (e) { say('log', { level: 'error', text: 'The background worker did not start: ' + (e && e.message || e) }); say('bgReady', {}); return; }
  var running = false;
  var parse = JSON.parse;
  worker.addEventListener('message', function (e) { var d = e.data; if (d && typeof d.__krunk === 'string') { running = true; host.postMessage(parse(d.__krunk)); } });
  // A worker that never ran (its script did not load): the reason, and queued events are let
  // go so nothing waits for ever. A running worker reports its own errors.
  worker.addEventListener('error', function (e) {
    if (e && e.preventDefault) e.preventDefault();
    if (running) return;
    say('log', { level: 'error', text: 'Service worker: ' + (e && e.message || 'the script could not be loaded'), file: e && e.filename || info.boot, line: e && e.lineno || 0 });
    say('bgReady', {});
  });
  host.addEventListener('message', function (e) { try { worker.postMessage({ __krunkIn: e.data }); } catch (err) { /* not cloneable: nothing the worker could use */ } });
  return;
}
var pending = {}, nextId = 1;
var listeners = { message: [], storage: [], actionClicked: [], tabUpdated: [], tabActivated: [], tabRemoved: [], alarm: [], installed: [], startup: [], connect: [], windowRemoved: [], windowFocus: [] };
var manifest = fetchSync('/manifest.json') || {}, messages = fetchSync('/_krunk/messages.json') || {};
// What the host says about this run: `log` (KRUNK_EXT_LOG=1) sends every console level of the
// extension's contexts to krunk.log, not only errors.
var env = fetchSync('/_krunk/env.json') || {};
function post(cmd, data) { host.postMessage({ cmd: 'ext', ext: extId, op: cmd, data: data === undefined ? null : data }); }
function call(cmd, data) {
  return new Promise(function (resolve, reject) {
    var id = nextId++;
    pending[id] = { resolve: resolve, reject: reject };
    // A call the host does not take (over the bridge's size limit, not cloneable) fails; it
    // must not wait for an answer that never comes.
    try { host.postMessage({ cmd: 'ext', ext: extId, op: cmd, reqId: id, data: data === undefined ? null : data }); }
    catch (err) { delete pending[id]; reject(new Error(cmd + ': ' + (err && err.message || 'the call could not be sent'))); }
  });
}
function each(kind, fn) { listeners[kind].slice().forEach(function (l) { try { fn(l); } catch (err) { report(err); } }); }
host.addEventListener('message', function (e) {
  var d = e.data; if (!d) return;
  if (d.event === 'reply' && d.reqId !== undefined && pending[d.reqId]) { var p = pending[d.reqId]; delete pending[d.reqId]; if (d.ok) p.resolve(d.data === undefined ? null : d.data); else p.reject(new Error(d.error || 'failed')); return; }
  if (d.event !== 'ext' || d.ext !== extId) return;
  d = Object.assign({ event: d.kind }, d.data || {});
  if (d.event === 'message') {
    // One message may go to several contexts (0.17.0): a context without listeners says so,
    // one whose listeners do not answer says it is done, and the first real answer wins.
    var responded = false, async = false;
    var done = function (how) { if (responded) return; responded = true; post('respond', how); };
    var sendResponse = function (r) { done({ id: d.id, data: r === undefined ? null : r }); };
    if (!listeners.message.length) { done({ id: d.id, none: true }); return; }
    each('message', function (l) {
      var r = l(d.data, d.sender || { id: extId }, sendResponse);
      if (r === true) async = true;
      else if (r && typeof r.then === 'function') { async = true; r.then(sendResponse, function (err) { report(err); done({ id: d.id, empty: true }); }); }
    });
    if (!async) done({ id: d.id, empty: true });
  } else if (d.event === 'portConnect') {
    if (!listeners.connect.length) { post('port.close', { id: d.port }); return; }
    var port = makePort(d.port, d.name || '', d.sender || { id: extId });
    post('port.accept', { id: d.port });
    each('connect', function (l) { l(port); });
  } else if (d.event === 'portMessage') { if (ports[d.port]) ports[d.port].message(d.data); }
  else if (d.event === 'portDisconnect') { if (ports[d.port]) ports[d.port].closed(d.error); }
  else if (d.event === 'storageChanged') each('storage', function (l) { l(d.changes, d.area); });
  else if (d.event === 'actionClicked') each('actionClicked', function (l) { l(d.tab); });
  else if (d.event === 'tabUpdated') each('tabUpdated', function (l) { l(d.tabId, d.changeInfo, d.tab); });
  else if (d.event === 'tabActivated') each('tabActivated', function (l) { l({ tabId: d.tabId, windowId: d.windowId }); });
  else if (d.event === 'tabRemoved') each('tabRemoved', function (l) { l(d.tabId, { windowId: d.windowId, isWindowClosing: false }); });
  else if (d.event === 'alarm') each('alarm', function (l) { l(d.alarm); });
  else if (d.event === 'installed') each('installed', function (l) { l({ reason: d.reason || 'install', previousVersion: d.previousVersion }); });
  else if (d.event === 'startup') each('startup', function (l) { l(); });
  else if (d.event === 'windowRemoved') each('windowRemoved', function (l) { l(d.windowId); });
  else if (d.event === 'windowFocus') each('windowFocus', function (l) { l(d.windowId); });
});
// Errors of this context, for the extension's Errors list on krunk://extensions (0.17.0).
function report(e, file, line) {
  post('log', { level: 'error', text: typeof e === 'string' ? e.slice(0, 2000) : show(e).slice(0, 2000), file: file || '', line: line || 0 });
}
function cb(promise, callback) {
  if (typeof callback === 'function') { promise.then(function (v) { callback(v); }, function (e) { runtime.lastError = { message: String(e && e.message || e) }; try { callback(); } finally { runtime.lastError = undefined; } }); }
  return promise;
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
// Ports (0.17.0): chrome.runtime.connect / tabs.connect and onConnect. The host keeps the
// table (core::ext_runtime::PortTable) and tells each end about messages and about the other
// end going away; the id is this context's own choice.
var ports = {}, portSeq = 0;
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
    fire: function (a, b) { ls.slice().forEach(function (l) { try { l(a, b); } catch (err) { report(err); } }); }
  };
}
function makePort(id, name, sender) {
  var open = true, onMessage = portEvent(), onDisconnect = portEvent();
  var port = {
    name: name,
    postMessage: function (msg) {
      if (!open) throw new Error('Attempting to use a disconnected port object');
      post('port.post', { id: id, data: msg === undefined ? null : msg });
    },
    disconnect: function () { if (!open) return; open = false; delete ports[id]; post('port.close', { id: id }); },
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
function connect(name, tabId, frameId) {
  var id = portId(), port = makePort(id, name, null);
  post('port.open', { id: id, name: name, tabId: tabId === undefined ? null : tabId, frameId: frameId === undefined ? null : frameId });
  return port;
}
function area(name) {
  return {
    get: function (keys, callback) { if (typeof keys === 'function') { callback = keys; keys = null; } return cb(call('storage.get', { area: name, keys: keys === undefined ? null : keys }), callback); },
    set: function (items, callback) { return cb(call('storage.set', { area: name, items: items }), callback); },
    remove: function (keys, callback) { return cb(call('storage.remove', { area: name, keys: keys }), callback); },
    clear: function (callback) { return cb(call('storage.clear', { area: name }), callback); },
    getBytesInUse: function (keys, callback) { if (typeof keys === 'function') { callback = keys; keys = null; } return cb(call('storage.bytes', { area: name, keys: keys === undefined ? null : keys }), callback); },
    setAccessLevel: function (o, callback) { return cb(call('storage.setAccessLevel', { area: name, accessLevel: o && o.accessLevel }), callback); },
    onChanged: event('storage'),
    QUOTA_BYTES: name === 'sync' ? 102400 : 10485760
  };
}
var runtime = {
  id: extId,
  lastError: undefined,
  // Krunk's address of the file (https://<id>.ext.krunk.invalid/…): the one that loads here.
  // Senders are reported in Chrome's form (chrome-extension://<id>/…), see docs/EXTENSIONS.md.
  // In the background worker the address is in Chrome's form: there it is compared with
  // `sender.url` and tab addresses, which the host reports in that form (an extension that
  // lets only its own pages talk to it: `[getURL('/popup.html')].includes(sender.url)`).
  // The worker's fetch and importScripts and the host's tabs/windows calls take that form.
  // In pages the address is the one the engine can load (image and frame sources).
  getURL: function (p) { return (inWorker ? 'chrome-extension://' + extId + '/' : 'https://' + extId + '.ext.krunk.invalid/') + String(p || '').replace(/^\/+/, ''); },
  getManifest: function () { return JSON.parse(JSON.stringify(manifest)); },
  sendMessage: function () {
    var args = Array.prototype.slice.call(arguments);
    var callback = typeof args[args.length - 1] === 'function' ? args.pop() : null;
    var message = (args.length >= 2 && typeof args[0] === 'string' && args[0].length === 32) ? args[1] : args[0];
    return cb(call('runtime.sendMessage', { message: message === undefined ? null : message }), callback);
  },
  connect: function (a, b) { var o = (a && typeof a === 'object') ? a : (b || {}); return connect(String(o.name || '')); },
  openOptionsPage: function (callback) { return cb(call('runtime.openOptionsPage', {}), callback); },
  reload: function () { call('runtime.reload', {}); },
  setUninstallURL: function (u, callback) { return cb(Promise.resolve(), callback); },
  requestUpdateCheck: function (callback) { return cb(Promise.resolve({ status: 'no_update' }), callback); },
  getPlatformInfo: function (callback) { return cb(Promise.resolve({ os: 'win', arch: 'x86-64', nacl_arch: 'x86-64' }), callback); },
  getContexts: function (f, callback) { return cb(call('runtime.getContexts', f || {}), callback); },
  getBackgroundPage: function (callback) { return cb(Promise.reject(new Error('Krunk: the background is a service worker; it has no page')), callback); },
  onMessage: event('message'),
  onMessageExternal: noop(),
  onConnect: event('connect'),
  onConnectExternal: noop(),
  onInstalled: event('installed'),
  onStartup: event('startup'),
  onSuspend: noop(),
  onSuspendCanceled: noop(),
  onUpdateAvailable: noop(),
  OnInstalledReason: { INSTALL: 'install', UPDATE: 'update', CHROME_UPDATE: 'chrome_update', SHARED_MODULE_UPDATE: 'shared_module_update' },
  ContextType: { TAB: 'TAB', POPUP: 'POPUP', BACKGROUND: 'BACKGROUND', OFFSCREEN_DOCUMENT: 'OFFSCREEN_DOCUMENT', SIDE_PANEL: 'SIDE_PANEL' }
};
var tabs = {
  query: function (info, callback) { return cb(call('tabs.query', info || {}), callback); },
  get: function (id, callback) { return cb(call('tabs.get', { tabId: id }), callback); },
  getCurrent: function (callback) { return cb(call('tabs.getCurrent', {}), callback); },
  create: function (props, callback) { return cb(call('tabs.create', props || {}), callback); },
  update: function (a, b, c) { var id = typeof a === 'number' ? a : null, props = typeof a === 'number' ? b : a, callback = typeof a === 'number' ? c : b; return cb(call('tabs.update', { tabId: id, props: props || {} }), callback); },
  remove: function (ids, callback) { return cb(call('tabs.remove', { tabIds: Array.isArray(ids) ? ids : [ids] }), callback); },
  reload: function (id, props, callback) { if (typeof id !== 'number') { callback = props; props = id; id = null; } return cb(call('tabs.reload', { tabId: id }), callback); },
  sendMessage: function (id, message, options, callback) { if (typeof options === 'function') { callback = options; options = null; } return cb(call('tabs.sendMessage', { tabId: id, message: message === undefined ? null : message, frameId: options && options.frameId }), callback); },
  connect: function (id, o) { return connect(String(o && o.name || ''), id, o && o.frameId); },
  onUpdated: event('tabUpdated'),
  onActivated: event('tabActivated'),
  onRemoved: event('tabRemoved'),
  onCreated: noop(),
  onReplaced: noop(),
  onMoved: noop(),
  onAttached: noop(),
  onDetached: noop(),
  TAB_ID_NONE: -1
};
var action = {
  setTitle: function (d, callback) { return cb(call('action.setTitle', d), callback); },
  getTitle: function (d, callback) { return cb(call('action.getTitle', d || {}), callback); },
  setIcon: function (d, callback) { return cb(call('action.setIcon', { path: typeof d.path === 'string' ? d.path : (d.path ? Object.values(d.path).pop() : null), tabId: d.tabId }), callback); },
  setPopup: function (d, callback) { return cb(call('action.setPopup', d), callback); },
  getPopup: function (d, callback) { return cb(call('action.getPopup', d || {}), callback); },
  setBadgeText: function (d, callback) { return cb(call('action.setBadgeText', d), callback); },
  getBadgeText: function (d, callback) { return cb(call('action.getBadgeText', d || {}), callback); },
  setBadgeBackgroundColor: function (d, callback) { return cb(call('action.setBadgeBackgroundColor', { color: d.color, tabId: d.tabId }), callback); },
  getBadgeBackgroundColor: function (d, callback) { return cb(Promise.resolve([0, 0, 0, 0]), callback); },
  setBadgeTextColor: function (d, callback) { return cb(Promise.resolve(), callback); },
  enable: function (id, callback) { return cb(Promise.resolve(), callback); },
  disable: function (id, callback) { return cb(Promise.resolve(), callback); },
  isEnabled: function (id, callback) { return cb(Promise.resolve(true), callback); },
  getUserSettings: function (callback) { return cb(Promise.resolve({ isOnToolbar: true }), callback); },
  openPopup: function (o, callback) { if (typeof o === 'function') callback = o; return cb(call('action.openPopup', {}), callback); },
  onClicked: event('actionClicked')
};
var alarms = {
  create: function (name, info) { if (typeof name === 'object') { info = name; name = ''; } return call('alarms.create', { name: name || '', info: info || {} }); },
  get: function (name, callback) { if (typeof name === 'function') { callback = name; name = ''; } return cb(call('alarms.get', { name: name || '' }), callback); },
  getAll: function (callback) { return cb(call('alarms.getAll', {}), callback); },
  clear: function (name, callback) { if (typeof name === 'function') { callback = name; name = ''; } return cb(call('alarms.clear', { name: name || '' }), callback); },
  clearAll: function (callback) { return cb(call('alarms.clearAll', {}), callback); },
  onAlarm: event('alarm')
};
var i18n = {
  getMessage: function (name, subs) {
    var msg = messages[name] || messages[String(name).toLowerCase()];
    if (!msg || typeof msg.message !== 'string') return '';
    var text = msg.message, list = subs === undefined ? [] : (Array.isArray(subs) ? subs : [subs]);
    if (msg.placeholders) Object.keys(msg.placeholders).forEach(function (ph) {
      var content = String(msg.placeholders[ph].content || ''), v = content.replace(/\$(\d)/g, function (_, n) { return list[Number(n) - 1] === undefined ? '' : String(list[Number(n) - 1]); });
      text = text.split('$' + ph + '$').join(v).split('$' + ph.toUpperCase() + '$').join(v);
    });
    return text.replace(/\$(\d)/g, function (_, n) { return list[Number(n) - 1] === undefined ? '' : String(list[Number(n) - 1]); });
  },
  getUILanguage: function () { return nav.language; },
  getAcceptLanguages: function (callback) { return cb(Promise.resolve((nav.languages || [nav.language]).slice()), callback); },
  detectLanguage: function (t, callback) { return cb(Promise.resolve({ isReliable: false, languages: [] }), callback); }
};
var windows = {
  WINDOW_ID_CURRENT: -2,
  WINDOW_ID_NONE: -1,
  getCurrent: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getCurrent', {}), callback); },
  getLastFocused: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getLastFocused', {}), callback); },
  getAll: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getAll', {}), callback); },
  get: function (id, info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.get', { windowId: id }), callback); },
  // type "popup" (or "panel"): a small window of its own showing the page; else a tab.
  create: function (data, callback) { if (typeof data === 'function') { callback = data; data = null; } return cb(call('windows.create', data || {}), callback); },
  update: function (id, info, callback) { return cb(call('windows.update', { windowId: id, info: info || {} }), callback); },
  remove: function (id, callback) { return cb(call('windows.remove', { windowId: id }), callback); },
  onCreated: noop(), onRemoved: event('windowRemoved'), onFocusChanged: event('windowFocus'), onBoundsChanged: noop()
};
var scripting = {
  // `func` with `args` travels as source text (it cannot see its closure, as in Chrome); the
  // result comes back by value. world "MAIN": the page's own world.
  executeScript: function (d, callback) {
    d = d || {};
    var fn = typeof d.func === 'function' ? d.func : (typeof d.function === 'function' ? d.function : null);
    var code = fn ? '(' + fn.toString() + ')(' + (d.args || []).map(function (a) { return a === undefined ? 'undefined' : JSON.stringify(a); }).join(',') + ')' : null;
    return cb(call('scripting.executeScript', { tabId: d.target && d.target.tabId, files: d.files || null, code: code, world: d.world || 'ISOLATED' }), callback);
  },
  insertCSS: function (d, callback) { return cb(call('scripting.insertCSS', { tabId: d && d.target && d.target.tabId, files: d && d.files, css: d && d.css }), callback); },
  removeCSS: function (d, callback) { return cb(Promise.resolve(), callback); },
  registerContentScripts: function (s, callback) { return cb(call('scripting.registerContentScripts', { scripts: s || [] }), callback); },
  getRegisteredContentScripts: function (f, callback) { if (typeof f === 'function') { callback = f; f = null; } return cb(call('scripting.getRegisteredContentScripts', { ids: f && f.ids || null }), callback); },
  unregisterContentScripts: function (f, callback) { if (typeof f === 'function') { callback = f; f = null; } return cb(call('scripting.unregisterContentScripts', { ids: f && f.ids || null }), callback); },
  updateContentScripts: function (s, callback) { post('unsupported', { api: 'scripting.updateContentScripts' }); return cb(Promise.reject(new Error('Krunk: scripting.updateContentScripts is not provided')), callback); },
  ExecutionWorld: { ISOLATED: 'ISOLATED', MAIN: 'MAIN' }
};
var dnr = {
  isRegexSupported: function (o, callback) { return cb(Promise.resolve({ isSupported: true }), callback); },
  getMatchedRules: function (f, callback) { if (typeof f === 'function') callback = f; return cb(Promise.resolve({ rulesMatchedInfo: [] }), callback); },
  getEnabledRulesets: function (callback) { return cb(call('dnr.getEnabledRulesets', {}), callback); },
  getAvailableStaticRuleCount: function (callback) { return cb(Promise.resolve(30000), callback); },
  getDynamicRules: function (f, callback) { if (typeof f === 'function') callback = f; return cb(Promise.resolve([]), callback); },
  getSessionRules: function (f, callback) { if (typeof f === 'function') callback = f; return cb(Promise.resolve([]), callback); },
  updateDynamicRules: function (o, callback) { post('unsupported', { api: 'declarativeNetRequest.updateDynamicRules' }); return cb(Promise.reject(new Error('Krunk: dynamic rules are not provided')), callback); },
  updateSessionRules: function (o, callback) { post('unsupported', { api: 'declarativeNetRequest.updateSessionRules' }); return cb(Promise.reject(new Error('Krunk: session rules are not provided')), callback); },
  updateEnabledRulesets: function (o, callback) { post('unsupported', { api: 'declarativeNetRequest.updateEnabledRulesets' }); return cb(Promise.reject(new Error('Krunk: switching rulesets at runtime is not provided')), callback); },
  setExtensionActionOptions: function (o, callback) { return cb(Promise.resolve(), callback); },
  onRuleMatchedDebug: noop(),
  MAX_NUMBER_OF_STATIC_RULESETS: 50, GUARANTEED_MINIMUM_STATIC_RULES: 30000
};
// Namespaces Krunk does not provide get inert stand-ins (events that never fire, calls
// that resolve to `value`) so an extension's start-up does not die on the first missing
// property; the first use is reported and listed on krunk://extensions ("Not in Krunk").
function stub(ns, methods, events, values) {
  var o = {}, told = false;
  var tell = function () { if (!told) { told = true; post('unsupported', { api: ns }); } };
  methods.forEach(function (m) { o[m] = function () { tell(); var last = arguments[arguments.length - 1]; return cb(Promise.resolve(values ? values[m] : undefined), typeof last === 'function' ? last : null); }; });
  events.forEach(function (e) { o[e] = { addListener: function () { tell(); }, removeListener: function () {}, hasListener: function () { return false; }, hasListeners: function () { return false; } }; });
  return o;
}
// chrome.notifications (0.17.0): accepted and answered, nothing is shown (listed as not in
// Krunk on first use).
var notifications = stub('notifications', ['update', 'clear', 'getAll', 'getPermissionLevel'], ['onClosed', 'onClicked', 'onButtonClicked', 'onPermissionLevelChanged'], { update: false, clear: true, getAll: {}, getPermissionLevel: 'granted' });
var notificationSeq = 0;
notifications.create = function (id, options, callback) {
  if (typeof id !== 'string') { callback = options; id = 'krunk-notification-' + (++notificationSeq); }
  post('unsupported', { api: 'notifications' });
  return cb(Promise.resolve(id), typeof callback === 'function' ? callback : null);
};
var offscreen = {
  // One hidden page of the extension (0.17.0), with messaging; the reasons are not checked.
  createDocument: function (o, callback) { return cb(call('offscreen.createDocument', { url: o && o.url || '' }), callback); },
  closeDocument: function (callback) { return cb(call('offscreen.closeDocument', {}), callback); },
  hasDocument: function (callback) { return cb(call('offscreen.hasDocument', {}), callback); },
  Reason: { TESTING: 'TESTING', AUDIO_PLAYBACK: 'AUDIO_PLAYBACK', IFRAME_SCRIPTING: 'IFRAME_SCRIPTING', DOM_SCRAPING: 'DOM_SCRAPING', BLOBS: 'BLOBS', DOM_PARSER: 'DOM_PARSER', USER_MEDIA: 'USER_MEDIA', DISPLAY_MEDIA: 'DISPLAY_MEDIA', WEB_RTC: 'WEB_RTC', CLIPBOARD: 'CLIPBOARD', LOCAL_STORAGE: 'LOCAL_STORAGE', WORKERS: 'WORKERS', BATTERY_STATUS: 'BATTERY_STATUS', MATCH_MEDIA: 'MATCH_MEDIA', GEOLOCATION: 'GEOLOCATION' }
};
var management = {
  getSelf: function (callback) {
    var perms = (manifest.permissions || []).filter(function (p) { return typeof p === 'string'; });
    return cb(Promise.resolve({
      id: extId, name: i18nText(manifest.name), shortName: i18nText(manifest.short_name || manifest.name), description: i18nText(manifest.description),
      version: String(manifest.version || ''), enabled: true, mayDisable: true, installType: 'normal', type: 'extension', offlineEnabled: false,
      optionsUrl: manifest.options_page ? runtime.getURL(manifest.options_page) : '', permissions: perms, hostPermissions: manifest.host_permissions || []
    }), callback);
  }
};
function i18nText(s) { var k = /^__MSG_(.+)__$/.exec(String(s || '')); return k ? i18n.getMessage(k[1]) : String(s || ''); }
var chrome = {
  commands: stub('commands', ['getAll'], ['onCommand'], { getAll: [] }),
  contextMenus: stub('contextMenus', ['create', 'update', 'remove', 'removeAll'], ['onClicked']),
  notifications: notifications,
  offscreen: offscreen,
  management: management,
  userScripts: stub('userScripts', ['register', 'unregister', 'update', 'getScripts', 'configureWorld'], []),
  webNavigation: stub('webNavigation', ['getFrame', 'getAllFrames'], ['onCommitted', 'onCompleted', 'onBeforeNavigate', 'onDOMContentLoaded', 'onHistoryStateUpdated']),
  // Listeners are accepted and never called (Krunk does not show extensions the requests).
  webRequest: stub('webRequest', ['handlerBehaviorChanged'], ['onBeforeRequest', 'onBeforeSendHeaders', 'onSendHeaders', 'onHeadersReceived', 'onAuthRequired', 'onResponseStarted', 'onBeforeRedirect', 'onCompleted', 'onErrorOccurred']),
  // No cookie access: nothing is found, nothing is set.
  cookies: stub('cookies', ['get', 'getAll', 'set', 'remove', 'getAllCookieStores'], ['onChanged'], { get: null, getAll: [], set: null, remove: null, getAllCookieStores: [] }),
  idle: stub('idle', ['queryState', 'setDetectionInterval'], ['onStateChanged'], { queryState: 'active' }),
  runtime: runtime, storage: { local: area('local'), sync: area('sync'), session: area('session'), managed: { get: function (k, c) { return cb(Promise.resolve({}), c); }, onChanged: noop() }, onChanged: event('storage') },
  tabs: tabs, action: action, browserAction: action, alarms: alarms, i18n: i18n, windows: windows, scripting: scripting, declarativeNetRequest: dnr,
  extension: { getURL: runtime.getURL, inIncognitoContext: false, getViews: function () { return []; }, getBackgroundPage: function () { return null; }, isAllowedIncognitoAccess: function (c) { return cb(Promise.resolve(false), c); }, isAllowedFileSchemeAccess: function (c) { return cb(Promise.resolve(false), c); } },
  permissions: { contains: function (p, c) { return cb(Promise.resolve(true), c); }, getAll: function (c) { return cb(call('permissions.getAll', {}), c); }, request: function (p, c) { return cb(Promise.resolve(true), c); }, remove: function (p, c) { return cb(Promise.resolve(false), c); }, onAdded: noop(), onRemoved: noop() }
};
// Anything else (sidePanel, identity, …) is absent: `undefined`, reported once.
var reported = {};
var proxied = typeof Proxy === 'function' ? new Proxy(chrome, { get: function (t, k) { if (k in t) return t[k]; if (typeof k === 'string' && k !== 'then' && !reported[k]) { reported[k] = true; post('unsupported', { api: k }); } return undefined; } }) : chrome;
Object.defineProperty(G, 'chrome', { value: proxied, configurable: true, writable: true });
Object.defineProperty(G, 'browser', { value: proxied, configurable: true, writable: true });
// Uncaught errors, unhandled rejections and console.error of every context of the extension.
G.addEventListener('error', function (e) { report(e && (e.error || e.message), e && e.filename, e && e.lineno); });
G.addEventListener('unhandledrejection', function (e) { report(e && e.reason); });
// A console argument as text. Only plain data is walked: anything else (a window, a DOM
// node, a class instance) is named, not read. Walking a global object would run every
// getter on it (hardened extensions replace those with ones that warn or throw).
function show(a) {
  try {
    if (a === null || a === undefined || typeof a !== 'object' && typeof a !== 'function') return String(a);
    if (typeof a === 'function') return '[function]';
    if (typeof a.stack === 'string' || typeof a.message === 'string' && typeof a.name === 'string') return String(a.stack || a.message);
    var plain = function (v) { var p = Object.getPrototypeOf(v); return Array.isArray(v) || p === Object.prototype || p === null; };
    if (!plain(a)) return Object.prototype.toString.call(a);
    return JSON.stringify(a, function (k, v) { return v && typeof v === 'object' && !plain(v) ? Object.prototype.toString.call(v) : v; });
  } catch (x) { return '[unprintable]'; }
}
function shown(args) { return Array.prototype.map.call(args, show).join(' ').slice(0, 2000); }
if (G.console && typeof G.console.error === 'function') {
  var consoleError = G.console.error;
  try {
    G.console.error = function () {
      try { report(shown(arguments)); } catch (x) { /* a value that cannot be shown */ }
      return consoleError.apply(this, arguments);
    };
  } catch (x) { /* a frozen console */ }
  if (env.log) ['log', 'info', 'warn', 'debug'].forEach(function (level) {
    var original = G.console[level];
    if (typeof original !== 'function') return;
    try {
      G.console[level] = function () {
        try { post('log', { level: level, text: shown(arguments) }); } catch (x) { /* never in the way */ }
        return original.apply(this, arguments);
      };
    } catch (x) { /* a frozen console */ }
  });
}
if (inWorker) {
  // The extension's own files by their Chrome address (see runtime.getURL): loaded from where
  // Krunk serves them.
  (function () {
    var chromeBase = 'chrome-extension://' + extId + '/', krunkBase = 'https://' + extId + '.ext.krunk.invalid/';
    var own = function (u) { return typeof u === 'string' && u.indexOf(chromeBase) === 0 ? krunkBase + u.slice(chromeBase.length) : u; };
    var nativeFetch = G.fetch, nativeImport = G.importScripts;
    if (typeof nativeFetch === 'function') G.fetch = function (input, init) {
      if (typeof input === 'string') input = own(input);
      else if (input && typeof input.href === 'string' && typeof input.url !== 'string') input = own(input.href);
      return nativeFetch.call(G, input, init);
    };
    if (typeof nativeImport === 'function') G.importScripts = function () {
      var urls = []; for (var i = 0; i < arguments.length; i++) urls.push(own(String(arguments[i])));
      return nativeImport.apply(G, urls);
    };
  })();
  // The service worker's own globals (a dedicated worker has none of them). The worker is
  // started "activated": Krunk has no installing or waiting phase. `install` and `activate`
  // are dispatched on the first start after an installation or update only.
  var dispatch = G.dispatchEvent.bind(G), Ev = G.Event, fresh = String(G.name || '') === 'krunk-sw:install';
  var target = function () { return { addEventListener: function () {}, removeEventListener: function () {}, dispatchEvent: function () { return true; } }; };
  var sw = Object.assign(target(), { state: 'activated', scriptURL: runtime.getURL(String(manifest.background && manifest.background.service_worker || '')), onstatechange: null, onerror: null, postMessage: function () {} });
  var registration = Object.assign(target(), {
    scope: runtime.getURL(''), active: sw, installing: null, waiting: null, updateViaCache: 'imports', onupdatefound: null,
    navigationPreload: { enable: function () { return Promise.resolve(); }, disable: function () { return Promise.resolve(); }, setHeaderValue: function () { return Promise.resolve(); }, getState: function () { return Promise.resolve({ enabled: false, headerValue: '' }); } },
    unregister: function () { return Promise.resolve(true); }, update: function () { return Promise.resolve(registration); },
    showNotification: function () { return Promise.resolve(); }, getNotifications: function () { return Promise.resolve([]); }
  });
  var define = function (name, value) { try { Object.defineProperty(G, name, { value: value, configurable: true, writable: true }); } catch (x) { /* the engine has it */ } };
  define('serviceWorker', sw);
  define('registration', registration);
  define('skipWaiting', function () { return Promise.resolve(); });
  define('clients', { claim: function () { return Promise.resolve(); }, matchAll: function () { return Promise.resolve([]); }, get: function () { return Promise.resolve(undefined); }, openWindow: function (u) { return call('tabs.create', { url: String(u) }).then(function () { return null; }); } });
  // Called by the bootstrap once the service worker's script ran (or failed): the events of
  // a fresh installation, then the host lets the queued events (onInstalled, messages,
  // connections) go to the listeners that exist now.
  define('__krunkActivate', function (error) {
    if (error) report(error);
    if (fresh) ['install', 'activate'].forEach(function (type) {
      try { var ev = new Ev(type); ev.waitUntil = function (p) { if (p && typeof p.catch === 'function') p.catch(report); }; dispatch(ev); } catch (err) { report(err); }
    });
    post('bgReady', {});
  });
} else {
  // A page. The action popup tells the host its size (the host sizes the window to the
  // content; it ignores this from any other page).
  // (Taken now: a hardened page removes these globals later, and the size is measured later.)
  var doc = G.document, lastSize = '', computed = G.getComputedStyle.bind(G), Resize = G.ResizeObserver, num = G.parseFloat;
  // The window starts at the largest size the popup may have (hidden), so the body's own
  // width (popups set one) and height are the content's; a body without a width keeps the
  // viewport's. Reported on load and whenever the body resizes.
  var sendSize = function () {
    var b = doc.body, de = doc.documentElement; if (!b || !de) return;
    var cs = computed(b), ds = computed(de);
    var mx = (num(cs.marginLeft) || 0) + (num(cs.marginRight) || 0) + (num(ds.marginLeft) || 0) + (num(ds.marginRight) || 0);
    var my = (num(cs.marginTop) || 0) + (num(cs.marginBottom) || 0) + (num(ds.marginTop) || 0) + (num(ds.marginBottom) || 0);
    var w = Math.ceil(b.offsetWidth + mx), h = Math.ceil(Math.max(b.offsetHeight + my, b.scrollHeight + my));
    var key = w + 'x' + h; if (key === lastSize || !w || !h) return; lastSize = key;
    post('popupSize', { width: w, height: h });
  };
  G.addEventListener('load', function () { sendSize(); timer(sendSize, 100); timer(sendSize, 500); });
  if (typeof Resize === 'function') doc.addEventListener('DOMContentLoaded', function () { if (doc.body) new Resize(sendSize).observe(doc.body); });
  doc.addEventListener('keydown', function (e) { if (e.key === 'Escape') post('closePopup', {}); });
  // window.close() of a page in a view of its own (the popup, a popup window): the host
  // closes the window.
  var nativeClose = G.close;
  try { G.close = function () { post('closeSelf', {}); try { nativeClose.call(G); } catch (x) { /* not script-closable */ } }; } catch (x) { /* fixed by the engine */ }
}
post('ready', { url: loc.href });
