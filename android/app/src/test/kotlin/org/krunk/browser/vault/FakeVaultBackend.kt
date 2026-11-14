package org.krunk.browser.vault

import java.net.URI

/**
 * An in-memory [VaultBackend] for JVM tests: no file, no cryptography, but the documented
 * behaviour of the real one.
 *
 * - **States:** [VaultFileState.NONE] until [create]; then locked or unlocked. Calls that need
 *   the open vault throw [VaultFailure.Locked] (or [VaultFailure.NoVault] without a vault).
 * - **Passwords:** at least [minPasswordLength] characters, else [VaultFailure.WeakPassword].
 * - **Rate limit:** after the n-th wrong password in a row the next attempt must wait
 *   1 s x 2^(n-1), at most 60 s; an attempt before that throws [VaultFailure.RateLimited]
 *   without looking at the password. A successful unlock clears the count.
 * - **Unlock key:** 32 bytes, new at [create] and at every [changePassword] (so a biometric
 *   blob made before the change no longer opens the vault: [unlockWithKey] throws
 *   [VaultFailure.WrongPassword]). The blob itself is kept until someone removes it.
 * - **Matching** ([matchesFor], [secretForFill], [saveDecision], [hasHint]): the exact origin
 *   (scheme, host, port), else for https on both sides the same last two host labels (a
 *   stand-in for the registrable domain: good for `example.com`, wrong for `co.uk`).
 *   [forHttpAuth] is exact origin only.
 * - **Lock timer:** [autolockDue] is true once `minutes` have passed since the last [touch]
 *   (or unlock) while unlocked.
 *
 * The switches ([keyLost], [failCreate]) and counters are for tests to set and read.
 */
class FakeVaultBackend : VaultBackend {
    private class Entry(
        val id: String,
        var origin: String,
        var username: String,
        var password: String,
        var note: String,
        val created: Long,
        var updated: Long,
    )

    private var password: String? = null
    private var open = false
    private var kek = ByteArray(0)
    private var kekSeed = 0
    private val entries = LinkedHashMap<String, Entry>()
    private var nextId = 1
    private var failures = 0
    private var lastFailure = 0L
    private var lastUse = 0L
    private var blob: ByteArray? = null

    /** The device key is gone: [unlock] and [unlockWithKey] throw [VaultFailure.KeyLost]. */
    var keyLost = false

    /** The key store does not answer: [unlock] throws [VaultFailure.KeyUnavailable]. */
    var keyUnavailable = false

    /** When set, [create] throws it and makes nothing. */
    var failCreate: VaultFailure? = null

    /** How often [lock] was called. */
    var lockCalls = 0
        private set

    /** How often [unlockWithKey] was called. */
    var keyUnlocks = 0
        private set

    val entryCount: Int get() = entries.size

    override fun state(): VaultFileState = when {
        password == null -> VaultFileState.NONE
        open -> VaultFileState.UNLOCKED
        else -> VaultFileState.LOCKED
    }

    override fun create(password: String, now: Long) {
        failCreate?.let { throw it }
        if (this.password != null) throw VaultFailure.AlreadyExists()
        if (password.length < minPasswordLength()) throw VaultFailure.WeakPassword()
        this.password = password
        newKek()
        open = true
        lastUse = now
    }

    override fun unlock(password: String, now: Long) {
        val expected = this.password ?: throw VaultFailure.NoVault()
        if (keyLost) throw VaultFailure.KeyLost("fake: no device key")
        if (keyUnavailable) throw VaultFailure.KeyUnavailable("fake: key store silent")
        val wait = retryWaitMs(now)
        if (wait > 0) throw VaultFailure.RateLimited(wait)
        if (password != expected) {
            failures++
            lastFailure = now
            throw VaultFailure.WrongPassword()
        }
        failures = 0
        open = true
        lastUse = now
    }

    override fun retryWaitMs(now: Long): Long {
        if (failures == 0) return 0
        val delay = minOf(1000L shl minOf(failures - 1, 16), 60_000L)
        return maxOf(0, lastFailure + delay - now)
    }

    override fun unlockWithKey(key: ByteArray) {
        keyUnlocks++
        if (password == null) throw VaultFailure.NoVault()
        if (keyLost) throw VaultFailure.KeyLost("fake: no device key")
        if (!key.contentEquals(kek)) throw VaultFailure.WrongPassword()
        failures = 0
        open = true
    }

    override fun unlockKey(): ByteArray {
        requireOpen()
        return kek.copyOf()
    }

    override fun lock() {
        lockCalls++
        open = false
    }

    override fun changePassword(old: String, new: String) {
        requireOpen()
        if (old != password) throw VaultFailure.WrongPassword()
        if (new.length < minPasswordLength()) throw VaultFailure.WeakPassword()
        password = new
        newKek()
    }

    override fun search(query: String): List<VaultItem> {
        requireOpen()
        val q = query.trim().lowercase()
        return entries.values
            .filter { q.isEmpty() || it.origin.lowercase().contains(q) || it.username.lowercase().contains(q) }
            .map { VaultItem(it.id, it.origin, it.username, it.note.isNotEmpty(), it.created, it.updated) }
    }

