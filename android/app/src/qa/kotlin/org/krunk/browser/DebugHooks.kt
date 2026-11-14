package org.krunk.browser

import android.content.Context
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.GeckoEngine

/** QA builds (release code, lower versionCode) have no debug hooks, like release builds. */
object DebugHooks {
    @Suppress("UNUSED_PARAMETER")
    fun install(context: Context, engine: GeckoEngine, core: KrunkCore) = Unit
}
