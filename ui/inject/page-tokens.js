// Generic cosmetic filtering, step 1: report the class names and ids used
// on the page (top-level document only) so the host can return matching
// "##.ad-banner" style rules. Capped to keep it cheap.
(function () {
  'use strict';
  if (window.top !== window || !window.__lbHost) return;
  if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
  // Never on Krunk's own pages (they are not web content).
  if (/(^|\.)krunk\.invalid$/.test(location.hostname)) return;
  var MAX = 2000;
  function collect() {
    var classes = new Set();
    var ids = new Set();
    var nodes = document.querySelectorAll('[class],[id]');
    for (var i = 0; i < nodes.length && classes.size + ids.size < MAX; i++) {
      var n = nodes[i];
      if (n.id && n.id.length <= 100) ids.add(n.id);
      var cl = n.classList;
      for (var j = 0; cl && j < cl.length && classes.size + ids.size < MAX; j++) {
        if (cl[j].length <= 100) classes.add(cl[j]);
      }
    }
    if (classes.size || ids.size) {
      window.__lbHost.postMessage({ lb: 'pageTokens', classes: Array.from(classes), ids: Array.from(ids) });
    }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', collect, { once: true });
  else collect();
})();
