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
  });
})();
