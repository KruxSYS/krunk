package org.krunk.browser

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.util.Log
import androidx.core.content.ContextCompat
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import org.json.JSONObject
import org.krunk.browser.core.KrunkCore
import org.krunk.browser.engine.GeckoEngine
import org.krunk.browser.engine.blocker.AllowList
import org.krunk.browser.engine.blocker.BlockerController

/**
 * Debug builds only (this file is in the `debug` source set): commands for the device checks,
 * sent with `adb shell am broadcast -a org.krunk.browser.DEBUG --es cmd <name> ...`, answered in
 * the log under the tag `KrunkGuard`. Used by `.pipeline/evidence-android-0.2.0/bin/a3.sh`.
 *
 *   status                         the blocker's state, the allow list, the last update
 *   allow | unallow --es host H    change Krunk's allow list and send it to the blocker
 *   update                         "Update filter lists"
 *   lists                          the blocker's selected lists
 *   setting --es name adblock|auto|strip --es value on|off
 *   blocker --es channel C --es msg JSON     one message to the blocker (e.g. to force its
 *                                  auto-update on for the gate test)
 *   guard-disable                  disable the guard extension (the "guard lost" test)
 *   recreate-session               throw the active tab's engine session away and give the tab a
 *                                  new one, as after a discard or a crash of the page's process
 */
object DebugHooks {
    private const val ACTION = "org.krunk.browser.DEBUG"
    private const val MAX_REPLY = 1500
    private val scope = MainScope()

    fun install(context: Context, engine: GeckoEngine, core: KrunkCore) {
        ContextCompat.registerReceiver(
            context,
            object : BroadcastReceiver() {
                override fun onReceive(c: Context, intent: Intent) {
                    if (intent.getStringExtra("cmd") == "recreate-session") {
                        recreateSession(c.applicationContext as KrunkApp)
                    } else {
                        run(intent, engine, core)
                    }
                }
            },
            IntentFilter(ACTION),
            ContextCompat.RECEIVER_EXPORTED,
        )
    }

    private fun say(text: String) {
        Log.i(BlockerController.GUARD_TAG, "debug: $text")
    }

    /** The active tab gets a new session through the tab manager's own path (crash recovery). */
    private fun recreateSession(app: KrunkApp) {
        val tabs = app.tabs
        val id = tabs.activeId.value
        if (!tabs.started || id == null || !tabs.recreateSession(id)) {
            say("recreate-session: no active tab with a session")
            return
        }
        say("recreate-session: tab $id has a new session, desktop=${tabs.session(id)?.current?.desktopMode}")
    }

    private fun run(intent: Intent, engine: GeckoEngine, core: KrunkCore) {
        val blocker = engine.blocker
        val allow = AllowList(core.store)
        when (val cmd = intent.getStringExtra("cmd")) {
            "status" -> say(
                "status ${blocker.debugState()} allow=${allow.hosts()} " +
                    "updated=${runCatching { core.store.kvGet(AllowList.KV_LISTS_UPDATED) }.getOrNull()} " +
                    "adblock=${core.settings.current.adblockEnabled} auto=${core.settings.current.filterAutoUpdate} " +
                    "strip=${core.settings.current.stripTrackingParams}",
            )

            "allow", "unallow" -> {
                val host = intent.getStringExtra("host").orEmpty()
                val result = if (cmd == "allow") allow.allow(host) else allow.remove(host)
                blocker.allowListChanged { ok ->
                    say("$cmd $host -> $result, blocker took it: $ok, list ${allow.hosts()}")
                }
            }

            "update" -> {
                blocker.updateNow()
                say("update started: ${blocker.update.value}")
            }

            "lists" -> blocker.lists { say("lists ${it?.joinToString { l -> "${l.key}:${l.filters}:${l.written}" }}") }

            "setting" -> {
                val on = intent.getStringExtra("value") == "on"
                val name = intent.getStringExtra("name")
                scope.launch {
                    val r = core.settings.update { s ->
                        when (name) {
                            "adblock" -> s.copy(adblockEnabled = on)
                            "auto" -> s.copy(filterAutoUpdate = on)
                            "strip" -> s.copy(stripTrackingParams = on)
                            else -> s
                        }
                    }
                    if (r.isSuccess) engine.applySettings(core.settings.current)
                    say("setting $name=$on saved=${r.isSuccess}")
                }
            }

            "blocker" -> blocker.debugCall(
                intent.getStringExtra("channel") ?: "dashboard",
                JSONObject(intent.getStringExtra("msg") ?: "{}"),
            ) { ok, reply -> say("blocker reply ok=$ok ${reply.toString().take(MAX_REPLY)}") }

            "guard-disable" -> {
                blocker.debugDisableGuard()
                say("guard disable requested")
            }

            else -> say("unknown command $cmd")
        }
    }
}
