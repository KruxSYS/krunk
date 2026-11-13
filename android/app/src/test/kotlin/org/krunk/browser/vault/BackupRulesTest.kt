package org.krunk.browser.vault

import java.io.File
import javax.xml.parsers.DocumentBuilderFactory
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.w3c.dom.Element

/**
 * The vault must never be part of a backup or a device transfer: these tests read the rule
 * files from the source tree and hold them against the directory the code uses.
 */
class BackupRulesTest {
    private class Rule(val include: Boolean, val domain: String, val path: String)

    /** One rule set: `full-backup-content`, `cloud-backup` or `device-transfer`. */
    private class Section(val name: String, val rules: List<Rule>) {
        val includes get() = rules.filter { it.include }
        val excludes get() = rules.filter { !it.include }
    }

    private fun parse(xml: String, vararg sections: String): List<Section> {
        val root = DocumentBuilderFactory.newInstance().newDocumentBuilder()
            .parse(xml.byteInputStream()).documentElement
        return sections.map { name ->
            val element = if (root.tagName == name) root else root.getElementsByTagName(name).item(0) as Element
            val rules = ArrayList<Rule>()
            val children = element.childNodes
            for (i in 0 until children.length) {
                val child = children.item(i) as? Element ?: continue
                assertTrue("unknown rule <${child.tagName}>", child.tagName == "include" || child.tagName == "exclude")
                rules += Rule(child.tagName == "include", child.getAttribute("domain"), child.getAttribute("path"))
            }
            Section(name, rules)
        }
    }

    private fun sourceFile(name: String): String {
        // Gradle runs the tests in the module directory.
        val file = File("src/main/res/xml/$name")
        assertTrue("${file.absolutePath} not found", file.isFile)
        return file.readText()
    }

    private fun sections(): List<Section> = parse(sourceFile("backup_rules.xml"), "full-backup-content") +
        parse(sourceFile("data_extraction_rules.xml"), "cloud-backup", "device-transfer")

    /** The vault directory as a rule of [domain] would have to name it, or null if it cannot. */
    private fun vaultPathIn(domain: String): String? = when (domain) {
        "file" -> VaultFiles.DIR
        "root" -> "files/${VaultFiles.DIR}"
        else -> null
    }

    private fun normal(path: String): String = path.removePrefix("./").trim('/')

    /** Does a rule on [rule]'s path take in the vault directory or something inside it? */
    private fun touchesVault(rule: Rule): Boolean {
        val vault = vaultPathIn(rule.domain) ?: return false
        val path = normal(rule.path)
        return path == "." || path.isEmpty() || path == vault || vault.startsWith("$path/") ||
            path.startsWith("$vault/")
    }

    /** Does an exclude on [rule]'s path cover the whole vault directory? */
    private fun coversVault(rule: Rule): Boolean {
        val vault = vaultPathIn(rule.domain) ?: return false
        val path = normal(rule.path)
        return path == "." || path == vault || vault.startsWith("$path/")
    }

    /** What is wrong with [section], as text; empty when the vault stays out of it. */
    private fun problems(section: Section): List<String> {
        val found = ArrayList<String>()
        if (section.excludes.none { it.domain == "file" && it.path == VaultFiles.DIR }) {
            found += "${section.name}: no exclude of file/${VaultFiles.DIR}"
        }
        for (include in section.includes.filter(::touchesVault)) {
            if (section.excludes.none(::coversVault)) {
                found += "${section.name}: the include of ${include.domain}/${include.path} carries the vault"
            }
        }
        return found
    }

    @Test
    fun theThreeRuleSetsAreAllThere() {
        assertEquals(listOf("full-backup-content", "cloud-backup", "device-transfer"), sections().map { it.name })
    }

    @Test
    fun everyRuleSetExcludesTheVaultByName() {
        for (section in sections()) {
            val named = section.excludes.filter { it.domain == "file" && it.path == VaultFiles.DIR }
            assertEquals("${section.name} names the vault directory once", 1, named.size)
        }
        assertEquals("krunk-vault", VaultFiles.DIR)
    }

    @Test
    fun noIncludeCarriesTheVault() {
        for (section in sections()) assertEquals(emptyList<String>(), problems(section))
    }

    @Test
    fun deviceTransferCarriesKrunksDataButNotTheVault() {
        val transfer = sections().first { it.name == "device-transfer" }
        assertEquals(listOf("file" to "krunk"), transfer.includes.map { it.domain to it.path })
        // The vault lies beside that directory, not in it.
        assertFalse(transfer.includes.any(::touchesVault))
        val filesDir = File("/data/user/0/org.krunk.browser/files")
        assertEquals(File(filesDir, "krunk-vault"), VaultFiles.dir(filesDir))
        assertEquals(VaultFiles.dir(filesDir), VaultFiles.vaultFile(filesDir).parentFile)
        assertEquals(VaultFiles.dir(filesDir), VaultFiles.stateFile(filesDir).parentFile)
        assertFalse(VaultFiles.dir(filesDir).path.startsWith(File(filesDir, "krunk").path + File.separator))
    }

    @Test
    fun cloudBackupStillCarriesNothing() {
        for (section in sections().filter { it.name != "device-transfer" }) {
            assertTrue("${section.name} includes something", section.includes.isEmpty())
            for (domain in listOf("root", "file", "database", "sharedpref", "external")) {
                assertTrue(
                    "${section.name} does not exclude all of $domain",
                    section.excludes.any { it.domain == domain && it.path == "." },
                )
            }
        }
    }

    /** The check itself must be able to fail: rule sets that would let the vault out. */
    @Test
    fun theCheckCatchesBadRules() {
        fun count(rules: String): Int {
            val xml = "<data-extraction-rules><device-transfer>$rules</device-transfer></data-extraction-rules>"
            return problems(parse(xml, "device-transfer").single()).size
        }

        fun include(domain: String, path: String) = "<include domain=\"$domain\" path=\"$path\" />"
        val exclude = "<exclude domain=\"file\" path=\"${VaultFiles.DIR}\" />"

        // Not named:
        assertEquals(1, count(include("file", "krunk")))
        // Not named, and carried along:
        assertEquals(2, count(include("file", ".")))
        assertEquals(2, count(include("file", "${VaultFiles.DIR}/vault.json")))
        assertEquals(2, count(include("root", ".")))
        assertEquals(2, count(include("root", "files")))
        assertEquals(2, count(include("file", ".") + "<exclude domain=\"file\" path=\"krunk\" />"))
        // Fine:
        assertEquals(0, count(include("file", ".") + exclude))
        assertEquals(0, count(include("file", "krunk") + exclude))
        assertEquals(0, count(include("database", ".") + exclude))
    }
}
