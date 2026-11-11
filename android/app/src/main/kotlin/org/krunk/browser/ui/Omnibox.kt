package org.krunk.browser.ui

import android.util.Log
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.core.readCapped
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
class Omnibox(private val core: KrunkCore) {
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
    ): List<AddressSuggestion>? {
        val template = core.settings.searchTemplate
        if (!suggestWanted(typed, template)) return null
        val url = suggestUrl(engine, typed) ?: return null
        val remote = synchronized(cache) { cache[typed] } ?: withContext(Dispatchers.IO) { fetch(url) }
            .also { synchronized(cache) { cache[typed] = it } }
        if (remote.isEmpty()) return null
        return mergeSearchSuggestions(rows, typed, remote, template)
    }

    private companion object {
        /** The core's parser refuses more than this anyway. */
        const val MAX_BODY = 64 * 1024
    }

    private fun fetch(url: String): List<String> {
        return try {
            val conn = URL(url).openConnection() as HttpURLConnection
            conn.connectTimeout = 3000
            conn.readTimeout = 3000
            conn.useCaches = false
            conn.setRequestProperty("Accept", "application/json, text/plain, */*")
            conn.setRequestProperty("Cookie", "")
            try {
                if (conn.responseCode != 200) return emptyList()
                val bytes = conn.inputStream.use { it.readCapped(MAX_BODY, maxMillis = 5_000) }
                if (bytes == null) emptyList() else parseSuggestBody(bytes)
            } finally {
                conn.disconnect()
            }
        } catch (e: Exception) {
            Log.d(KrunkApp.TAG, "suggest: ${e.message}")
            emptyList()
        }
    }
}
