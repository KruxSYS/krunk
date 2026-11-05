//! krunk://adblock's "Filter lists" and "My filters": turning lists on and off, adding a list
//! by its address, removing it, and the user's own filters. Every change rebuilds the engine
//! on the filter worker; the page gets its reply once the new engine is in use.

use super::blocking::Owed;
use super::Browser;
use lb_core::blocker;
use lb_core::filterlists::{self, check_user_filters, custom_id, MAX_USER_FILTERS_BYTES, USER_FILTERS_KEY, USER_LIST};
use lb_core::protocol::{Event, Request};
use lb_core::settings::{CustomList, Settings, FILTER_LISTS};
use lb_core::tabs::TabId;
use std::collections::BTreeMap;

impl Browser {
    pub(super) fn dispatch_filter_lists(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::GetFilterLists {} => self.get_filter_lists(tab, req_id),
            Request::SetFilterList { id, enabled } => {
                let builtin = FILTER_LISTS.contains(&id.as_str());
                if !builtin && !self.settings.borrow().custom_lists.iter().any(|l| custom_id(&l.url) == id) {
                    self.reply(tab, Event::err(req_id, "No such list"));
                    return;
                }
                let r = self.try_change_settings(|s| {
                    if builtin {
                        s.filter_lists.retain(|l| *l != id);
                        if enabled {
                            s.filter_lists.push(id.clone());
                            // In their usual order.
                            s.filter_lists.sort_by_key(|l| FILTER_LISTS.iter().position(|f| f == l));
                        }
                    } else if let Some(l) = s.custom_lists.iter_mut().find(|l| custom_id(&l.url) == id) {
                        l.enabled = enabled;
                    }
                });
                self.after_filter_change(tab, req_id, r, Owed::Status);
            }
            Request::AddFilterList { url } => {
                let url = url.trim().to_string();
                if self.settings.borrow().custom_lists.iter().any(|l| l.url == url) {
                    self.reply(tab, Event::err(req_id, "This list is already there"));
                    return;
                }
                let r =
                    self.try_change_settings(|s| s.custom_lists.push(CustomList { url: url.clone(), enabled: true }));
                // With ad blocking off it is downloaded when it is turned on.
                let owed = if self.net.enabled() { Owed::Added(custom_id(&url)) } else { Owed::Status };
                self.after_filter_change(tab, req_id, r, owed);
            }
            Request::RemoveFilterList { id } => {
                if !self.settings.borrow().custom_lists.iter().any(|l| custom_id(&l.url) == id) {
                    self.reply(tab, Event::err(req_id, "Only lists you added can be removed"));
                    return;
                }
                let r = self.try_change_settings(|s| s.custom_lists.retain(|l| custom_id(&l.url) != id));
                self.after_filter_change(tab, req_id, r, Owed::Status);
            }
            Request::RestoreDefaultLists {} => {
                let r = self.try_change_settings(|s| s.filter_lists = Settings::default().filter_lists);
                self.after_filter_change(tab, req_id, r, Owed::Status);
            }
            Request::GetUserFilters {} => {
                let text = self.store.borrow().kv_get(USER_FILTERS_KEY).ok().flatten().unwrap_or_default();
                let report = check_user_filters(&text);
                self.reply(tab, Event::ok(req_id, serde_json::json!({ "text": text, "report": report })));
            }
            Request::SaveUserFilters { text } => {
                if text.len() > MAX_USER_FILTERS_BYTES {
                    self.reply(tab, Event::err(req_id, "Too long (at most 200 KB)"));
                    return;
                }
                let text = text.replace("\r\n", "\n");
                if let Err(e) = self.store.borrow().kv_set(USER_FILTERS_KEY, &text) {
                    crate::log::warning!("saving my filters: {e}");
                    self.reply(tab, Event::err(req_id, "Could not save the filters"));
                    return;
                }
                let report = check_user_filters(&text);
                let changed = *self.filters.user.borrow() != text;
                *self.filters.user.borrow_mut() = text;
                if changed && self.net.enabled() && self.net.has_engine() {
                    self.spawn_filter_job(super::blocking::Fetching::Nothing);
                }
                self.after_filter_change(tab, req_id, Ok(()), Owed::Report(report));
            }
            _ => {}
        }
    }

    /// Replies once the rebuild a change started is done (at once if none runs).
    fn after_filter_change(&self, tab: Option<TabId>, req_id: Option<u32>, r: Result<(), String>, owed: Owed) {
        if let Err(e) = r {
            self.reply(tab, Event::err(req_id, &e));
            return;
        }
        self.filters.pending_replies.borrow_mut().push((tab, req_id, owed));
        if !self.filters.busy.get() {
            self.answer_filter_replies();
        }
    }

    /// Every list with its rule count (the lists are read on a worker thread).
    fn get_filter_lists(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let all = self.filter_sources().every_list();
        let dir = self.dirs.filters();
        let state = self.filters.update.borrow().clone();
        self.spawn_job(
            "filter-list-info",
            move || -> BTreeMap<String, usize> {
                filterlists::current_texts(&dir, &all.sources(), &state)
                    .into_iter()
                    .map(|(id, text)| (id, filterlists::rule_count(&text)))
                    .collect()
            },
            move |b, rules| b.reply(tab, Event::ok(req_id, b.filter_status_with(&rules))),
        );
    }

    /// The name of a list for the blocked-requests page.
    pub(super) fn list_name(&self, id: &str) -> String {
        if id == USER_LIST {
            return "My filters".into();
        }
        if id.starts_with("custom-") {
            let state = self.filters.update.borrow();
            if let Some(t) = state.downloaded.get(id).and_then(|d| d.title.clone()) {
                return t;
            }
            return self
                .settings
                .borrow()
                .custom_lists
                .iter()
                .find(|l| custom_id(&l.url) == id)
                .map_or_else(|| "a list you added".into(), |l| l.url.clone());
        }
        blocker::list_title(id).to_string()
    }
}
