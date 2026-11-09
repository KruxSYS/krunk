//! A Linux sync client for the tests (0.9.0): the second (and third) device next to Krunk on the
//! VM and in `server/e2e/sync.cjs`. It runs Krunk's own `core::sync` (keys, directories, records)
//! against a real server; only the transport (`ureq`) and the storage of its files differ.
//!
//!   krunk-sync-cli <command> --dir <folder> [options]
//!
//! The folder holds `cli.json` (server, ids, the refresh token and a cached access token — in
//! clear: a test tool), `sync.json` (sealed, without a platform wrapper), `vault.json` and, from
//! S7 on, `browser.db`. Output lines are `key: value` for the test scripts. Exit codes: 0 ok,
//! 2 usage, 3 failed, 4 security alarm (a fork or a tampered directory).

use lb_core::sync::api::{HttpClient, HttpResponse};
use lb_core::sync::client::{self, AApproval, AStep, Api, NApproval, NStep, Refresh, SyncFail};
use lb_core::sync::keys::{self, Me, PakState, Secret};
use lb_core::sync::state::{self, parse_account_id, SyncState};
use lb_core::vault::{KdfParams, NoWrap, Vault, VaultSync};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

const DEFAULT_VAULT_PW: &str = "cli vault password";

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn out(k: &str, v: impl std::fmt::Display) {
    println!("{k}: {v}");
}

fn fail(code: i32, msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(code)
}

fn sfail(e: SyncFail) -> ! {
    let alarm = matches!(&e, SyncFail::Keys(k) if k.is_alarm());
    if alarm {
        out("alarm", &e);
    }
    fail(if alarm { 4 } else { 3 }, e)
}

// ---- Transport -----------------------------------------------------------------------------

struct Ureq {
    agent: ureq::Agent,
    base: String,
}

impl Ureq {
    fn new(server: &str) -> Ureq {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build();
        Ureq { agent: config.into(), base: server.trim_end_matches('/').to_string() }
    }
}

impl HttpClient for Ureq {
    fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, String> {
        let url = format!("{}/api/v1/{path}", self.base);
        let auth = bearer.map(|t| format!("Bearer {t}"));
        let ua = "Krunk/0.9.0 (sync-cli; Linux)";
        let res = match method {
            "GET" => {
                let mut r = self.agent.get(&url).header("User-Agent", ua);
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.call()
            }
            _ => {
                let mut r = self.agent.post(&url).header("User-Agent", ua).header("Content-Type", "application/json");
                if let Some(a) = &auth {
                    r = r.header("Authorization", a);
                }
                r.send(body.unwrap_or(b"{}"))
            }
        };
        let mut res = res.map_err(|e| e.to_string())?;
        let status = res.status().as_u16();
        let header = |n: &str| res.headers().get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (retry_after, date) = (header("retry-after"), header("date"));
        let body = res.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().map_err(|e| e.to_string())?;
        Ok(HttpResponse { status, body, retry_after, date })
    }
}

// ---- The device's files --------------------------------------------------------------------

#[derive(Serialize, Deserialize, Default)]
struct CliFile {
    server: String,
    account_id: String,
    device_id: String,
    label: String,
    refresh_token: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    access_until_ms: i64,
}

struct Device {
    dir: PathBuf,
    cli: CliFile,
    http: Ureq,
}

impl Device {
    fn open(dir: &Path) -> Device {
        let text =
            std::fs::read_to_string(dir.join("cli.json")).unwrap_or_else(|_| fail(2, "not linked: run link first"));
        let cli: CliFile = serde_json::from_str(&text).unwrap_or_else(|_| fail(3, "cli.json"));
        let http = Ureq::new(&cli.server);
        Device { dir: dir.to_path_buf(), cli, http }
    }

    fn save(&self) {
        std::fs::write(self.dir.join("cli.json"), serde_json::to_vec_pretty(&self.cli).unwrap_or_default())
            .unwrap_or_else(|e| fail(3, e));
    }

    /// A valid access token (refreshed when it is about to expire; the refresh token rotates).
    fn token(&mut self) -> String {
        if !self.cli.access_token.is_empty() && self.cli.access_until_ms > now_ms() + 60_000 {
            return self.cli.access_token.clone();
        }
        let body = serde_json::to_vec(&json!({"refresh_token": self.cli.refresh_token})).unwrap_or_default();
        let r = self.http.call("POST", "token/refresh", None, Some(&body)).unwrap_or_else(|e| fail(3, e));
        if r.status != 200 {
            out("refresh", r.status);
            fail(3, format!("token refresh: {}", r.status));
        }
        let v: Value = serde_json::from_slice(&r.body).unwrap_or_default();
        self.cli.refresh_token = v["refresh_token"].as_str().unwrap_or_default().into();
        self.cli.access_token = v["access_token"].as_str().unwrap_or_default().into();
        self.cli.access_until_ms = now_ms() + v["expires_in"].as_i64().unwrap_or(900) * 1000;
        self.save();
        self.cli.access_token.clone()
    }

