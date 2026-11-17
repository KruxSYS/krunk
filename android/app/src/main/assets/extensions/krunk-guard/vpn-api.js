/* Krunk guard: the VPN route (privileged, parent process).
 *
 * While Krunk's in-browser VPN is on, one channel filter (nsIProtocolProxyService, the pattern of
 * Firefox's own IP Protection, IPPChannelFilter.sys.mjs) decides for every connection the engine
 * opens:
 *   - localhost, 127.0.0.0/8, ::1: direct, always (they never leave the device);
 *   - scope "private" and a normal (not private) channel: direct;
 *   - "local network directly" on and a LAN address (RFC 1918, 169.254/16, fe80::/10, fc00::/7,
 *     .local, .lan, a name without dots): direct;
 *   - everything else, engine requests and DNS-over-HTTPS channels included: through Krunk's
 *     VPN proxy on 127.0.0.1 as SOCKS5 with user name and password (Gecko does not send a
 *     Proxy-Authorization header to an http proxy, spike S2). Host names travel unresolved.
 *     While the proxy is not up yet ("pending") these channels are held, and cancelled with
 *     NS_ERROR_PROXY_CONNECTION_REFUSED when it is not up within 20 s.
 * The filter must never throw: Gecko connects directly when a filter fails (spike S2). Every
 * answer is wrapped; on an error the channel goes to the proxy if there is one, else it is
 * cancelled.
 *
 * Backstop: the proxy prefs below on the engine's default branch point every channel the filter
 * misses at the same SOCKS port, without the password, so the proxy refuses it. They and the
 * leak prefs (DNS prefetch, HTTPS RR, TRR, HTTP/3, WebTransport, WebRTC host candidates) are
 * written by Krunk's engine config when the VPN is on at start (EngineConfig.vpnPrefs mirrors
 * VPN_PREFS; a JVM test compares them) and set or restored here at run time. Default-branch
 * values are never saved to prefs.js. What they were before is kept in a default-branch pref
 * too (no secrets in it), so a restart of this extension can still restore them.
 *
 * The port and the password come from the app through background.js and stay in this module's
 * memory: never in a pref (prefs reach content processes), never logged.
 */
/* globals ExtensionAPI, Services, Cc, Ci, Cr, ChromeUtils */
"use strict";

const HOLD_MS = 20_000;
const MAX_HELD = 500;
const PROXY_USER = "krunk";
// Where the previous default values (and the scope) are kept while the prefs below are applied.
const PREF_SAVED = "krunk.vpn.saved";
const MARKER = "krunk.vpn.engineConfig";

// BEGIN VPN_PREFS — [name, value while the VPN is on, the engine's own default].
// The backstop points at SOCKS 127.0.0.1:1, a port nothing can listen on: a channel the filter
// misses fails. The proxy's real port and the password are never in a pref. The last column is
// used only to restore a start whose engine config already held these values (MARKER true), when
// the engine's own defaults are no longer readable. Mirrored by EngineConfig.vpnPrefs (JVM test).
const VPN_PREFS = [
  ["network.proxy.type", 1, 5],
  ["network.proxy.socks", "127.0.0.1", ""],
  ["network.proxy.socks_port", 1, 0],
  ["network.proxy.socks_version", 5, 5],
  ["network.proxy.socks_remote_dns", true, false],
  ["network.proxy.no_proxies_on", "", ""],
  ["network.proxy.allow_hijacking_localhost", false, false],
  ["network.proxy.failover_direct", false, true],
  ["network.dns.disablePrefetch", true, false],
  ["network.dns.disablePrefetchFromHTTPS", true, true],
  ["network.dns.native_https_query", false, true],
  ["network.dns.upgrade_with_https_rr", false, true],
  ["network.dns.use_https_rr_as_altsvc", false, true],
  ["network.trr.mode", 5, 0],
  ["network.http.http3.enable", false, true],
  ["network.http.altsvc.enabled", false, true],
  ["network.webtransport.enabled", false, true],
  ["network.http.speculative-parallel-limit", 0, 20],
  ["network.early-hints.preconnect.enabled", false, true],
  ["media.peerconnection.ice.proxy_only", true, false],
  ["media.peerconnection.ice.no_host", true, false],
  ["media.peerconnection.ice.default_address_only", true, false],
  ["krunk.vpn.engineConfig", true, false]
];
// END VPN_PREFS

