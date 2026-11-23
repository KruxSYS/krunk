//! Settings → Account (0.8.0, sync milestone S3; the rules and texts are in `lb_core::account`).
//!
//! Nothing here runs on its own: a request goes to the server only after **Sign in**, while the
//! card is on screen for a signed-in device (at most every 10 minutes), and on **Sign out**.
//! Network calls run on worker threads (`spawn_job`); the device code and the access token stay
//! in memory, the refresh token is kept DPAPI-wrapped in `account.json`. No code, token or
//! address is ever logged.

use super::Browser;
use crate::ffi::http::{self, ProxyMode};
use crate::ffi::secure::DpapiAccount;
use crate::log::{info, warning};
use lb_core::account::{self as acct, AccountFile, Answer, CardState, Failure, Linking, Next, Refreshed};
use lb_core::pages::Page;
use lb_core::protocol::Event;
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::vault::KeyWrapper;
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

const TIMEOUT_MS: i32 = 15_000;
/// A signed-in card asks the server again at most this often.
const CHECK_EVERY_MS: i64 = 10 * 60 * 1000;
/// The access token is renewed this long before it runs out.
const ACCESS_MARGIN_MS: i64 = 60 * 1000;
const SIGNED_OUT_OFFLINE: &str =
    "Signed out on this device. Krunk couldn't reach krunk.kruxsys.com, so the device may \
     still be listed on your account page: you can remove it there.";
const SOCKS_TEXT: &str = "Krunk can't sign in through a SOCKS proxy. Change the proxy in Settings → Advanced.";

/// Where the device stands.
enum Stage {
    SignedOut(Option<String>),
    Working(String),
    Linking(Linking),
    SignedIn { file: AccountFile, message: Option<String> },
}

/// Who waits for an access token.
type TokenWaiter = Box<dyn FnOnce(&Browser, Result<Secret, Failure>)>;

pub(super) struct AccountHost {
    stage: RefCell<Option<Stage>>,
    /// The access token and when it expires (ms).
    access: RefCell<Option<(Secret, i64)>>,
    /// A token refresh is running; these wait for its result (0.9.0: the card and sync share
    /// one refresh at a time, since presenting a rotated refresh token twice revokes the device).
    refreshing: Cell<bool>,
    waiting: RefCell<Vec<TokenWaiter>>,
    /// Bumped by every new action, so answers to an abandoned one are ignored.
    generation: Cell<u32>,
    last_check_ms: Cell<i64>,
    /// 0.24.0: when this Krunk signs its own website in (one per run: a sign-out on the site
    /// holds until restart).
    auto_sign_in: RefCell<acct::AutoSignIn>,
}

impl Default for AccountHost {
    fn default() -> Self {
        AccountHost {
            stage: RefCell::new(None),
            access: RefCell::new(None),
            refreshing: Cell::new(false),
            waiting: RefCell::new(Vec::new()),
            generation: Cell::new(0),
            last_check_ms: Cell::new(0),
            auto_sign_in: RefCell::default(),
        }
    }
}

/// The site of this build (`build.rs`: https://krunk.kruxsys.com in a release).
fn base() -> &'static str {
    env!("KRUNK_UPDATE_BASE_URL")
}

/// One API call on a worker thread.
fn call(proxy: &ProxyMode, method: &str, path: &str, bearer: Option<&Secret>, body: Option<&str>) -> Answer {
    let agent = format!("Krunk/{}", env!("CARGO_PKG_VERSION"));
    let opts =
        http::Options { proxy, timeout_ms: TIMEOUT_MS, user_agent: &agent, allow_http: cfg!(feature = "dev-updates") };
    let auth = bearer.map(|t| Zeroizing::new(format!("Bearer {}", &**t)));
    let mut headers = vec![("Content-Type", "application/json"), ("Accept", "application/json")];
    if let Some(a) = &auth {
        headers.push(("Authorization", a.as_str()));
    }
    http::request(method, &acct::endpoint(base(), path), &headers, body.map(str::as_bytes), acct::MAX_RESPONSE, &opts)
        .map(|r| (r.status, r.body))
}

