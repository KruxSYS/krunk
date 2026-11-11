// Krunk guard: the background script. It connects the app (one native-messaging port) with the
// gate (gate-api.js), the bridge to the bundled blocker (bridge-api.js) and the rewriter below.
//
// App -> guard:  {t:"hello"}                 answer with "ready" (the app asks after it changed
//                                            something that restarts this extension)
//                {t:"gate", open}            open or close the gate for the blocker's requests
//                {t:"engine", mode}          allow | log | block the engine's own requests
//                {t:"blocker", id, channel, msg}   one message for the blocker
//                {t:"strip", on}             tracking-parameter rewriting on or off
//                {t:"rewritten", id, url}    the answer to a "rewrite" question ("" = leave it)
// Guard -> app:  {t:"ready", gate}           after every start of this script
//                {t:"gate", gate}            the gate's state after a change
//                {t:"event", e}              something the gate did (a cancelled request, a page
//                                            the blocker stopped)
//                {t:"blocker-reply", id, ok, result | error}
//                {t:"rewrite", id, url}      may this top-level address be cleaned?
"use strict";

const port = browser.runtime.connectNative("krunk");
const send = message => {
  try {
    port.postMessage(message);
  } catch (e) {
    // the app is gone
  }
};

// ----- gate -----

browser.krunkGate.onEvent.addListener(text => send({ t: "event", e: JSON.parse(text) }));

const gateState = promise =>
  promise.then(text => send({ t: "gate", gate: JSON.parse(text) })).catch(e => send({ t: "gate", error: String(e) }));

// ----- bridge -----

// The list of filter lists is large; the app needs the selected ones and their dates.
function brief(what, reply) {
  if (what !== "getLists" || !reply || !reply.available) return reply;
  const lists = [];
  for (const [key, list] of Object.entries(reply.available)) {
    if (list.off === true) continue;
    const cached = reply.cache && reply.cache[key];
    lists.push({ key, written: (cached && cached.writeTime) || 0, filters: list.entryCount || 0, inPackage: !!list.hasLocalURL });
  }
  return { isUpdating: !!reply.isUpdating, autoUpdate: !!reply.autoUpdate, netFilters: reply.netFilterCount || 0, cosmeticFilters: reply.cosmeticFilterCount || 0, lists };
}

function toBlocker(m) {
  browser.krunkBridge
    .send(m.channel, JSON.stringify(m.msg))
    .then(text => send({ t: "blocker-reply", id: m.id, ok: true, result: brief(m.msg.what, JSON.parse(text)) }))
    .catch(e => send({ t: "blocker-reply", id: m.id, ok: false, error: String(e) }));
}

// ----- tracking parameters -----

// rewriter.js decides which top-level requests are shown to the app and what its answer does.
let nextQuestion = 1;
const questions = new Map(); // id -> resolve

const rewriter = KrunkRewriter.create(
  url =>
    new Promise(resolve => {
      const id = nextQuestion++;
      questions.set(id, resolve);
      // An answer that never comes is dropped by the rewriter's own time limit.
      setTimeout(() => questions.delete(id), 4 * KrunkRewriter.ANSWER_MS);
      send({ t: "rewrite", id, url });
    }),
  { set: (fn, ms) => setTimeout(fn, ms), clear: handle => clearTimeout(handle) }
);

const MAIN_FRAMES = { urls: ["http://*/*", "https://*/*"], types: ["main_frame"] };
browser.webRequest.onBeforeRequest.addListener(details => rewriter.onBeforeRequest(details), MAIN_FRAMES, ["blocking"]);
browser.webRequest.onCompleted.addListener(details => rewriter.forget(details.requestId), MAIN_FRAMES);
browser.webRequest.onErrorOccurred.addListener(details => rewriter.forget(details.requestId), MAIN_FRAMES);

// ----- app -----

const ready = () =>
  browser.krunkGate
    .state()
    .then(text => send({ t: "ready", gate: JSON.parse(text) }))
    .catch(e => send({ t: "ready", error: String(e) }));

port.onMessage.addListener(m => {
  switch (m && m.t) {
    case "hello":
      ready();
      break;
    case "gate":
      gateState(browser.krunkGate.setBlockerGate(m.open === true));
      break;
    case "engine":
      gateState(browser.krunkGate.setEngineRequests(m.mode));
      break;
    case "blocker":
      toBlocker(m);
      break;
    case "strip":
      rewriter.setOn(m.on === true);
      break;
    case "rewritten": {
      const answer = questions.get(m.id);
      if (answer) {
        questions.delete(m.id);
        answer(typeof m.url === "string" ? m.url : "");
      }
      break;
    }
  }
});

ready();
