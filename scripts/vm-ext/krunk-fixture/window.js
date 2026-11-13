// A popup-type window of the extension: says hello to the background over a port, with the
// window it is in, and closes itself when told to.
chrome.windows.getCurrent(function (win) {
  var port = chrome.runtime.connect({ name: 'window' });
  port.onMessage.addListener(function (m) { if (m && m.close) { document.getElementById('out').textContent = 'closing'; window.close(); } });
  port.postMessage({ windowId: win && win.id, type: win && win.type, href: location.href });
  document.getElementById('out').textContent = 'window ' + (win && win.id) + ' (' + (win && win.type) + ')';
});
