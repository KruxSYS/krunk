//! Integration tests against a real PostgreSQL (milestone S2 acceptance, PLAN §9).
//!
//! They run when `KRUNK_TEST_DATABASE_URL` points at a server where the user may create
//! databases, e.g. a throwaway container:
//! ```sh
//! docker run -d --name krunk-pgtest -e POSTGRES_PASSWORD=itpass -p 55432:5432 postgres:18.6-alpine3.24
//! KRUNK_TEST_DATABASE_URL=postgres://postgres:itpass@localhost:55432/postgres cargo test it::
//! ```
//! Each test gets its own fresh database, migrated from scratch. Without the variable they pass
//! trivially (and say so). Turnstile is a local mock whose answer is driven by the token:
//! `ok:<action>:<n>` passes for that action; `fail:<n>` fails; `badhost:…`, `old:…` fail the
//! hostname and age checks.

use crate::config::Config;
use crate::http::{router, AppState};
use crate::turnstile::Turnstile;
use crate::{client_ip::TrustedPeers, jobs, sessions, totp};
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower::ServiceExt;

mod s3;
mod s6;
mod s7;

static N: AtomicU32 = AtomicU32::new(0);

fn next() -> u32 {
    N.fetch_add(1, Ordering::Relaxed)
}

/// A fresh Turnstile token for `action`.
fn ts(action: &str) -> String {
    format!("ok:{action}:{}", next())
}

