// Content script: counts visits in chrome.storage.local, asks the background for a greeting,
// shows a banner, and reports what it sees (isolated world: no page globals).
(function () {
  var pageSeesUs = typeof window.krunkFixtureMarker !== 'undefined';
  // Stages, so a test page can tell how far the script got: start → storage → reply.
  var meta = document.createElement('meta'); meta.name = 'krunk-fixture'; meta.content = 'start'; (document.head || document.documentElement).appendChild(meta);
  var stage = function (s) { meta.content = s; };
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
