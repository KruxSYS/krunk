// Krunk icons: collects the page's icon links once the document is ready and hands them to the
// app over native messaging (port name "krunk"). No network requests are made here; the app
// decides which icon to fetch.
(function () {
  "use strict";
  function collect() {
    const out = [];
    for (const link of document.querySelectorAll("link[rel~='icon'], link[rel='apple-touch-icon'], link[rel='apple-touch-icon-precomposed']")) {
      if (!link.href) continue;
      out.push({ href: link.href, rel: link.rel, sizes: link.getAttribute("sizes") || "", type: link.type || "" });
      if (out.length >= 16) break;
    }
    return out;
  }
  try {
    browser.runtime.sendNativeMessage("krunk", { type: "icons", url: location.href, icons: collect() });
  } catch (e) {
    // The app side is not listening (e.g. a page without a session delegate): nothing to do.
  }
})();
