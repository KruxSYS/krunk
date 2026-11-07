// Krunk 0.6.0 extensions (R26): the `chrome.*` shim for an extension's own pages (the
// background page Krunk builds from the service worker, the action popup, the options page)
// on https://<id>.ext.krunk.invalid. Runs in the page's main world before its scripts; talks
// to the host through the page bridge (__lbHost) like Krunk's own pages, with the `ext`
// commands only. The host enforces what an extension may ask for.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
var m = /^([a-p]{32})\.ext\.krunk\.invalid$/.exec(location.hostname);
if (!m) return;
var extId = m[1];
var host = __lbHost;
var pending = {}, nextId = 1;
var listeners = { message: [], storage: [], actionClicked: [], tabUpdated: [], tabActivated: [], tabRemoved: [], alarm: [], installed: [], startup: [] };
var manifest = null, messages = {};
function post(cmd, data) { host.postMessage({ cmd: 'ext', ext: extId, op: cmd, data: data === undefined ? null : data }); }
function call(cmd, data) {
  return new Promise(function (resolve, reject) {
    var id = nextId++;
    pending[id] = { resolve: resolve, reject: reject };
    host.postMessage({ cmd: 'ext', ext: extId, op: cmd, reqId: id, data: data === undefined ? null : data });
  });
}
host.addEventListener('message', function (e) {
  var d = e.data; if (!d) return;
  if (d.event === 'reply' && d.reqId !== undefined && pending[d.reqId]) { var p = pending[d.reqId]; delete pending[d.reqId]; if (d.ok) p.resolve(d.data === undefined ? null : d.data); else p.reject(new Error(d.error || 'failed')); return; }
  if (d.event !== 'ext' || d.ext !== extId) return;
  d = Object.assign({ event: d.kind }, d.data || {});
  if (d.event === 'message') {
    var responded = false, async = false;
    var sendResponse = function (r) { if (responded) return; responded = true; post('respond', { id: d.id, data: r === undefined ? null : r }); };
    listeners.message.forEach(function (l) { try { if (l(d.data, d.sender || { id: extId }, sendResponse) === true) async = true; } catch (err) { report(err); } });
    if (!async && !responded) sendResponse(null);
  } else if (d.event === 'storageChanged') listeners.storage.forEach(function (l) { try { l(d.changes, d.area); } catch (err) { report(err); } });
  else if (d.event === 'actionClicked') listeners.actionClicked.forEach(function (l) { try { l(d.tab); } catch (err) { report(err); } });
  else if (d.event === 'tabUpdated') listeners.tabUpdated.forEach(function (l) { try { l(d.tabId, d.changeInfo, d.tab); } catch (err) { report(err); } });
  else if (d.event === 'tabActivated') listeners.tabActivated.forEach(function (l) { try { l({ tabId: d.tabId, windowId: d.windowId }); } catch (err) { report(err); } });
  else if (d.event === 'tabRemoved') listeners.tabRemoved.forEach(function (l) { try { l(d.tabId, { windowId: d.windowId, isWindowClosing: false }); } catch (err) { report(err); } });
  else if (d.event === 'alarm') listeners.alarm.forEach(function (l) { try { l(d.alarm); } catch (err) { report(err); } });
  else if (d.event === 'installed') listeners.installed.forEach(function (l) { try { l({ reason: d.reason || 'install', previousVersion: d.previousVersion }); } catch (err) { report(err); } });
  else if (d.event === 'startup') listeners.startup.forEach(function (l) { try { l(); } catch (err) { report(err); } });
});
function report(e) { post('log', { level: 'error', text: String(e && e.stack || e) }); }
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
    setAccessLevel: function (o, callback) { return cb(Promise.resolve(), callback); },
    onChanged: event('storage'),
    QUOTA_BYTES: name === 'sync' ? 102400 : 10485760
  };
}
var runtime = {
  id: extId,
  lastError: undefined,
  getURL: function (p) { return 'https://' + extId + '.ext.krunk.invalid/' + String(p || '').replace(/^\/+/, ''); },
  getManifest: function () { return manifest ? JSON.parse(JSON.stringify(manifest)) : {}; },
  sendMessage: function () {
    var args = Array.prototype.slice.call(arguments);
    var callback = typeof args[args.length - 1] === 'function' ? args.pop() : null;
    var message = (args.length >= 2 && typeof args[0] === 'string' && args[0].length === 32) ? args[1] : args[0];
    return cb(call('runtime.sendMessage', { message: message === undefined ? null : message }), callback);
  },
  connect: function () { post('unsupported', { api: 'runtime.connect' }); throw new Error('Krunk: chrome.runtime.connect (ports) is not provided; use sendMessage'); },
  openOptionsPage: function (callback) { return cb(call('runtime.openOptionsPage', {}), callback); },
  reload: function () { call('runtime.reload', {}); },
  setUninstallURL: function (u, callback) { return cb(Promise.resolve(), callback); },
  getPlatformInfo: function (callback) { return cb(Promise.resolve({ os: 'win', arch: 'x86-64', nacl_arch: 'x86-64' }), callback); },
  getContexts: function (f, callback) { return cb(Promise.resolve([]), callback); },
  onMessage: event('message'),
  onMessageExternal: noop(),
  onConnect: noop(),
  onConnectExternal: noop(),
  onInstalled: event('installed'),
  onStartup: event('startup'),
  onSuspend: noop(),
  onUpdateAvailable: noop()
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
  getUILanguage: function () { return navigator.language; },
  getAcceptLanguages: function (callback) { return cb(Promise.resolve(navigator.languages.slice()), callback); },
  detectLanguage: function (t, callback) { return cb(Promise.resolve({ isReliable: false, languages: [] }), callback); }
};
var windows = {
  WINDOW_ID_CURRENT: -2,
  WINDOW_ID_NONE: -1,
  getCurrent: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getCurrent', {}), callback); },
  getLastFocused: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getCurrent', {}), callback); },
  getAll: function (info, callback) { if (typeof info === 'function') { callback = info; } return cb(call('windows.getAll', {}), callback); },
  create: function (data, callback) { return cb(call('tabs.create', { url: data && data.url, active: true }).then(function (t) { return { id: t && t.windowId, tabs: [t] }; }), callback); },
  update: function (id, info, callback) { return cb(Promise.resolve({ id: id }), callback); },
  onCreated: noop(), onRemoved: noop(), onFocusChanged: noop()
};
var scripting = {
  executeScript: function (d, callback) { return cb(call('scripting.executeScript', { tabId: d && d.target && d.target.tabId, files: d && d.files, code: d && typeof d.func === 'function' ? '(' + d.func.toString() + ')(' + (d.args || []).map(function (a) { return JSON.stringify(a); }).join(',') + ')' : null }), callback); },
  insertCSS: function (d, callback) { return cb(call('scripting.insertCSS', { tabId: d && d.target && d.target.tabId, files: d && d.files, css: d && d.css }), callback); },
  removeCSS: function (d, callback) { return cb(Promise.resolve(), callback); },
  registerContentScripts: function (s, callback) { post('unsupported', { api: 'scripting.registerContentScripts' }); return cb(Promise.resolve(), callback); },
  getRegisteredContentScripts: function (f, callback) { return cb(Promise.resolve([]), callback); },
  unregisterContentScripts: function (f, callback) { return cb(Promise.resolve(), callback); }
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
// that resolve to nothing) so an extension's start-up does not die on the first missing
// property; the first use is reported and listed on krunk://extensions.
function stub(ns, methods, events) {
  var o = {}, told = false;
  var tell = function () { if (!told) { told = true; post('unsupported', { api: ns }); } };
  methods.forEach(function (m) { o[m] = function () { tell(); var last = arguments[arguments.length - 1]; return cb(Promise.resolve(undefined), typeof last === 'function' ? last : null); }; });
  events.forEach(function (e) { o[e] = { addListener: function () { tell(); }, removeListener: function () {}, hasListener: function () { return false; }, hasListeners: function () { return false; } }; });
  return o;
}
var chrome = {
  commands: stub('commands', ['getAll'], ['onCommand']),
  contextMenus: stub('contextMenus', ['create', 'update', 'remove', 'removeAll'], ['onClicked']),
  notifications: stub('notifications', ['create', 'update', 'clear', 'getAll', 'getPermissionLevel'], ['onClosed', 'onClicked', 'onButtonClicked']),
  offscreen: stub('offscreen', ['createDocument', 'closeDocument', 'hasDocument'], []),
  userScripts: stub('userScripts', ['register', 'unregister', 'update', 'getScripts', 'configureWorld'], []),
  webNavigation: stub('webNavigation', ['getFrame', 'getAllFrames'], ['onCommitted', 'onCompleted', 'onBeforeNavigate', 'onDOMContentLoaded', 'onHistoryStateUpdated']),
  cookies: stub('cookies', ['get', 'getAll', 'set', 'remove', 'getAllCookieStores'], ['onChanged']),
  idle: stub('idle', ['queryState', 'setDetectionInterval'], ['onStateChanged']),
  runtime: runtime, storage: { local: area('local'), sync: area('sync'), session: area('session'), managed: { get: function (k, c) { return cb(Promise.resolve({}), c); }, onChanged: noop() }, onChanged: event('storage') },
  tabs: tabs, action: action, browserAction: action, alarms: alarms, i18n: i18n, windows: windows, scripting: scripting, declarativeNetRequest: dnr,
  extension: { getURL: runtime.getURL, inIncognitoContext: false, isAllowedIncognitoAccess: function (c) { return cb(Promise.resolve(false), c); }, isAllowedFileSchemeAccess: function (c) { return cb(Promise.resolve(false), c); } },
  permissions: { contains: function (p, c) { return cb(Promise.resolve(true), c); }, getAll: function (c) { return cb(call('permissions.getAll', {}), c); }, request: function (p, c) { return cb(Promise.resolve(true), c); }, remove: function (p, c) { return cb(Promise.resolve(false), c); }, onAdded: noop(), onRemoved: noop() }
};
var reported = {};
var proxied = typeof Proxy === 'function' ? new Proxy(chrome, { get: function (t, k) { if (k in t) return t[k]; if (typeof k === 'string' && k !== 'then' && !reported[k]) { reported[k] = true; post('unsupported', { api: k }); } return undefined; } }) : chrome;
Object.defineProperty(window, 'chrome', { value: proxied, configurable: true, writable: true });
Object.defineProperty(window, 'browser', { value: proxied, configurable: true, writable: true });
// The background page Krunk builds runs the service worker as a classic script: give it the
// worker's globals it uses.
if (location.pathname === '/_krunk/background.html') {
  window.self = window;
  window.importScripts = function () {
    Array.prototype.slice.call(arguments).forEach(function (u) {
      var x = new XMLHttpRequest(); x.open('GET', runtime.getURL(u), false); x.send();
      if (x.status === 200) (0, eval)(x.responseText + '\n//# sourceURL=' + runtime.getURL(u)); else report(new Error('importScripts: ' + u + ' ' + x.status));
    });
  };
  window.skipWaiting = function () { return Promise.resolve(); };
  window.clients = { claim: function () { return Promise.resolve(); }, matchAll: function () { return Promise.resolve([]); } };
  window.registration = { scope: runtime.getURL(''), unregister: function () { return Promise.resolve(true); }, update: function () { return Promise.resolve(); } };
  window.addEventListener('error', function (e) { report(e.error || e.message); });
  window.addEventListener('unhandledrejection', function (e) { report(e.reason); });
}
// The manifest and the locale messages before the page's own scripts run: synchronous
// requests to the extension's own origin (served by the host, never the network).
function fetchSync(path) {
  try { var x = new XMLHttpRequest(); x.open('GET', path, false); x.send(); return x.status === 200 ? JSON.parse(x.responseText) : null; } catch (e) { return null; }
}
manifest = fetchSync('/manifest.json') || {};
messages = fetchSync('/_krunk/messages.json') || {};
// The popup tells the host its size (the host sizes the window to the content).
if (location.pathname !== '/_krunk/background.html') {
  var lastSize = '';
  // The window starts at the largest size the popup may have (hidden), so the body's own
  // width (popups set one) and height are the content's; a body without a width keeps the
  // viewport's. Reported on load and whenever the body resizes.
  var sendSize = function () {
    var b = document.body, de = document.documentElement; if (!b || !de) return;
    var cs = getComputedStyle(b), ds = getComputedStyle(de);
    var mx = (parseFloat(cs.marginLeft) || 0) + (parseFloat(cs.marginRight) || 0) + (parseFloat(ds.marginLeft) || 0) + (parseFloat(ds.marginRight) || 0);
    var my = (parseFloat(cs.marginTop) || 0) + (parseFloat(cs.marginBottom) || 0) + (parseFloat(ds.marginTop) || 0) + (parseFloat(ds.marginBottom) || 0);
    var w = Math.ceil(b.offsetWidth + mx), h = Math.ceil(Math.max(b.offsetHeight + my, b.scrollHeight + my));
    var key = w + 'x' + h; if (key === lastSize || !w || !h) return; lastSize = key;
    post('popupSize', { width: w, height: h });
  };
  window.addEventListener('load', function () { sendSize(); setTimeout(sendSize, 100); setTimeout(sendSize, 500); });
  if (typeof ResizeObserver === 'function') document.addEventListener('DOMContentLoaded', function () { if (document.body) new ResizeObserver(sendSize).observe(document.body); });
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') post('closePopup', {}); });
}
post('ready', { url: location.href });