/// A refresh (rotation) with the stored token: the new tokens.
fn refresh(proxy: &ProxyMode, file: &AccountFile) -> Result<Refreshed, Failure> {
    let wrapped = file.wrapped_refresh().ok_or(Failure::SignedOut)?;
    let token = DpapiAccount.unprotect(&wrapped).map_err(|_| Failure::SignedOut)?;
    let token = Secret::new(String::from_utf8_lossy(&token).into_owned());
    let body = Zeroizing::new(serde_json::json!({ "refresh_token": &*token }).to_string());
    acct::parse(&call(proxy, "POST", acct::REFRESH, None, Some(&body)))
}

fn wrap(token: &Secret) -> Result<Vec<u8>, String> {
    DpapiAccount.protect(token.as_bytes())
}

impl Browser {
    fn account_proxy(&self) -> Option<ProxyMode> {
        self.own_route()
    }

    /// The stage, read from `account.json` the first time (no network).
    fn with_account_stage<R>(&self, f: impl FnOnce(&mut Stage) -> R) -> R {
        let mut s = self.account.stage.borrow_mut();
        let stage = s.get_or_insert_with(|| {
            match std::fs::read(self.dirs.account()).ok().and_then(|b| AccountFile::parse(&b)) {
                Some(file) => Stage::SignedIn { file, message: None },
                None => Stage::SignedOut(None),
            }
        });
        f(stage)
    }

    fn set_account_stage(&self, stage: Stage) {
        *self.account.stage.borrow_mut() = Some(stage);
        self.push_account();
    }

    fn next_generation(&self) -> u32 {
        let g = self.account.generation.get().wrapping_add(1);
        self.account.generation.set(g);
        g
    }

    fn card(&self) -> CardState {
        self.with_account_stage(|s| match s {
            Stage::SignedOut(m) => CardState::SignedOut { message: m.clone() },
            Stage::Working(m) => CardState::Working { message: m.clone() },
            Stage::Linking(l) => CardState::Linking {
                user_code: l.user_code.clone(),
                verify_url: l.verify_url.clone(),
                expires_at_ms: l.expires_at_ms,
                qr: l.qr.clone(),
            },
            Stage::SignedIn { file, message } => CardState::SignedIn {
                email: file.email.clone(),
                device_name: acct::display_label(&file.label),
                version: env!("CARGO_PKG_VERSION").to_string(),
                message: message.clone(),
            },
        })
    }

    fn tab_is_private(&self, id: TabId) -> bool {
        self.tab(id).is_some_and(|t| t.private)
    }

