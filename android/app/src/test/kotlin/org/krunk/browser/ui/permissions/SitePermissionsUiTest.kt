package org.krunk.browser.ui.permissions

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.engine.permissions.PermissionDefault
import org.krunk.browser.engine.permissions.PermissionKind
import org.krunk.browser.ui.permissions.SitePermissionsUi.Choice
import org.krunk.ffi.SitePermission

class SitePermissionsUiTest {
    private val rows = listOf(
        SitePermission("https://b.example", "notifications", false),
        SitePermission("https://a.example", "sound", true),
        SitePermission("https://a.example", "geolocation", true),
        SitePermission("https://a.example", "camera", true),
        SitePermission("https://a.example", "notifications", false),
        SitePermission("http://a.example:8080", "camera", false),
    )

    @Test
    fun sitesAreGroupedSortedAndSummarised() {
        val sites = SitePermissionsUi.sites(rows)
        assertEquals(listOf("https://a.example", "http://a.example:8080", "https://b.example"), sites.map { it.origin })
        // Known kinds in the screens' order, unknown ones after them.
        assertEquals(listOf("camera", "geolocation", "sound"), sites[0].allowed)
        assertEquals(listOf("notifications"), sites[0].denied)
        assertEquals(emptyList<String>(), sites[1].allowed)
        assertEquals("a.example", sites[0].label)
        // A plain-http origin keeps its scheme in the label, so it cannot pass for the https one.
        assertEquals("http://a.example:8080", sites[1].label)
        assertTrue(SitePermissionsUi.sites(emptyList()).isEmpty())
    }

    @Test
    fun searchNarrowsTheList() {
        assertEquals(listOf("https://b.example"), SitePermissionsUi.sites(rows, " B.EXA ").map { it.origin })
        assertEquals(listOf("http://a.example:8080"), SitePermissionsUi.sites(rows, "8080").map { it.origin })
        assertTrue(SitePermissionsUi.sites(rows, "nothing").isEmpty())
        assertEquals(3, SitePermissionsUi.sites(rows, "  ").size)
    }

    @Test
    fun aSitesPageListsEveryKindAndWhatElseIsStored() {
        val detail = SitePermissionsUi.detail("https://a.example", rows)
        assertEquals(PermissionKind.entries.map { it.key } + "sound", detail.map { it.key })
        val byKey = detail.associateBy { it.key }
        assertEquals(Choice.ALLOW, byKey.getValue("camera").choice)
        assertEquals(Choice.DENY, byKey.getValue("notifications").choice)
        assertEquals(Choice.DEFAULT, byKey.getValue("microphone").choice)
        assertEquals(PermissionDefault.ASK, byKey.getValue("microphone").default)
        assertEquals(PermissionDefault.ALLOW, byKey.getValue("autoplayInaudible").default)
        assertEquals(PermissionDefault.DENY, byKey.getValue("autoplayAudible").default)
        assertEquals(PermissionDefault.DENY, byKey.getValue("xr").default)
        // The unknown kind: no kind, every choice, its stored value.
        val sound = byKey.getValue("sound")
        assertNull(sound.kind)
        assertEquals(Choice.ALLOW, sound.choice)
        assertEquals(Choice.entries, sound.choices)
        // Another origin's rows do not show.
        val other = SitePermissionsUi.detail("https://c.example", rows)
        assertEquals(PermissionKind.entries.size, other.size)
        assertTrue(other.all { it.choice == Choice.DEFAULT })
    }

    @Test
    fun anAllowThatCannotBeRememberedIsNotOffered() {
        val storage = SitePermissionsUi.detail("https://a.example", rows).first { it.key == "storageAccess" }
        assertEquals(listOf(Choice.DENY, Choice.DEFAULT), storage.choices)
        // Unless it is stored (by another version): then it shows and can be changed.
        val stored = SitePermissionsUi.detail(
            "https://a.example",
            listOf(SitePermission("https://a.example", "storageAccess", true)),
        ).first { it.key == "storageAccess" }
        assertEquals(Choice.ALLOW, stored.choice)
        assertEquals(listOf(Choice.ALLOW, Choice.DENY, Choice.DEFAULT), stored.choices)
    }

    @Test
    fun choicesMapToWhatIsStored() {
        assertEquals(true, SitePermissionsUi.valueOf(Choice.ALLOW))
        assertEquals(false, SitePermissionsUi.valueOf(Choice.DENY))
        assertNull(SitePermissionsUi.valueOf(Choice.DEFAULT))
        assertTrue(SitePermissionsUi.isSite("https://a.example"))
        assertTrue(SitePermissionsUi.isSite("http://10.0.2.2:18080"))
        assertFalse(SitePermissionsUi.isSite(null))
        assertFalse(SitePermissionsUi.isSite("about:blank"))
    }
}
