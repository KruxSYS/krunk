function dump() { chrome.storage.local.get(null, function (all) { document.getElementById('dump').textContent = JSON.stringify(all, null, 2); document.getElementById('greeting').value = all.greeting || ''; }); }
document.getElementById('save').onclick = function () { chrome.storage.local.set({ greeting: document.getElementById('greeting').value }, dump); };
chrome.storage.onChanged.addListener(dump);
dump();
