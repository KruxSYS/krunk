package org.krunk.browser.engine.permissions

/**
 * Krunk's record of permission answers: the shared table (`site_permissions`, through the
 * `ffi` methods), one row per origin (`scheme://host[:port]`) and kind key. No row means the
 * kind's default ([PermissionKind.default]).
 */
interface PermissionStore {
    fun get(origin: String, kind: String): Boolean?

    fun set(origin: String, kind: String, allow: Boolean)

    fun remove(origin: String, kind: String)
}

/**
 * Who decides a site's request, and what is remembered. Pure: the engine and Android sides are
 * in `SitePermissions.kt`.
 *
 * **The division between Krunk's table and the engine's own permission store** (decided after
 * reading `GeckoViewPermission.sys.mjs` and `GeckoViewStorageController.sys.mjs` of GeckoView 160):
 *
 * - Krunk's table is the record for every kind. The screens list and change it, and every
 *   request that reaches the delegate is answered from it (a stored allow or deny: no prompt).
 * - The engine writes each answer the delegate gives for a content kind into its own store by
 *   itself, without an end date (`Services.perms.addFromPrincipal(..., EXPIRE_NEVER)`; storage
 *   access keeps its own entry), and for some kinds (notifications at least) it reads that
 *   store before it asks anyone. It never stores camera or microphone answers (the camera gets
 *   a one-shot entry). So the engine's store is treated as a copy that must agree:
 *   - an answer that is a row in the table is given to the engine as allow/deny, so both hold it;
 *   - an allow that is *not* a row (not remembered, a default, a private tab) is still written
 *     by the engine; Krunk forgets that entry again when the tab loads another document or
 *     closes ([EngineEntries]). A deny that is not a row is answered so that the engine stores
 *     nothing at all;
 *   - a change on the screens changes the engine's entry of that origin in the same step, and
 *     "Ask" removes both, so the next request reaches the delegate and finds no row: a prompt;
 *   - at start the engine's entries are compared with the table once and corrected (a process
 *     that died before it could forget a one-time allow).
 * - Camera and microphone exist only in Krunk's table; the engine asks for them every time.
 *
 * **Private tabs** read and write nothing of the table. A remembered private answer lives in
 * this object until the last private tab closes ([privateSessionEnded]); the engine's entry for
 * it is one of the "not a row" entries above.
 *
 * **Prompt spam**: after a deny (a tap on Deny, a dismissed prompt, or Android refusing its own
 * permission) the same kind is denied without a prompt in that tab until it loads another
 * document ([newDocument]), also when the deny was not remembered.
 */
class PermissionPolicy(private val store: PermissionStore) {
    sealed class Decision {
        /** Every kind is allowed (stored, or by default): no prompt. */
        data object Allow : Decision()

        /** At least one kind is denied: the whole request is. [stored]: by a remembered answer. */
        data class Deny(val stored: Boolean) : Decision()

        /** These kinds need the user's answer (the others of the request are allowed already). */
        data class Ask(val kinds: List<PermissionKind>) : Decision()
    }

    private val privateAnswers = HashMap<Pair<String, String>, Boolean>()
    private val deniedThisLoad = HashMap<UInt, MutableSet<PermissionKind>>()

    /** The remembered answer: the table's for a normal tab, this session's for a private one. */
    fun remembered(origin: String?, private: Boolean, kind: PermissionKind): Boolean? = when {
        origin == null -> null
        private -> privateAnswers[origin to kind.key]
        else -> store.get(origin, kind.key)
    }

    /**
     * What to do with a request of [tab] for [kinds], asked by [origin] (null: a page without a
     * web origin, which is asked every time and never remembered).
     */
    fun decide(tab: UInt, origin: String?, private: Boolean, kinds: List<PermissionKind>): Decision {
        val ask = ArrayList<PermissionKind>()
        for (kind in kinds) {
            val remembered = remembered(origin, private, kind)
            if (remembered == false) return Decision.Deny(stored = true)
            // Also over a stored allow: Android refused its own permission on this page.
            if (deniedThisLoad[tab]?.contains(kind) == true) return Decision.Deny(stored = false)
            if (remembered == true) continue
            when (kind.default) {
                PermissionDefault.ALLOW -> Unit
                PermissionDefault.DENY -> return Decision.Deny(stored = false)
                PermissionDefault.ASK -> ask += kind
            }
        }
        return if (ask.isEmpty()) Decision.Allow else Decision.Ask(ask)
    }

    /** Whether the prompt can offer "Remember" at all. */
    fun canRemember(origin: String?): Boolean = origin != null

