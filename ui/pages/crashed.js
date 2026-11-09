// krunk://crashed?url=… (0.11.0): the page a tab showed has ended (it stopped responding and
// the user closed it, or its renderer crashed). Reload loads its address again.
(function () {
  'use strict';
  const target = new URLSearchParams(location.search).get('url') || '';
  let ok = false;
  try { const u = new URL(target); ok = u.protocol === 'https:' || u.protocol === 'http:'; } catch (e) { ok = false; }
  document.getElementById('where').textContent = ok ? new URL(target).host : '';
  const reload = document.getElementById('reload');
  reload.disabled = !ok;
  reload.addEventListener('click', function () { if (ok) location.replace(target); });
  reload.focus();
  LB.post('ready');
})();
