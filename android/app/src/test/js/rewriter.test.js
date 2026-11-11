// Tests for the guard's rewriter (assets/extensions/krunk-guard/rewriter.js), run with
// `node --test` by the Gradle task guardJsTest (part of testDebugUnitTest).
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");
const R = require(path.join(__dirname, "../../main/assets/extensions/krunk-guard/rewriter.js"));

// Timers the test fires by hand.
function fakeTimers() {
  const pending = new Map();
  let next = 1;
  return {
    set(fn, ms) {
      pending.set(next, { fn, ms });
      return next++;
    },
    clear(handle) {
      pending.delete(handle);
    },
    fire() {
      for (const [handle, t] of [...pending]) {
        pending.delete(handle);
        t.fn();
      }
    },
    size: () => pending.size,
    delays: () => [...pending.values()].map(t => t.ms),
  };
}

// The app's side: strips utm_* / fbclid / gclid like the core does, "" when nothing changes.
function cleaner(calls) {
  return url => {
    calls.push(url);
    const [base, query = ""] = url.split("?");
    const kept = query.split("&").filter(p => p && !/^(utm_|fbclid=|gclid=)/.test(p));
    const out = kept.length ? base + "?" + kept.join("&") : base;
    return Promise.resolve(out === url ? "" : out);
  };
}

const get = (url, requestId = "1") => ({ requestId, url, method: "GET", type: "main_frame" });

test("a top-level GET with tracking parameters is redirected to the cleaned address", async () => {
  const calls = [];
  const r = R.create(cleaner(calls), fakeTimers());
  r.setOn(true);
  const out = await r.onBeforeRequest(get("https://example.com/landing.html?utm_source=news&id=5&fbclid=abc"));
  assert.deepEqual(out, { redirectUrl: "https://example.com/landing.html?id=5" });
  assert.deepEqual(calls, ["https://example.com/landing.html?utm_source=news&id=5&fbclid=abc"]);
});

test("what is not a top-level GET with a query is never shown to the app", async () => {
  const calls = [];
  const r = R.create(cleaner(calls), fakeTimers());
  r.setOn(true);
  const untouched = [
    { requestId: "1", url: "https://example.com/post?utm_source=form&x=1", method: "POST", type: "main_frame" },
    { requestId: "2", url: "https://example.com/img.png?utm_source=x", method: "GET", type: "image" },
    { requestId: "3", url: "https://example.com/frame?utm_source=x", method: "GET", type: "sub_frame" },
    { requestId: "4", url: "https://example.com/xhr?utm_source=x", method: "GET", type: "xmlhttprequest" },
    { requestId: "5", url: "https://example.com/plain", method: "GET", type: "main_frame" },
    { requestId: "6", url: "https://example.com/a#utm_source=x", method: "GET", type: "main_frame" },
    { requestId: "7", url: "moz-extension://abc/page.html?utm_source=x", method: "GET", type: "main_frame" },
    { requestId: "8", url: "file:///sdcard/a.html?utm_source=x", method: "GET", type: "main_frame" },
    { requestId: "9", url: "https://example.com/?utm_source=x", method: "HEAD", type: "main_frame" },
  ];
  for (const details of untouched) assert.deepEqual(r.onBeforeRequest(details), {}, details.url);
  assert.deepEqual(calls, []);
});

test("with the setting off nothing is asked and nothing is rewritten", async () => {
  const calls = [];
  const r = R.create(cleaner(calls), fakeTimers());
  assert.equal(r.isOn(), false); // off until the app says otherwise
  assert.deepEqual(r.onBeforeRequest(get("https://example.com/?utm_source=x")), {});
  r.setOn(true);
  assert.ok((await r.onBeforeRequest(get("https://example.com/?utm_source=x"))).redirectUrl);
  r.setOn(false);
  assert.deepEqual(r.onBeforeRequest(get("https://example.com/?utm_source=x")), {});
  assert.equal(calls.length, 1);
});

test("an address that is already clean passes unchanged, so the redirect does not loop", async () => {
  const calls = [];
  const r = R.create(cleaner(calls), fakeTimers());
  r.setOn(true);
  const first = await r.onBeforeRequest(get("https://example.com/p?utm_source=a&x=1"));
  assert.deepEqual(first, { redirectUrl: "https://example.com/p?x=1" });
  // The engine calls the listener again for the redirect target, same request id.
  assert.deepEqual(await r.onBeforeRequest(get(first.redirectUrl)), {});
  assert.deepEqual(await r.onBeforeRequest(get("https://example.com/p?x=1", "2")), {});
});

