//! Integration tests of milestone S3 (PLAN §9): device linking in every RFC 8628 state, the
//! code brute-force limits, refresh rotation and reuse detection, immediate revocation, the
//! devices and sessions lists, the S3 housekeeping, and S2 data surviving migration 0003.

use super::*;

/// A native client (Krunk): no cookies, no Origin; a bearer token when it has one.
pub(super) struct Device {
    pub ip: IpAddr,
}

impl Device {
    pub fn new() -> Device {
        let n = next();
        Device { ip: format!("198.18.{}.{}", n / 250, n % 250 + 1).parse().unwrap() }
    }

    pub async fn call(&self, e: &Env, method: Method, path: &str, body: Option<Value>, bearer: Option<&str>) -> Resp {
        let mut b = Request::builder()
            .method(method.clone())
            .uri(format!("/api/v1/{path}"))
            .header(header::USER_AGENT, "Krunk/0.8.0 (it-test)");
        if method != Method::GET {
            b = b.header(header::CONTENT_TYPE, "application/json");
        }
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let mut req = b.body(body.map(|v| Body::from(v.to_string())).unwrap_or_else(Body::empty)).unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::new(self.ip, 50000)));
        let res = router(e.state.clone()).oneshot(req).await.unwrap();
        let (parts, body) = res.into_parts();
        let bytes = axum::body::to_bytes(body, 1 << 20).await.unwrap();
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let body: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        Resp { status: parts.status, headers: parts.headers, body, raw }
    }

    pub async fn post(&self, e: &Env, path: &str, body: Value) -> Resp {
        self.call(e, Method::POST, path, Some(body), None).await
    }

    pub async fn authorize(&self, e: &Env) -> (String, String) {
        let r = self
            .post(
                e,
                "link/authorize",
                json!({"platform": "windows", "label": "Krunk 0.8.0 on Windows 11", "client_version": "0.8.0"}),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
        (r.body["device_code"].as_str().unwrap().to_string(), r.body["user_code"].as_str().unwrap().to_string())
    }

    pub async fn poll(&self, e: &Env, device_code: &str) -> Resp {
        // The poll interval is the client's job; tests move the last poll back instead of waiting.
        sqlx::query("UPDATE link_grants SET last_poll_at = last_poll_at - interval '1 minute'")
            .execute(&e.db)
            .await
            .unwrap();
        self.post(e, "link/token", json!({"device_code": device_code})).await
    }
}

fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    k.sort();
    k
}

