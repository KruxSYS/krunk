package org.krunk.browser.downloads

/** Which posted notifications are downloads' and stale (plain Kotlin, see [DownloadNotifier.sweep]). */
object DownloadNoteIds {
    /**
     * The notification ids among [active] that are a download's (`[base, limit)`, the id is
     * `base + download id`) whose download is not in [keep].
     */
    fun stale(active: List<Int>, keep: Set<UInt>, base: Int, limit: Int): List<Int> =
        active.filter { it in base until limit && (it - base).toUInt() !in keep }
}

/**
 * When the foreground service is (re)started; the rule of [DownloadController] and
 * [DownloadService] in plain Kotlin.
 *
 * - The service is asked for while a download runs and no start is pending ([up]).
 * - Android refused to start it (`startForegroundService` threw: Krunk was in the background):
 *   the downloads go on without it; it is asked for again at the next change of state and when
 *   Krunk comes to the foreground ([unblock]).
 * - The service started but could not become a foreground one (`startForeground` threw, e.g.
 *   Android 15's daily time limit is used up): [blocked]. The service stops, the downloads are
 *   treated as at Android's time limit (resumable ones paused, the rest interrupted), and no new
 *   start is tried until Krunk comes to the foreground or the user starts or resumes a download.
 *   So a refusal never turns into a loop of starts.
 */
class ServiceGate {
    private var up = false
    private var blocked = false

    /** True when the caller is to start the service now (it is then counted as up). */
    @Synchronized
    fun shouldStart(running: Int): Boolean {
        if (running <= 0 || up || blocked) return false
        up = true
        return true
    }

    /** `startForegroundService` was refused: nothing is up. */
    @Synchronized
    fun startRefused() {
        up = false
    }

    /** The service could not become a foreground one: no start until [unblock]. */
    @Synchronized
    fun foregroundRefused() {
        blocked = true
    }

    /** The service ended. */
    @Synchronized
    fun stopped() {
        up = false
    }

    /** Krunk came to the foreground, or the user started or resumed a download. */
    @Synchronized
    fun unblock() {
        blocked = false
    }
}
