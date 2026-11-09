//! The PAK collections against the in-memory server (S8, PLAN §9): logins and the never-save
//! list round-trip between two vaults, a conflict keeps the newer edit and the losing version,
//! a login for the same site and user adopts the shared id, records wait for a locked vault, a
//! swapped record raises the alarm, and the G4 check: everything in sync.json together cannot
//! open a password record.

use super::client::{self, Api, SyncFail};
use super::engine::{self, RunOpts};
use super::envelope::{self, RecordPlain};
use super::keys::{self, Me, PakPin, PakState};
use super::memserver::Mem;
use super::pak::{self, PakReport};
use super::state::{self, parse_account_id, AkPin, SyncState};
use crate::storage::{now_ms, Store};
use crate::vault::{KdfParams, NoWrap, Vault, VaultSync};
use serde_json::{json, Value};
use std::path::PathBuf;
use zeroize::Zeroizing;

const ACC: &str = "0b1c2d3e4f5061728394a5b6c7d8e9f0";
const PAK: [u8; 32] = [9; 32];
const PW: &str = "vault master password";

struct Dev {
    store: Store,
    st: SyncState,
    vault: Vault,
    path: PathBuf,
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn dev(name: &str, short: u16) -> Dev {
    let path = std::env::temp_dir().join(format!("lb-pak-{name}-{}-{:x}", std::process::id(), rand_tag()));
    let mut vault = Vault::create(&path, PW, KdfParams::insecure_for_tests(), &NoWrap).unwrap();
    let pak = PakState {
        vault_sk: Zeroizing::new([short as u8; 32]),
        pak: PakPin { pak_epoch: 1, pak: Zeroizing::new(PAK), seq: 1, hash: [0; 32], esc_pk: [0; 32], rr_gen: 1 },
        pak_old: vec![],
        book: Default::default(),
    };
    vault.set_sync_part(Some(VaultSync { state: pak.to_bytes(), dirty: vec![] }), &NoWrap).unwrap();
    let store = Store::open_in_memory().unwrap();
    store.sync_set_tracking(true).unwrap();
    let st = SyncState {
        account_id: ACC.into(),
        device_id: name.into(),
        dev_short: short,
        device_sk: Zeroizing::new([1; 32]),
        ak: Some(AkPin { epoch: 1, ak: Zeroizing::new([7; 32]), seq: 1, hash: [0; 32], min_record_epoch: 1 }),
        keyset_gen: "g".into(),
        ..Default::default()
    };
    Dev { store, st, vault, path }
}

fn rand_tag() -> u64 {
    u64::from_le_bytes(super::crypto::random::<8>().unwrap())
}

fn pak_of(d: &Dev) -> PakState {
    PakState::from_bytes(&d.vault.sync_part().unwrap().state).unwrap()
}

fn try_psync_at(d: &mut Dev, mem: &Mem, now: i64) -> Result<PakReport, SyncFail> {
    let api = Api { http: mem, token: "t", now_ms: now };
    let mut p = pak_of(d);
    pak::run(&api, &mut d.vault, &mut p, &d.store, &mut d.st, &[], &NoWrap)
}

fn psync(d: &mut Dev, mem: &Mem) -> PakReport {
    try_psync_at(d, mem, now_ms()).unwrap()
}

/// Runs the devices in turn until a full round moves nothing.
fn settle(devs: &mut [&mut Dev], mem: &Mem) {
    for _ in 0..6 {
        let mut moved = false;
        for d in devs.iter_mut() {
            let r = psync(d, mem);
            moved |= r.pushed > 0 || r.pulled > 0 || r.conflicts > 0;
        }
        if !moved {
            return;
        }
    }
    panic!("no steady state after 6 rounds");
}

/// (origin, username, password, note) by id, sorted.
fn logins(d: &Dev) -> Vec<(String, String, String, String, String)> {
    let mut v: Vec<_> = d
        .vault
        .entries()
        .iter()
        .map(|e| (e.id.clone(), e.origin.clone(), e.username.clone(), e.password.to_string(), e.note.to_string()))
        .collect();
    v.sort();
    v
}

fn pw(s: &str) -> Zeroizing<String> {
    Zeroizing::new(s.into())
}

#[test]
fn logins_and_the_never_save_list_round_trip() {
    let mem = Mem::new();
    let (mut a, mut b) = (dev("a", 1), dev("b", 2));
    let t = now_ms();
    let id1 = a.vault.save_login("https://mail.example/login", "ann", pw("first secret"), t, &NoWrap).unwrap();
    let id2 = a.vault.save_login("https://shop.example/", "ann@x", pw("shop secret"), t, &NoWrap).unwrap();
    a.store.never_save_add("https://bank.example", t).unwrap();
    let r = psync(&mut a, &mem);
    assert_eq!((r.pushed, r.pulled), (3, 0), "{r:?}");
    // The server holds no login in clear.
    let all =
        serde_json::to_string(&mem.0.borrow().records.values().map(|(_, e)| e.clone()).collect::<Vec<_>>()).unwrap();
    assert!(!all.contains("secret") && !all.contains("mail.example") && !all.contains("bank.example"));
    assert!(mem.0.borrow().records.values().all(|(_, e)| e["k"] == "pak"));
    let r = psync(&mut b, &mem);
    assert_eq!(r.pulled, 3);
    assert!(r.passwords && r.never_save);
    assert_eq!(logins(&a), logins(&b), "same logins under the same ids");
    assert!(b.store.never_save("https://bank.example").unwrap());
    // A's own records don't come back as changes; nothing to push.
    let r = psync(&mut a, &mem);
    assert_eq!((r.pushed, r.pulled, r.passwords), (0, 0, false), "{r:?}");
    // Edit, delete, never-save removed on B; A follows.
    b.vault
        .upsert(Some(&id1), "https://mail.example", "ann", Some(pw("second secret")), None, t + 10, &NoWrap)
        .unwrap();
    b.vault.delete(&id2, &NoWrap).unwrap();
    b.store.never_save_remove("https://bank.example").unwrap();
    b.store.never_save_add("https://other.example", t).unwrap();
    let r = psync(&mut b, &mem);
    assert_eq!(r.pushed, 4, "{r:?}");
    psync(&mut a, &mem);
    assert_eq!(logins(&a), logins(&b));
    assert_eq!(a.vault.entry(&id1).unwrap().password.as_str(), "second secret");
    assert!(a.vault.entry(&id2).is_none());
    assert_eq!(a.store.never_save_list().unwrap(), vec!["https://other.example".to_string()]);
    // A deletion by another device is kept as a previous version.
    assert!(pak_of(&a).book.previous.iter().any(|p| p.id == id2 && p.password.as_str() == "shop secret"));
    // Steady state: nothing moves.
    for d in [&mut a, &mut b] {
        let r = psync(d, &mem);
        assert_eq!((r.pushed, r.pulled), (0, 0), "{r:?}");
    }
}

#[test]
fn a_conflict_keeps_the_newer_edit_and_the_losing_version() {
    let mem = Mem::new();
    let (mut a, mut b) = (dev("a", 1), dev("b", 2));
    let t = now_ms();
    let id = a.vault.save_login("https://mail.example", "ann", pw("v1"), t, &NoWrap).unwrap();
    psync(&mut a, &mem);
    psync(&mut b, &mem);
    // Both edit offline; B's edit is later.
    a.vault.upsert(Some(&id), "https://mail.example", "ann", Some(pw("from a")), None, t + 100, &NoWrap).unwrap();
    b.vault.upsert(Some(&id), "https://mail.example", "ann", Some(pw("from b")), None, t + 200, &NoWrap).unwrap();
    psync(&mut a, &mem);
    let r = psync(&mut b, &mem);
    assert!(r.conflicts >= 1, "{r:?}");
    settle(&mut [&mut a, &mut b], &mem);
    assert_eq!(logins(&a), logins(&b));
    assert_eq!(a.vault.entry(&id).unwrap().password.as_str(), "from b", "the newer edit wins");
    let prev_a = pak_of(&a).book.previous;
    let prev_b = pak_of(&b).book.previous;
    assert!(
        prev_a.iter().chain(prev_b.iter()).any(|p| p.password.as_str() == "from a"),
        "the losing version is kept in a vault"
    );
    // 31 days later the previous versions are gone.
    let later = now_ms() + 31 * 86_400_000;
    try_psync_at(&mut a, &mem, later).unwrap();
    try_psync_at(&mut b, &mem, later).unwrap();
    assert!(pak_of(&a).book.previous.is_empty() && pak_of(&b).book.previous.is_empty());
}

#[test]
fn the_same_site_and_user_is_adopted_not_duplicated() {
    let mem = Mem::new();
    let (mut a, mut b) = (dev("a", 1), dev("b", 2));
    let t = now_ms();
    a.vault.save_login("https://mail.example", "ann", pw("older"), t, &NoWrap).unwrap();
    b.vault.save_login("https://mail.example", "ann", pw("newer"), t + 50, &NoWrap).unwrap();
    b.vault.save_login("https://b-only.example", "bob", pw("b"), t, &NoWrap).unwrap();
    settle(&mut [&mut a, &mut b], &mem);
    assert_eq!(logins(&a), logins(&b));
    let la = logins(&a);
    assert_eq!(la.len(), 2, "{la:?}");
    let mail = la.iter().find(|l| l.1 == "https://mail.example").unwrap();
    assert_eq!(mail.3, "newer", "the newer of the two");
}

#[test]
fn records_wait_for_the_vault_and_a_deletion_loses_to_an_edit() {
    let mem = Mem::new();
    let (mut a, mut b) = (dev("a", 1), dev("b", 2));
    let t = now_ms();
    let id = a.vault.save_login("https://mail.example", "ann", pw("v1"), t, &NoWrap).unwrap();
    psync(&mut a, &mem);
    // B's vault is locked: the AK run skips the PAK records and counts them.
    let api = Api { http: &mem, token: "t", now_ms: now_ms() };
    let rep = engine::run(&api, &mut b.store, &mut b.st, &RunOpts::default()).unwrap();
    assert_eq!(rep.pak_waiting, 1);
    assert!(b.vault.entries().is_empty());
    // Switched off: nothing.
    let mut p = pak_of(&b);
    let r = pak::run(&api, &mut b.vault, &mut p, &b.store, &mut b.st, &["passwords".into()], &NoWrap).unwrap();
    assert_eq!(r, PakReport::default());
    // Unlocked: they come.
    psync(&mut b, &mem);
    assert_eq!(logins(&a), logins(&b));
    // A deletes while B edits: the edit wins on both.
    a.vault.delete(&id, &NoWrap).unwrap();
    b.vault.upsert(Some(&id), "https://mail.example", "ann", Some(pw("kept")), None, t + 10, &NoWrap).unwrap();
    settle(&mut [&mut a, &mut b], &mem);
    assert_eq!(logins(&a), logins(&b));
    assert_eq!(a.vault.entry(&id).map(|e| e.password.to_string()).as_deref(), Some("kept"));
}

#[test]
fn a_swapped_or_forged_record_raises_the_alarm() {
    let mem = Mem::new();
    let (mut a, mut b) = (dev("a", 1), dev("b", 2));
    let t = now_ms();
    a.vault.save_login("https://one.example", "u1", pw("p1"), t, &NoWrap).unwrap();
    a.vault.save_login("https://two.example", "u2", pw("p2"), t, &NoWrap).unwrap();
    psync(&mut a, &mem);
    // The server serves record two's ciphertext under record one's id: the AAD binds the id.
    {
        let mut s = mem.0.borrow_mut();
        let keys: Vec<_> = s.records.keys().filter(|(c, _)| c == "passwords").cloned().collect();
        let (one, two) = (keys[0].clone(), keys[1].clone());
        let env_two = s.records[&two].1.clone();
        let mut swapped = env_two.clone();
        swapped["id"] = s.records[&one].1["id"].clone();
        s.head += 1;
        let seq = s.head;
        s.records.insert(one, (seq, swapped));
    }
    let e = try_psync_at(&mut b, &mem, now_ms()).unwrap_err();
    assert!(matches!(&e, SyncFail::Keys(k) if k.is_alarm()), "{e:?}");
    // A record sealed under a key the server made up fails too.
    let mem2 = Mem::new();
    let mut c = dev("c", 3);
    let acc = parse_account_id(ACC).unwrap();
    let keys = envelope::collection_keys(&[3; 32], &acc, "passwords", 1);
    let nk = "0123456789abcdef0123456789abcdef";
    let id = envelope::record_id(&keys.ik, nk.as_bytes());
    let plain = RecordPlain {
        ver: 1,
        dev: 9,
        deleted: false,
        data: json!({"id": nk, "origin": "https://evil.example", "username": "x", "password": "y", "note": "", "created": 1, "updated": 1}),
    };
    let env = envelope::seal_record(&keys.ck, &acc, "passwords", &id, 1, &plain, None).unwrap();
    {
        let mut s = mem2.0.borrow_mut();
        s.head = 1;
        s.records.insert(("passwords".into(), env.id.clone()), (1, serde_json::to_value(&env).unwrap()));
    }
    let e = try_psync_at(&mut c, &mem2, now_ms()).unwrap_err();
    assert!(matches!(&e, SyncFail::Keys(k) if k.is_alarm()), "{e:?}");
    assert!(c.vault.entries().is_empty(), "nothing was written");
}

/// THREAT_MODEL G4: with sync.json and its wrapper (DPAPI) but without the vault password, no
/// password plaintext can be derived. Every key sync.json holds, and every collection key derived
/// from them, fails to open a passwords record; PAK is not in the file.
#[test]
fn g4_sync_json_alone_cannot_open_a_password() {
    let acc = parse_account_id(ACC).unwrap();
    let plan = keys::plan_setup(acc, Me { device_id: "dev-a".into(), short: 1 }, true).unwrap();
    let (_commit, joined) = keys::setup_commit(&plan, 1).unwrap();
    let pak = joined.pak.clone().unwrap();
    let st = client::new_state(ACC, &Me { device_id: "dev-a".into(), short: 1 }, &joined, "g".into());
    // A login record under PAK.
    let ck = envelope::collection_keys(&pak.pak.pak, &acc, "passwords", 1);
    let nk = "00112233445566778899aabbccddeeff";
    let id = envelope::record_id(&ck.ik, nk.as_bytes());
    let plain = RecordPlain {
        ver: 5,
        dev: 1,
        deleted: false,
        data: json!({"id": nk, "origin": "https://bank.example", "username": "ann", "password": "hunter2-very-secret", "note": "", "created": 1, "updated": 1}),
    };
    let env = envelope::seal_record(&ck.ck, &acc, "passwords", &id, 1, &plain, None).unwrap();
    assert!(envelope::open_record(&ck.ck, &acc, &env).is_ok(), "PAK opens it");
    // sync.json, as written (the platform wrapper is a pass-through here: the attacker has it).
    let path = std::env::temp_dir().join(format!("lb-g4-{}-{:x}.json", std::process::id(), rand_tag()));
    state::save(&path, &st, &NoWrap).unwrap();
    let back = state::load(&path, &NoWrap).unwrap().unwrap();
    let file = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let clear = serde_json::to_vec(&back).unwrap();
    let pak_b64 = envelope::b64(pak.pak.pak.as_ref());
    for hay in [&file, &clear] {
        assert!(!contains(hay, pak_b64.as_bytes()) && !contains(hay, pak.pak.pak.as_ref()), "PAK is not in sync.json");
        assert!(!contains(hay, pak.vault_sk.as_ref()), "nor the vault key");
    }
    // Every 32-byte key in it, directly and as a root for the collection keys of every epoch
    // it knows (and a few beyond).
    let mut roots: Vec<[u8; 32]> = vec![*back.device_sk];
    if let Some(a) = &back.ak {
        roots.push(*a.ak);
    }
    roots.extend(back.ak_old.iter().map(|k| *k.key));
    for root in &roots {
        assert!(envelope::open_record(root, &acc, &env).is_err());
        for e in 0..4 {
            for c in ["passwords", "never_save", "history", "bookmarks", "settings"] {
                let k = envelope::collection_keys(root, &acc, c, e);
                assert!(envelope::open_record(&k.ck, &acc, &env).is_err(), "{c} {e}");
            }
        }
    }
    let as_value: Value = serde_json::from_slice(&clear).unwrap();
    assert!(as_value.get("pak").is_none() && as_value.get("vault_sk").is_none());
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn random_password_edits_converge() {
    let seeds: u64 = std::env::var("KRUNK_SYNC_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(100);
    for seed in 0..seeds {
        let mem = Mem::new();
        let mut devs = [dev("a", 1), dev("b", 2), dev("c", 3)];
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut t = now_ms();
        for _step in 0..25 {
            t += 1 + next(50) as i64;
            let i = next(3) as usize;
            let d = &mut devs[i];
            let site = format!("https://s{}.example", next(4));
            let user = format!("u{}", next(2));
            match next(6) {
                0 | 1 => {
                    d.vault.save_login(&site, &user, pw(&format!("p{}", next(1000))), t, &NoWrap).unwrap();
                }
                2 => {
                    let ids: Vec<String> = d.vault.entries().iter().map(|e| e.id.clone()).collect();
                    if !ids.is_empty() {
                        let id = &ids[next(ids.len() as u64) as usize];
                        d.vault.delete(id, &NoWrap).unwrap();
                    }
                }
                3 => {
                    let _ = d.store.never_save_add(&site, t);
                }
                4 => {
                    d.store.never_save_remove(&site).unwrap();
                }
                _ => {
                    psync(d, &mem);
                }
            }
        }
        let [a, b, c] = &mut devs;
        settle(&mut [a, b, c], &mem);
        assert_eq!(logins(&devs[0]), logins(&devs[1]), "seed {seed}");
        assert_eq!(logins(&devs[1]), logins(&devs[2]), "seed {seed}");
        let n0 = devs[0].store.never_save_list().unwrap();
        assert_eq!(n0, devs[1].store.never_save_list().unwrap(), "seed {seed}");
        assert_eq!(n0, devs[2].store.never_save_list().unwrap(), "seed {seed}");
        // One login per site and user name (save_login's own rule), even when two devices
        // saved the same one before they synced.
        let l = logins(&devs[0]);
        let mut pairs: Vec<_> = l.iter().map(|e| (e.1.clone(), e.2.clone())).collect();
        pairs.sort();
        let n = pairs.len();
        pairs.dedup();
        assert_eq!(pairs.len(), n, "seed {seed}: {l:?}");
    }
}
