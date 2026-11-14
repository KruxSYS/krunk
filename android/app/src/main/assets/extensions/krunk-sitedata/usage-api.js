/* Krunk site data: how much each web origin has stored (privileged, parent process).
 *
 * The WebExtension APIs have no call for this, so this one function asks the engine's quota
 * manager, the way the engine's own browsingData API does before it clears storage
 * (ext-browsingData.js, clearQuotaManager): Services.qms.getUsage lists every origin that has a
 * storage directory (localStorage, IndexedDB, Cache API, ...) with its size on disk. Only
 * origins and sizes leave here; nothing is read from the stored data and nothing is changed.
 *
 * Left out: origins that are not web pages (the engine's own, extensions), data of private
 * tabs, and origins whose directory is empty.
 */
/* globals ExtensionAPI, ExtensionUtils, Services, Cr */
"use strict";

const MAX_ORIGINS = 20000;

this.krunkSiteData = class extends ExtensionAPI {
  getAPI() {
    const { ExtensionError } = ExtensionUtils;
    return {
      krunkSiteData: {
        usage() {
          return new Promise((resolve, reject) => {
            Services.qms.getUsage(request => {
              if (request.resultCode != Cr.NS_OK) {
                reject(new ExtensionError("storage sizes are not available"));
                return;
              }
              const out = [];
              for (const item of request.result) {
                if (out.length >= MAX_ORIGINS) break;
                try {
                  if (!item.persisted && !(item.usage > 0)) continue;
                  const principal = Services.scriptSecurityManager.createContentPrincipalFromOrigin(item.origin);
                  if (principal.scheme !== "http" && principal.scheme !== "https") continue;
                  const attributes = principal.originAttributes;
                  if (attributes.privateBrowsingId) continue;
                  out.push({
                    origin: principal.originNoSuffix,
                    partitionKey: attributes.partitionKey || "",
                    bytes: item.usage,
                  });
                } catch (e) {
                  // an origin the engine itself cannot parse: not a site
                }
              }
              resolve(JSON.stringify(out));
            });
          });
        },
      },
    };
  }
};
