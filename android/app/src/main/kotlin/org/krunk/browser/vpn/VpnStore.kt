package org.krunk.browser.vpn

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import org.krunk.browser.vault.AndroidWrapKeyStore
import org.krunk.browser.vault.KeystoreKeyWrapper
import org.krunk.browser.vault.WrapFailure
import org.krunk.browser.vault.WrapFailureKind

/** A stored VPN profile, as lists show it (no secrets). */
data class VpnProfile(
    val id: String,
    val name: String,
    /** "wireguard" or "openvpn". */
    val kind: String,
    val server: String,
    val needsCredentials: Boolean,
    /** The name of the imported file (the runtime detects the kind by name and content). */
    val fileName: String,
)

/** An OpenVPN user name and password. [toString] never shows the password. */
class VpnCredentials(val username: String, val password: String) {
    override fun toString() = "VpnCredentials(username=…, password=…)"

    override fun equals(other: Any?) =
        other is VpnCredentials && other.username == username && other.password == password

    override fun hashCode() = username.hashCode() * 31 + password.hashCode()
}

/** What wraps the store's data key: the Keystore on the device, a fake in JVM tests. */
interface VpnKeyWrap {
    @Throws(WrapFailure::class)
    fun protect(data: ByteArray): ByteArray

    @Throws(WrapFailure::class)
    fun unprotect(data: ByteArray): ByteArray
}

/** The store's key or files cannot be read; [lost] when they never will be again (key gone). */
class VpnStoreException(message: String, val lost: Boolean = false) : IOException(message)

/**
 * VPN profiles in `files/krunk-vpn/` (contract §5, plan §7):
 *
 * - `key.bin`: a random AES-256 data key, wrapped by the Keystore key `krunk.vpn.wrap.v1`
 *   (made on the first import; it never leaves the device).
 * - `index.bin`: the list (id, name, kind, server, needs credentials, file name), AES-GCM.
 * - `p-<id>.bin`: one per profile, AES-GCM: the original file and the optional OpenVPN
 *   credentials. The original is re-imported at every connect (Windows behaviour).
 *
 * Every sealed file is `0x01 | 12-byte IV | ciphertext+tag`, with the file's role as associated
 * data, so files cannot be swapped for each other. Nothing here is logged.
 */