/// A signed-in website user who re-authenticated just now.
pub(super) async fn signed_in(e: &Env, email: &str) -> Browser {
    let mut b = registered_and_verified(e, email, PW).await;
    let r = login(e, &mut b, email, PW, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let r = b.post(e, "reauth", json!({"password": PW})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.raw);
    b
}

/// Links a new device to the signed-in user; returns `(device_id, access, refresh, codes)`.
pub(super) async fn linked(e: &Env, b: &mut Browser, d: &Device) -> (String, String, String, (String, String)) {
    let (dc, uc) = d.authorize(e).await;
    let r = b.post(e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let r = d.poll(e, &dc).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let s = |k: &str| r.body[k].as_str().unwrap().to_string();
    (s("device_id"), s("access_token"), s("refresh_token"), (dc, uc))
}

#[tokio::test]
async fn link_flow_in_every_state() {
    let Some(e) = env().await else { return };
    let d = Device::new();
    // Input checks.
    for bad in [
        json!({"platform": "linux", "label": "x", "client_version": "1"}),
        json!({"platform": "windows", "label": "two\nlines", "client_version": "1"}),
        json!({"platform": "windows", "label": "", "client_version": "1"}),
        json!({"platform": "windows", "label": "x".repeat(65), "client_version": "1"}),
        json!({"platform": "windows", "label": "x", "client_version": "1", "extra": 1}),
    ] {
        let r = d.post(&e, "link/authorize", bad.clone()).await;
        assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "bad_request"), "{bad}");
    }
    // A browser page of another site can't use the device endpoints.
    let mut req = Request::post("/api/v1/link/authorize")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"platform": "windows", "label": "x", "client_version": "1"}).to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(d.ip, 1)));
    assert_eq!(router(e.state.clone()).oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);

    let r = d
        .post(
            &e,
            "link/authorize",
            json!({"platform": "windows", "label": " Krunk 0.8.0 on Windows 11 ", "client_version": "0.8.0"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(
        keys_of(&r.body),
        ["device_code", "expires_in", "interval", "user_code", "verification_uri", "verification_uri_complete"],
        "exactly the documented fields (no key material)"
    );
    assert_eq!((r.body["expires_in"].as_i64(), r.body["interval"].as_i64()), (Some(600), Some(5)));
    let dc = r.body["device_code"].as_str().unwrap().to_string();
    let uc = r.body["user_code"].as_str().unwrap().to_string();
    assert!(crate::crypto::b64url_decode::<32>(&dc).is_some(), "32 random bytes");
    assert_eq!(
        crate::devices::normalize_user_code(&uc).map(|n| crate::devices::display_user_code(&n)),
        Some(uc.clone())
    );
    assert_eq!(r.body["verification_uri"], "https://krunk.209.wf/link");
    assert_eq!(r.body["verification_uri_complete"], format!("https://krunk.209.wf/link?code={uc}"));
    // Only hashes are stored.
    let (dh, uh): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT device_code_hash, user_code_hash FROM link_grants").fetch_one(&e.db).await.unwrap();
    assert_ne!(dh, crate::crypto::b64url_decode::<32>(&dc).unwrap().to_vec());
    assert!(!String::from_utf8_lossy(&uh).contains(&uc[..4]));

    // Pending, then too fast: slow_down, and the interval grows by 5 s each time.
    let r = d.post(&e, "link/token", json!({"device_code": dc})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "authorization_pending"));
    let r = d.post(&e, "link/token", json!({"device_code": dc})).await;
    assert_eq!(err(&r), "slow_down");
    let r = d.post(&e, "link/token", json!({"device_code": dc})).await;
    assert_eq!(err(&r), "slow_down");
    let interval: i32 = sqlx::query_scalar("SELECT interval_s FROM link_grants").fetch_one(&e.db).await.unwrap();
    assert_eq!(interval, 15);
    sqlx::query("UPDATE link_grants SET last_poll_at = now() - interval '14 seconds'").execute(&e.db).await.unwrap();
    assert_eq!(err(&d.post(&e, "link/token", json!({"device_code": dc})).await), "slow_down", "14 s < 15 s");
    assert_eq!(err(&d.poll(&e, &dc).await), "authorization_pending");
    let r = d.post(&e, "link/token", json!({"device_code": crate::crypto::new_token()})).await;
    assert_eq!(err(&r), "expired_token", "an unknown code");
    assert_eq!(err(&d.post(&e, "link/token", json!({"device_code": "short"})).await), "bad_request");

    // The website: a session is needed, then a re-auth, then the same code again.
    let mut anon = Browser::new();
    assert_eq!(anon.get(&e, &format!("link/{uc}")).await.status, StatusCode::UNAUTHORIZED);
    let mut b = registered_and_verified(&e, "linker@example.org", PW).await;
    login(&e, &mut b, "linker@example.org", PW, None).await;
    let typed = uc.to_lowercase().replace('-', " ");
    let r = b.get(&e, &format!("link/{}", typed.replace(' ', "%20"))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(keys_of(&r.body), ["age_s", "country", "label", "platform", "user_code", "your_country"]);
    assert_eq!(
        (r.body["label"].as_str(), r.body["platform"].as_str()),
        (Some("Krunk 0.8.0 on Windows 11"), Some("windows"))
    );
    assert_eq!(r.body["user_code"], uc.as_str());
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "reauth_required"));
    assert_eq!(b.post(&e, "reauth", json!({"password": PW})).await.status, StatusCode::NO_CONTENT);
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": "ZZZZ-ZZZZ"})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "bad_request"), "a different confirmed code");
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc.to_lowercase()})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(keys_of(&r.body), ["device_id", "short"]);
    assert_eq!(r.body["short"], 0);
    let mails = outbox(&e.db).await;
    let (to, tpl, p) = mails.last().unwrap();
    assert_eq!((to.as_str(), tpl.as_str()), ("linker@example.org", "device_linked"));
    assert_eq!(p["label"], "Krunk 0.8.0 on Windows 11");
    // Approved once: the code is gone from the website.
    assert_eq!(b.get(&e, &format!("link/{uc}")).await.status, StatusCode::NOT_FOUND);

    // The tokens, once.
    let r = d.poll(&e, &dc).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(keys_of(&r.body), ["access_token", "account_id", "device_id", "expires_in", "refresh_token"]);
    assert_eq!(r.body["expires_in"], 900);
    let access = r.body["access_token"].as_str().unwrap().to_string();
    assert_eq!(err(&d.poll(&e, &dc).await), "expired_token", "single use");
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM link_grants").fetch_one(&e.db).await.unwrap();
    assert_eq!(grants, 0, "the grant is deleted once its tokens were issued");
    let me = d.call(&e, Method::GET, "device/me", None, Some(&access)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.raw);
    assert_eq!(keys_of(&me.body), ["account_id", "created", "device_id", "email", "label", "platform"]);
    assert_eq!(me.body["email"], "linker@example.org");
    assert_eq!(me.body["device_id"], r.body["device_id"]);
    assert_eq!(d.call(&e, Method::GET, "device/me", None, None).await.status, StatusCode::UNAUTHORIZED);
    let wrong = crate::crypto::new_token();
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&wrong)).await.status, StatusCode::UNAUTHORIZED);
    // The access token expires after 15 minutes.
    sqlx::query("UPDATE access_tokens SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(b.get(&e, "me").await.body["devices"], 1);

    // Denied: the next poll says so once; then the code is gone. (A second website session: the
    // first one has used its 5 code lookups.)
    let mut b = Browser::new();
    login(&e, &mut b, "linker@example.org", PW, None).await;
    b.post(&e, "reauth", json!({"password": PW})).await;
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "approved codes can't be approved again");
    let (dc2, uc2) = d.authorize(&e).await;
    assert_eq!(b.post(&e, &format!("link/{uc2}/deny"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(err(&d.poll(&e, &dc2).await), "access_denied");
    assert_eq!(err(&d.poll(&e, &dc2).await), "expired_token");

    // Expired after 10 minutes: the website no longer shows it and the poll says expired_token.
    let (dc3, uc3) = d.authorize(&e).await;
    sqlx::query("UPDATE link_grants SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    assert_eq!(b.get(&e, &format!("link/{uc3}")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(err(&d.poll(&e, &dc3).await), "expired_token");
    assert_eq!(err(&d.poll(&e, &dc3).await), "expired_token");
}

#[tokio::test]
async fn code_lookups_are_limited() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "guess@example.org").await;
    for i in 0..5 {
        let r = b.get(&e, &format!("link/AAAA-AAA{i}")).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.raw);
    }
    let r = b.get(&e, "link/AAAA-AAA9").await;
    assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"), "6th lookup in a session");
    assert!(r.headers.contains_key(header::RETRY_AFTER));
    // Approve and deny with an unknown code count as lookups too, and are refused once the
    // limit is used up.
    let r = b.post(&e, "link/AAAA-AAAA/deny", json!({})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    let mut fresh = Browser { ip: "203.0.113.200".parse().unwrap(), cookie: None, csrf: None };
    login(&e, &mut fresh, "guess@example.org", PW, None).await;
    fresh.post(&e, "reauth", json!({"password": PW})).await;
    for i in 0..5 {
        let r =
            fresh.post(&e, &format!("link/CCCC-CCC{i}/approve"), json!({"confirm_code": format!("CCCC-CCC{i}")})).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.raw);
    }
    assert_eq!(fresh.get(&e, "link/CCCC-CCC9").await.status, StatusCode::TOO_MANY_REQUESTS);
    // Acting on a code that exists costs nothing more: view it, approve after a re-auth, view
    // and approve more codes in the same session.
    // (Another account: approvals are also limited to 10 an hour per account.)
    let mut owner = registered_and_verified(&e, "owner2@example.org", PW).await;
    login(&e, &mut owner, "owner2@example.org", PW, None).await;
    let d = Device::new();
    // Five views fill the session's window; the fifth code can still be approved.
    for i in 0..5 {
        let (_, uc) = d.authorize(&e).await;
        assert_eq!(owner.get(&e, &format!("link/{uc}")).await.status, StatusCode::OK);
        if i == 0 {
            let r = owner.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
            assert_eq!(err(&r), "reauth_required");
            owner.post(&e, "reauth", json!({"password": PW})).await;
        }
        let r = owner.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    }
    // Per IP: 20 an hour across sessions. A lookup refused by the session limit doesn't count.
    let ip = b.ip;
    let mut answered = 5;
    let mut limited = false;
    for _ in 0..4 {
        let mut s = Browser { ip, cookie: None, csrf: None };
        let r = login(&e, &mut s, "guess@example.org", PW, None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
        for _ in 0..5 {
            let r = s.get(&e, "link/BBBB-BBBB").await;
            if r.status == StatusCode::TOO_MANY_REQUESTS {
                limited = true;
                break;
            }
            assert_eq!(r.status, StatusCode::NOT_FOUND);
            answered += 1;
        }
        if limited {
            break;
        }
    }
    assert!(limited, "the per-IP limit applies across sessions");
    assert_eq!(answered, 20);
    // Authorize: 10 an hour per IP.
    let d = Device::new();
    for _ in 0..10 {
        d.authorize(&e).await;
    }
    let r = d.post(&e, "link/authorize", json!({"platform": "windows", "label": "x", "client_version": "1"})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn refresh_rotates_and_reuse_revokes_the_family() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "rot@example.org").await;
    let d = Device::new();
    let (id, _a1, r1, _) = linked(&e, &mut b, &d).await;
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT current_hash FROM refresh_families").fetch_one(&e.db).await.unwrap();
    assert_ne!(stored, crate::crypto::b64url_decode::<32>(&r1).unwrap().to_vec(), "only an HMAC is stored");
    let r = d.post(&e, "token/refresh", json!({"refresh_token": r1})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(keys_of(&r.body), ["access_token", "expires_in", "refresh_token"]);
    let r2 = r.body["refresh_token"].as_str().unwrap().to_string();
    assert_ne!(r2, r1, "rotated");
    let r = d.post(&e, "token/refresh", json!({"refresh_token": r2})).await;
    assert_eq!(r.status, StatusCode::OK);
    let r3 = r.body["refresh_token"].as_str().unwrap().to_string();
    let a3 = r.body["access_token"].as_str().unwrap().to_string();
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&a3)).await.status, StatusCode::OK);
    // A thief replays r1: the family and the device are revoked, the owner is told.
    let mails_before = outbox(&e.db).await.len();
    let r = d.post(&e, "token/refresh", json!({"refresh_token": r1})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::UNAUTHORIZED, "unauthorized"));
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&a3)).await.status, StatusCode::UNAUTHORIZED, "at once");
    assert_eq!(d.post(&e, "token/refresh", json!({"refresh_token": r3})).await.status, StatusCode::UNAUTHORIZED);
    let mails = outbox(&e.db).await;
    assert_eq!(mails.len(), mails_before + 1);
    assert_eq!(
        (mails.last().unwrap().0.as_str(), mails.last().unwrap().1.as_str()),
        ("rot@example.org", "token_reuse")
    );
    // A second replay sends no second mail.
    d.post(&e, "token/refresh", json!({"refresh_token": r2})).await;
    assert_eq!(outbox(&e.db).await.len(), mails_before + 1);
    let devs = b.get(&e, "devices").await;
    assert_eq!(devs.body[0]["id"], id.as_str());
    assert!(devs.body[0]["revoked_at"].is_string());
    assert_eq!(devs.body[0]["has_keys"], false);
    // Garbage and unknown tokens: the generic 401.
    for t in ["x", &crate::crypto::new_token()] {
        assert_eq!(d.post(&e, "token/refresh", json!({"refresh_token": t})).await.status, StatusCode::UNAUTHORIZED);
    }
    // 90 days unused: expired.
    let mut b2 = signed_in(&e, "idle@example.org").await;
    let (_, _, r1, _) = linked(&e, &mut b2, &d).await;
    sqlx::query("UPDATE refresh_families SET rotated_at = now() - interval '91 days'").execute(&e.db).await.unwrap();
    assert_eq!(d.post(&e, "token/refresh", json!({"refresh_token": r1})).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revoking_rename_and_sessions() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "owner@example.org").await;
    let d = Device::new();
    let (id, access, refresh, _) = linked(&e, &mut b, &d).await;
    let (id2, access2, _, _) = linked(&e, &mut b, &d).await;
    let devs = b.get(&e, "devices").await;
    assert_eq!(devs.body.as_array().unwrap().len(), 2);
    assert_eq!(
        keys_of(&devs.body[0]),
        [
            "client_version",
            "created",
            "has_keys",
            "id",
            "label",
            "last_country",
            "last_sync",
            "last_used",
            "platform",
            "revoked_at",
            "short"
        ]
    );
    let shorts: Vec<i64> = devs.body.as_array().unwrap().iter().map(|x| x["short"].as_i64().unwrap()).collect();
    assert!(shorts.contains(&0) && shorts.contains(&1), "{shorts:?}");

    // Rename (no re-auth needed); the device sees the new name.
    let r = b.post(&e, &format!("devices/{id}/rename"), json!({"label": "  Gaming PC  "})).await;
    assert_eq!((r.status, r.body["label"].as_str()), (StatusCode::OK, Some("Gaming PC")));
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access)).await.body["label"], "Gaming PC");
    assert_eq!(
        b.post(&e, &format!("devices/{id}/rename"), json!({"label": "a\u{7}b"})).await.status,
        StatusCode::BAD_REQUEST
    );
    // Someone else's device is not found.
    let mut other = signed_in(&e, "other@example.org").await;
    assert_eq!(
        other.post(&e, &format!("devices/{id}/rename"), json!({"label": "mine"})).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(other.post(&e, &format!("devices/{id}/revoke"), json!({})).await.status, StatusCode::NOT_FOUND);
    assert_eq!(other.post(&e, "devices/not-a-uuid/revoke", json!({})).await.status, StatusCode::NOT_FOUND);

    // Sessions: this website session (current), the other one, and both devices.
    let mut second = Browser::new();
    login(&e, &mut second, "owner@example.org", PW, None).await;
    let s = b.get(&e, "sessions").await;
    let items = s.body.as_array().unwrap();
    assert_eq!(items.iter().filter(|x| x["kind"] == "web").count(), 2);
    assert_eq!(items.iter().filter(|x| x["kind"] == "device").count(), 2);
    assert_eq!(items.iter().filter(|x| x["current"] == true).count(), 1);
    let web_other = items.iter().find(|x| x["kind"] == "web" && x["current"] == false).unwrap();
    assert!(web_other["ip_masked"].as_str().unwrap().ends_with(".0/24"));
    let r = b.post(&e, &format!("sessions/{}/revoke", web_other["id"].as_str().unwrap()), json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(second.get(&e, "me").await.status, StatusCode::UNAUTHORIZED, "that session ended");

    // Revoke from the website: re-auth needed, then the tokens die at once.
    sqlx::query("UPDATE web_sessions SET reauth_at = now() - interval '11 minutes'").execute(&e.db).await.unwrap();
    let r = b.post(&e, &format!("devices/{id}/revoke"), json!({})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "reauth_required"));
    b.post(&e, "reauth", json!({"password": PW})).await;
    let r = b.post(&e, &format!("devices/{id}/revoke"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(d.post(&e, "token/refresh", json!({"refresh_token": refresh})).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(outbox(&e.db).await.last().unwrap().1, "device_revoked");
    assert_eq!(b.post(&e, &format!("devices/{id}/rename"), json!({"label": "x"})).await.status, StatusCode::NOT_FOUND);
    assert_eq!(b.get(&e, "me").await.body["devices"], 1);

    // Unlink from the device itself.
    let r = d.call(&e, Method::POST, "device/unlink", Some(json!({})), Some(&access2)).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access2)).await.status, StatusCode::UNAUTHORIZED);
    let revoked: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT revoked_at FROM devices WHERE id = $1::uuid")
            .bind(&id2)
            .fetch_one(&e.db)
            .await
            .unwrap();
    assert!(revoked.is_some());
    assert_eq!(b.get(&e, "me").await.body["devices"], 0);

    // A device revoked through the sessions list (re-auth still fresh).
    let (id3, access3, _, _) = linked(&e, &mut b, &d).await;
    assert_eq!(b.post(&e, &format!("sessions/{id3}/revoke"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access3)).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn device_quota() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "many@example.org").await;
    sqlx::query(
        "INSERT INTO devices (id, account_id, short, platform, label, client_version)
         SELECT gen_random_uuid(), a.id, n, 'windows', 'old ' || n, '0.8.0' FROM accounts a, generate_series(0, 9) n
         WHERE a.email = 'many@example.org'",
    )
    .execute(&e.db)
    .await
    .unwrap();
    let d = Device::new();
    let (_, uc) = d.authorize(&e).await;
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "quota_exceeded"));
    sqlx::query("UPDATE devices SET revoked_at = now() WHERE short = 3").execute(&e.db).await.unwrap();
    let r = b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["short"], 10, "shorts of revoked devices are not reused while they exist");
}

