package org.krunk.browser.engine.permissions

import android.content.Context
import android.content.pm.PackageManager
import androidx.core.content.ContextCompat

/**
 * Android's own permissions (camera, microphone, location, notifications), asked only when a
 * site's request needs one. The system dialog needs an activity: the activity on screen
 * [attach]es its result launcher, and one request is in flight at a time (the next waits).
 * Without an activity a request is answered with what is granted already.
 */
class AndroidPermissionGate(private val context: Context) {
    /** Android's answer: what is granted now, and whether the system refuses to ask any more. */
    class Outcome(val granted: Set<String>, val noDialog: Boolean)

    private class Request(val permissions: List<String>, val done: (Outcome) -> Unit)

    private var owner: Any? = null
    private var launch: ((Array<String>) -> Unit)? = null
    private var rationale: ((String) -> Boolean)? = null
    private val waiting = ArrayDeque<Request>()
    private var inFlight: Request? = null

    fun isGranted(permission: String): Boolean =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

    fun grantedOf(permissions: List<String>): Set<String> = permissions.filter(::isGranted).toSet()

    /** The activity on screen can show the system dialog. */
    fun attach(owner: Any, launch: (Array<String>) -> Unit, shouldShowRationale: (String) -> Boolean) {
        this.owner = owner
        this.launch = launch
        this.rationale = shouldShowRationale
        next()
    }

    /**
     * The activity is gone: whatever waits is answered with the current state. A detach of an
     * activity that a newer one has replaced already changes nothing.
     */
    fun detach(owner: Any) {
        if (this.owner !== owner) return
        this.owner = null
        launch = null
        rationale = null
        val open = listOfNotNull(inFlight) + waiting
        inFlight = null
        waiting.clear()
        open.forEach { it.done(Outcome(grantedOf(it.permissions), noDialog = false)) }
    }

    /** Asks Android for what of [permissions] is missing; [done] is called exactly once. */
    fun request(permissions: List<String>, done: (Outcome) -> Unit) {
        if (AndroidPermissions.satisfied(permissions, grantedOf(permissions)) || launch == null) {
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
        if (AndroidPermissions.satisfied(request.permissions, grantedOf(request.permissions))) {
            request.done(Outcome(grantedOf(request.permissions), noDialog = false))
            next()
            return
        }
        inFlight = request
        start(request.permissions.filterNot(::isGranted).toTypedArray())
    }

    /** The launcher's result (the map is not trusted over the system's own state). */
    fun onResult() {
        val request = inFlight ?: return
        inFlight = null
        val granted = grantedOf(request.permissions)
        // Denied and the system sees no reason to explain: it will not show its dialog again
        // ("Don't allow" twice, or a policy). The message then offers the app's settings page.
        val noDialog = !AndroidPermissions.satisfied(request.permissions, granted) &&
            request.permissions.filterNot { it in granted }.none { rationale?.invoke(it) == true }
        request.done(Outcome(granted, noDialog))
        next()
    }
}
