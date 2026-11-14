package org.krunk.browser.engine

import org.krunk.browser.engine.permissions.CameraChoice
import org.krunk.browser.engine.permissions.PermissionKind
import org.krunk.browser.engine.permissions.SiteLabel

/**
 * A site asks for a permission (camera, location, notifications, ...). It waits in the same
 * queue as the other page prompts, so it shows only while its tab is on screen and never on top
 * of another prompt. Exactly one of [answer] and [dismiss] takes effect; dismissing (Back, a tap
 * outside, the tab closing or loading another document) is a deny that is not remembered.
 *
 * Who is named (see `PermissionSubject`): the title is the site that gets the answer, which is
 * the one the settings list shows; for storage access it is the embedded site that asks.
 */
class PermissionPrompt(
    override val tabId: UInt,
    /** The title's site: the page's, or for storage access the embedded site that asks. */
    val title: SiteLabel,
    /** Storage access: the page's site the embedded one asks inside of. Else null. */
    val embeddedIn: SiteLabel?,
    /**
     * Another site's frame inside the page asked (the answer is still the page's): its site;
     * [frameWithoutAddress] when the frame has no web address. Else null.
     */
    val frame: SiteLabel?,
    val frameWithoutAddress: Boolean,
    val kinds: List<PermissionKind>,
    /** More than one camera: the user picks which (the first is the engine's suggestion). */
    val cameras: List<CameraChoice>,
    /** Whether "Remember" is offered. */
    val canRemember: Boolean,
    /** A private tab: remembered only until the private tabs are closed. */
    val private: Boolean,
    private val onAnswer: (allow: Boolean, remember: Boolean, cameraId: String?) -> Unit,
) : PagePrompt() {
    override val site: String get() = title.text

    fun answer(allow: Boolean, remember: Boolean, cameraId: String? = null) = onAnswer(allow, remember, cameraId)

    override fun dismiss() = onAnswer(false, false, null)
}
