//! Integration tests of milestone S7 (PLAN §9): records in write order under concurrency,
//! conditional writes and conflicts, the envelope checks, quotas, purges and the 410 horizon,
//! the 2 MiB batch limit, the rotation freeze and the rekey, and S6 data surviving migration
//! 0005. Envelopes are built by hand: the server never sees plaintext.

use super::s3::{linked, signed_in, Device};
use super::*;
use crate::crypto::{b64url, random};
use uuid::Uuid;

struct Dev {
    d: Device,
    token: String,
    id: String,
}

impl Dev {
    async fn get(&self, e: &Env, path: &str) -> Resp {
        self.d.call(e, Method::GET, path, None, Some(&self.token)).await
    }
    async fn post(&self, e: &Env, path: &str, body: Value) -> Resp {
        self.d.call(e, Method::POST, path, Some(body), Some(&self.token)).await
    }
}

/// An account with a keyset (sync_state at epoch 1/1) and two devices.
async fn account(e: &Env, email: &str) -> (Browser, Dev, Dev, Uuid) {
    let mut owner = signed_in(e, email).await;
    let mk = |d: Device, (id, token, _, _): (String, String, String, (String, String))| Dev { d, token, id };
    let d1 = Device::new();
    let l1 = linked(e, &mut owner, &d1).await;
    let a = mk(d1, l1);
    let d2 = Device::new();
    let l2 = linked(e, &mut owner, &d2).await;
    let b = mk(d2, l2);
    let acc: Uuid = sqlx::query_scalar("SELECT account_id FROM devices WHERE id = $1::uuid")
        .bind(&a.id)
        .fetch_one(&e.db)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sync_state (account_id, keyset_gen, current_epoch, pak_epoch, rr_gen) VALUES ($1, $2, 1, 1, 1)",
    )
    .bind(acc)
    .bind(Uuid::new_v4())
    .execute(&e.db)
    .await
    .unwrap();
    sqlx::query("UPDATE devices SET has_keys = true, had_keys = true WHERE account_id = $1")
        .bind(acc)
        .execute(&e.db)
        .await
        .unwrap();
    (owner, a, b, acc)
}

fn rid(n: u16) -> String {
    let mut x = [0u8; 16];
    x[..2].copy_from_slice(&n.to_be_bytes());
    x[15] = 0xaa;
    b64url(&x)
}

fn env_of(c: &str, id: &str, e: u32, ct_len: usize) -> Value {
    let k = if ["passwords", "never_save", "payments"].contains(&c) { "pak" } else { "ak" };
    let ct: Vec<u8> = (0..ct_len).map(|i| (i % 251) as u8).collect();
    json!({"v": 1, "c": c, "id": id, "e": e, "k": k, "n": b64url(&random::<24>()), "ct": b64url(&ct)})
}

fn put(c: &str, id: &str, if_seq: i64) -> Value {
    json!({"c": c, "id": id, "if_seq": if_seq, "envelope": env_of(c, id, 1, 272)})
}

