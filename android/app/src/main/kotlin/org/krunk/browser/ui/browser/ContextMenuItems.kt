package org.krunk.browser.ui.browser

import org.krunk.browser.engine.ContextTarget
import org.krunk.browser.engine.LoadPolicy

/** One entry of the link/image long-press menu. */
enum class ContextAction {
    OPEN_NEW_TAB,
    OPEN_PRIVATE_TAB,
    COPY_LINK,
    SHARE_LINK,
    COPY_LINK_TEXT,
    DOWNLOAD_LINK,
    OPEN_IMAGE_NEW_TAB,
    SAVE_IMAGE,
    COPY_IMAGE_LINK,
    SAVE_MEDIA,
}

/**
 * Addresses the menu opens in a tab or downloads: web addresses only. A page chooses its links'
 * targets, and Krunk must not load a `file:`, `data:` or `blob:` address for it that the engine
 * would refuse on a normal tap.
 */
private fun web(uri: String?): Boolean = uri != null && LoadPolicy.isWebUrl(uri.trim())

/** An address worth copying or sharing: anything but `javascript:` (`mailto:`, `tel:`, ... included). */
private fun copyable(uri: String?): Boolean =
    !uri.isNullOrBlank() && !uri.trim().startsWith("javascript:", ignoreCase = true)

/** Whether the Android `DownloadManager` can fetch this URL (it only speaks http/https). */
fun downloadable(uri: String?): Boolean {
    val u = uri?.lowercase() ?: return false
    return u.startsWith("http://") || u.startsWith("https://")
}

/**
 * The actions for a long-pressed element, in menu order. Links come first; an image inside a
 * link adds the image entries below. In a private tab "open in new tab" already opens a private
 * tab, so the separate private entry is left out. An empty list means there is nothing to offer
 * (the sheet is not shown).
 */
fun contextActions(target: ContextTarget, privateTab: Boolean): List<ContextAction> {
    val out = ArrayList<ContextAction>()
    if (target.isLink && copyable(target.linkUri)) {
        val open = web(target.linkUri)
        if (open) {
            out += ContextAction.OPEN_NEW_TAB
            if (!privateTab) out += ContextAction.OPEN_PRIVATE_TAB
        }
        out += ContextAction.COPY_LINK
        out += ContextAction.SHARE_LINK
        if (!target.copyableText.isNullOrEmpty()) out += ContextAction.COPY_LINK_TEXT
        if (open) out += ContextAction.DOWNLOAD_LINK
    }
    if (target.isImage && web(target.srcUri)) {
        out += ContextAction.OPEN_IMAGE_NEW_TAB
        out += ContextAction.SAVE_IMAGE
        out += ContextAction.COPY_IMAGE_LINK
    } else if (target.isMedia && web(target.srcUri)) {
        out += ContextAction.SAVE_MEDIA
    }
    return out
}

/**
 * A file name for a download: the `filename` of a Content-Disposition header when present,
 * else the URL's last path segment, else "download"; path separators and control characters
 * are never part of it.
 */
fun downloadFileName(url: String, contentDisposition: String?, fallback: String = "download"): String {
    fromContentDisposition(contentDisposition)?.let { return sanitize(it, fallback) }
    val path = url.substringBefore('#').substringBefore('?')
    val last = path.substringAfterLast('/', "")
    val decoded = runCatching { java.net.URLDecoder.decode(last, "UTF-8") }.getOrDefault(last)
    return sanitize(decoded, fallback)
}

private fun fromContentDisposition(header: String?): String? {
    header ?: return null
    // RFC 6266: filename*=UTF-8''encoded wins over filename="plain".
    val star = FILENAME_STAR.find(header)?.groupValues?.get(1)?.trim()
    if (!star.isNullOrEmpty()) {
        return runCatching { java.net.URLDecoder.decode(star.replace("+", "%2B"), "UTF-8") }.getOrDefault(star)
    }
    val plain = FILENAME_PLAIN.find(header) ?: return null
    return (plain.groupValues[1].ifEmpty { plain.groupValues[2] }).trim().ifEmpty { null }
}

// Header parameter names are case-insensitive (RFC 6266).
private val FILENAME_STAR = Regex("""filename\*\s*=\s*UTF-8''([^;]+)""", RegexOption.IGNORE_CASE)
private val FILENAME_PLAIN = Regex("""filename\s*=\s*(?:"([^"]*)"|([^;]+))""", RegexOption.IGNORE_CASE)

/** File systems allow 255 bytes for a name; this leaves room for the system's " (1)" suffixes. */
private const val MAX_NAME_BYTES = 200

/**
 * A name that is safe to show and to store: no path separators, no control characters and no
 * characters that reorder or hide text (a right-to-left override makes `photo[RLO]gpj.apk` read
 * as `photoapk.jpg`), no leading dots (hidden files), no trailing dots or spaces, and short
 * enough in bytes. Too long a name keeps its end, where the extension is.
 */
private fun sanitize(name: String, fallback: String): String {
    val cleaned = name
        .replace('\\', '_')
        .replace('/', '_')
        .filterNot(::isDeceptive)
        .trim()
        .trimStart('.')
        .trimEnd('.', ' ')
    if (cleaned.isEmpty()) return fallback
    return lastBytes(cleaned, MAX_NAME_BYTES)
}

/** The end of [s] that fits into [max] UTF-8 bytes, never cutting a character in two. */
private fun lastBytes(s: String, max: Int): String {
    var bytes = 0
    var start = s.length
    while (start > 0) {
        val cp = s.codePointBefore(start)
        val size = when {
            cp < 0x80 -> 1
            cp < 0x800 -> 2
            cp < 0x10000 -> 3
            else -> 4
        }
        if (bytes + size > max) break
        bytes += size
        start -= Character.charCount(cp)
    }
    return s.substring(start).trimStart('.', ' ').ifEmpty { s.takeLast(1) }
}
