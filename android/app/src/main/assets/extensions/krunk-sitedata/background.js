// Krunk site data: the background script. It answers the app over one native-messaging port
// (site data, and the certificate exceptions further down), and does nothing until asked.
//
// App -> here:  {t:"list", id}
//               {t:"hello"}                       the app's answer to "ready" (see below)
//               {t:"videos", id, url}             the <video> elements of the tab showing url (Cast, 0.15.0)
// Here -> app:  {t:"ready"}                       after every connection
//               {t:"connected"}                   the app answered "ready" on this connection
//               {t:"list", id, cookies, usage, error}
//                 cookies: JSON text of [[host, count], ...], or "" when the cookies could not be listed
//                 usage:   JSON text of [[origin, topHost, bytes], ...], or "" when sizes are not available
//               {t:"videos", id, ok, result, error}
//                 result: JSON text of [{src, type, title, poster, time, duration, mse, paused, area}, ...]
//
// The connection: GeckoView hands a native-app connection to the app only when the app's message
// delegate exists at the moment the connection is made. This script starts while the extension is
// being installed, which can be before the app is listening: the port is then never delivered to
// the app, and the app has no way to ask again. So a connection is only trusted once the app has
// answered "ready" with {t:"hello"}; until then this script reconnects (a connection made later,
// when the app is listening, is delivered at once).
//
// Cookies: browser.cookies.getAll with firstPartyDomain null (no filter on first-party isolation)
// and partitionKey {} (cookies kept apart per site as well as ordinary ones): all cookies of the
// normal cookie store. Cookies of private tabs are in another store, which this extension may
// not read (it is not allowed in private browsing) and does not ask for.
"use strict";

const CONFIRM_MS = 1000;
const RETRY_MS = 400;
const RETRY_MAX_MS = 3000;
let port = null;
let confirmed = false;
let retryMs = RETRY_MS;
let retryTimer = 0;

const send = message => {
  if (!port) return;
  try {
    port.postMessage(message);
  } catch (e) {
    // the app is gone
  }
};

function scheduleRetry() {
  if (retryTimer) return;
  retryTimer = setTimeout(() => {
    retryTimer = 0;
    connect();
    retryMs = Math.min(retryMs * 2, RETRY_MAX_MS);
  }, retryMs);
}

function connect() {
  let p;
  try {
    p = browser.runtime.connectNative("krunk");
  } catch (e) {
    scheduleRetry();
    return;
  }
  port = p;
  confirmed = false;
  let closing = false;
  const drop = () => {
    if (closing) return;
    closing = true;
    if (port === p) port = null;
    scheduleRetry();
  };
  p.onDisconnect.addListener(drop);
  p.onMessage.addListener(m => {
    // Any answer from the app means it holds this port: it says "hello" on every connection.
    confirmed = true;
    retryMs = RETRY_MS;
    onAppMessage(m);
  });
  send({ t: "ready" });
  // Not answered: the app was not listening when this connection was made. Start over.
  setTimeout(() => {
    if (confirmed || port !== p) return;
    try {
      p.disconnect();
    } catch (e) {
      // already gone
    }
    drop();
  }, CONFIRM_MS);
}

async function list(id) {
  let cookies = null;
  let usage = null;
  const errors = [];
  try {
    cookies = KrunkSiteData.cookieHosts(await browser.cookies.getAll({ firstPartyDomain: null, partitionKey: {} }));
  } catch (e) {
    errors.push("cookies: " + String(e));
  }
  try {
    usage = KrunkSiteData.usageRows(JSON.parse(await browser.krunkSiteData.usage()));
  } catch (e) {
    errors.push("usage: " + String(e));
  }
  // As JSON text: GeckoView's message bridge refuses arrays that mix strings and numbers
  // ("Invalid event data property"), so the rows travel as one string each.
  send({
    t: "list",
    id,
    cookies: cookies === null ? "" : JSON.stringify(cookies),
    usage: usage === null ? "" : JSON.stringify(usage),
    error: errors.join("; ").slice(0, 300),
  });
}

// Certificate exceptions (certs-api.js). Every call answers {t:"certs", id, ok, result, error}.
//   {t:"cert-add", id, host, port, der, temporary, private}  result: the SHA-256 fingerprint
//   {t:"cert-remove", id, host, port}
//   {t:"cert-list", id}                                      result: JSON text [{host, port, sha256}]
async function certs(m) {
  try {
    let result = "";
    if (m.t === "cert-add") {
      result = await browser.krunkCerts.add(m.host, m.port, m.der, m.temporary === true, m.private === true);
    } else if (m.t === "cert-remove") {
      await browser.krunkCerts.remove(m.host, m.port);
    } else {
      result = await browser.krunkCerts.list();
    }
    send({ t: "certs", id: m.id, ok: true, result: String(result || ""), error: "" });
  } catch (e) {
    send({ t: "certs", id: m.id, ok: false, result: "", error: String(e).slice(0, 300) });
  }
}

// Cast (0.15.0): the videos of one tab. The app marks the tab it means as active just before it
// asks (GeckoView's setTabActive), so the tab is the one showing m.url, the active one first. The
// probe runs in every frame of that tab (tabs.executeScript with the <all_urls> host permission
// this extension already has) and only reads properties of the <video> elements; it changes
// nothing on the page.
function probeVideos() {
  const out = [];
  let pageTitle = document.title;
  try {
    pageTitle = window.top.document.title;
  } catch (e) {
    // a cross-origin frame: its own title
  }
  for (const v of document.querySelectorAll("video")) {
    if (out.length >= 20) break;
    const src = String(v.currentSrc || v.src || "");
    let type = "";
    for (const s of v.querySelectorAll("source")) {
      if (s.src === src && s.type) {
        type = s.type;
        break;
      }
    }
    const r = v.getBoundingClientRect();
    const d = v.duration;
    out.push({
      src: src.slice(0, 4096),
      type: String(type).slice(0, 100),
      title: String(v.title || v.getAttribute("aria-label") || pageTitle || "").slice(0, 300),
      poster: String(v.poster || "").slice(0, 2048),
      time: Number.isFinite(v.currentTime) ? v.currentTime : 0,
      duration: d === Infinity ? -1 : Number.isFinite(d) ? d : 0,
      mse: /^blob:/i.test(src) || !!v.srcObject,
      paused: !!v.paused,
      area: Math.max(0, Math.round(r.width * r.height)),
    });
  }
  return out;
}

async function videos(m) {
  try {
    const tabs = await browser.tabs.query({});
    const same = tabs.filter(t => t.url === m.url);
    const tab = same.find(t => t.active) || same[0] || tabs.find(t => t.active);
    if (!tab) throw new Error("no tab shows that page");
    const frames = await browser.tabs.executeScript(tab.id, {
      code: "(" + probeVideos.toString() + ")()",
      allFrames: true,
    });
    const all = [];
    for (const list of frames || []) {
      if (Array.isArray(list)) all.push(...list);
    }
    send({ t: "videos", id: m.id, ok: true, result: JSON.stringify(all.slice(0, 50)), error: "" });
  } catch (e) {
    send({ t: "videos", id: m.id, ok: false, result: "", error: String(e).slice(0, 300) });
  }
}

function onAppMessage(m) {
  if (!m) return;
  // The app's answer to "ready": this connection is the one it holds.
  if (m.t === "hello") send({ t: "connected" });
  else if (m.t === "list") list(m.id);
  else if (m.t === "videos") videos(m);
  else if (m.t === "cert-add" || m.t === "cert-remove" || m.t === "cert-list") certs(m);
}

connect();
