// About page: versions (the engine versions are read from the loaded
// libcef.dll at runtime by the host).
(function () {
  'use strict';
  LB.post('ready');
  LB.request('getAbout').then(function (a) {
    document.getElementById('version').textContent = a.version;
    document.getElementById('chromium').textContent = a.chromium;
    document.getElementById('cef').textContent = a.cef;
    document.getElementById('dataDir').textContent = a.dataDir;
    document.getElementById('arguments').textContent = a.arguments;
    // Whether the installed files are this release's (0.7.0); krunk://gpu has the details and
    // the Repair button.
    var i = a.integrity || {};
    document.getElementById('integrity').textContent = !i.checked ? 'checked a few seconds after start (see krunk://gpu)'
      : i.healthy ? i.summary + ' (libcef.dll ' + i.libcef + '…)' : i.summary + ' (krunk://gpu has a Repair button)';
  });
})();
