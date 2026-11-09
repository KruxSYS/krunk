//! Revocation and rotation against the in-memory server (S9, PLAN §12 item 13), with the real key
//! flows: A sets up, the others restore with the passphrase, devices are removed, keys rotate.
//! - a removed device can't decrypt the next epoch; its open tabs and name become tombstones;
//! - writes frozen with no device online: a restorer joins and rotates in one commit;
//! - a CAS race between two rotators: one wins, the other takes the winner's directory;
//! - `min_record_epoch`: a record replayed under the old epoch is never applied;
//! - a phantom device (listed by the server, not in the directory) gets no seal;
//! - a key the server injects into the directories fails the MAC (alarm);
//! - AK-only malware can't add a vault key; the passwords key rotates too;
//! - the local queue is kept while writes are frozen;
//! - "Finish securing" writes a new anchor at the rotated heads.

use super::client::{self, Api, SyncFail};
use super::crypto::{self, KeyPair, SealKind};
use super::dir::{self, VaultKey};
use super::engine::{self, Report, RunOpts};
use super::envelope::{self, b64, unb64, Envelope};
use super::keys::{self, Me, PakState, Secret};
use super::memserver::Mem;
use super::pak;
use super::rotate::{self, AkRotation};
use super::state::{parse_account_id, SyncState};
use super::AccountId;
use crate::storage::{now_ms, Store, BOOKMARKS_BAR};
use crate::vault::{KdfParams, NoWrap, Vault, VaultSync};
use serde_json::{json, Value};
use std::path::PathBuf;
use zeroize::Zeroizing;

const ACC: &str = "5a1c2d3e4f5061728394a5b6c7d8e9f0";

