// Krunk 0.7.0 (R7): tells the browser when the top-level document has painted its first content,
// so a tab switch that kept the old page on screen while this tab's new view was still blank
// (a discarded tab coming back, a tab opened in the background) can show it now. Main frame of
// web pages only; once per document; no global.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
if (window.top !== window) return;
if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
if (/(^|\.)krunk\.invalid$/.test(location.hostname)) return;   // Krunk's own pages say `ready`
var host = __lbHost, sent = false;
function done() {
  if (sent) return;
  sent = true;
  // One more frame, so the paint has reached the screen.
  requestAnimationFrame(function () { setTimeout(function () { host.postMessage({ lb: 'painted' }); }, 0); });
}
try {
  new PerformanceObserver(function (list) {
    if (list.getEntries().some(function (e) { return e.name === 'first-contentful-paint'; })) done();
  }).observe({ type: 'paint', buffered: true });
} catch (e) { /* no paint timing: the load below */ }
addEventListener('load', done, { once: true });
// 0.25.0: shown again after being hidden from the engine (a tab switch to it): two animation
// frames later its page is drawn, and a switch waiting for it can bring it on top. Cheaper and
// earlier than the browser asking through DevTools.
document.addEventListener('visibilitychange', function () {
  if (document.visibilityState !== 'visible') return;
  requestAnimationFrame(function () { requestAnimationFrame(function () { host.postMessage({ lb: 'shown' }); }); });
});