#[tokio::test]
async fn records_round_trip_in_write_order_with_conflicts() {
    let Some(e) = env().await else { return };
    let (_o, a, b, _) = account(&e, "records@example.org").await;
    // Nothing yet; then two writes.
    let r = a.get(&e, "records?since=0").await;
    assert_eq!(
        (r.status, r.body["head_seq"].as_i64(), r.body["records"].as_array().unwrap().len()),
        (StatusCode::OK, Some(0), 0)
    );
    let r = a
        .post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(1), 0), put("history", &rid(2), 0)]}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(r.body["results"][0], json!({"id": rid(1), "status": "ok", "seq": 1}));
    assert_eq!(r.body["head_seq"], 2);
    // B pulls them, in order, with the exact envelopes.
    let p = b.get(&e, "records?since=0").await.body;
    assert_eq!(p["records"].as_array().unwrap().iter().map(|x| x["seq"].as_i64().unwrap()).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(p["records"][0]["envelope"]["id"], rid(1));
    // B edits record 1 based on seq 1; A's stale edit (if_seq 0 = "new") is a conflict that
    // carries B's current version.
    let r = b.post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(1), 1)]})).await;
    assert_eq!(r.body["results"][0]["seq"], 3);
    let r = a
        .post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(1), 1), put("settings", &rid(3), 0)]}))
        .await;
    assert_eq!(r.body["results"][0]["status"], "conflict");
    assert_eq!(r.body["results"][0]["current"]["seq"], 3);
    assert_eq!(r.body["results"][1]["status"], "ok", "the rest of the batch goes through");
    let since = a.get(&e, "records?since=2").await.body;
    assert_eq!(since["records"].as_array().unwrap().len(), 2, "records 1 (seq 3) and 3 (seq 4)");
    // Paging.
    let page = a.get(&e, "records?since=0&limit=2").await.body;
    assert_eq!((page["records"].as_array().unwrap().len(), page["has_more"].as_bool()), (2, Some(true)));
    let st = a.get(&e, "sync/state").await.body;
    assert_eq!(
        (st["head_seq"].as_i64(), st["quota"]["records"].as_i64(), st["quota"]["history"].as_i64()),
        (Some(4), Some(3), Some(1))
    );
    // Another account sees nothing of these.
    let (_o2, x, _, _) = account(&e, "records-other@example.org").await;
    assert_eq!(x.get(&e, "records?since=0").await.body["records"].as_array().unwrap().len(), 0);
    // The devices list shows the last sync.
    assert!(a.get(&e, "devices").await.body.as_array().unwrap().iter().any(|d| d["last_sync"].is_string()));
}

