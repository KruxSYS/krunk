// Krunk 0.6.0: `Notification.requestPermission()` asks Krunk (the engine never asks the
// embedder for notifications in Alloy style, so the call would never settle). Krunk's
// prompt writes the answer into the engine's content settings; this shim waits until
// `Notification.permission` leaves "default" (or 30 s pass: dismissed), then resolves.
(function () {
  'use strict';
  if (typeof Notification !== 'function') return;
  var desc = Object.getOwnPropertyDescriptor(Notification, 'permission');
  var read = function () {
    try { return desc && desc.get ? desc.get.call(Notification) : Notification.permission; } catch (e) { return 'default'; }
  };
  var asked = false;
  Notification.requestPermission = function (callback) {
    var p = new Promise(function (resolve) {
      var now = read();
      if (now !== 'default') { resolve(now); return; }
      if (!asked) {
        asked = true;
        try { __lbHost.postMessage({ lb: 'notificationPermission' }); } catch (e) { /* no bridge */ }
      }
      var n = 0;
      var t = setInterval(function () {
        var s = read();
        if (s !== 'default' || ++n > 150) { clearInterval(t); asked = false; resolve(s); }
      }, 200);
    });
    if (typeof callback === 'function') p.then(callback);
    return p;
  };
})();
