package org.krunk.browser.engine

import java.security.cert.X509Certificate

/** Whether mixed (http) content was seen on an https page, as the engine reports it. */
enum class MixedContent {
    NONE,

    /** The engine blocked it. */
    BLOCKED,

    /** It loaded: parts of the page came over a connection that is not secure. */
    LOADED,
}

/**
 * Everything the engine said about the connection of the page a tab shows (GeckoView's
 * `SecurityInformation`), kept in memory with the tab's [PageState] only: never stored, never
 * synced, never logged (the certificate included).
 *
 * GeckoView 160 hands over the server's leaf [certificate] whenever it knows the site's identity,
 * also when the user accepted the certificate by hand ([isException]; the page then is not
 * [secure]). It gives none for a page that loaded mixed passive content (the connection counts
 * as broken) or for a failed load.
 */
data class PageSecurity(
    val secure: Boolean,
    /** The host as the engine displays it (an international name in Unicode); "" when not given. */
    val host: String = "",
    /** The page's origin as the engine reports it; "" when not given. */
    val origin: String = "",
    /** The user accepted this site's certificate ("Continue anyway"). */
    val isException: Boolean = false,
    val mixedActive: MixedContent = MixedContent.NONE,
    val mixedPassive: MixedContent = MixedContent.NONE,
    val certificate: X509Certificate? = null,
) {
    /** Parts of the page were loaded without a secure connection. */
    val mixedLoaded: Boolean get() = mixedActive == MixedContent.LOADED || mixedPassive == MixedContent.LOADED

    // The certificate is left out on purpose: a state dump must never carry it.
    override fun toString(): String =
        "PageSecurity(secure=$secure, exception=$isException, mixed=$mixedActive/$mixedPassive, " +
            "cert=${certificate != null})"
}

/** What the "Connection" part of the shield sheet shows for a page. */
enum class ConnectionKind {
    /** Nothing: an empty tab, the new-tab page, or the engine has not reported yet. */
    NONE,

    /** One of Krunk's own pages. */
    KRUNK_PAGE,

    /** https with a certificate the engine trusts. */
    SECURE,

    /** https with a certificate the user accepted by hand. */
    EXCEPTION,

    /** https, but the engine counts the connection as broken (e.g. mixed content loaded). */
    BROKEN,

    /** Plain http (or another scheme without encryption). */
    NOT_SECURE,
}

fun connectionKind(url: String, internal: Boolean, security: PageSecurity?): ConnectionKind {
    if (internal) return ConnectionKind.KRUNK_PAGE
    val scheme = url.substringBefore(':', "").lowercase()
    return when {
        scheme != "http" && scheme != "https" -> ConnectionKind.NONE
        security == null -> ConnectionKind.NONE
        security.isException -> ConnectionKind.EXCEPTION
        scheme == "http" -> ConnectionKind.NOT_SECURE
        security.secure -> ConnectionKind.SECURE
        else -> ConnectionKind.BROKEN
    }
}
