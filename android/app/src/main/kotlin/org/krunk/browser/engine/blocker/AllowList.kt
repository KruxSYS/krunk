package org.krunk.browser.engine.blocker

import org.krunk.ffi.KrunkStore
import org.krunk.ffi.allowEntryFor
import org.krunk.ffi.allowHost
import org.krunk.ffi.hostOf

/**
 * The sites where blocking is switched off. Krunk owns the list (the shared `adblock_allowlist`
 * table, with the core's rules: an entry covers the host and its subdomains); the blocker gets a
 * copy as its "trusted sites" after every change and at every start.
 *
 * Private tabs offer no switch (plan 2.4): the blocker has one list for all tabs and stores it,
 * so an entry made in a private tab would outlast it on disk and would show in normal tabs.
 */
class AllowList(private val store: KrunkStore) {
    /** The entries, sorted. */
    fun hosts(): List<String> = runCatching { store.allowlist() }.getOrDefault(emptyList())

    /** The entry that covers the page at [url], if blocking is off for it. */
    fun entryFor(url: String): String? {
        val host = siteOf(url) ?: return null
        return allowEntryFor(hosts(), host)
    }

    /** Switches blocking off for the site of [urlOrHost]; returns the stored entry, or null. */
    fun allow(urlOrHost: String): String? {
        val host = siteOf(urlOrHost) ?: allowHost(urlOrHost) ?: return null
        return runCatching { store.allowlistAdd(host) }.getOrNull()
    }

    /** Removes an entry; false when it was not on the list. */
    fun remove(entry: String): Boolean = runCatching { store.allowlistRemove(entry) }.getOrDefault(false)

    companion object {
        /** `kv` key: when a user-triggered update last brought newer filter lists (Unix ms). */
        const val KV_LISTS_UPDATED = "android.blocker.lists_updated"

        /** The host a web page's allow entry is made for, or null (not a web page). */
        fun siteOf(url: String): String? {
            if (!url.startsWith("http://", ignoreCase = true) &&
                !url.startsWith("https://", ignoreCase = true)
            ) {
                return null
            }
            return hostOf(url)?.let { allowHost(it) }
        }

        /** Whether the sheet offers the switch: never in a private tab, only for web pages. */
        fun switchOffered(privateTab: Boolean, url: String): Boolean = !privateTab && siteOf(url) != null

        /**
         * The text of the blocker's trusted-sites list: its own default entries (which keep it
         * out of extension pages) first, then Krunk's hosts. Nothing else ever goes in.
         */
        fun trustedSites(blockerDefaults: List<String>, hosts: List<String>): String =
            (blockerDefaults.map { it.trim() }.filter { it.isNotEmpty() } + hosts.mapNotNull { allowHost(it) })
                .distinct()
                .joinToString("\n")

        /** Whether the blocker's list ([reported]) is the one Krunk sent. */
        fun sameSites(reported: List<String>, blockerDefaults: List<String>, hosts: List<String>): Boolean =
            reported.map { it.trim() }.filter { it.isNotEmpty() }.toSet() ==
                trustedSites(blockerDefaults, hosts).split('\n').filter { it.isNotEmpty() }.toSet()
    }
}
