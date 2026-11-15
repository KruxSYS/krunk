/* Krunk certificate exceptions (privileged, parent process).
 *
 * GeckoView has no call for "continue past this certificate", so the app asks the engine's own
 * certificate override service, the one Firefox's "Accept the risk and continue" uses
 * (nsICertOverrideService, contract @mozilla.org/security/certoverride;1). An exception is bound
 * to host, port and the exact certificate: the engine compares the certificate's SHA-256
 * fingerprint, so a site that shows another certificate is refused again.
 *
 *  add:    nsIX509CertDB.constructX509FromBase64(der) and
 *          rememberValidityOverride(host, port, originAttributes, cert, temporary).
 *          Private tabs ({privateBrowsingId: 1}) are always temporary, whatever the app asked.
 *  remove: clearValidityOverride(host, port, originAttributes) for normal and private tabs,
 *          then the open connections are dropped so the next request checks the certificate.
 *  list:   getOverrides(), normal tabs only, as JSON text [{host, port, sha256}].
 *
 * Nothing here reads pages or makes requests.
 */
/* globals ExtensionAPI, ExtensionUtils, Services, Cc, Ci */
"use strict";

const MAX_OVERRIDES = 5000;
const MAX_DER_BASE64 = 64 * 1024;

const overrideService = () => Cc["@mozilla.org/security/certoverride;1"].getService(Ci.nsICertOverrideService);
const attributes = isPrivate => (isPrivate ? { privateBrowsingId: 1 } : {});

this.krunkCerts = class extends ExtensionAPI {
  getAPI() {
    const { ExtensionError } = ExtensionUtils;
    const checkHostPort = (host, port) => {
      if (typeof host !== "string" || host.length === 0 || host.length > 253 || !/^[a-z0-9.:-]+$/i.test(host)) {
        throw new ExtensionError("not a host name");
      }
      if (!Number.isInteger(port) || port < 1 || port > 65535) throw new ExtensionError("not a port");
    };
    return {
      krunkCerts: {
        async add(host, port, derBase64, temporary, isPrivate) {
          checkHostPort(host, port);
          if (typeof derBase64 !== "string" || derBase64.length === 0 || derBase64.length > MAX_DER_BASE64) {
            throw new ExtensionError("no certificate");
          }
          const certDb = Cc["@mozilla.org/security/x509certdb;1"].getService(Ci.nsIX509CertDB);
          const cert = certDb.constructX509FromBase64(derBase64);
          overrideService().rememberValidityOverride(
            host,
            port,
            attributes(isPrivate),
            cert,
            isPrivate ? true : Boolean(temporary),
          );
          return cert.sha256Fingerprint;
        },

        async remove(host, port) {
          checkHostPort(host, port);
          const service = overrideService();
          service.clearValidityOverride(host, port, attributes(false));
          service.clearValidityOverride(host, port, attributes(true));
          // Connections already open to the site were accepted with the exception: drop them.
          try {
            Services.obs.notifyObservers(null, "net:prune-all-connections");
          } catch (e) {
            // the network layer may not listen; the next new connection checks anyway
          }
        },

        async list() {
          const out = [];
          for (const o of overrideService().getOverrides()) {
            if (out.length >= MAX_OVERRIDES) break;
            try {
              const oa = o.originAttributes || {};
              if (oa.privateBrowsingId) continue;
              out.push({ host: o.asciiHost, port: o.port, sha256: o.fingerprint });
            } catch (e) {
              // an entry the engine cannot describe
            }
          }
          return JSON.stringify(out);
        },
      },
    };
  }
};