#[tokio::test]
async fn a_pull_can_ask_for_one_key_kind() {
    let Some(e) = env().await else { return };
    let (_o, a, _b, _) = account(&e, "records-kinds@example.org").await;
    let writes = json!({"writes": [put("bookmarks", &rid(1), 0), put("passwords", &rid(2), 0), put("history", &rid(3), 0),
        put("never_save", &rid(4), 0)]});
    assert_eq!(a.post(&e, "records/batch", writes).await.status, StatusCode::OK);
    let seqs =
        |v: &Value| v["records"].as_array().unwrap().iter().map(|x| x["seq"].as_i64().unwrap()).collect::<Vec<_>>();
    assert_eq!(seqs(&a.get(&e, "records?since=0&k=pak").await.body), [2, 4]);
    assert_eq!(seqs(&a.get(&e, "records?since=2&k=pak").await.body), [4]);
    assert_eq!(seqs(&a.get(&e, "records?since=0&k=ak").await.body), [1, 3]);
    assert_eq!(seqs(&a.get(&e, "records?since=0").await.body), [1, 2, 3, 4]);
    assert_eq!(a.get(&e, "records?since=0&k=x").await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn seq_is_strictly_increasing_under_concurrent_batches() {
    let Some(e) = env().await else { return };
    let (_o, a, b, acc) = account(&e, "concurrent@example.org").await;
    let a = std::sync::Arc::new(a);
    let b = std::sync::Arc::new(b);
    let e = std::sync::Arc::new(e);
    let mut tasks = Vec::new();
    for t in 0..4u8 {
        let dev = if t % 2 == 0 { a.clone() } else { b.clone() };
        let e = e.clone();
        tasks.push(tokio::spawn(async move {
            for i in 0..10u8 {
                let writes: Vec<Value> = (0..5u8)
                    .map(|j| put("history", &rid(u16::from(t) * 100 + u16::from(i) * 5 + u16::from(j) + 1), 0))
                    .collect();
                let r = dev.post(&e, "records/batch", json!({"writes": writes})).await;
                assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
            }
        }));
    }
    for t in tasks {
        t.await.unwrap();
    }
    let seqs: Vec<i64> = sqlx::query_scalar("SELECT seq FROM records WHERE account_id = $1 ORDER BY seq")
        .bind(acc)
        .fetch_all(&e.db)
        .await
        .unwrap();
    assert_eq!(seqs, (1..=200).collect::<Vec<i64>>(), "every write got the next seq, none twice");
    let head: i64 = sqlx::query_scalar("SELECT head_seq FROM sync_state WHERE account_id = $1")
        .bind(acc)
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(head, 200);
}

#[tokio::test]
async fn envelopes_are_checked_for_structure() {
    let Some(e) = env().await else { return };
    let (_o, a, _, _) = account(&e, "envelopes@example.org").await;
    let id = rid(9);
    let mut wrong_c = env_of("history", &id, 1, 272);
    wrong_c["c"] = "bookmarks".into();
    let mut wrong_k = env_of("passwords", &id, 1, 272);
    wrong_k["k"] = "ak".into();
    let mut bad_nonce = env_of("history", &id, 1, 272);
    bad_nonce["n"] = b64url(&[1; 8]).into();
    let mut extra = env_of("history", &id, 1, 272);
    extra["x"] = 1.into();
    let cases = [
        (
            "unknown collection",
            json!({"c": "cookies", "id": id, "if_seq": 0, "envelope": env_of("history", &id, 1, 272)}),
        ),
        ("collection mismatch", json!({"c": "history", "id": id, "if_seq": 0, "envelope": wrong_c})),
        ("key kind mismatch", json!({"c": "passwords", "id": id, "if_seq": 0, "envelope": wrong_k})),
        ("id mismatch", json!({"c": "history", "id": rid(8), "if_seq": 0, "envelope": env_of("history", &id, 1, 272)})),
        ("short nonce", json!({"c": "history", "id": id, "if_seq": 0, "envelope": bad_nonce})),
        ("an unknown field", json!({"c": "history", "id": id, "if_seq": 0, "envelope": extra})),
        (
            "ciphertext below a bucket",
            json!({"c": "history", "id": id, "if_seq": 0, "envelope": env_of("history", &id, 1, 100)}),
        ),
        ("wrong epoch", json!({"c": "history", "id": id, "if_seq": 0, "envelope": env_of("history", &id, 2, 272)})),
        ("neither envelope nor purge", json!({"c": "history", "id": id, "if_seq": 0})),
    ];
    for (what, w) in cases {
        let r = a.post(&e, "records/batch", json!({"writes": [w]})).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}: {}", r.raw);
    }
    let big = a.post(&e, "records/batch", json!({"writes": [json!({"c": "history", "id": id, "if_seq": 0, "envelope": env_of("history", &id, 1, 65536 + 17)})]})).await;
    assert_eq!(big.status, StatusCode::PAYLOAD_TOO_LARGE, "a record above the largest bucket");
    assert_eq!(a.post(&e, "records/batch", json!({"writes": []})).await.status, StatusCode::BAD_REQUEST);
    let many: Vec<Value> = (0..501u16)
        .map(|i| {
            put("history", &b64url(&[(i % 256) as u8, (i / 256) as u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]), 0)
        })
        .collect();
    assert_eq!(
        a.post(&e, "records/batch", json!({"writes": many})).await.status,
        StatusCode::BAD_REQUEST,
        "501 writes"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM records").fetch_one(&e.db).await.unwrap(),
        0,
        "nothing was written"
    );
}

#[tokio::test]
async fn two_mebibyte_batches_and_quotas() {
    let Some(e) = env().await else { return };
    let (_o, a, _, _) = account(&e, "quota@example.org").await;
    // 28 records of 64 KiB: about 2.4 MB of base64 JSON is refused; 20 (about 1.8 MB) is fine.
    let writes = |n: u8| -> Vec<Value> {
        (0..n).map(|i| json!({"c": "history", "id": rid(u16::from(i) + 1), "if_seq": 0, "envelope": env_of("history", &rid(u16::from(i) + 1), 1, 65536 + 16)})).collect()
    };
    let r = a.post(&e, "records/batch", json!({"writes": writes(28)})).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
    let r = a.post(&e, "records/batch", json!({"writes": writes(20)})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", &r.raw[..r.raw.len().min(200)]);
    // Small quotas: the batch that would go over is refused as a whole.
    let mut state = e.state.clone();
    let mut cfg = (*state.config).clone();
    cfg.quota = crate::config::Quota { max_bytes: 1_350_000, max_records: 22, max_history: 21 };
    state.config = Arc::new(cfg);
    let small = Env { state, db: e.db.clone() };
    let r = a
        .post(&small, "records/batch", json!({"writes": [put("history", &rid(101), 0), put("history", &rid(102), 0)]}))
        .await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "quota_exceeded"), "history above 21");
    let r = a.post(&small, "records/batch", json!({"writes": [put("bookmarks", &rid(103), 0), put("bookmarks", &rid(104), 0), put("bookmarks", &rid(105), 0)]})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "quota_exceeded"), "records above 22");
    let r = a.post(&small, "records/batch", json!({"writes": [json!({"c": "bookmarks", "id": rid(106), "if_seq": 0, "envelope": env_of("bookmarks", &rid(106), 1, 65536 + 16)})]})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::BAD_REQUEST, "quota_exceeded"), "bytes above 1.35 MB");
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM records").fetch_one(&e.db).await.unwrap(), 20);
    let r = a.post(&small, "records/batch", json!({"writes": [put("bookmarks", &rid(107), 0)]})).await;
    assert_eq!(r.status, StatusCode::OK, "below every limit");
}

