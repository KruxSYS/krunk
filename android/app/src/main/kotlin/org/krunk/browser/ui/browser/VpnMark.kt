package org.krunk.browser.ui.browser

import org.krunk.browser.vpn.VpnPhase
import org.krunk.browser.vpn.VpnUiState

/**
 * The VPN mark in the address pill (0.10.0): the phase to draw, or null for none. Shown while
 * the VPN is on and "Show the VPN in the address bar" is set. Plain Kotlin (JVM test).
 */
fun vpnMark(state: VpnUiState): VpnPhase? =
    state.phase.takeIf { state.settings.enabled && state.settings.showInAddressBar && it != VpnPhase.OFF }
