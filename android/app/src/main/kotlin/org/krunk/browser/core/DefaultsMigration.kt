package org.krunk.browser.core

import org.krunk.ffi.AndroidSettings
import org.krunk.ffi.SearchEngineId

/**
 * Moves installs that still carry an old default to the current one, once. The `kv` row
 * [KEY] records the defaults version a profile has been brought to; each step only touches
 * values that still equal the old default, so a user's own choice is never overwritten.
 */
object DefaultsMigration {
    const val KEY = "android.defaultsVersion"

    /** 1: DuckDuckGo engine and homepage → Google (0.7.0). */
    const val CURRENT = 1

    const val GOOGLE_HOMEPAGE = "https://www.google.com"

    private val OLD_HOMEPAGES = setOf("https://duckduckgo.com/", "https://duckduckgo.com")

    /** [settings] brought from defaults version [from] (0 when no marker is stored) to [CURRENT]. */
    fun migrate(settings: AndroidSettings, from: Int): AndroidSettings {
        var s = settings
        if (from < 1) {
            if (s.searchEngine == SearchEngineId.DUCK_DUCK_GO) s = s.copy(searchEngine = SearchEngineId.GOOGLE)
            if (s.homepage.trim() in OLD_HOMEPAGES) s = s.copy(homepage = GOOGLE_HOMEPAGE)
        }
        return s
    }
}
