chrome.tabs.query({ active: true, currentWindow: true }, function (tabs) {
  var t = tabs[0];
  document.getElementById('tab').textContent = t ? (t.title || '(no title)') + ' — ' + (t.url || '(url hidden)') : 'none';
  document.getElementById('ping').onclick = function () {
    chrome.tabs.sendMessage(t.id, { kind: 'ping' }, function (r) {
      document.getElementById('out').textContent = r ? 'content script answered: ' + JSON.stringify(r) : 'no content script there (' + (chrome.runtime.lastError ? chrome.runtime.lastError.message : '') + ')';
    });
  };
});
chrome.storage.local.get(['visits', 'messages', 'ticks'], function (v) {
  document.getElementById('visits').textContent = v.visits || 0;
  document.getElementById('messages').textContent = v.messages || 0;
  document.getElementById('ticks').textContent = v.ticks || 0;
});
document.getElementById('options').onclick = function () { chrome.runtime.openOptionsPage(); };
