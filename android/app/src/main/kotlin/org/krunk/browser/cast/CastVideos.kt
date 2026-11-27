package org.krunk.browser.cast

/**
 * One `<video>` element of a page, as the `krunk-sitedata` extension reports it (its
 * background.js, request "videos"). [duration] is -1 for a live stream (the element says
 * Infinity) and 0 when unknown; [mse] is true when the element plays from a `blob:` address
 * (Media Source Extensions or a local file) or a `srcObject` stream: nothing a Cast device can
 * fetch.
 */
data class PageVideo(
    val src: String,
    val type: String = "",
    val title: String = "",
    val poster: String = "",
    val currentTime: Double = 0.0,
    val duration: Double = 0.0,
    val mse: Boolean = false,
    val paused: Boolean = true,
    val area: Long = 0,
)

/** What is sent to the Cast device: the receiver fetches [url] itself. */
data class CastMedia(
    val url: String,
    val contentType: String,
    val title: String,
    /** An http(s) poster image, or null. */
    val poster: String?,
    /** Where to start, in milliseconds (0 for a live stream). */
    val startMs: Long,
    val live: Boolean,
)

/** The outcome of looking at a page's videos. */
sealed interface CastDecision {
    data class Castable(val media: CastMedia) : CastDecision

    /** The page has no video (or the extension could not look). */
    data object NoVideo : CastDecision

    /** There is a video, but no address a Cast device could load (blob:/MSE, data:, none). */
    data object NotCastable : CastDecision
}

/** Plain logic of the Cast entry: which video, whether it can be sent, its content type. */
object CastVideos {
    private const val MAX_URL = 4096

    /**
     * The video the user most likely means: a playing one before a paused one, then the
     * largest on screen, then the longest.
     */
    fun pick(videos: List<PageVideo>): PageVideo? = videos.maxWithOrNull(
        compareBy<PageVideo>({ !it.paused }, { it.area }, { it.duration }),
    )

    /** Whether a Cast receiver could load [url] by itself: an http(s) address, nothing else. */
    fun castableUrl(url: String): Boolean {
        if (url.isBlank() || url.length > MAX_URL) return false
        val lower = url.trim().lowercase()
        return (lower.startsWith("https://") || lower.startsWith("http://")) && lower.length > "https://".length
    }

    fun castable(video: PageVideo): Boolean = !video.mse && castableUrl(video.src)

    /**
     * The content type for the receiver: the page's declared type when it is a media type
     * (without codecs parameters), else a guess from the address's file extension, else
     * video/mp4.
     */
    fun contentType(url: String, declared: String = ""): String {
        val type = declared.substringBefore(';').trim().lowercase()
        if (type.startsWith("video/") || type.startsWith("audio/") || type in STREAM_TYPES) return type
        val path = url.substringBefore('#').substringBefore('?').lowercase()
        val ext = path.substringAfterLast('/').substringAfterLast('.', "")
        return EXTENSIONS[ext] ?: DEFAULT_TYPE
    }

    /** Picks the page's video and decides whether and how it can be sent. */
    fun decide(videos: List<PageVideo>, pageTitle: String): CastDecision {
        if (videos.isEmpty()) return CastDecision.NoVideo
        // A castable video is preferred to a better placed one that is not (an ad in a blob:).
        val video = pick(videos.filter(::castable)) ?: return CastDecision.NotCastable
        val live = video.duration < 0
        val start = if (live || !video.currentTime.isFinite() ||
            video.currentTime <= 0
        ) {
            0L
        } else {
            (video.currentTime * 1000).toLong()
        }
        return CastDecision.Castable(
            CastMedia(
                url = video.src.trim(),
                contentType = contentType(video.src, video.type),
                title = video.title.ifBlank {
                    pageTitle
                }.ifBlank { video.src.substringAfterLast('/').substringBefore('?') }.take(MAX_TITLE),
                poster = video.poster.takeIf(::castableUrl),
                startMs = start,
                live = live,
            ),
        )
    }

    private const val MAX_TITLE = 300
    private const val DEFAULT_TYPE = "video/mp4"
    private val STREAM_TYPES = setOf(
        "application/x-mpegurl",
        "application/vnd.apple.mpegurl",
        "application/dash+xml",
    )
    private val EXTENSIONS = mapOf(
        "mp4" to "video/mp4",
        "m4v" to "video/mp4",
        "webm" to "video/webm",
        "m3u8" to "application/x-mpegURL",
        "mpd" to "application/dash+xml",
        "ogv" to "video/ogg",
        "ogg" to "audio/ogg",
        "mkv" to "video/x-matroska",
        "mp3" to "audio/mpeg",
        "m4a" to "audio/mp4",
        "aac" to "audio/aac",
        "ts" to "video/mp2t",
    )
}
