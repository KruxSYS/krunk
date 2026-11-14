package org.krunk.browser.notify

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationBookTest {
    private class Note(val text: String)

    private val base = Channels.ID_SITE_BASE
    private val site = "https://a.test"

    @Test
    fun idsCountUpFromTheBase() {
        val book = NotificationBook<Note>(base)
        assertEquals(base, book.show(site, "", false, Note("1")).id)
        assertEquals(base + 1, book.show(site, "", false, Note("2")).id)
        assertEquals(base + 2, book.show("https://b.test", "t", false, Note("3")).id)
        assertEquals(3, book.size)
        // Never in the range of downloads.
        assertTrue(base > Channels.ID_DOWNLOAD_BASE)
    }

    @Test
    fun theSameSiteAndTagReplace() {
        val book = NotificationBook<Note>(base)
        val first = Note("first")
        val a = book.show(site, "chat", false, first)
        assertNull(a.replaced)
        val second = Note("second")
        val b = book.show(site, "chat", false, second)
        assertEquals(a.id, b.id)
        assertSame(first, b.replaced)
        assertSame(second, book[a.id]?.notification)
        assertEquals(1, book.size)
        // Another site, a private page or another tag is another notification.
        assertNotEquals(a.id, book.show("https://b.test", "chat", false, Note("x")).id)
        assertNotEquals(a.id, book.show(site, "chat", true, Note("y")).id)
        assertNotEquals(a.id, book.show(site, "mail", false, Note("z")).id)
        // Without a tag nothing is replaced.
        assertNotEquals(book.show(site, "", false, Note("p")).id, book.show(site, "", false, Note("q")).id)
    }

    @Test
    fun closingFindsTheEntryByObjectOrBySiteAndTag() {
        val book = NotificationBook<Note>(base)
        val untagged = Note("u")
        val tagged = Note("t")
        val u = book.show(site, "", false, untagged).id
        val t = book.show(site, "chat", false, tagged).id
        assertEquals(u, book.find(untagged, site, "", false)?.id)
        // A copy of the tagged one.
        assertEquals(t, book.find(Note("copy"), site, "chat", false)?.id)
        // A copy of an untagged one cannot be told apart: nothing is closed by guess.
        assertNull(book.find(Note("copy"), site, "", false))
        assertNull(book.find(Note("copy"), site, "chat", true))
    }

    @Test
    fun aTapOrSwipeEndsTheEntry() {
        val book = NotificationBook<Note>(base)
        val note = Note("n")
        val id = book.show(site, "chat", false, note).id
        assertSame(note, book.remove(id)?.notification)
        assertNull(book.remove(id))
        assertNull(book[id])
        // The tag is free again: a later one is new, not a replacement.
        assertNull(book.show(site, "chat", false, Note("m")).replaced)
    }

    @Test
    fun privateNotificationsEndWithThePrivateSession() {
        val book = NotificationBook<Note>(base)
        val normal = book.show(site, "a", false, Note("n")).id
        val p1 = book.show(site, "a", true, Note("p1")).id
        val p2 = book.show("https://b.test", "", true, Note("p2")).id
        assertEquals(listOf(p1, p2), book.removePrivate().map { it.id })
        assertEquals(1, book.size)
        assertEquals(site, book[normal]?.origin)
        assertTrue(book.removePrivate().isEmpty())
    }

    @Test
    fun idsStillInTheShadeAreNotHandedOut() {
        val book = NotificationBook<Note>(base)
        book.reserve(listOf(base, base + 1))
        assertEquals(base + 2, book.show(site, "", false, Note("1")).id)
        book.release(base)
        val small = NotificationBook<Note>(base, span = 3)
        small.reserve(listOf(base + 1))
        assertEquals(base, small.show(site, "", false, Note("a")).id)
        assertEquals(base + 2, small.show(site, "", false, Note("b")).id)
        // Full: the leftovers give way first, then the oldest entry.
        assertEquals(base + 1, small.show(site, "", false, Note("c")).id)
        val next = small.show(site, "", false, Note("d")).id
        assertEquals(base, next)
        assertEquals(3, small.size)
        assertEquals("d", small[base]?.notification?.text)
    }

    @Test
    fun aSiteCannotPostWithoutBound() {
        val book = NotificationBook<Note>(base, maxPerSite = 3, maxLive = 5)
        val first = book.show(site, "", false, Note("1"))
        book.show(site, "", false, Note("2"))
        book.show(site, "", false, Note("3"))
        // The fourth of the same site: the oldest of that site gives way and is handed back.
        val fourth = book.show(site, "", false, Note("4"))
        assertEquals(listOf("1"), fourth.evicted.map { it.notification.text })
        assertNull(book[first.id])
        assertEquals(3, book.size)
        assertEquals(listOf("2"), book.show(site, "t", false, Note("tagged")).evicted.map { it.notification.text })
        // A replacement (same tag) is no new notification: nothing gives way.
        assertTrue(book.show(site, "t", false, Note("tagged again")).evicted.isEmpty())
        // The same site in a private tab, and other sites, count on their own; all together are capped.
        assertTrue(book.show(site, "", true, Note("p")).evicted.isEmpty())
        assertTrue(book.show("https://b.test", "", false, Note("b")).evicted.isEmpty())
        assertEquals(5, book.size)
        val more = book.show("https://c.test", "", false, Note("c"))
        assertEquals(listOf("3"), more.evicted.map { it.notification.text })
        for (i in 0 until 20) book.show("https://c$i.test", "", false, Note("c$i"))
        assertEquals(5, book.size)
    }
}
