chrome.runtime.onInstalled.addListener(()=>{chrome.storage.local.set({installed:Date.now()})});
chrome.runtime.onMessage.addListener((m,s,reply)=>{chrome.tabs.query({},t=>{reply({got:m,tabs:t.length,sender:s.tab&&s.tab.id})});return true});
