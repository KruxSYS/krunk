//! One-step registration (service 2026-11-17): register → the email link → signed in; the
//! registering page's poll; the password-free verification in the browser that registered; the
//! auto-link of the Krunk the registration started from; sign-ins starting re-authenticated.

use super::s3::Device;
use super::*;

const CHROME: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

async fn register(e: &Env, b: &mut Browser, email: &str, extra: Value) -> Resp {
    let mut body = json!({"email": email, "password": PW, "turnstile": ts("register")});
    for (k, v) in extra.as_object().into_iter().flatten() {
        body[k] = v.clone();
    }
    let r = b.post(e, "register", body).await;
    assert_eq!((r.status, r.body["status"].as_str()), (StatusCode::ACCEPTED, Some("check_email")), "{}", r.raw);
    r
}

fn poll_token(r: &Resp) -> String {
    r.body["reg_poll"].as_str().unwrap().to_string()
}

async fn poll(e: &Env, b: &mut Browser, token: &str) -> Resp {
    b.post(e, "register/poll", json!({"reg_poll": token})).await
}

/// Seconds the grant behind `device_code` has left.
async fn grant_left(e: &Env, device_code: &str) -> f64 {
    let hash = crate::devices::device_code_hash(&e.state.config.keys, device_code);
    sqlx::query_scalar(
        "SELECT extract(epoch FROM expires_at - now())::float8 FROM link_grants WHERE device_code_hash = $1",
    )
    .bind(hash.as_slice())
    .fetch_one(&e.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn same_browser_verifies_without_the_password() {
    let Some(e) = env().await else { return };
    let mut b = Browser::new();
    let r = register(&e, &mut b, "same@example.org", json!({"next": "/account/#add-device"})).await;
    let set = r.headers[header::SET_COOKIE].to_str().unwrap();
    assert!(set.starts_with("__Host-krunk_reg=") && set.contains("SameSite=Lax") && set.contains("HttpOnly"));
    assert!(b.reg.is_some() && b.cookie.is_none(), "registering doesn't sign in");
    let token = poll_token(&r);
    // The registering tab (same browser) waits.
    let mut tab = b.clone();
    assert_eq!(err(&poll(&e, &mut tab, &token).await), "authorization_pending");

    let link = link_token(&e.db, "same@example.org", "verify").await;
    let r = b.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["next"], "/account/#add-device");
    assert_eq!((r.body["link"].clone(), r.body["linked"].clone()), (Value::Null, Value::Null));
    assert!(b.cookie.is_some() && b.reg.is_none(), "signed in; the registration cookie is cleared");
    let me = b.get(&e, "me").await;
    assert_eq!((me.body["email_verified"].clone(), me.body["reauth_valid"].clone()), (json!(true), json!(true)));

    // The tab gets its own session, once.
    let r = poll(&e, &mut tab, &token).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["next"], "/account/#add-device");
    assert_ne!(tab.cookie, b.cookie);
    assert_eq!(tab.get(&e, "me").await.body["reauth_valid"], true);
    assert_eq!(err(&poll(&e, &mut tab, &token).await), "expired_token", "single use");
    let via: Vec<String> = sqlx::query_scalar("SELECT via FROM web_sessions").fetch_all(&e.db).await.unwrap();
    assert_eq!(via, ["password", "password"]);
}

#[tokio::test]
async fn another_browser_needs_the_password_once() {
    let Some(e) = env().await else { return };
    let mut b = Browser::new();
    let token = poll_token(&register(&e, &mut b, "elsewhere@example.org", json!({})).await);
    let link = link_token(&e.db, "elsewhere@example.org", "verify").await;
    let mut phone = Browser::new();
    let r = phone.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::UNAUTHORIZED, "password_required"));
    let r = phone.post(&e, "verify-email", json!({"token": link, "password": ""})).await;
    assert_eq!(err(&r), "password_required", "an empty password is none");
    let r = phone.post(&e, "verify-email", json!({"token": link, "password": "wrong password!!"})).await;
    assert_eq!(err(&r), "unauthorized");
    // Asking didn't spend the link.
    let r = phone.post(&e, "verify-email", json!({"token": link, "password": PW})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(phone.get(&e, "me").await.status, StatusCode::OK);
    // The registering page signs in too.
    let r = poll(&e, &mut b, &token).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(b.get(&e, "me").await.body["email"], "elsewhere@example.org");
}

