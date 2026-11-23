//! The Android sync flows over the core's in-memory server, with a small front for the account
//! endpoints (link, token, refresh, device/me, unlink) the in-memory server doesn't have.

use super::*;
use lb_core::storage::{BOOKMARKS_BAR, OTHER_BOOKMARKS};
use lb_core::sync::memserver::Mem;
use lb_core::vault::{KdfParams, NoWrap};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

const ACC: &str = "0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0";

/// The website + API for the tests. The in-memory server takes the device id as bearer token.
struct Srv {
    mem: Mem,
    /// device code → device id, once "approved on the website".
    approved: RefCell<BTreeMap<String, String>>,
    next_code: Cell<u32>,
    unlinked: RefCell<Vec<String>>,
    /// Requests per path (to check what went out).
    seen: RefCell<Vec<String>>,
}

impl Srv {
    fn new() -> Srv {
        Srv {
            mem: Mem::new(),
            approved: RefCell::new(BTreeMap::new()),
            next_code: Cell::new(0),
            unlinked: RefCell::new(Vec::new()),
            seen: RefCell::new(Vec::new()),
        }
    }
    /// The user approves the latest code on the website for `device`.
    fn approve(&self, device: &str) {
        self.mem.link(device);
        self.approved.borrow_mut().insert(format!("dc{}", self.next_code.get()), device.into());
    }
    fn count(&self, path: &str) -> usize {
        self.seen.borrow().iter().filter(|p| p.as_str() == path).count()
    }
    fn revoked(&self, device: &str) -> bool {
        self.mem.0.borrow().devices.iter().any(|d| d.id == device && d.revoked)
    }
}

fn ok(v: serde_json::Value) -> Result<HttpResponse, String> {
    Ok(HttpResponse { status: 200, body: v.to_string().into_bytes(), ..Default::default() })
}

fn err(status: u16, code: &str) -> Result<HttpResponse, String> {
    Ok(HttpResponse {
        status,
        body: serde_json::json!({ "error": code }).to_string().into_bytes(),
        ..Default::default()
    })
}

impl HttpClient for Srv {
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        self.seen.borrow_mut().push(path.split('?').next().unwrap_or(path).to_string());
        let json: serde_json::Value = body.and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default();
        match (method, path) {
            ("POST", "link/authorize") => {
                assert_eq!(json["platform"], "android");
                self.next_code.set(self.next_code.get() + 1);
                let n = self.next_code.get();
                ok(serde_json::json!({
                    "device_code": format!("dc{n}"), "user_code": "ABCD-EFGH",
                    "verification_uri_complete": "https://krunk.kruxsys.com/link?code=ABCD-EFGH",
                    "expires_in": 600, "interval": 5
                }))
            }
            ("POST", "link/token") => {
                match self.approved.borrow().get(json["device_code"].as_str().unwrap_or_default()) {
                    Some(dev) => ok(serde_json::json!({
                        "device_id": dev, "account_id": ACC, "access_token": dev, "refresh_token": format!("r-{dev}")
                    })),
                    None => err(400, "authorization_pending"),
                }
            }
            ("POST", "token/refresh") => {
                let dev = json["refresh_token"].as_str().unwrap_or_default().trim_start_matches("r-").to_string();
                if self.revoked(&dev) || self.unlinked.borrow().contains(&dev) {
                    err(401, "unauthorized")
                } else {
                    ok(serde_json::json!({ "access_token": dev, "refresh_token": format!("r-{dev}") }))
                }
            }
            ("GET", "device/me") => {
                let dev = bearer.unwrap_or_default();
                ok(
                    serde_json::json!({ "email": "fx@example.com", "account_id": ACC, "device_id": dev, "label": "Krunk on Android 16" }),
                )
            }
            ("POST", "device/unlink") => {
                let dev = bearer.unwrap_or_default().to_string();
                self.mem.revoke(&dev);
                self.unlinked.borrow_mut().push(dev);
                Ok(HttpResponse { status: 204, ..Default::default() })
            }
            // The in-memory server shows a keyset "g" to its key-less engine tests; a real server
            // has none until the first device sets up.
            ("GET", "sync/state") if self.mem.0.borrow().ak_dirs.is_empty() => {
                let r = self.mem.call(method, path, bearer, body)?;
                let mut v: serde_json::Value = serde_json::from_slice(&r.body).unwrap_or_default();
                if r.status == 200 {
                    v["keyset_gen"] = serde_json::Value::Null;
                }
                Ok(HttpResponse { body: v.to_string().into_bytes(), ..r })
            }
            _ => self.mem.call(method, path, bearer, body),
        }
    }
}

