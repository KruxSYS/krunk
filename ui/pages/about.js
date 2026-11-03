// About page: versions (the runtime version comes from
// GetAvailableCoreWebView2BrowserVersionString on the host).
(function () {
  'use strict';
  LB.post('ready');
  LB.request('getAbout').then(function (a) {
    document.getElementById('version').textContent = a.version;
    document.getElementById('runtime').textContent = a.runtime;
    document.getElementById('loader').textContent = a.loader;
    document.getElementById('dataDir').textContent = a.dataDir;
    document.getElementById('arguments').textContent = a.arguments;
  });
})();
