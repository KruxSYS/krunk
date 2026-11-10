// Agent tools fixture (0.12.0 M5 flow i): every event the agent's tools cause goes to the test
// server's access log as /log?tools=<what>, so the VM run can check what really arrived.
(function () {
  'use strict';
  function log(what) { fetch('/log?tools=' + encodeURIComponent(what)).catch(function () {}); }
  document.getElementById('f').addEventListener('submit', function (e) { e.preventDefault(); log('submit'); });
  document.getElementById('btn').addEventListener('click', function (e) {
    log('click-btn trusted=' + e.isTrusted);
    document.getElementById('btn').textContent = 'Pressed';
  });
  document.getElementById('target').addEventListener('click', function (e) {
    log('click-target x=' + e.clientX + ' y=' + e.clientY + ' trusted=' + e.isTrusted);
    document.getElementById('target').textContent = 'clicked';
  });
  ['q', 'name', 'color', 'pw', 'cc'].forEach(function (id) {
    const el = document.getElementById(id);
    let inputs = 0, keys = 0;
    el.addEventListener('keydown', function () { keys++; });
    el.addEventListener('input', function () { inputs++; });
    el.addEventListener('change', function () {
      const v = (id === 'pw' || id === 'cc') ? (el.value ? 'set' : 'empty') : el.value;
      log('change ' + id + ' value=' + v + ' inputs=' + inputs + ' keydowns=' + keys);
    });
    el.addEventListener('blur', function () { if (id === 'q') log('blur q value=' + el.value + ' keydowns=' + keys + ' inputs=' + inputs); });
  });
  let scrolled = false;
  window.addEventListener('scroll', function () {
    if (!scrolled && window.scrollY > 1000) { scrolled = true; log('scrolled y=' + Math.round(window.scrollY)); }
  });
  // A late element for wait_for.
  setTimeout(function () {
    const p = document.createElement('p');
    p.id = 'late';
    p.textContent = 'I arrived late.';
    document.body.insertBefore(p, document.getElementById('spacer'));
    log('late-added');
  }, 9000);
  // Media: the mp4 is fetched (a resource-timing entry) and played from a blob: URL.
  fetch('test-h264-aac.mp4').then(function (r) { return r.blob(); }).then(function (b) {
    const v = document.getElementById('v');
    v.src = URL.createObjectURL(b);
    log('video-src ' + v.src.slice(0, 5));
  }).catch(function (e) { log('video-error ' + e); });
  // The page's title changes when edit_page's CSS lands (the outline colour is read back).
  setInterval(function () {
    const o = getComputedStyle(document.getElementById('btn')).outlineColor;
    if (o === 'rgb(0, 128, 0)' && !window.__cssSeen) { window.__cssSeen = true; log('css-injected'); }
  }, 300);
  log('loaded ' + location.search);
})();
