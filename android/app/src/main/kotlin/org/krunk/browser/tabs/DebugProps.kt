package org.krunk.browser.tabs

import org.krunk.browser.BuildConfig

/**
 * Debug-only knobs set with `adb shell setprop debug.krunk.<name> <value>`; always absent in
 * release builds. `debug.krunk.sleep_seconds N` shrinks the memory-policy timers (see
 * [MemoryTicker.policyFor]); `debug.krunk.discard_test 1` turns discard on for that test.
 */
object DebugProps {
    const val SLEEP_SECONDS = "debug.krunk.sleep_seconds"
    const val DISCARD_TEST = "debug.krunk.discard_test"

    fun get(name: String): String? {
        if (!BuildConfig.DEBUG) return null
        return runCatching {
            val cls = Class.forName("android.os.SystemProperties")
            val m = cls.getMethod("get", String::class.java, String::class.java)
            (m.invoke(null, name, "") as? String)?.takeIf { it.isNotEmpty() }
        }.getOrNull()
    }

    fun long(name: String): Long? = get(name)?.trim()?.toLongOrNull()

    fun flag(name: String): Boolean = get(name)?.trim().let { it == "1" || it == "true" }
}
