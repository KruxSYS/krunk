// Krunk 0.17.0: the script of the dedicated Worker that is an extension's background
// (core::ext_runtime::worker_bootstrap fills in the two marked places; the host serves the
// result next to the service worker's own file, so relative importScripts and bundlers'
// automatic public paths resolve as they do in Chrome).
//
// Order matters: the bridge takes postMessage and addEventListener first, then the chrome.*
// shim (ext-page.js) runs, then the extension's service worker. Hardened extensions (SES,
// LavaMoat) remove or freeze globals when they start; nothing here is looked up after that.
(function (sw) {
  var post = self.postMessage.bind(self), listen = self.addEventListener.bind(self);
  var bridge = {
    // To the host, through the hidden background page (which relays).
    postMessage: function (m) { post({ __krunk: m }); },
    // From the host. The extension's own `message` listeners never see these.
    addEventListener: function (type, fn) {
      listen('message', function (e) {
        if (e.data && e.data.__krunkIn !== undefined) { e.stopImmediatePropagation(); fn({ data: e.data.__krunkIn }); }
      });
    }
  };
  (function (__lbHost) {
/*KRUNK_SHIM*/
  })(bridge);
  // The shim leaves this for us (and for nobody else): called once the script ran or failed.
  var activate = self.__krunkActivate;
  try { delete self.__krunkActivate; } catch (e) { /* not configurable: harmless */ }
  if (typeof activate !== 'function') return;
  if (sw.module) {
    import(sw.script).then(function () { activate(); }, function (e) { activate(e); });
  } else {
    try { importScripts(sw.script); } catch (e) { activate(e); return; }
    activate();
  }
})(/*KRUNK_SW*/null);
