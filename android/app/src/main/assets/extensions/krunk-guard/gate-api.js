/* Krunk guard: the request gate (privileged, parent process).
 *
 * One observer looks at every HTTP request when it is opened and at who started it:
 *   - the bundled blocker (uBlock Origin): cancelled unless the gate is open. Krunk opens it only
 *     for an update the user asked for, or for good when the user turned on automatic updates.
 *     The blocker is recognised by the add-on id on the request's principal, never by its
 *     moz-extension:// address (that is random per install);
 *   - the engine itself (system principal, not a page load, not in a tab): allowed, logged or
 *     cancelled, as set. OCSP checks belong to a page's connection and are never touched;
 *   - anything else (pages, Krunk's own extensions): not touched.
 * A second observer reports top-level page loads that the blocker stopped, so Krunk can show its
 * own "blocked" page.
 *
 * The observers are registered when the extension starts ("startup" event), before any
 * background page runs, and the gate starts closed: nothing has to be sent to it first. The
 * start-up state comes from two prefs Krunk writes into the engine's config file.
 */
/* globals ExtensionAPI, ExtensionCommon, Services, Ci, Cr */
"use strict";

const BLOCKER_ID = "uBlock0@raymondhill.net";
const PREF_BLOCKER = "krunk.guard.blockerUpdates"; // "open" or anything else (closed)
const PREF_ENGINE = "krunk.guard.engineRequests"; // "allow", "block" or anything else (log)
const MAX_BACKLOG = 200;
const MAX_URL = 300;

const gate = {
  blockerOpen: false,
  engine: "log",
  listeners: new Set(),
  backlog: [],
  opening: null,
  stopping: null,
};

function report(event) {
  const text = JSON.stringify(event);
  if (gate.listeners.size === 0) {
    if (gate.backlog.length < MAX_BACKLOG) gate.backlog.push(text);
    return;
  }
  for (const fire of gate.listeners) fire.async(text);
}

function header(channel, name) {
  try {
    return channel.getRequestHeader(name);
  } catch (e) {
    return "";
  }
}

function onOpening(subject) {
  try {
    const channel = subject.QueryInterface(Ci.nsIHttpChannel);
    const info = channel.loadInfo;
    if (!info) return;
    const loading = info.loadingPrincipal;
    const triggering = info.triggeringPrincipal;
    const addon = (loading && loading.addonId) || (triggering && triggering.addonId) || "";
    if (addon === BLOCKER_ID) {
      const cancelled = !gate.blockerOpen;
      if (cancelled) channel.cancel(Cr.NS_ERROR_ABORT);
      report({ k: "blocker", url: channel.URI.spec.slice(0, MAX_URL), cancelled });
      return;
    }
    if (addon) return; // Krunk's own extensions
    if (gate.engine === "allow") return;
    const engine =
      !!triggering &&
      triggering.isSystemPrincipal &&
      (!loading || loading.isSystemPrincipal) &&
      info.externalContentPolicyType !== Ci.nsIContentPolicy.TYPE_DOCUMENT &&
      !info.browsingContextID;
    if (!engine) return;
    // A certificate status check is part of a page's TLS handshake.
    if (header(channel, "Content-Type") === "application/ocsp-request") return;
    const cancelled = gate.engine === "block";
    if (cancelled) channel.cancel(Cr.NS_ERROR_ABORT);
    report({ k: "engine", url: channel.URI.spec.slice(0, MAX_URL), cancelled });
  } catch (e) {
    // A request must never fail because of the gate's own bookkeeping.
  }
}

function onStopping(subject) {
  try {
    const channel = subject.QueryInterface(Ci.nsIHttpChannel);
    const info = channel.loadInfo;
    if (!info || info.externalContentPolicyType !== Ci.nsIContentPolicy.TYPE_DOCUMENT) return;
    const bag = subject.QueryInterface(Ci.nsIPropertyBag);
    let by = "";
    for (const name of ["cancelledByExtension", "redirectedByExtension"]) {
      try {
        by = by || String(bag.getProperty(name));
      } catch (e) {
        // not set
      }
    }
    if (by !== BLOCKER_ID) return;
    report({ k: "stopped", url: channel.URI.spec, cancelled: true });
  } catch (e) {
    // see above
  }
}

function install() {
  if (gate.opening) return;
  gate.blockerOpen = Services.prefs.getStringPref(PREF_BLOCKER, "closed") === "open";
  const engine = Services.prefs.getStringPref(PREF_ENGINE, "log");
  gate.engine = engine === "allow" || engine === "block" ? engine : "log";
  gate.opening = { observe: onOpening };
  gate.stopping = { observe: onStopping };
  Services.obs.addObserver(gate.opening, "http-on-opening-request");
  Services.obs.addObserver(gate.stopping, "http-on-stop-request");
}

function uninstall() {
  if (!gate.opening) return;
  Services.obs.removeObserver(gate.opening, "http-on-opening-request");
  Services.obs.removeObserver(gate.stopping, "http-on-stop-request");
  gate.opening = null;
  gate.stopping = null;
}

const stateJson = () => JSON.stringify({ blockerOpen: gate.blockerOpen, engine: gate.engine });

this.krunkGate = class extends ExtensionAPI {
  onStartup() {
    install();
  }

  onShutdown() {
    uninstall();
  }

  getAPI(context) {
    install();
    return {
      krunkGate: {
        async setBlockerGate(open) {
          gate.blockerOpen = !!open;
          return stateJson();
        },
        async setEngineRequests(mode) {
          gate.engine = mode;
          return stateJson();
        },
        async state() {
          return stateJson();
        },
        onEvent: new ExtensionCommon.EventManager({
          context,
          name: "krunkGate.onEvent",
          register: fire => {
            gate.listeners.add(fire);
            for (const text of gate.backlog.splice(0)) fire.async(text);
            return () => gate.listeners.delete(fire);
          },
        }).api(),
      },
    };
  }
};
