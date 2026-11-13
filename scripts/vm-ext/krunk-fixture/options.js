function dump() { chrome.storage.local.get(null, function (all) { document.getElementById('dump').textContent = JSON.stringify(all, null, 2).slice(0, 4000); document.getElementById('greeting').value = all.greeting || ''; }); }
document.getElementById('save').onclick = function () { chrome.storage.local.set({ greeting: document.getElementById('greeting').value }, dump); };
chrome.storage.onChanged.addListener(dump);
dump();

// The self-test (0.17.0): one PASS / FAIL / SKIP line per step, in the page, in the title
// ("fixture self-test: 12 PASS, 0 FAIL") and in chrome.storage.local.selftest.
var lines = [];
function line(kind, name, detail) {
  lines.push(kind + ' ' + name + (detail ? ': ' + detail : ''));
  document.getElementById('selftest').textContent = lines.join('\n');
}
function check(ok, name, detail) { line(ok ? 'PASS' : 'FAIL', name, ok ? '' : detail); }
function text(v) { try { return JSON.stringify(v); } catch (e) { return String(v); } }
function withTimeout(promise, ms, what) {
  return Promise.race([promise, new Promise(function (_, reject) { setTimeout(function () { reject(new Error(what + ': no answer within ' + ms + ' ms')); }, ms); })]);
}
function step(name, fn) {
  return Promise.resolve().then(fn).catch(function (e) { line('FAIL', name, String(e && e.message || e)); });
}
function echoOnce(message) {
  return new Promise(function (resolve, reject) {
    var port = chrome.runtime.connect({ name: 'echo' });
    port.onMessage.addListener(function (m) { resolve({ reply: m, port: port }); });
    port.onDisconnect.addListener(function () { reject(new Error('the port closed: ' + (chrome.runtime.lastError ? chrome.runtime.lastError.message : 'no reason'))); });
    port.postMessage(message);
  });
}
function run() {
  lines = [];
  var own = 'chrome-extension://' + chrome.runtime.id, envBefore = null;
  document.getElementById('summary').textContent = 'running…';
  // What extensions compute themselves and compare senders with (docs/EXTENSIONS.md).
  check(new URL(own + '/x.html').origin === own, 'engine: new URL("chrome-extension://<id>/x").origin is "chrome-extension://<id>"', new URL(own + '/x.html').origin);
  return step('background: a worker with the service worker globals', function () {
    return withTimeout(chrome.runtime.sendMessage({ kind: 'env' }), 5000, 'env').then(function (env) {
      envBefore = env;
      check(env && env.noWindow && env.noDocument, 'background: a worker (no window, no document)', text(env));
      check(env && env.swState === 'activated' && env.clients && env.registration && env.skipWaiting, 'background: self.serviceWorker.state is "activated", clients, registration, skipWaiting', text(env));
      check(env && env.imported === 'imported', 'background: importScripts from a subfolder', text(env && env.imported));
      check(env && env.globalChrome, 'background: chrome is a global of the worker');
      var s = env && env.sender || {};
      check(s.id === chrome.runtime.id && s.origin === own && new URL(s.url).origin === own && s.tab && typeof s.tab.id === 'number', 'message: the sender of an extension page in Chrome\'s form, with its tab', text(s));
    });
  }).then(function () {
    return step('port: extension page to background and back', function () {
      return withTimeout(echoOnce({ n: 1 }), 5000, 'echo').then(function (r) {
        var s = r.reply && r.reply.sender || {};
        check(r.reply && r.reply.echo && r.reply.echo.n === 1, 'port: extension page to background and back', text(r.reply));
        check(s.origin === own && new URL(s.url).origin === own && /\/options\.html/.test(s.url), 'port: sender.url and sender.origin are chrome-extension://<id>', text(s));
        r.port.disconnect();
        return new Promise(function (resolve) { setTimeout(resolve, 300); }).then(function () { return chrome.runtime.sendMessage({ kind: 'env' }); }).then(function (env) {
          check(env.echoDisconnects === envBefore.echoDisconnects + 1, 'port: disconnect() reaches the other end', 'onDisconnect count ' + envBefore.echoDisconnects + ' → ' + env.echoDisconnects);
        });
      });
    });
  }).then(function () {
    return step('storage: session area, onChanged in a tab page, beyond 10 MiB', function () {
      var changed = new Promise(function (resolve) { var l = function (c, area) { if (area === 'session' && c.selfKey) { chrome.storage.onChanged.removeListener(l); resolve(c.selfKey.newValue); } }; chrome.storage.onChanged.addListener(l); });
      return chrome.storage.session.set({ selfKey: 'v1' }).then(function () { return withTimeout(changed, 3000, 'storage.onChanged'); }).then(function (v) {
        check(v === 'v1', 'storage: session area and onChanged in an extension page in a tab', text(v));
        var big = new Array(11 * 1024 * 1024 + 1).join('x');
        return chrome.storage.local.set({ selfBig: big }).then(function () { return chrome.storage.local.getBytesInUse('selfBig'); }).then(function (n) {
          check(n > 10 * 1024 * 1024, 'storage: unlimitedStorage (11 MiB in one value)', String(n));
          return chrome.storage.local.remove('selfBig');
        });
      });
    });
  }).then(function () {
    return step('offscreen document', function () {
      return withTimeout(chrome.runtime.sendMessage({ kind: 'offscreen' }), 10000, 'offscreen').then(function (r) {
        check(r && r.parsed === 'ok' && r.has === true && r.hasAfterClose === false, 'offscreen: createDocument, a message from it, hasDocument, closeDocument', text(r));
      });
    });
  }).then(function () {
    return step('popup window', function () {
      return withTimeout(chrome.runtime.sendMessage({ kind: 'window' }), 12000, 'window').then(function (r) {
        check(r && r.created && r.created.type === 'popup' && r.hello && r.hello.windowId === r.created.id && r.hello.type === 'popup' && r.listed && r.removed && !r.error,
          'windows: create({type:"popup"}), a port from its page, getAll, window.close(), onRemoved', text(r));
      });
    });
  }).then(function () {
    return step('registerContentScripts', function () {
      return withTimeout(chrome.runtime.sendMessage({ kind: 'register' }), 5000, 'register').then(function (r) {
        check(r && r.ids && r.ids.indexOf('dyn') >= 0 && /Duplicate script ID/.test(r.duplicate), 'scripting: registerContentScripts, getRegisteredContentScripts, a duplicate id is refused', text(r));
      });
    });
  }).then(function () {
    return step('small APIs', function () {
      return Promise.all([chrome.management.getSelf(), chrome.permissions.contains({ permissions: ['storage'] }), chrome.i18n.getAcceptLanguages(), chrome.runtime.getContexts({ contextTypes: ['TAB'] }), chrome.notifications.create('self', { type: 'basic', title: 't', message: 'm', iconUrl: 'icons/48.png' }), chrome.cookies ? chrome.cookies.getAll({}) : []]).then(function (r) {
        check(r[0] && r[0].id === chrome.runtime.id && r[1] === true && Array.isArray(r[2]) && r[2].length > 0 && Array.isArray(r[3]) && r[3].some(function (c) { return /options\.html/.test(c.documentUrl || ''); }) && r[4] === 'self' && Array.isArray(r[5]),
          'management.getSelf, permissions.contains, i18n.getAcceptLanguages, runtime.getContexts, notifications.create, cookies.getAll', text(r).slice(0, 600));
        check(typeof chrome.sidePanel === 'undefined' && typeof chrome.identity === 'undefined', 'sidePanel and identity are absent');
      });
    });
  }).then(function () {
    return chrome.tabs.query({ url: ['http://localhost/*', 'http://127.0.0.1/*'] }).then(function (tabs) {
      var tab = tabs.filter(function (t) { return /\/ext\.html/.test(t.url || ''); })[0];
      if (!tab) { line('SKIP', 'the steps in a web page', 'no tab on http://localhost:8094/ext.html'); return; }
      return step('executeScript', function () {
        return withTimeout(chrome.scripting.executeScript({ target: { tabId: tab.id }, func: function (a, b) { return { sum: a + b, marker: typeof window.krunkFixtureMarker, api: typeof chrome.runtime.id }; }, args: [2, 3] }), 5000, 'executeScript').then(function (r) {
          var v = r && r[0] && r[0].result;
          check(v && v.sum === 5 && v.marker === 'undefined' && v.api === 'string', 'scripting.executeScript: func with args, its result, in the isolated world', text(r));
          return withTimeout(chrome.scripting.executeScript({ target: { tabId: tab.id }, world: 'MAIN', func: function () { return { marker: window.krunkFixtureMarker, main: window.krunkFixtureMain }; } }), 5000, 'executeScript MAIN');
        }).then(function (r) {
          var v = r && r[0] && r[0].result;
          check(v && v.marker === 'page global', 'scripting.executeScript: world MAIN sees the page\'s globals', text(r));
          check(v && v.main && v.main.world === 'MAIN' && v.main.hasExtensionApi === false && v.main.beforePageScripts === true, 'content script in the MAIN world at document_start, without an extension API', text(v && v.main));
        });
      }).then(function () {
        return step('tabs.connect', function () {
          return withTimeout(chrome.runtime.sendMessage({ kind: 'tab-port', tabId: tab.id }), 5000, 'tabs.connect').then(function (r) {
            var s = r && r.reply && r.reply.sender || {};
            check(r && r.reply && r.reply.pong && r.reply.pong.ping === true && s.id === chrome.runtime.id && new URL(s.url || 'x:').origin === own, 'tabs.connect: background to the content script and back', text(r));
          });
        });
      }).then(function () {
        return step('content script checks', function () {
          return withTimeout(chrome.scripting.executeScript({ target: { tabId: tab.id }, func: function () { var m = document.querySelector('meta[name=krunk-fixture-checks]'), d = document.querySelector('meta[name=krunk-fixture-dyn]'); return { checks: m ? m.content : '', dyn: d ? d.content : '' }; } }), 5000, 'checks').then(function (r) {
            var v = r && r[0] && r[0].result || {};
            String(v.checks || '').split('\n').filter(Boolean).forEach(function (l) { lines.push(l); });
            document.getElementById('selftest').textContent = lines.join('\n');
            check(/PASS content: port to the background/.test(v.checks || ''), 'content script: its checks ran (4 lines above)', text(v.checks));
            if (v.dyn) line('PASS', 'scripting: the registered content script ran in the page');
            else line('SKIP', 'scripting: the registered content script in the page', 'reload ext.html after the first run, then run again');
          });
        });
      });
    });
  }).then(function () {
    var pass = lines.filter(function (l) { return l.indexOf('PASS') === 0; }).length, fail = lines.filter(function (l) { return l.indexOf('FAIL') === 0; }).length;
    var summary = 'fixture self-test: ' + pass + ' PASS, ' + fail + ' FAIL';
    document.getElementById('summary').textContent = summary;
    document.title = summary;
    return chrome.storage.local.set({ selftest: lines });
  });
}
document.getElementById('run').onclick = run;
// Three errors in the background (console.error, uncaught, unhandled rejection) and one here:
// they must show in the extension's Errors list on krunk://extensions and in krunk.log.
document.getElementById('errors').onclick = function () {
  chrome.runtime.sendMessage({ kind: 'errors' }).then(function (r) { document.getElementById('summary').textContent = 'made ' + ((r && r.made || 0) + 1) + ' errors: see krunk://extensions'; });
  console.error('fixture: console.error from the options page');
};
if (location.hash === '#selftest') run();
