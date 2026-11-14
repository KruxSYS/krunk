package org.krunk.browser.update

import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.Signature
import java.util.Base64
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test
import org.krunk.ffi.AndroidUpdate
import org.krunk.ffi.UpdateFailure
import org.krunk.ffi.UpdateOffer
import org.krunk.ffi.updateCheckDownload
import org.krunk.ffi.updateOffer
import org.krunk.ffi.updatePickFile
import org.krunk.ffi.updateUrls
import org.krunk.ffi.updateVerify
import org.krunk.ffi.updateVerifyWithKey

/**
 * The update check's trust decisions, through the same FFI the app calls (the host build of the
 * core). Test keys are made here with the JDK's Ed25519; the release key is never used to sign
 * anything in tests.
 */
class UpdateVerifyTest {
    private val base = "https://krunk.example"
    private val pkg = "org.krunk.browser"
    private val cert = "c94b8915cd8b393a5fc48bc132a6de026374e76337c4341c59dd60328373e624"

    private fun key(): KeyPair = KeyPairGenerator.getInstance("Ed25519").generateKeyPair()

    private fun pem(k: KeyPair): String =
        "-----BEGIN PUBLIC KEY-----\n" + Base64.getEncoder().encodeToString(k.public.encoded) +
            "\n-----END PUBLIC KEY-----\n"

    private fun sign(k: KeyPair, body: ByteArray): ByteArray {
        val s = Signature.getInstance("Ed25519")
        s.initSign(k.private)
        s.update(body)
        return Base64.getEncoder().encode(s.sign())
    }

    private fun sha(data: ByteArray) = UpdateRules.hex(MessageDigest.getInstance("SHA-256").digest(data))

    private fun manifest(versionCode: Int = 7, fileUrl: String = "/download/android/0.5.0/K-x86_64.apk"): ByteArray =
        """
        {"channel":"android-arm64","version":"0.5.0","version_code":$versionCode,"released":"2026-11-14",
         "package":"$pkg","min_sdk":26,"signing_cert_sha256":"$cert","notes":"- notes",
         "files":[{"abi":"arm64-v8a","url":"/download/android/0.5.0/K-arm64-v8a.apk","size":3,"sha256":"${sha(
            "arm".toByteArray(),
        )}"},
                  {"abi":"x86_64","url":"$fileUrl","size":3,"sha256":"${sha("x86".toByteArray())}"},
                  {"abi":"universal","url":"/download/android/0.5.0/K-universal.apk","size":3,"sha256":"${sha(
            "uni".toByteArray(),
        )}"}]}
        """.trimIndent().toByteArray()

    private fun verify(k: KeyPair, body: ByteArray, sig: ByteArray): AndroidUpdate =
        updateVerifyWithKey(pem(k), base, false, pkg, body, sig)

    private inline fun <reified T : UpdateFailure> refused(block: () -> Unit): T {
        try {
            block()
        } catch (e: UpdateFailure) {
            if (e is T) return e
            fail("expected ${T::class.simpleName}, got $e")
        }
        fail("expected ${T::class.simpleName}, nothing thrown")
        throw AssertionError()
    }

    @Test
    fun goodSignatureIsAccepted() {
        val k = key()
        val body = manifest()
        val u = verify(k, body, sign(k, body))
        assertEquals("0.5.0", u.version)
        assertEquals(7UL, u.versionCode)
        assertEquals(cert, u.signingCertSha256)
        assertEquals(3, u.files.size)
        assertEquals("https://krunk.example/download/android/0.5.0/K-x86_64.apk", u.files[1].url)
    }

    @Test
    fun badSignatureIsRefused() {
        val k = key()
        val body = manifest()
        val sig = sign(k, body)
        val tampered = body.copyOf().also { it[40] = (it[40].toInt() xor 1).toByte() }
        refused<UpdateFailure.BadSignature> { verify(k, tampered, sig) }
        refused<UpdateFailure.BadSignature> { verify(k, body, "not a signature".toByteArray()) }
        assertEquals(UpdateErrorKind.SIGNATURE, UpdateRules.kindOf(UpdateFailure.BadSignature()))
    }

    @Test
    fun wrongKeyIsRefused() {
        val k = key()
        val other = key()
        val body = manifest()
        refused<UpdateFailure.BadSignature> { verify(k, body, sign(other, body)) }
        // Krunk's own release key refuses a test signature too.
        refused<UpdateFailure.BadSignature> { updateVerify(base, false, pkg, body, sign(k, body)) }
    }

