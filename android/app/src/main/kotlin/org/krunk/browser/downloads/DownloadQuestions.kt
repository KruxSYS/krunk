package org.krunk.browser.downloads

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * A download a page started (a response the engine cannot show), waiting for the user's answer.
 * [fileName] and [host] are what the question shows; [private] says it comes from a private tab,
 * [tabId] from which tab (the question goes when that tab closes).
 */
class PendingDownload(
    val response: DownloadResponse,
    val fileName: String,
    val host: String,
    val private: Boolean,
    val pageUrl: String?,
    val tabId: UInt? = null,
)

/**
 * The questions "Download this file?" that wait for an answer, one shown at a time. Nothing is
 * stored before the user said yes, and a response that is not downloaded is always closed:
 * when the answer is no, when its tab closes, when the private session ends (private ones), and
 * when more than [max] wait (a page cannot pile up open responses). Lives with the app, not
 * with a screen.
 */
class DownloadQuestions(private val max: Int = MAX_WAITING) {
    /** What became of an offered question. */
    enum class Offer {
        ASKED,

        /** Too many wait: dropped (closed). The first drop of a burst: the user is told once. */
        DROPPED_TELL,

        /** Dropped again in the same burst (until no question waits any more): nothing to say. */
        DROPPED,
    }

    private val queue = ArrayDeque<PendingDownload>()
    private val _current = MutableStateFlow<PendingDownload?>(null)
    private var told = false

    /** The question to show now. */
    val current: StateFlow<PendingDownload?> = _current.asStateFlow()

    val waiting: Int
        @Synchronized get() = queue.size

    /** Adds a question; when too many wait already its response is closed. */
    @Synchronized
    fun offer(download: PendingDownload): Offer {
        if (queue.size >= max) {
            download.response.close()
            if (told) return Offer.DROPPED
            told = true
            return Offer.DROPPED_TELL
        }
        queue.addLast(download)
        _current.value = queue.first()
        return Offer.ASKED
    }

    /**
     * The user's answer. Returns [download] when it is to be saved, else null (declined: its
     * response is closed; or it was not waiting any more).
     */
    @Synchronized
    fun answer(download: PendingDownload, accept: Boolean): PendingDownload? {
        val known = queue.remove(download)
        changed()
        if (!known) return null
        if (!accept) {
            download.response.close()
            return null
        }
        return download
    }

    /** Drops the questions of tabs that are not [open] any more (their responses are closed). */
    @Synchronized
    fun keepTabs(open: Set<UInt>) = drop { it.tabId != null && it.tabId !in open }

    /** The private session ended: its questions go (their responses are closed). */
    @Synchronized
    fun dropPrivate() = drop { it.private }

    /** Drops every question (their responses are closed). */
    @Synchronized
    fun clear() = drop { true }

    private fun drop(which: (PendingDownload) -> Boolean) {
        val gone = queue.filter(which)
        if (gone.isEmpty()) return
        queue.removeAll(gone.toSet())
        gone.forEach { it.response.close() }
        changed()
    }

    private fun changed() {
        _current.value = queue.firstOrNull()
        // A burst ends when no question waits any more.
        if (queue.isEmpty()) told = false
    }

    companion object {
        const val MAX_WAITING = 4
    }
}
