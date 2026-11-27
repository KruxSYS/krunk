//! Casting (0.30.0): the device picker and the toolbar's cast button on top of the engine's
//! Media Router bridge (`lb_core::cast`, scripts/cef-build/patches/krunk-cast-bridge.patch).
//!
//! The picker is one popup for one tab. Opening it sends the engine `open`; the engine answers
//! with `krunk.cast.open` and keeps sending `krunk.cast.model` while devices come and go (the
//! LAN is searched only from the first opening on: the engine waits for a user gesture). Every
//! start is decided here from the host's copy of the engine's model (`cast::plan`), never from
//! what the popup sends. Closing the popup sends `close` (a page's pending request is then
//! rejected, as in Chrome).

use super::*;
use lb_core::cast::{self as c, Command, Model, TabState};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct CastHost {
    /// The engine's latest model per tab (while the picker is open for it).
    models: RefCell<HashMap<TabId, Model>>,
    /// Each tab's session, kept while the picker is closed (the toolbar button).
    states: RefCell<HashMap<TabId, TabState>>,
    /// The tab the open picker is for.
    picker_tab: Cell<Option<TabId>>,
}

impl Browser {
    fn casting_enabled(&self) -> bool {
        self.settings.borrow().casting
    }

    /// The ⋮ menu's and the page menu's "Cast…", and the toolbar button (`rect`).
    pub(super) fn cast_command(&self, rect: Option<PopupRect>) {
        let Some(tab) = self.active_id() else { return };
        self.cast_open(tab, rect);
    }

    /// The page menu's "Cast…" in tab `tab`.
    pub(super) fn cast_open_for(&self, tab: TabId) {
        self.cast_open(tab, None);
    }

    fn cast_open(&self, tab: TabId, rect: Option<PopupRect>) {
        if !self.casting_enabled() {
            info!("cast: turned off in Settings");
            return;
        }
        let Some(view) = self.tab_view(tab) else { return };
        if !view.cast_command(&Command::Open) {
            // The engine refused: casting was turned off and Krunk not restarted yet, or this
            // engine has no cast bridge.
            warning!("cast: the engine did not open the picker");
            self.cast_show(c::View { tab_id: tab, unavailable: true, ..Default::default() }, rect);
            return;
        }
        self.cast.picker_tab.set(Some(tab));
        // Until the engine's first model: "Looking for devices…".
        let model = self.cast.models.borrow().get(&tab).cloned();
        let mut view = match &model {
            Some(m) => c::view(tab, m, self.cast_page_has_video(tab)),
            None => c::View { tab_id: tab, searching: true, ..Default::default() },
        };
        if view.state == TabState::default() {
            view.state = self.cast.states.borrow().get(&tab).cloned().unwrap_or_default();
        }
        self.cast_show(view, rect);
    }

    fn cast_show(&self, view: c::View, rect: Option<PopupRect>) {
        let content = PopupContent::Cast { view };
        match rect {
            Some(r) => self.show_popup(content, Some(r)),
            None => self.show_prompt(content, 0),
        }
    }

    /// The tab's own video list (the Picture-in-Picture tracking): the engine's media session
    /// only knows playing media.
    fn cast_page_has_video(&self, tab: TabId) -> bool {
        self.views.borrow().get(&tab).is_some_and(|v| !v.net.videos.borrow().is_empty())
    }

    /// A message from the engine's cast bridge for tab `tab` (already checked to come from the
    /// browser process).
    pub(super) fn cast_message(&self, tab: TabId, name: &str, json: &str) {
        match name {
            c::MSG_OPEN | c::MSG_MODEL => {
                let Some(model) = c::parse_model(json) else {
                    warning!("cast: unreadable model from the engine");
                    return;
                };
                self.cast_state_changed(tab, model.state.clone());
                self.cast.models.borrow_mut().insert(tab, model.clone());
                if name == c::MSG_OPEN && !self.casting_enabled() {
                    // Turned off since start: the engine still has its router until the restart.
                    if let Some(view) = self.tab_view(tab) {
                        view.cast_command(&Command::Close);
                    }
                    return;
                }
                let view = c::view(tab, &model, self.cast_page_has_video(tab));
                if self.cast_picker_open_for(tab) {
                    self.cast_post(&view);
                } else if name == c::MSG_OPEN {
                    // A page asked (its cast button): the picker comes up for that tab, in its
                    // window, like Chrome's dialog.
                    if self.active_id() == Some(tab) {
                        self.cast.picker_tab.set(Some(tab));
                        self.cast_show(view, None);
                    } else if let Some(view) = self.tab_view(tab) {
                        // Not the active tab of the current window: no picker over another page.
                        view.cast_command(&Command::Close);
                    }
                }
            }
            c::MSG_STATE => {
                if let Some(state) = c::parse_state(json) {
                    if let Some(m) = self.cast.models.borrow_mut().get_mut(&tab) {
                        m.state = state.clone();
                    }
                    self.cast_state_changed(tab, state);
                    if self.cast_picker_open_for(tab) {
                        let model = self.cast.models.borrow().get(&tab).cloned();
                        if let Some(m) = model {
                            self.cast_post(&c::view(tab, &m, self.cast_page_has_video(tab)));
                        }
                    }
                }
            }
            c::MSG_CLOSED => {
                self.cast.models.borrow_mut().remove(&tab);
                if self.cast_picker_open_for(tab) {
                    self.cast.picker_tab.set(None);
                    self.hide_popup();
                }
            }
            _ => {}
        }
    }

