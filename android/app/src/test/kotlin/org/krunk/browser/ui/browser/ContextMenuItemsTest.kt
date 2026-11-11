package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.ContextTarget
import org.mozilla.geckoview.GeckoSession.ContentDelegate.ContextElement

class ContextMenuItemsTest {
    private fun link(uri: String, text: String? = "text") =
        ContextTarget(uri, null, null, null, text, ContextElement.TYPE_NONE)

    private fun image(src: String, link: String? = null) =
        ContextTarget(link, src, null, "alt", null, ContextElement.TYPE_IMAGE)

    @Test
    fun webLinksOfferEverything() {
        assertEquals(
            listOf(
                ContextAction.OPEN_NEW_TAB,
                ContextAction.OPEN_PRIVATE_TAB,
                ContextAction.COPY_LINK,
                ContextAction.SHARE_LINK,
                ContextAction.COPY_LINK_TEXT,
                ContextAction.DOWNLOAD_LINK,
            ),
            contextActions(link("https://example.com/a"), privateTab = false),
        )
        assertFalse(ContextAction.OPEN_PRIVATE_TAB in contextActions(link("https://example.com/a"), privateTab = true))
    }

    @Test
    fun aPageCannotGetOtherSchemesOpenedOrDownloaded() {
        for (uri in listOf(
            "file:///data/data/org.krunk.browser/files/krunk/browser.db",
            "data:text/html,<script>alert(1)</script>",
            "blob:https://example.com/1234",
            "resource://android/assets/",
            "about:config",
            "https://user:pw@example.com/",
        )) {
            val actions = contextActions(link(uri), privateTab = false)
            assertFalse(uri, ContextAction.OPEN_NEW_TAB in actions)
            assertFalse(uri, ContextAction.OPEN_PRIVATE_TAB in actions)
            assertFalse(uri, ContextAction.DOWNLOAD_LINK in actions)
        }
        val img = contextActions(image("data:image/png;base64,AAAA"), privateTab = false)
        assertTrue(img.isEmpty())
    }

    @Test
    fun mailAndPhoneLinksCanBeCopied() {
        assertEquals(
            listOf(ContextAction.COPY_LINK, ContextAction.SHARE_LINK, ContextAction.COPY_LINK_TEXT),
            contextActions(link("mailto:someone@example.com"), privateTab = false),
        )
        assertEquals(
            listOf(ContextAction.COPY_LINK, ContextAction.SHARE_LINK),
            contextActions(link("tel:+491234", text = null), privateTab = false),
        )
    }

    @Test
    fun scriptLinksOfferNothing() {
        assertTrue(contextActions(link("javascript:void(0)"), privateTab = false).isEmpty())
        assertTrue(contextActions(link(" JavaScript:alert(1)"), privateTab = false).isEmpty())
    }

    @Test
    fun imagesInsideLinksAddTheImageRows() {
        val actions = contextActions(image("https://example.com/a.png", link = "https://example.com/"), false)
        assertTrue(ContextAction.OPEN_NEW_TAB in actions)
        assertEquals(
            listOf(ContextAction.OPEN_IMAGE_NEW_TAB, ContextAction.SAVE_IMAGE, ContextAction.COPY_IMAGE_LINK),
            actions.takeLast(3),
        )
    }
}
