package org.krunk.browser.engine.permissions

/**
 * Which site a request is decided and kept for, and what its prompt names. Pure: the web-origin
 * rule is the core's (`webOrigin`), handed in so this runs in plain JVM tests.
 *
 * The engine keeps every content answer for the **top-level** page (`GeckoViewPermission.sys.mjs`
 * hands `uri` = the top-level principal, and writes its own entry for that principal); a frame
 * may only ask because the page delegated the feature to it. So the prompt names the page's site
 * as the one that gets the permission, the stored row and the settings list use that same origin,
 * and a frame of another site that asked is named on a line of its own. Camera and microphone
 * (whose answers only Krunk keeps) follow the same rule, so the Deny shortcut before Android's
 * dialog, the prompt and the row agree. Storage access is the exception: there the embedded site
 * asks for its own cookies inside the page, and both are named.
 *
 * A request no site can be named for (the top-level page has no web address: `file:`,
 * `about:`, ...) is refused without a prompt.
 */
data class PermissionSubject(
    /** The origin the answer is decided and kept for (the top-level page's). */
    val origin: String,
    /** Storage access only: the embedded site that asks (the prompt's title). */
    val asker: String? = null,
    /**
     * A frame asked: its origin, or "" for a frame without a web address of its own
     * (`about:blank`, `srcdoc`, `blob:`, a sandboxed frame). Null when the page itself asked.
     */
    val frame: String? = null,
) {
    companion object {
        /**
         * A content request: [uri] and [thirdPartyOrigin] as the engine gives them (the latter is
         * the requesting document's principal origin, with `^attributes` possibly appended).
         * For storage access [uri] is the embedded site and [pageUrl] the tab's page.
         */
        fun content(
            kind: PermissionKind,
            uri: String?,
            thirdPartyOrigin: String?,
            pageUrl: String,
            webOrigin: (String) -> String?,
        ): PermissionSubject? {
            if (kind == PermissionKind.STORAGE_ACCESS) {
                val page = webOrigin(pageUrl) ?: return null
                val asker = uri?.let(webOrigin) ?: return null
                return PermissionSubject(page, asker = asker)
            }
            val top = uri?.let(webOrigin) ?: return null
            val frame = thirdPartyOrigin?.takeIf { it.isNotEmpty() }?.let { webOrigin(it.substringBefore('^')) ?: "" }
            return PermissionSubject(top, frame = frame?.takeIf { it != top })
        }

        /**
         * A camera/microphone request: [documentUri] is the document that called
         * `getUserMedia` (the page or a frame), [pageUrl] the tab's committed page.
         */
        fun media(documentUri: String?, pageUrl: String, webOrigin: (String) -> String?): PermissionSubject? {
            val top = webOrigin(pageUrl) ?: return null
            val frame = documentUri?.takeIf { it.isNotEmpty() }?.let { webOrigin(it) ?: "" }
            return PermissionSubject(top, frame = frame?.takeIf { it != top })
        }
    }
}

/**
 * How a site is shown in a prompt or a notification: the registrable domain is the part that
 * counts and is never cut; the labels in front of it are cut from the start when the host is
 * long (`…accounts.google.com.x.` + `evil.example`). Plain `http://` is kept, so an unencrypted
 * origin never reads like the encrypted one; `https://` is left out.
 */
data class SiteLabel(val scheme: String, val sub: String, val registrable: String, val port: String) {
    val text: String get() = scheme + sub + registrable + port

    companion object {
        /** Characters of the host shown at most. */
        const val MAX_HOST = 60
        private const val ELLIPSIS = "…"

        /** [origin]: `scheme://host[:port]`; [siteOfHost]: the core's registrable-domain rule. */
        fun of(origin: String, siteOfHost: (String) -> String): SiteLabel {
            val scheme = origin.substringBefore("://", "")
            val rest = origin.substringAfter("://")
            val host: String
            val port: String
            if (rest.startsWith("[")) {
                host = rest.substringBefore(']') + "]"
                port = rest.substringAfter(']', "")
            } else {
                host = rest.substringBefore(':')
                port = rest.substringAfter(':', "").let { if (it.isEmpty()) "" else ":$it" }
            }
            val site = siteOfHost(host).ifEmpty { host }
            var sub = if (host != site && host.endsWith(".$site")) host.removeSuffix(site) else ""
            var registrable = if (sub.isEmpty()) host else site
            if (registrable.length > MAX_HOST) {
                registrable = ELLIPSIS + registrable.takeLast(MAX_HOST - 1)
                sub = ""
            } else if (sub.length + registrable.length > MAX_HOST) {
                sub = ELLIPSIS + sub.takeLast(MAX_HOST - registrable.length - 1)
            }
            return SiteLabel(if (scheme.equals("http", ignoreCase = true)) "http://" else "", sub, registrable, port)
        }
    }
}
