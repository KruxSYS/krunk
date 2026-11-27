package org.krunk.browser.cast

import android.view.Menu
import com.google.android.gms.cast.framework.CastButtonFactory
import com.google.android.gms.cast.framework.media.widget.ExpandedControllerActivity
import org.krunk.browser.R

/**
 * The Cast SDK's full-screen controls for what plays on the Cast device (play/pause, seek bar):
 * opened after a video was sent and from the SDK's media notification. The Cast button in its
 * toolbar opens the SDK's device dialog with "Stop casting".
 */
class KrunkExpandedControllerActivity : ExpandedControllerActivity() {
    override fun onCreateOptionsMenu(menu: Menu): Boolean {
        super.onCreateOptionsMenu(menu)
        menuInflater.inflate(R.menu.cast_expanded_controller, menu)
        CastButtonFactory.setUpMediaRouteButton(this, menu, R.id.cast_route_item)
        return true
    }
}
