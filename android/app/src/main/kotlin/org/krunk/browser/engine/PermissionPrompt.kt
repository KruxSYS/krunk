package org.krunk.browser.engine

import org.krunk.browser.engine.permissions.CameraChoice
import org.krunk.browser.engine.permissions.PermissionKind

/**
 * A site asks for a permission (camera, location, notifications, ...). It waits in the same
 * queue as the other page prompts, so it shows only while its tab is on screen and never on top
 * of another prompt. Exactly one of [answer] and [dismiss] takes effect; dismissing (Back, a tap
 * outside, the tab closing or loading another page) is a deny that is not remembered.
 */
class PermissionPrompt(
    override val tabId: UInt,
    /** The site that asks (host and port; "" = no web origin): the page, or a frame inside it. */
    override val site: String,
    /**
     * When the asking site is not the page's own (a frame of another site, storage access): the
     * site of the page it is embedded in, so the prompt names both. Else "".
     */
    val embeddedIn: String,
    val kinds: List<PermissionKind>,
    /** More than one camera: the user picks which (the first is the engine's suggestion). */
    val cameras: List<CameraChoice>,
    /** Whether "Remember" is offered (not for a page without a web origin). */
    val canRemember: Boolean,
    /** A private tab: remembered only until the private tabs are closed. */
    val private: Boolean,
    private val onAnswer: (allow: Boolean, remember: Boolean, cameraId: String?) -> Unit,
) : PagePrompt() {
    fun answer(allow: Boolean, remember: Boolean, cameraId: String? = null) = onAnswer(allow, remember, cameraId)

    override fun dismiss() = onAnswer(false, false, null)
}