    fn account(&self) -> lb_core::sync::AccountId {
        parse_account_id(&self.cli.account_id).unwrap_or_else(|| fail(3, "account id"))
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("sync.json")
    }

    fn sync_state(&self) -> SyncState {
        state::load(&self.state_path(), &NoWrap)
            .unwrap_or_else(|e| fail(3, e))
            .unwrap_or_else(|| fail(3, "sync is off: no sync.json"))
    }

    fn save_state(&self, st: &SyncState) {
        state::save(&self.state_path(), st, &NoWrap).unwrap_or_else(|e| fail(3, e));
    }

    fn vault(&self, pw: &str) -> Vault {
        let p = self.dir.join("vault.json");
        if p.exists() {
            Vault::open(&p, pw, &NoWrap).unwrap_or_else(|e| fail(3, format!("vault: {e}")))
        } else {
            Vault::create(&p, pw, KdfParams::insecure_for_tests(), &NoWrap)
                .unwrap_or_else(|e| fail(3, format!("vault: {e}")))
        }
    }

    fn pak_state(&self, pw: &str) -> Option<(Vault, PakState)> {
        let v = self.vault(pw);
        let s = v.sync_part().map(|p| PakState::from_bytes(&p.state).unwrap_or_else(|e| fail(3, e)))?;
        Some((v, s))
    }

    fn save_pak(&self, vault: &mut Vault, pak: &PakState) {
        let dirty = vault.sync_part().map(|p| p.dirty.clone()).unwrap_or_default();
        vault
            .set_sync_part(Some(VaultSync { state: pak.to_bytes(), dirty }), &NoWrap)
            .unwrap_or_else(|e| fail(3, format!("vault: {e}")));
    }

    fn me(&mut self, api_token: &str) -> Me {
        let api = Api { http: &self.http, token: api_token, now_ms: now_ms() };
        let short = client::my_short(&api, &self.cli.device_id).unwrap_or_else(|e| sfail(e));
        Me { device_id: self.cli.device_id.clone(), short }
    }
}

// ---- Commands --------------------------------------------------------------------------------

struct Args {
    cmd: String,
    pos: Vec<String>,
    opt: HashMap<String, String>,
    flags: Vec<String>,
}

fn args() -> Args {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    let mut a = Args { cmd, pos: vec![], opt: HashMap::new(), flags: vec![] };
    let rest: Vec<String> = it.collect();
    let mut i = 0;
    while i < rest.len() {
        let x = &rest[i];
        if let Some(name) = x.strip_prefix("--") {
            let takes_value = i + 1 < rest.len() && !rest[i + 1].starts_with("--");
            if ["vault", "share-passwords", "json"].contains(&name) || !takes_value {
                a.flags.push(name.into());
            } else {
                a.opt.insert(name.into(), rest[i + 1].clone());
                i += 1;
            }
        } else {
            a.pos.push(x.clone());
        }
        i += 1;
    }
    a
}

impl Args {
    fn get(&self, k: &str) -> Option<&str> {
        self.opt.get(k).map(String::as_str)
    }
    fn need(&self, k: &str) -> &str {
        self.get(k).unwrap_or_else(|| fail(2, format!("--{k} is needed")))
    }
    fn flag(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f == k)
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(self.get("timeout").and_then(|t| t.parse().ok()).unwrap_or(300))
    }
    fn vault_pw(&self) -> String {
        self.get("vault-password").unwrap_or(DEFAULT_VAULT_PW).to_string()
    }
}

