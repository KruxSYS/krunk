package org.krunk.browser.downloads

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * A download a page started (a response the engine cannot show), waiting for the user's answer.
 * [fileName] and [host] are what the question shows; [private] says it comes from a private tab.
 */
class PendingDownload(
    val response: DownloadResponse,
    val fileName: String,
    val host: String,
    val private: Boolean,
    val pageUrl: String?,
)

/**
 * The questions "Download this file?" that wait for an answer, one shown at a time. Nothing is
 * stored before the user said yes, and a response that is not downloaded is always closed:
 * when the answer is no, and when more than [max] wait (a page cannot pile up open responses).
 * Lives with the app, not with a screen.
 */
class DownloadQuestions(private val max: Int = MAX_WAITING) {
    private val queue = ArrayDeque<PendingDownload>()
    private val _current = MutableStateFlow<PendingDownload?>(null)

    /** The question to show now. */
    val current: StateFlow<PendingDownload?> = _current.asStateFlow()

    val waiting: Int
        @Synchronized get() = queue.size

    /** Adds a question; false when too many wait already (the response is closed). */
    @Synchronized
    fun offer(download: PendingDownload): Boolean {
        if (queue.size >= max) {
            download.response.close()
            return false
        }
        queue.addLast(download)
        _current.value = queue.first()
        return true
    }

    /**
     * The user's answer. Returns [download] when it is to be saved, else null (declined: its
     * response is closed; or it was not waiting any more).
     */
    @Synchronized
    fun answer(download: PendingDownload, accept: Boolean): PendingDownload? {
        val known = queue.remove(download)
        _current.value = queue.firstOrNull()
        if (!known) return null
        if (!accept) {
            download.response.close()
            return null
        }
        return download
    }

    /** Drops every question (their responses are closed). */
    @Synchronized
    fun clear() {
        queue.forEach { it.response.close() }
        queue.clear()
        _current.value = null
    }

    companion object {
        const val MAX_WAITING = 4
    }
}
