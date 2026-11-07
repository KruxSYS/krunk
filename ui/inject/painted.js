// Krunk 0.7.0 (R7): tells the browser when the top-level document has painted its first content,
// so a tab switch that kept the old page on screen while this tab's new view was still blank
// (a discarded tab coming back, a tab opened in the background) can show it now. Main frame of
// web pages only; once per document; no global.
'use strict';
if (typeof __lbHost !== 'object' || !__lbHost) return;
if (window.top !== window) return;
if (location.protocol !== 'https:' && location.protocol !== 'http:') return;
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
