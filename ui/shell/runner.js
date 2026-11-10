// The macro runner (0.12.0, WP-F): the async API a macro's code gets. The host starts a macro
// with DevTools `Runtime.evaluate` (`window.__krunkRunMacro(source)`), so the page's CSP needs
// no eval. Every API call is a request the host carries out.
(function () {
  'use strict';
  LB.post('ready');
})();
