// Content script: counts visits in chrome.storage.local, asks the background for a greeting,
// shows a banner, and reports what it sees (isolated world: no page globals).
// 0.17.0: also a port to the background, the declarations of lib/shared.js (the file before
// this one in the same entry), `chrome` as a global of the world, and an answer on a port
// the background opens with tabs.connect. The results are in <meta name=krunk-fixture-checks>.
(function () {
  var pageSeesUs = typeof window.krunkFixtureMarker !== 'undefined';
  // Stages, so a test page can tell how far the script got: start → storage → reply.
  var meta = document.createElement('meta'); meta.name = 'krunk-fixture'; meta.content = 'start'; (document.head || document.documentElement).appendChild(meta);
  var stage = function (s) { meta.content = s; };
  var checks = document.createElement('meta'); checks.name = 'krunk-fixture-checks'; checks.content = ''; (document.head || document.documentElement).appendChild(checks);
  var lines = [];
  var check = function (ok, name, detail) { lines.push((ok ? 'PASS ' : 'FAIL ') + name + (detail ? ': ' + detail : '')); checks.content = lines.join('\n'); };
  check(typeof krunkFixtureShared === 'object' && krunkFixtureShared.answer === 42 && typeof krunkFixtureHelper === 'function', 'content: declarations of the file before it');
  check(typeof globalThis.chrome === 'object' && !!globalThis.chrome && globalThis.chrome.runtime.id === chrome.runtime.id && typeof globalThis.browser === 'object', 'content: chrome is a global of its world');
  check((function () { return this === undefined; })() === false, 'content: not forced into strict mode');
  try {
    var port = chrome.runtime.connect({ name: 'cs' }), got = false;
    port.onMessage.addListener(function (m) {
      if (got) return; got = true;
      var s = m && m.sender || {};
      check(m && m.echo && m.echo.hi === 1 && s.id === chrome.runtime.id && s.tab && typeof s.tab.id === 'number' && s.frameId === 0 && s.url === location.href && s.origin === location.origin,
        'content: port to the background', 'sender ' + JSON.stringify({ url: s.url, origin: s.origin, frameId: s.frameId, tab: s.tab && s.tab.id }));
      port.disconnect();
    });
    port.onDisconnect.addListener(function () { if (!got) check(false, 'content: port to the background', 'disconnected: ' + (chrome.runtime.lastError && chrome.runtime.lastError.message)); });
    port.postMessage({ hi: 1 });
  } catch (e) { check(false, 'content: port to the background', String(e && e.message || e)); }
  chrome.runtime.onConnect.addListener(function (p) {
    p.onMessage.addListener(function (m) { p.postMessage({ pong: m, title: document.title, sender: p.sender }); });
  });
  chrome.storage.local.get({ visits: 0 }, function (v) {
    stage('storage');
    var n = (v.visits || 0) + 1;
    chrome.storage.local.set({ visits: n, last: location.href });
    chrome.runtime.sendMessage({ kind: 'hello', url: location.href, visits: n }, function (reply) {
      var banner = document.createElement('div');
      banner.id = 'krunk-fixture-banner';
      banner.textContent = chrome.i18n.getMessage('banner', [chrome.runtime.getManifest().name, String(n)]) + ' · background says: ' + (reply && reply.text) + ' · pageSeesUs=' + pageSeesUs;
      document.body.appendChild(banner);
      stage('visit ' + n);
    });
  });
  chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
    if (msg && msg.kind === 'ping') { sendResponse({ pong: true, title: document.title }); }
  });
})();
