// Krunk 0.7.0 (R12): with "Send client hints" off (or a custom user agent without hints) the
// engine has no user-agent metadata and sends no Sec-CH-UA* headers, yet navigator.userAgentData
// would still be there, empty. It goes, so the page sees what the requests say. Every frame;
// with the engine's metadata nothing changes. (Evaluated at the page's top level: nothing may
// be declared outside the function.)
(function () {
  'use strict';
  try {
    var d = Object.getOwnPropertyDescriptor(Navigator.prototype, 'userAgentData');
    var v = d && d.get ? d.get.call(navigator) : null;
    if (v && v.brands && v.brands.length === 0 && !v.platform) delete Navigator.prototype.userAgentData;
  } catch (e) { /* not a secure context, or no such property */ }
})();
