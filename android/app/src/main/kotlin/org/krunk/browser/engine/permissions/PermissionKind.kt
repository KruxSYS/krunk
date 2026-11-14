package org.krunk.browser.engine.permissions

/** What happens when a site asks for something and no answer is stored for it. */
enum class PermissionDefault { ASK, ALLOW, DENY }

/**
 * Everything a site can ask for through the engine's permission delegate, under the key Krunk
 * stores its answer with (`camera`, `microphone`, `geolocation` and `notifications` are the keys
 * Windows uses in the same table).
 *
 * [geckoType] is the engine's `PermissionDelegate.PERMISSION_*` number (null for camera and
 * microphone, which come through the media request). The engine's eleventh type, `TRACKING` (7),
 * is not here on purpose: it is a content-blocking exception, never a request, and the shield
 * owns exceptions.
 */
enum class PermissionKind(
    val key: String,
    val default: PermissionDefault,
    val geckoType: Int?,
    /**
     * Whether "Allow" can be remembered for the site. Not for storage access: the stored answer
     * is per top-level site, and an allow would cover every other site embedded there, not only
     * the one the prompt named.
     */
    val rememberAllow: Boolean = true,
) {
    CAMERA("camera", PermissionDefault.ASK, null),
    MICROPHONE("microphone", PermissionDefault.ASK, null),
    GEOLOCATION("geolocation", PermissionDefault.ASK, 0),
    NOTIFICATIONS("notifications", PermissionDefault.ASK, 1),
    PERSISTENT_STORAGE("persistentStorage", PermissionDefault.ASK, 2),

    /** No XR runtime ships with Krunk. */
    XR("xr", PermissionDefault.DENY, 3),
    AUTOPLAY_INAUDIBLE("autoplayInaudible", PermissionDefault.ALLOW, 4),
    AUTOPLAY_AUDIBLE("autoplayAudible", PermissionDefault.DENY, 5),
    DRM("drm", PermissionDefault.ASK, 6),
    STORAGE_ACCESS("storageAccess", PermissionDefault.ASK, 8, rememberAllow = false),
    LOCAL_DEVICE("localDevice", PermissionDefault.ASK, 9),
    LOCAL_NETWORK("localNetwork", PermissionDefault.ASK, 10),
    ;

    companion object {
        /** The engine's number of the tracking-protection exception: known, never asked, never stored. */
        const val GECKO_TRACKING = 7

        fun ofKey(key: String): PermissionKind? = entries.firstOrNull { it.key == key }

        fun ofGecko(type: Int): PermissionKind? = entries.firstOrNull { it.geckoType == type }
    }
}

/**
 * The Android permissions a kind needs before the engine can serve it. `POST_NOTIFICATIONS`
 * exists from Android 13 (API 33) on; before that notifications need none.
 */
fun androidPermissionsFor(kind: PermissionKind, sdk: Int): List<String> = when (kind) {
    PermissionKind.CAMERA -> listOf(AndroidPermissions.CAMERA)

    PermissionKind.MICROPHONE -> listOf(AndroidPermissions.RECORD_AUDIO)

    PermissionKind.GEOLOCATION -> listOf(AndroidPermissions.FINE_LOCATION, AndroidPermissions.COARSE_LOCATION)

    PermissionKind.NOTIFICATIONS ->
        if (sdk >=
            AndroidPermissions.SDK_POST_NOTIFICATIONS
        ) {
            listOf(AndroidPermissions.POST_NOTIFICATIONS)
        } else {
            emptyList()
        }

    else -> emptyList()
}

/** The names of `android.Manifest.permission`, as text so the policy runs in plain JVM tests. */
object AndroidPermissions {
    const val CAMERA = "android.permission.CAMERA"
    const val RECORD_AUDIO = "android.permission.RECORD_AUDIO"
    const val FINE_LOCATION = "android.permission.ACCESS_FINE_LOCATION"
    const val COARSE_LOCATION = "android.permission.ACCESS_COARSE_LOCATION"
    const val POST_NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"

    /**
     * The engine asks for it before a page reaches the local network on Android versions that
     * guard it. Krunk targets API 35, where the system does not; it is not in the manifest.
     */
    const val LOCAL_NETWORK = "android.permission.ACCESS_LOCAL_NETWORK"
    const val SDK_POST_NOTIFICATIONS = 33

    /** What the user must be told is missing, one per group Android shows in its dialog. */
    enum class Group { CAMERA, MICROPHONE, LOCATION, NOTIFICATIONS, OTHER }

    fun groupOf(permission: String): Group = when (permission) {
        CAMERA -> Group.CAMERA
        RECORD_AUDIO -> Group.MICROPHONE
        FINE_LOCATION, COARSE_LOCATION -> Group.LOCATION
        POST_NOTIFICATIONS -> Group.NOTIFICATIONS
        else -> Group.OTHER
    }

    /**
     * Whether Android's answer is enough. Location: the user may hand out the approximate
     * location only, which is a grant (the engine then gets coarse fixes). Everything else
     * must be granted.
     */
    fun satisfied(asked: List<String>, granted: Set<String>): Boolean {
        val location = asked.filter { groupOf(it) == Group.LOCATION }
        val rest = asked - location.toSet()
        return rest.all { it in granted } && (location.isEmpty() || location.any { it in granted })
    }

    /** The groups that are still missing after Android's answer, for the message. */
    fun missingGroups(asked: List<String>, granted: Set<String>): List<Group> {
        val out = LinkedHashSet<Group>()
        val locationOk = asked.any { groupOf(it) == Group.LOCATION && it in granted }
        for (p in asked) {
            val g = groupOf(p)
            if (g == Group.LOCATION) {
                if (!locationOk) out += g
            } else if (p !in granted) {
                out += g
            }
        }
        return out.toList()
    }
}
