// Global Privacy Control signal (https://globalprivacycontrol.org/).
// Injected into every frame at document creation when GPC is enabled; the
// matching Sec-GPC: 1 request header is added by the host.
(function () {
  'use strict';
  try {
    Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', {
      get: function () { return true; }, configurable: true, enumerable: true,
    });
  } catch (e) { /* already defined */ }
})();
