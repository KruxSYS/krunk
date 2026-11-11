package org.krunk.browser

import android.content.Intent
import android.os.Bundle
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import org.krunk.browser.ui.BrowserViewModel
import org.krunk.browser.ui.KrunkRoot
import org.krunk.ffi.inputToUrl

class MainActivity : ComponentActivity() {
    private val vm: BrowserViewModel by viewModels()
    private val app get() = application as KrunkApp

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        app.tabs.start()
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
        when (intent.action) {
            Intent.ACTION_VIEW -> intent.dataString?.let { url ->
                Log.i(KrunkApp.TAG, "open from intent: $url")
                app.tabs.openTab(url, activate = true, live = true)
                vm.showBrowser()
            }
            Intent.ACTION_SEND -> intent.getStringExtra(Intent.EXTRA_TEXT)?.let { text ->
                app.tabs.openTab(inputToUrl(text.trim(), app.core.settings.searchTemplate), activate = true, live = true)
                vm.showBrowser()
            }
            Intent.ACTION_WEB_SEARCH -> intent.getStringExtra("query")?.let { q ->
                app.tabs.openTab(inputToUrl(q, app.core.settings.searchTemplate), activate = true, live = true)
                vm.showBrowser()
            }
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
