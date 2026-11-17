//! Picture-in-Picture (Krunk 0.6.0, R24): the engine's own floating video window, driven
//! through DevTools with a user gesture (CEF allows PiP only with user activation, and the
//! shell's button lives in another window). The page's media script (`ui/inject/media.js`)
//! lists the videos and does the request on a `krunk-pip` DOM event fired by the evaluate
//! call, so nothing is added to the page's globals.
//!
//! The volume slider (0.7.0, R8; inside the player since 0.11.0): the engine's floating window
//! has mute but no volume. Krunk finds that window (`ffi::pipwin`), follows it and takes the
//! mouse wheel over it. While the pointer is on the engine's mute button (or on the slider), and
//! for a moment after a change, Krunk's own small popup view with a volume slider opens inside
//! the window, next to that button. (Until 0.10 it was a separate bar under the window, shown
//! whenever the pointer was over the video.) The slider sets the video's volume through the
//! same DOM event; the page's own changes (the engine's mute button too) come back with the
//! media script's report.

use super::{Browser, PopupSlot, TabId};
use crate::ffi::pipwin::{self, PipEvents, PipWindow};
use crate::ffi::window::{ScreenRect, WindowEvents};
use crate::log::{info, warning};
use lb_core::layout;
use lb_core::protocol::{Event, PopupContent, PopupRect, VideoInfo};
use lb_core::storage::now_ms;
use lb_core::urlclean;
use std::rc::Weak;

/// Polls while a video is in the floating window: finds the window, hides the bar.
pub(super) const PIP_TIMER: usize = 18;
const PIP_TICK_MS: u32 = 100;
/// The engine opens its window shortly after the page's `enterpictureinpicture`.
const FIND_TRIES: u32 = 30;
/// The bar stays this long after the pointer left (to reach it from the window) …
const HOVER_GRACE_MS: u64 = 300;
/// … and this long after a volume change.
const CHANGE_HOLD_MS: u64 = 2000;
/// One wheel notch.
const WHEEL_STEP: f64 = 5.0;

/// The floating window of one tab's video and its volume bar.
pub(super) struct PipBar {
    tab: TabId,
    window: Option<PipWindow>,
    tries: u32,
    volume: u32,
    muted: bool,
    shown: bool,
    /// Keep the bar until then (ms), even without the pointer.
    hold_until: u64,
}

/// The bar's popup window takes no activation and needs no window events.
pub(super) struct PipBarEvents;

impl WindowEvents for PipBarEvents {}

/// The engine window's moves, wheel and hovering, handled after its message.
struct PipHook(Weak<Browser>);

impl PipEvents for PipHook {
    fn wheel(&self, delta: f64) {
        if let Some(b) = self.0.upgrade() {
            b.defer(move |b| b.pip_wheel(delta));
        }
    }
    fn moved(&self) {
        if let Some(b) = self.0.upgrade() {
            b.defer(|b| b.pip_bar_place());
        }
    }
    fn hovered(&self) {
        if let Some(b) = self.0.upgrade() {
            b.defer(|b| b.pip_hovered());
        }
    }
    fn destroyed(&self) {
        if let Some(b) = self.0.upgrade() {
            b.defer(|b| b.pip_window_gone());
        }
    }
}

/// The evaluate expression that sets the volume of the video in the floating window.
fn volume_expression(volume: u32, muted: bool) -> String {
    format!(
        "(function(){{var r=null;document.dispatchEvent(new CustomEvent('krunk-pip',{{detail:{{volume:{volume},\
         muted:{muted},done:function(p){{r=p}}}}}}));return r||Promise.reject(new Error('no media script'))}})()"
    )
}

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

/// The evaluate expression for the context menu's item: the video at CSS point `x`,`y` of the
/// main frame, else the one playing `src`.
fn pip_at_expression(x: f64, y: f64, src: &str) -> String {
    let src = serde_json::to_string(src).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{var r=null;document.dispatchEvent(new CustomEvent('krunk-pip',{{detail:{{at:{{x:{x},y:{y}}},\
         src:{src},done:function(p){{r=p}}}}}}));return r||Promise.reject(new Error('no media script'))}})()"
    )
}

