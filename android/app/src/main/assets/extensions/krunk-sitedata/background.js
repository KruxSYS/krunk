// Krunk site data: the background script. It answers one question of the app over one
// native-messaging port, and does nothing until asked.
//
// App -> here:  {t:"list", id}
// Here -> app:  {t:"ready"}                       after every start of this script
//               {t:"list", id, cookies, usage, error}
//                 cookies: JSON text of [[host, count], ...], or "" when the cookies could not be listed
//                 usage:   JSON text of [[origin, topHost, bytes], ...], or "" when sizes are not available
//
// Cookies: browser.cookies.getAll with firstPartyDomain null (no filter on first-party isolation)
// and partitionKey {} (cookies kept apart per site as well as ordinary ones): all cookies of the
// normal cookie store. Cookies of private tabs are in another store, which this extension may
// not read (it is not allowed in private browsing) and does not ask for.
"use strict";

const port = browser.runtime.connectNative("krunk");
const send = message => {
  try {
    port.postMessage(message);
  } catch (e) {
    // the app is gone
  }
};

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

port.onMessage.addListener(m => {
  if (m && m.t === "list") list(m.id);
});

send({ t: "ready" });
