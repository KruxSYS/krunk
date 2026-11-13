package org.krunk.browser.engine.logins

import org.krunk.browser.engine.originOf
import org.krunk.browser.vault.SaveDecision

/**
 * When Krunk offers a saved login to a page and when it offers to save one. Pure decisions; the
 * matching of a login to an address is the shared core's (`core::vault::match_kind`): exact
 * origin, else the same registrable domain and then only between https pages. A login saved on
 * an https page is therefore never offered to an http page.
 *
 * The rules of this file:
 * - a private tab gets nothing unless the user armed it with "Fill from vault", and never a save
 *   offer;
 * - a page loaded over plain http gets a warning the user must confirm, before a fill and before
 *   a save;
 * - a locked vault asks for the password only at the moment a login is wanted (the user tapped a
 *   sign-in field on a site with logins, or chose Save); otherwise it stays quiet;
 * - a form in a frame of another origin than the tab's address is not filled.
 */
object LoginPolicy {
    /** The user name of the entry that stands for "there are logins, unlock to see them". */
    const val LOCKED_PLACEHOLDER_USER = "Krunk"

    /** The password every entry handed to the engine carries until the user picks one. */
    const val PLACEHOLDER_PASSWORD = "krunk-vault-placeholder"

    fun isWeb(origin: String?): Boolean =
        origin != null && (origin.startsWith("https://") || origin.startsWith("http://"))

    fun isPlainHttp(url: String): Boolean = originOf(url)?.startsWith("http://") == true

    /** What the tab is in when one of its fields asks for a login. */
    data class FillContext(
        val private: Boolean,
        /** The origin "Fill from vault" was chosen for in this tab, if any. */
        val armedOrigin: String?,
        /** The user closed the picker (or filled) on this page already. */
        val suppressed: Boolean,
        /** The tab's address. */
        val pageUrl: String,
        /** The origin of the form, as the engine reports it. */
        val formOrigin: String?,
        val vault: LoginVault.State,
        /** Locked vault: the hint list knows logins for the page. */
        val hint: Boolean,
    )

    sealed class Fill {
        data class No(val why: String) : Fill()

        /** Show the picker, after the unlock dialog when [unlockFirst], after the warning when [warnInsecure]. */
        data class Ask(val unlockFirst: Boolean, val warnInsecure: Boolean) : Fill()
    }

    fun fill(c: FillContext): Fill {
        val page = originOf(c.pageUrl)
        if (!isWeb(page)) return Fill.No("not a web page")
        if (c.formOrigin == null || originOf(c.formOrigin) != page) return Fill.No("form of another origin")
        val armed = c.armedOrigin != null && c.armedOrigin == page
        if (c.private && !armed) return Fill.No("private tab")
        if (c.suppressed && !armed) return Fill.No("closed on this page")
        val insecure = isPlainHttp(c.pageUrl)
        return when (c.vault) {
            LoginVault.State.NONE -> Fill.No("no vault")

            LoginVault.State.KEY_LOST -> Fill.No("vault key lost")

            LoginVault.State.LOCKED ->
                if (c.hint || armed) Fill.Ask(unlockFirst = true, warnInsecure = insecure) else Fill.No("no logins")

            LoginVault.State.UNLOCKED -> Fill.Ask(unlockFirst = false, warnInsecure = insecure)
        }
    }

    /** May the engine be told that logins exist for a tab's page (placeholders only)? */
    fun announce(pageUrl: String, vault: LoginVault.State): Boolean =
        isWeb(originOf(pageUrl)) && (vault == LoginVault.State.UNLOCKED || vault == LoginVault.State.LOCKED)

    data class SaveContext(
        val private: Boolean,
        /** The origin of the form (or of the server of an HTTP sign-in). */
        val origin: String?,
        val passwordEmpty: Boolean,
        val vault: LoginVault.State,
        val neverSave: Boolean,
        val offersEnabled: Boolean,
        /** The vault's verdict when it is unlocked. */
        val known: SaveDecision?,
    )

    enum class SaveKind { NEW, UPDATE, LOCKED, NO_VAULT }

    sealed class Save {
        data class No(val why: String) : Save()

        data class Offer(val kind: SaveKind, val updateId: String?, val warnInsecure: Boolean) : Save()
    }

    fun save(c: SaveContext): Save {
        if (c.private) return Save.No("private tab")
        val origin = c.origin?.let { originOf(it) }
        if (!isWeb(origin)) return Save.No("not a web page")
        if (c.passwordEmpty) return Save.No("no password")
        if (!c.offersEnabled) return Save.No("offers are off")
        if (c.neverSave) return Save.No("never for this site")
        val insecure = origin?.startsWith("http://") == true
        return when (c.vault) {
            LoginVault.State.KEY_LOST -> Save.No("vault key lost")

            LoginVault.State.NONE -> Save.Offer(SaveKind.NO_VAULT, null, insecure)

            LoginVault.State.LOCKED -> Save.Offer(SaveKind.LOCKED, null, insecure)

            LoginVault.State.UNLOCKED -> when (val k = c.known) {
                null -> Save.No("vault did not answer")
                SaveDecision.Unchanged -> Save.No("already saved")
                SaveDecision.New -> Save.Offer(SaveKind.NEW, null, insecure)
                is SaveDecision.Update -> Save.Offer(SaveKind.UPDATE, k.id, insecure)
            }
        }
    }

    /**
     * Is a prompt raised for ([promptTab], [promptOrigin]) still good when the user answers? Only
     * while that tab is on screen and still shows that origin (as `core::vault::prompt_still_valid`,
     * but exact: a prompt never follows the tab to another host).
     */
    fun stillValid(promptTab: UInt, promptOrigin: String, activeTab: UInt?, activeUrl: String): Boolean =
        activeTab == promptTab && originOf(activeUrl) == originOf(promptOrigin)

    /** A save offer follows a sign-in to the next page of the same site, not further. */
    fun saveStillValid(promptTab: UInt, promptOrigin: String, activeTab: UInt?, activeUrl: String): Boolean {
        if (activeTab != promptTab) return false
        val a = originOf(promptOrigin) ?: return false
        val b = originOf(activeUrl) ?: return false
        return a == b || (a.startsWith("https://") && b.startsWith("https://") && sameSite(hostOf(a), hostOf(b)))
    }

    private fun hostOf(origin: String): String = origin.substringAfter("://").substringBefore(':')

    /** One host is the other or a subdomain of it (`example.com` and `accounts.example.com`). */
    private fun sameSite(a: String, b: String): Boolean = a == b || a.endsWith(".$b") || b.endsWith(".$a")
}
