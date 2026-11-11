package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class LruMapTest {
    @Test
    fun evictsTheOldestStoredEntry() {
        val evicted = ArrayList<String>()
        val m = LruMap<Int, String>(3).apply { onEvict = { evicted += it } }
        m[1] = "a"
        m[2] = "b"
        m[3] = "c"
        assertEquals(3, m.size)
        // Reading does not refresh an entry.
        assertEquals("a", m[1])
        m[4] = "d"
        assertNull(m[1])
        assertEquals(listOf("a"), evicted)
        assertEquals(setOf(2, 3, 4), m.keys())
    }

    @Test
    fun storingAgainMovesToNewestAndReportsTheReplacedValue() {
        val evicted = ArrayList<String>()
        val m = LruMap<Int, String>(2).apply { onEvict = { evicted += it } }
        m[1] = "a"
        m[2] = "b"
        m[1] = "a2"
        assertEquals(listOf("a"), evicted)
        m[3] = "c"
        // 2 was the oldest store now.
        assertNull(m[2])
        assertEquals("a2", m[1])
        assertEquals(listOf("a", "b"), evicted)
    }

    @Test
    fun removeAndClearReportEvictions() {
        val evicted = ArrayList<String>()
        val m = LruMap<Int, String>(5).apply { onEvict = { evicted += it } }
        m[1] = "a"
        m[2] = "b"
        assertEquals("a", m.remove(1))
        assertNull(m.remove(1))
        m.clear()
        assertEquals(0, m.size)
        assertEquals(listOf("a", "b"), evicted)
    }
}