#[tokio::test]
async fn purges_raise_the_horizon_and_old_cursors_get_410() {
    let Some(e) = env().await else { return };
    let (_o, a, b, acc) = account(&e, "horizon@example.org").await;
    for i in 1..=5u16 {
        a.post(&e, "records/batch", json!({"writes": [put("history", &rid(i), 0)]})).await;
    }
    // A purge must name the current seq; then the row is gone and the horizon moves to it.
    let r = a
        .post(&e, "records/batch", json!({"writes": [{"c": "history", "id": rid(2), "if_seq": 1, "purge": true}]}))
        .await;
    assert_eq!(r.body["results"][0]["status"], "conflict", "stale");
    let r = a
        .post(&e, "records/batch", json!({"writes": [{"c": "history", "id": rid(2), "if_seq": 2, "purge": true}]}))
        .await;
    assert_eq!(r.body["results"][0]["status"], "ok", "{}", r.raw);
    let (records, horizon): (i32, i64) =
        sqlx::query_as("SELECT records, purge_horizon_seq FROM sync_state WHERE account_id = $1")
            .bind(acc)
            .fetch_one(&e.db)
            .await
            .unwrap();
    assert_eq!((records, horizon), (4, 2));
    assert_eq!(b.get(&e, "records?since=1").await.status, StatusCode::GONE, "a cursor below the horizon");
    assert_eq!(b.get(&e, "records?since=2").await.status, StatusCode::OK);
    assert_eq!(
        b.get(&e, "records?since=0").await.body["records"].as_array().unwrap().len(),
        4,
        "a full download is fine"
    );
}

