// Tests for the guard's VPN route (assets/extensions/krunk-guard/vpn-api.js), run with
// `node --test` by the Gradle task guardJsTest. The engine's globals are stand-ins.
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const V = require(path.join(__dirname, "../../main/assets/extensions/krunk-guard/vpn-api.js"));

const PASS = "ab".repeat(32);
const REFUSED = 0x804b0048;
const ABORTED = 0x804b0002;

// ----- stand-ins -----

const env = { filters: [], topics: [], timers: new Map(), nextTimer: 1, defaults: new Map(), infos: [] };

function branch() {
  const d = env.defaults;
  const typeOf = v => (typeof v === "boolean" ? 128 : typeof v === "number" ? 64 : 32);
  return {
    PREF_INVALID: 0,
    PREF_STRING: 32,
    PREF_INT: 64,
    PREF_BOOL: 128,
    getPrefType: n => (d.has(n) ? typeOf(d.get(n)) : 0),
    getBoolPref: n => d.get(n),
    getIntPref: n => d.get(n),
    getStringPref: (n, fallback) => (d.has(n) ? d.get(n) : fallback),
    setBoolPref: (n, v) => d.set(n, v),
    setIntPref: (n, v) => d.set(n, v),
    setStringPref: (n, v) => d.set(n, v),
  };
}

global.Ci = {
  nsIProtocolProxyService: {},
  nsIProxyInfo: { TRANSPARENT_PROXY_RESOLVES_HOST: 1 },
};
global.Cr = { NS_ERROR_PROXY_CONNECTION_REFUSED: REFUSED, NS_BINDING_ABORTED: ABORTED };
global.Cc = {
  "@mozilla.org/network/protocol-proxy-service;1": {
    getService: () => ({
      registerChannelFilter: f => env.filters.push(f),
      unregisterChannelFilter: f => (env.filters = env.filters.filter(x => x !== f)),
      newProxyInfoWithAuth: (...args) => {
        assert.equal(args.length, 10);
        const info = { args };
        env.infos.push(info);
        return info;
      },
    }),
  },
};
global.Services = {
  prefs: { getDefaultBranch: () => branch() },
  obs: { notifyObservers: (s, topic) => env.topics.push(topic) },
};
global.ChromeUtils = {
  generateQI: () => () => {},
  importESModule: () => ({
    setTimeout: (fn, ms) => {
      const id = env.nextTimer++;
      env.timers.set(id, { fn, ms });
      return id;
    },
    clearTimeout: id => env.timers.delete(id),
  }),
};

function reset(defaults = {}) {
  V.shutdown();
  V.vpn.saved = null;
  env.filters = [];
  env.topics = [];
  env.timers.clear();
  env.infos = [];
  env.defaults = new Map(Object.entries(defaults));
}

function channel(url, pb = 0) {
  const c = { cancelled: null, URI: { host: new URL(url).hostname.replace(/^\[|\]$/g, "") } };
  c.loadInfo = { originAttributes: { privateBrowsingId: pb } };
  c.cancel = status => (c.cancelled = status);
  return c;
}

/** Runs the registered filter; returns {result, called, channel}. */
function open(url, pb = 0) {
  const c = channel(url, pb);
  const out = { channel: c, called: false, result: undefined };
  const cb = { onProxyFilterResult: info => Object.assign(out, { called: true, result: info }) };
  assert.equal(env.filters.length, 1, "one filter registered");
  env.filters[0].applyFilter(c, { backstop: true }, cb);
  return out;
}

const on = (extra = {}) => JSON.stringify({ t: "vpn", on: true, port: 4321, user: "krunk", pass: PASS, scope: "all", lan: false, ...extra });

// ----- pure -----

test("local, LAN and other hosts are told apart", () => {
  for (const h of ["localhost", "a.localhost", "127.0.0.1", "127.9.9.9", "::1", "[::1]"]) assert.ok(V.isLocalHost(h), h);
  for (const h of ["10.1.2.3", "172.16.0.1", "172.31.255.255", "192.168.1.1", "169.254.1.1", "fe80::1", "fd00::1", "fc12::5", "printer.local", "nas.lan", "router"]) {
    assert.ok(V.isLan(h), h);
  }
  for (const h of ["8.8.8.8", "172.32.0.1", "172.15.0.1", "example.com", "2001:db8::1", "127.0.0.1.example.com"]) {
    assert.ok(!V.isLan(h) && !V.isLocalHost(h), h);
  }
});

test("decide follows the scope and the LAN switch", () => {
  const all = { scope: "all", lan: false };
  assert.equal(V.decide("example.com", false, all), "proxy");
  assert.equal(V.decide("localhost", true, all), "direct");
  assert.equal(V.decide("192.168.1.1", false, all), "proxy");
  assert.equal(V.decide("192.168.1.1", false, { scope: "all", lan: true }), "direct");
  assert.equal(V.decide("example.com", false, { scope: "private", lan: false }), "direct");
  assert.equal(V.decide("example.com", true, { scope: "private", lan: false }), "proxy");
});

test("config messages are checked", () => {
  assert.deepEqual(V.parseConfig('{"on":false}'), { on: false });
  assert.equal(V.parseConfig(on()).port, 4321);
  assert.equal(V.parseConfig('{"on":true,"pending":true,"scope":"private"}').scope, "private");
  assert.throws(() => V.parseConfig(on({ port: 0 })));
  assert.throws(() => V.parseConfig(on({ pass: "short" })));
  assert.throws(() => V.parseConfig("{"));
});

// ----- the filter -----

