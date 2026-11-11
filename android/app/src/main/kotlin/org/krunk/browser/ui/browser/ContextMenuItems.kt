package org.krunk.browser.ui.browser

import org.krunk.browser.engine.ContextTarget

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

/** Schemes a link menu makes sense for (no `javascript:`, `mailto:`, `tel:` ...). */
private fun webish(uri: String?): Boolean {
    val u = uri?.lowercase() ?: return false
    return u.startsWith("http://") || u.startsWith("https://") || u.startsWith("data:") || u.startsWith("blob:") ||
        u.startsWith("file:")
}

/** Whether the Android `DownloadManager` can fetch this URL (it only speaks http/https). */
fun downloadable(uri: String?): Boolean {
    val u = uri?.lowercase() ?: return false
    return u.startsWith("http://") || u.startsWith("https://")
}

/**
 * The actions for a long-pressed element, in menu order. Links come first; an image inside a
 * link adds the image entries below. In a private tab "open in new tab" already opens a private
 * tab, so the separate private entry is left out.
 */
fun contextActions(target: ContextTarget, privateTab: Boolean): List<ContextAction> {
    val out = ArrayList<ContextAction>()
    if (target.isLink && webish(target.linkUri)) {
        out += ContextAction.OPEN_NEW_TAB
        if (!privateTab) out += ContextAction.OPEN_PRIVATE_TAB
        out += ContextAction.COPY_LINK
        out += ContextAction.SHARE_LINK
        if (!target.copyableText.isNullOrEmpty()) out += ContextAction.COPY_LINK_TEXT
        if (downloadable(target.linkUri)) out += ContextAction.DOWNLOAD_LINK
    }
    if (target.isImage && webish(target.srcUri)) {
        out += ContextAction.OPEN_IMAGE_NEW_TAB
        if (downloadable(target.srcUri)) out += ContextAction.SAVE_IMAGE
        out += ContextAction.COPY_IMAGE_LINK
    } else if (target.isMedia && downloadable(target.srcUri)) {
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
    val star = Regex("""filename\*\s*=\s*(?:UTF-8|utf-8)''([^;]+)""").find(header)?.groupValues?.get(1)?.trim()
    if (!star.isNullOrEmpty()) {
        return runCatching { java.net.URLDecoder.decode(star.replace("+", "%2B"), "UTF-8") }.getOrDefault(star)
    }
    val plain = Regex("""filename\s*=\s*(?:"([^"]*)"|([^;]+))""").find(header) ?: return null
    return (plain.groupValues[1].ifEmpty { plain.groupValues[2] }).trim().ifEmpty { null }
}

private fun sanitize(name: String, fallback: String): String {
    val cleaned = name
        .replace('\\', '_')
        .replace('/', '_')
        .filter { it >= ' ' }
        .trim()
        .trimStart('.')
    if (cleaned.isEmpty()) return fallback
    return if (cleaned.length > 200) cleaned.takeLast(200) else cleaned
}