#[tokio::test]
async fn writes_freeze_on_removal_and_the_rekey_swaps_old_records() {
    let Some(e) = env().await else { return };
    let (mut owner, a, b, acc) = account(&e, "rekey@example.org").await;
    for i in 1..=3u16 {
        a.post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(i), 0)]})).await;
    }
    a.post(&e, "records/batch", json!({"writes": [put("passwords", &rid(50), 0)]})).await;
    // B is removed: every write is 423, reads go on.
    owner.post(&e, "reauth", json!({"password": PW})).await;
    assert_eq!(owner.post(&e, &format!("devices/{}/revoke", b.id), json!({})).await.status, StatusCode::OK);
    let r = a.post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(9), 0)]})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::LOCKED, "rotation_pending"));
    assert_eq!(a.get(&e, "records?since=0").await.status, StatusCode::OK);
    // The next AK epoch (what keys/commit does): AK writes resume, only under epoch 2; PAK stays frozen.
    sqlx::query("UPDATE sync_state SET current_epoch = 2, rotation_ak = false WHERE account_id = $1")
        .bind(acc)
        .execute(&e.db)
        .await
        .unwrap();
    let r = a.post(&e, "records/batch", json!({"writes": [put("bookmarks", &rid(9), 0)]})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "epoch 1 is over");
    let r = a.post(&e, "records/batch", json!({"writes": [json!({"c": "bookmarks", "id": rid(9), "if_seq": 0, "envelope": env_of("bookmarks", &rid(9), 2, 272)})]})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let r = a.post(&e, "records/batch", json!({"writes": [put("passwords", &rid(51), 0)]})).await;
    assert_eq!(r.status, StatusCode::LOCKED, "passwords wait for the PAK rotation");
    assert_eq!(
        a.get(&e, "sync/state").await.body["rekey"]["remaining_old"],
        3,
        "3 AK records under epoch 1 (the PAK one isn't old yet)"
    );
    // The rotator pulls what is old and swaps it (new ids under epoch 2).
    let old = a.get(&e, "records?since=0&epoch_lt=2").await.body;
    let olds: Vec<String> =
        old["records"].as_array().unwrap().iter().map(|x| x["envelope"]["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(olds.len(), 3);
    let replaces: Vec<Value> = olds.iter().enumerate().map(|(i, o)| json!({"old_id": o, "c": "bookmarks", "envelope": env_of("bookmarks", &rid(200 + i as u16), 2, 272)})).collect();
    let r = a.post(&e, "rekey/batch", json!({"epoch": 2, "replaces": replaces[..2]})).await;
    assert_eq!(
        (r.status, r.body["done"].as_i64(), r.body["remaining_old"].as_i64()),
        (StatusCode::OK, Some(2), Some(1)),
        "{}",
        r.raw
    );
    let r = a.post(&e, "rekey/batch", json!({"epoch": 2, "replaces": replaces})).await;
    assert_eq!(
        (r.body["done"].as_i64(), r.body["remaining_old"].as_i64()),
        (Some(1), Some(0)),
        "the two done already are skipped"
    );
    let r = a.post(&e, "rekey/batch", json!({"epoch": 1, "replaces": [{"old_id": rid(1), "c": "bookmarks", "envelope": env_of("bookmarks", &rid(1), 1, 272)}]})).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "a stale rotator");
    let (records, head): (i32, i64) = sqlx::query_as("SELECT records, head_seq FROM sync_state WHERE account_id = $1")
        .bind(acc)
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(records, 5, "3 swapped + the new one + the password");
    let max: i64 = sqlx::query_scalar("SELECT max(seq) FROM records WHERE account_id = $1")
        .bind(acc)
        .fetch_one(&e.db)
        .await
        .unwrap();
    assert_eq!(max, head);
}

/// Migration 0005 is additive: an S6 database (keys, escrow, approvals) migrates intact.
#[tokio::test]
async fn s6_data_survives_migration_0005() {
    let Some(e) = env().await else { return };
    let admin_url = std::env::var("KRUNK_TEST_DATABASE_URL").unwrap();
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url).await.unwrap();
    let name = format!("krunk_it_s6_{}_{}", std::process::id(), next());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').unwrap().0);
    let db = PgPoolOptions::new().max_connections(5).connect(&url).await.unwrap();
    crate::db::MIGRATOR.run_to(4, &db).await.unwrap();
    let has_records: bool =
        sqlx::query_scalar("SELECT to_regclass('records') IS NOT NULL").fetch_one(&db).await.unwrap();
    assert!(!has_records, "S6 schema only");
    let mut state = e.state.clone();
    let mut cfg = (*state.config).clone();
    cfg.database_url = url.clone();
    state.config = Arc::new(cfg);
    state.db = db.clone();
    let s6 = Env { state, db: db.clone() };
    let (_o, a, _, acc) = account(&s6, "survivor6@example.org").await;
    sqlx::query("INSERT INTO ak_dirs (account_id, seq, epoch, bytes, hash, mac) VALUES ($1, 1, 1, '\\x7b7d', decode(repeat('00', 32), 'hex'), decode(repeat('00', 32), 'hex'))")
        .bind(acc)
        .execute(&db)
        .await
        .unwrap();
    let count = "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM devices), (SELECT count(*) FROM sync_state), (SELECT count(*) FROM ak_dirs)";
    let before: (i64, i64, i64, i64) = sqlx::query_as(count).fetch_one(&db).await.unwrap();
    crate::db::migrate(&db).await.unwrap();
    let after: (i64, i64, i64, i64) = sqlx::query_as(count).fetch_one(&db).await.unwrap();
    assert_eq!(before, after);
    let r = a.post(&s6, "records/batch", json!({"writes": [put("history", &rid(1), 0)]})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(a.get(&s6, "sync/state").await.body["head_seq"], 1);
}
