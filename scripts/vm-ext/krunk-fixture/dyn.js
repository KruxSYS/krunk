// Registered at runtime with chrome.scripting.registerContentScripts (id "dyn").
(function () {
  var meta = document.createElement('meta'); meta.name = 'krunk-fixture-dyn'; meta.content = 'registered script ran';
  (document.head || document.documentElement).appendChild(meta);
})();
