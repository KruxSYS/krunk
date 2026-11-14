package org.krunk.browser.engine

import android.util.Log
import kotlin.coroutines.resume
import kotlinx.coroutines.suspendCancellableCoroutine
import org.json.JSONArray
import org.json.JSONObject
import org.krunk.browser.KrunkApp
import org.krunk.browser.ui.sitedata.SiteDataModel
import org.krunk.ffi.isSiteName
import org.krunk.ffi.webOrigin
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.WebExtension

/**
 * How Krunk learns which sites hold data: the bundled `krunk-sitedata` extension
 * (assets/extensions/krunk-sitedata), asked over one native-messaging port.
 *
 * Why an extension. GeckoView 160's `StorageController` can clear (`clearData`,
 * `clearDataFromHost`, `clearDataFromBaseDomain`) but lists nothing: there is no class for
 * cookies, usage or quota in the AAR. What the pinned engine does have (read in its `omni.ja`):
 *  - the WebExtension `cookies` API on Android as everywhere (`ext-toolkit.json` registers
 *    `ext-cookies.js`, and GeckoView loads that file: `category webextension-modules toolkit`).
 *    `cookies.getAll({firstPartyDomain: null, partitionKey: {}})` returns every cookie of the
 *    normal store: `null` drops the first-party-isolation filter, `{}` includes the cookies kept
 *    apart per site (Krunk's cookie behaviour isolates third parties, so a plain `getAll({})`
 *    would leave those out). The extension needs `<all_urls>`: cookies of hosts it has no
 *    permission for are filtered out. Private tabs: the query is bound to
 *    `privateBrowsingId: 0` unless the private store is named, and naming it is refused for an
 *    extension that is not allowed in private browsing. This one is not: a built-in
 *    (privileged) extension is given `internal:privateBrowsingAllowed` by the engine unless its
 *    manifest says `"incognito": "not_allowed"` (`Extension.sys.mjs`), and the manifest says so.
 *  - no WebExtension API for sizes, but a built-in extension is privileged
 *    (`ExtensionData.getIsPrivileged`: `builtIn`), which is all an experiment API asks for
 *    (`canUseAPIExperiment`); `krunk-guard` carries two. `usage-api.js` is one function around `Services.qms.getUsage`, the call
 *    the engine's own `browsingData` API makes (`ext-browsingData.js`): origins with their
 *    stored bytes (localStorage, IndexedDB, Cache API, ...; not cookies, not the HTTP cache).
 *
 * The answer may be missing (the extension is not up yet, did not install, a part failed): the
 * screen then works from Krunk's own records (`ui/sitedata/SiteDataModel`). Main thread only.
 */
class SiteDataExtension(runtime: GeckoRuntime) {
    private var port: WebExtension.Port? = null
    private var failed = false
    private var nextId = 1

    /** Requests without an answer yet; the ones in [unsent] wait for the port. */
    private val waiting = HashMap<Int, (EngineSiteData?) -> Unit>()
    private val unsent = ArrayList<Int>()

    init {
        runtime.webExtensionController.ensureBuiltIn(RESOURCE_URI, EXTENSION_ID).accept(
            { ext ->
                if (ext == null) {
                    fail()
                } else {
                    Log.i(KrunkApp.TAG, "site data extension ready: ${ext.metaData.version}")
                    adopt(ext)
                }
            },
            { e ->
                Log.e(KrunkApp.TAG, "site data extension failed to install", e)
                fail()
            },
        )
    }

    private fun adopt(ext: WebExtension) {
        ext.setMessageDelegate(
            object : WebExtension.MessageDelegate {
                override fun onConnect(port: WebExtension.Port) {
                    this@SiteDataExtension.port = port
                    port.setDelegate(
                        object : WebExtension.PortDelegate {
                            override fun onPortMessage(message: Any, port: WebExtension.Port) {
                                if (port === this@SiteDataExtension.port) (message as? JSONObject)?.let(::fromExtension)
                            }

                            override fun onDisconnect(port: WebExtension.Port) {
                                if (port !== this@SiteDataExtension.port) return
                                this@SiteDataExtension.port = null
                                // What was sent on this port is never answered.
                                answerAll { it !in unsent }
                            }
                        },
                    )
                    val ids = unsent.toList()
                    unsent.clear()
                    ids.forEach(::send)
                }
            },
            NATIVE_APP,
        )
    }

