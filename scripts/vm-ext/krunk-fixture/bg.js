// Background service worker: answers content scripts, counts messages, keeps an alarm, and
// (0.17.0) is the far end of the self-test the options page runs: a worker global, a script
// imported from a subfolder, ports, an offscreen document, a popup window and content scripts
// registered at runtime.
importScripts('lib/sw-helper.js');
var messages = 0, sawInstall = false, echoDisconnects = 0;
self.addEventListener('install', function () { sawInstall = true; });
chrome.runtime.onInstalled.addListener(function (d) { chrome.storage.local.set({ installedReason: d.reason }); });

// Ports. "echo": every message comes back with who sent it. "cs": the same for content
// scripts. "window": the popup window of the self-test says hello and is told to close.
var windowHello = null;
chrome.runtime.onConnect.addListener(function (port) {
  if (port.name === 'echo' || port.name === 'cs') {
    port.onMessage.addListener(function (msg) { port.postMessage({ echo: msg, sender: port.sender, name: port.name }); });
    port.onDisconnect.addListener(function () { echoDisconnects++; });
  } else if (port.name === 'window') {
    port.onMessage.addListener(function (msg) { if (windowHello) windowHello(msg, port); });
  }
});

var DYN = { id: 'dyn', matches: ['http://localhost/*', 'http://127.0.0.1/*'], js: ['dyn.js'], runAt: 'document_end' };
function offscreenTest() {
  return new Promise(function (resolve) {
    var done = false, finish = function (r) { if (done) return; done = true; chrome.runtime.onMessage.removeListener(listen); resolve(r); };
    var listen = function (msg) {
      if (!msg || msg.kind !== 'offscreen-ready') return;
      chrome.offscreen.hasDocument().then(function (has) {
        return chrome.offscreen.closeDocument().then(function () { return chrome.offscreen.hasDocument(); }).then(function (after) { finish({ parsed: msg.parsed, has: has, hasAfterClose: after }); });
      }).catch(function (e) { finish({ error: String(e && e.message || e) }); });
    };
    chrome.runtime.onMessage.addListener(listen);
    chrome.offscreen.createDocument({ url: 'offscreen.html', reasons: ['DOM_PARSER'], justification: 'self-test' }).catch(function (e) { finish({ error: String(e && e.message || e) }); });
    setTimeout(function () { finish({ error: 'the offscreen document did not report within 8 s' }); }, 8000);
  });
}
function windowTest() {
  return new Promise(function (resolve) {
    var out = { created: null, hello: null, listed: false, removed: false }, done = false;
    var finish = function () { if (done) return; done = true; windowHello = null; chrome.windows.onRemoved.removeListener(removed); resolve(out); };
    var removed = function (id) { if (out.created && id === out.created.id) { out.removed = true; finish(); } };
    chrome.windows.onRemoved.addListener(removed);
    windowHello = function (msg, port) {
      out.hello = msg;
      chrome.windows.getAll().then(function (all) {
        out.listed = all.some(function (w) { return out.created && w.id === out.created.id && w.type === 'popup'; });
        port.postMessage({ close: true });
      });
    };
    chrome.windows.create({ url: 'window.html', type: 'popup', width: 340, height: 260 }).then(function (w) { out.created = w; }, function (e) { out.error = String(e && e.message || e); finish(); });
    setTimeout(function () { if (!out.error) out.error = 'the window did not open, connect and close within 10 s'; finish(); }, 10000);
  });
}
function registerTest() {
  return chrome.scripting.unregisterContentScripts({ ids: ['dyn'] }).catch(function () {})
    .then(function () { return chrome.scripting.registerContentScripts([DYN]); })
    .then(function () { return chrome.scripting.registerContentScripts([DYN]).then(function () { return 'no error'; }, function (e) { return String(e && e.message || e); }); })
    .then(function (duplicate) { return chrome.scripting.getRegisteredContentScripts().then(function (list) { return { ids: list.map(function (s) { return s.id; }), duplicate: duplicate }; }); })
    .catch(function (e) { return { error: String(e && e.message || e) }; });
}

chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
  messages++;
  if (msg && msg.kind === 'hello') {
    chrome.storage.local.set({ messages: messages, lastSender: sender && sender.tab ? sender.tab.id : null });
    sendResponse({ text: 'hi tab ' + (sender && sender.tab ? sender.tab.id : '?') + ' (message #' + messages + ')' });
    chrome.action.setBadgeText({ text: String(messages) });
    return true;
  }
  if (msg && msg.kind === 'tabs') {
    chrome.tabs.query({}, function (tabs) { sendResponse({ count: tabs.length, active: tabs.filter(function (t) { return t.active; }).map(function (t) { return t.url; }) }); });
    return true;
  }
  // The self-test (options.js).
  if (msg && msg.kind === 'env') {
    sendResponse({
      noWindow: typeof window === 'undefined', noDocument: typeof document === 'undefined',
      swState: self.serviceWorker ? self.serviceWorker.state : null, imported: self.krunkSwHelper || null,
      globalChrome: typeof globalThis.chrome === 'object' && globalThis.chrome.runtime.id === chrome.runtime.id,
      sawInstall: sawInstall, echoDisconnects: echoDisconnects, sender: sender,
      clients: typeof self.clients === 'object', registration: typeof self.registration === 'object', skipWaiting: typeof self.skipWaiting === 'function'
    });
    return false;
  }
  if (msg && msg.kind === 'offscreen') { offscreenTest().then(sendResponse); return true; }
  if (msg && msg.kind === 'window') { windowTest().then(sendResponse); return true; }
  if (msg && msg.kind === 'register') { registerTest().then(sendResponse); return true; }
  if (msg && msg.kind === 'tab-port') {
    // tabs.connect to the content script of a tab: it answers on the port.
    var port = chrome.tabs.connect(msg.tabId, { name: 'from-background' }), answered = false;
    port.onMessage.addListener(function (m) { answered = true; sendResponse({ reply: m }); port.disconnect(); });
    port.onDisconnect.addListener(function () { if (!answered) sendResponse({ error: chrome.runtime.lastError ? chrome.runtime.lastError.message : 'disconnected' }); });
    port.postMessage({ ping: true });
    return true;
  }
});
chrome.alarms.create('tick', { periodInMinutes: 0.05 });
chrome.alarms.onAlarm.addListener(function (a) { chrome.storage.local.get({ ticks: 0 }, function (v) { chrome.storage.local.set({ ticks: v.ticks + 1, lastAlarm: a.name }); }); });
chrome.tabs.onActivated.addListener(function (info) { chrome.storage.local.set({ lastActivated: info.tabId }); });
// Accepted by Krunk and never called: listed on krunk://extensions, nothing else happens.
try { chrome.webRequest.onBeforeRequest.addListener(function () {}, { urls: ['<all_urls>'] }); } catch (e) {}