async fn mock_siteverify() -> String {
    use axum::routing::post;
    let app = axum::Router::new().route(
        "/siteverify",
        post(|axum::extract::Form(f): axum::extract::Form<Vec<(String, String)>>| async move {
            let token = f.iter().find(|(k, _)| k == "response").map(|(_, v)| v.clone()).unwrap_or_default();
            let mut parts = token.split(':');
            let kind = parts.next().unwrap_or("");
            let action = parts.next().unwrap_or("");
            let now = chrono::Utc::now();
            let ts = if kind == "old" { now - chrono::Duration::seconds(400) } else { now };
            let host = if kind == "badhost" { "evil.example" } else { "krunk.209.wf" };
            axum::Json(json!({
                "success": kind != "fail",
                "hostname": host,
                "action": action,
                "challenge_ts": ts.to_rfc3339(),
                "error-codes": if kind == "fail" { json!(["invalid-input-response"]) } else { json!([]) },
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/siteverify")
}

struct Env {
    state: AppState,
    db: PgPool,
}

async fn env() -> Option<Env> {
    let Ok(admin_url) = std::env::var("KRUNK_TEST_DATABASE_URL") else {
        eprintln!("KRUNK_TEST_DATABASE_URL is not set: skipping the database integration test");
        return None;
    };
    let admin = PgPoolOptions::new().max_connections(2).connect(&admin_url).await.unwrap();
    let name = format!("krunk_it_{}_{}", std::process::id(), next());
    // The name is ours (letters, digits, underscores).
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').unwrap().0);
    let db = PgPoolOptions::new().max_connections(10).connect(&url).await.unwrap();
    crate::db::migrate(&db).await.unwrap();
    let env = [
        ("DATABASE_URL", url.as_str()),
        ("PUBLIC_BASE_URL", "https://krunk.209.wf"),
        ("TURNSTILE_SITE_KEY", "0x4AAAsite"),
        ("TURNSTILE_SECRET_KEY", "0x4AAAsecret"),
        ("SMTP_HOST", "smtp.example.org"),
    ];
    let config = Config::from_lookup(&|k| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())).unwrap();
    let turnstile = Turnstile::with_url(&mock_siteverify().await, "0x4AAAsecret", "krunk.209.wf", false).unwrap();
    let state = AppState {
        config: Arc::new(config),
        db: db.clone(),
        trusted: TrustedPeers::default(),
        passwords: crate::http::tests::passwords(),
        turnstile,
    };
    Some(Env { state, db })
}

struct Resp {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    raw: String,
}

/// A browser-like client from one IP, keeping its session cookie and CSRF token.
#[derive(Clone)]
struct Browser {
    ip: IpAddr,
    cookie: Option<String>,
    csrf: Option<String>,
}

impl Browser {
    fn new() -> Browser {
        let n = next();
        Browser { ip: format!("203.0.{}.{}", n / 250, n % 250 + 1).parse().unwrap(), cookie: None, csrf: None }
    }

    async fn call(&mut self, e: &Env, method: Method, path: &str, body: Option<Value>) -> Resp {
        let mut b = Request::builder()
            .method(method.clone())
            .uri(format!("/api/v1/web/{path}"))
            .header(header::USER_AGENT, "it-test/1.0");
        if method != Method::GET {
            b = b.header(header::ORIGIN, "https://krunk.209.wf").header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(c) = &self.cookie {
            b = b.header(header::COOKIE, format!("{}={c}", sessions::COOKIE));
        }
        if let Some(c) = &self.csrf {
            b = b.header(sessions::CSRF_HEADER, c);
        }
        let mut req = b.body(body.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty)).unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(self.ip, 40000)));
        let res = router(e.state.clone()).oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        if let Some(set) = parts.headers.get(header::SET_COOKIE).and_then(|v| v.to_str().ok()) {
            let v = set.split(';').next().unwrap().split_once('=').unwrap().1.to_string();
            self.cookie = (!v.is_empty()).then_some(v);
        }
        let body: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        if let Some(c) = body.get("csrf").and_then(|v| v.as_str()) {
            self.csrf = Some(c.to_string());
        }
        Resp { status: parts.status, headers: parts.headers, body, raw }
    }

    async fn post(&mut self, e: &Env, path: &str, body: Value) -> Resp {
        self.call(e, Method::POST, path, Some(body)).await
    }

    async fn get(&mut self, e: &Env, path: &str) -> Resp {
        self.call(e, Method::GET, path, None).await
    }
}

fn err(r: &Resp) -> &str {
    r.body["error"].as_str().unwrap_or("")
}

async fn outbox(db: &PgPool) -> Vec<(String, String, Value)> {
    sqlx::query_as("SELECT to_addr, template, params FROM email_outbox ORDER BY id").fetch_all(db).await.unwrap()
}

/// The token in the newest queued link mail of `template` to `to`.
async fn link_token(db: &PgPool, to: &str, template: &str) -> String {
    let rows = outbox(db).await;
    let (_, _, p) = rows.iter().rev().find(|(t, tpl, _)| t == to && tpl == template).expect("a queued mail");
    p["link"].as_str().unwrap().split("token=").nth(1).unwrap().to_string()
}

async fn registered_and_verified(e: &Env, email: &str, pw: &str) -> Browser {
    let mut b = Browser::new();
    let r = b.post(e, "register", json!({"email": email, "password": pw, "turnstile": ts("register")})).await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.raw);
    let t = link_token(&e.db, email, "verify").await;
    let r = b.post(e, "verify-email", json!({"token": t, "password": pw})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    b
}

async fn login(e: &Env, b: &mut Browser, email: &str, pw: &str, totp: Option<&str>) -> Resp {
    let mut body = json!({"email": email, "password": pw, "turnstile": ts("login")});
    if let Some(t) = totp {
        body["totp"] = json!(t);
    }
    b.post(e, "login", body).await
}

const PW: &str = "correct horse battery";

#[tokio::test]
async fn register_verify_login_logout() {
    let Some(e) = env().await else { return };
    let mut b = Browser::new();
    let r = b
        .post(&e, "register", json!({"email": "Alice@Example.org", "password": PW, "turnstile": ts("register")}))
        .await;
    assert_eq!((r.status, r.body["status"].as_str()), (StatusCode::ACCEPTED, Some("check_email")));
    assert_eq!(r.headers[header::CACHE_CONTROL], "no-store");
    // Not verified yet: the right password gets email_unverified, a wrong one the generic 401.
    let r = login(&e, &mut b, "alice@example.org", PW, None).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "email_unverified"));
    assert!(b.cookie.is_none());
    let r = login(&e, &mut b, "alice@example.org", "wrong password!!", None).await;
    assert_eq!((r.status, err(&r)), (StatusCode::UNAUTHORIZED, "unauthorized"));
    // The mail went to the address as typed (domain lower-cased).
    let mails = outbox(&e.db).await;
    assert_eq!(mails.len(), 1);
    assert_eq!((mails[0].0.as_str(), mails[0].1.as_str()), ("Alice@example.org", "verify"));
    let token = link_token(&e.db, "Alice@example.org", "verify").await;
    assert!(mails[0].2["link"].as_str().unwrap().starts_with("https://krunk.209.wf/account/verify?token="));
    let r = b.post(&e, "verify-email", json!({"token": token, "password": "not the password"})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "verifying needs the password too");
    let r = b.post(&e, "verify-email", json!({"token": token, "password": PW})).await;
    assert_eq!((r.status, r.body["status"].as_str()), (StatusCode::OK, Some("verified")));
    let r = b.post(&e, "verify-email", json!({"token": token, "password": PW})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "invalid_link"), "single use");

    let r = login(&e, &mut b, "ALICE@example.ORG", PW, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let set = r.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(
        set.starts_with("__Host-krunk_session=")
            && set.contains("Secure")
            && set.contains("HttpOnly")
            && set.contains("SameSite=Strict")
    );
    let r = b.get(&e, "me").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body["email"], "Alice@example.org");
    assert_eq!(r.body["email_verified"], true);
    assert_eq!(r.body["totp"], false);
    assert_eq!(r.body["csrf"].as_str(), b.csrf.as_deref());
    // The session row: hashed token, IP, country unknown, user agent.
    let (ua, ip): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT user_agent, host(ip) FROM web_sessions").fetch_one(&e.db).await.unwrap();
    assert_eq!(ua.as_deref(), Some("it-test/1.0"));
    assert_eq!(ip, Some(b.ip.to_string()));
    let stored: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM web_sessions").fetch_one(&e.db).await.unwrap();
    assert_ne!(stored, crate::crypto::b64url_decode::<32>(b.cookie.as_ref().unwrap()).unwrap().to_vec());

    // Logout without the CSRF header is refused; with it, the session ends.
    let mut no_csrf = Browser { csrf: None, ..b.clone() };
    assert_eq!(no_csrf.post(&e, "logout", json!({})).await.status, StatusCode::FORBIDDEN);
    let old = b.clone();
    let r = b.post(&e, "logout", json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(r.headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));
    let mut old = old;
    assert_eq!(old.get(&e, "me").await.status, StatusCode::UNAUTHORIZED, "the old cookie is dead");
}

