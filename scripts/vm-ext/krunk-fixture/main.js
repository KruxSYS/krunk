// world: "MAIN", document_start: runs in the page's own world before the page's scripts,
// without any extension API.
window.krunkFixtureMain = {
  world: 'MAIN',
  hasExtensionApi: typeof chrome !== 'undefined' && !!(chrome.runtime && chrome.runtime.id),
  readyState: document.readyState,
  beforePageScripts: typeof window.krunkFixtureMarker === 'undefined'
};
