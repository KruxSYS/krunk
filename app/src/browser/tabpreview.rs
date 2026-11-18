//! Tab hover cards (0.15.0, like Firefox): the pointer rests on a tab for a moment (the shell
//! waits 500 ms, then sends `TabHover`) and a card under it shows the tab's title, its site and a
//! small picture of the page as it was last seen. Moving to another tab while the card is open
//! updates it at once; leaving the strip, a click, a drag or a scroll hides it.
//!
//! The card is its own popup window ([`PopupSlot::Preview`]): never activated, so an open
//! sign-in or page dialog in the main popup stays where it is. The pictures are small JPEGs
//! (a full-size `Page.captureScreenshot`, cut and scaled to 280 px wide with WIC on a job
//! thread) taken only from the active tab: when the user
//! leaves it (it stays on screen a moment after the switch) and a while after it finished
//! loading. A background tab is never captured again. They are kept in memory only, at most
//! [`CACHE_CAP`], and dropped with their tab. Private tabs (0.22.0) never get a picture: they are
//! never captured, nothing is stored for them and anything cached is dropped; their card shows
//! the title and the site only. With the setting off there are no cards and no captures.

use super::{Browser, PopupSlot};
use crate::ffi::window::WindowEvents;
use crate::log::warning;
use base64ct::{Base64, Encoding};
use lb_core::layout;
use lb_core::protocol::{PopupContent, PopupRect};
use lb_core::tabs::TabId;
use lb_core::urlclean;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

/// Win32 timer: the active tab's picture a while after its page finished loading.
pub(super) const PREVIEW_TIMER: usize = 28;
/// How long after a load the picture is taken (late images and fonts are in by then).
const CAPTURE_AFTER_LOAD_MS: u32 = 1500;
/// At most this many pictures are kept (least recently used go first).
const CACHE_CAP: usize = 48;
/// JPEG quality of the full-size capture (the card's picture is scaled from it).
const CAPTURE_QUALITY: u32 = 70;

#[derive(Default)]
pub(super) struct PreviewHost {
    /// The card's popup window (created when first needed).
    pub(super) popup: RefCell<Option<super::Popup>>,
    /// Pictures as `data:` URLs, least recently used first.
    cache: RefCell<VecDeque<(TabId, String)>>,
    /// The tab the card shows now.
    shown: Cell<Option<TabId>>,
    /// The tab the load timer is for.
    after_load: Cell<Option<TabId>>,
}

/// The card's window has nothing to react to: it never takes activation.
pub(super) struct PreviewEvents;

impl WindowEvents for PreviewEvents {}

impl Browser {
    fn previews_on(&self) -> bool {
        self.settings.try_borrow().is_ok_and(|s| s.tab_hover_preview)
    }

    /// The shell's `TabHover`: the card for tab `id` under `rect`, or none.
    pub(super) fn tab_hover(&self, id: Option<TabId>, rect: Option<PopupRect>) {
        if !self.previews_on() {
            self.preview_clear();
            return;
        }
        let (Some(id), Some(rect)) = (id, rect.filter(PopupRect::is_valid)) else {
            self.hide_tab_preview();
            return;
        };
        // Only for a tab of the current window, and never over another popup (a menu, a dialog).
        let current = self.cur_id().is_some_and(|w| self.window_of(id) == Some(w));
        if !current || self.popup_kind().is_some() {
            self.hide_tab_preview();
            return;
        }
        let active = self.active_id() == Some(id);
        let Some((title, url, private)) = self.tab(id).map(|t| (t.title.clone(), t.url.clone(), t.private)) else {
            return;
        };
        let thumbnail = if active || private {
            if private {
                self.preview_drop(id);
            }
            None
        } else {
            self.preview_get(id)
        };
        let host = urlclean::host_of(&url).unwrap_or_default();
        let url = lb_core::pages::display_url(&url);
        let Some(w) = self.window() else { return };
        let width_css = f64::from(w.client_size().0) / w.scale();
        let place = layout::tab_preview_rect(rect, thumbnail.is_some(), width_css);
        let content = PopupContent::TabPreview { tab_id: id, title, host, url, thumbnail, active, private };
        self.preview.shown.set(Some(id));
        self.show_popup_in(PopupSlot::Preview, content, Some(place));
    }

    /// The card goes (if it is open).
    pub(super) fn hide_tab_preview(&self) {
        if self.preview.shown.take().is_none() {
            return;
        }
        let Ok(mut guard) = self.preview.popup.try_borrow_mut() else { return };
        let Some(p) = guard.as_mut() else { return };
        if p.open() {
            p.window.hide();
            if let Some(c) = &p.view {
                c.post_json(&lb_core::protocol::Event::PopupClear.to_json());
            }
        }
        p.content = None;
        p.pending = None;
    }

    /// The setting went off: no card, no pictures.
    pub(super) fn preview_clear(&self) {
        self.hide_tab_preview();
        self.preview.cache.borrow_mut().clear();
        self.preview.after_load.set(None);
    }

    fn preview_get(&self, id: TabId) -> Option<String> {
        let mut cache = self.preview.cache.borrow_mut();
        let i = cache.iter().position(|(t, _)| *t == id)?;
        let entry = cache.remove(i)?;
        let url = entry.1.clone();
        cache.push_back(entry);
        Some(url)
    }

