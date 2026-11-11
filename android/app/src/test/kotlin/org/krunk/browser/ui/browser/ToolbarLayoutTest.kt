package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ToolbarLayoutTest {
    @Test
    fun onA360DpPhoneTheAddressPillGetsMoreThanSixtyPercent() {
        val layout = toolbarLayout(360)
        // Row padding 4 + 4, pill padding 4 + 4, tab counter 48, menu 48.
        assertEquals(112, layout.fixedDp)
        assertEquals(248, layout.pillDp)
        assertTrue("share ${pillShare(360)}", pillShare(360) >= 0.60f)
        assertEquals(0.689f, pillShare(360), 0.001f)
        assertFalse(layout.showHome)
        assertFalse(layout.showNotSecureWords)
    }

    @Test
    fun everyPhoneWidthKeepsTheMajorityForTheAddress() {
        // 320 dp is the narrowest phone Android supports in practice.
        for (width in listOf(320, 340, 360, 393, 411, 480, 599)) {
            assertTrue("$width dp: ${pillShare(width)}", pillShare(width) >= 0.60f)
            assertFalse(toolbarLayout(width).showHome)
        }
    }

    @Test
    fun fromTabletWidthHomeIsInTheToolbar() {
        val layout = toolbarLayout(600)
        assertTrue(layout.showHome)
        assertTrue(layout.showNotSecureWords)
        assertEquals(160, layout.fixedDp)
        assertEquals(440, layout.pillDp)
        assertTrue(pillShare(600) >= 0.60f)
        assertTrue(pillShare(1280) > 0.85f)
    }

    @Test
    fun aWidthThatIsNotThereYetGivesNoNegativeSizes() {
        assertEquals(0, toolbarLayout(0).pillDp)
        assertEquals(0f, pillShare(0), 0f)
        assertEquals(0, toolbarLayout(80).pillDp)
    }

    @Test
    fun theShieldAndReloadInsideThePillAreFullSizeTargets() {
        // Both sit inside the pill; with them the site still has room for a host on 360 dp.
        val text = toolbarLayout(360).pillDp - 2 * ToolbarLayout.BUTTON_DP
        assertEquals(48, ToolbarLayout.BUTTON_DP)
        assertTrue("room for the site: $text dp", text >= 140)
    }
}