test("on: proxied with SOCKS5 and the password, direct for localhost; off restores everything", () => {
  reset({ "network.proxy.type": 5, "network.trr.mode": 0, "network.http.http3.enable": true });
  const state = JSON.parse(V.configure(on()));
  assert.deepEqual(state, { installed: true, held: 0 });
  assert.deepEqual(env.topics, ["net:cancel-all-connections", "net:prune-all-connections"]);
  assert.equal(env.defaults.get("network.proxy.type"), 1);
  assert.equal(env.defaults.get("network.proxy.socks_port"), 1, "the real port never goes into a pref");
  assert.equal(env.defaults.get("network.trr.mode"), 5);
  // The password is never in a pref.
  for (const v of env.defaults.values()) assert.ok(!String(v).includes(PASS));

  const page = open("https://example.com/");
  assert.equal(page.result.args[0], "socks");
  assert.deepEqual(page.result.args.slice(1, 5), ["127.0.0.1", 4321, "krunk", PASS]);
  const priv = open("https://example.com/", 1);
  assert.notEqual(priv.result.args[6], page.result.args[6], "private and normal channels do not share connections");
  assert.equal(open("http://localhost:8080/").result, null, "direct is null, not the backstop");

  // The same config again: nothing is cut.
  env.topics = [];
  V.configure(on());
  assert.deepEqual(env.topics, []);

  const off = JSON.parse(V.configure('{"t":"vpn","on":false}'));
  assert.equal(off.installed, false);
  assert.equal(env.filters.length, 0);
  assert.equal(env.defaults.get("network.proxy.type"), 5);
  assert.equal(env.defaults.get("network.trr.mode"), 0);
  assert.equal(env.defaults.get("network.http.http3.enable"), true);
  assert.equal(env.defaults.get("krunk.vpn.engineConfig"), false);
  assert.equal(env.defaults.get("krunk.vpn.saved"), "");
});

test("scope private: normal channels direct, private ones proxied", () => {
  reset();
  V.configure(on({ scope: "private" }));
  assert.equal(open("https://example.com/").result, null);
  assert.equal(open("https://example.com/", 1).result.args[0], "socks");
});

test("pending: held, released when configured, cancelled after 20 s", () => {
  reset();
  const pending = JSON.parse(V.configure('{"t":"vpn","on":true,"pending":true,"scope":"all","lan":false}'));
  assert.equal(pending.installed, true);
  assert.equal(env.defaults.get("network.proxy.socks_port"), 1, "closed port until the proxy is up");
  const a = open("https://a.example/");
  const b = open("https://b.example/");
  assert.equal(a.called, false);
  assert.equal(open("http://127.0.0.1/").result, null, "localhost is never held");
  assert.equal(V.vpn.held.length, 2);

  // b times out.
  const [, timer] = [...env.timers.entries()][1];
  assert.equal(timer.ms, 20000);
  timer.fn();
  assert.equal(b.channel.cancelled, REFUSED);
  assert.equal(b.result, null);

  env.topics = [];
  V.configure(on());
  assert.deepEqual(env.topics, ["net:prune-all-connections"], "pending -> up is not a switch");
  assert.equal(a.result.args[0], "socks");
  assert.equal(a.channel.cancelled, null);
  assert.equal(V.vpn.held.length, 0);
});

test("a filter error never lets a channel go direct", () => {
  reset();
  V.configure('{"t":"vpn","on":true,"pending":true,"scope":"all","lan":false}');
  const bad = { loadInfo: null, cancel: s => (bad.cancelled = s) };
  Object.defineProperty(bad, "URI", { get: () => { throw new Error("boom"); } });
  let result = "unset";
  // A URI that throws reads as host "" and is held; a callback that throws is caught too.
  env.filters[0].applyFilter(bad, null, { onProxyFilterResult: r => (result = r) });
  assert.equal(result, "unset");
  V.configure(on());
  assert.equal(result.args[0], "socks");

  // Even the routing itself failing: proxied when configured.
  const saved = V.vpn.cfg;
  V.vpn.cfg = { get on() { throw new Error("broken"); } };
  let r2 = "unset";
  env.filters[0].applyFilter(channel("https://x.example/"), null, { onProxyFilterResult: r => (r2 = r) });
  V.vpn.cfg = saved;
  assert.equal(r2.args[0], "socks");
});

test("a bad message changes nothing and does not echo the password", () => {
  reset();
  const text = V.configure(on({ port: "x" }));
  assert.ok(!text.includes(PASS));
  assert.equal(JSON.parse(text).installed, false);
  assert.equal(env.filters.length, 0);
});

test("a start with the engine config's values holds until told, and off restores the engine's own", () => {
  reset({ "network.proxy.type": 1, "network.trr.mode": 5, "krunk.vpn.engineConfig": true });
  V.resume();
  assert.equal(env.filters.length, 1);
  assert.equal(open("https://example.com/").called, false);
  V.configure('{"t":"vpn","on":false}');
  assert.equal(env.defaults.get("network.proxy.type"), 5);
  assert.equal(env.defaults.get("network.trr.mode"), 0);
  assert.equal(env.defaults.get("krunk.vpn.engineConfig"), false);
});

test("an extension restart keeps the backstop and what to restore", () => {
  reset({ "network.proxy.type": 0 });
  V.configure(on({ scope: "private" }));
  V.shutdown();
  V.vpn.saved = null; // a new instance of the script
  assert.equal(env.filters.length, 0);
  assert.equal(env.defaults.get("network.proxy.type"), 1, "backstop stays while the filter is gone");
  V.resume();
  assert.equal(V.vpn.cfg.scope, "private");
  V.configure('{"t":"vpn","on":false}');
  assert.equal(env.defaults.get("network.proxy.type"), 0);
});