test("every hop of a server redirect chain is cleaned", async () => {
  const r = R.create(cleaner([]), fakeTimers());
  r.setOn(true);
  // /redir has no query: not asked. The server answers 302 to an address with parameters.
  assert.deepEqual(r.onBeforeRequest(get("https://example.com/redir", "7")), {});
  const hop = await r.onBeforeRequest(get("https://example.com/landing.html?utm_campaign=y&keep=1&gclid=zz", "7"));
  assert.deepEqual(hop, { redirectUrl: "https://example.com/landing.html?keep=1" });
  assert.deepEqual(await r.onBeforeRequest(get(hop.redirectUrl, "7")), {});
});

test("one request is rewritten at most three times", async () => {
  // An app side that always finds something to change (it must not happen; the cap is the net).
  let n = 0;
  const r = R.create(url => Promise.resolve(url.split("?")[0] + "?n=" + ++n), fakeTimers());
  r.setOn(true);
  let url = "https://example.com/loop?n=0";
  for (let i = 1; i <= R.MAX_REWRITES; i++) {
    const out = await r.onBeforeRequest(get(url, "9"));
    assert.equal(out.redirectUrl, "https://example.com/loop?n=" + i);
    url = out.redirectUrl;
  }
  assert.equal(R.MAX_REWRITES, 3);
  assert.deepEqual(r.onBeforeRequest(get(url, "9")), {});
  assert.equal(n, 3);
  // Another request has its own count; a finished request's count is dropped.
  assert.ok((await r.onBeforeRequest(get("https://example.com/loop?n=0", "10"))).redirectUrl);
  assert.equal(r.pending(), 2);
  r.forget("9");
  r.forget("10");
  assert.equal(r.pending(), 0);
  assert.ok((await r.onBeforeRequest(get("https://example.com/loop?n=0", "9"))).redirectUrl);
});

test("no answer within the time limit lets the request through unchanged", async () => {
  const timers = fakeTimers();
  const r = R.create(() => new Promise(() => {}), timers); // the app never answers
  r.setOn(true);
  const waiting = r.onBeforeRequest(get("https://example.com/?utm_source=x"));
  assert.deepEqual(timers.delays(), [300]);
  assert.equal(R.ANSWER_MS, 300);
  timers.fire();
  assert.deepEqual(await waiting, {});
});

test("a late answer after the time limit changes nothing, and an answer in time cancels the timer", async () => {
  const timers = fakeTimers();
  let answer;
  const r = R.create(() => new Promise(resolve => (answer = resolve)), timers);
  r.setOn(true);
  const late = r.onBeforeRequest(get("https://example.com/?utm_source=x", "1"));
  await Promise.resolve();
  timers.fire();
  answer("https://example.com/");
  assert.deepEqual(await late, {});

  const inTime = r.onBeforeRequest(get("https://example.com/?utm_source=x", "2"));
  await Promise.resolve();
  answer("https://example.com/");
  assert.deepEqual(await inTime, { redirectUrl: "https://example.com/" });
  assert.equal(timers.size(), 0);
});

test("an answer that is empty, the same, not a web address or an error leaves the request alone", async () => {
  for (const reply of ["", null, undefined, 42, "https://example.com/?utm_source=x", "javascript:alert(1)", "file:///x"]) {
    const r = R.create(() => Promise.resolve(reply), fakeTimers());
    r.setOn(true);
    assert.deepEqual(await r.onBeforeRequest(get("https://example.com/?utm_source=x")), {}, String(reply));
  }
  const failing = R.create(() => Promise.reject(new Error("port closed")), fakeTimers());
  failing.setOn(true);
  assert.deepEqual(await failing.onBeforeRequest(get("https://example.com/?utm_source=x")), {});
  const throwing = R.create(() => {
    throw new Error("no port");
  }, fakeTimers());
  throwing.setOn(true);
  assert.deepEqual(await throwing.onBeforeRequest(get("https://example.com/?utm_source=x")), {});
});