    fn cast_picker_open_for(&self, tab: TabId) -> bool {
        self.popup_kind() == Some(PopupKind::Cast) && self.cast.picker_tab.get() == Some(tab)
    }

    /// The open picker's new view: kept as the popup's content (a later re-placement shows the
    /// latest) and posted to the popup document.
    fn cast_post(&self, view: &c::View) {
        let json = Event::Cast { view: view.clone() }.to_json();
        let target = {
            let Ok(mut guard) = self.popup.try_borrow_mut() else { return };
            let Some(p) = guard.as_mut() else { return };
            if !matches!(&p.content, Some(PopupContent::Cast { view: v }) if v.tab_id == view.tab_id) {
                return;
            }
            p.content = Some(PopupContent::Cast { view: view.clone() });
            p.view.clone().filter(|_| p.ready)
        };
        if let Some(p) = target {
            p.post_json(&json);
        }
    }

    fn cast_state_changed(&self, tab: TabId, state: TabState) {
        let changed = {
            let mut states = self.cast.states.borrow_mut();
            let before = states.get(&tab).map(|s| (s.casting, s.sink.clone()));
            let after = (state.casting, state.sink.clone());
            if state.casting {
                states.insert(tab, state);
            } else {
                states.remove(&tab);
            }
            before.unwrap_or((false, String::new())) != after
        };
        if changed {
            self.cast_push_button();
        }
    }

    /// The toolbar's cast button: shown while some tab casts.
    pub(super) fn cast_push_button(&self) {
        self.post_shell(&Event::CastButton { view: self.cast_button_view() });
    }

    fn cast_button_view(&self) -> c::ButtonView {
        let states = self.cast.states.borrow();
        let active = self.active_id();
        let mine = active.and_then(|t| states.get(&t));
        match mine.or_else(|| states.values().next()) {
            Some(s) if self.casting_enabled() => {
                c::ButtonView { show: true, sink: s.sink.clone(), active_tab: mine.is_some() }
            }
            _ => c::ButtonView::default(),
        }
    }

    /// The button's state for a shell that has just loaded.
    pub(super) fn send_cast_button_to(&self, win: WindowId) {
        self.post_shell_to(win, &Event::CastButton { view: self.cast_button_view() });
    }

    /// The toolbar button: the picker for the casting tab when the active one doesn't cast.
    pub(super) fn show_cast_popup(&self, rect: PopupRect) {
        let active = self.active_id();
        let tab = match active {
            Some(t) if self.cast.states.borrow().contains_key(&t) => Some(t),
            _ => active,
        };
        if let Some(tab) = tab {
            self.cast_open(tab, Some(rect));
        }
    }

    /// The picker went away (closed, replaced by another popup).
    pub(super) fn cast_picker_gone(&self, tab: TabId) {
        if self.cast.picker_tab.get() == Some(tab) {
            self.cast.picker_tab.set(None);
        }
        self.cast.models.borrow_mut().remove(&tab);
        if let Some(view) = self.tab_view(tab) {
            view.cast_command(&Command::Close);
        }
    }

    /// The tab closed: its session ends with it (the engine stops mirroring a closed tab).
    pub(super) fn cast_tab_gone(&self, tab: TabId) {
        self.cast.models.borrow_mut().remove(&tab);
        if self.cast.picker_tab.get() == Some(tab) {
            self.cast.picker_tab.set(None);
        }
        if self.cast.states.borrow_mut().remove(&tab).is_some() {
            self.cast_push_button();
        }
    }

    pub(super) fn dispatch_cast(&self, req_id: Option<u32>, request: Request) {
        // The popup browser also shows menus and other panels: only the picker casts.
        let Some(tab) = self.cast.picker_tab.get().filter(|_| self.popup_kind() == Some(PopupKind::Cast)) else {
            warning!("cast: a command from a popup that is not the cast picker was refused");
            self.reply_popup(Event::err(req_id, "Not from this popup"));
            return;
        };
        let Some(view) = self.tab_view(tab) else { return };
        let r: Result<(), String> = match request {
            Request::CastStart { sink_id, source } => {
                let model = self.cast.models.borrow().get(&tab).cloned().unwrap_or_default();
                match model.sinks.iter().find(|s| s.id == sink_id) {
                    None => Err("This device is gone".into()),
                    Some(sink) => match c::plan(source, sink, &model.video) {
                        None => Err(format!("{} can't show this", sink.name)),
                        Some(plan) => {
                            if let Some(n) = &plan.notice {
                                info!("cast: {n}");
                            }
                            let cmd = Command::Start { sink: sink_id.clone(), mode: plan.mode };
                            if view.cast_command(&cmd) {
                                info!("cast: tab {tab} -> {} as {:?} ({source:?})", sink.name, plan.mode);
                                Ok(())
                            } else {
                                Err("Casting could not start".into())
                            }
                        }
                    },
                }
            }
            Request::CastStop { route_id } => {
                if view.cast_command(&Command::Stop { route: route_id }) {
                    Ok(())
                } else {
                    Err("Could not stop".into())
                }
            }
            Request::CastMedia { action, value } => {
                if view.cast_command(&Command::Media { action, value }) {
                    Ok(())
                } else {
                    Err("The device did not take it".into())
                }
            }
            _ => Ok(()),
        };
        self.reply_popup(match r {
            Ok(()) => Event::ok(req_id, serde_json::Value::Null),
            Err(e) => Event::err(req_id, &e),
        });
    }

    fn reply_popup(&self, event: Event) {
        let popup = self.popup.try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.view.clone()));
        if let Some(p) = popup {
            p.post_json(&event.to_json());
        }
    }
}