struct Phone {
    root: PathBuf,
    engine: Engine,
    vault: Arc<KrunkVault>,
}

impl Phone {
    fn new(name: &str) -> Phone {
        let root = std::env::temp_dir().join(format!("krunk-ffi-sync-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let vault =
            KrunkVault::with_wrapper(root.join("krunk-vault"), Box::new(NoWrap), KdfParams::insecure_for_tests());
        let engine = Engine::new(
            root.join("krunk-sync"),
            root.join("browser.db"),
            Box::new(NoWrap),
            Some(vault.clone()),
            "Krunk on Android 16".into(),
            "0.6.0".into(),
            Some("https://krunk.kruxsys.com".into()),
        );
        Phone { root, engine, vault }
    }
    fn store(&self) -> Store {
        Store::open(&self.root.join("browser.db")).unwrap()
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.root.join("krunk-sync"))
            .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        v.sort();
        v
    }
    fn sign_in(&mut self, srv: &Srv, device: &str) {
        let info = self.engine.link_start(srv, 1).unwrap();
        assert_eq!(info.user_code, "ABCD-EFGH");
        assert!(matches!(self.engine.link_poll(srv, 2).unwrap(), LinkPoll::Wait { .. }));
        srv.approve(device);
        assert_eq!(self.engine.link_poll(srv, 3).unwrap(), LinkPoll::SignedIn { email: "fx@example.com".into() });
        assert_eq!(self.engine.status().stage, SyncStage::Off);
    }
    /// First device: returns the passphrase and the recovery key.
    fn set_up(&mut self, srv: &Srv) -> (String, String) {
        let TurnOn::Setup { secrets } = self.engine.turn_on(srv, now()).unwrap() else { panic!("expected setup") };
        assert!(matches!(self.engine.setup_confirm(srv, "WRONG".into(), now()), Err(SyncFfiError::WrongCode { .. })));
        let group = keys::recovery_key_check_group(&secrets.recovery_key);
        self.engine.setup_confirm(srv, group, now()).unwrap();
        assert_eq!(self.engine.status().stage, SyncStage::On);
        (secrets.passphrase, secrets.recovery_key)
    }
    fn titles(&self) -> Vec<String> {
        let s = self.store();
        let mut t: Vec<String> =
            [BOOKMARKS_BAR, OTHER_BOOKMARKS].iter().flat_map(|r| s.children(*r).unwrap()).map(|b| b.title).collect();
        t.sort();
        t
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn now() -> i64 {
    crate::now_ms()
}

#[test]
fn nothing_goes_out_before_sign_in() {
    let srv = Srv::new();
    let mut p = Phone::new("quiet");
    let st = p.engine.status();
    assert_eq!(st.stage, SyncStage::SignedOut);
    assert!(p.engine.remote_tabs().is_empty());
    p.engine.local_changed(now());
    p.engine.vault_unlocked(now());
    assert!(matches!(p.engine.run(&srv, now()), Err(SyncFfiError::NotNow { .. })));
    assert!(matches!(p.engine.turn_on(&srv, now()), Err(SyncFfiError::SignedOut { .. })));
    assert!(srv.seen.borrow().is_empty(), "requests before sign-in: {:?}", srv.seen.borrow());
    p.engine.link_start(&srv, 1).unwrap();
    assert_eq!(*srv.seen.borrow(), vec!["link/authorize".to_string()]);
}

#[test]
fn set_up_restore_and_sync_both_ways() {
    let srv = Srv::new();
    // "Windows": the first device.
    let mut a = Phone::new("a");
    a.sign_in(&srv, "dev-a");
    a.store().add_bookmark(BOOKMARKS_BAR, "From A", "https://a.example/").unwrap();
    let (passphrase, _rk) = a.set_up(&srv);
    a.engine.run(&srv, now()).unwrap();

    // The phone, with its own bookmark: restore with a wrong passphrase, then the right one.
    let mut b = Phone::new("b");
    b.store().add_bookmark(BOOKMARKS_BAR, "From B", "https://b.example/").unwrap();
    b.sign_in(&srv, "dev-b");
    assert_eq!(b.engine.turn_on(&srv, now()).unwrap(), TurnOn::Join);
    b.engine.restore_begin(&srv, now()).unwrap();
    let wrong = b.engine.restore_try(&srv, "abandon ability able about above absent absorb".into(), false, now());
    assert!(matches!(wrong, Err(SyncFfiError::WrongSecret { .. })), "{wrong:?}");
    assert_eq!(b.engine.status().stage, SyncStage::Join { passwords: false }, "no partial state");
    assert!(!b.files().contains(&"sync.json".to_string()));
    assert_eq!(b.titles(), vec!["From B".to_string()], "local data untouched");
    let stage = b.engine.restore_try(&srv, passphrase, false, now()).unwrap();
    assert!(matches!(stage, SyncStage::RestoreConfirm { ref devices, .. } if devices.len() == 1), "{stage:?}");
    assert_eq!(srv.count("keys/escrow"), 1, "one escrow fetch for both attempts");
    b.engine.restore_confirm(&srv, now()).unwrap();
    assert_eq!(b.engine.status().stage, SyncStage::On);
    assert_eq!(b.files(), vec!["account.json".to_string(), "sync.json".to_string()]);

    // First sync merges: both bookmarks on both sides.
    let r = b.engine.run(&srv, now()).unwrap();
    assert!(r.changed.bookmarks && r.pushed > 0, "{r:?}");
    a.engine.run(&srv, now()).unwrap();
    assert_eq!(a.titles(), vec!["From A".to_string(), "From B".to_string()]);
    assert_eq!(b.titles(), a.titles());

    // Incremental, both ways, and a fresh Engine (an app restart) keeps syncing.
    b.store().add_bookmark(OTHER_BOOKMARKS, "Later on B", "https://later.example/").unwrap();
    b.engine.local_changed(now());
    b.engine.run(&srv, now()).unwrap();
    a.engine.run(&srv, now()).unwrap();
    assert!(a.titles().contains(&"Later on B".to_string()));
    // An app restart: a new Engine over the same files.
    b.engine = Engine::new(
        b.root.join("krunk-sync"),
        b.root.join("browser.db"),
        Box::new(NoWrap),
        Some(b.vault.clone()),
        "Krunk on Android 16".into(),
        "0.6.0".into(),
        None,
    );
    assert_eq!(b.engine.status().stage, SyncStage::On, "after a restart");
    a.store().add_bookmark(BOOKMARKS_BAR, "After restart", "https://r.example/").unwrap();
    a.engine.run(&srv, now()).unwrap();
    b.engine.run(&srv, now()).unwrap();
    assert!(b.titles().contains(&"After restart".to_string()));

    // Settings and open tabs from A.
    let st = a.store();
    let mut s = st.load_settings().unwrap();
    s.homepage = "https://home-from-a.example/".into();
    st.save_settings(&s).unwrap();
    st.kv_set(
        "session",
        r#"{"version":2,"windows":[{"tabs":[{"url":"https://open.example/","title":"Open"},{"url":"javascript:alert(1)","title":"x"}],"active":0}]}"#,
    )
    .unwrap();
    a.engine.run(&srv, now()).unwrap();
    let r = b.engine.run(&srv, now()).unwrap();
    assert!(r.changed.settings, "{r:?}");
    assert_eq!(b.store().load_settings().unwrap().homepage, "https://home-from-a.example/");
    let tabs = b.engine.remote_tabs();
    assert_eq!(tabs.len(), 1);
    assert_eq!(
        tabs[0].tabs,
        vec![RemoteTab { url: "https://open.example/".into(), title: "Open".into() }],
        "web pages only"
    );
}

#[test]
fn a_removed_device_drops_its_keys_and_keeps_its_data() {
    let srv = Srv::new();
    let mut a = Phone::new("rev-a");
    a.sign_in(&srv, "dev-a");
    let (_, rk) = a.set_up(&srv);
    let mut b = Phone::new("rev-b");
    b.sign_in(&srv, "dev-b");
    b.engine.turn_on(&srv, now()).unwrap();
    b.engine.restore_try(&srv, rk, true, now()).unwrap();
    b.engine.restore_confirm(&srv, now()).unwrap();
    b.store().add_bookmark(BOOKMARKS_BAR, "Mine", "https://mine.example/").unwrap();
    b.engine.run(&srv, now()).unwrap();

    srv.mem.revoke("dev-b"); // removed on Windows
    let e = b.engine.run(&srv, now() + 2 * ACCESS_MS); // the access token has run out by then
    assert!(matches!(e, Err(SyncFfiError::Revoked { .. })), "{e:?}");
    let st = b.engine.status();
    assert_eq!(st.stage, SyncStage::SignedOut);
    assert!(st.message.unwrap().contains("removed"));
    assert!(b.files().is_empty(), "account.json and sync.json gone: {:?}", b.files());
    assert_eq!(b.titles(), vec!["Mine".to_string()], "data kept");
    let c = b.store();
    assert_eq!(local::queue_len(c.sync_conn()).unwrap(), 0, "bookkeeping gone");
    assert!(local::meta_get(c.sync_conn(), "track").unwrap().is_none(), "no more tracking");
    let before = srv.seen.borrow().len();
    assert!(b.engine.run(&srv, now()).is_err());
    assert_eq!(srv.seen.borrow().len(), before, "a removed device sends nothing more");
}

#[test]
fn sign_out_keeps_or_removes_what_came_from_sync() {
    for remove in [false, true] {
        let srv = Srv::new();
        let mut a = Phone::new(&format!("so-a-{remove}"));
        a.sign_in(&srv, "dev-a");
        a.store().add_bookmark(BOOKMARKS_BAR, "From A", "https://a.example/").unwrap();
        let (_, rk) = a.set_up(&srv);
        a.engine.run(&srv, now()).unwrap();
        let mut b = Phone::new(&format!("so-b-{remove}"));
        b.store().add_bookmark(BOOKMARKS_BAR, "Own", "https://own.example/").unwrap();
        b.sign_in(&srv, "dev-b");
        b.engine.turn_on(&srv, now()).unwrap();
        b.engine.restore_try(&srv, rk, true, now()).unwrap();
        b.engine.restore_confirm(&srv, now()).unwrap();
        b.engine.run(&srv, now()).unwrap();
        assert_eq!(b.titles(), vec!["From A".to_string(), "Own".to_string()]);
        assert!(b.engine.sign_out(&srv, remove, now()), "the server was told");
        assert!(srv.unlinked.borrow().contains(&"dev-b".to_string()));
        assert!(b.files().is_empty());
        assert_eq!(b.engine.status().stage, SyncStage::SignedOut);
        let want: Vec<String> = if remove { vec!["Own".into()] } else { vec!["From A".into(), "Own".into()] };
        assert_eq!(b.titles(), want, "remove synced data: {remove}");
    }
}

#[test]
fn a_copied_database_without_its_keys_is_forgotten() {
    let p = Phone::new("transfer");
    {
        let mut s = p.store();
        s.sync_set_tracking(true).unwrap();
        s.add_bookmark(BOOKMARKS_BAR, "Copied", "https://c.example/").unwrap();
        assert!(local::queue_len(s.sync_conn()).unwrap() > 0);
        let _ = &mut s;
    }
    let mut p = p;
    assert_eq!(p.engine.status().stage, SyncStage::SignedOut);
    let s = p.store();
    assert!(local::meta_get(s.sync_conn(), "track").unwrap().is_none());
    assert_eq!(local::queue_len(s.sync_conn()).unwrap(), 0);
    assert_eq!(p.titles(), vec!["Copied".to_string()], "the data itself stays");
}

#[test]
fn bad_input_is_refused_before_any_request() {
    let srv = Srv::new();
    let mut p = Phone::new("input");
    p.sign_in(&srv, "dev-p");
    let n = srv.seen.borrow().len();
    assert!(matches!(p.engine.join_code(&srv, "12ab56".into(), now()), Err(SyncFfiError::NotNow { .. })));
    assert!(matches!(p.engine.restore_try(&srv, "x".repeat(300), false, now()), Err(SyncFfiError::WrongSecret { .. })));
    assert!(matches!(p.engine.set_collection("cookies".into(), false, now()), Err(SyncFfiError::NotNow { .. })));
    assert_eq!(srv.seen.borrow().len(), n);
    // A label from Kotlin is cleaned.
    let e = Engine::new(
        p.root.join("x"),
        p.root.join("y.db"),
        Box::new(NoWrap),
        None,
        "A\u{7}B".repeat(40),
        "v".into(),
        None,
    );
    assert_eq!(e.label.chars().count(), MAX_LABEL);
    assert!(!e.label.contains('\u{7}'));
}

#[test]
fn errors_map_to_plain_language() {
    let cases = [
        (SyncFail::Api(ApiError::Offline("dns".into())), "Offline"),
        (SyncFail::Api(ApiError::Unauthorized), "Revoked"),
        (SyncFail::Api(ApiError::RateLimited(120)), "RateLimited"),
        (SyncFail::Keys(KeyError::WrongSecret), "WrongSecret"),
        (SyncFail::Keys(KeyError::SasMismatch), "WrongCode"),
        (SyncFail::Reset, "Reset"),
        (SyncFail::Api(ApiError::Server(503)), "Server"),
    ];
    for (e, kind) in cases {
        let f = sync_err(&e);
        assert!(format!("{f:?}").starts_with(kind), "{e:?} → {f:?}");
        assert!(!f.text().is_empty() && !f.text().contains("dns"), "no transport detail: {}", f.text());
    }
    // 0.22.2 / 0.7.2: a wait is never shown (or kept) longer than a minute.
    assert!(sync_err(&SyncFail::Api(ApiError::RateLimited(120))).text().contains("60 s"));
}

#[test]
fn passwords_sync_through_unlocked_vaults_only() {
    let srv = Srv::new();
    let mut a = Phone::new("pw-a");
    a.vault.create("vault password a".into(), 1).unwrap();
    a.vault.save_login("https://site.example/login".into(), "alice".into(), "s3cret-a".into(), 2).unwrap();
    a.sign_in(&srv, "dev-a");
    let (_, rk) = a.set_up(&srv);
    a.engine.run(&srv, now()).unwrap();
    a.engine.run(&srv, now()).unwrap(); // the first passwords run only pulls

    let mut b = Phone::new("pw-b");
    b.vault.create("vault password b".into(), 1).unwrap();
    b.sign_in(&srv, "dev-b");
    b.engine.turn_on(&srv, now()).unwrap();
    b.engine.restore_try(&srv, rk, true, now()).unwrap();
    b.engine.restore_confirm(&srv, now()).unwrap();
    let r = b.engine.run(&srv, now()).unwrap();
    assert!(r.changed.passwords, "{r:?}");
    let got = b.vault.summaries().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].username, "alice");
    // B adds one; A gets it. With A's vault locked it waits on the server.
    b.vault.save_login("https://other.example/".into(), "bob".into(), "s3cret-b".into(), 3).unwrap();
    b.engine.run(&srv, now()).unwrap();
    a.vault.lock();
    let r = a.engine.run(&srv, now()).unwrap();
    assert!(!r.changed.passwords, "locked: nothing decrypted");
    a.vault.unlock("vault password a".into(), 4).unwrap();
    a.engine.vault_unlocked(now());
    let r = a.engine.run(&srv, now()).unwrap();
    assert!(r.changed.passwords, "{r:?}");
    assert_eq!(a.vault.summaries().unwrap().len(), 2);
    // Signing out removes the passwords key from the vault (the logins stay).
    assert!(b.engine.sign_out(&srv, false, now()));
    assert!(b.vault.sync_view(|v| v.sync_part().is_none()).unwrap());
    assert_eq!(b.vault.summaries().unwrap().len(), 2);
}

/// 0.6.1: a phone that joined without a vault gets the passwords key later with "Add passwords"
/// (the operator's report: the vault stayed empty).
#[test]
fn a_phone_that_joined_without_its_vault_adds_passwords_later() {
    let srv = Srv::new();
    let mut a = Phone::new("addpw-a");
    a.vault.create("vault password a".into(), 1).unwrap();
    a.vault.save_login("https://site.example/".into(), "alice".into(), "s3cret".into(), 2).unwrap();
    a.sign_in(&srv, "dev-a");
    let (passphrase, rk) = a.set_up(&srv);
    a.engine.run(&srv, now()).unwrap();
    a.engine.run(&srv, now()).unwrap();

    // The phone joins with no vault at all: everything but passwords syncs.
    let mut b = Phone::new("addpw-b");
    b.sign_in(&srv, "dev-b");
    b.engine.turn_on(&srv, now()).unwrap();
    b.engine.restore_try(&srv, rk, true, now()).unwrap();
    b.engine.restore_confirm(&srv, now()).unwrap();
    b.engine.run(&srv, now()).unwrap();
    assert_eq!(b.engine.status().passwords, PasswordsSync::NoVault);

    // A vault now, but no passwords key yet: the status says so.
    b.vault.create("vault password b".into(), 3).unwrap();
    assert_eq!(b.engine.status().passwords, PasswordsSync::NotOnThisDevice);
    assert!(b.vault.summaries().unwrap().is_empty());

    // The vault password (the operator's 0.6.1 report) isn't a sync passphrase at all: refused
    // at once, with no escrow fetch (no "recovery data downloaded" mail).
    let escrow_before = srv.count("keys/escrow");
    for typed in ["vault password b", "KRNK-NOPE-NOPE"] {
        match b.engine.add_passwords(&srv, typed.into(), false, now()) {
            Err(SyncFfiError::WrongSecret { text }) => assert!(text.contains("vault password"), "{text}"),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(srv.count("keys/escrow"), escrow_before);
    // A wrong passphrase changes nothing.
    let wrong = b.engine.add_passwords(&srv, "abandon ability able about above absent absorb".into(), false, now());
    assert!(matches!(wrong, Err(SyncFfiError::WrongSecret { .. })), "{wrong:?}");
    assert_eq!(b.engine.status().passwords, PasswordsSync::NotOnThisDevice);
    // The right one: the key is in the vault, and the next run brings the logins.
    b.engine.add_passwords(&srv, passphrase, false, now()).unwrap();
    assert_eq!(srv.count("keys/escrow"), escrow_before + 1, "one escrow fetch for both tries");
    assert_eq!(b.engine.status().passwords, PasswordsSync::On);
    let r = b.engine.run(&srv, now()).unwrap();
    assert!(r.changed.passwords, "{r:?}");
    assert_eq!(b.vault.summaries().unwrap()[0].username, "alice");

    // And the other way: a login made on the phone reaches the first device.
    b.vault.save_login("https://phone.example/".into(), "bob".into(), "p".into(), 4).unwrap();
    b.engine.run(&srv, now()).unwrap();
    a.engine.run(&srv, now()).unwrap();
    assert_eq!(a.vault.summaries().unwrap().len(), 2);

    // A locked vault reads as "locked", not as "not on this device".
    b.vault.lock();
    assert_eq!(b.engine.status().passwords, PasswordsSync::VaultLocked);
}

/// The account endpoints behind the Kotlin-side transport interface, for tests of the
/// [`KrunkSync`] object itself ([`Srv`]'s in-memory server can't cross threads).
#[derive(Default)]
struct AccountFront {
    approved: std::sync::atomic::AtomicBool,
    revoked: std::sync::atomic::AtomicBool,
    seen: Mutex<Vec<String>>,
}

impl AccountFront {
    fn count(&self, path: &str) -> usize {
        self.seen.lock().unwrap().iter().filter(|p| p.as_str() == path).count()
    }
}

impl SyncTransport for AccountFront {
    fn call(
        &self,
        method: String,
        path: String,
        bearer: Option<String>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpAnswer, TransportError> {
        use std::sync::atomic::Ordering::SeqCst;
        self.seen.lock().unwrap().push(path.clone());
        let json: serde_json::Value = body.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let r = match (method.as_str(), path.as_str()) {
            ("POST", "link/authorize") => ok(serde_json::json!({
                "device_code": "dc1", "user_code": "ABCD-EFGH",
                "verification_uri_complete": "https://krunk.kruxsys.com/link?code=ABCD-EFGH",
                "expires_in": 600, "interval": 5
            })),
            ("POST", "link/token") if self.approved.load(SeqCst) => ok(serde_json::json!({
                "device_id": "dev-w", "account_id": ACC, "access_token": "at-w", "refresh_token": "r-w"
            })),
            ("POST", "link/token") => err(400, "authorization_pending"),
            ("POST", "token/refresh") if self.revoked.load(SeqCst) => err(401, "unauthorized"),
            ("POST", "token/refresh") => ok(serde_json::json!({ "access_token": "at-w", "refresh_token": "r-w" })),
            ("GET", "device/me") => ok(
                serde_json::json!({ "email": "fx@example.com", "account_id": ACC, "device_id": "dev-w", "label": "Krunk on Android 16" }),
            ),
            ("POST", "device/web-sign-in") if self.revoked.load(SeqCst) || bearer.as_deref() != Some("at-w") => {
                err(401, "unauthorized")
            }
            ("POST", "device/web-sign-in") => {
                assert_eq!(json, serde_json::json!({}));
                ok(serde_json::json!({ "code": "c0de-w_x", "expires_in": 60 }))
            }
            _ => err(404, "not_found"),
        };
        let r = r.map_err(|detail| TransportError::Offline { detail })?;
        Ok(HttpAnswer { status: r.status, body: r.body, retry_after: r.retry_after, date: r.date })
    }
}

struct PlainWrap;

impl VaultKeyWrapper for PlainWrap {
    fn protect(&self, data: Vec<u8>) -> Result<Vec<u8>, crate::vault::WrapError> {
        Ok(data)
    }
    fn unprotect(&self, data: Vec<u8>) -> Result<Vec<u8>, crate::vault::WrapError> {
        Ok(data)
    }
}

#[test]
fn a_signed_in_phone_signs_the_website_in() {
    use std::sync::atomic::Ordering::SeqCst;
    let root = std::env::temp_dir().join(format!("krunk-ffi-sync-web-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let srv = Arc::new(AccountFront::default());
    let make = |site: Option<&str>| {
        KrunkSync::new(
            root.join("krunk-sync").to_string_lossy().into_owned(),
            root.join("browser.db").to_string_lossy().into_owned(),
            srv.clone(),
            Arc::new(PlainWrap),
            None,
            "Krunk on Android 16".into(),
            "0.11.0".into(),
            site.map(str::to_string),
        )
    };
    let login = "https://krunk.kruxsys.com/account/login?next=%2Faccount%2F".to_string();
    let k = make(Some("https://krunk.kruxsys.com"));
    assert_eq!(k.status().stage, SyncStage::SignedOut);
    assert_eq!(k.auto_sign_in_check(login.clone(), false), None, "signed out: never");
    assert!(matches!(k.web_sign_in_url(None), Err(SyncFfiError::SignedOut { .. })));
    assert_eq!(srv.count("device/web-sign-in"), 0);

    k.link_start().unwrap();
    srv.approved.store(true, SeqCst);
    assert_eq!(k.link_poll().unwrap(), LinkPoll::SignedIn { email: "fx@example.com".into() });
    // Signed in (sync off): the check knows without another status call.
    assert_eq!(k.auto_sign_in_check(login.clone(), true), None, "private tab: never");
    assert_eq!(k.auto_sign_in_check("https://krunk.kruxsys.com/account/".into(), false), None, "other pages: no");
    assert_eq!(k.auto_sign_in_check(login.clone(), false), Some(AutoSignInTarget { next: Some("/account/".into()) }));
    assert_eq!(k.auto_sign_in_check(login.clone(), false), None, "once per 30 s");
    // The real path through the transport (Kotlin's allowlist has to pass it as well).
    let url = k.web_sign_in_url(Some("/account/".into())).unwrap();
    assert_eq!(url, "https://krunk.kruxsys.com/account/login?auto=c0de-w_x&next=%2Faccount%2F");
    assert_eq!(srv.count("device/web-sign-in"), 1);
    // A sign-out on the website keeps it signed out until the app restarts.
    assert_eq!(k.auto_sign_in_check("https://krunk.kruxsys.com/account/login?signed_out=1".into(), false), None);

    // An app restart: a new object (the account is read by the first status call).
    let k = make(Some("https://krunk.kruxsys.com"));
    assert_eq!(k.auto_sign_in_check(login.clone(), false), None, "before the first status call");
    k.status();
    assert!(k.auto_sign_in_check(login.clone(), false).is_some(), "after a restart");
    // A test build against a staging stack has no site: never.
    let staging = make(None);
    staging.status();
    assert_eq!(staging.auto_sign_in_check(login.clone(), false), None);

    // The device removed on the website: no code, and the phone notices it is signed out.
    srv.revoked.store(true, SeqCst);
    let e = k.web_sign_in_url(None);
    assert!(matches!(e, Err(SyncFfiError::Revoked { .. })), "{e:?}");
    assert_eq!(k.status().stage, SyncStage::SignedOut);
    let _ = std::fs::remove_dir_all(&root);
}