struct Dev {
    id: String,
    st: SyncState,
    store: Store,
    vault: Option<Vault>,
    path: PathBuf,
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acc() -> AccountId {
    parse_account_id(ACC).unwrap()
}

fn api<'a>(mem: &'a Mem, id: &'a str) -> Api<'a> {
    Api { http: mem, token: id, now_ms: now_ms() }
}

fn vault_with(id: &str, pak: Option<PakState>) -> (Option<Vault>, PathBuf) {
    let tag = u64::from_le_bytes(crypto::random::<8>().unwrap());
    let path = std::env::temp_dir().join(format!("lb-rot-{id}-{}-{tag:x}", std::process::id()));
    let Some(p) = pak else { return (None, path) };
    let mut v = Vault::create(&path, "vault password", KdfParams::insecure_for_tests(), &NoWrap).unwrap();
    v.set_sync_part(Some(VaultSync { state: p.to_bytes(), dirty: vec![] }), &NoWrap).unwrap();
    (Some(v), path)
}

fn store() -> Store {
    let s = Store::open_in_memory().unwrap();
    s.sync_set_tracking(true).unwrap();
    s
}

/// A sets up (with a vault); returns A and the passphrase.
fn first(mem: &Mem) -> (Dev, String) {
    let short = mem.link("A");
    let me = Me { device_id: "A".into(), short };
    let plan = keys::plan_setup(acc(), me.clone(), true).unwrap();
    let (commit, joined) = keys::setup_commit(&plan, now_ms() as u64).unwrap();
    client::commit(&api(mem, "A"), &commit).unwrap();
    let st = client::new_state(ACC, &me, &joined, "gen-1".into());
    let (vault, path) = vault_with("A", joined.pak.clone());
    (Dev { id: "A".into(), st, store: store(), vault, path }, plan.passphrase.to_string())
}

fn restore(mem: &Mem, id: &str, passphrase: &str, with_vault: bool) -> Dev {
    let short = mem.link(id);
    let me = Me { device_id: id.into(), short };
    let a = api(mem, id);
    let plan = client::restore_begin(&a, &Secret::Passphrase(Zeroizing::new(passphrase.into())), acc()).unwrap();
    let (joined, gen) = client::restore_finish(&a, &plan, &me, with_vault).unwrap();
    let st = client::new_state(ACC, &me, &joined, gen);
    let (vault, path) = vault_with(id, joined.pak.clone());
    Dev { id: id.into(), st, store: store(), vault, path }
}

fn run(d: &mut Dev, mem: &Mem) -> Result<Report, SyncFail> {
    let name = format!("Device {}", d.id);
    let a = api(mem, &d.id);
    engine::run(
        &a,
        &mut d.store,
        &mut d.st,
        &RunOpts { device_name: &name, confirm_rotation: true, ..Default::default() },
    )
}

fn pak_of(d: &Dev) -> PakState {
    PakState::from_bytes(&d.vault.as_ref().unwrap().sync_part().unwrap().state).unwrap()
}

fn prun(d: &mut Dev, mem: &Mem) -> pak::PakReport {
    let mut p = pak_of(d);
    let a = api(mem, &d.id);
    pak::run(&a, d.vault.as_mut().unwrap(), &mut p, &d.store, &mut d.st, &[], &NoWrap).unwrap()
}

fn bookmarks(d: &Dev) -> Vec<String> {
    let mut v: Vec<String> = d
        .store
        .children(BOOKMARKS_BAR)
        .unwrap()
        .into_iter()
        .map(|b| format!("{} {}", b.title, b.url.unwrap_or_default()))
        .collect();
    v.sort();
    v
}

/// Can any key `st` holds open this record (as the revoked device would try)?
fn opens(st: &SyncState, env: &Envelope) -> bool {
    let acc = acc();
    let mut roots: Vec<[u8; 32]> = st.ak_old.iter().map(|k| *k.key).collect();
    if let Some(a) = &st.ak {
        roots.push(*a.ak);
    }
    roots.iter().any(|r| {
        (0..5).any(|e| envelope::open_record(&envelope::collection_keys(r, &acc, &env.c, e).ck, &acc, env).is_ok())
    })
}

fn envelopes(mem: &Mem) -> Vec<Envelope> {
    mem.0.borrow().records.values().map(|(_, e)| serde_json::from_value(e.clone()).unwrap()).collect()
}

#[test]
fn a_removed_device_loses_the_next_epoch_and_its_tabs_go() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let mut b = restore(&mem, "B", &pass, false);
    let mut c = restore(&mem, "C", &pass, false);
    a.store.add_bookmark(BOOKMARKS_BAR, "Rust", "https://www.rust-lang.org/").unwrap();
    c.store
        .kv_set(
            "session",
            &json!({"version": 2, "windows": [{"tabs": [{"url": "https://c.example/", "title": "C"}], "active": 0}]})
                .to_string(),
        )
        .unwrap();
    for _ in 0..2 {
        for d in [&mut a, &mut b, &mut c] {
            run(d, &mem).unwrap();
        }
    }
    assert_eq!(bookmarks(&a), bookmarks(&b));
    let c_keys = c.st.clone();
    // C is removed on the website: writes freeze.
    mem.revoke("C");
    assert!(mem.0.borrow().rotation_ak);
    b.store.add_bookmark(BOOKMARKS_BAR, "While frozen", "https://frozen.example/").unwrap();
    // B's lease-less run: A holds the lease, B waits; its change stays queued.
    mem.0.borrow_mut().lease_ak = Some("A".into());
    let r = run(&mut b, &mem).unwrap();
    assert!(r.frozen && r.queue_left > 0 && r.rotated.is_none(), "{r:?}");
    mem.0.borrow_mut().lease_ak = None;
    // A rotates, rekeys and finishes.
    let r = run(&mut a, &mem).unwrap();
    assert_eq!(r.rotated, Some(2), "{r:?}");
    {
        let s = mem.0.borrow();
        assert!(!s.rotation_ak && s.epoch == 2);
        assert!(s.records.values().all(|(_, e)| e["k"] != "ak" || e["e"] == 2), "every AK record rekeyed");
    }
    let pin = a.st.ak.clone().unwrap();
    assert_eq!((pin.epoch, pin.min_record_epoch), (2, 2), "the final commit raised min_record_epoch");
    assert!(a.st.ak_old.is_empty() && a.st.secure_needed);
    // C, with everything it ever held, opens none of the new records.
    for env in envelopes(&mem).iter().filter(|e| e.k == envelope::KeyKind::Ak) {
        assert!(!opens(&c_keys, env), "{} readable by the removed device", env.c);
    }
    // C's tokens are gone too.
    assert!(matches!(run(&mut c, &mem), Err(SyncFail::Api(_))));
    // B follows the new epoch (sealed to it), pushes its queued change; everything converges.
    let r = run(&mut b, &mem).unwrap();
    assert!(r.pushed >= 1, "{r:?}");
    run(&mut a, &mem).unwrap();
    assert_eq!(b.st.ak.as_ref().unwrap().epoch, 2);
    assert_eq!(bookmarks(&a), bookmarks(&b));
    assert!(bookmarks(&a).iter().any(|x| x.contains("frozen.example")));
    // C's open tabs and name are tombstones now.
    let acc = acc();
    let ak2 = a.st.ak.clone().unwrap().ak;
    for c_name in ["tabs", "device_meta"] {
        let k = envelope::collection_keys(&ak2, &acc, c_name, 2);
        let id = b64(&envelope::record_id(&k.ik, b"C"));
        let s = mem.0.borrow();
        let (_, e) = s.records.get(&(c_name.to_string(), id)).unwrap_or_else(|| panic!("{c_name} of C was rekeyed"));
        let env: Envelope = serde_json::from_value(e.clone()).unwrap();
        assert!(envelope::open_record(&k.ck, &acc, &env).unwrap().deleted, "{c_name} of C");
    }
    assert!(!super::coll::remote_tabs(b.store.sync_conn()).unwrap().contains_key("C"), "B no longer shows C's tabs");
}

