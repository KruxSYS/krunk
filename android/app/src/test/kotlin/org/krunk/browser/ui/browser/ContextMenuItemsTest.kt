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

    @Test
    fun fileNamesComeFromTheHeaderOrTheAddress() {
        assertEquals("report.pdf", downloadFileName("https://example.com/files/report.pdf?x=1#y", null))
        assertEquals("a b.txt", downloadFileName("https://example.com/a%20b.txt", null))
        assertEquals("download", downloadFileName("https://example.com/", null))
        assertEquals("invoice.pdf", downloadFileName("https://example.com/get", "attachment; filename=\"invoice.pdf\""))
        assertEquals("invoice.pdf", downloadFileName("https://example.com/get", "attachment; FILENAME=invoice.pdf"))
        assertEquals(
            "naïve file.txt",
            downloadFileName(
                "https://example.com/get",
                "attachment; filename=x.txt; filename*=utf-8''na%C3%AFve%20file.txt",
            ),
        )
    }

    @Test
    fun fileNamesCannotLeaveTheFolderOrHide() {
        assertEquals(
            "_.._etc_passwd",
            downloadFileName("https://e.example/x", "attachment; filename=\"../../etc/passwd\""),
        )
        val traversal = downloadFileName("https://e.example/x", "attachment; filename=\"..\\\\..\\\\boot.ini\"")
        assertFalse(traversal.contains('/') || traversal.contains('\\'))
        assertFalse(traversal.startsWith("."))
        assertEquals("bashrc", downloadFileName("https://e.example/x", "attachment; filename=\".bashrc\""))
        assertEquals("download", downloadFileName("https://e.example/x", "attachment; filename=\"...\""))
        // Trailing dots and spaces are dropped (they hide the real extension on some systems).
        assertEquals("setup.exe", downloadFileName("https://e.example/x", "attachment; filename=\"setup.exe. .\""))
    }

    @Test
    fun fileNamesCannotReorderOrHideTheirExtension() {
        // A right-to-left override would show "photo‮gpj.apk" as "photoapk.jpg".
        assertEquals(
            "photogpj.apk",
            downloadFileName("https://e.example/x", "attachment; filename=\"photo\u202Egpj.apk\""),
        )
        assertEquals(
            "update.apk",
            downloadFileName("https://e.example/x", "attachment; filename=\"up\u200Bdate\u0007.apk\""),
        )
    }

    @Test
    fun longFileNamesKeepTheirEndAndFitInBytes() {
        val long = "文".repeat(300) + ".pdf" // 3 bytes per character
        val name = downloadFileName("https://e.example/x", "attachment; filename=\"$long\"")
        assertTrue(name.endsWith(".pdf"))
        assertTrue(name.toByteArray(Charsets.UTF_8).size <= 200)
        // A surrogate pair is never cut in two.
        val emoji = "😀".repeat(100) + ".png"
        val cut = downloadFileName("https://e.example/x", "attachment; filename=\"$emoji\"")
        assertTrue(cut.endsWith(".png"))
        assertFalse(cut.first().isLowSurrogate())
        assertTrue(cut.toByteArray(Charsets.UTF_8).size <= 200)
    }
}
