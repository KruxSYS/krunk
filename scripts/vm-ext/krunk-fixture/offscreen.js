// The offscreen document: has a DOM (the worker has none) and messaging.
var doc = new DOMParser().parseFromString('<p id="x">ok</p>', 'text/html');
chrome.runtime.sendMessage({ kind: 'offscreen-ready', parsed: doc.getElementById('x').textContent });
