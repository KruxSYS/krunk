package org.krunk.browser.engine.permissions

import android.annotation.SuppressLint
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.core.content.ContextCompat
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.receiveAsFlow
import org.krunk.browser.KrunkApp
import org.krunk.browser.engine.PermissionPrompt
import org.krunk.ffi.KrunkStore
import org.krunk.ffi.SitePermission
import org.krunk.ffi.siteOfHost
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

/** What clearing a site's data needs of the permissions: its origins' answers reset. */
interface SitePermissionReset {
    /** The origins the private session holds remembered answers for (main thread). */
    fun privateOrigins(): Set<String>

    /** "Reset permissions" of one origin (main thread). */
    fun resetOrigin(origin: String)
}

/**
 * The engine and Android side of site permissions: the delegate every tab's session gets, the
 * prompts, Android's own permissions and the engine's copy of the answers. The rules are
 * [PermissionPolicy]'s (read its text for what is stored where), who is named and keyed is
 * [PermissionSubject]'s. Everything here runs on the main thread, where the engine calls its
 * delegates.
 *
 * A request of a background tab waits: its prompt is queued like every page prompt and shows
 * when the user goes to that tab. A tab that closes or loads another document denies what it
 * still had open, so no request of the engine stays unanswered.
 */
class SitePermissions(context: Context, store: KrunkStore, private val storage: () -> StorageController) :
    SitePermissionReset {
    private val app = context.applicationContext
    private val sdk = Build.VERSION.SDK_INT

    val table = TablePermissionStore.of(store, System::currentTimeMillis, ::webOrigin)
    val policy = PermissionPolicy(table)
    val android = AndroidPermissionGate(
        isGranted = { ContextCompat.checkSelfPermission(app, it) == PackageManager.PERMISSION_GRANTED },
        sdk = sdk,
        now = SystemClock::uptimeMillis,
    )

    private val entries = EngineEntries<ContentPermission>()
    private val open = HashMap<UInt, MutableList<PermissionPrompt>>()

    // A channel: a message raised while another screen is on top waits until the browser screen
    // is on top again and collects (BrowserScreen collects only then).
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

    /** The tab's top-level document changed (`DocumentChange`): open prompts are denied, one-time answers end. */
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
        // denies are answers of this moment and stay tracked until the tab moves on again (and
        // most are forgotten at once anyway: PermissionKind.forgetEngineEntryAtOnce). The
        // engine's writes and these forgets travel the same queue to the engine's thread, so
        // a forget never overtakes the write of an answer given before it.
        entries.take(tab).forEach(::forgetInEngine)
        open.remove(tab)?.toList()?.forEach { it.dismiss() }
    }

    /** The last private tab closed. */
    fun privateSessionEnded() = policy.privateSessionEnded()

    /** Reads the table once, off the main thread, so no request waits for the database. */
    fun warm() = table.warm()

    // ----- the screens -----

    fun list(): List<SitePermission> = table.list()

    /** Allow, Deny or (null) Ask from the site's page of the settings; applies to the next request. */
    fun setFromUi(origin: String, kind: String, allow: Boolean?) {
        if (allow == null) table.remove(origin, kind) else table.set(origin, kind, allow)
        policy.changedByUser()
        val known = PermissionKind.ofKey(kind) ?: return
        if (known == PermissionKind.NOTIFICATIONS) {
            notificationsDropTold = false
            // The user allows a site to notify: the moment to ask Android, not when a
            // notification is due later at some arbitrary moment.
            if (allow == true) askAndroidForNotifications()
        }
        val type = known.geckoType ?: return
        mirror(origin, setOf(type), if (allow == null) EngineAgreement.FORGET else engineValue(allow))
    }

    /** "Reset permissions": the site asks again for everything, in normal and private tabs. */
    override fun resetOrigin(origin: String) {
        table.clearOrigin(origin)
        policy.forgetPrivate(origin)
        policy.changedByUser()
        mirror(origin, PermissionKind.entries.mapNotNull { it.geckoType }.toSet(), EngineAgreement.FORGET)
    }

    override fun privateOrigins(): Set<String> = policy.privateOrigins()

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

    /**
     * Makes the engine's entries of [origin] for [kind] agree with the table right after the
     * engine wrote an answer: one without a row behind it (and every private one) is forgotten.
     * The query travels the engine's queue behind the answer just given, so it sees the
     * engine's write of it (the engine writes when it gets the answer, before it handles the
     * next message). [force]: forgotten even under a row (the engine wrote a deny the user
     * never gave). An entry missed here is still tracked and forgotten when the tab moves on.
     */
    private fun forgetUnbacked(origin: String, private: Boolean, kind: PermissionKind, force: Boolean = false) {
        val type = kind.geckoType ?: return
        storage().getPermissions(origin, private).accept(
            { list ->
                for (p in list.orEmpty()) {
                    if (p.permission != type || p.privateMode != private || webOrigin(p.uri) != origin) continue
                    // Kept only when a row says the same; never turned into an allow or deny here.
                    val backed = !private && !force && when (table.get(origin, kind.key)) {
                        true -> p.value == EngineAgreement.ALLOW
                        false -> p.value == EngineAgreement.DENY
                        null -> false
                    }
                    if (!backed) forgetInEngine(p)
                }
            },
            { e -> Log.w(KrunkApp.TAG, "site permissions: engine entry not read: ${e?.javaClass?.simpleName}") },
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
        val needed = kinds.flatMap { androidPermissionsFor(it, sdk) }
        if (needed.isEmpty()) return then(true)
        android.request(needed) { outcome ->
            val ok = AndroidPermissions.satisfied(needed, outcome.granted, sdk)
            if (!ok) {
                policy.deniedForNow(tab, kinds)
                _messages.trySend(
                    AndroidDenied(AndroidPermissions.missingGroups(needed, outcome.granted, sdk), outcome.noDialog),
                )
            }
            then(ok)
        }
    }

    /** "The site could not notify you" was said once since the user last changed a notification answer. */
    private var notificationsDropTold = false

    /** Whether Android lets Krunk post a site's notification now (never asks). */
    fun notificationsPostable(): Boolean = android.satisfied(androidPermissionsFor(PermissionKind.NOTIFICATIONS, sdk))

    /**
     * A site's notification was dropped because Android's permission is missing. Android's
     * dialog is not shown for this (it would come at whatever moment the site sends something);
     * the user is told once, and only while Krunk is on screen.
     */
    fun notificationDropped() {
        if (notificationsDropTold || !android.hasActivity) return
        notificationsDropTold = true
        _messages.trySend(AndroidDenied(listOf(AndroidPermissions.Group.NOTIFICATIONS), openSettings = true))
    }

    private fun askAndroidForNotifications() {
        val needed = androidPermissionsFor(PermissionKind.NOTIFICATIONS, sdk)
        if (needed.isEmpty()) return
        android.request(needed) { outcome ->
            if (!AndroidPermissions.satisfied(needed, outcome.granted, sdk)) {
                _messages.trySend(AndroidDenied(listOf(AndroidPermissions.Group.NOTIFICATIONS), outcome.noDialog))
            }
        }
    }

    // ----- prompts -----

    private fun label(origin: String) = SiteLabel.of(origin, ::siteOfHost)

    private fun ask(
        tab: Tab,
        subject: PermissionSubject,
        private: Boolean,
        kinds: List<PermissionKind>,
        cameras: List<CameraChoice>,
        onAnswer: (allow: Boolean, remember: Boolean, cameraId: String?) -> Unit,
    ) {
        var answered = false
        var shown: PermissionPrompt? = null
        val canRemember = policy.canRemember(subject.origin)
        val asker = subject.asker
        val prompt = PermissionPrompt(
            tabId = tab.id,
            title = label(asker ?: subject.origin),
            embeddedIn = if (asker != null && asker != subject.origin) label(subject.origin) else null,
            frame = subject.frame?.takeIf { it.isNotEmpty() }?.let(::label),
            frameWithoutAddress = subject.frame?.isEmpty() == true,
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

    private inner class Delegate(private val tab: Tab) : GeckoSession.PermissionDelegate {
        override fun onContentPermissionRequest(session: GeckoSession, perm: ContentPermission): GeckoResult<Int> {
            // A type Krunk does not know, or the tracking exception (never a request): refused,
            // and like every answer without a row forgotten when the tab moves on.
            val kind = PermissionKind.ofGecko(perm.permission)
            val private = tab.private || perm.privateMode
            val subject = kind?.let {
                PermissionSubject.content(it, perm.uri, perm.thirdPartyOrigin, tab.pageUrl(), ::webOrigin)
            }
            if (kind == null || subject == null) {
                // Also a request no site can be named for (the page has no web address).
                entries.add(tab.id, perm)
                return GeckoResult.fromValue(ContentPermission.VALUE_DENY)
            }
            val origin = subject.origin
            val result = GeckoResult<Int>()
            val kinds = listOf(kind)

            fun finish(allow: Boolean) {
                // The engine writes the answer into its own store. Without a row of the table
                // behind it, it must not outlive the page: it is tracked and forgotten when the
                // tab moves on, for most kinds at once (never before: the engine writes after
                // the answer reached it). Storage access keeps the engine's own dated entry.
                val row = if (allow) policy.isRow(origin, private, kind) else policy.isDenyRow(origin, private, kind)
                result.complete(if (allow) ContentPermission.VALUE_ALLOW else ContentPermission.VALUE_DENY)
                if (row || kind == PermissionKind.STORAGE_ACCESS) return
                entries.add(tab.id, perm)
                // A location allow is written only after the engine's own Android step: forgotten
                // from onAndroidPermissionsRequest (and here, when the engine skips that step).
                if (kind.forgetEngineEntryAtOnce(allow)) forgetUnbacked(origin, private, kind)
            }

            when (val decision = policy.decide(tab.id, origin, private, kinds)) {
                PermissionPolicy.Decision.Allow -> withAndroid(tab.id, kinds, ::finish)

                is PermissionPolicy.Decision.Deny -> finish(false)

                is PermissionPolicy.Decision.Ask -> {
                    ask(tab, subject, private, decision.kinds, emptyList()) { allow, remember, _ ->
                        if (!allow) {
                            policy.answered(tab.id, origin, private, kinds, allow = false, remember = remember)
                            finish(false)
                        } else {
                            // Android first: an allow is only kept when the site really gets it.
                            // Location: what passes here also passes the engine's own Android
                            // step that follows (same rule, AndroidPermissions.locationUsable), so
                            // the row and the engine's entry agree.
                            withAndroid(tab.id, kinds) { granted ->
                                if (granted) policy.answered(tab.id, origin, private, kinds, true, remember)
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
            // `uri` is the document that called getUserMedia (the page or a frame). The answer is
            // the page's, as for every other kind (PermissionSubject); without a page that has
            // a web address there is no site to ask for.
            val subject = PermissionSubject.media(uri, tab.pageUrl(), ::webOrigin) ?: return reject()
            val origin = subject.origin

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
                    ask(tab, subject, tab.private, decision.kinds, choices) { allow, remember, cameraId ->
                        if (!allow) {
                            policy.answered(tab.id, origin, tab.private, decision.kinds, false, remember)
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
            val pageOrigin = webOrigin(tab.pageUrl())
            if (asked.all { AndroidPermissions.groupOf(it) == AndroidPermissions.Group.LOCATION }) {
                return engineLocationStep(asked, pageOrigin, callback)
            }
            // The engine wants camera and microphone access before it lists the devices, that
            // is before the site's own prompt. A site that is denied anyway (for the page's
            // origin, which the media answer is kept for) does not get Android's dialog on screen.
            val kinds = asked.mapNotNull {
                when (AndroidPermissions.groupOf(it)) {
                    AndroidPermissions.Group.CAMERA -> PermissionKind.CAMERA
                    AndroidPermissions.Group.MICROPHONE -> PermissionKind.MICROPHONE
                    else -> null
                }
            }.distinct()
            if (kinds.isNotEmpty() &&
                (
                    pageOrigin == null ||
                        policy.decide(tab.id, pageOrigin, tab.private, kinds) is PermissionPolicy.Decision.Deny
                    )
            ) {
                return callback.reject()
            }
            android.request(asked) { outcome ->
                if (AndroidPermissions.satisfied(asked, outcome.granted, sdk)) {
                    callback.grant()
                } else {
                    policy.deniedForNow(tab.id, kinds)
                    _messages.trySend(
                        AndroidDenied(AndroidPermissions.missingGroups(asked, outcome.granted, sdk), outcome.noDialog),
                    )
                    callback.reject()
                }
            }
        }

        /**
         * The engine's own Android step after Krunk allowed a location request: it asks for
         * `ACCESS_FINE_LOCATION` only. Krunk asked Android already (both location permissions,
         * so the user could choose "approximate"), so this is answered from what Android granted
         * and never brings a second dialog: granted when the engine can work with it
         * ([AndroidPermissions.locationUsable]: precise, or approximate from Android 12 on, where
         * the site then gets a coarse position). Else refused with the message, and the deny the
         * engine then writes for the page is removed again, so no permanent engine deny ever
         * sits under an Allow row.
         */
        private fun engineLocationStep(
            asked: List<String>,
            pageOrigin: String?,
            callback: GeckoSession.PermissionDelegate.Callback,
        ) {
            if (android.satisfied(asked)) {
                callback.grant()
                // A one-time allow is written only now: forgotten at once (PermissionKind).
                pageOrigin?.let { forgetUnbacked(it, tab.private, PermissionKind.GEOLOCATION) }
                return
            }
            policy.deniedForNow(tab.id, listOf(PermissionKind.GEOLOCATION))
            _messages.trySend(AndroidDenied(listOf(AndroidPermissions.Group.LOCATION), openSettings = true))
            callback.reject()
            pageOrigin?.let { forgetUnbacked(it, tab.private, PermissionKind.GEOLOCATION, force = true) }
        }
    }
}
