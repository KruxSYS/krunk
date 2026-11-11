// Krunk guard: which top-level requests are shown to the app for cleaning, and what happens
// with the answer. No browser API in here, so the rules can be tested with plain node
// (android/app/src/test/js/rewriter.test.js).
//
// A request is asked about when rewriting is on, it is a top-level GET to a web address and the
// address has a query. The app answers with the address without tracking parameters (the shared
// core's rules) or with "" to leave it. A different address becomes a redirect; every redirect
// hop comes here again. One request is rewritten at most MAX_REWRITES times, and no answer
// within ANSWER_MS lets it through unchanged.
"use strict";

var KrunkRewriter = (() => {
  const MAX_REWRITES = 3;
  const ANSWER_MS = 300;

  // details: {requestId, url, method, type}
  function wanted(details, on) {
    if (!on || !details) return false;
    if (details.type !== "main_frame" || details.method !== "GET") return false;
    const url = details.url;
    if (typeof url !== "string" || !/^https?:\/\//i.test(url)) return false;
    return url.includes("?");
  }

  // ask(url) -> Promise<string>: the app's answer. timers: {set(fn, ms) -> handle, clear(handle)}.
  function create(ask, timers) {
    let on = false;
    const counts = new Map(); // requestId -> rewrites done

    function answer(url) {
      return new Promise(resolve => {
        let done = false;
        const finish = value => {
          if (done) return;
          done = true;
          timers.clear(timer);
          resolve(typeof value === "string" ? value : "");
        };
        const timer = timers.set(() => finish(""), ANSWER_MS);
        Promise.resolve()
          .then(() => ask(url))
          .then(finish, () => finish(""));
      });
    }

    return {
      setOn(value) {
        on = value === true;
      },
      isOn() {
        return on;
      },
      // What webRequest.onBeforeRequest returns: {} (leave it) or a promise of {} / {redirectUrl}.
      onBeforeRequest(details) {
        if (!wanted(details, on)) return {};
        const count = counts.get(details.requestId) || 0;
        if (count >= MAX_REWRITES) return {};
        return answer(details.url).then(url => {
          if (!url || url === details.url || !/^https?:\/\//i.test(url)) return {};
          counts.set(details.requestId, count + 1);
          return { redirectUrl: url };
        });
      },
      // The request ended (completed or failed): its count is not needed any more.
      forget(requestId) {
        counts.delete(requestId);
      },
      pending() {
        return counts.size;
      },
    };
  }

  return { MAX_REWRITES, ANSWER_MS, wanted, create };
})();

if (typeof module !== "undefined") module.exports = KrunkRewriter;
