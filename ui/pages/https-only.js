// HTTPS-only interstitial: the http:// URL is in ?url=. "Continue" asks the
// host to allow that host over HTTP for this session and load it once.
(function () {
  'use strict';
  const target = new URLSearchParams(location.search).get('url') || '';
  let host = '';
  try {
    const u = new URL(target);
    if (u.protocol === 'http:') host = u.hostname;
  } catch (e) { /* invalid */ }
  document.getElementById('host').textContent = host || '(invalid address)';
  document.getElementById('url').textContent = target;
  document.getElementById('continue').disabled = !host;
  document.getElementById('continue').addEventListener('click', function () {
    LB.post('httpsOnlyProceed', { url: target });
  });
  document.getElementById('back').addEventListener('click', function () { history.back(); });
  LB.post('ready');
})();
