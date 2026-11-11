package org.krunk.browser.engine.blocker

/**
 * What the blocker reports for one tab's page. It arrives as the title of the blocker's toolbar
 * action, which the blocker sets per tab: "uBlock Origin (12)", "uBlock Origin (off)" on a site
 * where blocking is switched off, "uBlock Origin (!)" for a page that loaded before the blocker
 * was ready.
 */
data class Shield(
    /** Requests blocked on this page, as the blocker formats it ("0", "12", "1.2k"). */
    val count: String = "0",
    /** Blocking is off for this page's site. */
    val off: Boolean = false,
    /** The page loaded before the blocker was filtering; it was not checked. */
    val unfiltered: Boolean = false,
)

object ShieldTitle {
    private val VALUE = Regex("""\(([^()]*)\)\s*$""")

    /** The reading in an action title, or null when the title carries none (the default title). */
    fun parse(title: String?): Shield? {
        val value = VALUE.find(title ?: return null)?.groupValues?.get(1)?.trim() ?: return null
        return when {
            value.equals("off", ignoreCase = true) -> Shield(count = "", off = true)
            value == "!" -> Shield(count = "", unfiltered = true)
            value.isEmpty() -> Shield()
            value.length <= MAX_COUNT && value.first().isDigit() -> Shield(count = value)
            else -> null
        }
    }

    private const val MAX_COUNT = 8
}

/**
 * The readings per tab. A tab's entry starts again with every new document, repeats change
 * nothing (the blocker sends many), and a reading is only ever read for its own tab, so a count
 * cannot leak from one tab to another. Main thread only.
 */
class ShieldStore {
    private val byTab = HashMap<UInt, Shield>()

    /** A new action title for [tab]. Returns true when the tab's reading changed. */
    fun title(tab: UInt, title: String?): Boolean {
        val reading = ShieldTitle.parse(title) ?: return false
        if (byTab[tab] == reading) return false
        byTab[tab] = reading
        return true
    }

    /** A new top-level document started in [tab]: the previous page's count is not its count. */
    fun newDocument(tab: UInt): Boolean = byTab.remove(tab) != null

    /** The tab is gone (closed, unloaded). */
    fun forget(tab: UInt): Boolean = byTab.remove(tab) != null

    /** Every reading is void (the blocker was switched off or restarted). */
    fun clear(): Boolean {
        if (byTab.isEmpty()) return false
        byTab.clear()
        return true
    }

    fun of(tab: UInt): Shield? = byTab[tab]

    fun snapshot(): Map<UInt, Shield> = HashMap(byTab)
}
