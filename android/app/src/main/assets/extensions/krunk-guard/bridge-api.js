/* Krunk guard: the bridge to the bundled blocker (privileged, extension process).
 *
 * Krunk keeps one page of the blocker open in a session that is never shown
 * (no-dashboard.html: it loads the blocker's messaging client and nothing else). This API finds
 * that page and calls the function it already has, vAPI.messaging.send(channel, message): the
 * call the blocker's own settings pages make. Nothing is added to the page and nothing of the
 * blocker is changed; its background page receives an ordinary message from one of its own pages.
 */
/* globals ExtensionAPI, ExtensionUtils, ChromeUtils, Cu */
"use strict";

const BLOCKER_ID = "uBlock0@raymondhill.net";
const PAGE = "/no-dashboard.html";

this.krunkBridge = class extends ExtensionAPI {
  getAPI() {
    const { ExtensionPageChild } = ChromeUtils.importESModule("resource://gre/modules/ExtensionPageChild.sys.mjs");
    const { ExtensionError } = ExtensionUtils;
    const blockerPage = () => {
      for (const context of ExtensionPageChild.extensionContexts.values()) {
        try {
          const win = context.contentWindow;
          if (context.extension && context.extension.id === BLOCKER_ID && win && win.location.pathname === PAGE) return win;
        } catch (e) {
          // a context that is going away
        }
      }
      return null;
    };
    return {
      krunkBridge: {
        async send(channel, message) {
          const window = blockerPage();
          if (!window) throw new ExtensionError("blocker page not open");
          const page = Cu.waiveXrays(window);
          if (!page.vAPI || !page.vAPI.messaging || typeof page.vAPI.messaging.send !== "function") {
            throw new ExtensionError("blocker page not ready");
          }
          const reply = await page.vAPI.messaging.send(channel, Cu.cloneInto(JSON.parse(message), window));
          if (reply === undefined) return "null";
          const text = page.JSON.stringify(reply);
          return typeof text === "string" ? text : "null";
        },
      },
    };
  }
};