#[tokio::test]
async fn s3_housekeeping() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "hk3@example.org").await;
    let d = Device::new();
    let (live, _, _, _) = linked(&e, &mut b, &d).await;
    let (idle, _, _, _) = linked(&e, &mut b, &d).await;
    let (old_revoked, _, _, _) = linked(&e, &mut b, &d).await;
    // An approved link whose tokens were never collected, and an old pending grant.
    let (_, uc) = d.authorize(&e).await;
    b.post(&e, &format!("link/{uc}/approve"), json!({"confirm_code": uc})).await;
    d.authorize(&e).await;
    sqlx::query("UPDATE link_grants SET expires_at = now() - interval '11 minutes'").execute(&e.db).await.unwrap();
    sqlx::query("UPDATE devices SET created_at = now() - interval '31 minutes'").execute(&e.db).await.unwrap();
    sqlx::query(
        "UPDATE devices SET last_used_at = now() - interval '91 days', last_ip = '198.51.100.7' WHERE id = $1::uuid",
    )
    .bind(&live)
    .execute(&e.db)
    .await
    .unwrap();
    sqlx::query("UPDATE refresh_families SET rotated_at = now() - interval '91 days' WHERE device_id = $1::uuid")
        .bind(&idle)
        .execute(&e.db)
        .await
        .unwrap();
    sqlx::query("UPDATE devices SET revoked_at = now() - interval '31 days' WHERE id = $1::uuid")
        .bind(&old_revoked)
        .execute(&e.db)
        .await
        .unwrap();
    sqlx::query("UPDATE access_tokens SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    jobs::run_once(&e.db, 90).await.unwrap();
    let grants: i64 = sqlx::query_scalar("SELECT count(*) FROM link_grants").fetch_one(&e.db).await.unwrap();
    assert_eq!(grants, 0);
    let ids: Vec<(String, Option<String>, bool)> =
        sqlx::query_as("SELECT id::text, host(last_ip), revoked_at IS NOT NULL FROM devices ORDER BY created_at")
            .fetch_all(&e.db)
            .await
            .unwrap();
    assert_eq!(ids.len(), 2, "the uncollected and the long-revoked devices are gone: {ids:?}");
    let live_row = ids.iter().find(|(id, _, _)| *id == live).unwrap();
    assert_eq!((live_row.1.as_deref(), live_row.2), (None, false), "IP nulled after 90 days, still linked");
    let idle_row = ids.iter().find(|(id, _, _)| *id == idle).unwrap();
    assert!(idle_row.2, "90 days without a refresh: signed out");
    let tokens: i64 = sqlx::query_scalar("SELECT count(*) FROM access_tokens").fetch_one(&e.db).await.unwrap();
    assert_eq!(tokens, 0);
}