#[tokio::test]
async fn nothing_reveals_whether_an_account_exists() {
    let Some(e) = env().await else { return };
    registered_and_verified(&e, "bob@example.org", PW).await;
    let mut b = Browser::new();
    let known =
        b.post(&e, "register", json!({"email": "bob@example.org", "password": PW, "turnstile": ts("register")})).await;
    let new = b
        .post(&e, "register", json!({"email": "carol@example.org", "password": PW, "turnstile": ts("register")}))
        .await;
    assert_eq!((known.status, &known.raw), (new.status, &new.raw));
    // The existing owner is told someone tried.
    assert!(outbox(&e.db).await.iter().any(|(t, tpl, _)| t == "bob@example.org" && tpl == "register_exists"));

    let mut b = Browser::new();
    let wrong = login(&e, &mut b, "bob@example.org", "wrong password!!", None).await;
    let unknown = login(&e, &mut b, "nobody@example.org", "wrong password!!", None).await;
    assert_eq!((wrong.status, &wrong.raw), (unknown.status, &unknown.raw));

    let mut b = Browser::new();
    let a = b.post(&e, "password-reset/request", json!({"email": "bob@example.org", "turnstile": ts("reset")})).await;
    let z = b.post(&e, "password-reset/request", json!({"email": "zed@example.org", "turnstile": ts("reset")})).await;
    assert_eq!((a.status, &a.raw), (z.status, &z.raw));
    let r = b.post(&e, "resend-verification", json!({"email": "bob@example.org", "turnstile": ts("resend")})).await;
    let s = b.post(&e, "resend-verification", json!({"email": "zed@example.org", "turnstile": ts("resend")})).await;
    assert_eq!((r.status, &r.raw), (s.status, &s.raw));
    assert!(!outbox(&e.db).await.iter().any(|(t, _, _)| t.starts_with("zed@")), "no mail to unknown addresses");

    // The lockout is per email, account or not: 5 failures, then 429 with Retry-After.
    registered_and_verified(&e, "lock@example.org", PW).await;
    for email in ["lock@example.org", "nobody2@example.org"] {
        let mut b = Browser::new();
        for i in 0..5 {
            let r = login(&e, &mut b, email, "wrong password!!", None).await;
            assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{email} attempt {i}");
        }
        let r = login(&e, &mut b, email, "wrong password!!", None).await;
        assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"), "{email}");
        let wait: u64 = r.headers[header::RETRY_AFTER].to_str().unwrap().parse().unwrap();
        assert!((1..=60).contains(&wait), "{wait}");
        // Even the right password waits.
        let mut b2 = Browser::new();
        assert_eq!(login(&e, &mut b2, email, PW, None).await.status, StatusCode::TOO_MANY_REQUESTS);
    }
}

