//! `krunk://storage` (cookies and site data of all sites, default profile)
//! and `krunk://sitedata` (one site's cookies, local and session storage).
//!
//! The site-data page is opened from the shield panel for the active tab.
//! The host binds it to that tab, its origin and its profile when it opens
//! the page; the page's requests name none of them. A private tab's page
//! opens as a private tab and only ever touches the private profile.

use super::Browser;
use crate::ffi::cef::{delete_cookies, list_cookies, set_cookie_value, Profile, View};
use crate::log::{info, warning};
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::sitedata::{
    self, cookies_for_origin, cookies_of_site, dom_storage_id, dom_storage_items, is_site, is_web_origin, summarize,
    web_origin, CookieSelection, OriginUsage, StorageArea, MAX_KEY_LEN, MAX_USAGE_ORIGINS, MAX_VALUE_LEN,
};
use lb_core::tabs::{Position, TabId};
use std::cell::RefCell;
use std::rc::Rc;

/// What a `krunk://sitedata` tab was opened for.
#[derive(Debug, Clone)]
pub(super) struct SiteDataBinding {
    /// The tab whose site this is (its DevTools session reaches the site's
    /// DOM storage).
    source: TabId,
    origin: String,
    private: bool,
}

impl SiteDataBinding {
    fn profile(&self) -> Profile {
        if self.private {
            Profile::Private
        } else {
            Profile::Default
        }
    }
}

impl Browser {
    // ------------------------------------------------------------ krunk://storage

