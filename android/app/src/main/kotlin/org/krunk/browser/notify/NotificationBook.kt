package org.krunk.browser.notify

/**
 * Which Android notification stands for which site notification. Pure bookkeeping ([N] is the
 * engine's notification object): ids are counted up from [base] within [span] and never handed
 * out twice while in use; a notification with the same site, tag and privacy as an earlier one
 * takes that one's id, so Android replaces it.
 *
 * The book lives in memory. After the process died the notifications still in the shade have
 * no entry here: a tap then only opens the site (see `SiteNotifications`).
 */
class NotificationBook<N>(
    private val base: Int,
    private val span: Int = DEFAULT_SPAN,
    /** Live notifications of one site (and privacy) at most: the oldest gives way. */
    private val maxPerSite: Int = MAX_PER_SITE,
    /** Live site notifications at most, of all sites together. */
    private val maxLive: Int = MAX_LIVE,
) {
    data class Entry<N>(val id: Int, val origin: String?, val tag: String, val private: Boolean, val notification: N)

    /**
     * [replaced] is the earlier notification under the same id, whose place this one took;
     * [evicted] are older ones that had to go for the limits (to be cancelled and dismissed).
     */
    data class Shown<N>(val id: Int, val replaced: N?, val evicted: List<Entry<N>> = emptyList())

    private val byId = LinkedHashMap<Int, Entry<N>>()
    private val reserved = HashSet<Int>()
    private var next = 0

    val size: Int get() = byId.size

    /** Ids of notifications an earlier process left in the shade: not handed out again. */
    fun reserve(ids: Collection<Int>) {
        reserved += ids
    }

    /** That leftover is gone from the shade. */
    fun release(id: Int) {
        reserved -= id
    }

    private fun sameSlot(e: Entry<N>, origin: String?, tag: String, private: Boolean) =
        tag.isNotEmpty() && e.tag == tag && e.origin == origin && e.private == private

    fun show(origin: String?, tag: String, private: Boolean, notification: N): Shown<N> {
        val earlier = byId.values.firstOrNull { sameSlot(it, origin, tag, private) }
        val evicted = ArrayList<Entry<N>>()
        if (earlier == null) {
            // A site cannot post without bound (Android would drop what is over its per-app
            // limit silently, and the downloads' notifications need room too).
            val ofSite = byId.values.filter { it.origin == origin && it.private == private }
            ofSite.take((ofSite.size - maxPerSite + 1).coerceAtLeast(0)).forEach { evicted += it }
            val others = byId.values.filter { it !in evicted }
            others.take((others.size - maxLive + 1).coerceAtLeast(0)).forEach { evicted += it }
            evicted.forEach { byId.remove(it.id) }
        }
        val id = earlier?.id ?: freshId()
        byId[id] = Entry(id, origin, tag, private, notification)
        return Shown(id, earlier?.notification, evicted)
    }

    private fun freshId(): Int {
        // More than `span` at once cannot be: the oldest gives way.
        if (byId.size + reserved.size >= span) {
            reserved.clear()
            if (byId.size >= span) byId.remove(byId.keys.first())
        }
        while (true) {
            val id = base + next
            next = (next + 1) % span
            if (id !in byId && id !in reserved) return id
        }
    }

    operator fun get(id: Int): Entry<N>? = byId[id]

    /** The notification was tapped, swiped away or closed: its entry ends. */
    fun remove(id: Int): Entry<N>? = byId.remove(id)

    /**
     * The entry of the notification the engine closes: the very object, else the one with the
     * same site, tag and privacy (the engine may hand a copy).
     */
    fun find(notification: N, origin: String?, tag: String, private: Boolean): Entry<N>? =
        byId.values.firstOrNull { it.notification === notification }
            ?: byId.values.firstOrNull { sameSlot(it, origin, tag, private) }

    /** The private session ended: its notifications, removed from the book. */
    fun removePrivate(): List<Entry<N>> {
        val gone = byId.values.filter { it.private }
        gone.forEach { byId.remove(it.id) }
        return gone
    }

    companion object {
        const val DEFAULT_SPAN = 100_000
        const val MAX_PER_SITE = 10
        const val MAX_LIVE = 40
    }
}
