// Krunk site data: turns what the engine lists (cookies, stored data per origin) into the short
// answer the app gets. No browser API in here, so it can be tested with plain node
// (android/app/src/test/js/sitedata.test.js).
//
// Which site something is counted for: Krunk keeps the cookies and the storage of third parties
// apart per site the user visits ("partitioned"). A tracker's cookie that was set inside
// news.example belongs to the user's visit of news.example, and clearing news.example removes
// it. So partitioned cookies and partitioned storage are counted for the top-level site they are
// kept under; everything else for its own host.
"use strict";

var KrunkSiteData = (() => {
  const MAX_HOSTS = 5000;
  const MAX_USAGE = 20000;

  // "https://news.example" or "https://news.example:8443" -> "news.example"; "" when odd.
  function hostOfSite(site) {
    if (typeof site !== "string") return "";
    const m = /^[a-z][a-z0-9+.-]*:\/\/(\[[^\]]*\]|[^/:?#]*)/i.exec(site);
    return m ? m[1].toLowerCase() : "";
  }

  // The engine's partition key text: "(scheme,domain[,port][,f])", or the bare domain in data of
  // old versions -> the domain; "" for no partition.
  function hostOfPartitionKey(key) {
    if (typeof key !== "string" || key === "") return "";
    if (!key.startsWith("(")) return key.toLowerCase();
    const parts = key.slice(1, key.endsWith(")") ? -1 : undefined).split(",");
    return (parts[1] || "").toLowerCase();
  }

  // cookies: what browser.cookies.getAll returned -> [[host, count], ...] sorted by host.
  function cookieHosts(cookies) {
    const counts = new Map();
    for (const cookie of Array.isArray(cookies) ? cookies : []) {
      if (!cookie || typeof cookie.domain !== "string") continue;
      const top = cookie.partitionKey ? hostOfSite(cookie.partitionKey.topLevelSite) : "";
      const host = top || cookie.domain.replace(/^\./, "").toLowerCase();
      if (!host) continue;
      if (!counts.has(host) && counts.size >= MAX_HOSTS) continue;
      counts.set(host, (counts.get(host) || 0) + 1);
    }
    return [...counts].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  }

  // items: [{origin, partitionKey, bytes}] from the usage API -> [[origin, topHost, bytes], ...]
  // (topHost "" when the data is the origin's own), one row per origin and top-level site.
  function usageRows(items) {
    const rows = new Map();
    for (const item of Array.isArray(items) ? items : []) {
      if (!item || typeof item.origin !== "string" || !/^https?:\/\//i.test(item.origin)) continue;
      const bytes = Number.isFinite(item.bytes) && item.bytes > 0 ? Math.floor(item.bytes) : 0;
      const top = hostOfPartitionKey(item.partitionKey);
      const key = item.origin + " " + top;
      const row = rows.get(key);
      if (row) {
        row[2] += bytes;
      } else if (rows.size < MAX_USAGE) {
        rows.set(key, [item.origin, top, bytes]);
      }
    }
    return [...rows.values()];
  }

  return { hostOfSite, hostOfPartitionKey, cookieHosts, usageRows };
})();

if (typeof module !== "undefined") module.exports = KrunkSiteData;
