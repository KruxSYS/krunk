package org.krunk.browser.ui.downloads

import org.krunk.browser.downloads.DownloadNames
import org.krunk.browser.ui.browser.pillText
import org.krunk.ffi.DownloadEntry
import org.krunk.ffi.DownloadPage
import org.krunk.ffi.DownloadState

/** How a row of the downloads screen reads. */
enum class RowStatus {
    RUNNING,
    PAUSED,

    /** Stopped by an error but able to go on: shown as failed, with "Retry". */
    FAILED_CAN_RETRY,
    DONE,
    FAILED,
    CANCELLED,
}

enum class RowAction {
    OPEN,
    SHARE,
    PAUSE,
    RESUME,
    RETRY,
    RESTART,
    CANCEL,
    REMOVE,
}

/** A line of explanation under a row. */
enum class RowNote {
    /** The server gave nothing to go on with: this download can only be cancelled. */
    CANNOT_PAUSE,

    /** A `blob:` or `data:` download: its address is gone, only the page can start it again. */
    AGAIN_FROM_PAGE,
}

/** Time left, as the screen words it. */
data class TimeLeft(val hours: Long, val minutes: Long, val seconds: Long)

data class DownloadRow(
    val id: UInt,
    val fileName: String,
    /** Where the file is once it is finished (see `DownloadPaths`). */
    val path: String,
    /** The site the file comes from (host), or empty. */
    val site: String,
    val status: RowStatus,
    val received: Long,
    val total: Long?,
    /** 0..1 when the size is known. */
    val fraction: Float?,
    /** Bytes per second while running, else 0. */
    val speed: Long,
    val timeLeft: TimeLeft?,
    val reason: String?,
    val private: Boolean,
    /** What a tap on the row does. */
    val primary: RowAction?,
    val actions: List<RowAction>,
    val note: RowNote?,
) {
    companion object {
        fun of(e: DownloadEntry): DownloadRow {
            val web = DownloadNames.isWeb(e.url)
            val status = when (e.state) {
                DownloadState.IN_PROGRESS -> RowStatus.RUNNING
                DownloadState.PAUSED -> if (pausedOnPurpose(e.reason)) RowStatus.PAUSED else RowStatus.FAILED_CAN_RETRY
                DownloadState.COMPLETED -> RowStatus.DONE
                DownloadState.INTERRUPTED -> RowStatus.FAILED
                DownloadState.CANCELLED -> RowStatus.CANCELLED
            }
            val canPause = e.resume != null
            val actions = when (status) {
                RowStatus.RUNNING -> if (canPause) {
                    listOf(
                        RowAction.PAUSE,
                        RowAction.CANCEL,
                    )
                } else {
                    listOf(RowAction.CANCEL)
                }

                RowStatus.PAUSED -> listOf(RowAction.RESUME, RowAction.CANCEL)

                RowStatus.FAILED_CAN_RETRY -> listOf(RowAction.RETRY, RowAction.CANCEL)

                RowStatus.DONE -> listOf(RowAction.OPEN, RowAction.SHARE, RowAction.REMOVE)

                RowStatus.FAILED, RowStatus.CANCELLED ->
                    if (web) listOf(RowAction.RESTART, RowAction.REMOVE) else listOf(RowAction.REMOVE)
            }
            val note = when {
                status == RowStatus.RUNNING && !canPause -> RowNote.CANNOT_PAUSE
                (status == RowStatus.FAILED || status == RowStatus.CANCELLED) && !web -> RowNote.AGAIN_FROM_PAGE
                else -> null
            }
            val total = e.total?.takeIf { it > 0 }
            val running = status == RowStatus.RUNNING
            val from = e.pageUrl?.takeIf { it.isNotBlank() } ?: e.url.takeIf { web }
            return DownloadRow(
                id = e.id,
                fileName = e.fileName,
                path = e.path,
                site = from?.let(::pillText).orEmpty(),
                status = status,
                received = e.received.coerceAtLeast(0),
                total = total,
                fraction = if (status == RowStatus.DONE) {
                    null
                } else {
                    total?.let { (e.received.toFloat() / it).coerceIn(0f, 1f) }
                },
                speed = if (running) e.speed.toLong() else 0,
                timeLeft = if (running) e.eta?.toLong()?.let(::timeLeft) else null,
                reason = e.reason?.takeIf { status != RowStatus.DONE && it.isNotBlank() },
                private = e.private,
                primary = when (status) {
                    RowStatus.DONE -> RowAction.OPEN
                    else -> null
                },
                actions = actions,
                note = note,
            )
        }

        /**
         * A pause the user (or closing Krunk, or Android's time limit) caused reads as paused;
         * any other reason on a paused entry is an error that "Retry" continues. The reasons
         * of the first kind all start with "Paused" (the core's "Paused when Krunk closed").
         */
        fun pausedOnPurpose(reason: String?): Boolean = reason.isNullOrBlank() || reason.startsWith("Paused")

        fun timeLeft(seconds: Long): TimeLeft {
            val s = seconds.coerceAtLeast(0)
            return TimeLeft(s / 3600, s % 3600 / 60, s % 60)
        }
    }
}

/** What the screen shows when the list is empty. */
enum class EmptyKind {
    NO_DOWNLOADS,
    NO_MATCH,
}

/**
 * The downloads screen's state: the search text, how many entries are asked for (it grows a
 * page at a time while scrolling) and the rows of the last answer. Running downloads are part
 * of the same answer (the list merges them), so a refresh is one query.
 */
data class DownloadsUiState(
    val query: String = "",
    val limit: Int = PAGE,
    val rows: List<DownloadRow> = emptyList(),
    /** Finished entries in the whole list (what "Clear finished" would take). */
    val finished: Int = 0,
    val loaded: Boolean = false,
) {
    /** More may be there: the last answer filled the limit. */
    val canLoadMore: Boolean get() = loaded && rows.size >= limit

    val canClearFinished: Boolean get() = finished > 0

    val empty: EmptyKind?
        get() = when {
            !loaded || rows.isNotEmpty() -> null
            query.isBlank() -> EmptyKind.NO_DOWNLOADS
            else -> EmptyKind.NO_MATCH
        }

    /** A new search starts at the first page. */
    fun search(text: String): DownloadsUiState = if (text == query) this else copy(query = text, limit = PAGE)

    fun more(): DownloadsUiState = if (canLoadMore) copy(limit = limit + PAGE) else this

    /** The answer to [query] and [limit]; one for another search is ignored. */
    fun answer(forQuery: String, page: DownloadPage): DownloadsUiState {
        if (forQuery != query) return this
        return copy(rows = page.items.map(DownloadRow::of), finished = (page.done + page.failed).toInt(), loaded = true)
    }

    companion object {
        const val PAGE = 50
    }
}
