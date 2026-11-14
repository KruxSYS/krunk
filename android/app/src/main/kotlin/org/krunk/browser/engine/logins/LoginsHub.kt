package org.krunk.browser.engine.logins

import android.util.Log
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.originOf
import org.krunk.browser.vault.VaultFailure
import org.krunk.browser.vault.VaultSecret
import org.mozilla.geckoview.Autocomplete
import org.mozilla.geckoview.GeckoResult

/** A page a tab shows, as far as the engine's login lookup needs to know. */
data class LoginPage(val url: String, val private: Boolean, val armedOrigin: String?)

/**
 * The engine's login storage as Krunk answers it, one for the runtime.
 *
 * The engine asks for the logins of a domain whenever a page has a sign-in form and again when
 * a field is tapped; it is not told which tab asks. Krunk answers with **entries that carry no
 * password**: one per saved login of the pages its tabs show (so the engine knows a tap on the
 * field should ask Krunk), or one nameless stand-in while the vault is locked. The real password
 * goes to the engine only for the login the user picks, in the tab's own prompt
 * (`KrunkSession.onLoginSelect`), where the tab, its privacy and its address are known. The
 * engine's own filling is off (`loginAutofillEnabled(false)`), and nothing is ever stored
 * through this delegate.
 */
class LoginsHub {
    private val failed = CoroutineExceptionHandler { _, e ->
        Log.w(KrunkApp.TAG, "logins: task failed: ${e.javaClass.simpleName}")
    }
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate + failed)

    /** Set once the app has its vault; without it nothing is offered. */
    @Volatile
    var vault: LoginVault? = null

    /** The pages of the open tabs (set by the tab manager). */
    @Volatile
    var pages: () -> List<LoginPage> = { emptyList() }

    /** Tells the UI something in one line (a login was saved after the vault was created). */
    @Volatile
    var notify: (LoginNotice) -> Unit = {}

    enum class LoginNotice { SAVED, SAVE_FAILED }

    /** A login waiting for "Create vault": kept in memory, a few minutes at most. */
    private class Pending(val origin: String, val secret: VaultSecret, val until: Long)

    private var pending: Pending? = null

    private var pendingExpiry: Job? = null

    fun keepForNewVault(origin: String, secret: VaultSecret, now: Long) {
        val kept = Pending(origin, secret, now + PENDING_MS)
        pending = kept
        // Forgotten when the time is up, whether or not a vault was made.
        pendingExpiry?.cancel()
        pendingExpiry = scope.launch {
            delay(PENDING_MS)
            if (pending === kept) pending = null
        }
    }

    /** For tests: is a login waiting for a new vault? */
    fun hasPending(): Boolean = pending != null

    /** The vault changed state: an open one takes the waiting login; a lock forgets it. */
    fun onVaultState(state: LoginVault.State, now: Long) {
        val p = pending ?: return
        if (state == LoginVault.State.LOCKED || now > p.until) {
            pending = null
            return
        }
        if (state != LoginVault.State.UNLOCKED) return
        pending = null
        val v = vault ?: return
        scope.launch {
            try {
                v.use { it.saveLogin(p.origin, p.secret.username, p.secret.password, now) }
                notify(LoginNotice.SAVED)
            } catch (e: VaultFailure) {
                Log.w(KrunkApp.TAG, "logins: saving after vault creation failed: ${e.javaClass.simpleName}")
                notify(LoginNotice.SAVE_FAILED)
            }
        }
    }

    val storage = object : Autocomplete.StorageDelegate {
        override fun onLoginFetch(domain: String): GeckoResult<Array<Autocomplete.LoginEntry>> {
            val result = GeckoResult<Array<Autocomplete.LoginEntry>>()
            val v = vault
            val state = v?.state()
            if (v == null || state == null) {
                result.complete(emptyArray())
                return result
            }
            val origins = announcedOrigins(domain, pages(), state)
            when {
                origins.isEmpty() -> result.complete(emptyArray())

                state == LoginVault.State.LOCKED -> result.complete(
                    origins.filter { it.armed || v.hasHint(it.origin) }
                        .map { placeholder("locked", it.origin, LoginPolicy.LOCKED_PLACEHOLDER_USER) }
                        .toTypedArray(),
                )

                else -> scope.launch {
                    // Whatever happens, the engine gets its answer: a lookup left open would
                    // hold up the page's form handling.
                    var entries = emptyList<Autocomplete.LoginEntry>()
                    try {
                        // peek: the engine asking is not the user using the vault.
                        entries = v.peek { b ->
                            origins.flatMap { o ->
                                b.matchesFor(o.origin).map { placeholder(it.id, o.origin, it.username) }
                            }
                        }
                    } catch (e: VaultFailure) {
                        // Locked meanwhile, or the vault could not answer: nothing to offer.
                    } finally {
                        result.complete(entries.toTypedArray())
                    }
                }
            }
            return result
        }

        // Nothing is stored through the engine: Krunk answers its save request with a dismissal
        // and asks the user itself (see KrunkSession.onLoginSave).
        override fun onLoginSave(login: Autocomplete.LoginEntry) = Unit
    }

    private fun placeholder(guid: String, origin: String, user: String): Autocomplete.LoginEntry =
        Autocomplete.LoginEntry.Builder()
            .guid(guid)
            .origin(origin)
            .formActionOrigin(origin)
            .username(user)
            .password(LoginPolicy.PLACEHOLDER_PASSWORD)
            .build()

    companion object {
        private const val PENDING_MS = 5 * 60_000L

        data class Announced(val origin: String, val armed: Boolean)

        /**
         * The origins of open tabs the engine may hear about for its lookup of [domain]: web
         * pages of that domain, in normal tabs or in a private tab armed for exactly that page.
         */
        fun announcedOrigins(domain: String, pages: List<LoginPage>, state: LoginVault.State): List<Announced> {
            val d = domain.lowercase()
            return pages.mapNotNull { p ->
                val origin = originOf(p.url) ?: return@mapNotNull null
                if (!LoginPolicy.announce(p.url, state)) return@mapNotNull null
                val hostPort = origin.substringAfter("://")
                // An IPv6 host keeps its brackets; otherwise only a port follows a colon.
                val host = if (hostPort.startsWith(
                        "[",
                    )
                ) {
                    hostPort.substringBefore("]") + "]"
                } else {
                    hostPort.substringBefore(':')
                }
                if (host != d && !host.endsWith(".$d")) return@mapNotNull null
                val armed = p.armedOrigin == origin
                if (p.private && !armed) return@mapNotNull null
                Announced(origin, armed)
            }.distinctBy { it.origin }
        }
    }
}
