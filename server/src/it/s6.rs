//! Integration tests of milestone S6 (PLAN §9): keys and escrow (first commit, CAS, every
//! structural check), the escrow fetch limit and mail, revocation → frozen writes → rotation,
//! the rotation lease, the approval relay in order and single use, the device API's devices and
//! sessions, and S3 data surviving migration 0004.
//!
//! Directories here are built by hand with dummy tags: the server never checks cryptography (the
//! clients do, PROTOCOL §4.4); the real client's directories are exercised by the Linux test
//! client against staging (server/e2e/sync.cjs).

use super::s3::{linked, signed_in, Device};
use super::*;
use crate::crypto::{b64url, sha256};
use uuid::Uuid;

const PP_KDF: &str = r#"{"alg":"argon2id","m_kib":131072,"t":3,"p":1,"salt":"AAAAAAAAAAAAAAAAAAAAAA"}"#;

fn pp_kdf() -> Value {
    serde_json::from_str(PP_KDF).unwrap()
}

/// A device of an account in a test: its id and access token.
pub(super) struct Dev {
    d: Device,
    id: String,
    pub(super) token: String,
}

impl Dev {
    pub(super) async fn get(&self, e: &Env, path: &str) -> Resp {
        self.d.call(e, Method::GET, path, None, Some(&self.token)).await
    }
    pub(super) async fn post(&self, e: &Env, path: &str, body: Value) -> Resp {
        self.d.call(e, Method::POST, path, Some(body), Some(&self.token)).await
    }
}

async fn new_dev(e: &Env, b: &mut Browser) -> Dev {
    let d = Device::new();
    let (id, token, _, _) = linked(e, b, &d).await;
    Dev { d, id, token }
}

async fn account_of(e: &Env, dev: &Dev) -> Uuid {
    sqlx::query_scalar("SELECT account_id FROM devices WHERE id = $1::uuid")
        .bind(&dev.id)
        .fetch_one(&e.db)
        .await
        .unwrap()
}

#[derive(Clone)]
struct Ver {
    bytes: Vec<u8>,
}

impl Ver {
    fn hash(&self) -> String {
        b64url(&sha256(&self.bytes))
    }
}

#[allow(clippy::too_many_arguments)]
fn ak_dir(
    acc: Uuid,
    author: &str,
    seq: u64,
    prev: Option<&Ver>,
    epoch: u32,
    devices: &[&str],
    revoked: &[&str],
) -> Ver {
    let v = json!({
        "v": 1, "account_id": b64url(acc.as_bytes()), "epoch": epoch, "seq": seq,
        "prev_hash": prev.map(Ver::hash).unwrap_or_default(),
        "devices": devices.iter().enumerate().map(|(i, d)| json!({"id": d, "short": i, "pk": "pk", "added_by": author, "added_seq": seq})).collect::<Vec<_>>(),
        "revoked": revoked, "min_record_epoch": epoch,
        "seals": devices.iter().map(|d| json!({"to": d, "enc": "e", "ct": "c"})).chain([json!({"to": "escrow", "enc": "e", "ct": "c"})]).collect::<Vec<_>>(),
        "created_ms": 1, "created_by": author,
    });
    Ver { bytes: serde_json::to_vec(&v).unwrap() }
}

#[allow(clippy::too_many_arguments)]
fn pak_dir(
    acc: Uuid,
    author: &str,
    seq: u64,
    prev: Option<&Ver>,
    epoch: u32,
    vault: &[&str],
    rr_gen: u32,
    esc: u8,
) -> Ver {
    let v = json!({
        "v": 1, "account_id": b64url(acc.as_bytes()), "pak_epoch": epoch, "seq": seq,
        "prev_hash": prev.map(Ver::hash).unwrap_or_default(),
        "pp_kdf": pp_kdf(), "esc_pk": b64url(&[esc; 32]), "rr_gen": rr_gen,
        "vault_keys": vault.iter().map(|d| json!({"device": d, "pk": "pk"})).collect::<Vec<_>>(),
        "seals": vault.iter().map(|d| json!({"to": d, "enc": "e", "ct": "c"})).chain([json!({"to": "escrow", "enc": "e", "ct": "c"})]).collect::<Vec<_>>(),
        "created_ms": 1, "created_by": author,
    });
    Ver { bytes: serde_json::to_vec(&v).unwrap() }
}

fn blob(v: &Ver, chain: bool, escrow: bool) -> Value {
    let mut b = json!({"bytes": b64url(&v.bytes), "mac": b64url(&[1; 32])});
    if chain {
        b["chain"] = b64url(&[2; 32]).into();
    }
    if escrow {
        b["escrow_mac"] = b64url(&[3; 32]).into();
    }
    b
}

