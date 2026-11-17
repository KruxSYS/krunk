//! Integration tests of 0.13.0 (PLAN §7 and §9 S10, PROTOCOL §6.5): "Add a device" by QR, for a
//! native client and for a phone's browser, in every state; the usage and activity panels; the
//! data download; sync reset; account deletion; and a 0006 database surviving the 0007 expand
//! step with the earlier api still able to start against it.

use super::s3::{linked, signed_in, Device};
use super::*;
use uuid::Uuid;

/// The token in a pairing's `url` (`<base>/pair#<token>`).
fn token_of(start: &Resp) -> String {
    let url = start.body["url"].as_str().expect("a url");
    assert!(url.starts_with("https://krunk.209.wf/pair#"), "{url}");
    url.split_once('#').unwrap().1.to_string()
}

async fn native_claim(e: &Env, d: &Device, token: &str) -> Resp {
    d.post(
        e,
        "link/pair/claim",
        json!({"token": token, "platform": "android", "label": "Krunk on Android", "client_version": "0.1.0"}),
    )
    .await
}

async fn kinds(e: &Env, email: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT e.kind FROM ip_events e JOIN accounts a ON a.id = e.account_id WHERE a.email = $1 ORDER BY e.id",
    )
    .bind(email)
    .fetch_all(&e.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn pairing_a_native_device() {
    let Some(e) = env().await else { return };
    // Starting needs a recent re-auth.
    let mut fresh = registered_and_verified(&e, "pair-fresh@example.org", PW).await;
    assert_eq!(login(&e, &mut fresh, "pair-fresh@example.org", PW, None).await.status, StatusCode::OK);
    lapse_reauth(&e.db).await;
    let r = fresh.post(&e, "pair/start", json!({})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "reauth_required"));

    let mut b = signed_in(&e, "pair@example.org").await;
    assert_eq!(b.get(&e, "pair").await.body["state"], "none");
    let start = b.post(&e, "pair/start", json!({})).await;
    assert_eq!(start.status, StatusCode::OK, "{}", start.raw);
    assert_eq!(start.body["expires_in"], 300);
    assert!(start.body["qr_svg"].as_str().unwrap().contains("<svg"));
    let token = token_of(&start);
    // Only the HMAC is stored.
    let raw: i64 = sqlx::query_scalar("SELECT count(*) FROM pair_grants WHERE token_hash = convert_to($1, 'UTF8')")
        .bind(&token)
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(raw, 0);
    assert_eq!(b.get(&e, "pair").await.body["state"], "waiting");

    // Approving before a claim is refused.
    let r = b.post(&e, "pair/approve", json!({"confirm_code": "000000"})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::CONFLICT, "pair_not_claimed"));

    let d = Device::new();
    let c = native_claim(&e, &d, &token).await;
    assert_eq!(c.status, StatusCode::OK, "{}", c.raw);
    let code = c.body["confirm_code"].as_str().unwrap().to_string();
    assert_eq!(code.len(), 6);
    assert_eq!(c.body["interval"], 5);
    let dc = c.body["device_code"].as_str().unwrap().to_string();
    // Single use: a second claim, native or in a browser, is refused.
    let again = native_claim(&e, &Device::new(), &token).await;
    assert_eq!((again.status, err(&again)), (StatusCode::CONFLICT, "pair_used"));
    let again = Browser::new().post(&e, "pair/claim", json!({"token": token})).await;
    assert_eq!((again.status, err(&again)), (StatusCode::CONFLICT, "pair_used"));
    // The paired grant can't be reached through /link (its user code is never shown).
    let hidden: i64 =
        sqlx::query_scalar("SELECT count(*) FROM link_grants WHERE pair_id IS NOT NULL AND status = 'pending'")
            .fetch_one(&e.db)
            .await
            .unwrap();
    assert_eq!(hidden, 1);

    let st = b.get(&e, "pair").await;
    assert_eq!(st.body["state"], "claimed", "{}", st.raw);
    assert_eq!(st.body["confirm_code"], code.as_str());
    let who = &st.body["claimant"];
    assert_eq!((who["kind"].as_str(), who["platform"].as_str()), (Some("device"), Some("android")));
    assert_eq!(who["label"], "Krunk on Android");
    assert_eq!(who["ip"], d.ip.to_string());
    assert_eq!(d.poll(&e, &dc).await.body["error"], "authorization_pending");

    let r = b.post(&e, "pair/approve", json!({"confirm_code": "123"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = b.post(&e, "pair/approve", json!({"confirm_code": code})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["kind"], "device");
    let t = d.poll(&e, &dc).await;
    assert_eq!(t.status, StatusCode::OK, "{}", t.raw);
    let access = t.body["access_token"].as_str().unwrap();
    let me = d.call(&e, Method::GET, "device/me", None, Some(access)).await;
    assert_eq!((me.body["platform"].as_str(), me.body["email"].as_str()), (Some("android"), Some("pair@example.org")));
    let devs = b.get(&e, "devices").await;
    assert!(devs.body.as_array().unwrap().iter().any(|x| x["platform"] == "android"), "{}", devs.raw);
    assert_eq!(b.get(&e, "pair").await.body["state"], "approved");
    // Approved once: a second approval or claim is refused.
    let r = b.post(&e, "pair/approve", json!({"confirm_code": code})).await;
    assert_eq!(err(&r), "pair_used");
    assert!(outbox(&e.db).await.iter().any(|(to, t, _)| to == "pair@example.org" && t == "device_linked"));
    let k = kinds(&e, "pair@example.org").await;
    for want in ["pair_started", "pair_claimed", "device_linked", "pair_approved"] {
        assert!(k.iter().any(|x| x == want), "{want} in {k:?}");
    }
}

#[tokio::test]
async fn pairing_denied_expired_and_replaced() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "pair-deny@example.org").await;
    // Denied: the device's poll gets access_denied; the token stays spent.
    let token = token_of(&b.post(&e, "pair/start", json!({})).await);
    let d = Device::new();
    let c = native_claim(&e, &d, &token).await;
    let dc = c.body["device_code"].as_str().unwrap().to_string();
    assert_eq!(b.post(&e, "pair/deny", json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(b.get(&e, "pair").await.body["state"], "denied");
    assert_eq!(d.poll(&e, &dc).await.body["error"], "access_denied");
    assert_eq!(err(&native_claim(&e, &d, &token).await), "pair_used");
    let r = b.post(&e, "pair/approve", json!({"confirm_code": c.body["confirm_code"]})).await;
    assert_eq!(err(&r), "pair_used");

    // A new start replaces the old pairing: its token is unknown now.
    let old = token_of(&b.post(&e, "pair/start", json!({})).await);
    let new = token_of(&b.post(&e, "pair/start", json!({})).await);
    let r = native_claim(&e, &Device::new(), &old).await;
    assert_eq!((r.status, err(&r)), (StatusCode::NOT_FOUND, "not_found"));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM pair_grants").fetch_one(&e.db).await.unwrap();
    assert_eq!(n, 1, "one pairing per account");

    // Expired: claims are refused with 410, the page says so.
    sqlx::query("UPDATE pair_grants SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    let r = native_claim(&e, &Device::new(), &new).await;
    assert_eq!((r.status, err(&r)), (StatusCode::GONE, "expired_token"));
    let r = Browser::new().post(&e, "pair/claim", json!({"token": new})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::GONE, "expired_token"));
    assert_eq!(b.get(&e, "pair").await.body["state"], "expired");

    // A claimed pairing that expires before the approval: the approval is refused.
    let token = token_of(&b.post(&e, "pair/start", json!({})).await);
    let c = native_claim(&e, &d, &token).await;
    sqlx::query("UPDATE pair_grants SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    let r = b.post(&e, "pair/approve", json!({"confirm_code": c.body["confirm_code"]})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::GONE, "expired_token"));

    // Cancel removes it; garbage tokens are refused before the database.
    assert_eq!(b.post(&e, "pair/cancel", json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(b.get(&e, "pair").await.body["state"], "none");
    assert_eq!(err(&Browser::new().post(&e, "pair/claim", json!({"token": "x"})).await), "not_found");
    let bad = d
        .post(&e, "link/pair/claim", json!({"token": token, "platform": "ios", "label": "x", "client_version": "1"}))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pair_claims_are_limited_per_ip() {
    let Some(e) = env().await else { return };
    let phone = Browser::new();
    for i in 0..20 {
        let r = phone.clone().post(&e, "pair/claim", json!({"token": crate::crypto::new_token()})).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "attempt {i}");
    }
    let r = phone.clone().post(&e, "pair/claim", json!({"token": crate::crypto::new_token()})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn pairing_signs_in_a_phone_browser() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "pair-web@example.org").await;
    let token = token_of(&b.post(&e, "pair/start", json!({})).await);
    let mut phone = Browser::new();
    let c = phone.post(&e, "pair/claim", json!({"token": token})).await;
    assert_eq!(c.status, StatusCode::OK, "{}", c.raw);
    assert_eq!(c.body["interval"], 2);
    let poll_token = c.body["poll_token"].as_str().unwrap().to_string();
    let poll = |p: &mut Browser| {
        let e = &e;
        let t = poll_token.clone();
        let mut p = p.clone();
        async move {
            sqlx::query("UPDATE pair_grants SET last_poll_at = last_poll_at - interval '1 minute'")
                .execute(&e.db)
                .await
                .unwrap();
            let r = p.post(e, "pair/poll", json!({"poll_token": t})).await;
            (r, p)
        }
    };
    let (r, _) = poll(&mut phone).await;
    assert_eq!(err(&r), "authorization_pending");
    let r = phone.clone().post(&e, "pair/poll", json!({"poll_token": poll_token})).await;
    assert_eq!(err(&r), "slow_down");
    assert!(phone.cookie.is_none(), "no session before the approval");

    let st = b.get(&e, "pair").await;
    assert_eq!(st.body["claimant"]["kind"], "web");
    assert_eq!(st.body["claimant"]["browser"], "A browser");
    let r = b.post(&e, "pair/approve", json!({"confirm_code": c.body["confirm_code"]})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert!(outbox(&e.db).await.iter().any(|(to, t, _)| to == "pair-web@example.org" && t == "qr_sign_in"));

    let (r, mut phone) = poll(&mut phone).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert!(phone.cookie.is_some() && phone.csrf.is_some());
    assert_eq!(phone.get(&e, "me").await.body["email"], "pair-web@example.org");
    // Single use: the poll token is spent.
    let (r, _) = poll(&mut Browser::new()).await;
    assert_eq!(err(&r), "expired_token");
    // Labelled as a QR sign-in; the activity marks the phone's own session.
    let sessions = b.get(&e, "sessions").await;
    let vias: Vec<&str> = sessions.body.as_array().unwrap().iter().filter_map(|s| s["via"].as_str()).collect();
    assert!(vias.contains(&"qr") && vias.contains(&"password"), "{vias:?}");
    let act = phone.get(&e, "activity").await;
    let qr = act.body["events"].as_array().unwrap().iter().find(|x| x["kind"] == "qr_sign_in").unwrap().clone();
    assert_eq!(qr["source"]["current"], true, "{qr}");
    assert_eq!(qr["ip"], phone.ip.to_string());
    // A QR session has no re-auth of its own.
    assert_eq!(phone.get(&e, "me").await.body["reauth_valid"], false);
}

#[tokio::test]
async fn usage_and_activity() {
    let Some(e) = env().await else { return };
    let mut b = signed_in(&e, "usage@example.org").await;
    let d = Device::new();
    let (device_id, _, _, _) = linked(&e, &mut b, &d).await;
    let acc: Uuid =
        sqlx::query_scalar("SELECT id FROM accounts WHERE email = 'usage@example.org'").fetch_one(&e.db).await.unwrap();
    for (i, (c, size)) in [("history", 300), ("history", 200), ("bookmarks", 1000)].into_iter().enumerate() {
        let mut id = [0u8; 16];
        id[0] = i as u8;
        sqlx::query(
            "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, $2, $3, $4, 1, 'ak', '{}', $5)",
        )
        .bind(acc)
        .bind(c)
        .bind(id.as_slice())
        .bind(i as i64 + 1)
        .bind(size)
        .execute(&e.db)
        .await
        .unwrap();
    }
    let u = b.get(&e, "usage").await;
    assert_eq!(u.status, StatusCode::OK, "{}", u.raw);
    let r = &u.body["records"];
    assert_eq!((r["total"].as_i64(), r["bytes"].as_i64(), r["history"].as_i64()), (Some(3), Some(1500), Some(2)));
    assert_eq!(
        (r["max"].as_i64(), r["max_history"].as_i64(), r["max_record_bytes"].as_i64()),
        (Some(100_000), Some(25_000), Some(65536))
    );
    assert_eq!(r["collections"].as_array().unwrap().len(), 2);
    assert_eq!((u.body["devices"]["active"].as_i64(), u.body["devices"]["max"].as_i64()), (Some(1), Some(10)));
    let item = &u.body["devices"]["items"][0];
    assert_eq!(
        (item["id"].as_str(), item["last_ip"].as_str()),
        (Some(device_id.as_str()), Some(d.ip.to_string().as_str()))
    );
    let a = &u.body["account"];
    assert!(a["last_sign_in"].is_string() && a["password_changed"].is_string() && a["created"].is_string());
    assert_eq!((a["totp"].as_bool(), a["backup_codes_left"].as_i64()), (Some(false), Some(0)));

    // Activity: newest first, paged, the current session marked, full IPs.
    let all = b.get(&e, "activity").await;
    assert_eq!(all.body["retention_days"], 90);
    let events = all.body["events"].as_array().unwrap().clone();
    assert!(events.len() >= 3, "{}", all.raw);
    assert!(events.windows(2).all(|w| w[0]["id"].as_i64() > w[1]["id"].as_i64()));
    let linked_ev = events.iter().find(|x| x["kind"] == "device_linked").unwrap();
    assert_eq!(
        (linked_ev["source"]["kind"].as_str(), linked_ev["source"]["current"].as_bool()),
        (Some("web"), Some(true))
    );
    assert_eq!(linked_ev["ip"], b.ip.to_string());
    assert!(all.body["next_before"].is_null());
    let p1 = b.get(&e, "activity?limit=2").await;
    assert_eq!(p1.body["events"].as_array().unwrap().len(), 2);
    let before = p1.body["next_before"].as_i64().unwrap();
    let p2 = b.get(&e, &format!("activity?limit=2&before={before}")).await;
    assert!(p2.body["events"][0]["id"].as_i64().unwrap() < before);
    // Devices and sessions show the full IP too.
    let devs = b.get(&e, "devices").await;
    assert_eq!(devs.body[0]["last_ip"], d.ip.to_string());
    let s = b.get(&e, "sessions").await;
    assert!(s.body.as_array().unwrap().iter().any(|x| x["ip"] == b.ip.to_string()));
}

#[tokio::test]
async fn the_data_download() {
    let Some(e) = env().await else { return };
    let setup = super::s6::setup(&e, "export@example.org").await;
    let mut b = setup.owner.clone();
    let mut id = [7u8; 16];
    id[0] = 1;
    sqlx::query(
        "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, 'bookmarks', $2, 1, 1, 'ak', $3, 4)",
    )
    .bind(setup.acc)
    .bind(id.as_slice())
    .bind(json!({"v": 1, "ct": "c2VjcmV0"}))
    .execute(&e.db)
    .await
    .unwrap();
    // Needs a re-auth.
    sqlx::query("UPDATE web_sessions SET reauth_at = NULL").execute(&e.db).await.unwrap();
    let r = b.post(&e, "export", json!({})).await;
    assert_eq!(err(&r), "reauth_required");
    b.post(&e, "reauth", json!({"password": PW})).await;
    let mails_before = outbox(&e.db).await.len();
    let r = b.post(&e, "export", json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.headers[header::CONTENT_TYPE], "application/json");
    let cd = r.headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(cd.starts_with("attachment; filename=\"krunk-account-") && cd.ends_with(".json\""), "{cd}");
    let v: Value = serde_json::from_str(&r.raw).expect("valid JSON");
    assert_eq!(v["schema"], "krunk-account-export/1");
    assert_eq!(v["account"]["email"], "export@example.org");
    for k in ["devices", "web_sessions", "activity", "emails", "records"] {
        assert!(v[k].is_array(), "{k}");
    }
    assert!(v["pending"]["link_grants"].is_array() && v["pending"]["pair_grants"].is_array());
    assert_eq!(v["devices"].as_array().unwrap().len(), 1);
    assert!(v["web_sessions"].as_array().unwrap().iter().any(|s| s["current"] == true));
    assert!(v["emails"].as_array().unwrap().iter().any(|m| m["template"] == "verify"));
    assert_eq!(v["sync"]["has_keyset"], true);
    assert_eq!(v["records"].as_array().unwrap().len(), 1);
    assert_eq!(v["records"][0]["envelope"]["ct"], "c2VjcmV0");
    assert_eq!(v["records"][0]["id"], crate::crypto::b64url(&id));
    // No key material: no directories, seals or escrow wraps; no hashes or tokens.
    for bad in ["rr_wrap", "ak_dir", "pak_dir", "seals", "token_hash", "pw_phc", "totp_secret", "current_hash"] {
        assert!(!r.raw.contains(bad), "{bad} in the export");
    }
    // Never mailed; an event; at most 5 an hour (the refused try above counted too).
    assert_eq!(outbox(&e.db).await.len(), mails_before);
    assert!(kinds(&e, "export@example.org").await.contains(&"export".to_string()));
    for _ in 0..3 {
        assert_eq!(b.post(&e, "export", json!({})).await.status, StatusCode::OK);
    }
    assert_eq!(b.post(&e, "export", json!({})).await.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn sync_reset_keeps_the_account_and_devices() {
    let Some(e) = env().await else { return };
    let setup = super::s6::setup(&e, "reset@example.org").await;
    let mut b = setup.owner.clone();
    sqlx::query(
        "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, 'bookmarks', $2, 1, 1, 'ak', '{}', 4)",
    )
    .bind(setup.acc)
    .bind([9u8; 16].as_slice())
    .execute(&e.db)
    .await
    .unwrap();
    let r = b.post(&e, "sync/reset", json!({"confirm": "delete sync data"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = b.post(&e, "sync/reset", json!({"confirm": "DELETE SYNC DATA"})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["deleted"]["records"], 1);
    for t in ["records", "ak_dirs", "pak_dirs", "escrow", "sync_state", "approvals"] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {t} WHERE account_id = $1")))
            .bind(setup.acc)
            .fetch_one(&e.db)
            .await
            .unwrap();
        assert_eq!(n, 0, "{t}");
    }
    let (n, keys): (i64, bool) = sqlx::query_as(
        "SELECT count(*), bool_or(has_keys OR had_keys) FROM devices WHERE account_id = $1 AND revoked_at IS NULL",
    )
    .bind(setup.acc)
    .fetch_one(&e.db)
    .await
    .unwrap();
    assert_eq!((n, keys), (1, false));
    // The device stays signed in and sees no keyset; the account works.
    let head = setup.a.get(&e, "keys/head").await;
    assert_eq!((head.status, err(&head)), (StatusCode::NOT_FOUND, "no_keyset"));
    assert_eq!(b.get(&e, "me").await.body["sync"]["has_keyset"], false);
    assert!(outbox(&e.db).await.iter().any(|(to, t, _)| to == "reset@example.org" && t == "sync_reset"));
}

#[tokio::test]
async fn deleting_an_account() {
    let Some(e) = env().await else { return };
    let setup = super::s6::setup(&e, "delete@example.org").await;
    let mut b = setup.owner.clone();
    let mut other = signed_in(&e, "bystander@example.org").await;
    sqlx::query(
        "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, 'history', $2, 1, 1, 'ak', '{}', 4)",
    )
    .bind(setup.acc)
    .bind([3u8; 16].as_slice())
    .execute(&e.db)
    .await
    .unwrap();
    let cookie = b.clone();
    let body = |email: &str, pw: &str, t: &str| json!({"password": pw, "turnstile": t, "confirm_email": email});
    let r = b.post(&e, "account/delete", body("someone@example.org", PW, &ts("delete"))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "the typed email must match");
    let r = b.post(&e, "account/delete", body("delete@example.org", PW, &ts("login"))).await;
    assert_eq!(err(&r), "turnstile_failed", "the action must be delete");
    let r = b.post(&e, "account/delete", body("delete@example.org", "wrong password!!", &ts("delete"))).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = b.post(&e, "account/delete", body("DELETE@example.org", PW, &ts("delete"))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["remaining"], json!({"records": 0, "devices": 0, "sessions": 0, "keys": 0}));
    assert!(r.headers[header::SET_COOKIE].to_str().unwrap().contains("Max-Age=0"));

    // Every table that names an account holds nothing of it.
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.columns
         WHERE table_schema = 'public' AND column_name = 'account_id' AND table_name <> 'account_deletions'",
    )
    .fetch_all(&e.db)
    .await
    .unwrap();
    assert!(tables.len() >= 15, "{tables:?}");
    for t in &tables {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {t} WHERE account_id = $1")))
            .bind(setup.acc)
            .fetch_one(&e.db)
            .await
            .unwrap();
        assert_eq!(n, 0, "{t}");
    }
    let logged: i64 = sqlx::query_scalar("SELECT count(*) FROM account_deletions WHERE account_id = $1")
        .bind(setup.acc)
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(logged, 1);
    // The old cookie and the device's token answer 401.
    assert_eq!(cookie.clone().get(&e, "me").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(setup.a.get(&e, "device/me").await.status, StatusCode::UNAUTHORIZED);
    // The receipt is queued (with no account), nothing else to that address remains.
    let mails: Vec<(String, Option<Uuid>)> =
        sqlx::query_as("SELECT template, account_id FROM email_outbox WHERE to_addr = 'delete@example.org'")
            .fetch_all(&e.db)
            .await
            .unwrap();
    assert_eq!(mails, vec![("account_deleted".to_string(), None)]);
    // Another account is untouched.
    assert_eq!(other.get(&e, "me").await.body["email"], "bystander@example.org");
}

/// A 0006 database (as live before 0.13.0), seeded in SQL, survives the 0007 expand step: its
/// users, sessions, devices and records still work, `_sqlx_migrations` still ends at 6, and the
/// migrator as the earlier api ran it (unknown versions refused) still accepts the database, so
/// a rolled-back api starts.
#[tokio::test]
async fn pre_0007_data_survives_the_expand_step() {
    let Some(e) = env().await else { return };
    let admin_url = std::env::var("KRUNK_TEST_DATABASE_URL").unwrap();
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url).await.unwrap();
    let name = format!("krunk_it_s6_{}_{}", std::process::id(), next());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').unwrap().0);
    let db = PgPoolOptions::new().max_connections(5).connect(&url).await.unwrap();
    crate::db::MIGRATOR.run(&db).await.unwrap();
    let has_pairs: bool =
        sqlx::query_scalar("SELECT to_regclass('pair_grants') IS NOT NULL").fetch_one(&db).await.unwrap();
    assert!(!has_pairs, "0006 schema only");

    let keys = &e.state.config.keys;
    let acc = Uuid::new_v4();
    let phc = e.state.passwords.hash(PW).await.unwrap();
    sqlx::query(
        "INSERT INTO accounts (id, email, email_verified_at, pw_phc, pepper_id) VALUES ($1, 'old@example.org', now(), $2, $3)",
    )
    .bind(acc)
    .bind(&phc)
    .bind(crate::password::PEPPER_ID)
    .execute(&db)
    .await
    .unwrap();
    let cookie = crate::crypto::new_token();
    sqlx::query(
        "INSERT INTO web_sessions (id, account_id, token_hash, ip, country) VALUES ($1, $2, $3, '198.51.100.7', 'DE')",
    )
    .bind(Uuid::new_v4())
    .bind(acc)
    .bind(sessions::token_hash(keys, &cookie).as_slice())
    .execute(&db)
    .await
    .unwrap();
    let dev = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO devices (id, account_id, short, platform, label, client_version, last_ip) VALUES ($1, $2, 0, 'windows', 'Old PC', '0.12.4', '198.51.100.8')",
    )
    .bind(dev)
    .bind(acc)
    .execute(&db)
    .await
    .unwrap();
    let access = crate::crypto::new_token();
    sqlx::query("INSERT INTO refresh_families (id, device_id, current_hash) VALUES ($1, $2, $3)")
        .bind(Uuid::new_v4())
        .bind(dev)
        .bind(crate::crypto::random::<32>().as_slice())
        .execute(&db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO access_tokens (hash, device_id, expires_at) VALUES ($1, $2, now() + interval '10 minutes')",
    )
    .bind(crate::crypto::hmac(&keys.token_hmac, "krunk-access-token", &[access.as_bytes()]).as_slice())
    .bind(dev)
    .execute(&db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, 'bookmarks', $2, 1, 1, 'ak', '{}', 4)")
        .bind(acc)
        .bind([1u8; 16].as_slice())
        .execute(&db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO ip_events (account_id, kind, ip, country) VALUES ($1, 'login', '198.51.100.7', 'DE')")
        .bind(acc)
        .execute(&db)
        .await
        .unwrap();
    let count = "SELECT (SELECT count(*) FROM accounts) + 10 * (SELECT count(*) FROM web_sessions)
        + 100 * (SELECT count(*) FROM devices) + 1000 * (SELECT count(*) FROM records)
        + 10000 * (SELECT count(*) FROM ip_events)";
    let before: i64 = sqlx::query_scalar(count).fetch_one(&db).await.unwrap();

    crate::db::migrate(&db).await.unwrap();
    crate::db::expand(&db).await.unwrap(); // idempotent
    let after: i64 = sqlx::query_scalar(count).fetch_one(&db).await.unwrap();
    assert_eq!(before, after);
    // (Ordinary sqlx migrations follow from 0008 on, db.rs.)
    let n7: i64 =
        sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version = 7").fetch_one(&db).await.unwrap();
    assert_eq!(n7, 0, "0007 is not a sqlx migration");
    let steps: Vec<String> = sqlx::query_scalar("SELECT step FROM krunk_schema_steps").fetch_all(&db).await.unwrap();
    assert_eq!(steps, ["0007_account_section"]);
    // The earlier api's migrator still accepts this database.
    crate::db::MIGRATOR.run(&db).await.expect("a rolled-back api starts");

    // The old data works with the new code.
    let mut state = e.state.clone();
    let mut cfg = (*state.config).clone();
    cfg.database_url = url.clone();
    state.config = Arc::new(cfg);
    state.db = db.clone();
    let env7 = Env { state, db: db.clone() };
    let mut old = Browser::new();
    old.cookie = Some(cookie);
    assert_eq!(old.get(&env7, "me").await.body["email"], "old@example.org");
    let sessions = old.get(&env7, "sessions").await;
    assert!(sessions.body.as_array().unwrap().iter().any(|s| s["via"] == "password"), "{}", sessions.raw);
    let act = old.get(&env7, "activity").await;
    assert_eq!(act.body["events"][0]["source"]["kind"], Value::Null);
    let me = Device::new().call(&env7, Method::GET, "device/me", None, Some(&access)).await;
    assert_eq!(me.body["label"], "Old PC");
    let mut fresh = Browser::new();
    assert_eq!(login(&env7, &mut fresh, "old@example.org", PW, None).await.status, StatusCode::OK);
    assert_eq!(fresh.get(&env7, "usage").await.body["records"]["total"], 1);
}

/// The 0.13.0 part of the log-redaction flow (called from `logs_hold_no_secrets`): pairing in a
/// browser and natively, and the data download; returns every secret value it used.
pub(super) async fn s10_flow_secrets(e: &Env) -> Vec<String> {
    let mut b = signed_in(e, "s10log@example.org").await;
    let start = b.post(e, "pair/start", json!({})).await;
    let token = token_of(&start);
    let mut phone = Browser::new();
    let c = phone.post(e, "pair/claim", json!({"token": token})).await;
    let poll = c.body["poll_token"].as_str().unwrap().to_string();
    let code = c.body["confirm_code"].as_str().unwrap().to_string();
    b.post(e, "pair/approve", json!({"confirm_code": code})).await;
    phone.post(e, "pair/poll", json!({"poll_token": poll})).await;
    let token2 = token_of(&b.post(e, "pair/start", json!({})).await);
    let d = Device::new();
    let nc = native_claim(e, &d, &token2).await;
    let dc = nc.body["device_code"].as_str().unwrap().to_string();
    let code2 = nc.body["confirm_code"].as_str().unwrap().to_string();
    b.post(e, "pair/approve", json!({"confirm_code": code2})).await;
    let t = d.poll(e, &dc).await;
    let export = b.post(e, "export", json!({})).await;
    assert_eq!(export.status, StatusCode::OK);
    let mut out = vec![token, token2, poll, dc, phone.cookie.clone().unwrap_or_default(), "s10log".into()];
    out.extend(["access_token", "refresh_token"].iter().filter_map(|k| t.body[*k].as_str().map(String::from)));
    // A 6-digit code could match a number elsewhere in a log line: only check it in context.
    out.push(format!("\"confirm_code\":\"{code}\""));
    out
}
