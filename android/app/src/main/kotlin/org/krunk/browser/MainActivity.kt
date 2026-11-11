package org.krunk.browser

import android.content.Intent
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import org.krunk.browser.engine.LoadPolicy
import org.krunk.browser.engine.LoadSource
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.KrunkRoot
import org.krunk.ffi.searchUrl

class MainActivity : ComponentActivity() {
    private val vm: BrowserViewModel by viewModels()
    private val app get() = application as KrunkApp

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // Saved instance state means Android is bringing the task back (it killed the process in
        // the background): the tabs return whatever the start-up setting is.
        app.tabs.start(restoredBySystem = savedInstanceState != null)
        handleIntent(intent, fresh = savedInstanceState == null)
        setContent { KrunkRoot(vm) }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent, fresh = true)
    }

    private fun handleIntent(intent: Intent?, fresh: Boolean) {
        intent ?: return
        if (!fresh) return
        // Reopened from Recents: the system hands back the intent that started the task long
        // ago. Its link was opened then; opening it again would add a tab on every return.
        if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        // What another app sends is never trusted: only web addresses are opened, and they are
        // loaded as external links (LoadPolicy). Addresses are not written to the log.
        val url = when (intent.action) {
            Intent.ACTION_VIEW -> LoadPolicy.fromViewIntent(intent.dataString).also {
                if (it == null) Log.w(KrunkApp.TAG, "ignored a VIEW intent that is not a web address")
            }

            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)?.takeIf { it.isNotBlank() }?.let { text ->
                // A shared link opens; any other text is searched for (never loaded as an address).
                LoadPolicy.urlInSharedText(text)
                    ?: searchUrl(LoadPolicy.searchText(text), app.core.settings.searchTemplate)
            }

            else -> null
        }
        if (url != null && app.tabs.openUntrusted(url, LoadSource.EXTERNAL_APP, activate = true, live = true) != null) {
            vm.showBrowser()
        }
        // Consume so a rotation does not reopen it.
        intent.action = null
    }

    override fun onStart() {
        super.onStart()
        app.tabs.markRunning()
    }

    override fun onStop() {
        super.onStop()
        // Going to the background (or being finished) is a clean exit as far as crash
        // detection is concerned; the session is saved here too.
        app.tabs.markCleanExit()
    }
}
