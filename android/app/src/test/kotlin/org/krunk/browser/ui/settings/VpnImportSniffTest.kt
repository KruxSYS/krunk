package org.krunk.browser.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class VpnImportSniffTest {
    @Test
    fun wireGuard() {
        val text = """
            [Interface]
            PrivateKey = aGVsbG8=
            Address = 10.0.0.2/32

            [Peer]
            PublicKey = d29ybGQ=
            Endpoint = vpn.example:51820
        """.trimIndent()
        assertEquals(VpnImportSniff.Kind.WIREGUARD, VpnImportSniff.kind(text))
        assertEquals(VpnImportSniff.Kind.WIREGUARD, VpnImportSniff.kind("\uFEFF[interface]\r\nprivatekey=x\r\n"))
    }

    @Test
    fun openVpn() {
        assertEquals(VpnImportSniff.Kind.OPENVPN, VpnImportSniff.kind("client\ndev tun\nremote vpn.example 1194 udp\n"))
        assertEquals(VpnImportSniff.Kind.OPENVPN, VpnImportSniff.kind("dev tun\nremote 1.2.3.4\n<ca>\n</ca>\n"))
    }

    @Test
    fun ordinaryTextIsNotAProfile() {
        assertNull(VpnImportSniff.kind("https://example.com/page"))
        assertNull(VpnImportSniff.kind("remote work is great\nfor every client"))
        assertNull(VpnImportSniff.kind("[Interface]\nAddress = 10.0.0.2/32\n"))
        assertNull(VpnImportSniff.kind("# client\n# remote x\n"))
        assertNull(VpnImportSniff.kind(""))
    }

    @Test
    fun tooLargeIsNotAProfile() {
        val big = "[Interface]\nPrivateKey = x\n" + "#".repeat(VpnImportSniff.MAX_BYTES)
        assertFalse(VpnImportSniff.looksLikeProfile(big))
    }
}