class VpnStore(
    private val dir: File,
    private val wrap: VpnKeyWrap,
    private val random: SecureRandom = SecureRandom(),
) {
    private var dataKey: ByteArray? = null

    @Synchronized
    @Throws(VpnStoreException::class)
    fun list(): List<VpnProfile> {
        val index = File(dir, INDEX_FILE)
        if (!index.isFile || !File(dir, KEY_FILE).isFile) return emptyList()
        return decodeIndex(open(key(create = false), read(index), AAD_INDEX))
    }

    /** Saves a checked profile ([info] from the runtime's import) and returns it. */
    @Synchronized
    @Throws(VpnStoreException::class)
    fun add(fileName: String, bytes: ByteArray, info: VpnProfileInfo, creds: VpnCredentials?): VpnProfile {
        val key = try {
            key(create = true)
        } catch (e: VpnStoreException) {
            if (!e.lost) throw e
            // The device key is gone: what is stored can never be read again. Start over.
            wipe()
            key(create = true)
        }
        val current = if (File(
                dir,
                INDEX_FILE,
            ).isFile
        ) {
            decodeIndex(open(key, read(File(dir, INDEX_FILE)), AAD_INDEX))
        } else {
            emptyList()
        }
        var id: String
        do {
            id = newId()
        } while (current.any { it.id == id })
        val profile = VpnProfile(id, info.name, info.kind, info.server, info.needsCredentials, fileName)
        write(profileFile(id), seal(key, encodePayload(fileName, bytes, creds), aadProfile(id)))
        write(File(dir, INDEX_FILE), seal(key, encodeIndex(current + profile), AAD_INDEX))
        return profile
    }

    /** Replaces (or with null removes) the OpenVPN credentials of profile [id]. */
    @Synchronized
    @Throws(VpnStoreException::class)
    fun setCredentials(id: String, creds: VpnCredentials?) {
        val (file, bytes, _) = readPayload(id)
        val key = key(create = false)
        write(profileFile(id), seal(key, encodePayload(file, bytes, creds), aadProfile(id)))
    }

    @Synchronized
    @Throws(VpnStoreException::class)
    fun remove(id: String) {
        if (!validId(id)) return
        val index = File(dir, INDEX_FILE)
        if (index.isFile && File(dir, KEY_FILE).isFile) {
            val key = key(create = false)
            val rest = decodeIndex(open(key, read(index), AAD_INDEX)).filter { it.id != id }
            write(index, seal(key, encodeIndex(rest), AAD_INDEX))
        }
        profileFile(id).delete()
    }

    /** The original file and the credentials of profile [id]. */
    @Synchronized
    @Throws(VpnStoreException::class)
    fun read(id: String): Pair<ByteArray, VpnCredentials?> {
        val (_, bytes, creds) = readPayload(id)
        return bytes to creds
    }

    /** Deletes every profile and the wrapped key (the Keystore key stays). */
    @Synchronized
    fun wipe() {
        dataKey?.fill(0)
        dataKey = null
        dir.listFiles()?.forEach { if (it.name != SETTINGS_FILE) it.delete() }
    }

    private fun readPayload(id: String): Triple<String, ByteArray, VpnCredentials?> {
        if (!validId(id)) throw VpnStoreException("no such profile")
        val file = profileFile(id)
        if (!file.isFile) throw VpnStoreException("no such profile")
        return decodePayload(open(key(create = false), read(file), aadProfile(id)))
    }

    private fun profileFile(id: String) = File(dir, "p-$id.bin")

    private fun key(create: Boolean): ByteArray {
        dataKey?.let { return it }
        val file = File(dir, KEY_FILE)
        val key = if (file.isFile) {
            try {
                wrap.unprotect(read(file))
            } catch (e: WrapFailure) {
                throw VpnStoreException(
                    "the device key cannot open the VPN profiles",
                    lost =
                        e.kind != WrapFailureKind.FAILED,
                )
            }
        } else {
            if (!create) throw VpnStoreException("no key")
            val fresh = ByteArray(32).also(random::nextBytes)
            val wrapped = try {
                wrap.protect(fresh)
            } catch (e: WrapFailure) {
                throw VpnStoreException("the device key store failed (${e.kind})")
            }
            write(file, wrapped)
            fresh
        }
        if (key.size != 32) throw VpnStoreException("bad key", lost = true)
        dataKey = key
        return key
    }

    private fun newId(): String = ByteArray(16).also(random::nextBytes).joinToString("") { "%02x".format(it) }

    private fun seal(key: ByteArray, plain: ByteArray, aad: ByteArray): ByteArray = try {
        val iv = ByteArray(IV_LENGTH).also(random::nextBytes)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(TAG_BITS, iv))
        cipher.updateAAD(aad)
        byteArrayOf(VERSION) + iv + cipher.doFinal(plain)
    } catch (e: GeneralSecurityException) {
        throw VpnStoreException("encryption failed (${e.javaClass.simpleName})")
    } finally {
        plain.fill(0)
    }

    private fun open(key: ByteArray, sealed: ByteArray, aad: ByteArray): ByteArray {
        if (sealed.size < 1 + IV_LENGTH + TAG_BITS / 8 || sealed[0] != VERSION) {
            throw VpnStoreException("damaged file")
        }
        return try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                SecretKeySpec(key, "AES"),
                GCMParameterSpec(TAG_BITS, sealed, 1, IV_LENGTH),
            )
            cipher.updateAAD(aad)
            cipher.doFinal(sealed, 1 + IV_LENGTH, sealed.size - 1 - IV_LENGTH)
        } catch (e: GeneralSecurityException) {
            throw VpnStoreException("damaged file (${e.javaClass.simpleName})")
        }
    }

    private fun read(file: File): ByteArray = try {
        file.readBytes()
    } catch (e: IOException) {
        throw VpnStoreException("cannot read ${file.name}")
    }

    private fun write(file: File, data: ByteArray) {
        try {
            dir.mkdirs()
            val tmp = File(dir, file.name + ".tmp")
            tmp.writeBytes(data)
            if (!tmp.renameTo(file)) {
                tmp.delete()
                throw IOException("rename")
            }
        } catch (e: IOException) {
            throw VpnStoreException("cannot write ${file.name}")
        }
    }

    companion object {
        const val DIR = "krunk-vpn"
        const val SETTINGS_FILE = "settings.json"
        const val KEY_FILE = "key.bin"
        const val INDEX_FILE = "index.bin"

        /** The Keystore alias of the key that wraps the store's data key. */
        const val WRAP_ALIAS = "krunk.vpn.wrap.v1"

        /** The largest profile file the runtime takes. */
        const val MAX_PROFILE = 64 * 1024
        private const val VERSION: Byte = 0x01
        private const val IV_LENGTH = 12
        private const val TAG_BITS = 128
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private val AAD_INDEX = "krunk-vpn:index".toByteArray()

        private fun aadProfile(id: String) = "krunk-vpn:profile:$id".toByteArray()

        fun dir(filesDir: File): File = File(filesDir, DIR)

        /** The store on the device's key store (the key is made at the first import). */
        fun onDevice(filesDir: File): VpnStore {
            val wrapper = KeystoreKeyWrapper(AndroidWrapKeyStore(), WRAP_ALIAS, createOnProtect = true)
            return VpnStore(
                dir(filesDir),
                object : VpnKeyWrap {
                    override fun protect(data: ByteArray) = wrapper.protect(data)

                    override fun unprotect(data: ByteArray) = wrapper.unprotect(data)
                },
            )
        }

        fun validId(id: String): Boolean = id.length == 32 && id.all { it in '0'..'9' || it in 'a'..'f' }

        private fun encodeIndex(list: List<VpnProfile>): ByteArray {
            val bytes = ByteArrayOutputStream()
            DataOutputStream(bytes).use { out ->
                out.writeInt(list.size)
                for (p in list) {
                    out.writeUTF(p.id)
                    out.writeUTF(p.name)
                    out.writeUTF(p.kind)
                    out.writeUTF(p.server)
                    out.writeBoolean(p.needsCredentials)
                    out.writeUTF(p.fileName)
                }
            }
            return bytes.toByteArray()
        }

        private fun decodeIndex(data: ByteArray): List<VpnProfile> = try {
            DataInputStream(ByteArrayInputStream(data)).use { input ->
                val n = input.readInt()
                if (n !in 0..10_000) throw IOException("count")
                List(n) {
                    VpnProfile(
                        id = input.readUTF(),
                        name = input.readUTF(),
                        kind = input.readUTF(),
                        server = input.readUTF(),
                        needsCredentials = input.readBoolean(),
                        fileName = input.readUTF(),
                    )
                }
            }
        } catch (e: IOException) {
            throw VpnStoreException("damaged index")
        }

        private fun encodePayload(fileName: String, bytes: ByteArray, creds: VpnCredentials?): ByteArray {
            val out = ByteArrayOutputStream(bytes.size + 256)
            DataOutputStream(out).use { d ->
                d.writeUTF(fileName)
                d.writeInt(bytes.size)
                d.write(bytes)
                d.writeBoolean(creds != null)
                if (creds != null) {
                    d.writeUTF(creds.username)
                    d.writeUTF(creds.password)
                }
            }
            return out.toByteArray()
        }

        private fun decodePayload(data: ByteArray): Triple<String, ByteArray, VpnCredentials?> = try {
            DataInputStream(ByteArrayInputStream(data)).use { input ->
                val name = input.readUTF()
                val n = input.readInt()
                if (n !in 0..MAX_PROFILE) throw IOException("size")
                val bytes = ByteArray(n).also(input::readFully)
                val creds = if (input.readBoolean()) VpnCredentials(input.readUTF(), input.readUTF()) else null
                Triple(name, bytes, creds)
            }
        } catch (e: IOException) {
            throw VpnStoreException("damaged profile")
        } finally {
            data.fill(0)
        }
    }
}