#[tokio::test]
async fn registering_from_a_krunk_tab_links_that_krunk() {
    let Some(e) = env().await else { return };
    let d = Device::new();
    let (dc, uc) = d.authorize(&e).await;
    // The /link tab: the engine's user agent, the same IP as Krunk's own client.
    let mut b = Browser::at(d.ip);
    b.ua = CHROME;
    let next = format!("/link?code={uc}");
    let token = poll_token(&register(&e, &mut b, "krunk@example.org", json!({"next": next, "link_code": uc})).await);
    assert!(grant_left(&e, &dc).await > 3500.0, "the grant waits for the mail");
    // Past the usual 10 minutes, Krunk still hears "pending".
    sqlx::query("UPDATE link_grants SET created_at = now() - interval '20 minutes'").execute(&e.db).await.unwrap();
    assert_eq!(err(&d.poll(&e, &dc).await), "authorization_pending");

    let link = link_token(&e.db, "krunk@example.org", "verify").await;
    let r = b.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["link"], "linked");
    assert_eq!(r.body["linked"], json!({"label": "Krunk on Windows 11", "platform": "windows"}));
    // Krunk's next poll gets its tokens; the device is on the account, and the mail went out.
    let r = d.poll(&e, &dc).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let devices = b.get(&e, "devices").await;
    let list = devices.body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], r.body["device_id"]);
    assert!(outbox(&e.db).await.iter().any(|(t, tpl, _)| t == "krunk@example.org" && tpl == "device_linked"));
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM ip_events ORDER BY at, kind").fetch_all(&e.db).await.unwrap();
    assert!(kinds.contains(&"device_linked".to_string()) && kinds.contains(&"email_verified".to_string()));
    // The registering tab hears the same.
    let mut tab = Browser::at(d.ip);
    tab.ua = CHROME;
    let r = poll(&e, &mut tab, &token).await;
    assert_eq!((r.body["link"].as_str(), r.body["linked"]["platform"].as_str()), (Some("linked"), Some("windows")));
}

#[tokio::test]
async fn codes_that_dont_qualify_are_ignored() {
    let Some(e) = env().await else { return };
    // A grant that a browser (not Krunk's own client) asked for.
    let mut not_krunk = Device::new();
    not_krunk.ua = CHROME;
    let (dc1, uc1) = not_krunk.authorize(&e).await;
    let mut b1 = Browser::at(not_krunk.ip);
    register(&e, &mut b1, "nk@example.org", json!({"link_code": uc1})).await;
    // A Krunk grant, but the registration comes from elsewhere (a code sent to a victim).
    let d = Device::new();
    let (dc2, uc2) = d.authorize(&e).await;
    let mut b2 = Browser::new();
    register(&e, &mut b2, "ip@example.org", json!({"link_code": uc2})).await;
    for (dc, b, email) in [(&dc1, &mut b1, "nk@example.org"), (&dc2, &mut b2, "ip@example.org")] {
        assert!(grant_left(&e, dc).await < 700.0, "{email}: not extended");
        let link = link_token(&e.db, email, "verify").await;
        let r = b.post(&e, "verify-email", json!({"token": link})).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
        assert_eq!(r.body["link"], Value::Null, "{email}");
    }
    assert_eq!(err(&d.poll(&e, &dc2).await), "authorization_pending", "the grant waits for /link as always");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM devices").fetch_one(&e.db).await.unwrap();
    assert_eq!(n, 0);
    // And a code that doesn't exist, or isn't one.
    let mut b3 = Browser::new();
    register(&e, &mut b3, "junk@example.org", json!({"link_code": "ZZZZ-ZZZZ"})).await;
    register(&e, &mut b3, "junk2@example.org", json!({"link_code": "not a code", "next": "https://evil.example/"}))
        .await;
    let link = link_token(&e.db, "junk2@example.org", "verify").await;
    let r = b3.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!((r.body["next"].clone(), r.body["link"].clone()), (Value::Null, Value::Null));
}

#[tokio::test]
async fn an_expired_denied_or_full_link_still_verifies() {
    let Some(e) = env().await else { return };
    let d = Device::new();
    let mut cases = Vec::new();
    for email in ["exp@example.org", "deny@example.org", "full@example.org"] {
        let (dc, uc) = d.authorize(&e).await;
        let mut b = Browser::at(d.ip);
        register(&e, &mut b, email, json!({"link_code": uc})).await;
        cases.push((dc, uc, b, email));
    }
    // Expired meanwhile.
    let hash = crate::devices::device_code_hash(&e.state.config.keys, &cases[0].0);
    sqlx::query("UPDATE link_grants SET expires_at = now() - interval '1 second' WHERE device_code_hash = $1")
        .bind(hash.as_slice())
        .execute(&e.db)
        .await
        .unwrap();
    // Denied on /link by someone signed in.
    let mut other = registered_and_verified(&e, "someone@example.org", PW).await;
    login(&e, &mut other, "someone@example.org", PW, None).await;
    let uc = cases[1].1.clone();
    assert_eq!(other.post(&e, &format!("link/{uc}/deny"), json!({})).await.status, StatusCode::NO_CONTENT);
    // The account is somehow full already.
    sqlx::query(
        "INSERT INTO devices (id, account_id, short, platform, label, client_version)
         SELECT gen_random_uuid(), a.id, n, 'windows', 'old', '0.1.0' FROM accounts a, generate_series(0, $1 - 1) n
         WHERE a.email = 'full@example.org'",
    )
    .bind(e.state.config.quota_max_devices)
    .execute(&e.db)
    .await
    .unwrap();
    for (_, _, b, email) in cases.iter_mut() {
        let link = link_token(&e.db, email, "verify").await;
        let r = b.post(&e, "verify-email", json!({"token": link})).await;
        assert_eq!(r.status, StatusCode::OK, "{email}: {}", r.raw);
        assert_eq!((r.body["link"].as_str(), r.body["linked"].clone()), (Some("unavailable"), Value::Null), "{email}");
        assert_eq!(b.get(&e, "me").await.body["email_verified"], true, "{email}");
    }
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM devices WHERE label <> 'old'").fetch_one(&e.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn every_address_gets_the_same_registration() {
    let Some(e) = env().await else { return };
    registered_and_verified(&e, "known@example.org", PW).await;
    let mut pending = Browser::new();
    register(&e, &mut pending, "pending@example.org", json!({})).await;
    let d = Device::new();
    let mut answers = Vec::new();
    for email in ["known@example.org", "pending@example.org", "fresh@example.org"] {
        let (dc, uc) = d.authorize(&e).await;
        let mut b = Browser::at(d.ip);
        let r = register(&e, &mut b, email, json!({"link_code": uc})).await;
        assert!(b.reg.is_some(), "{email}: the cookie");
        assert!(grant_left(&e, &dc).await > 3500.0, "{email}: the grant is extended alike");
        let token = poll_token(&r);
        assert_eq!(err(&poll(&e, &mut b, &token).await), "authorization_pending", "{email}");
        let mut keys = keys_of(&r.body);
        keys.sort();
        answers.push((r.status, keys, token.len()));
    }
    assert!(answers.windows(2).all(|w| w[0] == w[1]), "{answers:?}");
    // The known address's registration never completes, even once its owner signs in.
    let mut owner = Browser::new();
    assert_eq!(login(&e, &mut owner, "known@example.org", PW, None).await.status, StatusCode::OK);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM registrations WHERE account_id IS NULL")
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(err(&poll(&e, &mut Browser::new(), &crate::crypto::new_token()).await), "expired_token");
    assert_eq!(err(&poll(&e, &mut Browser::new(), "junk").await), "expired_token");
}