fn escrow(rr_gen: u32) -> Value {
    let w = || json!({"n": b64url(&[4; 24]), "ct": b64url(&[5; 48]), "commit": b64url(&[6; 32])});
    json!({"rr_gen": rr_gen, "rr_wrap_pp": w(), "rr_wrap_rk": w(), "pp_kdf": pp_kdf()})
}

/// A set-up account: device A holds the keys (ak_dir and pak_dir seq 1).
pub(super) struct Setup {
    pub(super) acc: Uuid,
    pub(super) owner: Browser,
    pub(super) a: Dev,
    ak: Ver,
    pak: Ver,
}

pub(super) async fn setup(e: &Env, email: &str) -> Setup {
    let mut owner = signed_in(e, email).await;
    let a = new_dev(e, &mut owner).await;
    let acc = account_of(e, &a).await;
    let ak = ak_dir(acc, &a.id, 1, None, 1, &[&a.id], &[]);
    let pak = pak_dir(acc, &a.id, 1, None, 1, &[&a.id], 1, 7);
    let r = a
        .post(e, "keys/commit", json!({"ak_dir": blob(&ak, false, true), "pak_dir": blob(&pak, false, true), "if_absent": true, "escrow": escrow(1)}))
        .await;
    assert_eq!(
        (r.status, r.body["ak_seq"].as_i64(), r.body["pak_seq"].as_i64()),
        (StatusCode::OK, Some(1), Some(1)),
        "{}",
        r.raw
    );
    Setup { acc, owner, a, ak, pak }
}