    /**
     * The user answered a prompt. A deny counts for the rest of this page load in any case;
     * with [remember] the answer is kept (the table, or this private session).
     */
    fun answered(
        tab: UInt,
        origin: String?,
        private: Boolean,
        kinds: List<PermissionKind>,
        allow: Boolean,
        remember: Boolean,
    ) {
        if (!allow) deniedForNow(tab, kinds)
        if (!remember || origin == null) return
        for (kind in kinds) {
            if (allow && !kind.rememberAllow) continue
            if (private) privateAnswers[origin to kind.key] = allow else store.set(origin, kind.key, allow)
        }
    }

    /** Denied without a stored answer (dismissed, or Android said no): quiet until the next document. */
    fun deniedForNow(tab: UInt, kinds: List<PermissionKind>) {
        deniedThisLoad.getOrPut(tab) { HashSet() }.addAll(kinds)
    }

    /**
     * Whether an answer of the engine's own store for this origin and kind is backed by a row
     * of the table. Anything else the engine wrote must be forgotten again (see the class text).
     */
    fun isRow(origin: String?, private: Boolean, kind: PermissionKind): Boolean =
        !private && origin != null && store.get(origin, kind.key) != null

    /**
     * Whether a deny must be written into the engine's store too: only when the table holds
     * that deny (anything else is answered so that the engine stores nothing).
     */
    fun isDenyRow(origin: String?, private: Boolean, kind: PermissionKind): Boolean =
        !private && origin != null && store.get(origin, kind.key) == false

    /** The user changed an answer on the screens: it counts from the next request on, in every tab. */
    fun changedByUser() {
        deniedThisLoad.clear()
    }

    /** The tab loads another document: it may ask again. */
    fun newDocument(tab: UInt) {
        deniedThisLoad.remove(tab)
    }

    fun tabClosed(tab: UInt) {
        deniedThisLoad.remove(tab)
    }

    /** The last private tab closed: nothing of the private session is left. */
    fun privateSessionEnded() {
        privateAnswers.clear()
    }

    /** For tests and the log: how many private answers are held. */
    val privateAnswerCount: Int get() = privateAnswers.size
}

/**
 * The engine's entries that no row of the table backs: each belongs to the tab whose request
 * made the engine write it, and is forgotten when that tab loads another document or closes.
 * [E] is the engine's permission object (kept generic so this runs in plain JVM tests).
 */
class EngineEntries<E> {
    private val byTab = HashMap<UInt, MutableList<E>>()

    fun add(tab: UInt, entry: E) {
        byTab.getOrPut(tab) { ArrayList() } += entry
    }

    /** The entries to forget now that [tab] left its page (or closed). */
    fun take(tab: UInt): List<E> = byTab.remove(tab).orEmpty()

    /** Everything, when all of it must go (clearing data). */
    fun takeAll(): List<E> {
        val all = byTab.values.flatten()
        byTab.clear()
        return all
    }

    val size: Int get() = byTab.values.sumOf { it.size }
}

/**
 * What the engine's store should hold for one of its entries, given the table: the row's value,
 * or nothing. Used at start to correct what an earlier process left behind.
 */
object EngineAgreement {
    const val FORGET = 0
    const val ALLOW = 1
    const val DENY = 2

    /**
     * The value to set for an engine entry of [geckoType] at [origin] currently holding
     * [engineValue], or null when it already agrees or is none of Krunk's business (private
     * entries end with the process; tracking exceptions and storage access are the engine's own).
     */
    fun correction(geckoType: Int, origin: String?, private: Boolean, engineValue: Int, store: PermissionStore): Int? {
        if (private || origin == null) return null
        val kind = PermissionKind.ofGecko(geckoType) ?: return null
        if (kind == PermissionKind.STORAGE_ACCESS) return null
        val want = when (store.get(origin, kind.key)) {
            true -> ALLOW
            false -> DENY
            null -> FORGET
        }
        return if (want == engineValue) null else want
    }
}

/** A camera a site may be given, as the prompt names it. */
data class CameraChoice(val id: String, val facing: Facing, val name: String) {
    enum class Facing { FRONT, BACK, OTHER }

    companion object {
        private const val MAX_NAME = 60

        /**
         * The engine names Android's cameras like "Camera 1, Facing front, Orientation 270";
         * the prompt says "Front camera" or "Back camera" and falls back to the (bounded) name.
         */
        fun of(id: String, name: String?): CameraChoice {
            val n = name.orEmpty()
            val lower = n.lowercase()
            val facing = when {
                "front" in lower || "user" in lower -> Facing.FRONT
                "back" in lower || "rear" in lower || "environment" in lower -> Facing.BACK
                else -> Facing.OTHER
            }
            return CameraChoice(id, facing, n.filter { it >= ' ' }.take(MAX_NAME))
        }
    }
}
