package org.krunk.browser.ui

import android.util.Log
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.core.readCapped
import org.krunk.browser.vpn.VpnRoute
import org.krunk.ffi.AddressSuggestion
import org.krunk.ffi.OmniboxResult
import org.krunk.ffi.SearchEngineId
import org.krunk.ffi.mergeSearchSuggestions
import org.krunk.ffi.parseSuggestBody
import org.krunk.ffi.suggestUrl
import org.krunk.ffi.suggestWanted

/**
 * Address-bar suggestions: local rows from the core (history, bookmarks, the typed site's pages)
 * and, when the setting is on and the tab is not private, the engine's suggestions. The request
 * carries no cookies and uses a 3 s timeout, like Windows.
 */
class Omnibox(private val core: KrunkCore, private val userAgent: () -> String = { "" }) {
    private val cache = object : LinkedHashMap<String, List<String>>(32, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, List<String>>?): Boolean = size > 32
    }

    /** Forgets remembered engine suggestions (they echo what was typed). */
    fun clearCache() = synchronized(cache) { cache.clear() }

    suspend fun local(text: String): OmniboxResult = withContext(Dispatchers.IO) {
        core.store.omniboxQuery(text, core.settings.searchTemplate)
    }

    suspend fun withRemote(
        rows: List<AddressSuggestion>,
        typed: String,
        engine: SearchEngineId,
        forPrivateTab: Boolean,
    ): List<AddressSuggestion>? {
        val template = core.settings.searchTemplate
        if (!suggestWanted(typed, template)) return null
        val url = suggestUrl(engine, typed) ?: return null
        // Remembered per engine, and only real answers: a timeout or an offline moment must not
        // switch suggestions off for that text.
        val key = "$engine\n$typed"
        val remote = synchronized(cache) { cache[key] }
            ?: withContext(Dispatchers.IO) {
                fetch(url, forPrivateTab)
            }?.also { synchronized(cache) { cache[key] = it } }
            ?: return null
        if (remote.isEmpty()) return null
        return mergeSearchSuggestions(rows, typed, remote, template)
    }

    private companion object {
        /** The core's parser refuses more than this anyway. */
        const val MAX_BODY = 64 * 1024
    }

    /** The engine's suggestions, or null when the request failed (not worth remembering). */
    private fun fetch(url: String, forPrivateTab: Boolean): List<String>? {
        return try {
            val conn = VpnRoute.open(URL(url), forPrivateTab)
            conn.connectTimeout = 3000
            conn.readTimeout = 3000
            conn.useCaches = false
            conn.setRequestProperty("Accept", "application/json, text/plain, */*")
            userAgent().takeIf { it.isNotEmpty() }?.let { conn.setRequestProperty("User-Agent", it) }
            try {
                if (conn.responseCode != 200) return null
                val bytes = conn.inputStream.use { it.readCapped(MAX_BODY, maxMillis = 5_000) }
                if (bytes == null) null else parseSuggestBody(bytes)
            } finally {
                conn.disconnect()
            }
        } catch (e: Exception) {
            Log.d(KrunkApp.TAG, "suggest: ${e.javaClass.simpleName}")
            null
        }
    }
}