    pub(super) fn dispatch_storage(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let weak = self.me.clone();
        let reply = move |event: Event| {
            if let Some(b) = weak.upgrade() {
                b.reply(tab, event);
            }
        };
        match request {
            Request::StorageList => {
                let mut origins = self.store.borrow().visited_origins(None).unwrap_or_default();
                for (_, t) in self.ws.borrow().tabs() {
                    if let Some(o) = web_origin(&t.url).filter(|_| !t.private) {
                        origins.push(o);
                    }
                }
                origins.sort();
                origins.dedup();
                list_cookies(Profile::Default, move |cookies| {
                    reply(Event::ok(req_id, serde_json::json!({ "sites": summarize(&cookies, &origins) })));
                });
            }
            Request::StorageUsage { origins } => {
                let origins: Vec<String> =
                    origins.into_iter().filter(|o| is_web_origin(o)).take(MAX_USAGE_ORIGINS).collect();
                // The shell uses the default profile: its DevTools session
                // reads the profile's storage quota for any origin.
                let Some(shell) = self.shell() else { return reply(Event::err(req_id, "Not ready")) };
                usage_of(&shell, origins, move |usage| {
                    reply(Event::ok(req_id, serde_json::json!({ "usage": usage })));
                });
            }
            Request::StorageCookies { site } => {
                if !is_site(&site) {
                    return reply(Event::err(req_id, "Not a site"));
                }
                list_cookies(Profile::Default, move |cookies| {
                    reply(Event::ok(req_id, serde_json::json!({ "cookies": cookies_of_site(&cookies, &site) })));
                });
            }
            Request::StorageDeleteCookie { cookie } => {
                if !cookie.is_valid() {
                    return reply(Event::err(req_id, "Not a cookie"));
                }
                let which = CookieSelection::One(cookie);
                delete_cookies(
                    Profile::Default,
                    move |c| which.matches(c),
                    move |gone| {
                        reply(Event::ok(req_id, serde_json::json!({ "deleted": gone.len() })));
                    },
                );
            }
            Request::StorageDeleteSite { site } => {
                if !is_site(&site) {
                    return reply(Event::err(req_id, "Not a site"));
                }
                let which = CookieSelection::Site(site.clone());
                delete_cookies(
                    Profile::Default,
                    move |c| which.matches(c),
                    move |gone| {
                        info!("storage page: deleted {} cookies of one site", gone.len());
                        reply(Event::ok(req_id, serde_json::json!({ "deleted": gone.len() })));
                    },
                );
            }
            Request::StorageDeleteAllCookies => {
                delete_cookies(
                    Profile::Default,
                    |_| true,
                    move |gone| {
                        info!("storage page: deleted all {} cookies", gone.len());
                        reply(Event::ok(req_id, serde_json::json!({ "deleted": gone.len() })));
                    },
                );
            }
            Request::StorageClearOrigin { origin } => {
                if !is_web_origin(&origin) {
                    return reply(Event::err(req_id, "Not a web origin"));
                }
                let Some(shell) = self.shell() else { return reply(Event::err(req_id, "Not ready")) };
                shell.devtools_call(
                    "Storage.clearDataForOrigin",
                    &serde_json::json!({ "origin": origin, "storageTypes": "all" }),
                    move |r| {
                        reply(match r {
                            Ok(_) => Event::ok(req_id, ()),
                            Err(e) => {
                                warning!("clearing site data failed: {e}");
                                Event::err(req_id, "Clearing the site data failed")
                            }
                        })
                    },
                );
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ krunk://sitedata

    /// Shield panel → "Cookies and site data": opens the site-data page next
    /// to the active tab, in the same profile, bound to that tab's site.
    pub(super) fn open_site_data(&self) {
        self.hide_popup();
        let Some((source, url, private)) = self.active_tab().map(|t| (t.id, t.url.clone(), t.private)) else {
            return;
        };
        // The shield panel offers this only on web pages.
        let Some(origin) = web_origin(&url) else { return };
        let Some(win) = self.window_of(source) else { return };
        let opened = self.ws.borrow_mut().open(
            win,
            &Page::SiteData.url(),
            private,
            Position::AfterOpener(source),
            true,
            lb_core::storage::now_ms(),
        );
        let Some((id, effects)) = opened else { return };
        self.site_data.borrow_mut().insert(id, SiteDataBinding { source, origin, private });
        self.apply(effects);
    }

    /// The binding of a site-data tab, and whether its source tab still
    /// shows that site (in the same profile).
    fn site_data_binding(&self, tab: Option<TabId>) -> Option<(SiteDataBinding, Option<View>)> {
        let binding = self.site_data.borrow().get(&tab?).cloned()?;
        let live = self.tab(binding.source).is_some_and(|t| {
            t.private == binding.private && web_origin(&t.url).as_deref() == Some(binding.origin.as_str())
        });
        let view = if live { self.tab_view(binding.source) } else { None };
        Some((binding, view))
    }

    pub(super) fn dispatch_site_data(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        let weak = self.me.clone();
        let reply = move |event: Event| {
            if let Some(b) = weak.upgrade() {
                b.reply(tab, event);
            }
        };
        let Some((binding, view)) = self.site_data_binding(tab) else {
            return reply(Event::err(req_id, "Open this page from the shield panel of a site"));
        };
        let origin = binding.origin.clone();
        match request {
            Request::SiteDataGet {} => {
                let live = view.is_some();
                let profile = binding.profile();
                let private = binding.private;
                // Permissions are never stored for private tabs.
                let permissions = if private {
                    serde_json::json!({ "localNetwork": null })
                } else {
                    self.site_permissions_json(&origin)
                };
                let view2 = view.clone();
                list_cookies(profile, move |cookies| {
                    let cookies = cookies_for_origin(&cookies, &origin);
                    let base = serde_json::json!({
                        "origin": origin, "private": private, "live": live, "cookies": cookies,
                        "local": [], "session": [], "permissions": permissions,
                    });
                    let Some(v) = view2 else { return reply(Event::ok(req_id, base)) };
                    dom_storage_both(&v, &origin, move |local, session| {
                        let mut data = base;
                        data["local"] = serde_json::json!(local);
                        data["session"] = serde_json::json!(session);
                        reply(Event::ok(req_id, data));
                    });
                });
            }
            Request::SiteDataSet { area, key, value } => {
                let Some(v) = view else { return reply(Event::err(req_id, "The site is no longer open")) };
                if key.is_empty() || key.len() > MAX_KEY_LEN || value.len() > MAX_VALUE_LEN {
                    return reply(Event::err(req_id, "Key or value too long"));
                }
                let params =
                    serde_json::json!({ "storageId": dom_storage_id(&origin, area), "key": key, "value": value });
                dom_storage_call(&v, "DOMStorage.setDOMStorageItem", params, move |r| reply(result_event(req_id, r)));
            }
            Request::SiteDataRemove { area, key } => {
                let Some(v) = view else { return reply(Event::err(req_id, "The site is no longer open")) };
                let params = serde_json::json!({ "storageId": dom_storage_id(&origin, area), "key": key });
                dom_storage_call(&v, "DOMStorage.removeDOMStorageItem", params, move |r| {
                    reply(result_event(req_id, r))
                });
            }
            Request::SiteDataClear { area } => {
                let Some(v) = view else { return reply(Event::err(req_id, "The site is no longer open")) };
                let params = serde_json::json!({ "storageId": dom_storage_id(&origin, area) });
                dom_storage_call(&v, "DOMStorage.clear", params, move |r| reply(result_event(req_id, r)));
            }
            Request::SiteCookieSet { cookie, value } => {
                if !cookie.is_valid() {
                    return reply(Event::err(req_id, "Not a cookie"));
                }
                if let Some(problem) = sitedata::cookie_value_problem(&cookie.name, &value) {
                    return reply(Event::err(req_id, problem));
                }
                let profile = binding.profile();
                list_cookies(profile, move |all| {
                    // Only a cookie this site can see, whatever the page asks for.
                    let one = CookieSelection::One(cookie);
                    let Some(c) = cookies_for_origin(&all, &origin).into_iter().find(|c| one.matches(c)) else {
                        return reply(Event::err(req_id, "The cookie is no longer there"));
                    };
                    set_cookie_value(profile, &c, &value, move |ok| {
                        reply(if ok {
                            Event::ok(req_id, ())
                        } else {
                            Event::err(req_id, "The engine did not accept this value")
                        })
                    });
                });
            }
            Request::SitePermissionRemove { kind } => {
                let removed = self.store.borrow().permission_remove(&origin, &kind).unwrap_or(false);
                if removed {
                    info!("site data: {kind} permission of {origin} removed");
                }
                reply(Event::ok(req_id, removed));
            }
            Request::SiteCookiesDelete { cookie } => {
                if cookie.as_ref().is_some_and(|c| !c.is_valid()) {
                    return reply(Event::err(req_id, "Not a cookie"));
                }
                // Only cookies this site can see, whatever the page asks for.
                let which = move |c: &sitedata::CookieInfo| {
                    !cookies_for_origin(std::slice::from_ref(c), &origin).is_empty()
                        && cookie.as_ref().is_none_or(|k| CookieSelection::One(k.clone()).matches(c))
                };
                delete_cookies(binding.profile(), which, move |gone| {
                    reply(Event::ok(req_id, serde_json::json!({ "deleted": gone.len() })));
                });
            }
            _ => {}
        }
    }
}

fn result_event(req_id: Option<u32>, r: Result<serde_json::Value, String>) -> Event {
    match r {
        Ok(_) => Event::ok(req_id, ()),
        Err(e) => {
            warning!("site data: {e}");
            Event::err(req_id, "The page's storage could not be changed")
        }
    }
}

/// A DOMStorage call on the site's tab. The storage id names the storage
/// key; engines that still want the older `securityOrigin` form get a retry.
fn dom_storage_call(
    view: &View,
    method: &'static str,
    params: serde_json::Value,
    done: impl FnOnce(Result<serde_json::Value, String>) + 'static,
) {
    let v = view.clone();
    view.devtools_call("DOMStorage.enable", &serde_json::json!({}), move |_| {
        let retry = v.clone();
        let mut legacy = params.clone();
        v.devtools_call(method, &params, move |r| match r {
            Err(_) => {
                if let Some(id) = legacy.get_mut("storageId").and_then(|s| s.as_object_mut()) {
                    if let Some(key) = id.remove("storageKey") {
                        let origin = key.as_str().unwrap_or_default().trim_end_matches('/').to_string();
                        id.insert("securityOrigin".into(), serde_json::json!(origin));
                    }
                }
                retry.devtools_call(method, &legacy, done);
            }
            ok => done(ok),
        });
    });
}

/// Local and session storage items of `origin` in the site's tab.
fn dom_storage_both(
    view: &View,
    origin: &str,
    done: impl FnOnce(Vec<(String, String)>, Vec<(String, String)>) + 'static,
) {
    let v = view.clone();
    let origin2 = origin.to_string();
    let get = |area| serde_json::json!({ "storageId": dom_storage_id(&origin2, area) });
    let session_params = get(StorageArea::Session);
    dom_storage_call(view, "DOMStorage.getDOMStorageItems", get(StorageArea::Local), move |local| {
        let local = dom_storage_items(local.as_ref().ok());
        dom_storage_call(&v, "DOMStorage.getDOMStorageItems", session_params, move |session| {
            done(local, dom_storage_items(session.as_ref().ok()));
        });
    });
}

type UsageDone = Rc<RefCell<Option<Box<dyn FnOnce(Vec<OriginUsage>)>>>>;

/// `Storage.getUsageAndQuota` for each origin.
fn usage_of(view: &View, origins: Vec<String>, done: impl FnOnce(Vec<OriginUsage>) + 'static) {
    let results: Rc<RefCell<Vec<OriginUsage>>> = Rc::new(RefCell::new(Vec::new()));
    let pending = Rc::new(std::cell::Cell::new(origins.len()));
    let done: UsageDone = Rc::new(RefCell::new(Some(Box::new(done))));
    if origins.is_empty() {
        if let Some(d) = done.borrow_mut().take() {
            d(Vec::new());
        }
        return;
    }
    for origin in origins {
        let (results, pending, done) = (results.clone(), pending.clone(), done.clone());
        let o = origin.clone();
        view.devtools_call("Storage.getUsageAndQuota", &serde_json::json!({ "origin": origin }), move |r| {
            results.borrow_mut().push(OriginUsage::from_devtools(&o, r.as_ref().ok()));
            pending.set(pending.get() - 1);
            if pending.get() == 0 {
                if let Some(d) = done.borrow_mut().take() {
                    d(std::mem::take(&mut *results.borrow_mut()));
                }
            }
        });
    }
}
