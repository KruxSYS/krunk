package org.krunk.browser.tabs

/**
 * A small map that keeps at most [capacity] entries, dropping the least recently *stored* one.
 * Reads do not reorder (the tab list reads every entry on each refresh, which would defeat an
 * access-ordered cache); storing an existing key moves it to the newest position.
 */
class LruMap<K, V>(val capacity: Int) {
    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    private val map = LinkedHashMap<K, V>()

    /** Called with each evicted or replaced value (to free native memory). */
    var onEvict: ((V) -> Unit)? = null

    val size: Int get() = map.size

    operator fun get(key: K): V? = map[key]

    operator fun set(key: K, value: V) {
        map.remove(key)?.let { old -> if (old !== value) onEvict?.invoke(old) }
        map[key] = value
        while (map.size > capacity) {
            val eldest = map.entries.iterator().next()
            map.remove(eldest.key)
            onEvict?.invoke(eldest.value)
        }
    }

    fun remove(key: K): V? = map.remove(key)?.also { onEvict?.invoke(it) }

    fun clear() {
        val values = map.values.toList()
        map.clear()
        values.forEach { onEvict?.invoke(it) }
    }

    fun keys(): Set<K> = map.keys.toSet()
}
