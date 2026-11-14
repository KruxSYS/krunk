package org.krunk.browser.engine.permissions

import android.annotation.SuppressLint
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.PermissionPrompt
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SitePermission
import org.krunk.ffi.webOrigin
import org.mozilla.geckoview.GeckoResult
import org.mozilla.geckoview.GeckoSession
import org.mozilla.geckoview.GeckoSession.PermissionDelegate.ContentPermission
import org.mozilla.geckoview.GeckoSession.PermissionDelegate.MediaSource
import org.mozilla.geckoview.StorageController

/** Android refused one of its own permissions: what the snackbar says. */
data class AndroidDenied(
    val groups: List<AndroidPermissions.Group>,
    /** The system no longer shows its dialog: the message offers the app's settings page. */
    val openSettings: Boolean,
)

/**
 * The engine and Android side of site permissions: the delegate every tab's session gets, the
 * prompts, Android's own permissions and the engine's copy of the answers. The rules are
 * [PermissionPolicy]'s (read its text for what is stored where). Everything here runs on the
 * main thread, where the engine calls its delegates.
 *
 * A request of a background tab waits: its prompt is queued like every page prompt and shows
 * when the user goes to that tab. A tab that closes or loads another document denies what it
 * still had open, so no request of the engine stays unanswered.
 */
class SitePermissions(context: Context, store: KrunkStore, private val storage: () -> StorageController) {
    val table = TablePermissionStore(store, System::currentTimeMillis)
    val policy = PermissionPolicy(table)
    val android = AndroidPermissionGate(context.applicationContext)

    private val entries = EngineEntries<ContentPermission>()
    private val open = HashMap<UInt, MutableList<PermissionPrompt>>()

