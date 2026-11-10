// ==UserScript==
// @name        Needs unsupported things
// @namespace   krunk-m5
// @version     1.0
// @match       https://example.com/*
// @require     https://cdn.example.com/lib.js
// @grant       GM_xmlhttpRequest
// ==/UserScript==
GM_xmlhttpRequest({ method: 'GET', url: 'https://example.com/' });