#[test]
fn with_no_device_online_a_restorer_rotates() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let mut b = restore(&mem, "B", &pass, false);
    a.store.add_bookmark(BOOKMARKS_BAR, "One", "https://one.example/").unwrap();
    run(&mut a, &mem).unwrap();
    run(&mut b, &mem).unwrap();
    let b_keys = b.st.clone();
    mem.revoke("B");
    // Nobody is online; D restores with the passphrase: it joins and rotates in one commit.
    let mut d = restore(&mem, "D", &pass, true);
    assert_eq!(d.st.ak.as_ref().unwrap().epoch, 2);
    assert!(!mem.0.borrow().rotation_ak && !mem.0.borrow().rotation_pak, "both keys rotated by the restorer");
    // Its first run rekeys; A, back online, follows.
    run(&mut d, &mem).unwrap();
    run(&mut a, &mem).unwrap();
    assert_eq!(a.st.ak.as_ref().unwrap().epoch, 2);
    assert_eq!(bookmarks(&a), bookmarks(&d));
    for env in envelopes(&mem).iter().filter(|e| e.k == envelope::KeyKind::Ak) {
        assert!(!opens(&b_keys, env));
    }
}

#[test]
fn two_rotators_race_and_one_wins() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let mut b = restore(&mem, "B", &pass, false);
    let _c = restore(&mem, "C", &pass, false);
    run(&mut a, &mem).unwrap();
    run(&mut b, &mem).unwrap();
    mem.revoke("C");
    mem.0.borrow_mut().ignore_leases = true;
    // B prepares its rotation on the current head, A commits first.
    let b_head = b.st.ak.clone().unwrap();
    assert_eq!(
        rotate::rotate_ak(&api(&mem, "A"), &mut a.st, a.store.sync_conn(), true).unwrap(),
        AkRotation::Rotated(2)
    );
    // B, still pinned to the old head, commits a rotation built on it: the CAS refuses it.
    let acc = acc();
    let (ak_blob, _) = client::head(&api(&mem, "A")).unwrap().unwrap();
    let mut dir_now = dir::parse_ak(&ak_blob.bytes).unwrap();
    dir_now.seq = b_head.seq + 1;
    dir_now.prev_hash = b64(&b_head.hash);
    let mine = [3u8; 32];
    let blob = dir::sign_ak(&dir_now, &mine, Some(&b_head.ak), None, &acc).unwrap();
    let lost = client::commit(
        &api(&mem, "B"),
        &keys::Commit { ak_dir: Some(blob), if_ak_prev_hash: Some(b_head.hash), ..Default::default() },
    );
    assert!(matches!(lost, Err(SyncFail::Api(super::api::ApiError::Conflict(_)))), "{lost:?}");
    assert_eq!(mem.0.borrow().cas_lost, 1);
    // B's next run takes A's directory and key.
    run(&mut b, &mem).unwrap();
    assert_eq!(*b.st.ak.as_ref().unwrap().ak, *a.st.ak.as_ref().unwrap().ak);
}

