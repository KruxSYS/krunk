// Do Not Track: navigator.doNotTrack = "1" to match the DNT: 1 header.
(function () {
  'use strict';
  try {
    Object.defineProperty(Navigator.prototype, 'doNotTrack', {
      get: function () { return '1'; }, configurable: true, enumerable: true,
    });
  } catch (e) { /* ignore */ }
})();
