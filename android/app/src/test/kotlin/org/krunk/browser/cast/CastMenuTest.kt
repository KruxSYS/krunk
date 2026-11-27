package org.krunk.browser.cast

import org.junit.Assert.assertEquals
import org.junit.Test

class CastMenuTest {
    @Test
    fun phoneWithPlayServicesHasBoth() {
        assertEquals(
            listOf(CastEntry.CAST, CastEntry.CAST_SCREEN),
            CastMenu.entries(isTv = false, castSdk = true, gmsAvailable = true),
        )
    }

    @Test
    fun noGmsBuildHasOnlyCastScreen() {
        assertEquals(
            listOf(CastEntry.CAST_SCREEN),
            CastMenu.entries(isTv = false, castSdk = false, gmsAvailable = false),
        )
        // Play Services on the device does not matter when the build has no Cast SDK.
        assertEquals(
            listOf(CastEntry.CAST_SCREEN),
            CastMenu.entries(isTv = false, castSdk = false, gmsAvailable = true),
        )
    }

    @Test
    fun gmsBuildWithoutPlayServicesHasOnlyCastScreen() {
        assertEquals(
            listOf(CastEntry.CAST_SCREEN),
            CastMenu.entries(isTv = false, castSdk = true, gmsAvailable = false),
        )
    }

    @Test
    fun tvHasNoCastEntries() {
        assertEquals(emptyList<CastEntry>(), CastMenu.entries(isTv = true, castSdk = true, gmsAvailable = true))
        assertEquals(emptyList<CastEntry>(), CastMenu.entries(isTv = true, castSdk = false, gmsAvailable = false))
    }
}