#[test]
fn old_epoch_replays_phantoms_and_injected_keys_are_refused() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let mut b = restore(&mem, "B", &pass, false);
    let c = restore(&mem, "C", &pass, false);
    run(&mut a, &mem).unwrap();
    run(&mut b, &mem).unwrap();
    // A phantom: the server lists P as an active device that no directory names.
    mem.link("P");
    mem.revoke("C");
    run(&mut a, &mem).unwrap();
    let (ak_blob, _) = client::head(&api(&mem, "A")).unwrap().unwrap();
    let d = dir::parse_ak(&ak_blob.bytes).unwrap();
    let mut to: Vec<&str> = d.seals.iter().map(|s| s.to.as_str()).collect();
    to.sort();
    assert_eq!(to, ["A", "B", "escrow"], "no seal for the phantom or the removed device");
    assert!(d.revoked.contains(&"C".to_string()));
    run(&mut b, &mem).unwrap();
    // The removed C (with the server) replays a bookmark edit under epoch 1: never applied. The
    // same edit under the current epoch is (the control).
    a.store.add_bookmark(BOOKMARKS_BAR, "Real", "https://real.example/").unwrap();
    run(&mut a, &mem).unwrap();
    run(&mut b, &mem).unwrap();
    let acc = acc();
    let ak2 = a.st.ak.clone().unwrap().ak;
    let (key, cur) = {
        let s = mem.0.borrow();
        let k2 = envelope::collection_keys(&ak2, &acc, "bookmarks", 2);
        s.records
            .iter()
            .filter(|((c, _), _)| c == "bookmarks")
            .map(|(k, (_, e))| (k.clone(), serde_json::from_value::<Envelope>(e.clone()).unwrap()))
            .find_map(|(k, env)| {
                let p = envelope::open_record(&k2.ck, &acc, &env).unwrap();
                (p.data["url"] == "https://real.example/").then_some((k, p))
            })
            .unwrap()
    };
    let guid = cur.data["guid"].as_str().unwrap().to_string();
    let edit = |epoch: u32, root: &[u8; 32], title: &str| {
        let mut p = cur.clone();
        p.ver = cur.ver + 1_000_000;
        p.data["title"] = json!(title);
        p.data["v_text"] = json!(p.ver);
        let k = envelope::collection_keys(root, &acc, "bookmarks", epoch);
        let id = envelope::record_id(&k.ik, guid.as_bytes());
        envelope::seal_record(&k.ck, &acc, "bookmarks", &id, epoch, &p, None).unwrap()
    };
    let put = |env: &Envelope| {
        let mut s = mem.0.borrow_mut();
        s.head += 1;
        let seq = s.head;
        s.records.insert(("bookmarks".into(), env.id.clone()), (seq, serde_json::to_value(env).unwrap()));
    };
    let ak1 = c.st.ak.clone().unwrap().ak;
    put(&edit(1, &ak1, "Replayed under epoch 1"));
    run(&mut b, &mem).unwrap();
    assert!(!bookmarks(&b).iter().any(|x| x.contains("Replayed")), "an old-epoch record is never applied");
    assert!(mem.0.borrow().records.contains_key(&key));
    put(&edit(2, &ak2, "Edited under epoch 2"));
    run(&mut b, &mem).unwrap();
    assert!(bookmarks(&b).iter().any(|x| x.contains("Edited under epoch 2")), "control: {:?}", bookmarks(&b));
    // The server injects the next epoch with a key of its own (it can't make the chain tag).
    let evil = [6u8; 32];
    let pin = b.st.ak.clone().unwrap();
    let (head_blob, _) = client::head(&api(&mem, "B")).unwrap().unwrap();
    let mut forged = dir::parse_ak(&head_blob.bytes).unwrap();
    forged.epoch = pin.epoch + 1;
    forged.seq = pin.seq + 1;
    forged.prev_hash = b64(&pin.hash);
    let b_pk = KeyPair::from_secret(*b.st.device_sk).pk;
    forged.seals = vec![dir::seal_key(SealKind::Ak, &evil, &acc, forged.epoch, "B", &b_pk).unwrap()];
    let fb = dir::sign_ak(&forged, &evil, Some(&evil), None, &acc).unwrap();
    {
        let mut s = mem.0.borrow_mut();
        let mut w = serde_json::to_value(client::BlobWire::from_blob(&fb)).unwrap();
        w["seq"] = json!(forged.seq);
        s.ak_dirs.push(w);
        s.epoch = forged.epoch;
    }
    let e = run(&mut b, &mem).unwrap_err();
    assert!(matches!(&e, SyncFail::Keys(k) if k.is_alarm()), "{e:?}");
}