    private fun fail() {
        failed = true
        answerAll { true }
    }

    private fun answerAll(which: (Int) -> Boolean) {
        val ids = waiting.keys.filter(which)
        ids.forEach { id -> waiting.remove(id)?.invoke(null) }
        unsent.removeAll(ids.toSet())
    }

    private fun send(id: Int) {
        val sent = runCatching { port?.postMessage(JSONObject().put("t", "list").put("id", id)) }.getOrNull() != null
        if (!sent) waiting.remove(id)?.invoke(null)
    }

    private fun fromExtension(m: JSONObject) {
        if (m.optString("t") != "list") return
        val done = waiting.remove(m.optInt("id")) ?: return
        val error = m.optString("error")
        if (error.isNotEmpty()) Log.w(KrunkApp.TAG, "site data: $error")
        done(EngineSiteData(cookies = cookies(rows(m, "cookies")), usage = usage(rows(m, "usage"))))
    }

    /** A list the extension sends as JSON text (see its background.js); null when it sent none. */
    private fun rows(m: JSONObject, key: String): JSONArray? {
        val text = m.optString(key)
        if (text.isEmpty() || text.length > MAX_JSON) return null
        return runCatching { JSONArray(text) }.getOrNull()
    }

    private fun cookies(rows: JSONArray?): Map<String, Int>? {
        rows ?: return null
        val out = LinkedHashMap<String, Int>()
        for (i in 0 until minOf(rows.length(), MAX_ROWS)) {
            val row = rows.optJSONArray(i) ?: continue
            val count = row.optInt(1)
            val host = SiteDataModel.checkedHost(row.optString(0), ::isSiteName) ?: continue
            if (host.length > MAX_TEXT || count <= 0) continue
            out[host] = count
        }
        return out
    }

    private fun usage(rows: JSONArray?): List<OriginUsage>? {
        rows ?: return null
        val out = ArrayList<OriginUsage>()
        for (i in 0 until minOf(rows.length(), MAX_ROWS)) {
            val row = rows.optJSONArray(i) ?: continue
            val raw = row.optString(0)
            val rawTop = row.optString(1)
            if (raw.length > MAX_TEXT || rawTop.length > MAX_TEXT) continue
            val (origin, top) = SiteDataModel.checkedUsage(raw, rawTop, ::webOrigin, ::isSiteName) ?: continue
            out += OriginUsage(origin, top, row.optLong(2).coerceAtLeast(0L))
        }
        return out
    }

    /**
     * Asks the engine what it holds. Null when there is no answer: the extension did not
     * install or went away. While it is still starting the call waits; callers set the limit
     * (`withTimeoutOrNull`).
     */
    suspend fun list(): EngineSiteData? = suspendCancellableCoroutine { cont ->
        if (failed) {
            cont.resume(null)
            return@suspendCancellableCoroutine
        }
        val id = nextId++
        waiting[id] = { answer -> if (cont.isActive) cont.resume(answer) }
        cont.invokeOnCancellation {
            waiting.remove(id)
            unsent.remove(id)
        }
        if (port == null) unsent += id else send(id)
    }

    companion object {
        const val EXTENSION_ID = "sitedata@krunk.org"
        const val RESOURCE_URI = "resource://android/assets/extensions/krunk-sitedata/"
        const val NATIVE_APP = "krunk"
        private const val MAX_ROWS = 20_000
        private const val MAX_TEXT = 2048

        /** The longest list text accepted from the extension (rows are cut to MAX_ROWS anyway). */
        private const val MAX_JSON = 8_000_000
    }
}
