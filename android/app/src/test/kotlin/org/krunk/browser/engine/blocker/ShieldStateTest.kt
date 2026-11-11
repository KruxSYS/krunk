package org.krunk.browser.engine.blocker

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ShieldStateTest {
    @Test
    fun theBlockersActionTitleIsReadAsCountOffOrUnfiltered() {
        val cases = listOf(
            "uBlock Origin (0)" to Shield(count = "0"),
            "uBlock Origin (5)" to Shield(count = "5"),
            "uBlock Origin (999)" to Shield(count = "999"),
            "uBlock Origin (1.2k)" to Shield(count = "1.2k"),
            "uBlock Origin (99k+)" to Shield(count = "99k+"),
            "uBlock Origin (off)" to Shield(count = "", off = true),
            "uBlock Origin (OFF) " to Shield(count = "", off = true),
            "uBlock Origin (!)" to Shield(count = "", unfiltered = true),
            // The plain default title and anything unexpected carry no reading.
            "uBlock Origin" to null,
            "" to null,
            "uBlock Origin (five)" to null,
            "uBlock Origin (123456789)" to null,
            "something (else) entirely" to null,
        )
        for ((title, want) in cases) assertEquals(title, want, ShieldTitle.parse(title))
        assertNull(ShieldTitle.parse(null))
    }

    @Test
    fun aCountBelongsToItsTabAndRepeatsChangeNothing() {
        val store = ShieldStore()
        assertTrue(store.title(1u, "uBlock Origin (0)"))
        assertTrue(store.title(1u, "uBlock Origin (5)"))
        // The blocker repeats itself many times per page.
        assertFalse(store.title(1u, "uBlock Origin (5)"))
        assertFalse(store.title(1u, "uBlock Origin"))
        assertEquals(Shield(count = "5"), store.of(1u))
        // Another tab has its own reading, and none before the blocker reported one.
        assertNull(store.of(2u))
        assertTrue(store.title(2u, "uBlock Origin (off)"))
        assertEquals(Shield(count = "", off = true), store.of(2u))
        assertEquals(Shield(count = "5"), store.of(1u))
        assertEquals(mapOf(1u to Shield("5"), 2u to Shield("", off = true)), store.snapshot())
    }

    @Test
    fun aNewDocumentStartsTheTabsCountAgain() {
        val store = ShieldStore()
        store.title(1u, "uBlock Origin (12)")
        store.title(2u, "uBlock Origin (3)")
        assertTrue(store.newDocument(1u))
        assertNull(store.of(1u))
        // The other tab keeps its count; a second "new document" has nothing to reset.
        assertEquals(Shield("3"), store.of(2u))
        assertFalse(store.newDocument(1u))
        // The new page's own count arrives.
        assertTrue(store.title(1u, "uBlock Origin (0)"))
        assertEquals(Shield("0"), store.of(1u))
    }

    @Test
    fun closedTabsAndASwitchedOffBlockerLeaveNoReadings() {
        val store = ShieldStore()
        store.title(1u, "uBlock Origin (12)")
        store.title(2u, "uBlock Origin (3)")
        assertTrue(store.forget(1u))
        assertFalse(store.forget(1u))
        assertEquals(setOf(2u), store.snapshot().keys)
        assertTrue(store.clear())
        assertFalse(store.clear())
        assertTrue(store.snapshot().isEmpty())
    }
}