#[test]
fn the_passwords_key_rotates_and_ak_only_malware_cannot_add_a_vault_key() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let mut b = restore(&mem, "B", &pass, true);
    let c = restore(&mem, "C", &pass, true);
    let t = now_ms();
    a.vault
        .as_mut()
        .unwrap()
        .save_login("https://mail.example", "ann", Zeroizing::new("s3cret".into()), t, &NoWrap)
        .unwrap();
    for d in [&mut a, &mut b] {
        run(d, &mem).unwrap();
        prun(d, &mem);
    }
    prun(&mut a, &mem);
    prun(&mut b, &mem);
    assert_eq!(b.vault.as_ref().unwrap().entries().len(), 1);
    // AK-only malware on B (AK, no PAK) adds its own vault key: B's PAK MAC can't be made.
    let acc = acc();
    let (_, pak_blob) = client::head(&api(&mem, "A")).unwrap().unwrap();
    let mut pd = dir::parse_pak(&pak_blob.bytes).unwrap();
    pd.seq += 1;
    pd.prev_hash = b64(&dir::hash(&pak_blob.bytes));
    pd.vault_keys.push(VaultKey { device: "B".into(), pk: b64(&KeyPair::generate().unwrap().pk) });
    let forged = dir::sign_pak(&pd, &b.st.ak.as_ref().unwrap().ak, None, None, &acc).unwrap();
    client::commit(
        &api(&mem, "B"),
        &keys::Commit {
            pak_dir: Some(forged),
            if_pak_prev_hash: Some(dir::hash(&pak_blob.bytes)),
            ..Default::default()
        },
    )
    .unwrap();
    let mut pa = pak_of(&a);
    let e = client::refresh(&api(&mem, "A"), &mut a.st, Some(&mut pa)).unwrap_err();
    assert!(matches!(&e, SyncFail::Keys(k) if k.is_alarm()), "{e:?}");
    mem.0.borrow_mut().pak_dirs.pop();
    // C is removed: AK rotates on A's run, PAK on A's passwords run.
    let c_pak = pak_of(&c);
    mem.revoke("C");
    run(&mut a, &mem).unwrap();
    assert!(mem.0.borrow().rotation_pak);
    prun(&mut a, &mem);
    assert!(!mem.0.borrow().rotation_pak);
    let pa = pak_of(&a);
    assert_eq!(pa.pak.pak_epoch, 2);
    assert!(pa.pak_old.is_empty(), "the rekey finished and the old key is dropped");
    {
        let s = mem.0.borrow();
        assert!(s.records.values().all(|(_, e)| e["k"] != "pak" || e["e"] == 2));
    }
    // C's PAK opens none of the passwords records any more.
    for env in envelopes(&mem).iter().filter(|e| e.k == envelope::KeyKind::Pak) {
        let k = envelope::collection_keys(&c_pak.pak.pak, &acc, &env.c, env.e);
        assert!(envelope::open_record(&k.ck, &acc, env).is_err());
    }
    // B gets the new PAK through its seal and keeps syncing passwords.
    run(&mut b, &mem).unwrap();
    b.vault
        .as_mut()
        .unwrap()
        .save_login("https://new.example", "bob", Zeroizing::new("n".into()), t + 5, &NoWrap)
        .unwrap();
    prun(&mut b, &mem);
    prun(&mut a, &mem);
    assert_eq!(pak_of(&b).pak.pak_epoch, 2);
    assert_eq!(a.vault.as_ref().unwrap().entries().len(), 2);
}

#[test]
fn finish_securing_writes_a_new_anchor() {
    let mem = Mem::new();
    let (mut a, pass) = first(&mem);
    let _b = restore(&mem, "B", &pass, false);
    run(&mut a, &mem).unwrap();
    mem.revoke("B");
    run(&mut a, &mem).unwrap();
    assert!(a.st.secure_needed);
    let mut pa = pak_of(&a);
    prun(&mut a, &mem);
    pa = PakState::from_bytes(&a.vault.as_ref().unwrap().sync_part().unwrap().state).unwrap_or(pa);
    let secret = Secret::Passphrase(Zeroizing::new(pass.clone()));
    client::finish_securing(&api(&mem, "A"), &mut a.st, Some(&mut pa), &secret).unwrap();
    assert!(!a.st.secure_needed);
    // The chain from the anchor now starts at the rotated heads.
    let s = mem.0.borrow();
    let anchor = s.ak_dirs.iter().rposition(|b| b.get("escrow_mac").is_some_and(|m| !m.is_null())).unwrap();
    assert_eq!(anchor, s.ak_dirs.len() - 1);
    let f: Value = serde_json::from_slice(&unb64(s.ak_dirs[anchor]["bytes"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(f["epoch"], 2);
    drop(s);
    // A wrong passphrase changes nothing.
    let bad = Secret::Passphrase(Zeroizing::new("wrong words entirely here".into()));
    assert!(client::finish_securing(&api(&mem, "A"), &mut a.st, None, &bad).is_err());
}
