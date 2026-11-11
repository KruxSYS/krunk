package org.krunk.browser

import android.content.Context
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.GeckoEngine

/** Release builds have no debug hooks (the debug build's version of this file has them). */
object DebugHooks {
    @Suppress("UNUSED_PARAMETER")
    fun install(context: Context, engine: GeckoEngine, core: KrunkCore) = Unit
}