#[tokio::test]
async fn first_commit_head_chain_and_compare_and_swap() {
    let Some(e) = env().await else { return };
    let mut owner = signed_in(&e, "keys@example.org").await;
    let a = new_dev(&e, &mut owner).await;
    let acc = account_of(&e, &a).await;
    // No keyset yet.
    let st = a.get(&e, "sync/state").await;
    assert_eq!(
        (st.status, st.body["keyset_gen"].is_null(), st.body["current_epoch"].as_i64()),
        (StatusCode::OK, true, Some(0))
    );
    for p in ["keys/head", "keys/chain?from=anchor", "keys/escrow"] {
        let r = a.get(&e, p).await;
        assert_eq!((r.status, err(&r)), (StatusCode::NOT_FOUND, "no_keyset"), "{p}");
    }
    assert_eq!(
        owner.get(&e, "me").await.body["sync"],
        json!({"has_keyset": false, "records": 0, "bytes": 0, "rotation_pending": false})
    );
    let ak = ak_dir(acc, &a.id, 1, None, 1, &[&a.id], &[]);
    let pak = pak_dir(acc, &a.id, 1, None, 1, &[&a.id], 1, 7);
    // The first commit needs both directories and the escrow.
    for body in [
        json!({"ak_dir": blob(&ak, false, true), "if_absent": true, "escrow": escrow(1)}),
        json!({"ak_dir": blob(&ak, false, true), "pak_dir": blob(&pak, false, true), "if_absent": true}),
        json!({"ak_dir": blob(&ak, false, true), "pak_dir": blob(&pak, false, true), "escrow": escrow(1)}),
    ] {
        let r = a.post(&e, "keys/commit", body).await;
        assert!(r.status == StatusCode::BAD_REQUEST || r.status == StatusCode::NOT_FOUND, "{}", r.raw);
    }
    let first = json!({"ak_dir": blob(&ak, false, true), "pak_dir": blob(&pak, false, true), "if_absent": true, "escrow": escrow(1)});
    assert_eq!(a.post(&e, "keys/commit", first.clone()).await.status, StatusCode::OK);
    let again = a.post(&e, "keys/commit", first).await;
    assert_eq!(
        (again.status, again.body["head"]["ak"]["seq"].as_i64()),
        (StatusCode::CONFLICT, Some(1)),
        "if_absent twice"
    );
    // The head comes back byte-identical; the state and the website see the keyset.
    let h = a.get(&e, "keys/head").await;
    assert_eq!(h.body["ak_dir"]["bytes"], b64url(&ak.bytes));
    assert_eq!(h.body["pak_dir"]["escrow_mac"], b64url(&[3; 32]));
    let st = a.get(&e, "sync/state").await.body;
    assert_eq!(
        (st["current_epoch"].as_i64(), st["pak_epoch"].as_i64(), st["rotation"]["ak"].as_bool()),
        (Some(1), Some(1), Some(false))
    );
    assert!(st["keyset_gen"].as_str().is_some_and(|g| g.len() == 36));
    assert_eq!(st["quota"]["max_records"], 100_000);
    assert_eq!(owner.get(&e, "me").await.body["sync"]["has_keyset"], true);
    let devs = owner.get(&e, "devices").await.body;
    assert_eq!(devs[0]["has_keys"], true);

    // Device B is added (an approval's commit): seq 2 with the CAS on seq 1's hash.
    let b = new_dev(&e, &mut owner).await;
    let ak2 = ak_dir(acc, &a.id, 2, Some(&ak), 1, &[&a.id, &b.id], &[]);
    let add = json!({"ak_dir": blob(&ak2, false, false), "if_ak_prev_hash": ak.hash()});
    let r = a.post(&e, "keys/commit", add.clone()).await;
    assert_eq!(
        (r.status, r.body["ak_seq"].as_i64(), r.body["pak_seq"].as_i64()),
        (StatusCode::OK, Some(2), Some(1)),
        "{}",
        r.raw
    );
    let r = a.post(&e, "keys/commit", add).await;
    assert_eq!((r.status, r.body["head"]["ak"]["seq"].as_i64()), (StatusCode::CONFLICT, Some(2)), "a stale CAS loses");
    assert_eq!(r.body["head"]["ak"]["hash"], ak2.hash());
    // Without the CAS, or with a wrong keyset generation: conflict too.
    let ak3 = ak_dir(acc, &a.id, 3, Some(&ak2), 1, &[&a.id, &b.id], &[]);
    assert_eq!(
        a.post(&e, "keys/commit", json!({"ak_dir": blob(&ak3, false, false)})).await.status,
        StatusCode::CONFLICT
    );
    let gen = json!({"ak_dir": blob(&ak3, false, false), "if_ak_prev_hash": ak2.hash(), "keyset_gen": Uuid::new_v4()});
    assert_eq!(a.post(&e, "keys/commit", gen).await.status, StatusCode::CONFLICT);
    // B (now listed) may commit too; it sees the chain from the anchor.
    assert_eq!(
        owner.get(&e, "devices").await.body.as_array().unwrap().iter().filter(|d| d["has_keys"] == true).count(),
        2
    );
    let chain = b.get(&e, "keys/chain?from=anchor").await;
    assert_eq!((chain.body["ak"].as_array().unwrap().len(), chain.body["pak"].as_array().unwrap().len()), (2, 1));
    let chain = b.get(&e, "keys/chain?from=1,1").await;
    assert_eq!((chain.body["ak"].as_array().unwrap().len(), chain.body["pak"].as_array().unwrap().len()), (1, 0));
    assert_eq!(b.get(&e, "keys/chain?from=x").await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn commits_are_checked_for_structure() {
    let Some(e) = env().await else { return };
    let s = setup(&e, "structure@example.org").await;
    let (acc, a) = (s.acc, &s.a);
    let mut owner = s.owner;
    let other = setup(&e, "someone-else@example.org").await;
    let mut gone_b = owner.clone();
    let gone = new_dev(&e, &mut gone_b).await;
    owner.post(&e, "reauth", json!({"password": PW})).await;
    assert_eq!(owner.post(&e, &format!("devices/{}/revoke", gone.id), json!({})).await.status, StatusCode::OK);
    let ak_cas = s.ak.hash();
    let bad_ak = |v: Ver| json!({"ak_dir": blob(&v, false, false), "if_ak_prev_hash": ak_cas});
    let cases = [
        ("another account", bad_ak(ak_dir(other.acc, &a.id, 2, Some(&s.ak), 1, &[&a.id], &[]))),
        ("another author", bad_ak(ak_dir(acc, &other.a.id, 2, Some(&s.ak), 1, &[&a.id], &[]))),
        ("a foreign device", bad_ak(ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id, &other.a.id], &[]))),
        (
            "an unknown device",
            bad_ak(ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id, &Uuid::new_v4().to_string()], &[])),
        ),
        ("a revoked device", bad_ak(ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id, &gone.id], &[]))),
        ("a listed device also revoked", bad_ak(ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id], &[&a.id]))),
        ("a seq gap", bad_ak(ak_dir(acc, &a.id, 3, Some(&s.ak), 1, &[&a.id], &[]))),
        ("a wrong prev_hash", bad_ak(ak_dir(acc, &a.id, 2, Some(&other.ak), 1, &[&a.id], &[]))),
        ("epoch + 1 without a chain tag", bad_ak(ak_dir(acc, &a.id, 2, Some(&s.ak), 2, &[&a.id], &[]))),
        ("not JSON", json!({"ak_dir": {"bytes": b64url(b"nope"), "mac": b64url(&[1; 32])}, "if_ak_prev_hash": ak_cas})),
        (
            "a short tag",
            json!({"ak_dir": {"bytes": b64url(&s.ak.bytes), "mac": b64url(&[1; 8])}, "if_ak_prev_hash": ak_cas}),
        ),
        ("an unknown field", json!({"ak_dir": blob(&s.ak, false, false), "if_ak_prev_hash": ak_cas, "extra": 1})),
    ];
    for (what, body) in cases {
        let r = a.post(&e, "keys/commit", body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}: {}", r.raw);
    }
    // The passwords directory: the KDF parameters must match the escrow's, the escrow key may
    // change only with a new recovery root (and its escrow), vault keys only of listed devices.
    let pak_cas = s.pak.hash();
    let mut kdf_off =
        serde_json::from_slice::<Value>(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id], 1, 7).bytes).unwrap();
    kdf_off["pp_kdf"]["m_kib"] = 1024.into();
    let kdf_off = Ver { bytes: serde_json::to_vec(&kdf_off).unwrap() };
    let mut owner2 = owner.clone();
    let unlisted = new_dev(&e, &mut owner2).await;
    let pak_cases = [
        ("weaker KDF than the escrow's", json!({"pak_dir": blob(&kdf_off, false, false), "if_pak_prev_hash": pak_cas})),
        (
            "esc_pk swapped",
            json!({"pak_dir": blob(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id], 1, 8), false, false), "if_pak_prev_hash": pak_cas}),
        ),
        (
            "new rr_gen without escrow",
            json!({"pak_dir": blob(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id], 2, 8), false, false), "if_pak_prev_hash": pak_cas}),
        ),
        (
            "escrow without the directory",
            json!({"ak_dir": blob(&ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id], &[]), false, false), "if_ak_prev_hash": ak_cas, "escrow": escrow(2)}),
        ),
        (
            "escrow generation mismatch",
            json!({"pak_dir": blob(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id], 2, 8), false, false), "if_pak_prev_hash": pak_cas, "escrow": escrow(3)}),
        ),
        (
            "a vault key of an unlisted device",
            json!({"pak_dir": blob(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id, &unlisted.id], 1, 7), false, false), "if_pak_prev_hash": pak_cas}),
        ),
    ];
    for (what, body) in pak_cases {
        let r = a.post(&e, "keys/commit", body).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{what}: {}", r.raw);
    }
    // A recovery-root rotation with its escrow is fine (esc_pk changes, rr_gen + 1).
    let rr = json!({"pak_dir": blob(&pak_dir(acc, &a.id, 2, Some(&s.pak), 1, &[&a.id], 2, 8), false, true), "if_pak_prev_hash": pak_cas, "escrow": escrow(2)});
    let r = a.post(&e, "keys/commit", rr).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(a.get(&e, "sync/state").await.body["rr_gen"], 2);
    // A device that isn't active can't commit at all.
    assert_eq!(
        gone.post(&e, "keys/commit", json!({"ak_dir": blob(&s.ak, false, false)})).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn escrow_fetches_are_mailed_and_limited() {
    let Some(e) = env().await else { return };
    let s = setup(&e, "escrow@example.org").await;
    let before = outbox(&e.db).await.len();
    for _ in 0..5 {
        let r = s.a.get(&e, "keys/escrow").await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(keys_of(&r.body), ["account_id", "pp_kdf", "rr_gen", "rr_wrap_pp", "rr_wrap_rk"]);
        assert_eq!(r.body["account_id"], b64url(s.acc.as_bytes()));
    }
    let ob = outbox(&e.db).await;
    assert_eq!(ob[before..].iter().filter(|m| m.1 == "escrow_fetched").count(), 5);
    let r = s.a.get(&e, "keys/escrow").await;
    assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
}

fn keys_of(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

#[tokio::test]
async fn removing_a_device_freezes_until_the_next_epoch() {
    let Some(e) = env().await else { return };
    let s = setup(&e, "rotate@example.org").await;
    let (acc, a) = (s.acc, &s.a);
    let mut owner = s.owner;
    let b = new_dev(&e, &mut owner).await;
    let c = new_dev(&e, &mut owner).await;
    let nokeys = new_dev(&e, &mut owner).await;
    let ak2 = ak_dir(acc, &a.id, 2, Some(&s.ak), 1, &[&a.id, &b.id, &c.id], &[]);
    assert_eq!(
        a.post(&e, "keys/commit", json!({"ak_dir": blob(&ak2, false, false), "if_ak_prev_hash": s.ak.hash()}))
            .await
            .status,
        StatusCode::OK
    );
    // Removing a device that never had keys rotates nothing.
    owner.post(&e, "reauth", json!({"password": PW})).await;
    assert_eq!(owner.post(&e, &format!("devices/{}/revoke", nokeys.id), json!({})).await.status, StatusCode::OK);
    assert_eq!(a.get(&e, "sync/state").await.body["rotation"], json!({"ak": false, "pak": false}));
    // B is removed (from another device, the device API): both kinds freeze.
    assert_eq!(
        a.post(&e, &format!("devices/{}/revoke", a.id), json!({})).await.status,
        StatusCode::BAD_REQUEST,
        "not oneself"
    );
    let r = a.post(&e, &format!("devices/{}/revoke", b.id), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    assert_eq!(a.get(&e, "sync/state").await.body["rotation"], json!({"ak": true, "pak": true}));
    assert_eq!(owner.get(&e, "me").await.body["sync"]["rotation_pending"], true);
    assert_eq!(b.get(&e, "sync/state").await.status, StatusCode::UNAUTHORIZED, "B is out at once");
    assert_eq!(outbox(&e.db).await.last().unwrap().1, "device_revoked");
    // The lease: A claims, C can't, then the lease runs out and C can.
    assert_eq!(a.post(&e, "keys/rotation/claim", json!({"kind": "ak"})).await.status, StatusCode::OK);
    assert_eq!(c.post(&e, "keys/rotation/claim", json!({"kind": "ak"})).await.status, StatusCode::CONFLICT);
    assert_eq!(c.post(&e, "keys/rotation/claim", json!({"kind": "pak"})).await.status, StatusCode::OK, "another kind");
    assert_eq!(c.post(&e, "keys/rotation/claim", json!({"kind": "x"})).await.status, StatusCode::BAD_REQUEST);
    sqlx::query("UPDATE sync_state SET lease_ak_until = now() - interval '1 second'").execute(&e.db).await.unwrap();
    assert_eq!(c.post(&e, "keys/rotation/claim", json!({"kind": "ak"})).await.status, StatusCode::OK);
    // A same-epoch commit (dropping B but keeping the old key) is refused while frozen.
    let same = ak_dir(acc, &a.id, 3, Some(&ak2), 1, &[&a.id, &c.id], &[&b.id]);
    let r =
        a.post(&e, "keys/commit", json!({"ak_dir": blob(&same, false, false), "if_ak_prev_hash": ak2.hash()})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::LOCKED, "rotation_pending"));
    // B can't stay in or come back; the next epoch without B unfreezes the AK side only.
    let with_b = ak_dir(acc, &a.id, 3, Some(&ak2), 2, &[&a.id, &b.id, &c.id], &[]);
    assert_eq!(
        a.post(&e, "keys/commit", json!({"ak_dir": blob(&with_b, true, false), "if_ak_prev_hash": ak2.hash()}))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let next = ak_dir(acc, &a.id, 3, Some(&ak2), 2, &[&a.id, &c.id], &[&b.id]);
    let r = a.post(&e, "keys/commit", json!({"ak_dir": blob(&next, true, false), "if_ak_prev_hash": ak2.hash()})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let st = a.get(&e, "sync/state").await.body;
    assert_eq!((st["current_epoch"].as_i64(), st["rotation"].clone()), (Some(2), json!({"ak": false, "pak": true})));
    // The revoked list only grows.
    let shrink = ak_dir(acc, &a.id, 4, Some(&next), 2, &[&a.id, &c.id], &[]);
    assert_eq!(
        a.post(&e, "keys/commit", json!({"ak_dir": blob(&shrink, false, false), "if_ak_prev_hash": next.hash()}))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    // The PAK side unfreezes with its own next epoch.
    let pak2 = pak_dir(acc, &a.id, 2, Some(&s.pak), 2, &[&a.id], 1, 7);
    let r =
        a.post(&e, "keys/commit", json!({"pak_dir": blob(&pak2, true, false), "if_pak_prev_hash": s.pak.hash()})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let st = a.get(&e, "sync/state").await.body;
    assert_eq!((st["pak_epoch"].as_i64(), st["rotation"].clone()), (Some(2), json!({"ak": false, "pak": false})));
    assert_eq!(owner.get(&e, "me").await.body["sync"]["rotation_pending"], false);
    // Unlinking itself (C ever had keys) freezes again; so does the idle job's sign-out.
    assert_eq!(c.post(&e, "device/unlink", json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(a.get(&e, "sync/state").await.body["rotation"], json!({"ak": true, "pak": true}));
    sqlx::query("UPDATE sync_state SET rotation_ak = false, rotation_pak = false").execute(&e.db).await.unwrap();
    let d4 = new_dev(&e, &mut owner).await;
    sqlx::query("UPDATE devices SET had_keys = true WHERE id = $1::uuid").bind(&d4.id).execute(&e.db).await.unwrap();
    sqlx::query("UPDATE refresh_families SET rotated_at = now() - interval '91 days' WHERE device_id = $1::uuid")
        .bind(&d4.id)
        .execute(&e.db)
        .await
        .unwrap();
    crate::jobs::run_once(&e.db, 90).await.unwrap();
    assert_eq!(a.get(&e, "sync/state").await.body["rotation"], json!({"ak": true, "pak": true}));
    let _ = account_of(&e, &d4).await;
}

#[tokio::test]
async fn approvals_relay_in_order_once() {
    let Some(e) = env().await else { return };
    let s = setup(&e, "approve@example.org").await;
    let a = &s.a;
    let mut owner = s.owner;
    let n = new_dev(&e, &mut owner).await;
    let c = new_dev(&e, &mut owner).await;
    sqlx::query("UPDATE devices SET has_keys = true WHERE id = $1::uuid").bind(&c.id).execute(&e.db).await.unwrap();
    let k32 = |x: u8| b64url(&[x; 32]);
    let boxv = |x: u8| b64url(&[x; 64]);
    // Only a device without keys asks; one with keys can't.
    assert_eq!(a.post(&e, "approvals", json!({"commit_n": k32(1)})).await.status, StatusCode::CONFLICT);
    assert_eq!(n.post(&e, "approvals", json!({"commit_n": "short"})).await.status, StatusCode::BAD_REQUEST);
    let before = outbox(&e.db).await.len();
    let r = n.post(&e, "approvals", json!({"commit_n": k32(1)})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let id = r.body["approval_id"].as_str().unwrap().to_string();
    assert_eq!(outbox(&e.db).await[before..].iter().filter(|m| m.1 == "approval_requested").count(), 1);
    // A sees it (and N doesn't: no keys); the count is in sync/state.
    let list = a.get(&e, "approvals").await;
    assert_eq!(
        (list.body[0]["approval_id"].as_str(), list.body[0]["state"].as_str()),
        (Some(id.as_str()), Some("requested"))
    );
    assert_eq!(list.body[0]["device"]["id"], n.id);
    assert_eq!(n.get(&e, "approvals").await.status, StatusCode::FORBIDDEN);
    assert_eq!(a.get(&e, "sync/state").await.body["pending_approvals"], 1);
    // Out of order: N can't reveal or confirm yet, A can't deliver keys yet.
    let p = |x: &str| format!("approvals/{id}/{x}");
    assert_eq!(
        n.post(&e, &p("reveal"), json!({"epk_N": k32(2), "nonce_N": k32(3), "box_N": boxv(4)})).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        n.post(&e, &p("respond"), json!({"epk_A": k32(5), "nonce_A": k32(6)})).await.status,
        StatusCode::FORBIDDEN,
        "N can't answer itself"
    );
    assert_eq!(a.post(&e, &p("keys"), json!({"box_A": boxv(9)})).await.status, StatusCode::FORBIDDEN, "not A's yet");
    // A responds (once); C, another device with keys, is then out of it.
    assert_eq!(
        a.post(&e, &p("respond"), json!({"epk_A": k32(5), "nonce_A": k32(6)})).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        a.post(&e, &p("respond"), json!({"epk_A": k32(5), "nonce_A": k32(6)})).await.status,
        StatusCode::CONFLICT
    );
    assert_eq!(c.get(&e, &format!("approvals/{id}")).await.status, StatusCode::NOT_FOUND);
    assert!(c.get(&e, "approvals").await.body.as_array().unwrap().is_empty());
    let mine = n.get(&e, &format!("approvals/{id}")).await.body;
    assert_eq!(mine["approver"], a.id, "N learns A's device id (part of the transcript)");
    assert_eq!(
        (mine["epk_A"].as_str(), mine["nonce_A"].as_str(), mine.get("commit_N")),
        (Some(k32(5).as_str()), Some(k32(6).as_str()), None)
    );
    assert_eq!(
        n.post(&e, &p("reveal"), json!({"epk_N": k32(2), "nonce_N": k32(3), "box_N": boxv(4)})).await.status,
        StatusCode::NO_CONTENT
    );
    let seen = a.get(&e, &format!("approvals/{id}")).await.body;
    assert_eq!(
        (seen["commit_N"].as_str(), seen["box_N"].as_str(), seen["state"].as_str()),
        (Some(k32(1).as_str()), Some(boxv(4).as_str()), Some("revealed"))
    );
    assert!(seen.get("box_A").is_none());
    assert_eq!(n.post(&e, &p("confirm"), json!({"tag_N": k32(7)})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(n.post(&e, &p("confirm"), json!({"tag_N": k32(7)})).await.status, StatusCode::CONFLICT, "one attempt");
    assert_eq!(a.get(&e, &format!("approvals/{id}")).await.body["tag_N"], k32(7));
    assert_eq!(a.post(&e, &p("keys"), json!({"box_A": boxv(9)})).await.status, StatusCode::NO_CONTENT);
    // N collects box_A once; the approval is then gone.
    let done = n.get(&e, &format!("approvals/{id}")).await.body;
    assert_eq!((done["state"].as_str(), done["box_A"].as_str()), (Some("done"), Some(boxv(9).as_str())));
    assert_eq!(n.get(&e, &format!("approvals/{id}")).await.status, StatusCode::NOT_FOUND);

    // A cancel (a SAS mismatch) ends it for both; an expired one is gone too.
    let id2 =
        n.post(&e, "approvals", json!({"commit_n": k32(1)})).await.body["approval_id"].as_str().unwrap().to_string();
    assert_eq!(
        a.post(&e, &format!("approvals/{id2}/respond"), json!({"epk_A": k32(5), "nonce_A": k32(6)})).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(n.post(&e, &format!("approvals/{id2}/cancel"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(a.get(&e, &format!("approvals/{id2}")).await.status, StatusCode::NOT_FOUND);
    let id3 =
        n.post(&e, "approvals", json!({"commit_n": k32(1)})).await.body["approval_id"].as_str().unwrap().to_string();
    sqlx::query("UPDATE approvals SET expires_at = now() - interval '1 second'").execute(&e.db).await.unwrap();
    assert_eq!(
        a.post(&e, &format!("approvals/{id3}/respond"), json!({"epk_A": k32(5), "nonce_A": k32(6)})).await.status,
        StatusCode::NOT_FOUND
    );
    crate::jobs::run_once(&e.db, 90).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM approvals").fetch_one(&e.db).await.unwrap(), 0);
    // 5 per account per day (3 used).
    for _ in 0..2 {
        assert_eq!(n.post(&e, "approvals", json!({"commit_n": k32(1)})).await.status, StatusCode::OK);
    }
    let r = n.post(&e, "approvals", json!({"commit_n": k32(1)})).await;
    assert_eq!((r.status, err(&r)), (StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
}

#[tokio::test]
async fn the_device_api_lists_devices_and_sessions() {
    let Some(e) = env().await else { return };
    let s = setup(&e, "devapi@example.org").await;
    let mut owner = s.owner;
    let b = new_dev(&e, &mut owner).await;
    let list = s.a.get(&e, "devices").await.body;
    let me: Vec<&Value> = list.as_array().unwrap().iter().filter(|d| d["self"] == true).collect();
    assert_eq!((me.len(), me[0]["id"].as_str()), (1, Some(s.a.id.as_str())));
    let sessions = s.a.get(&e, "sessions").await.body;
    assert!(sessions.as_array().unwrap().iter().all(|x| x["current"] == false));
    let web =
        sessions.as_array().unwrap().iter().find(|x| x["kind"] == "web").unwrap()["id"].as_str().unwrap().to_string();
    // A device can end a website session (no re-auth: it holds the account's keys) …
    assert_eq!(s.a.post(&e, &format!("sessions/{web}/revoke"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(owner.get(&e, "me").await.status, StatusCode::UNAUTHORIZED);
    // … and remove another device through the sessions list, but not itself.
    assert_eq!(s.a.post(&e, &format!("sessions/{}/revoke", s.a.id), json!({})).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(s.a.post(&e, &format!("sessions/{}/revoke", b.id), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(b.get(&e, "devices").await.status, StatusCode::UNAUTHORIZED);
    // Without a token: 401 everywhere.
    for p in ["sync/state", "keys/head", "approvals", "devices", "sessions"] {
        assert_eq!(Device::new().call(&e, Method::GET, p, None, None).await.status, StatusCode::UNAUTHORIZED, "{p}");
    }
}

/// Migration 0004 is additive: an S3 database (0001–0003) with an account, two-step on, two
/// devices (one removed), sessions and mail migrates, and everything still works.
// Historical (it passed when that migration shipped): it seeds through today's code on the older
// schema, and since 0.13.0 that code writes the 0007 columns (email_log, web_sessions.via, the
// event source). The 0.13.0 counterpart, which seeds a 0006 database in SQL, is
// `s10::pre_0007_data_survives_the_expand_step`.
#[tokio::test]
#[ignore = "historical: today's code needs the 0007 schema to seed"]
async fn s3_data_survives_migration_0004() {
    let Some(e) = env().await else { return };
    let admin_url = std::env::var("KRUNK_TEST_DATABASE_URL").unwrap();
    let admin = PgPoolOptions::new().max_connections(1).connect(&admin_url).await.unwrap();
    let name = format!("krunk_it_s3_{}_{}", std::process::id(), next());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}"))).execute(&admin).await.unwrap();
    let url = format!("{}/{name}", admin_url.rsplit_once('/').unwrap().0);
    let db = PgPoolOptions::new().max_connections(5).connect(&url).await.unwrap();
    crate::db::MIGRATOR.run_to(3, &db).await.unwrap();
    let has_keys_table: bool =
        sqlx::query_scalar("SELECT to_regclass('ak_dirs') IS NOT NULL").fetch_one(&db).await.unwrap();
    assert!(!has_keys_table, "S3 schema only");
    let mut state = e.state.clone();
    let mut cfg = (*state.config).clone();
    cfg.database_url = url.clone();
    state.config = Arc::new(cfg);
    state.db = db.clone();
    let s3 = Env { state, db: db.clone() };
    // S3-shaped data made through the S3 code paths (the S3 handlers run against 0003 here).
    let mut owner = signed_in(&s3, "survivor3@example.org").await;
    let kept = Device::new();
    let (kept_id, _, refresh, _) = linked(&s3, &mut owner, &kept).await;
    let (gone_id, _, _, _) = linked(&s3, &mut owner, &Device::new()).await;
    sqlx::query("UPDATE devices SET revoked_at = now() WHERE id = $1::uuid").bind(&gone_id).execute(&db).await.unwrap();
    let count = "SELECT (SELECT count(*) FROM accounts), (SELECT count(*) FROM web_sessions), (SELECT count(*) FROM devices),
                        (SELECT count(*) FROM devices WHERE revoked_at IS NOT NULL), (SELECT count(*) FROM refresh_families),
                        (SELECT count(*) FROM email_outbox)";
    let before: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(count).fetch_one(&db).await.unwrap();
    crate::db::migrate(&db).await.unwrap();
    let after: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(count).fetch_one(&db).await.unwrap();
    assert_eq!(before, after);
    // The new columns have their defaults; the old session, the kept device's token and the
    // website still work, and the account can now set up sync.
    let flags: (bool, bool, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT has_keys, had_keys, last_sync_at FROM devices WHERE id = $1::uuid")
            .bind(&kept_id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(flags, (false, false, None));
    assert_eq!(owner.get(&s3, "me").await.body["sync"]["has_keyset"], false);
    let r = kept.post(&s3, "token/refresh", json!({"refresh_token": refresh})).await;
    assert_eq!(r.status, StatusCode::OK);
    let dev = Dev { d: kept, id: kept_id.clone(), token: r.body["access_token"].as_str().unwrap().to_string() };
    let acc = account_of(&s3, &dev).await;
    let ak = ak_dir(acc, &kept_id, 1, None, 1, &[&kept_id], &[]);
    let pak = pak_dir(acc, &kept_id, 1, None, 1, &[&kept_id], 1, 7);
    let r = dev
        .post(&s3, "keys/commit", json!({"ak_dir": blob(&ak, false, true), "pak_dir": blob(&pak, false, true), "if_absent": true, "escrow": escrow(1)}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.raw);
    let list = owner.get(&s3, "devices").await.body;
    assert_eq!(list.as_array().unwrap().len(), 2);
}

/// The S6 part of the log-redaction flow (called from `logs_hold_no_secrets`): a keyset, an
/// escrow fetch, an approval with its boxes. Returns the values that must never be logged.
pub(super) async fn s6_flow_secrets(e: &Env) -> Vec<String> {
    let s = setup(e, "s6log@example.org").await;
    let mut owner = s.owner;
    s.a.get(e, "keys/escrow").await;
    let n = new_dev(e, &mut owner).await;
    let (commit, epk_a, nonce_a, epk_n, nonce_n, tag) = (
        b64url(&[0x11; 32]),
        b64url(&[0x12; 32]),
        b64url(&[0x13; 32]),
        b64url(&[0x14; 32]),
        b64url(&[0x15; 32]),
        b64url(&[0x16; 32]),
    );
    let (box_n, box_a) = (b64url(&[0x17; 80]), b64url(&[0x18; 120]));
    let id =
        n.post(e, "approvals", json!({"commit_n": commit})).await.body["approval_id"].as_str().unwrap().to_string();
    s.a.post(e, &format!("approvals/{id}/respond"), json!({"epk_A": epk_a, "nonce_A": nonce_a})).await;
    n.post(e, &format!("approvals/{id}/reveal"), json!({"epk_N": epk_n, "nonce_N": nonce_n, "box_N": box_n})).await;
    n.post(e, &format!("approvals/{id}/confirm"), json!({"tag_N": tag})).await;
    s.a.post(e, &format!("approvals/{id}/keys"), json!({"box_A": box_a})).await;
    n.get(e, &format!("approvals/{id}")).await;
    let w = escrow(1);
    vec![
        commit,
        epk_a,
        nonce_a,
        epk_n,
        nonce_n,
        tag,
        box_n,
        box_a,
        b64url(&s.ak.bytes),
        b64url(&s.pak.bytes),
        w["rr_wrap_pp"]["ct"].as_str().unwrap().to_string(),
        s.a.token.clone(),
        n.token.clone(),
        "s6log".into(),
    ]
}