/// Generic responses also take the same time (±20 %), on the means of 4 runs. Meaningful only
/// alone and optimised (in the parallel debug suite the hashes queue behind each other), so:
/// `cargo test --release it::timing -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn timing_of_known_and_unknown_addresses() {
    let Some(e) = env().await else { return };
    registered_and_verified(&e, "tim@example.org", PW).await;
    let mut known = Duration::ZERO;
    let mut unknown = Duration::ZERO;
    let (mut reg_known, mut reg_new) = (Duration::ZERO, Duration::ZERO);
    for i in 0..4 {
        let mut b = Browser::new();
        let t = Instant::now();
        login(&e, &mut b, "tim@example.org", "wrong password!!", None).await;
        known += t.elapsed();
        let t = Instant::now();
        login(&e, &mut b, &format!("nobody{i}@example.org"), "wrong password!!", None).await;
        unknown += t.elapsed();
        let mut b = Browser::new();
        let t = Instant::now();
        b.post(&e, "register", json!({"email": "tim@example.org", "password": PW, "turnstile": ts("register")})).await;
        reg_known += t.elapsed();
        let t = Instant::now();
        b.post(
            &e,
            "register",
            json!({"email": format!("new{i}@example.org"), "password": PW, "turnstile": ts("register")}),
        )
        .await;
        reg_new += t.elapsed();
    }
    let ratio = |a: Duration, b: Duration| a.as_secs_f64() / b.as_secs_f64();
    eprintln!(
        "login known/unknown {:.3}, register known/new {:.3} (means {:?} {:?} {:?} {:?})",
        ratio(known, unknown),
        ratio(reg_known, reg_new),
        known / 4,
        unknown / 4,
        reg_known / 4,
        reg_new / 4
    );
    assert!((0.8..=1.2).contains(&ratio(known, unknown)), "login timing differs");
    assert!((0.8..=1.2).contains(&ratio(reg_known, reg_new)), "register timing differs");
}

