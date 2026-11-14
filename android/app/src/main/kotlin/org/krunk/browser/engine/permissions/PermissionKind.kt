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

    /**
     * Whether the engine's own entry for an answer of this kind that is not a row of the table
     * (a one-time answer, a default, a private tab's) is forgotten right after the engine wrote
     * it, instead of when the tab leaves the page. Then it cannot reach the same site in another
     * tab, and Krunk's per-tab memory of the answer ([PermissionPolicy]) serves the page's next
     * request without a prompt. Denies of every kind the user is asked about; allows only where
     * the engine needs no entry to keep serving the page: a location fix and a DRM key session
     * are handed over when allowed, while notifications (checked at every `new Notification`),
     * persistent storage (`persisted()`) and local network/device access (checked per
     * connection) read the entry again and keep it until the tab leaves the page. Autoplay is
     * asked for at every play, and storage access keeps the engine's own dated entry.
     */
    fun forgetEngineEntryAtOnce(allow: Boolean): Boolean = when (this) {
        STORAGE_ACCESS, AUTOPLAY_AUDIBLE, AUTOPLAY_INAUDIBLE -> false
        GEOLOCATION, DRM -> true
        else -> !allow
    }

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

    /** Android 12: the user can hand out the approximate location only. */
    const val SDK_APPROXIMATE_LOCATION = 31

    /** Both location permissions: what is checked for any location request. */
    val LOCATION = listOf(FINE_LOCATION, COARSE_LOCATION)

    /**
     * Whether the engine can serve a page's location with what is [granted]. The engine itself
     * asks only for `ACCESS_FINE_LOCATION` after a site was allowed (`GeckoViewPermission.sys.mjs`:
     * `getAppPermissions([PERM_ACCESS_FINE_LOCATION])`), but what it then does works with the
     * approximate permission alone from Android 12 on: `GeckoAppShell.updateLocationListeners`
     * takes `LocationManager.getBestProvider(criteria, enabledOnly = true)` and
     * `getLastKnownLocation` of every provider, which Android 12+ allows with
     * `ACCESS_COARSE_LOCATION` and answers with coarsened fixes. Before Android 12 the GPS
     * provider throws without the precise permission (and Android's dialog had no "approximate"
     * choice), so there precise is required.
     */
    fun locationUsable(granted: Set<String>, sdk: Int): Boolean =
        FINE_LOCATION in granted || (sdk >= SDK_APPROXIMATE_LOCATION && COARSE_LOCATION in granted)

    /**
     * Whether Android's answer is enough for [asked]: everything but location must be granted;
     * location by [locationUsable], whichever of the two was asked ([granted] holds the state of
     * both location permissions when location was asked: see [toCheck]).
     */
    fun satisfied(asked: List<String>, granted: Set<String>, sdk: Int): Boolean {
        val rest = asked.filter { groupOf(it) != Group.LOCATION }
        val location = rest.size < asked.size
        return rest.all { it in granted } && (!location || locationUsable(granted, sdk))
    }

    /** What to check when [asked] is asked for: with location, both location permissions. */
    fun toCheck(asked: List<String>): List<String> =
        if (asked.any { groupOf(it) == Group.LOCATION }) (asked + LOCATION).distinct() else asked

    /** The groups that are still missing after Android's answer, for the message. */
    fun missingGroups(asked: List<String>, granted: Set<String>, sdk: Int): List<Group> {
        val out = LinkedHashSet<Group>()
        val locationOk = locationUsable(granted, sdk)
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
