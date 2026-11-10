// 0.11.7 audit fixture (C4): from its own content world, tries to act as the other test
// extension (krunk-fixture). Results go to the test server's access.log as /log?c4-...
(function () {
  var FIXTURE = 'pbjhgalngnghegmojbiidgmjhfibfjhn'; // the fixture zip's id (hash of its manifest name)
  var report = function (k, v) { fetch('/log?' + k + '=' + encodeURIComponent(JSON.stringify(v))).catch(function () {}); };
  // 1. The other extension's binding and deliveries are not in this world.
  report('c4-other-binding', typeof window['__krunkExt_' + FIXTURE]);
  report('c4-shared-binding', typeof window.__krunkExt);
  // 2. A forged call through our own binding naming the other extension.
  chrome.storage.local.set({ attackerOwn: 1 }, function () {
    var deliver = window.__krunkExtDeliver;
    window.__krunkExtDeliver = function (json) {
      try { var m = JSON.parse(json); if (m.reply === 424242) { report('c4-forged-reply', { ext: m.ext, keys: Object.keys(m.data || {}), error: m.error || null }); return; } } catch (e) {}
      return deliver(json);
    };
    window['__krunkExt_' + chrome.runtime.id](JSON.stringify({ ext: FIXTURE, id: 424242, cmd: 'storage.get', data: { area: 'local', keys: null } }));
  });
  // 3. The legitimate path: our own storage and our background.
  chrome.storage.local.get(null, function (v) { report('c4-own-storage', Object.keys(v)); });
  // Only on ext.html: the background then opens krunk://settings (C1).
  if (location.pathname === '/ext.html') chrome.runtime.sendMessage({ kind: 'c1' }, function (r) { report('c4-own-background', r || null); });
})();
