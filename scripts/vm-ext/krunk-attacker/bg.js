// 0.11.7 audit fixture (C1): asked by the content script, opens krunk://settings and tries to
// run code in it; then runs code in the web tab that asked (the legitimate path).
var report = function (k, v) { fetch('http://localhost:8094/log?' + k + '=' + encodeURIComponent(JSON.stringify(v))).catch(function () {}); };
chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
  if (!msg || msg.kind !== 'c1') return;
  sendResponse({ ok: true, from: 'attacker background' });
  var webTab = sender && sender.tab ? sender.tab.id : null;
  chrome.tabs.create({ url: 'https://krunk.invalid/settings.html' }, function (tab) {
    setTimeout(function () {
      chrome.scripting.executeScript({ target: { tabId: tab.id }, func: function () { return document.title; } })
        .then(function (r) { report('c1-internal', { ran: true, r: r }); }, function (e) { report('c1-internal', { ran: false, error: String(e && e.message || e) }); });
      if (webTab !== null) {
        chrome.scripting.executeScript({ target: { tabId: webTab }, func: function () { fetch('/log?c1-web-ran=1'); } })
          .then(function () { report('c1-web', 'ok'); }, function (e) { report('c1-web', String(e && e.message || e)); });
      }
    }, 2500);
  });
  return false;
});