fn wait_file(path: &str, deadline: Instant) -> String {
    loop {
        if let Ok(t) = std::fs::read_to_string(path) {
            if !t.trim().is_empty() {
                return t.trim().to_string();
            }
        }
        if Instant::now() > deadline {
            fail(3, format!("timed out waiting for {path}"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn link(a: &Args, dir: &Path) {
    std::fs::create_dir_all(dir).unwrap_or_else(|e| fail(3, e));
    let server = a.need("server");
    let label = a.get("label").unwrap_or("Krunk test client on Linux");
    let http = Ureq::new(server);
    let body = serde_json::to_vec(&json!({"platform": "windows", "label": label, "client_version": "0.9.0"}))
        .unwrap_or_default();
    let r = http.call("POST", "link/authorize", None, Some(&body)).unwrap_or_else(|e| fail(3, e));
    let v: Value = serde_json::from_slice(&r.body).unwrap_or_default();
    if r.status != 200 {
        fail(3, format!("authorize: {} {v}", r.status));
    }
    out("user_code", v["user_code"].as_str().unwrap_or_default());
    let dc = v["device_code"].as_str().unwrap_or_default().to_string();
    let mut interval = v["interval"].as_u64().unwrap_or(5);
    let deadline = Instant::now() + a.timeout();
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        let body = serde_json::to_vec(&json!({"device_code": dc})).unwrap_or_default();
        let r = http.call("POST", "link/token", None, Some(&body)).unwrap_or_else(|e| fail(3, e));
        let t: Value = serde_json::from_slice(&r.body).unwrap_or_default();
        if r.status == 200 {
            let cli = CliFile {
                server: server.into(),
                account_id: t["account_id"].as_str().unwrap_or_default().into(),
                device_id: t["device_id"].as_str().unwrap_or_default().into(),
                label: label.into(),
                refresh_token: t["refresh_token"].as_str().unwrap_or_default().into(),
                access_token: t["access_token"].as_str().unwrap_or_default().into(),
                access_until_ms: now_ms() + 840_000,
            };
            std::fs::write(dir.join("cli.json"), serde_json::to_vec_pretty(&cli).unwrap_or_default())
                .unwrap_or_else(|e| fail(3, e));
            out("device_id", &cli.device_id);
            out("account_id", &cli.account_id);
            return;
        }
        match t["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            e => fail(3, format!("link: {e:?}")),
        }
        if Instant::now() > deadline {
            fail(3, "link timed out");
        }
    }
}

fn save_joined(d: &Device, me: &Me, j: &keys::Joined, gen: String, vault_pw: Option<&str>) {
    let st = client::new_state(&d.cli.account_id, me, j, gen);
    d.save_state(&st);
    if let (Some(pak), Some(pw)) = (&j.pak, vault_pw) {
        let mut v = d.vault(pw);
        d.save_pak(&mut v, pak);
        out("passwords", "on");
    }
    out("epoch", j.ak.epoch);
    out("ak_seq", j.ak.seq);
}

fn main() {
    let a = args();
    let dir = PathBuf::from(a.get("dir").unwrap_or_else(|| fail(2, "--dir is needed")));
    match a.cmd.as_str() {
        "link" => link(&a, &dir),
        "state" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let r = d.http.call("GET", "sync/state", Some(&t), None).unwrap_or_else(|e| fail(3, e));
            println!("{}", String::from_utf8_lossy(&r.body));
        }
        "setup" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let me = d.me(&t);
            let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
            let plan = keys::plan_setup(d.account(), me.clone(), a.flag("vault")).unwrap_or_else(|e| sfail(e.into()));
            out("passphrase", &*plan.passphrase);
            out("recovery_key", &*plan.recovery_key);
            let (j, gen) = client::setup(&api, &plan).unwrap_or_else(|e| sfail(e));
            save_joined(&d, &me, &j, gen, a.flag("vault").then(|| a.vault_pw()).as_deref());
        }
        "restore" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let me = d.me(&t);
            let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
            let secret = match (a.get("passphrase"), a.get("recovery")) {
                (Some(p), _) => Secret::Passphrase(Zeroizing::new(p.into())),
                (None, Some(k)) => Secret::RecoveryKey(Zeroizing::new(k.into())),
                _ => fail(2, "--passphrase or --recovery is needed"),
            };
            let plan = client::restore_begin(&api, &secret, d.account()).unwrap_or_else(|e| sfail(e));
            for dev in &plan.ak_dir.devices {
                out("keeps_access", &dev.id);
            }
            for r in &plan.ak_dir.revoked {
                out("revoked", r);
            }
            let (j, gen) = client::restore_finish(&api, &plan, &me, a.flag("vault")).unwrap_or_else(|e| sfail(e));
            save_joined(&d, &me, &j, gen, a.flag("vault").then(|| a.vault_pw()).as_deref());
        }
        "join" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let me = d.me(&t);
            let deadline = Instant::now() + a.timeout();
            let name = a.get("name").unwrap_or("Linux test client").to_string();
            let mut n = {
                let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
                NApproval::start(&api, a.flag("vault")).unwrap_or_else(|e| sfail(e))
            };
            out("approval", &n.id);
            loop {
                let t = d.token();
                let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
                match n.poll(&api, &d.account(), &me.device_id, &name).unwrap_or_else(|e| sfail(e)) {
                    NStep::EnterCode => {
                        out("enter_code", "waiting");
                        let code = match a.get("code") {
                            Some(c) => c.to_string(),
                            None => wait_file(a.need("code-file"), deadline),
                        };
                        match n.confirm(&api, &code) {
                            Ok(()) => out("code", "sent"),
                            Err(e) => {
                                out("code", "mismatch");
                                sfail(e)
                            }
                        }
                    }
                    NStep::Ready => {
                        let (j, p) = n.finish(&api, d.account(), &me.device_id).unwrap_or_else(|e| sfail(e));
                        let st = client::state(&api).unwrap_or_else(|e| sfail(e));
                        out("dev_short", p.dev_short);
                        save_joined(
                            &d,
                            &me,
                            &j,
                            st.keyset_gen.unwrap_or_default(),
                            a.flag("vault").then(|| a.vault_pw()).as_deref(),
                        );
                        return;
                    }
                    NStep::Gone => fail(3, "the approval was cancelled or expired"),
                    NStep::Waiting(_) => {}
                }
                if Instant::now() > deadline {
                    n.cancel(&api);
                    fail(3, "join timed out");
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
        "approve" => {
            let mut d = Device::open(&dir);
            let deadline = Instant::now() + a.timeout();
            let mut st = d.sync_state();
            // Wait for a request.
            let item = loop {
                let t = d.token();
                let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
                let list = client::approvals(&api).unwrap_or_else(|e| sfail(e));
                if let Some(i) = list.into_iter().find(|i| i.state == "requested") {
                    break i;
                }
                if Instant::now() > deadline {
                    fail(3, "no approval request arrived");
                }
                std::thread::sleep(Duration::from_secs(3));
            };
            out("request_from", format!("{} ({})", item.device.label, item.device.id));
            let t = d.token();
            let mut ap = {
                let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
                AApproval::respond(&api, &item).unwrap_or_else(|e| sfail(e))
            };
            let mut shown = false;
            loop {
                let t = d.token();
                let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
                match ap.poll(&api, &d.account(), &st.device_id).unwrap_or_else(|e| sfail(e)) {
                    AStep::ShowCode(sas) if !shown => {
                        out("sas", &sas);
                        if let Some(f) = a.get("sas-file") {
                            std::fs::write(f, sas.replace(' ', "")).unwrap_or_else(|e| fail(3, e));
                        }
                        shown = true;
                    }
                    AStep::Confirmed => {
                        let share = a.flag("share-passwords").then(|| d.pak_state(&a.vault_pw())).flatten();
                        let pin = ap.admit(&api, &st, share.as_ref().map(|(_, p)| &p.pak)).unwrap_or_else(|e| sfail(e));
                        st.ak = Some(pin);
                        d.save_state(&st);
                        out("admitted", &item.device.id);
                        out("shared_passwords", share.is_some());
                        return;
                    }
                    AStep::Gone => fail(3, "the approval was cancelled or expired"),
                    _ => {}
                }
                if Instant::now() > deadline {
                    ap.cancel(&api);
                    fail(3, "approve timed out");
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
        "refresh" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
            let mut st = d.sync_state();
            let mut pak = a.flag("vault").then(|| d.pak_state(&a.vault_pw())).flatten();
            match client::refresh(&api, &mut st, pak.as_mut().map(|(_, p)| p)).unwrap_or_else(|e| sfail(e)) {
                Refresh::SelfRevoked => {
                    let _ = std::fs::remove_file(d.state_path());
                    out("refresh", "self-revoked (sync keys wiped)");
                }
                r => {
                    d.save_state(&st);
                    if let Some((mut v, p)) = pak {
                        d.save_pak(&mut v, &p);
                    }
                    out("refresh", format!("{r:?}"));
                    if let Some(p) = &st.ak {
                        out("epoch", p.epoch);
                        out("ak_seq", p.seq);
                    }
                }
            }
        }
        "devices" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let api = Api { http: &d.http, token: &t, now_ms: now_ms() };
            for x in client::devices(&api).unwrap_or_else(|e| sfail(e)) {
                println!(
                    "device: {} short={} keys={} self={} revoked={} {}",
                    x.id,
                    x.short,
                    x.has_keys,
                    x.is_self.unwrap_or(false),
                    x.revoked_at.is_some(),
                    x.label
                );
            }
        }
        "revoke" => {
            let mut d = Device::open(&dir);
            let t = d.token();
            let id = a.pos.first().unwrap_or_else(|| fail(2, "revoke <device id>"));
            let r = d
                .http
                .call("POST", &format!("devices/{id}/revoke"), Some(&t), Some(b"{}"))
                .unwrap_or_else(|e| fail(3, e));
            out("revoke", r.status);
        }
        _ => fail(2, "commands: link state setup restore join approve refresh devices revoke"),
    }
}
