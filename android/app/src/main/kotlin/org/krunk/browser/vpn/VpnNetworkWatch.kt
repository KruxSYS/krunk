package org.krunk.browser.vpn

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network

/**
 * Tells the VPN when the default network changes (Wi-Fi ↔ mobile) or its addresses change
 * (plan §6.2), so the tunnel binds a new socket and handshakes at once. The callbacks Android
 * delivers right after registering (the current network) are not a change.
 */
class VpnNetworkWatch(private val onChange: () -> Unit) {
    private var network: Network? = null
    private var addresses: List<String>? = null

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            val before = synchronized(this@VpnNetworkWatch) {
                val old = this@VpnNetworkWatch.network
                this@VpnNetworkWatch.network = network
                if (old != network) addresses = null
                old
            }
            if (before != null && before != network) onChange()
        }

        override fun onLinkPropertiesChanged(network: Network, linkProperties: LinkProperties) {
            val now = linkProperties.linkAddresses.map { it.toString() }.sorted()
            val changed = synchronized(this@VpnNetworkWatch) {
                if (network != this@VpnNetworkWatch.network) return
                val old = addresses
                addresses = now
                old != null && old != now
            }
            if (changed) onChange()
        }
    }

    fun register(context: Context) {
        val cm = context.getSystemService(ConnectivityManager::class.java) ?: return
        runCatching { cm.registerDefaultNetworkCallback(callback) }
    }
}
