//! Picture-in-Picture (Krunk 0.6.0, R24): the engine's own floating video window, driven
//! through DevTools with a user gesture (CEF allows PiP only with user activation, and the
//! shell's button lives in another window). The page's media script (`ui/inject/media.js`)
//! lists the videos and does the request on a `krunk-pip` DOM event fired by the evaluate
//! call, so nothing is added to the page's globals.

use super::{Browser, TabId};
use crate::log::{info, warning};
use lb_core::protocol::{PopupContent, PopupRect, VideoInfo};
use lb_core::storage::now_ms;
use lb_core::urlclean;

/// The evaluate expression: asks the media script for video `index`, or to leave PiP.
fn pip_expression(index: Option<u32>) -> String {
    let request = match index {
        Some(i) => format!("index:{i}"),
        None => "exit:true".to_string(),
    };
    format!(
        "(function(){{var r=null;document.dispatchEvent(new CustomEvent('krunk-pip',{{detail:{{{request},\
         done:function(p){{r=p}}}}}}));return r||Promise.reject(new Error('no media script'))}})()"
    )
}

impl Browser {
    /// The page's media script reported its videos (main frame only, for the page shown).
    pub(super) fn media_changed(&self, tab: TabId, origin: &str, videos: Vec<VideoInfo>) {
        let Some(net) = self.views.borrow().get(&tab).map(|v| v.net.clone()) else { return };
        if urlclean::origin_of(&net.top_url()).as_deref() != Some(origin) {
            return;
        }
        let pip_before = net.videos.borrow().iter().any(|v| v.pip);
        let pip_now = videos.iter().any(|v| v.pip);
        *net.videos.borrow_mut() = videos;
        if pip_before != pip_now {
            info!("tab {tab}: picture-in-picture {}", if pip_now { "on" } else { "off" });
            if !pip_now && self.auto_pip_tab.get() == Some(tab) {
                self.auto_pip_tab.set(None);
            }
        }
        self.send_nav();
    }

    /// "Back to tab" in the floating window (CEF does not implement `ActivateContents`, so
    /// the page's script tells the host): the tab and its window come to the front.
    pub(super) fn pip_back_to_tab(&self, tab: TabId) {
        let Some(win) = self.window_of(tab) else { return };
        info!("tab {tab}: back to tab from the floating video window");
        let now = now_ms();
        let ops =
            self.ws.borrow_mut().update(win, |m| ((), m.activate(tab, now))).map(|(_, ops)| ops).unwrap_or_default();
        self.apply(ops);
        if let Some(f) = self.frame(win) {
            f.window.bring_to_front();
        }
    }

    /// The toolbar button, the menu item or Alt+P: leave PiP when a video is in it; else one
    /// video goes straight in, several open the chooser (under the button, or as a prompt).
    pub(super) fn pip_command(&self, anchor: Option<PopupRect>) {
        let Some(tab) = self.active_id() else { return };
        let videos: Vec<VideoInfo> =
            self.views.borrow().get(&tab).map(|v| v.net.videos.borrow().clone()).unwrap_or_default();
        if videos.iter().any(|v| v.pip) {
            self.pip_exit(tab);
            return;
        }
        match videos.len() {
            0 => info!("tab {tab}: no video for picture-in-picture"),
            1 => self.pip_request(tab, videos[0].index),
            _ => {
                let content = PopupContent::Pip { tab_id: tab, videos };
                match anchor {
                    Some(r) => self.show_popup(content, Some(r)),
                    None => self.show_prompt(content, 0),
                }
            }
        }
    }

    /// A row of the chooser.
    pub(super) fn pip_pick(&self, tab: TabId, index: u32) {
        self.hide_popup();
        let known =
            self.views.borrow().get(&tab).is_some_and(|v| v.net.videos.borrow().iter().any(|x| x.index == index));
        if !known {
            warning!("tab {tab}: picture-in-picture pick of an unknown video {index}");
            return;
        }
        self.pip_request(tab, index);
    }

    fn pip_request(&self, tab: TabId, index: u32) {
        let Some(view) = self.tab_view(tab) else { return };
        let params = serde_json::json!({
            "expression": pip_expression(Some(index)),
            "userGesture": true,
            "awaitPromise": true,
            "returnByValue": true,
        });
        view.devtools_call("Runtime.evaluate", &params, move |r| match r {
            Ok(v) if v.get("exceptionDetails").is_none() => {
                let size = |k: &str| v.pointer(&format!("/result/value/{k}")).and_then(|x| x.as_u64()).unwrap_or(0);
                info!("tab {tab}: picture-in-picture, video {index} ({}x{})", size("width"), size("height"));
            }
            Ok(v) => {
                let why = v
                    .pointer("/exceptionDetails/exception/description")
                    .and_then(|d| d.as_str())
                    .map(|d| d.lines().next().unwrap_or("").to_string())
                    .unwrap_or_else(|| "refused".into());
                warning!("tab {tab}: picture-in-picture refused: {why}");
            }
            Err(e) => warning!("tab {tab}: picture-in-picture: {e}"),
        });
    }

    /// Leaves the floating window (no gesture needed).
    pub(super) fn pip_exit(&self, tab: TabId) {
        let Some(view) = self.tab_view(tab) else { return };
        let params = serde_json::json!({
            "expression": pip_expression(None),
            "awaitPromise": true,
            "returnByValue": true,
        });
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            if let Err(e) = r {
                warning!("tab {tab}: leaving picture-in-picture: {e}");
            }
        });
    }

    /// Automatic Picture-in-Picture (setting, off by default): leaving a tab that plays a
    /// video sends the video to the floating window; coming back takes it out again.
    pub(super) fn auto_pip_switch(&self, from: Option<TabId>, to: TabId) {
        if !self.settings.borrow().auto_pip {
            return;
        }
        if self.auto_pip_tab.get() == Some(to) {
            self.auto_pip_tab.set(None);
            self.pip_exit(to);
            return;
        }
        let Some(from) = from.filter(|f| *f != to) else { return };
        let playing = self.views.borrow().get(&from).and_then(|v| {
            let vs = v.net.videos.borrow();
            if vs.iter().any(|x| x.pip) {
                None
            } else {
                vs.iter().find(|x| x.playing).map(|x| x.index)
            }
        });
        if let Some(index) = playing {
            info!("tab {from}: automatic picture-in-picture (tab left while playing)");
            self.auto_pip_tab.set(Some(from));
            self.pip_request(from, index);
        }
    }
}