    fn preview_put(&self, id: TabId, url: String) {
        if self.is_private_tab(id) {
            self.preview_drop(id);
            return;
        }
        let mut cache = self.preview.cache.borrow_mut();
        cache.retain(|(t, _)| *t != id);
        cache.push_back((id, url));
        while cache.len() > CACHE_CAP {
            cache.pop_front();
        }
    }

    /// Is tab `id` private (or gone: treated alike, nothing is kept for it)?
    fn is_private_tab(&self, id: TabId) -> bool {
        self.tab(id).is_none_or(|t| t.private)
    }

    /// Drops the cached picture of tab `id`, if any.
    fn preview_drop(&self, id: TabId) {
        if let Ok(mut cache) = self.preview.cache.try_borrow_mut() {
            cache.retain(|(t, _)| *t != id);
        }
    }

    /// The tab closed: its picture goes.
    pub(super) fn preview_forget(&self, id: TabId) {
        self.preview_drop(id);
        if self.preview.shown.get() == Some(id) {
            self.hide_tab_preview();
        }
        if self.preview.after_load.get() == Some(id) {
            self.preview.after_load.set(None);
        }
    }

    /// The active tab `id` is about to be left (a tab switch). 0.25.0: no picture now. A
    /// full-page capture at this moment competed with the new tab's first frame (GPU readback
    /// and encode on every switch); the picture is taken once a tab has been active a moment
    /// ([`Browser::preview_activated`]) and after loads.
    pub(super) fn preview_leaving(&self, _id: TabId) {
        self.hide_tab_preview();
    }

    /// 0.25.0: tab `id` became active: its picture once it has stayed in front for
    /// [`CAPTURE_AFTER_LOAD_MS`] (a later switch moves the timer on to its tab).
    pub(super) fn preview_activated(&self, id: TabId) {
        if !self.previews_on() || self.is_private_tab(id) {
            return;
        }
        self.preview.after_load.set(Some(id));
        if let Some(app) = self.app() {
            app.set_timer(PREVIEW_TIMER, CAPTURE_AFTER_LOAD_MS);
        }
    }

    /// Tab `id` finished loading: if it is the active tab, its picture a little later.
    pub(super) fn preview_loaded(&self, id: TabId) {
        if !self.previews_on() || self.active_id() != Some(id) || self.is_private_tab(id) {
            return;
        }
        self.preview.after_load.set(Some(id));
        if let Some(app) = self.app() {
            app.set_timer(PREVIEW_TIMER, CAPTURE_AFTER_LOAD_MS);
        }
    }

    pub(super) fn preview_timer(&self) {
        if let Some(app) = self.app() {
            app.kill_timer(PREVIEW_TIMER);
        }
        let Some(id) = self.preview.after_load.take() else { return };
        // Still the active tab of its window, and done loading.
        let active =
            self.window_of(id).is_some_and(|w| self.ws.borrow().model(w).and_then(|m| m.active_id()) == Some(id));
        if active && self.tab(id).is_some_and(|t| !t.loading) {
            self.preview_capture(id);
        }
    }

    /// Takes the picture of tab `id`'s page, if it is on screen and has painted.
    fn preview_capture(&self, id: TabId) {
        if !self.previews_on() {
            return;
        }
        if self.is_private_tab(id) {
            self.preview_drop(id);
            return;
        }
        let loaded = self.views.borrow().get(&id).is_some_and(|v| v.loaded_once.get() && v.view.is_visible());
        let blank = self.tab(id).is_none_or(|t| lb_core::pages::is_blank(&t.url));
        let Some(view) = self.tab_view(id).filter(|_| loaded && !blank) else { return };
        let weak = self.me.clone();
        // The whole visible page as it is: no clip and no scale, which would make the engine
        // re-render the page on screen at the small size for a frame (seen on the VM). It is cut
        // and scaled on a job thread instead.
        let params = json!({ "format": "jpeg", "quality": CAPTURE_QUALITY, "optimizeForSpeed": true });
        let sent = crate::perf::now();
        view.devtools_call("Page.captureScreenshot", &params, move |r| {
            if sent != 0 {
                crate::perf::line(&lb_core::perftrace::capture_line(sent, id, crate::perf::ms_since(sent)));
            }
            let Some(b) = weak.upgrade() else { return };
            let Some(Value::String(data)) = r.ok().and_then(|mut v| v.get_mut("data").map(Value::take)) else {
                return warning!("tab {id}: no hover-card picture (the page may be hidden already)");
            };
            let Ok(jpeg) = Base64::decode_vec(&data) else { return };
            let (tw, th) = layout::TAB_PREVIEW_THUMB;
            b.spawn_job(
                "tab-preview",
                move || crate::ffi::image::thumbnail_jpeg(&jpeg, tw as u32, th as u32),
                move |b, small| match small {
                    // A tab closed meanwhile is not cached again.
                    // Nor a private one (0.22.0; checked again: `preview_put` drops it too).
                    Some(small) if !b.is_private_tab(id) && b.previews_on() => {
                        b.preview_put(id, format!("data:image/jpeg;base64,{}", Base64::encode_string(&small)));
                    }
                    Some(_) => {}
                    None => warning!("tab {id}: the hover-card picture could not be scaled"),
                },
            );
        });
    }
}
