//! Website sign-in from a signed-in Krunk (service 2026-11-17).

use super::s3::{linked, signed_in, Device};
use super::*;

async fn code(e: &Env, d: &Device, access: &str) -> Resp {
    d.call(e, Method::POST, "device/web-sign-in", Some(json!({})), Some(access)).await
}

#[tokio::test]
async fn a_signed_in_krunk_signs_the_website_in() {
    let Some(e) = env().await else { return };
    let mut owner = signed_in(&e, "auto@example.org").await;
    let d = Device::new();
    let (device_id, access, _, _) = linked(&e, &mut owner, &d).await;

    // Only with a device token.
    let r = d.call(&e, Method::POST, "device/web-sign-in", Some(json!({})), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = code(&e, &d, &access).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["expires_in"], 60);
    let c = r.body["code"].as_str().unwrap().to_string();
    let stored: Vec<u8> = sqlx::query_scalar("SELECT hash FROM web_sign_in_codes").fetch_one(&e.db).await.unwrap();
    assert_ne!(stored, crate::crypto::b64url_decode::<32>(&c).unwrap().to_vec(), "only the HMAC is stored");

    // The site, in that browser: signed in, by the device, without a fresh re-auth.
    let mut site = Browser::at(d.ip);
    let r = site.post(&e, "device-sign-in", json!({"code": c})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert!(r.body["csrf"].is_string() && site.cookie.is_some());
    let me = site.get(&e, "me").await;
    assert_eq!((me.body["email"].as_str(), me.body["reauth_valid"].clone()), (Some("auto@example.org"), json!(false)));
    let r = site.post(&e, "password/change", json!({"current": "wrong!", "new": "fresh passphrase 2"})).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "sensitive actions still want the password");
    let r = site.post(&e, &format!("devices/{device_id}/revoke"), json!({})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::FORBIDDEN, "reauth_required"));
    // The list of signed-in browsers names the device.
    let list = owner.get(&e, "sessions").await;
    let web: Vec<&Value> = list.body.as_array().unwrap().iter().filter(|x| x["via"] == "device").collect();
    assert_eq!(web.len(), 1);
    assert_eq!(web[0]["label"], "Krunk on Windows 11");
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM ip_events WHERE kind = 'device_sign_in'").fetch_all(&e.db).await.unwrap();
    assert_eq!(kinds.len(), 1);

    // Single use.
    let mut again = Browser::new();
    let r = again.post(&e, "device-sign-in", json!({"code": c})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "invalid_code"));
    assert!(again.cookie.is_none());
    // Junk.
    assert_eq!(err(&again.post(&e, "device-sign-in", json!({"code": "x"})).await), "invalid_code");
    assert_eq!(
        err(&again.post(&e, "device-sign-in", json!({"code": crate::crypto::new_token()})).await),
        "invalid_code"
    );
}

#[tokio::test]
async fn codes_expire_and_die_with_the_device() {
    let Some(e) = env().await else { return };
    let mut owner = signed_in(&e, "auto2@example.org").await;
    let d = Device::new();
    let (device_id, access, _, _) = linked(&e, &mut owner, &d).await;
    // Expired.
    let c1 = code(&e, &d, &access).await.body["code"].as_str().unwrap().to_string();
    sqlx::query("UPDATE web_sign_in_codes SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    assert_eq!(err(&Browser::new().post(&e, "device-sign-in", json!({"code": c1})).await), "invalid_code");
    // The device is removed before the code is used.
    let c2 = code(&e, &d, &access).await.body["code"].as_str().unwrap().to_string();
    sqlx::query("UPDATE devices SET revoked_at = now() WHERE id = $1::uuid")
        .bind(&device_id)
        .execute(&e.db)
        .await
        .unwrap();
    assert_eq!(err(&Browser::new().post(&e, "device-sign-in", json!({"code": c2})).await), "invalid_code");
    // And a removed device gets no more codes.
    assert_eq!(code(&e, &d, &access).await.status, StatusCode::UNAUTHORIZED);
    // Housekeeping.
    sqlx::query("UPDATE web_sign_in_codes SET expires_at = now() - interval '2 hours'").execute(&e.db).await.unwrap();
    jobs::run_once(&e.db, 90).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM web_sign_in_codes").fetch_one(&e.db).await.unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn a_foreign_page_cant_use_either_side() {
    let Some(e) = env().await else { return };
    // The device side refuses another site's Origin (no_foreign_origin), the web side needs ours.
    let d = Device::new();
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/device/web-sign-in")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::AUTHORIZATION, format!("Bearer {}", crate::crypto::new_token()))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(d.ip, 1)));
    assert_eq!(router(e.state.clone()).oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/api/v1/web/device-sign-in")
        .header(header::ORIGIN, "https://evil.example")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({"code": crate::crypto::new_token()}).to_string()))
        .unwrap();
    req.extensions_mut().insert(ConnectInfo(SocketAddr::new(d.ip, 1)));
    assert_eq!(router(e.state.clone()).oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
}