impl Browser {
    /// The context menu on a video (0.22.0): "Exit picture-in-picture" while one of the tab's
    /// videos is in it, else "Picture-in-picture" if the page has a video that allows it (the
    /// media script lists only those; which one was clicked is checked when chosen).
    pub(super) fn pip_menu_label(&self, tab: TabId) -> Option<String> {
        let views = self.views.try_borrow().ok()?;
        let videos = views.get(&tab)?.net.videos.borrow();
        if videos.iter().any(|v| v.pip) {
            Some("Exit picture-in-picture".into())
        } else {
            (!videos.is_empty()).then(|| "Picture-in-picture".into())
        }
    }

    /// The context menu's item was chosen: leave PiP, or send the clicked video (by the
    /// menu's point, else by its address) with a user gesture, as the toolbar button does.
    pub(super) fn pip_menu(&self, tab: TabId, x: i32, y: i32, src: String) {
        let in_pip = self.views.borrow().get(&tab).is_some_and(|v| v.net.videos.borrow().iter().any(|v| v.pip));
        if in_pip {
            self.pip_exit(tab);
            return;
        }
        let Some(view) = self.tab_view(tab) else { return };
        // The menu's point is in view DIP; the page's are CSS pixels.
        let zoom = view.zoom().max(0.01);
        self.pip_requested.set(Some((tab, now_ms())));
        let params = serde_json::json!({
            "expression": pip_at_expression(f64::from(x) / zoom, f64::from(y) / zoom, &src),
            "userGesture": true,
            "awaitPromise": true,
            "returnByValue": true,
        });
        let weak = self.me.clone();
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            let why = match r {
                Ok(v) if v.get("exceptionDetails").is_none() => {
                    info!("tab {tab}: picture-in-picture from the context menu");
                    return;
                }
                Ok(v) => v
                    .pointer("/exceptionDetails/exception/description")
                    .and_then(|d| d.as_str())
                    .map(|d| d.lines().next().unwrap_or("").to_string())
                    .unwrap_or_else(|| "refused".into()),
                Err(e) => e,
            };
            warning!("tab {tab}: picture-in-picture from the context menu refused: {why}");
            let message = if why.contains("disablePictureInPicture") {
                "This video doesn't allow picture-in-picture"
            } else {
                "This video can't be shown in picture-in-picture"
            };
            if let Some(b) = weak.upgrade() {
                b.defer(move |b| b.image_toast(super::source::MenuFrom::Tab(tab), message));
            }
        });
    }

    /// The page's media script reported its videos (main frame only, for the page shown).
    pub(super) fn media_changed(&self, tab: TabId, origin: &str, videos: Vec<VideoInfo>) {
        let Some(net) = self.views.borrow().get(&tab).map(|v| v.net.clone()) else { return };
        if urlclean::origin_of(&net.top_url()).as_deref() != Some(origin) {
            return;
        }
        let pip_before = net.videos.borrow().iter().any(|v| v.pip);
        let in_pip = videos.iter().find(|v| v.pip).map(|v| (v.volume.min(100), v.muted));
        let pip_now = in_pip.is_some();
        *net.videos.borrow_mut() = videos;
        if pip_before != pip_now {
            info!("tab {tab}: picture-in-picture {}", if pip_now { "on" } else { "off" });
            if !pip_now && self.auto_pip_tab.get() == Some(tab) {
                self.auto_pip_tab.set(None);
            }
        }
        match in_pip {
            Some((volume, muted)) => self.pip_bar_update(tab, volume, muted),
            None if self.pip_bar.borrow().as_ref().is_some_and(|p| p.tab == tab) => self.pip_bar_stop(),
            None => {}
        }
        self.send_nav_if_active(tab);
    }

    /// "Back to tab" in the floating window (CEF does not implement `ActivateContents`, so
    /// the page's script tells the host): the tab and its window come to the front.
    pub(super) fn pip_back_to_tab(&self, tab: TabId) {
        // Another tab's video just took the floating window (there is only one): this video
        // left it still playing, which looks like "Back to tab" but is not (0.24.2).
        let taken = self.pip_requested.get().is_some_and(|(t, at)| t != tab && now_ms() - at < 3000)
            || self.pip_tab().is_some_and(|t| t != tab);
        if taken {
            info!("tab {tab}: video left the floating window for another tab's; not a back to tab");
            return;
        }
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
        self.pip_requested.set(Some((tab, now_ms())));
        let weak = self.me.clone();
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
                Browser::auto_pip_refused(&weak, tab);
            }
            Err(e) => {
                warning!("tab {tab}: picture-in-picture: {e}");
                Browser::auto_pip_refused(&weak, tab);
            }
        });
    }

    /// An automatic request that failed no longer holds the floating window for its tab.
    fn auto_pip_refused(weak: &std::rc::Weak<Browser>, tab: TabId) {
        if let Some(b) = weak.upgrade() {
            if b.auto_pip_tab.get() == Some(tab) {
                b.auto_pip_tab.set(None);
            }
        }
    }

    /// The tab whose video is in the floating window now, as its media script last said.
    fn pip_tab(&self) -> Option<TabId> {
        self.views.borrow().iter().find(|(_, v)| v.net.videos.borrow().iter().any(|x| x.pip)).map(|(id, _)| *id)
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
        // One floating window: a video already in it (or on its way) keeps it, so two tabs
        // with media don't take it from each other (0.24.2).
        if let Some(holder) = self.auto_pip_tab.get().filter(|t| *t != from && self.views.borrow().contains_key(t)) {
            info!("tab {from}: no automatic picture-in-picture, tab {holder}'s video has the floating window");
            return;
        }
        if let Some(holder) = self.pip_tab().filter(|t| *t != from) {
            info!("tab {from}: no automatic picture-in-picture, tab {holder}'s video has the floating window");
            return;
        }
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

// ------------------------------------------------------------ the volume bar (0.7.0, R8)

impl Browser {
    /// The page reported the video in the floating window (with its volume): start following
    /// that window, or show the page's own change on the bar.
    fn pip_bar_update(&self, tab: TabId, volume: u32, muted: bool) {
        let other = self.pip_bar.borrow().as_ref().is_some_and(|p| p.tab != tab);
        if other {
            self.pip_bar_stop();
        }
        let refresh = {
            let mut guard = self.pip_bar.borrow_mut();
            match guard.as_mut() {
                Some(p) => {
                    let changed = (p.volume, p.muted) != (volume, muted);
                    p.volume = volume;
                    p.muted = muted;
                    changed && p.shown
                }
                None => {
                    *guard = Some(PipBar { tab, window: None, tries: 0, volume, muted, shown: false, hold_until: 0 });
                    if let Some(app) = self.app() {
                        app.set_timer(PIP_TIMER, PIP_TICK_MS);
                    }
                    false
                }
            }
        };
        if refresh {
            self.pip_bar_post();
        }
        if self.pip_bar.borrow().as_ref().is_some_and(|p| p.window.is_none()) {
            self.pip_find();
        }
    }

    /// Looks for the engine's window (the page announces PiP a moment before it appears).
    fn pip_find(&self) {
        let found = {
            let mut guard = self.pip_bar.borrow_mut();
            let Some(p) = guard.as_mut().filter(|p| p.window.is_none()) else { return };
            p.tries += 1;
            let w = pipwin::find(&[]);
            if let Some(w) = w {
                p.window = Some(w);
                let r = w.rect();
                info!("tab {}: floating video window found ({}x{} at {},{})", p.tab, r.width, r.height, r.x, r.y);
            } else if p.tries == FIND_TRIES {
                warning!("tab {}: no floating video window found; no volume bar", p.tab);
            }
            w
        };
        if let Some(w) = found {
            w.watch(std::rc::Rc::new(PipHook(self.me.clone())));
        }
    }

    /// Every [`PIP_TICK_MS`] while a video is in the floating window.
    pub(super) fn pip_tick(&self) {
        let (window, tries, shown, hold_until) = match self.pip_bar.borrow().as_ref() {
            Some(p) => (p.window, p.tries, p.shown, p.hold_until),
            None => {
                if let Some(app) = self.app() {
                    app.kill_timer(PIP_TIMER);
                }
                return;
            }
        };
        let Some(w) = window else {
            if tries < FIND_TRIES {
                self.pip_find();
            }
            return;
        };
        if !w.exists() {
            self.pip_window_gone();
            return;
        }
        let over = self.pip_pointer_over(w);
        let now = now_ms() as u64;
        if over {
            if let Some(p) = self.pip_bar.borrow_mut().as_mut() {
                p.hold_until = p.hold_until.max(now + HOVER_GRACE_MS);
            }
            if !shown {
                self.pip_bar_show();
            }
        } else if shown && now >= hold_until {
            self.pip_bar_hide();
        }
    }

    /// Is the pointer on the engine's mute button (a little around it) or on the slider?
    fn pip_pointer_over(&self, w: PipWindow) -> bool {
        let (x, y) = crate::ffi::cursor::position();
        let inside = |r: ScreenRect| x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;
        let slider = self.pip_popup.borrow().as_ref().filter(|p| p.window.is_visible()).map(|p| p.window.rect());
        w.is_visible() && inside(Self::pip_mute_area(w)) || slider.is_some_and(inside)
    }

    /// The engine's mute button, with a margin, and the gap to the slider.
    fn pip_mute_area(w: PipWindow) -> ScreenRect {
        let r = w.rect();
        let m = layout::pip_mute_rect(layout::Rect { x: r.x, y: r.y, width: r.width, height: r.height }, w.scale());
        let pad = (6.0 * w.scale()).round() as i32;
        ScreenRect { x: m.x - pad, y: m.y - pad, width: m.width + 2 * pad, height: m.height + 2 * pad }
    }

    fn pip_hovered(&self) {
        let shown = self.pip_bar.borrow().as_ref().map(|p| p.shown);
        let over = self.pip_bar.borrow().as_ref().and_then(|p| p.window).is_some_and(|w| self.pip_pointer_over(w));
        if shown == Some(false) && over {
            if let Some(p) = self.pip_bar.borrow_mut().as_mut() {
                p.hold_until = now_ms() as u64 + HOVER_GRACE_MS;
            }
            self.pip_bar_show();
        }
    }

    /// The slider's rectangle inside the floating window, next to its mute button (screen
    /// pixels).
    fn pip_bar_rect(w: PipWindow) -> ScreenRect {
        let r = w.rect();
        let b = layout::pip_slider_rect(layout::Rect { x: r.x, y: r.y, width: r.width, height: r.height }, w.scale());
        ScreenRect { x: b.x, y: b.y, width: b.width, height: b.height }
    }

    fn pip_bar_show(&self) {
        let Some(w) = self.pip_bar.borrow().as_ref().and_then(|p| p.window) else { return };
        if !w.is_visible() {
            return;
        }
        if self.pip_popup.borrow().is_none() {
            // Owned by the hidden application window: the bar stays when a browser window is
            // minimised (the floating window does too).
            let Some(app) = self.app() else { return };
            if !self.create_popup(app, PopupSlot::Pip) {
                return;
            }
        }
        if let Some(p) = self.pip_bar.borrow_mut().as_mut() {
            p.shown = true;
        }
        let r = Self::pip_bar_rect(w);
        if let Some(popup) = self.pip_popup.borrow().as_ref() {
            popup.window.set_activatable(false);
            popup.window.set_topmost(true);
            popup.window.show_at(r, false);
            if let Some(c) = &popup.view {
                c.set_bounds(0, 0, r.width, r.height);
                c.set_visible(true);
            }
        }
        self.pip_bar_post();
    }

    /// The floating window moved or changed size: the bar goes with it.
    fn pip_bar_place(&self) {
        let Some(w) = self.pip_bar.borrow().as_ref().filter(|p| p.shown).and_then(|p| p.window) else { return };
        if !w.is_visible() {
            self.pip_bar_hide();
            return;
        }
        let r = Self::pip_bar_rect(w);
        if let Some(popup) = self.pip_popup.borrow().as_ref() {
            popup.window.show_at(r, false);
            if let Some(c) = &popup.view {
                c.set_bounds(0, 0, r.width, r.height);
            }
        }
    }

    /// Sends the bar its content (now or once its document is ready).
    fn pip_bar_post(&self) {
        let Some(content) = self.pip_bar.borrow().as_ref().map(|p| PopupContent::PipVolume {
            tab_id: p.tab,
            volume: p.volume,
            muted: p.muted,
        }) else {
            return;
        };
        let mut guard = self.pip_popup.borrow_mut();
        let Some(popup) = guard.as_mut() else { return };
        popup.content = Some(content.clone());
        if popup.ready {
            if let Some(c) = &popup.view {
                c.post_json(&Event::Popup { content, seq: None }.to_json());
            }
        }
    }

    fn pip_bar_hide(&self) {
        if let Some(p) = self.pip_bar.borrow_mut().as_mut() {
            p.shown = false;
        }
        if let Some(popup) = self.pip_popup.borrow().as_ref() {
            if popup.window.is_visible() {
                popup.window.hide();
            }
        }
    }

    /// The video left the floating window (or its tab is gone).
    fn pip_bar_stop(&self) {
        let bar = self.pip_bar.borrow_mut().take();
        if let Some(w) = bar.and_then(|p| p.window) {
            w.unwatch();
        }
        self.pip_bar_hide();
        if let Some(app) = self.app() {
            app.kill_timer(PIP_TIMER);
        }
    }

    fn pip_window_gone(&self) {
        let tab = self.pip_bar.borrow().as_ref().map(|p| p.tab);
        if let Some(tab) = tab {
            info!("tab {tab}: floating video window closed");
        }
        self.pip_bar_stop();
    }

    /// The bar's slider or mute button.
    pub(super) fn pip_set_volume(&self, tab: TabId, volume: u32, muted: bool) {
        let volume = volume.min(100);
        {
            let mut guard = self.pip_bar.borrow_mut();
            let Some(p) = guard.as_mut().filter(|p| p.tab == tab) else { return };
            p.volume = volume;
            p.muted = muted;
            p.hold_until = now_ms() as u64 + CHANGE_HOLD_MS;
        }
        let Some(view) = self.tab_view(tab) else { return };
        let params = serde_json::json!({
            "expression": volume_expression(volume, muted),
            "awaitPromise": true,
            "returnByValue": true,
        });
        view.devtools_call("Runtime.evaluate", &params, move |r| {
            let set = r.as_ref().ok().and_then(|v| v.pointer("/result/value/set")).and_then(|v| v.as_bool());
            if set != Some(true) {
                warning!("tab {tab}: volume of the floating video not set ({r:?})");
            }
        });
    }

    /// The wheel over the floating window: up is louder; leaves mute.
    fn pip_wheel(&self, notches: f64) {
        let Some((tab, volume)) = self.pip_bar.borrow().as_ref().map(|p| (p.tab, p.volume)) else { return };
        let next = (f64::from(volume) + notches * WHEEL_STEP).round().clamp(0.0, 100.0) as u32;
        self.pip_set_volume(tab, next, false);
        if let Some(p) = self.pip_bar.borrow_mut().as_mut() {
            p.hold_until = now_ms() as u64 + CHANGE_HOLD_MS;
        }
        self.pip_bar_show();
    }
}