// ----- pure: which way a host goes -----

function bare(host) {
  let h = String(host || "").toLowerCase();
  if (h.startsWith("[") && h.endsWith("]")) h = h.slice(1, -1);
  if (h.endsWith(".")) h = h.slice(0, -1);
  return h;
}

function ipv4(h) {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(h);
  if (!m) return null;
  const parts = m.slice(1).map(Number);
  return parts.every(n => n <= 255) ? parts : null;
}

function isLocalHost(host) {
  const h = bare(host);
  if (h === "localhost" || h.endsWith(".localhost")) return true;
  if (h === "::1" || h === "0:0:0:0:0:0:0:1") return true;
  const v4 = ipv4(h);
  return !!v4 && v4[0] === 127;
}

function isLan(host) {
  const h = bare(host);
  if (!h) return false;
  const v4 = ipv4(h);
  if (v4) {
    const [a, b] = v4;
    return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || (a === 169 && b === 254);
  }
  if (h.includes(":")) {
    const first = parseInt(h.split(":")[0] || "0", 16);
    if (Number.isNaN(first)) return false;
    return (first & 0xffc0) === 0xfe80 || (first & 0xfe00) === 0xfc00;
  }
  return h.endsWith(".local") || h.endsWith(".lan") || !h.includes(".");
}

/** "direct" | "proxy" for a channel to [host]; [cfg] = {scope, lan}. */
function decide(host, privateChannel, cfg) {
  if (isLocalHost(host)) return "direct";
  if (cfg.scope === "private" && !privateChannel) return "direct";
  if (cfg.lan && isLan(host)) return "direct";
  return "proxy";
}

