package org.krunk.browser.ui.browser

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.krunk.browser.vpn.VpnPhase
import org.krunk.browser.vpn.VpnSettings
import org.krunk.browser.vpn.VpnUiState

class VpnMarkTest {
    private fun state(enabled: Boolean, show: Boolean, phase: VpnPhase) =
        VpnUiState(settings = VpnSettings(enabled = enabled, showInAddressBar = show), phase = phase)

    @Test
    fun shownWhileOnAndWanted() {
        assertEquals(VpnPhase.CONNECTED, vpnMark(state(true, true, VpnPhase.CONNECTED)))
        assertEquals(VpnPhase.CONNECTING, vpnMark(state(true, true, VpnPhase.CONNECTING)))
        assertEquals(VpnPhase.FAILED, vpnMark(state(true, true, VpnPhase.FAILED)))
    }

    @Test
    fun hiddenWhenOffOrNotWanted() {
        assertNull(vpnMark(state(false, true, VpnPhase.OFF)))
        assertNull(vpnMark(state(true, false, VpnPhase.CONNECTED)))
        assertNull(vpnMark(state(true, true, VpnPhase.OFF)))
    }
}