    /// Every Settings page gets the card (a private one gets nothing of the account).
    fn push_account(&self) {
        let card = self.card();
        let tabs: Vec<(TabId, bool)> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| Page::from_url(&t.url) == Some(Page::Settings))
            .map(|(_, t)| (t.id, t.private))
            .collect();
        for (id, private) in tabs {
            let c = if private { CardState::Private } else { card.clone() };
            if let Some(v) = self.tab_view(id) {
                v.post_json(&Event::Account { card: c }.to_json());
            }
        }
    }

    /// The ⋮ menu's account item: none in a private window, "" when signed out, else the address.
    pub(super) fn account_menu_label(&self) -> Option<String> {
        if self.active_tab().is_some_and(|t| t.private) {
            return None;
        }
        Some(self.with_account_stage(|s| match s {
            Stage::SignedIn { file, .. } => file.email.clone(),
            _ => String::new(),
        }))
    }

    pub(super) fn dispatch_account(&self, tab: Option<TabId>, request: lb_core::protocol::Request) {
        use lb_core::protocol::Request as R;
        let Some(tab) = tab else { return };
        if self.tab_is_private(tab) {
            if let Some(v) = self.tab_view(tab) {
                v.post_json(&Event::Account { card: CardState::Private }.to_json());
            }
            return;
        }
        match request {
            R::AccountState { check } => {
                if let Some(v) = self.tab_view(tab) {
                    v.post_json(&Event::Account { card: self.card() }.to_json());
                }
                let signed_in = self.with_account_stage(|s| matches!(s, Stage::SignedIn { .. }));
                if check && signed_in && now_ms() - self.account.last_check_ms.get() > CHECK_EVERY_MS {
                    self.account_check();
                }
            }
            R::AccountSignIn => self.account_sign_in(),
            R::AccountCancel => {
                if self.with_account_stage(|s| matches!(s, Stage::Linking(_))) {
                    self.next_generation();
                    info!("account: sign-in cancelled");
                    self.set_account_stage(Stage::SignedOut(None));
                }
            }
            R::AccountSignOut => self.account_sign_out(),
            R::AccountManage => {
                // 0.9.0: the website marks this device's row ("This device"); the id isn't secret.
                let mut url = acct::endpoint(base(), acct::ACCOUNT_PAGE);
                if let Some((_, device, _)) = self.account_ids() {
                    url.push_str(&format!("?this={device}"));
                }
                self.new_tab(Some(&url), false)
            }
            // 0.13.0: the website's sign-in QR for a phone.
            R::AccountAddDevice => {
                let url = format!("{}#add-device", acct::endpoint(base(), acct::ACCOUNT_PAGE));
                self.new_tab(Some(&url), false)
            }
            R::AccountReopen => {
                if let Some(url) =
                    self.with_account_stage(
                        |s| if let Stage::Linking(l) = s { Some(l.verify_url.clone()) } else { None },
                    )
                {
                    self.new_tab(Some(&url), false);
                }
            }
            _ => {}
        }
    }

    fn account_sign_in(&self) {
        if !self.with_account_stage(|s| matches!(s, Stage::SignedOut(_))) {
            return;
        }
        let Some(proxy) = self.account_proxy() else {
            self.set_account_stage(Stage::SignedOut(Some(SOCKS_TEXT.into())));
            return;
        };
        let gen = self.next_generation();
        info!("account: sign-in started");
        self.set_account_stage(Stage::Working("Asking krunk.kruxsys.com for a code…".into()));
        let label = acct::device_label(crate::ffi::window::windows_build());
        let body = acct::authorize_body(&label, env!("CARGO_PKG_VERSION"));
        self.spawn_job(
            "account-authorize",
            move || call(&proxy, "POST", acct::AUTHORIZE, None, Some(&body)),
            move |b, answer: Answer| {
                if b.account.generation.get() != gen {
                    return;
                }
                let linking = acct::parse::<acct::Authorized>(&answer).and_then(|a| {
                    // A test build (http allowed) talks to a staging stack with its own site name.
                    let site = (!cfg!(feature = "dev-updates")).then(base);
                    Linking::start(a, site, now_ms())
                });
                match linking {
                    Ok(l) => {
                        let (url, wait) = (l.verify_url.clone(), l.interval_s * 1000);
                        b.set_account_stage(Stage::Linking(l));
                        b.new_tab(Some(&url), false);
                        b.account_poll_later(gen, wait);
                    }
                    Err(f) => {
                        info!("account: sign-in could not start ({f:?})");
                        b.set_account_stage(Stage::SignedOut(Some(f.text().into())));
                    }
                }
            },
        );
    }

    fn account_poll_later(&self, gen: u32, wait_ms: u64) {
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(wait_ms as i64, move || {
            if let Some(b) = weak.upgrade() {
                b.account_poll(gen);
            }
        });
    }

    fn account_poll(&self, gen: u32) {
        if self.account.generation.get() != gen {
            return;
        }
        let Some(body) =
            self.with_account_stage(|s| if let Stage::Linking(l) = s { Some(l.poll_body()) } else { None })
        else {
            return;
        };
        let Some(proxy) = self.account_proxy() else { return };
        let body = Zeroizing::new(body);
        self.spawn_job(
            "account-poll",
            move || call(&proxy, "POST", acct::TOKEN, None, Some(&body)),
            move |b, answer: Answer| {
                if b.account.generation.get() != gen {
                    return;
                }
                let poll = acct::poll_answer(&answer);
                let next = b.with_account_stage(|s| {
                    if let Stage::Linking(l) = s {
                        let before = l.expires_at_ms;
                        Some((l.next(poll, now_ms()), l.expires_at_ms != before))
                    } else {
                        None
                    }
                });
                // The server keeps the code waiting past its 10 minutes (a registration from the
                // /link tab): the card gets the new deadline, or its countdown would sit at 0:00.
                let next = next.map(|(n, moved)| {
                    if moved {
                        b.push_account();
                    }
                    n
                });
                match next {
                    Some(Next::Poll(ms)) => b.account_poll_later(gen, ms),
                    Some(Next::SignedIn(t)) => b.account_finish(gen, t),
                    Some(Next::Stop(text)) => {
                        info!("account: sign-in ended without an account");
                        b.set_account_stage(Stage::SignedOut(Some(text.into())));
                    }
                    None => {}
                }
            },
        );
    }

    /// Tokens arrived: ask who this is, keep the refresh token wrapped, show the account.
    fn account_finish(&self, gen: u32, t: acct::Tokens) {
        let Some(proxy) = self.account_proxy() else { return };
        self.set_account_stage(Stage::Working("Signing in…".into()));
        let access = t.access_token.clone();
        self.spawn_job(
            "account-me",
            move || {
                let me = acct::parse::<acct::DeviceMe>(&call(&proxy, "GET", acct::ME, Some(&t.access_token), None));
                let wrapped = wrap(&t.refresh_token);
                (me, wrapped)
            },
            move |b, (me, wrapped): (Result<acct::DeviceMe, Failure>, Result<Vec<u8>, String>)| {
                if b.account.generation.get() != gen {
                    return;
                }
                match (me, wrapped) {
                    (Ok(me), Ok(wrapped)) => {
                        let file =
                            AccountFile::new(&me.email, &me.label, &me.account_id, &me.device_id, &wrapped, now_ms());
                        if let Err(e) = lb_core::vault::write_atomic(&b.dirs.account(), &file.to_bytes()) {
                            warning!("account: account.json could not be written: {e}");
                        }
                        *b.account.access.borrow_mut() = Some((access, now_ms() + 900_000));
                        b.account.last_check_ms.set(now_ms());
                        info!("account: signed in");
                        b.set_account_stage(Stage::SignedIn { file, message: None });
                    }
                    (Err(f), _) => {
                        info!("account: signed in, but the account could not be read ({f:?})");
                        b.set_account_stage(Stage::SignedOut(Some(f.text().into())));
                    }
                    (_, Err(e)) => {
                        warning!("account: the sign-in could not be kept: {e}");
                        b.set_account_stage(Stage::SignedOut(Some(
                            "Krunk couldn't keep the sign-in on this computer.".into(),
                        )));
                    }
                }
            },
        );
    }

    /// The card is on screen for a signed-in device: rotate the refresh token and ask who this
    /// is (the name may have changed on the website, or the device was removed there).
    fn account_check(&self) {
        if !self.with_account_stage(|s| matches!(s, Stage::SignedIn { .. })) {
            return;
        }
        let Some(proxy) = self.account_proxy() else { return };
        self.account.last_check_ms.set(now_ms());
        let gen = self.account.generation.get();
        self.refresh_access(true, move |b, token| {
            let Ok(access) = token else { return };
            if b.account.generation.get() != gen {
                return;
            }
            b.spawn_job(
                "account-me",
                move || acct::parse::<acct::DeviceMe>(&call(&proxy, "GET", acct::ME, Some(&access), None)),
                move |b, me: Result<acct::DeviceMe, Failure>| {
                    let Ok(me) = me else { return };
                    let Some(mut file) = b.with_account_stage(|s| {
                        if let Stage::SignedIn { file, .. } = s {
                            Some(file.clone())
                        } else {
                            None
                        }
                    }) else {
                        return;
                    };
                    if file.email != me.email || file.label != me.label {
                        file.email = me.email;
                        file.label = me.label;
                        if let Err(e) = lb_core::vault::write_atomic(&b.dirs.account(), &file.to_bytes()) {
                            warning!("account: account.json could not be written: {e}");
                        }
                        b.set_account_stage(Stage::SignedIn { file, message: None });
                    }
                },
            );
        });
    }

    /// A valid access token for `f` (0.9.0: the card and sync). A cached one is used while it
    /// has more than a minute left (unless `force`); otherwise one refresh runs (the refresh
    /// token rotates and `account.json` is rewritten) and everyone waiting gets its result.
    /// "Signed out" (the device was removed) signs this device out.
    pub(super) fn refresh_access(&self, force: bool, f: impl FnOnce(&Browser, Result<Secret, Failure>) + 'static) {
        let cached = self.account.access.borrow().clone().filter(|(_, exp)| *exp - ACCESS_MARGIN_MS > now_ms());
        if let (false, Some((t, _))) = (force, cached) {
            f(self, Ok(t));
            return;
        }
        let Some(file) =
            self.with_account_stage(|s| if let Stage::SignedIn { file, .. } = s { Some(file.clone()) } else { None })
        else {
            f(self, Err(Failure::SignedOut));
            return;
        };
        let Some(proxy) = self.account_proxy() else {
            f(self, Err(Failure::Offline));
            return;
        };
        self.account.waiting.borrow_mut().push(Box::new(f));
        if self.account.refreshing.replace(true) {
            return;
        }
        self.spawn_job(
            "account-refresh",
            move || {
                let r = refresh(&proxy, &file)?;
                let wrapped = wrap(&r.refresh_token).map_err(|_| Failure::Server)?;
                Ok::<_, Failure>((wrapped, r.access_token))
            },
            move |b, r: Result<(Vec<u8>, Secret), Failure>| {
                b.account.refreshing.set(false);
                let result = match r {
                    Ok((wrapped, access)) => {
                        if let Some(mut file) = b.with_account_stage(|s| {
                            if let Stage::SignedIn { file, .. } = s {
                                Some(file.clone())
                            } else {
                                None
                            }
                        }) {
                            file.set_wrapped_refresh(&wrapped);
                            if let Err(e) = lb_core::vault::write_atomic(&b.dirs.account(), &file.to_bytes()) {
                                warning!("account: account.json could not be written: {e}");
                            }
                            b.set_account_stage(Stage::SignedIn { file, message: None });
                        }
                        *b.account.access.borrow_mut() = Some((access.clone(), now_ms() + 900_000));
                        Ok(access)
                    }
                    Err(Failure::SignedOut) => {
                        info!("account: the server says this device is signed out");
                        let _ = std::fs::remove_file(b.dirs.account());
                        *b.account.access.borrow_mut() = None;
                        b.set_account_stage(Stage::SignedOut(Some(Failure::SignedOut.text().into())));
                        b.sync_signed_out();
                        Err(Failure::SignedOut)
                    }
                    Err(f) => {
                        info!("account: token refresh skipped ({f:?})");
                        Err(f)
                    }
                };
                let waiting = std::mem::take(&mut *b.account.waiting.borrow_mut());
                for w in waiting {
                    w(b, result.clone());
                }
            },
        );
    }

    /// 0.24.0: a top-level navigation of tab `id` to `url` is about to start. When this Krunk,
    /// signed in to the account, should sign its own website in (the rules are
    /// `acct::AutoSignIn`'s), this returns true: the caller cancels the navigation and the tab
    /// goes to the sign-in page with a single-use code instead. Whatever fails on the way opens
    /// `url` as it was (the 30 s guard lets that through, so the page shows its form). The code
    /// and the address with it are never logged.
    pub(super) fn auto_sign_in_navigation(&self, id: TabId, url: &str) -> bool {
        // Every navigation of every tab comes here: only the sign-in page is worth a look.
        if !url.contains(acct::LOGIN_PAGE) {
            return false;
        }
        // A tab that can't be looked up counts as private.
        let private = self.tab(id).is_none_or(|t| t.private);
        let signed_in = self.with_account_stage(|s| matches!(s, Stage::SignedIn { .. }));
        let Some(next) = self.account.auto_sign_in.borrow_mut().check(url, base(), now_ms(), signed_in, private) else {
            return false;
        };
        let Some(proxy) = self.account_proxy() else { return false };
        info!("tab {id}: signing the website in with this Krunk's account");
        let original = url.to_string();
        // Out of the engine's callback: the token refresh may answer at once.
        self.defer(move |b| {
            b.refresh_access(false, move |b, token| {
                let token = match token {
                    Ok(t) => t,
                    Err(f) => {
                        info!("tab {id}: website sign-in skipped ({f:?})");
                        b.navigate_tab(id, &original);
                        return;
                    }
                };
                b.spawn_job(
                    "account-web-sign-in",
                    move || {
                        acct::parse::<acct::WebSignInCode>(&call(
                            &proxy,
                            "POST",
                            acct::WEB_SIGN_IN,
                            Some(&token),
                            Some("{}"),
                        ))
                    },
                    move |b, r: Result<acct::WebSignInCode, Failure>| match r {
                        Ok(c) => {
                            let url = Zeroizing::new(acct::auto_sign_in_url(base(), &c.code, next.as_deref()));
                            b.navigate_tab(id, &url);
                        }
                        Err(f) => {
                            info!("tab {id}: website sign-in skipped ({f:?})");
                            b.navigate_tab(id, &original);
                        }
                    },
                );
            });
        });
        true
    }

    /// The account and device ids of a signed-in device (sync needs them; no network).
    pub(super) fn account_ids(&self) -> Option<(String, String, String)> {
        self.with_account_stage(|s| match s {
            Stage::SignedIn { file, .. } => Some((file.account_id.clone(), file.device_id.clone(), file.email.clone())),
            _ => None,
        })
    }

    /// The account and its proxy setting, for sync's own calls.
    pub(super) fn sync_proxy(&self) -> Option<ProxyMode> {
        self.account_proxy()
    }

    /// Sign out on this device: the local sign-in is deleted at once, then the server is told
    /// (with a fresh access token if needed). If that fails, the device stays listed on the
    /// account page, and the card says so.
    fn account_sign_out(&self) {
        let Some(file) =
            self.with_account_stage(|s| if let Stage::SignedIn { file, .. } = s { Some(file.clone()) } else { None })
        else {
            return;
        };
        let gen = self.next_generation();
        let access = self.account.access.borrow_mut().take().filter(|(_, exp)| *exp - ACCESS_MARGIN_MS > now_ms());
        if let Err(e) = std::fs::remove_file(self.dirs.account()) {
            warning!("account: account.json could not be deleted: {e}");
        }
        info!("account: signing out");
        self.set_account_stage(Stage::Working("Signing out…".into()));
        let proxy = self.account_proxy();
        self.spawn_job(
            "account-sign-out",
            move || {
                let proxy = proxy.ok_or(Failure::Offline)?;
                let token = match access {
                    Some((t, _)) => t,
                    None => refresh(&proxy, &file)?.access_token,
                };
                match call(&proxy, "POST", acct::UNLINK, Some(&token), Some("{}")) {
                    Ok((204, _)) => Ok(()),
                    other => Err(acct::failure(&other)),
                }
            },
            move |b, r: Result<(), Failure>| {
                if b.account.generation.get() != gen {
                    return;
                }
                let message = match r {
                    Ok(()) => {
                        info!("account: signed out (the server revoked this device)");
                        None
                    }
                    // Already signed out on the server (removed on the website): nothing left to do.
                    Err(Failure::SignedOut) => {
                        info!("account: signed out (the device was already signed out on the server)");
                        None
                    }
                    Err(f) => {
                        info!("account: signed out on this device only ({f:?})");
                        Some(SIGNED_OUT_OFFLINE.to_string())
                    }
                };
                b.set_account_stage(Stage::SignedOut(message));
            },
        );
    }
}
