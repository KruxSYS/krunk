// New tab page (tiles are filled in by milestone 4).
(function () {
  'use strict';
  document.getElementById('search').addEventListener('submit', function (e) {
    e.preventDefault();
    const q = document.getElementById('q').value.trim();
    if (q) LB.post('navigate', { input: q });
  });
  LB.post('ready');
})();