#[tokio::test]
async fn pre_registration_hijack_is_closed() {
    let Some(e) = env().await else { return };
    let mut attacker = Browser::new();
    let r = attacker
        .post(
            &e,
            "register",
            json!({"email": "vic@example.org", "password": "attacker pass 1", "turnstile": ts("register")}),
        )
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    let attacker_link = link_token(&e.db, "vic@example.org", "verify").await;
    // The victim registers too: their password replaces the attacker's, the old link dies.
    let mut victim = Browser::new();
    victim.post(&e, "register", json!({"email": "vic@example.org", "password": PW, "turnstile": ts("register")})).await;
    let victim_link = link_token(&e.db, "vic@example.org", "verify").await;
    assert_ne!(attacker_link, victim_link);
    let r = victim.post(&e, "verify-email", json!({"token": attacker_link, "password": PW})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "the attacker's link is void");
    // Had the attacker re-registered after the victim, the victim couldn't verify the attacker's
    // password with their own: the link needs the current password.
    let r = victim.post(&e, "verify-email", json!({"token": victim_link, "password": "attacker pass 1"})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = victim.post(&e, "verify-email", json!({"token": victim_link, "password": PW})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        login(&e, &mut attacker, "vic@example.org", "attacker pass 1", None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(&e, &mut victim, "vic@example.org", PW, None).await.status, StatusCode::OK);
}

#[tokio::test]
async fn turnstile_rules() {
    let Some(e) = env().await else { return };
    registered_and_verified(&e, "tu@example.org", PW).await;
    let mut b = Browser::new();
    let try_login = |tok: String| {
        let mut b = b.clone();
        let e = &e;
        async move { b.post(e, "login", json!({"email": "tu@example.org", "password": PW, "turnstile": tok})).await }
    };
    for tok in [
        format!("fail:{}", next()),
        format!("ok:register:{}", next()),
        format!("badhost:login:{}", next()),
        format!("old:login:{}", next()),
        String::new(),
    ] {
        let r = try_login(tok.clone()).await;
        assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "turnstile_failed"), "{}", &tok[..tok.len().min(20)]);
    }
    let r = try_login("x".repeat(2049)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "over 2048 characters is refused before siteverify");
    let tok = ts("login");
    assert_eq!(try_login(tok.clone()).await.status, StatusCode::OK);
    let r = try_login(tok).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "turnstile_failed"), "a token works once");
    // No Turnstile check, no account work: the failed checks queued nothing and set no cookie.
    assert!(
        b.post(
            &e,
            "register",
            json!({"email": "x@example.org", "password": PW, "turnstile": format!("fail:{}", next())})
        )
        .await
        .status
            == StatusCode::BAD_REQUEST
    );
    assert!(!outbox(&e.db).await.iter().any(|(t, _, _)| t == "x@example.org"));
}