/// Migration 0003 is additive: an S2 database (0001–0002) with accounts, sessions, codes and
/// mail migrates, and its users still sign in.
#[tokio::test]
async fn s2_data_survives_migration_0003() {
    let Some(e) = env().await else { return };
    // A second, empty database migrated only to 0002, like the live one before this release.
    let admin_url = std::env::var("KRUNK_TEST_DATABASE_URL").unwrap();
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url).await.unwrap();
    let name = format!("krunk_it_s2_{}_{}", std::process::id(), next());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').unwrap().0);
    let db = PgPoolOptions::new().max_connections(5).connect(&url).await.unwrap();
    crate::db::MIGRATOR.run_to(2, &db).await.unwrap();
    let has_devices: bool =
        sqlx::query_scalar("SELECT to_regclass('devices') IS NOT NULL").fetch_one(&db).await.unwrap();
    assert!(!has_devices, "S2 schema only");
    // An S2 account made through the S2 code path, then the S3 migration.
    let mut state = e.state.clone();
    let mut cfg = (*state.config).clone();
    cfg.database_url = url.clone();
    state.config = Arc::new(cfg);
    state.db = db.clone();
    let s2 = Env { state, db: db.clone() };
    let mut user = registered_and_verified(&s2, "survivor@example.org", PW).await;
    let r = login(&s2, &mut user, "survivor@example.org", PW, None).await;
    assert_eq!(r.status, StatusCode::OK);
    let before: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM web_sessions), (SELECT count(*) FROM email_outbox)",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    crate::db::migrate(&db).await.unwrap();
    let after: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM web_sessions), (SELECT count(*) FROM email_outbox)",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(before, after);
    // The old session still works and the user can link a device.
    assert_eq!(user.get(&s2, "me").await.body["email"], "survivor@example.org");
    let mut fresh = Browser::new();
    assert_eq!(login(&s2, &mut fresh, "survivor@example.org", PW, None).await.status, StatusCode::OK);
    fresh.post(&s2, "reauth", json!({"password": PW})).await;
    let (_, access, _, _) = linked(&s2, &mut fresh, &Device::new()).await;
    assert_eq!(Device::new().call(&s2, Method::GET, "device/me", None, Some(&access)).await.status, StatusCode::OK);
}

