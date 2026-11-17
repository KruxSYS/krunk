package org.krunk.browser.vpn

import java.io.File
import java.nio.file.Files
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import org.krunk.browser.vault.WrapFailure
import org.krunk.browser.vault.WrapFailureKind

class VpnStoreTest {
    /** The Keystore wrapper's stand-in: AES-GCM with a key held in memory. */
    private class FakeWrap(seed: Byte, var gone: Boolean = false) : VpnKeyWrap {
        private val key = SecretKeySpec(ByteArray(32) { seed }, "AES")

        override fun protect(data: ByteArray): ByteArray {
            val iv = ByteArray(12) { 7 }
            val c = Cipher.getInstance("AES/GCM/NoPadding")
            c.init(Cipher.ENCRYPT_MODE, key, GCMParameterSpec(128, iv))
            return iv + c.doFinal(data)
        }

        override fun unprotect(data: ByteArray): ByteArray {
            if (gone) throw WrapFailure(WrapFailureKind.MISSING, "no key")
            try {
                val c = Cipher.getInstance("AES/GCM/NoPadding")
                c.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, data, 0, 12))
                return c.doFinal(data, 12, data.size - 12)
            } catch (e: Exception) {
                throw WrapFailure(WrapFailureKind.FAILED, e.javaClass.simpleName)
            }
        }
    }

    private val dir: File = Files.createTempDirectory("krunk-vpn-test").toFile()

    @After
    fun cleanUp() {
        dir.deleteRecursively()
    }

    private val profileText = """
        [Interface]
        PrivateKey = SECRETPRIVATEKEYaaaaaaaaaaaaaaaaaaaaaaaaaaa=
        Address = 10.8.0.2/32
        [Peer]
        PublicKey = PEERKEYbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=
        Endpoint = vpn.example.net:51820
    """.trimIndent().toByteArray()

    private val info = VpnProfileInfo("wireguard", "Home", "vpn.example.net:51820", false, emptyList())

    @Test
    fun roundTrip() {
        val store = VpnStore(dir, FakeWrap(1))
        val creds = VpnCredentials("alice", "hunter2-PASSWORD")
        val p = store.add("home.conf", profileText.copyOf(), info, creds)
        assertTrue(VpnStore.validId(p.id))
        assertEquals(listOf(p), store.list())
        assertEquals(VpnProfile(p.id, "Home", "wireguard", "vpn.example.net:51820", false, "home.conf"), p)
        val (bytes, back) = store.read(p.id)
        assertArrayEquals(profileText, bytes)
        assertEquals(creds, back)

        // A new instance (new process) reads the same through the wrapped key.
        val again = VpnStore(dir, FakeWrap(1))
        assertEquals(listOf(p), again.list())
        assertArrayEquals(profileText, again.read(p.id).first)

        again.setCredentials(p.id, null)
        assertNull(again.read(p.id).second)
        val q = again.add("b.conf", profileText.copyOf(), info.copy(name = "Two"), null)
        assertEquals(listOf("Home", "Two"), again.list().map { it.name })
        again.remove(p.id)
        assertEquals(listOf(q), again.list())
        assertFalse(File(dir, "p-${p.id}.bin").exists())
    }

    @Test
    fun noPlaintextOnDisk() {
        val store = VpnStore(dir, FakeWrap(1))
        store.add("home.conf", profileText.copyOf(), info, VpnCredentials("alice", "hunter2-PASSWORD"))
        val needles =
            listOf("SECRETPRIVATEKEY", "PrivateKey", "hunter2", "alice", "vpn.example.net", "Home", "home.conf")
        val files = dir.listFiles()!!.toList()
        assertEquals(setOf("key.bin", "index.bin"), files.map { it.name }.filter { !it.startsWith("p-") }.toSet())
        for (f in files) {
            val text = String(f.readBytes(), Charsets.ISO_8859_1)
            for (n in needles) assertFalse("$n in ${f.name}", text.contains(n))
        }
    }

    @Test
    fun aWrongKeyOpensNothing() {
        val store = VpnStore(dir, FakeWrap(1))
        val p = store.add("home.conf", profileText.copyOf(), info, null)
        val other = VpnStore(dir, FakeWrap(2))
        try {
            other.read(p.id)
            fail("read with the wrong device key")
        } catch (e: VpnStoreException) {
            assertFalse(e.lost)
        }
        try {
            other.list()
            fail("listed with the wrong device key")
        } catch (_: VpnStoreException) {
        }
        // A profile file moved under another id does not open (the id is bound to it).
        val q = store.add("b.conf", profileText.copyOf(), info, null)
        File(dir, "p-${p.id}.bin").copyTo(File(dir, "p-${q.id}.bin"), overwrite = true)
        try {
            VpnStore(dir, FakeWrap(1)).read(q.id)
            fail("swapped file opened")
        } catch (_: VpnStoreException) {
        }
    }

    @Test
    fun aLostDeviceKeyStartsOverAtTheNextImport() {
        VpnStore(dir, FakeWrap(1)).add("home.conf", profileText.copyOf(), info, null)
        val lost = VpnStore(dir, FakeWrap(1, gone = true))
        try {
            lost.list()
            fail("listed without the key")
        } catch (e: VpnStoreException) {
            assertTrue(e.lost)
        }
        // A key store that merely fails (FAILED, not lost) never makes the store start over.
        try {
            VpnStore(dir, FakeWrap(3)).add("x.conf", profileText.copyOf(), info, null)
            fail("added over an unreadable store")
        } catch (e: VpnStoreException) {
            assertFalse(e.lost)
        }
        assertEquals(1, dir.listFiles()!!.count { it.name.startsWith("p-") })
        // The key is gone for good: the next import starts over with a new key.
        val p = lost.add("x.conf", profileText.copyOf(), info, null)
        assertEquals(listOf(p), lost.list())
        assertEquals(listOf("p-${p.id}.bin"), dir.listFiles()!!.map { it.name }.filter { it.startsWith("p-") })
    }

    @Test
    fun settingsRoundTripAndTolerateJunk() {
        val s = VpnSettings(
            enabled = true,
            connectAtStart = true,
            scope = VpnScope.PRIVATE,
            lanDirect = true,
            showInAddressBar = false,
            selectedProfileId = "0123456789abcdef0123456789abcdef",
        )
        assertEquals(s, VpnSettings.fromJson(s.toJson()))
        assertEquals(VpnSettings(), VpnSettings.fromJson(VpnSettings().toJson()))
        assertEquals(VpnSettings(), VpnSettings.fromJson("not json"))
        assertEquals(VpnSettings(), VpnSettings.fromJson(""))
        assertEquals(
            VpnSettings(enabled = true),
            VpnSettings.fromJson("""{ "enabled" : true, "future": "x", "selectedProfileId": "../../etc" }"""),
        )
        val file = VpnSettingsFile(dir)
        assertEquals(VpnSettings(), file.read())
        file.write(s)
        assertEquals(s, file.read())
        assertEquals(File(dir, "settings.json"), dir.listFiles()!!.single())
    }

    @Test
    fun theFolderIsInNoBackup() {
        val filesDir = File("/data/user/0/org.krunk.browser/files")
        assertEquals(File(filesDir, "krunk-vpn"), VpnStore.dir(filesDir))
        val backup = File("src/main/res/xml/backup_rules.xml").readText()
        val extraction = File("src/main/res/xml/data_extraction_rules.xml").readText()
        val rule = """<exclude domain="file" path="krunk-vpn" />"""
        assertTrue(backup.contains(rule))
        assertTrue(extraction.substringAfter("<cloud-backup>").substringBefore("</cloud-backup>").contains(rule))
        val transfer = extraction.substringAfter("<device-transfer>").substringBefore("</device-transfer>")
        val includes = Regex("""<include domain="([a-z]+)" path="([^"]+)" />""").findAll(transfer)
            .map { it.groupValues[1] to it.groupValues[2] }.toList()
        assertEquals(listOf("file" to "krunk"), includes)
    }
}
