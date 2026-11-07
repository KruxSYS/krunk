// Background (service worker in Chrome; a hidden page in Krunk): answers content scripts,
// counts messages, keeps an alarm, reacts to the toolbar click when there is no popup.
var messages = 0;
chrome.runtime.onInstalled.addListener(function (d) { chrome.storage.local.set({ installedReason: d.reason }); });
chrome.runtime.onMessage.addListener(function (msg, sender, sendResponse) {
  messages++;
  if (msg && msg.kind === 'hello') {
    chrome.storage.local.set({ messages: messages, lastSender: sender && sender.tab ? sender.tab.id : null });
    sendResponse({ text: 'hi tab ' + (sender && sender.tab ? sender.tab.id : '?') + ' (message #' + messages + ')' });
    chrome.action.setBadgeText({ text: String(messages) });
    return true;
  }
  if (msg && msg.kind === 'tabs') {
    chrome.tabs.query({}, function (tabs) { sendResponse({ count: tabs.length, active: tabs.filter(function (t) { return t.active; }).map(function (t) { return t.url; }) }); });
    return true;
  }
});
chrome.alarms.create('tick', { periodInMinutes: 0.05 });
chrome.alarms.onAlarm.addListener(function (a) { chrome.storage.local.get({ ticks: 0 }, function (v) { chrome.storage.local.set({ ticks: v.ticks + 1, lastAlarm: a.name }); }); });
chrome.tabs.onActivated.addListener(function (info) { chrome.storage.local.set({ lastActivated: info.tabId }); });
// Not provided by Krunk: listed on krunk://extensions and in the log, nothing else happens.
try { chrome.webRequest.onBeforeRequest.addListener(function () {}, { urls: ['<all_urls>'] }); } catch (e) {}