    // A channel: a message raised while another screen is on top waits for the browser screen.
    private val _messages = Channel<AndroidDenied>(capacity = 4, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    val messages: Flow<AndroidDenied> = _messages.receiveAsFlow()

    /** What a tab's session gives the delegate. */
    class Tab(
        val id: UInt,
        val private: Boolean,
        /** The address the engine last committed for the tab. */
        val pageUrl: () -> String,
        val show: (PermissionPrompt) -> Unit,
        val closed: (PermissionPrompt) -> Unit,
    )

    fun delegate(tab: Tab): GeckoSession.PermissionDelegate = Delegate(tab)

    // ----- the tab's life -----

    private val main = Handler(Looper.getMainLooper())

    /** The tab loads another document: open prompts are denied, one-time answers end. */
    fun newDocument(tab: UInt) {
        leavePage(tab)
        policy.newDocument(tab)
    }

    /** The tab's session is closed (the tab closed, was unloaded or crashed). */
    fun tabClosed(tab: UInt) {
        leavePage(tab)
        policy.tabClosed(tab)
        // What the prompts dismissed just now made the engine write is forgotten too, but only
        // once those answers are on their way (they are handed over through this same queue).
        main.post { entries.take(tab).forEach(::forgetInEngine) }
    }

    private fun leavePage(tab: UInt) {
        // First what the page before left in the engine's store, then its open prompts: their
        // denies are answers of this moment and stay tracked until the tab moves on again.
        entries.take(tab).forEach(::forgetInEngine)
        open.remove(tab)?.toList()?.forEach { it.dismiss() }
    }

    /** The last private tab closed. */
    fun privateSessionEnded() = policy.privateSessionEnded()

    // ----- the screens -----

    fun list(): List<SitePermission> = table.list()

    /** Allow, Deny or (null) Ask from the site's page of the settings; applies to the next request. */
    fun setFromUi(origin: String, kind: String, allow: Boolean?) {
        if (allow == null) table.remove(origin, kind) else table.set(origin, kind, allow)
        policy.changedByUser()
        val known = PermissionKind.ofKey(kind) ?: return
        val type = known.geckoType ?: return
        mirror(origin, setOf(type), if (allow == null) EngineAgreement.FORGET else engineValue(allow))
    }

    /** "Reset permissions": the site asks again for everything. */
    fun resetOrigin(origin: String) {
        table.clearOrigin(origin)
        policy.changedByUser()
        mirror(origin, PermissionKind.entries.mapNotNull { it.geckoType }.toSet(), EngineAgreement.FORGET)
    }

    /** "Clear browsing data": the table is emptied (the engine clears its own store itself). */
    fun clearAll() {
        table.clear()
        entries.takeAll()
    }

    /**
     * At start: the engine's entries are compared with the table and corrected. What a process
     * that died left behind (a one-time allow it could not forget any more) ends here.
     */
    fun reconcile() {
        storage().allPermissions.accept(
            { list ->
                var fixed = 0
                for (p in list.orEmpty()) {
                    val value = EngineAgreement.correction(
                        p.permission,
                        webOrigin(p.uri),
                        p.privateMode,
                        p.value,
                        table,
                    )
                    if (value != null) {
                        setInEngine(p, value)
                        fixed++
                    }
                }
                if (fixed > 0) Log.i(KrunkApp.TAG, "site permissions: corrected $fixed engine entries")
            },
            { e -> Log.w(KrunkApp.TAG, "site permissions: engine entries not read: ${e?.javaClass?.simpleName}") },
        )
    }

    /** Sets the engine's entries of [origin] for [types] to [value] (those that exist). */
    private fun mirror(origin: String, types: Set<Int>, value: Int) {
        storage().getPermissions(origin, false).accept(
            { list ->
                for (p in list.orEmpty()) {
                    if (p.permission in types && !p.privateMode && webOrigin(p.uri) == origin && p.value != value) {
                        setInEngine(p, value)
                    }
                }
            },
            { e -> Log.w(KrunkApp.TAG, "site permissions: engine entry not changed: ${e?.javaClass?.simpleName}") },
        )
    }

    private fun forgetInEngine(perm: ContentPermission) = setInEngine(perm, EngineAgreement.FORGET)

    /**
     * Writes [value] into the engine's entry. Forgetting is two steps: first the documented
     * `VALUE_PROMPT` (the engine asks again from here on), then value 0, which the engine's
     * permission manager takes as "remove the entry" (`GeckoViewStorageController.sys.mjs` hands
     * the number to `Services.perms.addFromPrincipal` as it is), so no row naming the site stays
     * in the engine's database. 0 is not one of the API's named values, hence the lint note.
     */
    @SuppressLint("WrongConstant")
    private fun setInEngine(perm: ContentPermission, value: Int) {
        val controller = storage()
        if (value == EngineAgreement.FORGET) {
            controller.setPermission(perm, ContentPermission.VALUE_PROMPT)
            controller.setPermission(perm, EngineAgreement.FORGET)
        } else {
            controller.setPermission(perm, value)
        }
    }

    private fun engineValue(allow: Boolean) = if (allow) EngineAgreement.ALLOW else EngineAgreement.DENY

    // ----- Android's permissions -----

    /**
     * Makes sure Android grants what [kinds] need, asking if it must; a refusal is told to the
     * user (which permission is missing) and counts as a deny for this page load.
     */
    private fun withAndroid(tab: UInt, kinds: List<PermissionKind>, then: (granted: Boolean) -> Unit) {
        val needed = kinds.flatMap { androidPermissionsFor(it, Build.VERSION.SDK_INT) }
        if (needed.isEmpty()) return then(true)
        android.request(needed) { outcome ->
            val ok = AndroidPermissions.satisfied(needed, outcome.granted)
            if (!ok) {
                policy.deniedForNow(tab, kinds)
                _messages.trySend(
                    AndroidDenied(AndroidPermissions.missingGroups(needed, outcome.granted), outcome.noDialog),
                )
            }
            then(ok)
        }
    }

    /** For a site notification that is due while Android's permission is missing. */
    fun ensureNotificationsAllowed(then: (granted: Boolean) -> Unit) {
        val needed = androidPermissionsFor(PermissionKind.NOTIFICATIONS, Build.VERSION.SDK_INT)
        if (needed.isEmpty()) return then(true)
        android.request(needed) { outcome ->
            val ok = AndroidPermissions.satisfied(needed, outcome.granted)
            if (!ok) _messages.trySend(AndroidDenied(listOf(AndroidPermissions.Group.NOTIFICATIONS), outcome.noDialog))
            then(ok)
        }
    }

    // ----- prompts -----

    private fun ask(
        tab: Tab,
        asker: String?,
        page: String?,
        private: Boolean,
        kinds: List<PermissionKind>,
        cameras: List<CameraChoice>,
        canRemember: Boolean,
        onAnswer: (allow: Boolean, remember: Boolean, cameraId: String?) -> Unit,
    ) {
        var answered = false
        var shown: PermissionPrompt? = null
        val prompt = PermissionPrompt(
            tabId = tab.id,
            site = siteOf(asker),
            embeddedIn = if (page != null && page != asker) siteOf(page) else "",
            kinds = kinds,
            cameras = cameras,
            canRemember = canRemember,
            private = private,
        ) { allow, remember, cameraId ->
            if (!answered) {
                answered = true
                shown?.let { p ->
                    open[tab.id]?.remove(p)
                    tab.closed(p)
                }
                onAnswer(allow, remember && canRemember, cameraId)
            }
        }
        shown = prompt
        open.getOrPut(tab.id) { ArrayList() } += prompt
        tab.show(prompt)
    }

    private fun siteOf(origin: String?): String = origin?.substringAfter("://").orEmpty()

    private inner class Delegate(private val tab: Tab) : GeckoSession.PermissionDelegate {
        override fun onContentPermissionRequest(session: GeckoSession, perm: ContentPermission): GeckoResult<Int> {
            // A type Krunk does not know, or the tracking exception (never a request): refused,
            // and like every answer without a row forgotten when the tab moves on.
            val kind = PermissionKind.ofGecko(perm.permission)
            if (kind == null) {
                entries.add(tab.id, perm)
                return GeckoResult.fromValue(ContentPermission.VALUE_DENY)
            }
            val private = tab.private || perm.privateMode
            val result = GeckoResult<Int>()
            // The engine names the top-level site in `uri` and the frame that asks in
            // `thirdPartyOrigin`; for storage access `uri` is the embedded site that asks.
            val pageOrigin: String?
            val asker: String?
            if (kind == PermissionKind.STORAGE_ACCESS) {
                pageOrigin = webOrigin(tab.pageUrl())
                asker = webOrigin(perm.uri)
            } else {
                pageOrigin = webOrigin(perm.uri)
                asker = perm.thirdPartyOrigin?.let { webOrigin(it.substringBefore('^')) } ?: pageOrigin
            }
            val kinds = listOf(kind)

            fun finish(allow: Boolean) {
                // The engine writes the answer into its own store. Without a row of the table
                // behind it, it must not outlive the page: it is forgotten when the tab moves
                // on (never right away: the engine writes after this call has returned).
                // Storage access is the exception, the engine keeps its own dated entry.
                val row = if (allow) {
                    policy.isRow(
                        pageOrigin,
                        private,
                        kind,
                    )
                } else {
                    policy.isDenyRow(pageOrigin, private, kind)
                }
                if (!row && kind != PermissionKind.STORAGE_ACCESS) entries.add(tab.id, perm)
                result.complete(if (allow) ContentPermission.VALUE_ALLOW else ContentPermission.VALUE_DENY)
            }

            when (val decision = policy.decide(tab.id, pageOrigin, private, kinds)) {
                PermissionPolicy.Decision.Allow -> withAndroid(tab.id, kinds, ::finish)

                is PermissionPolicy.Decision.Deny -> finish(false)

                is PermissionPolicy.Decision.Ask -> {
                    ask(tab, asker, pageOrigin, private, decision.kinds, emptyList(), policy.canRemember(pageOrigin)) {
                            allow,
                            remember,
                            _,
                        ->
                        if (!allow) {
                            policy.answered(tab.id, pageOrigin, private, kinds, allow = false, remember = remember)
                            finish(false)
                        } else {
                            // Android first: an allow is only kept when the site really got it.
                            withAndroid(tab.id, kinds) { granted ->
                                if (granted) policy.answered(tab.id, pageOrigin, private, kinds, true, remember)
                                finish(granted)
                            }
                        }
                    }
                }
            }
            return result
        }

        override fun onMediaPermissionRequest(
            session: GeckoSession,
            uri: String,
            video: Array<out MediaSource>?,
            audio: Array<out MediaSource>?,
            callback: GeckoSession.PermissionDelegate.MediaCallback,
        ) {
            var settled = false

            fun reject() {
                if (!settled) {
                    settled = true
                    callback.reject()
                }
            }
            // Only a camera and a microphone are offered: sharing the screen or the device's
            // own sound is not something Krunk lets a page have.
            val cameras = video?.filter { it.source == MediaSource.SOURCE_CAMERA }.orEmpty()
            val microphones = audio?.filter { it.source == MediaSource.SOURCE_MICROPHONE }.orEmpty()
            if ((video != null && cameras.isEmpty()) || (audio != null && microphones.isEmpty())) return reject()
            val kinds = buildList {
                if (video != null) add(PermissionKind.CAMERA)
                if (audio != null) add(PermissionKind.MICROPHONE)
            }
            if (kinds.isEmpty()) return reject()
            // `uri` is the document that called getUserMedia (the page or a frame): the answer
            // belongs to that site.
            val origin = webOrigin(uri)

            fun grant(cameraId: String?) {
                if (settled) return
                settled = true
                val camera = cameras.firstOrNull { it.id == cameraId } ?: cameras.firstOrNull()
                callback.grant(camera, microphones.firstOrNull())
            }

            when (val decision = policy.decide(tab.id, origin, tab.private, kinds)) {
                PermissionPolicy.Decision.Allow ->
                    withAndroid(tab.id, kinds) { granted -> if (granted) grant(null) else reject() }

                is PermissionPolicy.Decision.Deny -> reject()

                is PermissionPolicy.Decision.Ask -> {
                    val choices = if (cameras.size > 1) cameras.map { CameraChoice.of(it.id, it.name) } else emptyList()
                    ask(
                        tab,
                        origin,
                        webOrigin(tab.pageUrl()),
                        tab.private,
                        decision.kinds,
                        choices,
                        policy.canRemember(origin),
                    ) { allow, remember, cameraId ->
                        if (!allow) {
                            policy.answered(
                                tab.id,
                                origin,
                                tab.private,
                                decision.kinds,
                                allow = false,
                                remember = remember,
                            )
                            reject()
                        } else {
                            withAndroid(tab.id, kinds) { granted ->
                                if (granted) {
                                    policy.answered(tab.id, origin, tab.private, decision.kinds, true, remember)
                                    grant(cameraId)
                                } else {
                                    reject()
                                }
                            }
                        }
                    }
                }
            }
        }

        override fun onAndroidPermissionsRequest(
            session: GeckoSession,
            permissions: Array<out String>?,
            callback: GeckoSession.PermissionDelegate.Callback,
        ) {
            // Krunk targets an Android version that does not guard the local network.
            val asked = permissions.orEmpty().filter { it != AndroidPermissions.LOCAL_NETWORK }
            if (asked.isEmpty()) return callback.grant()
            if (asked.any { AndroidPermissions.groupOf(it) == AndroidPermissions.Group.OTHER }) {
                Log.w(KrunkApp.TAG, "site permissions: the engine asked for an Android permission Krunk does not hold")
                return callback.reject()
            }
            // The engine wants camera and microphone access before it lists the devices, that
            // is before the site's own prompt. A site that is denied anyway does not get
            // Android's dialog on screen.
            val kinds = asked.mapNotNull {
                when (AndroidPermissions.groupOf(it)) {
                    AndroidPermissions.Group.CAMERA -> PermissionKind.CAMERA
                    AndroidPermissions.Group.MICROPHONE -> PermissionKind.MICROPHONE
                    else -> null
                }
            }.distinct()
            val pageOrigin = webOrigin(tab.pageUrl())
            if (kinds.isNotEmpty() &&
                policy.decide(tab.id, pageOrigin, tab.private, kinds) is PermissionPolicy.Decision.Deny
            ) {
                return callback.reject()
            }
            android.request(asked) { outcome ->
                if (AndroidPermissions.satisfied(asked, outcome.granted)) {
                    callback.grant()
                } else {
                    policy.deniedForNow(tab.id, kinds)
                    _messages.trySend(
                        AndroidDenied(AndroidPermissions.missingGroups(asked, outcome.granted), outcome.noDialog),
                    )
                    callback.reject()
                }
            }
        }
    }
}