/** The app's message, checked. Throws on a malformed one. */
function parseConfig(json) {
  const m = typeof json === "string" ? JSON.parse(json) : json;
  if (!m || m.on !== true) return { on: false };
  const scope = m.scope === "private" ? "private" : "all";
  const lan = m.lan === true;
  if (m.pending === true) return { on: true, pending: true, scope, lan };
  const port = Number(m.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("bad port");
  if (typeof m.pass !== "string" || !/^[0-9a-f]{64}$/.test(m.pass)) throw new Error("bad password");
  return { on: true, pending: false, scope, lan, port, pass: m.pass };
}

/** "switch" (on/off, scope, lan changed), "prune" (only the proxy changed) or "same". */
function change(before, after) {
  if (before.on !== after.on) return "switch";
  if (!after.on) return "same";
  if (before.scope !== after.scope || before.lan !== after.lan) return "switch";
  if (before.pending !== after.pending || before.port !== after.port || before.pass !== after.pass) return "prune";
  return "same";
}

// ----- engine side -----

const vpn = {
  cfg: { on: false },
  filter: null,
  infos: null, // {normal, private}: nsIProxyInfo while configured
  held: [],
  saved: null, // {prev: {name: value|null}, scope, lan}
};

function pps() {
  return Cc["@mozilla.org/network/protocol-proxy-service;1"].getService(Ci.nsIProtocolProxyService);
}

function timers() {
  return ChromeUtils.importESModule("resource://gre/modules/Timer.sys.mjs");
}

function isolationKey() {
  return "krunk-vpn-" + Math.random().toString(36).slice(2, 14);
}

function makeInfos(port, pass) {
  const key = isolationKey();
  const info = suffix =>
    pps().newProxyInfoWithAuth(
      "socks",
      "127.0.0.1",
      port,
      PROXY_USER,
      pass,
      "",
      key + suffix,
      Ci.nsIProxyInfo.TRANSPARENT_PROXY_RESOLVES_HOST,
      10,
      null
    );
  return { normal: info("-n"), private: info("-p") };
}

function channelFacts(channel) {
  let host = "";
  try {
    host = channel.URI.host;
  } catch (e) {
    host = "";
  }
  let priv = false;
  try {
    const li = channel.loadInfo;
    priv = !!(li && li.originAttributes && li.originAttributes.privateBrowsingId);
  } catch (e) {
    priv = false;
  }
  return { host, priv };
}

function cancel(channel, status) {
  try {
    channel.cancel(status);
  } catch (e) {
    // already done
  }
}

function answer(cb, info) {
  try {
    cb.onProxyFilterResult(info);
  } catch (e) {
    // the channel is gone
  }
}

/** The one decision for a channel, also for held ones. Never throws. */
function route(channel, cb) {
  try {
    if (!vpn.cfg.on) return answer(cb, null);
    const { host, priv } = channelFacts(channel);
    // "direct" is null, not the default proxy: that default is the backstop (no password).
    if (decide(host, priv, vpn.cfg) === "direct") return answer(cb, null);
    if (vpn.infos) return answer(cb, priv ? vpn.infos.private : vpn.infos.normal);
    hold(channel, cb);
  } catch (e) {
    fallback(channel, cb);
  }
}

function fallback(channel, cb) {
  const infos = vpn.infos;
  if (infos) {
    answer(cb, infos.normal);
  } else {
    cancel(channel, Cr.NS_ERROR_PROXY_CONNECTION_REFUSED);
    answer(cb, null);
  }
}

function hold(channel, cb) {
  if (vpn.held.length >= MAX_HELD) {
    cancel(channel, Cr.NS_ERROR_PROXY_CONNECTION_REFUSED);
    return answer(cb, null);
  }
  const entry = { channel, cb, timer: null };
  entry.timer = timers().setTimeout(() => {
    const i = vpn.held.indexOf(entry);
    if (i < 0) return;
    vpn.held.splice(i, 1);
    cancel(channel, Cr.NS_ERROR_PROXY_CONNECTION_REFUSED);
    answer(cb, null);
  }, HOLD_MS);
  vpn.held.push(entry);
}

/** Held channels are decided again (released, sent direct, or kept). */
function rerouteHeld() {
  const held = vpn.held.splice(0);
  for (const entry of held) {
    try {
      timers().clearTimeout(entry.timer);
    } catch (e) {
      // ignore
    }
    route(entry.channel, entry.cb);
  }
}

function cancelHeld(status) {
  const held = vpn.held.splice(0);
  for (const entry of held) {
    try {
      timers().clearTimeout(entry.timer);
    } catch (e) {
      // ignore
    }
    cancel(entry.channel, status);
    answer(entry.cb, null);
  }
}

function register() {
  if (vpn.filter) return;
  vpn.filter = {
    QueryInterface: ChromeUtils.generateQI(["nsIProtocolProxyChannelFilter"]),
    applyFilter(channel, defaultInfo, cb) {
      // route() catches everything; this is the last line in case even that fails.
      try {
        route(channel, cb);
      } catch (e) {
        try {
          fallback(channel, cb);
        } catch (e2) {
          // nothing left to do
        }
      }
    },
  };
  pps().registerChannelFilter(vpn.filter, 0);
}

function unregister() {
  if (!vpn.filter) return;
  try {
    pps().unregisterChannelFilter(vpn.filter);
  } finally {
    vpn.filter = null;
  }
}

// ----- prefs (default branch only) -----

function readDefault(branch, name) {
  switch (branch.getPrefType(name)) {
    case branch.PREF_BOOL:
      return branch.getBoolPref(name);
    case branch.PREF_INT:
      return branch.getIntPref(name);
    case branch.PREF_STRING:
      return branch.getStringPref(name);
    default:
      return null;
  }
}

function writeDefault(branch, name, value) {
  try {
    if (typeof value === "boolean") branch.setBoolPref(name, value);
    else if (typeof value === "number") branch.setIntPref(name, value);
    else if (typeof value === "string") branch.setStringPref(name, value);
  } catch (e) {
    // A pref of another type: left alone (none in the list is).
  }
}

function loadSaved() {
  try {
    const text = Services.prefs.getDefaultBranch("").getStringPref(PREF_SAVED, "");
    const saved = text ? JSON.parse(text) : null;
    return saved && saved.prev ? saved : null;
  } catch (e) {
    return null;
  }
}

function storeSaved(saved) {
  writeDefault(Services.prefs.getDefaultBranch(""), PREF_SAVED, saved ? JSON.stringify(saved) : "");
}

function applyPrefs() {
  const branch = Services.prefs.getDefaultBranch("");
  if (!vpn.saved) {
    const prev = {};
    // Started with these values from the engine config: what was there before is the engine's own.
    const fromConfig = readDefault(branch, MARKER) === true;
    for (const [name, , engine] of VPN_PREFS) prev[name] = fromConfig ? engine : readDefault(branch, name);
    vpn.saved = { prev };
  }
  vpn.saved.scope = vpn.cfg.scope;
  vpn.saved.lan = vpn.cfg.lan;
  storeSaved(vpn.saved);
  for (const [name, value] of VPN_PREFS) writeDefault(branch, name, value);
}

function restorePrefs() {
  const branch = Services.prefs.getDefaultBranch("");
  let saved = vpn.saved;
  if (!saved && readDefault(branch, MARKER) === true) {
    saved = { prev: Object.fromEntries(VPN_PREFS.map(([name, , engine]) => [name, engine])) };
  }
  if (!saved) return;
  for (const [name] of VPN_PREFS) {
    const value = saved.prev[name];
    if (value !== null && value !== undefined) writeDefault(branch, name, value);
  }
  // Not in the saved list when it did not exist before: it must never stay true while off.
  writeDefault(branch, MARKER, false);
  vpn.saved = null;
  storeSaved(null);
}

function notify(topic) {
  try {
    Services.obs.notifyObservers(null, topic);
  } catch (e) {
    // ignore
  }
}

// ----- what the background script calls -----

function stateJson(error) {
  const s = { installed: !!vpn.filter, held: vpn.held.length };
  if (error) s.error = error;
  return JSON.stringify(s);
}

function configure(json) {
  let next;
  try {
    next = parseConfig(json);
  } catch (e) {
    // Never echo the message: it may hold the password.
    return stateJson("bad config");
  }
  const what = change(vpn.cfg, next);
  try {
    if (!next.on) {
      vpn.cfg = next;
      vpn.infos = null;
      unregister();
      cancelHeld(Cr.NS_BINDING_ABORTED);
      restorePrefs();
    } else {
      if (what !== "same" || !vpn.filter) {
        vpn.infos = next.pending ? null : makeInfos(next.port, next.pass);
      }
      vpn.cfg = next;
      applyPrefs();
      register();
      // The same config again keeps held channels on their running time limit.
      if (what !== "same") rerouteHeld();
    }
  } catch (e) {
    return stateJson("failed: " + (e && e.name ? e.name : "error"));
  } finally {
    if (what === "switch") notify("net:cancel-all-connections");
    if (what !== "same") notify("net:prune-all-connections");
  }
  return stateJson();
}

/** At start: the prefs are on (engine config or an earlier run of this script): hold until told. */
function resume() {
  if (vpn.filter) return;
  vpn.saved = loadSaved();
  const branch = Services.prefs.getDefaultBranch("");
  if (!vpn.saved && readDefault(branch, MARKER) !== true) return;
  const scope = vpn.saved && vpn.saved.scope === "private" ? "private" : "all";
  vpn.cfg = { on: true, pending: true, scope, lan: !!(vpn.saved && vpn.saved.lan) };
  vpn.infos = null;
  register();
}

/** The extension stops (restart, update): the filter goes, the backstop prefs stay (fail-closed). */
function shutdown() {
  try {
    unregister();
  } catch (e) {
    // ignore
  }
  cancelHeld(Cr.NS_ERROR_PROXY_CONNECTION_REFUSED);
  vpn.infos = null;
  vpn.cfg = { on: false };
}

if (typeof module !== "undefined") {
  // node tests (src/test/js/vpn.test.js) with stand-ins for the engine's globals.
  module.exports = { VPN_PREFS, isLocalHost, isLan, decide, parseConfig, change, configure, route, resume, shutdown, vpn };
} else {
  this.krunkVpn = class extends ExtensionAPI {
    onStartup() {
      try {
        resume();
      } catch (e) {
        // The backstop prefs still refuse what the filter would have held.
      }
    }

    onShutdown() {
      shutdown();
    }

    getAPI() {
      return {
        krunkVpn: {
          async configure(json) {
            return configure(json);
          },
          async state() {
            return stateJson();
          },
        },
      };
    }
  };
}
