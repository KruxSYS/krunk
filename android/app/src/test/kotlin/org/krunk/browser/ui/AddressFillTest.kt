package org.krunk.browser.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AddressFillTest {
    @Test
    fun nothingIsPendingWhenEditingStarts() {
        // What startEditing resets to: a new editor must keep the address it was opened with.
        assertFalse(AddressFill.NONE.appliesTo("https://example.com/"))
        assertFalse(AddressFill.NONE.appliesTo(""))
    }

    @Test
    fun aFillIsAppliedOnceAndCountsUp() {
        val first = AddressFill.NONE.next("krunk browser")
        assertEquals(1, first.sequence)
        assertTrue(first.appliesTo("kru"))
        // The editor already shows it: nothing to do.
        assertFalse(first.appliesTo("krunk browser"))
        val second = first.next("krunk browser android")
        assertEquals(2, second.sequence)
        assertTrue(second.appliesTo("krunk browser"))
    }

    @Test
    fun aFillFromAnEarlierSessionWouldHaveReplacedTheAddress() {
        // The 0.1.0 bug: the old fill stayed pending and the next editor took it.
        val stale = AddressFill.NONE.next("old suggestion")
        assertTrue(stale.appliesTo("https://example.com/"))
    }
}
