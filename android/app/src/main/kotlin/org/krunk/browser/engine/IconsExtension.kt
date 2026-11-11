package org.krunk.browser.engine

import android.util.Log
import org.json.JSONObject
import org.krunk.browser.KrunkApp
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoRuntime
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.WebExtension

/** One `<link rel=icon>` candidate a page declared. */
data class IconCandidate(val href: String, val rel: String, val sizes: String, val type: String)

/**
 * The bundled `krunk-icons` extension (assets/extensions/krunk-icons): its content script reports
 * each page's icon links over native messaging; this class installs it once (`ensureBuiltIn`)
 * and routes the messages of a session to a callback.
 */
class IconsExtension(private val runtime: GeckoRuntime) {
    @Volatile
    var extension: WebExtension? = null
        private set
    private val pending = mutableListOf<Pair<GeckoSession, (String, List<IconCandidate>) -> Unit>>()

    init {
        runtime.webExtensionController
            .ensureBuiltIn(RESOURCE_URI, EXTENSION_ID)
            .accept(
                { ext ->
                    Log.i(KrunkApp.TAG, "icons extension ready: ${ext?.id} ${ext?.metaData?.version}")
                    extension = ext
                    synchronized(pending) {
                        pending.forEach { (session, cb) -> attach(ext, session, cb) }
                        pending.clear()
                    }
                },
                { e -> Log.e(KrunkApp.TAG, "icons extension failed to install", e) },
            )
    }

    /** Delivers the icon candidates for pages loaded in [session] to [onIcons] (page URL, icons). */
    fun listen(session: GeckoSession, onIcons: (String, List<IconCandidate>) -> Unit) {
        val ext = extension
        if (ext == null) {
            synchronized(pending) { pending += session to onIcons }
        } else {
            attach(ext, session, onIcons)
        }
    }

    private fun attach(ext: WebExtension?, session: GeckoSession, onIcons: (String, List<IconCandidate>) -> Unit) {
        ext ?: return
        session.webExtensionController.setMessageDelegate(
            ext,
            object : WebExtension.MessageDelegate {
                override fun onMessage(
                    nativeApp: String,
                    message: Any,
                    sender: WebExtension.MessageSender,
                ): GeckoResult<Any>? {
                    val json = message as? JSONObject ?: return null
                    if (json.optString("type") != "icons") return null
                    val url = json.optString("url")
                    val arr = json.optJSONArray("icons")
                    val icons = ArrayList<IconCandidate>()
                    if (arr != null) {
                        for (i in 0 until arr.length()) {
                            val o = arr.optJSONObject(i) ?: continue
                            icons += IconCandidate(
                                href = o.optString("href"),
                                rel = o.optString("rel"),
                                sizes = o.optString("sizes"),
                                type = o.optString("type"),
                            )
                        }
                    }
                    onIcons(url, icons)
                    return null
                }
            },
            NATIVE_APP,
        )
    }

    companion object {
        const val EXTENSION_ID = "icons@krunk.org"
        const val RESOURCE_URI = "resource://android/assets/extensions/krunk-icons/"
        const val NATIVE_APP = "krunk"
    }
}