    override fun entry(id: String): VaultItemDetail {
        val e = find(id)
        return VaultItemDetail(e.id, e.origin, e.username, e.note, e.created, e.updated)
    }

    override fun reveal(id: String): String = find(id).password

    override fun saveLogin(url: String, username: String, password: String, now: Long): String =
        when (val decision = saveDecision(url, username, password)) {
            is SaveDecision.Update -> upsert(decision.id, url, username, password, null, now)
            SaveDecision.New -> upsert(null, url, username, password, null, now)
            SaveDecision.Unchanged -> entries.values.first { it.origin == originOf(url) && it.username == username }.id
        }

    override fun upsert(
        id: String?,
        url: String,
        username: String,
        password: String?,
        note: String?,
        now: Long,
    ): String {
        requireOpen()
        val origin = originOf(url) ?: throw VaultFailure.InvalidEntry("not a web address")
        if (id == null) {
            if (password.isNullOrEmpty()) throw VaultFailure.InvalidEntry("no password")
            val entry = Entry("id-${nextId++}", origin, username, password, note.orEmpty(), now, now)
            entries[entry.id] = entry
            return entry.id
        }
        val entry = find(id)
        entry.origin = origin
        entry.username = username
        if (password != null) entry.password = password
        if (note != null) entry.note = note
        entry.updated = now
        return id
    }

    override fun delete(id: String) {
        find(id)
        entries.remove(id)
    }

    override fun matchesFor(url: String): List<VaultMatch> {
        requireOpen()
        return matching(url)
    }

    override fun forHttpAuth(url: String): List<VaultMatch> {
        requireOpen()
        return matching(url).filter { it.exact }
    }

    override fun secretForFill(id: String, pageUrl: String): VaultSecret {
        val entry = find(id)
        if (matching(pageUrl).none { it.id == id }) throw VaultFailure.NotAllowed()
        return VaultSecret(entry.username, entry.password)
    }

    override fun saveDecision(url: String, username: String, password: String): SaveDecision {
        requireOpen()
        val origin = originOf(url) ?: throw VaultFailure.InvalidEntry("not a web address")
        val known = entries.values.firstOrNull { it.origin == origin && it.username == username }
        return when {
            known == null -> SaveDecision.New
            known.password == password -> SaveDecision.Unchanged
            else -> SaveDecision.Update(known.id)
        }
    }

    override fun hasHint(url: String): Boolean = password != null && matching(url).isNotEmpty()

    override fun bioBlob(): ByteArray? = blob?.copyOf()

    override fun setBioBlob(blob: ByteArray?) {
        this.blob = blob?.copyOf()
    }

    override fun touch(now: Long) {
        lastUse = now
    }

    override fun autolockDue(now: Long, minutes: Int): Boolean = open && now - lastUse >= maxOf(minutes, 1) * 60_000L

    override fun destroy() {
        password = null
        open = false
        entries.clear()
        blob = null
        failures = 0
    }

    override fun minPasswordLength(): Int = 8

    private fun requireOpen() {
        if (password == null) throw VaultFailure.NoVault()
        if (!open) throw VaultFailure.Locked()
    }

    private fun find(id: String): Entry {
        requireOpen()
        return entries[id] ?: throw VaultFailure.NotFound()
    }

    private fun newKek() {
        kekSeed++
        kek = ByteArray(32) { (it * 7 + kekSeed).toByte() }
    }

    /** Exact matches first, then same-site ones. */
    private fun matching(url: String): List<VaultMatch> {
        val page = originOf(url) ?: return emptyList()
        return entries.values.mapNotNull { e ->
            when {
                e.origin == page -> VaultMatch(e.id, e.origin, e.username, exact = true)
                sameSite(e.origin, page) -> VaultMatch(e.id, e.origin, e.username, exact = false)
                else -> null
            }
        }.sortedByDescending { it.exact }
    }

    companion object {
        /** `scheme://host[:port]` in lower case, the default port left out; null if not http(s). */
        fun originOf(url: String): String? {
            val uri = runCatching { URI(url.trim()) }.getOrNull() ?: return null
            val scheme = uri.scheme?.lowercase() ?: return null
            val host = uri.host?.lowercase() ?: return null
            val default = when (scheme) {
                "https" -> 443
                "http" -> 80
                else -> return null
            }
            val port = if (uri.port == -1 || uri.port == default) "" else ":${uri.port}"
            return "$scheme://$host$port"
        }

        private fun sameSite(a: String, b: String): Boolean {
            if (!a.startsWith("https://") || !b.startsWith("https://")) return false
            val siteA = siteOf(a) ?: return false
            return siteA == siteOf(b)
        }

        /** The last two host labels; none for IP addresses and single-label hosts. */
        private fun siteOf(origin: String): String? {
            val host = URI(origin).host ?: return null
            val labels = host.split('.')
            if (labels.size < 2 || labels.all { label -> label.all(Char::isDigit) }) return null
            return labels.takeLast(2).joinToString(".")
        }
    }
}
