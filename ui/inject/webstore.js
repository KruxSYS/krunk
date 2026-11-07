// Krunk 0.7.0 (R20): the Chrome Web Store's detail pages say "Add to Krunk". The store's own
// button (whatever it says: Add to Chrome, Switch to Chrome, Get Chrome) is relabelled and its
// click goes to Krunk, which takes the extension id from the tab's address, downloads the
// package itself, checks its signatures and asks before installing. Main frame of
// chromewebstore.google.com only; nothing is exposed to the page.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
var host = __lbHost;
var LABEL = /^\s*(add to chrome|switch to chrome|get chrome|add to (your )?browser|install|remove from chrome|checking…)\s*$/i;
function isDetail() { return /\/detail\/[^/]+\/[a-p]{32}/.test(location.pathname) || /\/detail\/[a-p]{32}/.test(location.pathname); }
function relabel(root) {
  if (!isDetail()) return;
  var buttons = (root || document).querySelectorAll('button, a[role="button"]');
  for (var i = 0; i < buttons.length; i++) {
    var b = buttons[i];
    if (b.dataset.krunk) continue;
    var spans = b.querySelectorAll('span');
    var textNode = null;
    for (var j = 0; j < spans.length; j++) if (LABEL.test(spans[j].textContent) && !spans[j].children.length) textNode = spans[j];
    if (!textNode && LABEL.test(b.textContent)) textNode = b;
    if (!textNode) continue;
    b.dataset.krunk = '1';
    textNode.textContent = 'Add to Krunk';
    b.disabled = false;
    b.removeAttribute('disabled');
    b.removeAttribute('aria-disabled');
  }
}
// The click: capture phase, before the store's own handler.
document.addEventListener('click', function (e) {
  var b = e.target && e.target.closest && e.target.closest('[data-krunk]');
  if (!b) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  host.postMessage({ lb: 'extStoreClick' });
  b.querySelectorAll('span').forEach(function (s) { if (s.textContent === 'Add to Krunk') s.textContent = 'Checking…'; });
  if (b.textContent === 'Add to Krunk') b.textContent = 'Checking…';
  setTimeout(function () { delete b.dataset.krunk; relabel(); }, 4000);
}, true);
// What Krunk did (shown under the button for a while).
host.addEventListener('message', function (m) {
  var d = m && m.data;
  if (!d || d.event !== 'krunkStore' || typeof d.text !== 'string') return;
  var n = document.getElementById('krunk-store-notice') || document.createElement('div');
  n.id = 'krunk-store-notice';
  n.setAttribute('role', 'status');
  n.style.cssText = 'position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:2147483647;padding:10px 16px;border-radius:8px;background:#1f2937;color:#fff;font:14px system-ui,sans-serif;box-shadow:0 4px 16px rgba(0,0,0,.3)';
  n.textContent = d.text;
  (document.body || document.documentElement).appendChild(n);
  setTimeout(function () { n.remove(); }, 6000);
});
// The store is a single-page app: its pages come and go without loads. Observed only here.
var pending = false;
new MutationObserver(function () {
  if (pending) return;
  pending = true;
  setTimeout(function () { pending = false; relabel(); }, 200);
}).observe(document, { childList: true, subtree: true });
if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', function () { relabel(); });
else relabel();
