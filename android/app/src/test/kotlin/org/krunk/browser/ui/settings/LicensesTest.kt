package org.krunk.browser.ui.settings

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.krunk.browser.BuildConfig

class LicensesTest {
    /** `android/ubo.lock`: unit tests run in `android/app`. */
    private val lock: Map<String, String> = File("../ubo.lock").readLines()
        .filter { it.isNotBlank() && !it.startsWith("#") && '=' in it }
        .associate { it.substringBefore('=').trim() to it.substringAfter('=').trim() }

    private val notices: List<String> = File("../THIRD-PARTY-NOTICES.md").readLines()

    private val ubo get() = LICENSES.single { it.name.startsWith("uBlock Origin") }

    @Test
    fun everyEntryIsComplete() {
        for (e in LICENSES) {
            assertTrue(e.name, e.name.isNotBlank() && e.license.isNotBlank() && e.note.isNotBlank())
            assertTrue(e.name, e.url.startsWith("https://"))
        }
        assertEquals(LICENSES.size, LICENSES.map { it.name }.distinct().size)
    }

    @Test
    fun theScreensBlockerVersionIsTheLockFiles() {
        val version = lock.getValue("UBO_VERSION")
        assertTrue(version, version.matches(Regex("""\d+\.\d+\.\d+\S*""")))
        assertEquals("uBlock Origin $version", ubo.name)
        assertEquals(version, BuildConfig.UBO_VERSION)
        assertEquals("GPL-3.0", ubo.license)
    }

    @Test
    fun theScreensBlockerSourceIsTheLockFiles() {
        val xpi = lock.getValue("UBO_XPI")
        assertEquals(xpi, BuildConfig.UBO_SOURCE_URL)
        assertEquals(UboSource.releasePage(xpi), ubo.url)
        assertTrue(ubo.url, ubo.url.endsWith("/releases/tag/" + lock.getValue("UBO_VERSION")))
        assertTrue(ubo.note, UboSource.fileName(xpi) in ubo.note)
        // Nothing about the release is written a second time in the list's source.
        val source = File("src/main/kotlin/org/krunk/browser/ui/settings/Licenses.kt").readText()
        assertTrue(lock.getValue("UBO_VERSION") !in source)
    }

    @Test
    fun theReleasePageIsDerivedFromTheReleaseFilesAddress() {
        assertEquals(
            "https://github.com/gorhill/uBlock/releases/tag/1.76.0",
            UboSource.releasePage(
                "https://github.com/gorhill/uBlock/releases/download/1.76.0/uBlock0_1.76.0.firefox.signed.xpi",
            ),
        )
        assertEquals(
            "uBlock0_1.76.0.firefox.signed.xpi",
            UboSource.fileName(
                "https://github.com/gorhill/uBlock/releases/download/1.76.0/uBlock0_1.76.0.firefox.signed.xpi",
            ),
        )
        // Another shape of address is shown as it is rather than guessed at.
        assertEquals("https://example.org/ubo.xpi", UboSource.releasePage("https://example.org/ubo.xpi"))
        assertEquals(
            "http://github.com/a/b/releases/download/1/x",
            UboSource.releasePage("http://github.com/a/b/releases/download/1/x"),
        )
    }

    @Test
    fun theListsInsideTheBlockerAreListedWithTheirLicences() {
        val wanted = mapOf(
            "EasyList and EasyPrivacy" to "GPL-3.0-or-later or CC BY-SA 3.0",
            "uBlock filters" to "GPL-3.0",
            "Peter Lowe's ad and tracking server list" to "McRae General Public License 4.r53",
            "Online Malicious URL Blocklist (URLhaus)" to "CC0-1.0",
            "Public Suffix List" to "MPL-2.0",
        )
        for ((name, licence) in wanted) assertEquals(name, licence, LICENSES.single { it.name == name }.license)
    }

    @Test
    fun theNoticesFileSaysTheSameAsTheScreen() {
        val rows = notices.filter { it.startsWith("| ") && !it.startsWith("| Component") }
            .map { row -> row.trim('|').split('|').map { it.trim() } }
        // The same components, in the same order, with the same licences. The file names the
        // blocker without its version: the version is in the lock file only.
        val onScreen = LICENSES.map { (if (it === ubo) "uBlock Origin" else it.name) to it.license }
        assertEquals(onScreen, rows.map { it[0] to it[1] })
        val text = notices.joinToString("\n")
        assertTrue("ubo.lock" in text && "UBO_VERSION" in text && "UBO_XPI" in text)
        assertTrue(lock.getValue("UBO_VERSION") !in text)
    }
}
