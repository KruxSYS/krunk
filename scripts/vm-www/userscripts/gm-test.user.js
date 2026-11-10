// ==UserScript==
// @name        GM test
// @namespace   krunk-m5
// @version     1.0
// @description Exercises document-start, GM_addStyle, GM_setValue/GM_getValue and GM_notification.
// @match       *://*/*
// @run-at      document-start
// @grant       GM_addStyle
// @grant       GM_setValue
// @grant       GM_getValue
// @grant       GM_notification
// ==/UserScript==
const startState = document.readyState;
GM_addStyle('h1 { color: rgb(200, 0, 0) !important; outline: 3px dashed rgb(200, 0, 0); }');
const n = GM_getValue('count', 0) + 1;
GM_setValue('count', n);
document.addEventListener('DOMContentLoaded', () => {
  document.title = 'GM count ' + n + ' (' + startState + ')';
  fetch('/log?gm-count=' + n + '&start=' + startState).catch(() => {});
});
GM_notification('GM test ran ' + n + ' times');
