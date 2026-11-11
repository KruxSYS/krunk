package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.blocker.BlockerStatus
import org.krunk.browser.engine.blocker.Shield
import org.krunk.browser.engine.blocker.ShieldStore

class ShieldUiTest {
    private val page = "https://news.example.com/article"

    private fun ui(
        status: BlockerStatus = BlockerStatus.READY,
        shield: Shield? = null,
        allowed: String? = null,
        url: String = page,
        isPage: Boolean = true,
        unprotected: Boolean = false,
    ) = shieldUi(url, isPage, status, shield, allowed, unprotected)

    @Test
    fun aFilteredPageShowsItsCount() {
        assertEquals(ShieldUi(ShieldMode.ACTIVE, "5"), ui(shield = Shield("5")))
        assertEquals(ShieldUi(ShieldMode.ACTIVE, "1.2k"), ui(shield = Shield("1.2k")))
        // Before the blocker has reported on a new page, and when nothing was blocked: zero,
        // and no number next to the icon.
        assertEquals(ShieldUi(ShieldMode.ACTIVE, "0"), ui(shield = null))
        assertFalse(ui(shield = Shield("0")).showCount)
        assertFalse(ui(shield = null).showCount)
        assertTrue(ui(shield = Shield("5")).showCount)
    }

    @Test
    fun allowedOffAndUnavailableAreThreeDifferentStates() {
        val allowed = ui(shield = Shield("", off = true), allowed = "example.com")
        val off = ui(status = BlockerStatus.OFF, shield = Shield("7"))
        val unavailable = ui(status = BlockerStatus.UNAVAILABLE)
        assertEquals(ShieldMode.ALLOWED, allowed.mode)
        assertEquals(ShieldMode.OFF, off.mode)
        assertEquals(ShieldMode.UNAVAILABLE, unavailable.mode)
        assertEquals(3, setOf(allowed.mode, off.mode, unavailable.mode).size)
        // None of them shows a count, least of all a stale one.
        for (state in listOf(allowed, off, unavailable)) {
            assertEquals("", state.count)
            assertFalse(state.showCount)
        }
    }

    @Test
    fun theAllowedStateShowsAtOnceAndAfterTheReload() {
        // Right after the switch: Krunk's list says allowed, the blocker still reports a count.
        assertEquals(ShieldMode.ALLOWED, ui(shield = Shield("5"), allowed = "example.com").mode)
        // After the reload the blocker says "off" itself.
        assertEquals(ShieldMode.ALLOWED, ui(shield = Shield("", off = true), allowed = "example.com").mode)
        // Removed from the list, reloaded: counting again.
        assertEquals(ShieldUi(ShieldMode.ACTIVE, "3"), ui(shield = Shield("3"), allowed = null))
    }

    @Test
    fun pagesThatAreNotWebPagesHaveNoShield() {
        assertEquals(ShieldMode.HIDDEN, ui(isPage = false).mode)
        assertEquals(ShieldMode.HIDDEN, ui(url = "").mode)
        assertEquals(ShieldMode.HIDDEN, ui(url = "about:blank").mode)
        assertEquals(ShieldMode.HIDDEN, ui(url = "krunk://settings").mode)
        // Also when blocking is off or broken: nothing to say about such a page.
        assertEquals(ShieldMode.HIDDEN, ui(status = BlockerStatus.OFF, isPage = false).mode)
    }

    @Test
    fun aPageThatLoadedBeforeTheBlockerWasReadyIsMarked() {
        assertEquals(ShieldMode.STARTING, ui(status = BlockerStatus.STARTING).mode)
        // Ten seconds passed and pages went ahead: a warning until the blocker reports.
        assertEquals(ShieldMode.UNFILTERED, ui(status = BlockerStatus.STARTING, unprotected = true).mode)
        // The blocker itself marks such a page.
        assertEquals(ShieldMode.UNFILTERED, ui(shield = Shield("", unfiltered = true)).mode)
        // A page loaded later, with the blocker running, is a normal page again.
        assertEquals(ShieldMode.ACTIVE, ui(shield = Shield("2"), unprotected = true).mode)
    }

    @Test
    fun theCountFollowsTheTabAndStartsAgainWithEachDocument() {
        val store = ShieldStore()
        store.title(1u, "uBlock Origin (12)")
        store.title(2u, "uBlock Origin (3)")
        // Each tab shows its own number.
        assertEquals("12", ui(shield = store.of(1u)).count)
        assertEquals("3", ui(shield = store.of(2u)).count)
        // Tab 1 navigates: zero at once, never tab 2's or its own old number.
        store.newDocument(1u)
        assertEquals(ShieldUi(ShieldMode.ACTIVE, "0"), ui(shield = store.of(1u)))
        store.title(1u, "uBlock Origin (4)")
        assertEquals("4", ui(shield = store.of(1u)).count)
        assertEquals("3", ui(shield = store.of(2u)).count)
        // A tab the blocker never reported on (just opened) shows zero.
        assertEquals("0", ui(shield = store.of(9u)).count)
    }

    @Test
    fun privateTabsGetAHintInsteadOfTheSwitch() {
        val active = ui(shield = Shield("5"))
        val allowed = ui(allowed = "example.com")
        assertEquals(AllowSwitch.BLOCKING, allowSwitch(active, privateTab = false))
        assertEquals(AllowSwitch.ALLOWED, allowSwitch(allowed, privateTab = false))
        // A site allowed from a normal tab is allowed in private tabs too; it cannot be changed there.
        assertEquals(AllowSwitch.PRIVATE_TAB, allowSwitch(active, privateTab = true))
        assertEquals(AllowSwitch.PRIVATE_TAB, allowSwitch(allowed, privateTab = true))
        // Nothing to switch when blocking is off altogether or broken.
        for (status in listOf(BlockerStatus.OFF, BlockerStatus.UNAVAILABLE)) {
            assertEquals(AllowSwitch.NONE, allowSwitch(ui(status = status), privateTab = false))
            assertEquals(AllowSwitch.NONE, allowSwitch(ui(status = status), privateTab = true))
        }
        assertEquals(AllowSwitch.NONE, allowSwitch(ui(isPage = false), privateTab = false))
    }
}