    @Test
    fun filesElsewhereAreRefused() {
        val k = key()
        val body = manifest(fileUrl = "https://evil.example/K.apk")
        val e = refused<UpdateFailure.NotOnServer> { verify(k, body, sign(k, body)) }
        assertEquals(UpdateErrorKind.INVALID, UpdateRules.kindOf(e))
    }

    @Test
    fun anotherAppsManifestIsRefused() {
        val k = key()
        val body = String(manifest()).replace("\"package\":\"$pkg\"", "\"package\":\"org.other\"").toByteArray()
        refused<UpdateFailure.Malformed> { verify(k, body, sign(k, body)) }
    }

    @Test
    fun hashMismatchIsRefused() {
        val k = key()
        val body = manifest()
        val u = verify(k, body, sign(k, body))
        val file = updatePickFile(u, listOf("x86_64"))!!
        updateCheckDownload(file, 3UL, sha("x86".toByteArray()))
        val e = refused<UpdateFailure.WrongHash> { updateCheckDownload(file, 3UL, sha("x87".toByteArray())) }
        assertEquals(UpdateErrorKind.HASH, UpdateRules.kindOf(e))
        refused<UpdateFailure.WrongSize> { updateCheckDownload(file, 4UL, sha("x86".toByteArray())) }
    }

    @Test
    fun versionCodesCompare() {
        val k = key()
        val body = manifest()
        val u = verify(k, body, sign(k, body))
        assertEquals(UpdateOffer.INSTALL, updateOffer(u, 5UL, 36U)) // 0.4.0
        assertEquals(UpdateOffer.INSTALL, updateOffer(u, 6UL, 29U)) // 0.5.0-qa
        assertEquals(UpdateOffer.UP_TO_DATE, updateOffer(u, 7UL, 36U)) // same build
        assertEquals(UpdateOffer.UP_TO_DATE, updateOffer(u, 8UL, 36U)) // no downgrade
        assertEquals(UpdateOffer.NEEDS_NEWER_ANDROID, updateOffer(u, 5UL, 25U))
    }

    @Test
    fun theDeviceGetsItsAbi() {
        val k = key()
        val body = manifest()
        val u = verify(k, body, sign(k, body))
        assertEquals("arm64-v8a", updatePickFile(u, listOf("arm64-v8a", "armeabi-v7a"))!!.abi)
        assertEquals("x86_64", updatePickFile(u, listOf("x86_64", "x86", "arm64-v8a"))!!.abi)
        assertEquals("universal", updatePickFile(u, listOf("armeabi-v7a"))!!.abi)
    }

    @Test
    fun releaseAddressesAreHttpsOnly() {
        val urls = updateUrls("https://krunk.209.wf", false)
        assertEquals("https://krunk.209.wf/updates/android-arm64/latest.json", urls.manifest)
        assertEquals("https://krunk.209.wf/updates/android-arm64/latest.json.sig", urls.signature)
        refused<UpdateFailure.NotOnServer> { updateUrls("http://10.250.136.3:18090", false) }
    }

    @Test
    fun downloadedApkMustBeKrunkWithTheSameCertificate() {
        val ok = setOf(cert)
        fun check(pkgName: String? = pkg, code: Long? = 7, apk: Set<String> = ok, installed: Set<String> = ok) =
            UpdateRules.checkApk(pkg, 7, cert, installed, pkgName, code, apk)
        assertNull(check())
        assertEquals(UpdateErrorKind.PACKAGE, check(pkgName = "org.evil"))
        assertEquals(UpdateErrorKind.PACKAGE, check(pkgName = null))
        assertEquals(UpdateErrorKind.PACKAGE, check(code = 6))
        assertEquals(UpdateErrorKind.CERTIFICATE, check(apk = setOf("00".repeat(32))))
        assertEquals(UpdateErrorKind.CERTIFICATE, check(apk = emptySet()))
        assertEquals(UpdateErrorKind.CERTIFICATE, check(apk = ok + "11".repeat(32)))
        // The installed app and the APK agree, but not with the signed manifest.
        val other = setOf("22".repeat(32))
        assertEquals(UpdateErrorKind.CERTIFICATE, check(apk = other, installed = other))
    }

    @Test
    fun releaseNotesAreShownWithoutMarkup() {
        val md = "Intro line\nwrapped.\n\n" +
            "- **Bold** item with `code`\n  and a second line.\n" +
            "- A [link](https://x.example) item\n"
        assertEquals(
            "Intro line wrapped.\n\n• Bold item with code and a second line.\n• A link item",
            UpdateRules.plainNotes(md),
        )
        assertEquals("", UpdateRules.plainNotes(""))
    }

    @Test
    fun userAgentCarriesOnlyTheVersion() {
        assertEquals("Krunk-Android/0.5.0", UpdateRules.userAgent("0.5.0"))
    }
}