fn keys_of(v: &Value) -> Vec<String> {
    v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

#[tokio::test]
async fn an_earlier_registration_of_the_address_is_void() {
    let Some(e) = env().await else { return };
    // The attacker registers the victim's address, then the victim does.
    let mut attacker = Browser::new();
    let attacker_poll = poll_token(&b_register(&e, &mut attacker, "victim@example.org", "attacker pass 1").await);
    let mut victim = Browser::new();
    register(&e, &mut victim, "victim@example.org", json!({})).await;
    let link = link_token(&e.db, "victim@example.org", "verify").await;
    // Even holding the victim's link, the attacker's cookie is no password.
    let r = attacker.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!(err(&r), "password_required");
    // Nor is a cookie of another account's registration.
    let mut stranger = Browser::new();
    register(&e, &mut stranger, "stranger@example.org", json!({})).await;
    assert_eq!(err(&stranger.post(&e, "verify-email", json!({"token": link})).await), "password_required");
    let r = victim.post(&e, "verify-email", json!({"token": link})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    // The attacker's page never signs in.
    assert_eq!(err(&poll(&e, &mut attacker, &attacker_poll).await), "authorization_pending");
    assert!(attacker.cookie.is_none());
}

async fn b_register(e: &Env, b: &mut Browser, email: &str, pw: &str) -> Resp {
    let r = b.post(e, "register", json!({"email": email, "password": pw, "turnstile": ts("register")})).await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.raw);
    r
}

#[tokio::test]
async fn link_approval_right_after_sign_in_needs_no_reauth() {
    let Some(e) = env().await else { return };
    let mut b = registered_and_verified(&e, "existing@example.org", PW).await;
    assert_eq!(login(&e, &mut b, "existing@example.org", PW, None).await.status, StatusCode::OK);
    let d = Device::new();
    let (dc, uc) = d.authorize(&e).await;
    assert_eq!(b.get(&e, &format!("link/{uc}")).await.status, StatusCode::OK);
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(d.poll(&e, &dc).await.status, StatusCode::OK);
    // The manual path still wants the code confirmed.
    let (_, uc2) = d.authorize(&e).await;
    let r = b.post(&e, &format!("link/{uc2}/approve"), json!({"confirm_code": "ZZZZ-ZZZZ"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn registrations_are_cleaned_up() {
    let Some(e) = env().await else { return };
    for email in ["old1@example.org", "old2@example.org", "live@example.org"] {
        register(&e, &mut Browser::new(), email, json!({})).await;
    }
    sqlx::query(
        "UPDATE registrations SET expires_at = now() - interval '25 hours'
         WHERE account_id = (SELECT id FROM accounts WHERE email = 'old1@example.org')",
    )
    .execute(&e.db)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE registrations SET verified_at = now() - interval '25 hours'
         WHERE account_id = (SELECT id FROM accounts WHERE email = 'old2@example.org')",
    )
    .execute(&e.db)
    .await
    .unwrap();
    jobs::run_once(&e.db, 90).await.unwrap();
    let left: Vec<String> = sqlx::query_scalar(
        "SELECT a.email::text FROM registrations r JOIN accounts a ON a.id = r.account_id ORDER BY 1",
    )
    .fetch_all(&e.db)
    .await
    .unwrap();
    assert_eq!(left, ["live@example.org"]);
}
