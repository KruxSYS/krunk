// 0.26.0 page-load harness: installed with CDP Page.addScriptToEvaluateOnNewDocument in every
// browser under test (Krunk harness builds, Chrome, Edge), so all three are measured the same way.
// It runs before the page's own scripts and before Web Page Replay's deterministic.js (which
// replaces Date), and keeps what load.mjs reads after the load in window.__lp.
(() => {
  if (window !== window.top || window.__lp) return;
  const now = Date.now.bind(Date);
  const lp = (window.__lp = { fcp: null, lcp: null, lcpSize: 0, lcpWhat: '', shows: [], now });
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) if (e.name === 'first-contentful-paint') lp.fcp = e.startTime;
    }).observe({ type: 'paint', buffered: true });
  } catch {}
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) {
        lp.lcp = e.startTime;
        lp.lcpSize = e.size;
        lp.lcpWhat = e.url ? e.url.slice(0, 100) : e.element ? e.element.tagName : '';
      }
    }).observe({ type: 'largest-contentful-paint', buffered: true });
  } catch {}
  // Keys that reach the page (the harness types into the address bar: any here were lost on the
  // way there).
  addEventListener('keydown', () => (lp.keys = (lp.keys || 0) + 1), true);
  // A back/forward-cache restore runs no new document: its time is the pageshow and the first
  // frame after it.
  addEventListener('pageshow', (e) => {
    if (!e.persisted) return;
    const t = performance.now();
    requestAnimationFrame(() => requestAnimationFrame(() => lp.shows.push({ t, frame: performance.now() })));
  });
})();