/// The S3 part of the log-redaction flow (called from `logs_hold_no_secrets`); returns every
/// secret value it used.
pub(super) async fn s3_flow_secrets(e: &Env) -> Vec<String> {
    let mut b = signed_in(e, "s3log@example.org").await;
    let d = Device::new();
    let (_, access, refresh, (dc, uc)) = linked(e, &mut b, &d).await;
    let r = d.post(e, "token/refresh", json!({"refresh_token": refresh})).await;
    let refresh2 = r.body["refresh_token"].as_str().unwrap().to_string();
    let access2 = r.body["access_token"].as_str().unwrap().to_string();
    d.call(e, Method::GET, "device/me", None, Some(&access2)).await;
    // Reuse (revokes the device and logs a warning line).
    d.post(e, "token/refresh", json!({"refresh_token": refresh})).await;
    let (_, uc2) = d.authorize(e).await;
    b.get(e, &format!("link/{uc2}")).await;
    let norm = |c: &str| c.replace('-', "");
    vec![access, refresh, refresh2, access2, dc, uc.clone(), norm(&uc), uc2.clone(), norm(&uc2), "s3log".into()]
}

/// 0.9.0: "Sign out everywhere else" (`web/sessions/revoke-all`). Re-auth; the other website
/// sessions end and this one stays; devices only with `include_devices` (each a mail); the limit.
#[tokio::test]
async fn sign_out_everywhere_else() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "owner@example.org").await;
    let d = Device::new();
    let (_id, access, _, _) = linked(&e, &mut b, &d).await;
    let mut second = Browser::new();
    login(&e, &mut second, "owner@example.org", PW, None).await;
    let mut third = Browser::new();
    login(&e, &mut third, "owner@example.org", PW, None).await;
    // Someone else's session is never touched.
    let mut other = signed_in(&e, "other@example.org").await;

    sqlx::query("UPDATE web_sessions SET reauth_at = now() - interval '11 minutes'").execute(&e.db).await.unwrap();
    let r = b.post(&e, "sessions/revoke-all", json!({})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "reauth_required"));
    assert_eq!(second.get(&e, "me").await.status, StatusCode::OK, "nothing ended without re-auth");
    assert_eq!(b.post(&e, "sessions/revoke-all", json!({"bogus": 1})).await.status, StatusCode::BAD_REQUEST);

    b.post(&e, "reauth", json!({"password": PW})).await;
    let mails = outbox(&e.db).await.len();
    let r = b.post(&e, "sessions/revoke-all", json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.raw);
    assert_eq!(second.get(&e, "me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(third.get(&e, "me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(b.get(&e, "me").await.status, StatusCode::OK, "this session stays");
    assert_eq!(other.get(&e, "me").await.status, StatusCode::OK, "another account is untouched");
    assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(&access)).await.status, StatusCode::OK, "devices stay");
    assert_eq!(outbox(&e.db).await.len(), mails, "no mail for website sessions");
    let s = b.get(&e, "sessions").await;
    let web: Vec<&Value> = s.body.as_array().unwrap().iter().filter(|x| x["kind"] == "web").collect();
    assert_eq!((web.len(), web[0]["current"].as_bool()), (1, Some(true)));

    // With the devices: every linked device is removed at once and mailed about.
    let (_, access2, refresh2, _) = linked(&e, &mut b, &d).await;
    let r = b.post(&e, "sessions/revoke-all", json!({"include_devices": true})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.raw);
    for t in [&access, &access2] {
        assert_eq!(d.call(&e, Method::GET, "device/me", None, Some(t)).await.status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(d.post(&e, "token/refresh", json!({"refresh_token": refresh2})).await.status, StatusCode::UNAUTHORIZED);
    let ob = outbox(&e.db).await;
    assert_eq!(ob[mails..].iter().filter(|x| x.1 == "device_revoked").count(), 2);
    assert_eq!(b.get(&e, "me").await.body["devices"], 0);

    // 10 per hour per account (3 used above: the refused one counts too).
    for _ in 0..7 {
        assert_eq!(b.post(&e, "sessions/revoke-all", json!({})).await.status, StatusCode::NO_CONTENT);
    }
    let r = b.post(&e, "sessions/revoke-all", json!({})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
}
