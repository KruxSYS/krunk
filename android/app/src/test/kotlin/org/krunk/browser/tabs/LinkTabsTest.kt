package org.krunk.browser.tabs

import org.junit.Assert.assertEquals
import org.junit.Test
import org.krunk.ffi.LinkTabsMode

class LinkTabsTest {
    @Test
    fun linkTabsComeToTheFrontAsTheSettingSays() {
        val table = listOf(
            // setting, how the tab was opened, in front?
            Triple(LinkTabsMode.AUTO, LinkOpen.LINK_MENU, false),
            Triple(LinkTabsMode.AUTO, LinkOpen.PAGE_WINDOW, true),
            Triple(LinkTabsMode.SWITCH, LinkOpen.LINK_MENU, true),
            Triple(LinkTabsMode.SWITCH, LinkOpen.PAGE_WINDOW, true),
            Triple(LinkTabsMode.BACKGROUND, LinkOpen.LINK_MENU, false),
            Triple(LinkTabsMode.BACKGROUND, LinkOpen.PAGE_WINDOW, false),
        )
        for ((mode, how, front) in table) assertEquals("$mode $how", front, opensInFront(mode, how))
    }
}
