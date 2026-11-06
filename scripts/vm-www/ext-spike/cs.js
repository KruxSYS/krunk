const b=document.createElement('div');b.textContent='SPIKE EXTENSION CONTENT SCRIPT RUNNING on '+location.host;
b.style.cssText='position:fixed;top:0;left:0;right:0;background:#e3b;color:#fff;font:bold 20px sans-serif;padding:10px;z-index:2147483647';
document.documentElement.appendChild(b);
try{chrome.runtime.sendMessage({hello:location.href},r=>{b.textContent+=' | bg says: '+JSON.stringify(r)+' | '+(chrome.runtime.lastError?chrome.runtime.lastError.message:'')});}catch(e){b.textContent+=' | sendMessage error '+e}