#[tokio::test]
async fn per_ip_rate_limits() {
    let Some(e) = env().await else { return };
    let mut b = Browser::new();
    for i in 0..5 {
        let r = b
            .post(
                &e,
                "register",
                json!({"email": format!("r{i}@example.org"), "password": PW, "turnstile": ts("register")}),
            )
            .await;
        assert_eq!(r.status, StatusCode::ACCEPTED);
    }
    let r =
        b.post(&e, "register", json!({"email": "r9@example.org", "password": PW, "turnstile": ts("register")})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    assert!(r.headers.contains_key(header::RETRY_AFTER));
    // Another IP is unaffected.
    let mut other = Browser::new();
    let r = other
        .post(&e, "register", json!({"email": "r9@example.org", "password": PW, "turnstile": ts("register")}))
        .await;
    assert_eq!(r.status, StatusCode::ACCEPTED);
    // Reset requests: 3 per day per address, also from different IPs.
    for i in 0..3 {
        let mut b = Browser::new();
        let r =
            b.post(&e, "password-reset/request", json!({"email": "r0@example.org", "turnstile": ts("reset")})).await;
        assert_eq!(r.status, StatusCode::ACCEPTED, "{i}");
    }
    let mut b = Browser::new();
    let r = b.post(&e, "password-reset/request", json!({"email": "R0@EXAMPLE.org", "turnstile": ts("reset")})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "case doesn't evade the per-address limit");
    // The table holds no addresses.
    let keys: Vec<Vec<u8>> = sqlx::query_scalar("SELECT key FROM rate_limits").fetch_all(&e.db).await.unwrap();
    assert!(keys.iter().all(|k| k.len() == 32));
}

#[tokio::test]
async fn password_reset() {
    let Some(e) = env().await else { return };
    let mut b1 = registered_and_verified(&e, "res@example.org", PW).await;
    assert_eq!(login(&e, &mut b1, "res@example.org", PW, None).await.status, StatusCode::OK);
    let mut anon = Browser::new();
    anon.post(&e, "password-reset/request", json!({"email": "res@example.org", "turnstile": ts("reset")})).await;
    let token = link_token(&e.db, "res@example.org", "reset").await;
    let r = anon.post(&e, "password-reset/confirm", json!({"token": token, "new_password": "password123"})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "password_common"));
    let r = anon.post(&e, "password-reset/confirm", json!({"token": token, "new_password": "res@example.org"})).await;
    assert_eq!(err(&r), "password_is_email");
    let r = anon.post(&e, "password-reset/confirm", json!({"token": token, "new_password": "short"})).await;
    assert_eq!(err(&r), "password_too_short");
    let r = anon
        .post(&e, "password-reset/confirm", json!({"token": token, "new_password": "a brand new passphrase"}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(b1.get(&e, "me").await.status, StatusCode::UNAUTHORIZED, "web sessions end");
    let r = anon
        .post(&e, "password-reset/confirm", json!({"token": token, "new_password": "another new passphrase"}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "single use");
    assert!(outbox(&e.db).await.iter().any(|(t, tpl, _)| t == "res@example.org" && tpl == "password_changed"));
    assert_eq!(login(&e, &mut anon, "res@example.org", PW, None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(login(&e, &mut anon, "res@example.org", "a brand new passphrase", None).await.status, StatusCode::OK);
    // An expired link is 410.
    anon.post(&e, "password-reset/request", json!({"email": "res@example.org", "turnstile": ts("reset")})).await;
    let token = link_token(&e.db, "res@example.org", "reset").await;
    sqlx::query("UPDATE email_tokens SET expires_at = now() - interval '1 second' WHERE kind = 'reset'")
        .execute(&e.db)
        .await
        .unwrap();
    let r = anon
        .post(&e, "password-reset/confirm", json!({"token": token, "new_password": "yet another passphrase"}))
        .await;
    assert_eq!((r.status, err(&r)), (StatusCode::GONE, "gone"));
    // Garbage tokens are a plain 400.
    let r = anon
        .post(&e, "password-reset/confirm", json!({"token": "nope", "new_password": "yet another passphrase"}))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn password_change_and_reauth() {
    let Some(e) = env().await else { return };
    let mut a = registered_and_verified(&e, "pc@example.org", PW).await;
    login(&e, &mut a, "pc@example.org", PW, None).await;
    let mut other = Browser::new();
    login(&e, &mut other, "pc@example.org", PW, None).await;
    let r = a.post(&e, "password/change", json!({"current": "wrong!", "new": "fresh passphrase 2"})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = a.post(&e, "password/change", json!({"current": PW, "new": "fresh passphrase 2"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(a.get(&e, "me").await.status, StatusCode::OK, "this session stays");
    assert_eq!(other.get(&e, "me").await.status, StatusCode::UNAUTHORIZED, "other sessions end");
    assert_eq!(a.get(&e, "me").await.body["reauth_valid"], false);
    assert_eq!(a.post(&e, "reauth", json!({"password": PW})).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(a.post(&e, "reauth", json!({"password": "fresh passphrase 2"})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(a.get(&e, "me").await.body["reauth_valid"], true);
}

#[tokio::test]
async fn totp_lifecycle() {
    let Some(e) = env().await else { return };
    let mut a = registered_and_verified(&e, "two@example.org", PW).await;
    login(&e, &mut a, "two@example.org", PW, None).await;
    assert_eq!(a.post(&e, "totp/setup", json!({"password": "wrong"})).await.status, StatusCode::UNAUTHORIZED);
    let r = a.post(&e, "totp/setup", json!({"password": PW})).await;
    assert_eq!(r.status, StatusCode::OK);
    let b32 = r.body["secret_b32"].as_str().unwrap().to_string();
    assert!(r.body["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/Krunk:two%40example.org?secret="));
    assert!(r.body["qr_svg"].as_str().unwrap().starts_with("<?xml"));
    // The secret is encrypted at rest.
    let enc: Vec<u8> = sqlx::query_scalar("SELECT totp_pending_enc FROM accounts").fetch_one(&e.db).await.unwrap();
    let secret = decode_b32(&b32);
    assert!(!enc.windows(secret.len()).any(|w| w == secret.as_slice()));
    let now = chrono::Utc::now().timestamp() as u64;
    let code = |step: u64| format!("{:06}", totp::code_at(&secret, step));
    let step = now / 30;
    assert_eq!(a.post(&e, "totp/enable", json!({"code": "000000"})).await.status, StatusCode::BAD_REQUEST);
    let r = a.post(&e, "totp/enable", json!({"code": code(step)})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let backup: Vec<String> =
        r.body["backup_codes"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(backup.len(), 10);
    assert_eq!(a.get(&e, "me").await.body["backup_codes_left"], 10);

    let mut b = Browser::new();
    let r = login(&e, &mut b, "two@example.org", PW, None).await;
    assert_eq!((r.status, err(&r)), (StatusCode::UNAUTHORIZED, "totp_required"));
    let r = login(&e, &mut b, "two@example.org", PW, Some(&code(step))).await;
    assert_eq!(
        (r.status, err(&r)),
        (StatusCode::UNAUTHORIZED, "totp_invalid"),
        "the step used to enable can't be replayed"
    );
    let r = login(&e, &mut b, "two@example.org", PW, Some(&code(step + 1))).await;
    assert_eq!(r.status, StatusCode::OK, "the next step (within the window)");
    let mut c = Browser::new();
    let r = login(&e, &mut c, "two@example.org", PW, Some(&backup[0].to_lowercase())).await;
    assert_eq!(r.status, StatusCode::OK, "a backup code, any case");
    let mut d = Browser::new();
    let r = login(&e, &mut d, "two@example.org", PW, Some(&backup[0])).await;
    assert_eq!(err(&r), "totp_invalid", "backup codes are single-use");
    assert_eq!(a.get(&e, "me").await.body["backup_codes_left"], 9);
    // Turning it off needs the password and a code.
    let r = a.post(&e, "totp/disable", json!({"password": PW, "code": "123456"})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = a.post(&e, "totp/disable", json!({"password": PW, "code": backup[1]})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let mails: Vec<String> = outbox(&e.db).await.into_iter().map(|(_, t, _)| t).collect();
    assert!(mails.contains(&"totp_enabled".to_string()) && mails.contains(&"totp_disabled".to_string()));
    let mut f = Browser::new();
    assert_eq!(login(&e, &mut f, "two@example.org", PW, None).await.status, StatusCode::OK);
}

fn decode_b32(s: &str) -> Vec<u8> {
    let (mut buf, mut bits, mut out) = (0u32, 0, Vec::new());
    for c in s.bytes() {
        let v = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".iter().position(|&x| x == c).unwrap() as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    out
}

#[tokio::test]
async fn housekeeping_and_ip_retention() {
    let Some(e) = env().await else { return };
    let mut a = registered_and_verified(&e, "hk@example.org", PW).await;
    login(&e, &mut a, "hk@example.org", PW, None).await;
    let mut u = Browser::new();
    u.post(&e, "register", json!({"email": "stale@example.org", "password": PW, "turnstile": ts("register")})).await;
    // Backdate: a session last used 91 days ago, old IP events, a stale unverified account.
    sqlx::query("INSERT INTO web_sessions (id, account_id, token_hash, created_at, last_used_at, ip)
                 SELECT gen_random_uuid(), id, '\\x01', now() - interval '20 days', now() - interval '91 days', '198.51.100.1'
                 FROM accounts WHERE email = 'hk@example.org'").execute(&e.db).await.unwrap();
    sqlx::query("UPDATE ip_events SET at = now() - interval '91 days' WHERE kind = 'register'")
        .execute(&e.db)
        .await
        .unwrap();
    sqlx::query("UPDATE accounts SET created_at = now() - interval '8 days' WHERE email = 'stale@example.org'")
        .execute(&e.db)
        .await
        .unwrap();
    jobs::run_once(&e.db, 90).await.unwrap();
    let ips: Vec<Option<String>> =
        sqlx::query_scalar("SELECT host(ip) FROM web_sessions ORDER BY last_used_at").fetch_all(&e.db).await.unwrap();
    assert_eq!(ips.len(), 1, "the 91-day-idle session is gone (dead > 30 days)");
    assert_eq!(ips[0], Some(a.ip.to_string()), "a live session keeps its IP");
    let old_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ip_events WHERE kind = 'register'").fetch_one(&e.db).await.unwrap();
    assert_eq!(old_events, 0);
    let login_events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM ip_events WHERE kind = 'login'").fetch_one(&e.db).await.unwrap();
    assert_eq!(login_events, 1);
    let stale: i64 = sqlx::query_scalar("SELECT count(*) FROM accounts WHERE email = 'stale@example.org'")
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(stale, 0);
    // IP nulling on its own: a session within its lifetime but idle past the retention.
    sqlx::query("INSERT INTO web_sessions (id, account_id, token_hash, created_at, last_used_at, ip)
                 SELECT gen_random_uuid(), id, '\\x02', now() - interval '1 day', now() - interval '2 days', '198.51.100.2'
                 FROM accounts WHERE email = 'hk@example.org'").execute(&e.db).await.unwrap();
    jobs::run_once(&e.db, 1).await.unwrap();
    let ip: Option<String> = sqlx::query_scalar("SELECT host(ip) FROM web_sessions WHERE token_hash = '\\x02'")
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(ip, None);
}

/// The logs of a whole flow contain no password, token, cookie, CSRF value, code or address.
#[tokio::test]
async fn logs_hold_no_secrets() {
    let Some(e) = env().await else { return };
    // A global subscriber (thread-local ones lose to cached callsite interest when tests run in
    // parallel). It also captures the other tests' logs, which must not hold PW either.
    let buf = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = {
        let buf = buf.clone();
        move || Capture(buf.clone())
    };
    let subscriber =
        tracing_subscriber::fmt().json().with_writer(writer).with_max_level(tracing::Level::DEBUG).finish();
    tracing::subscriber::set_global_default(subscriber).expect("only this test installs a subscriber");
    let email = "logcheck@example.org";
    let mut b = Browser::new();
    let tok = ts("register");
    b.post(&e, "register", json!({"email": email, "password": PW, "turnstile": tok})).await;
    let link = link_token(&e.db, email, "verify").await;
    b.post(&e, "verify-email", json!({"token": link, "password": PW})).await;
    login(&e, &mut b, email, "wrong password!!", None).await;
    login(&e, &mut b, email, PW, None).await;
    let r = b.post(&e, "totp/setup", json!({"password": PW})).await;
    let secret = r.body["secret_b32"].as_str().unwrap().to_string();
    b.post(&e, "totp/enable", json!({"code": "000000"})).await;
    b.post(&e, "password/change", json!({"current": PW, "new": "fresh passphrase 2"})).await;
    let s3_secrets = s3::s3_flow_secrets(&e).await;
    let s6_secrets = s6::s6_flow_secrets(&e).await;
    let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("\"request\""), "the request lines were captured: {} bytes", logs.len());
    let cookie = b.cookie.clone().unwrap();
    let csrf = b.csrf.clone().unwrap();
    for secret_value in
        [PW, "fresh passphrase 2", "wrong password!!", &tok, &link, &cookie, &csrf, &secret, email, "logcheck"]
    {
        assert!(!logs.contains(secret_value), "the logs contain {secret_value:?}");
    }
    for secret_value in s3_secrets.iter().chain(&s6_secrets) {
        assert!(!logs.contains(secret_value.as_str()), "the logs contain {secret_value:?}");
    }
    assert!(logs.contains("refresh token reuse"), "the S3 flow ran and was logged");
    assert!(
        logs.contains("keys committed") && logs.contains("approval keys delivered"),
        "the S6 flow ran and was logged"
    );
}

struct Capture(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
