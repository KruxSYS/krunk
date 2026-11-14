package org.krunk.browser.engine.permissions

/**
 * Android's own permissions (camera, microphone, location, notifications), asked only when a
 * site's request needs one. The system dialog needs an activity: the activity on screen
 * [attach]es its result launcher, and one request is in flight at a time (the next waits).
 * Without an activity a request is answered with what is granted already.
 *
 * Free of Android types ([isGranted] is `ContextCompat.checkSelfPermission`, [now] an uptime
 * clock), so it runs in plain JVM tests.
 *
 * **The activity goes away while Android's dialog is open.** Rotations do not recreate it
 * (`configChanges`), but the system may destroy it anyway ("Don't keep activities", a forced
 * recreate). Then the request stays in flight: Android delivers the dialog's result to the
 * recreated activity's launcher (the result registry keeps it across the recreation), which
 * calls [onResult]. Only an activity that is finishing (the user left Krunk) answers what is
 * open with the current state. Should a recreated activity come back to the front without that
 * result ([resumed]), the request is answered then, so nothing waits forever.
 */
class AndroidPermissionGate(
    private val isGranted: (String) -> Boolean,
    private val sdk: Int,
    private val now: () -> Long,
) {
    /** Android's answer: what is granted now, and whether the system refuses to ask any more. */
    class Outcome(val granted: Set<String>, val noDialog: Boolean)

    private class Request(val permissions: List<String>, val done: (Outcome) -> Unit) {
        var owner: Any? = null
        var launchedAt = 0L
    }

    private var owner: Any? = null
    private var launch: ((Array<String>) -> Unit)? = null
    private var rationale: ((String) -> Boolean)? = null
    private val waiting = ArrayDeque<Request>()
    private var inFlight: Request? = null

    /** Permissions the user refused in Android's dialog in this process. */
    private val refused = HashSet<String>()

    /** Whether an activity is there to show Android's dialog. */
    val hasActivity: Boolean get() = launch != null

    fun grantedOf(permissions: List<String>): Set<String> =
        AndroidPermissions.toCheck(permissions).filter(isGranted).toSet()

    /** Whether [permissions] are granted as far as a site needs them (no dialog). */
    fun satisfied(permissions: List<String>): Boolean =
        AndroidPermissions.satisfied(permissions, grantedOf(permissions), sdk)

    /** The activity on screen can show the system dialog. */
    fun attach(owner: Any, launch: (Array<String>) -> Unit, shouldShowRationale: (String) -> Boolean) {
        this.owner = owner
        this.launch = launch
        this.rationale = shouldShowRationale
        next()
    }

    /**
     * The activity [owner] is destroyed. [finishing]: it is not coming back (else the system
     * recreates it, and the in-flight request's result reaches the new one). A detach of an
     * activity that a newer one has replaced already changes nothing.
     */
    fun detach(owner: Any, finishing: Boolean) {
        if (this.owner !== owner) return
        this.owner = null
        launch = null
        rationale = null
        if (!finishing) return
        val open = listOfNotNull(inFlight) + waiting
        inFlight = null
        waiting.clear()
        open.forEach { it.done(Outcome(grantedOf(it.permissions), noDialog = false)) }
    }

    /**
     * The activity [owner] is in the front (`onResume`, after any result was delivered). A
     * request a destroyed activity launched and whose result never arrived ends here.
     */
    fun resumed(owner: Any) {
        val request = inFlight ?: return
        if (request.owner === owner || this.owner !== owner) return
        inFlight = null
        request.done(Outcome(grantedOf(request.permissions), noDialog = false))
        next()
    }

    /** Asks Android for what of [permissions] is missing; [done] is called exactly once. */
    fun request(permissions: List<String>, done: (Outcome) -> Unit) {
        // Without an activity: answered now, unless one is being recreated (a request in flight
        // waits for it), in which case this one waits too.
        if (satisfied(permissions) || (launch == null && inFlight == null)) {
            done(Outcome(grantedOf(permissions), noDialog = false))
            return
        }
        waiting += Request(permissions, done)
        next()
    }

    private fun next() {
        if (inFlight != null) return
        val start = launch ?: return
        val request = waiting.removeFirstOrNull() ?: return
        // An earlier request may have brought what this one needs.
        if (satisfied(request.permissions)) {
            request.done(Outcome(grantedOf(request.permissions), noDialog = false))
            next()
            return
        }
        inFlight = request
        request.owner = owner
        request.launchedAt = now()
        start(AndroidPermissions.toCheck(request.permissions).filterNot(isGranted).toTypedArray())
    }

    /** The launcher's result (the map is not trusted over the system's own state). */
    fun onResult() {
        val request = inFlight ?: return
        inFlight = null
        val granted = grantedOf(request.permissions)
        val missing = AndroidPermissions.toCheck(request.permissions).filterNot { it in granted }
        val ok = AndroidPermissions.satisfied(request.permissions, granted, sdk)
        // Denied and the system sees no reason to explain. That is the state after "Don't allow"
        // twice or a policy (the system will not show its dialog again: the message offers the
        // app's settings page), but also after the dialog was dismissed without a choice (Back,
        // a tap outside), where it would ask again. So: only when the answer came back too fast
        // for a dialog to have been on screen, or it is the second refusal in this process.
        val noRationale = missing.none { rationale?.invoke(it) == true }
        val quick = now() - request.launchedAt < NO_DIALOG_MS
        val again = missing.isNotEmpty() && missing.all { it in refused }
        val noDialog = !ok && noRationale && (quick || again)
        if (!ok) refused += missing
        request.done(Outcome(granted, noDialog))
        next()
    }

    companion object {
        /** An answer faster than this cannot have come from a dialog the user saw. */
        const val NO_DIALOG_MS = 250L
    }
}
