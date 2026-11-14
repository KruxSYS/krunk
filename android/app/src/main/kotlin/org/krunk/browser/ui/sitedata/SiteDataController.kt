package org.krunk.browser.ui.sitedata

import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.krunk.browser.KrunkApp
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.EngineSiteData
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.await
import org.krunk.browser.tabs.TabManager
import org.krunk.ffi.FfiException
import org.krunk.ffi.siteOfHost
import org.krunk.ffi.webOrigin

/** The site data screen's work: asking the engine and the store, and clearing one site. */
class SiteDataController(private val core: KrunkCore, private val engine: GeckoEngine, private val tabs: TabManager) {
    /** What the engine lists, or null when it has not answered within [timeoutMs]. */
    suspend fun engineData(timeoutMs: Long): EngineSiteData? = withTimeoutOrNull(timeoutMs) { engine.siteData.list() }

    /** The list for [data] (null: from Krunk's own records only). Private tabs add nothing. */
    suspend fun load(data: EngineSiteData?): SiteList {
        val open = tabs.tabs.value
            .filter { !it.private && it.internalPage == null }
            .mapNotNull { webOrigin(it.page.url.ifEmpty { it.url }) }
        return try {
            core.io { store ->
                val summaries =
                    store.siteSummaries(SiteDataModel.cookieDomains(data), SiteDataModel.extraOrigins(data, open))
                val permissions = store.sitePermissionsList().map { it.origin }
                SiteDataModel.build(summaries, data, permissions, ::siteOfHost)
            }
        } catch (e: FfiException) {
            Log.w(KrunkApp.TAG, "site data: the list could not be read", e)
            SiteList(emptyList(), SiteSource.HISTORY)
        }
    }

    /**
     * Clears what [site] holds in the engine and, with [withPermissions], the answers Krunk
     * stored for its origins; then reloads its open tabs, so no page keeps running with what
     * was just removed. Once started it is finished even when the screen is left. False when
     * the engine did not confirm.
     */
    suspend fun clear(site: String, withPermissions: Boolean): Boolean = withContext(NonCancellable) {
        try {
            val cleared = withTimeoutOrNull(CLEAR_TIMEOUT_MS) {
                engine.clearSiteData(site, withPermissions).await()
                true
            } ?: false
            if (!cleared) {
                Log.w(KrunkApp.TAG, "site data: the engine did not confirm clearing a site")
                return@withContext false
            }
            if (withPermissions) {
                core.io { store ->
                    store.sitePermissionsList().map { it.origin }.distinct()
                        .filter { SiteDataModel.isOfSite(it, site, ::siteOfHost) }
                        .forEach { store.sitePermissionsClearOrigin(it) }
                }
            }
            tabs.tabs.value
                .filter { it.live && SiteDataModel.isOfSite(it.page.url.ifEmpty { it.url }, site, ::siteOfHost) }
                .forEach { tabs.reload(it.id) }
            true
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Log.w(KrunkApp.TAG, "site data: clearing a site failed", e)
            false
        }
    }

    companion object {
        /** How long the first list waits for the engine before it shows Krunk's own records. */
        const val FIRST_ANSWER_MS = 1_500L

        /** How long the engine's answer is still taken after that (the extension is starting). */
        const val LATE_ANSWER_MS = 15_000L
        private const val CLEAR_TIMEOUT_MS = 20_000L
    }
}
