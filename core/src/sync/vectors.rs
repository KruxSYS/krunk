//! Test vectors (PROTOCOL §12) and robustness tests.
//!
//! - `rfc9180-a2.json`: the RFC's own HPKE vectors (X25519, HKDF-SHA256, ChaCha20Poly1305).
//!   They prove the `hpke` crate and our suite choice interoperate with any RFC 9180
//!   implementation (Android's Tink, for instance).
//! - `vectors-v1.json`: Krunk's formats, built from fixed inputs by [`build`] and frozen.
//!   Every run rebuilds the vectors and compares them with the file. Any change to a label,
//!   encoding or layout fails here, and the Kotlin port must match the same file.
//!   Regenerate deliberately with `KRUNK_GEN_VECTORS=1 cargo test -p core sync::vectors`.

use super::codes;
use super::crypto::{self, test_rng::DetRng, KdfParams, SealKind, Sealed, WrapKind, PASSPHRASE_KDF};
use super::dir::{self, AkDir, DeviceEntry};
use super::envelope::{self, RecordPlain};
use super::hlc;
use super::normalize;
use super::sas;
use super::{AccountId, SyncError};
use serde_json::{json, Value};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sync/");

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn arr<const N: usize>(v: &[u8]) -> [u8; N] {
    v.try_into().unwrap()
}

const ACC: AccountId = [0x4b, 0x52, 0x55, 0x4e, 0x4b, 0x2d, 0x41, 0x43, 0x43, 0x4f, 0x55, 0x4e, 0x54, 0x2d, 0x30, 0x31];
const AK: [u8; 32] = [0x11; 32];
const PAK: [u8; 32] = [0x22; 32];
const RR: [u8; 32] = [0x33; 32];

/// The 30 URL normalisation cases (PROTOCOL §9.6).
const URLS: &[&str] = &[
    "https://example.com",
    "https://example.com/",
    "HTTPS://EXAMPLE.COM/Path",
    "https://example.com:443/a",
    "http://example.com:80/a",
    "http://example.com:8080/a",
    "https://user:pass@example.com/x",
    "https://example.com/a#section",
    "https://example.com/a?b=1&c=2",
    "https://example.com/a?c=2&b=1",
    "https://example.com/?utm_source=news&utm_medium=email",
    "https://example.com/?id=7&utm_campaign=x&fbclid=abc",
    "https://example.com/?gclid=1",
    "https://www.youtube.com/watch?v=dQw4w9WgXcQ&si=abc",
    "https://example.com/?si=keep",
    "https://bücher.example/",
    "https://xn--bcher-kva.example/",
    "https://EXAMPLE.com./trailing-dot",
    "https://example.com/a b",
    "https://example.com/%7Euser",
    "https://example.com/caf%C3%A9",
    "https://example.com/café",
    "https://[2001:db8::1]/v6",
    "http://192.168.0.1/",
    "https://example.com/?",
    "https://example.com/?q=a%20b",
    "ftp://example.com/",
    "file:///C:/x.html",
    "about:blank",
    "javascript:alert(1)",
];

/// Builds every vector from fixed inputs. Deterministic: fixed nonces, a seeded RNG for the
/// HPKE ephemeral keys.
pub fn build() -> Value {
    // 1. Passphrase KDF at the shipped parameters.
    let argon: Vec<Value> = [("correct-horse-battery-staple-pine-hill", [0x01u8; 16]), ("kite-velvet-orbit-mango-trellis-ash", [0x02u8; 16])]
        .iter()
        .map(|(pp, salt)| {
            let kek = crypto::passphrase_kek(pp.as_bytes(), salt, &PASSPHRASE_KDF).unwrap();
            json!({"passphrase": pp, "salt": hex(salt), "m_kib": PASSPHRASE_KDF.m_kib, "t": PASSPHRASE_KDF.t, "p": PASSPHRASE_KDF.p, "kek": hex(kek.as_ref())})
        })
        .collect();

    // 2. HKDF outputs for every label.
    let labels = [
        super::labels::enc("history", 1),
        super::labels::id("history", 1),
        super::labels::enc("passwords", 1),
        super::labels::enc("never_save", 1),
        super::labels::ak_dir_mac(1),
        super::labels::pak_dir_mac(1),
        super::labels::ak_hist(2),
        super::labels::pak_hist(2),
        super::labels::device_name(1),
        super::labels::RK_KEK.to_string(),
        super::labels::ESCROW_X25519.to_string(),
        super::labels::ESCROW_MAC_AK.to_string(),
        super::labels::ESCROW_MAC_PAK.to_string(),
        super::labels::rr_commit("pp"),
        super::labels::rr_commit("rk"),
    ];
    let hkdf: Vec<Value> = labels
        .iter()
        .map(|l| json!({"ikm": hex(&AK), "salt": hex(&ACC), "info": l, "okm": hex(crypto::hkdf32(&AK, &ACC, l).as_ref())}))
        .collect();

    // 3. Recovery key and user code.
    let rks: Vec<Value> = [[0u8; 20], [0xffu8; 20], arr::<20>(&(1..=20).collect::<Vec<u8>>())]
        .iter()
        .map(|rk| json!({"bytes": hex(rk), "formatted": codes::format_recovery_key(rk), "kek_rk": hex(crypto::recovery_kek(rk, &ACC).as_ref())}))
        .collect();
    let user_code = json!({"bytes": hex(&[0x12, 0x34, 0x56, 0x78, 0x9a]), "formatted": codes::format_user_code(&[0x12, 0x34, 0x56, 0x78, 0x9a])});

    // 4. RR wraps.
    let rr_wraps: Vec<Value> = [(WrapKind::Passphrase, [0x44u8; 32], [0x05u8; 24]), (WrapKind::RecoveryKey, [0x55u8; 32], [0x06u8; 24])]
        .iter()
        .map(|(kind, kek, n)| {
            let w = crypto::rr_wrap_with_nonce(kek, &RR, *kind, &ACC, 1, n).unwrap();
            json!({"kind": kind.label(), "kek": hex(kek), "rr": hex(&RR), "rr_gen": 1, "n": hex(&w.n), "ct": hex(&w.ct), "commit": hex(&w.commit)})
        })
        .collect();

    // 5. HPKE seals with deterministic ephemerals; escrow keypair from RR.
    let esc = crypto::escrow_keypair(&RR, &ACC);
    let (dev_sk, dev_pk) = crypto::hpke_derive_keypair(&[0x66; 32]);
    let seals: Vec<Value> = [(SealKind::Ak, "dev-1", dev_pk, 1u32, AK), (SealKind::Pak, "escrow", esc.pk, 1, PAK)]
        .iter()
        .map(|(kind, to, pk, epoch, key)| {
            let info = crypto::seal_info(*kind, &ACC, *epoch, to).unwrap();
            let s = crypto::seal_with_rng(pk, &info, key, &mut DetRng::new(to.as_bytes())).unwrap();
            json!({"kind": kind.label(), "to": to, "epoch": epoch, "recipient_pk": hex(pk), "info": hex(&info), "pt": hex(key), "enc": hex(&s.enc), "ct": hex(&s.ct)})
        })
        .collect();

    // 7. Directory tags over fixed bytes.
    let d = AkDir {
        v: 1,
        account_id: envelope::b64(&ACC),
        epoch: 2,
        seq: 7,
        prev_hash: envelope::b64(&[0xaa; 32]),
        devices: vec![DeviceEntry {
            id: "dev-1".into(),
            short: 1,
            pk: envelope::b64(&dev_pk),
            added_by: "dev-1".into(),
            added_seq: 1,
        }],
        revoked: vec!["dev-9".into()],
        min_record_epoch: 2,
        seals: vec![],
        hist: None,
        created_ms: 1_793_840_000_000,
        created_by: "dev-1".into(),
    };
    let blob = dir::sign_ak(&d, &AK, Some(&[0x10; 32]), Some(&RR), &ACC).unwrap();
    let dirs = json!({"ak_dir_bytes": String::from_utf8(blob.bytes.clone()).unwrap(), "ak": hex(&AK), "prev_ak": hex(&[0x10; 32]), "rr": hex(&RR),
        "mac": hex(&blob.mac), "chain": hex(&blob.chain.unwrap()), "escrow_mac": hex(&blob.escrow_mac.unwrap()), "hash": hex(&dir::hash(&blob.bytes))});

    // 8. Records: id, AAD, padding boundaries, a full envelope.
    let keys = envelope::collection_keys(&AK, &ACC, "history", 1);
    let nk = normalize::history_key("https://example.com/a?b=1").unwrap();
    let id = envelope::record_id(&keys.ik, nk.as_bytes());
    let aad = envelope::aad(&ACC, "history", &id, envelope::KeyKind::Ak, 1).unwrap();
    let plain = RecordPlain {
        ver: hlc::encode(1_793_840_000_000, 3),
        dev: 1,
        deleted: false,
        data: json!({"url": nk, "title": "Example", "visits": [1793840000000u64], "count": 1}),
    };
    let env = envelope::seal_record_with_nonce(&keys.ck, &ACC, "history", &id, 1, &plain, None, &[0x07; 24]).unwrap();
    let padding: Vec<Value> = [0usize, 1, 254, 255, 256, 257, 511, 512, 65534, 65535]
        .iter()
        .map(|&n| json!({"len": n, "bucket": envelope::bucket_for(n).unwrap()}))
        .collect();
    let records = json!({"root": hex(&AK), "collection": "history", "epoch": 1, "ck": hex(keys.ck.as_ref()), "ik": hex(keys.ik.as_ref()),
        "natural_key": nk, "id": hex(&id), "aad": hex(&aad), "plain": serde_json::to_value(&plain).unwrap(), "envelope": serde_json::to_value(&env).unwrap(), "padding": padding});

    // 9. Device approval.
    let (esk_a, epk_a) = crypto::hpke_derive_keypair(&[0x0a; 32]);
    let (esk_n, epk_n) = crypto::hpke_derive_keypair(&[0x0b; 32]);
    let (nonce_a, nonce_n) = ([0x1a; 32], [0x1b; 32]);
    let commit = sas::commit(&epk_n, &nonce_n);
    let t = sas::Transcript {
        account_id: &ACC,
        approval_id: "appr-1",
        dev_id_a: "dev-1",
        dev_id_n: "dev-2",
        n_label: "Krunk 0.3 on Windows 11",
        n_platform: "windows",
        commit_n: &commit,
        epk_a: &epk_a,
        nonce_a: &nonce_a,
        epk_n: &epk_n,
        nonce_n: &nonce_n,
    }
    .bytes()
    .unwrap();
    let shared = sas::dh(&esk_a, &epk_n).unwrap();
    let derived = sas::derive(&shared, &t);
    let boxed =
        sas::box_seal_with_nonce(&derived.k_an, sas::Direction::AToN, b"{\"ak\":\"...\"}", &[0x0c; 24]).unwrap();
    let approval = json!({"esk_a": hex(&esk_a), "epk_a": hex(&epk_a), "esk_n": hex(&esk_n), "epk_n": hex(&epk_n), "nonce_a": hex(&nonce_a), "nonce_n": hex(&nonce_n),
        "commit_n": hex(&commit), "transcript": hex(&t), "dh": hex(shared.as_ref()), "sas": derived.sas, "sas_display": sas::format_sas(derived.sas),
        "k_an": hex(derived.k_an.as_ref()), "k_na": hex(derived.k_na.as_ref()), "confirm": hex(&sas::confirm_tag(&derived.k_na)), "box_a_to_n": hex(&boxed)});

    // 10. URLs.
    let urls: Vec<Value> = URLS.iter().map(|u| json!({"input": u, "key": normalize::history_key(u)})).collect();

    // 11. HLC.
    let mut h = hlc::Hlc::default();
    let ticks: Vec<u64> = [1000u64, 1000, 999, 2000].iter().map(|&now| h.tick(now)).collect();
    let hlc_v = json!({"encode_1793840000000_3": hlc::encode(1_793_840_000_000, 3), "ticks_at_1000_1000_999_2000": ticks,
        "clamp_now_1793840000000": h.observe(u64::MAX, 1_793_840_000_000).0});

    json!({
        "version": 1,
        "account_id": hex(&ACC),
        "notes": "Krunk sync v1 test vectors (docs/sync/PROTOCOL.md §12). Byte strings are hex; JSON bodies are exact bytes.",
        "argon2id": argon,
        "hkdf_sha256": hkdf,
        "recovery_keys": rks,
        "user_code": user_code,
        "rr_wraps": rr_wraps,
        "escrow": {"rr": hex(&RR), "esc_sk": hex(esc.sk.as_ref()), "esc_pk": hex(&esc.pk)},
        "hpke_seals": seals,
        "device_key": {"ikm": hex(&[0x66; 32]), "sk": hex(&dev_sk), "pk": hex(&dev_pk)},
        "directories": dirs,
        "records": records,
        "approval": approval,
        "urls": urls,
        "hlc": hlc_v,
    })
}

#[test]
fn frozen_vectors_match() {
    let built = build();
    let path = format!("{FIXTURES}vectors-v1.json");
    if std::env::var_os("KRUNK_GEN_VECTORS").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&built).unwrap() + "\n").unwrap();
    }
    let frozen: Value = serde_json::from_str(
        &std::fs::read_to_string(&path).expect("vectors-v1.json (run with KRUNK_GEN_VECTORS=1 once)"),
    )
    .unwrap();
    assert_eq!(built, frozen, "a sync format changed: if intended, bump the version and regenerate");
    // Every frozen seal opens with the frozen keys.
    for s in frozen["hpke_seals"].as_array().unwrap() {
        let sk: [u8; 32] = if s["to"] == "escrow" {
            *crypto::escrow_keypair(&RR, &ACC).sk
        } else {
            crypto::hpke_derive_keypair(&[0x66; 32]).0
        };
        let sealed = Sealed { enc: arr(&unhex(s["enc"].as_str().unwrap())), ct: unhex(s["ct"].as_str().unwrap()) };
        let pt = crypto::open(&sk, &unhex(s["info"].as_str().unwrap()), &sealed).unwrap();
        assert_eq!(hex(&pt), s["pt"]);
    }
    assert_eq!(frozen["urls"].as_array().unwrap().len(), 30);
}

#[test]
fn rfc9180_a2_interoperates() {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}rfc9180-a2.json")).unwrap()).unwrap();
    let s = |k: &str| unhex(v[k].as_str().unwrap());
    // DeriveKeyPair for both sides. The RFC prints private keys unclamped; the crate stores
    // the clamped X25519 scalar (RFC 7748 §5), so compare clamped forms. Public keys are exact.
    let clamp = |mut k: Vec<u8>| {
        k[0] &= 248;
        k[31] &= 127;
        k[31] |= 64;
        hex(&k)
    };
    let (sk_r, pk_r) = crypto::hpke_derive_keypair(&s("ikmR"));
    assert_eq!((clamp(sk_r.to_vec()), hex(&pk_r)), (clamp(s("skRm")), hex(&s("pkRm"))));
    let (sk_e, pk_e) = crypto::hpke_derive_keypair(&s("ikmE"));
    assert_eq!((clamp(sk_e.to_vec()), hex(&pk_e)), (clamp(s("skEm")), hex(&s("pkEm"))));
    // Sender side, with the RFC's ephemeral: same enc, same ciphertexts at every listed seq.
    use hpke::{Deserializable, Serializable};
    type K = hpke::kem::X25519HkdfSha256;
    let pk = <K as hpke::Kem>::PublicKey::from_bytes(&pk_r).unwrap();
    let mut rng = crypto::test_rng::DetRng::replay(&s("ikmE"));
    let (enc, mut ctx) = hpke::setup_sender_with_rng::<hpke::aead::ChaCha20Poly1305, hpke::kdf::HkdfSha256, K>(
        &hpke::OpModeS::Base,
        &pk,
        &s("info"),
        &mut rng,
    )
    .unwrap();
    assert_eq!(hex(enc.to_bytes().as_slice()), v["enc"]);
    let encs = v["encryptions"].as_array().unwrap();
    let pt = unhex(encs[0]["pt"].as_str().unwrap());
    for seq in 0..=256u64 {
        let aad = format!("Count-{seq}");
        let ct = ctx.seal(&pt, aad.as_bytes()).unwrap();
        if let Some(e) = encs.iter().find(|e| e["seq"] == seq) {
            assert_eq!(hex(&ct), e["ct"], "seq {seq}");
            assert_eq!(hex(aad.as_bytes()), e["aad"]);
        }
    }
    // Receiver side via our own open(): the first message (single-shot is seq 0).
    let first = &encs[0];
    let sealed = Sealed { enc: arr(&s("enc")), ct: unhex(first["ct"].as_str().unwrap()) };
    let sk = <K as hpke::Kem>::PrivateKey::from_bytes(&sk_r).unwrap();
    let encapped = <K as hpke::Kem>::EncappedKey::from_bytes(&sealed.enc).unwrap();
    let opened = hpke::single_shot_open::<hpke::aead::ChaCha20Poly1305, hpke::kdf::HkdfSha256, K>(
        &hpke::OpModeR::Base,
        &sk,
        &encapped,
        &s("info"),
        &sealed.ct,
        &unhex(first["aad"].as_str().unwrap()),
    )
    .unwrap();
    assert_eq!(hex(&opened), first["pt"]);
}

#[test]
fn frozen_rejection_cases() {
    // KDF floor.
    assert_eq!(
        crypto::passphrase_kek(b"x", &[0; 16], &KdfParams { m_kib: 65536, t: 3, p: 1 }).map(|_| ()),
        Err(SyncError::WeakKdf)
    );
    // rr_wrap: tampered commit, wrong length, wrong rr_gen.
    let w = crypto::rr_wrap_with_nonce(&[0x44; 32], &RR, WrapKind::Passphrase, &ACC, 1, &[5; 24]).unwrap();
    let mut t = w.clone();
    t.commit[0] ^= 1;
    assert_eq!(crypto::rr_unwrap(&[0x44; 32], &t, WrapKind::Passphrase, &ACC, 1).map(|_| ()), Err(SyncError::Auth));
    let mut l = w.clone();
    l.ct.truncate(47);
    assert!(crypto::rr_unwrap(&[0x44; 32], &l, WrapKind::Passphrase, &ACC, 1).is_err());
    assert_eq!(crypto::rr_unwrap(&[0x44; 32], &w, WrapKind::Passphrase, &ACC, 2).map(|_| ()), Err(SyncError::Auth));
    // Low-order X25519 points.
    for p in [[0u8; 32], {
        let mut b = [0u8; 32];
        b[0] = 1;
        b
    }] {
        assert_eq!(crypto::check_public_key(&p), Err(SyncError::LowOrderKey));
    }
}

// ---------------------------------------------------------------- robustness (no cargo-fuzz)

/// xorshift64*: a tiny seeded generator for mutation tests.
struct X(u64);
impl X {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// Every single-byte flip, every truncation and an extension of `data` must be rejected by
/// `check` (and never panic).
fn mutations_rejected(data: &[u8], check: impl Fn(&[u8]) -> bool) {
    for i in 0..data.len() {
        for bit in [0x01u8, 0x80] {
            let mut m = data.to_vec();
            m[i] ^= bit;
            assert!(!check(&m), "flip at {i}");
        }
    }
    for len in 0..data.len() {
        assert!(!check(&data[..len]), "truncation to {len}");
    }
    let mut ext = data.to_vec();
    ext.push(0);
    assert!(!check(&ext), "extension");
}

#[test]
fn mutated_envelopes_are_rejected() {
    let keys = envelope::collection_keys(&AK, &ACC, "bookmarks", 1);
    let id = envelope::record_id(&keys.ik, b"guid");
    let plain = RecordPlain { ver: 1, dev: 1, deleted: false, data: json!({"title": "t"}) };
    let env = envelope::seal_record(&keys.ck, &ACC, "bookmarks", &id, 1, &plain, None).unwrap();
    let json = serde_json::to_vec(&env).unwrap();
    // Any changed byte of the JSON either fails to parse, fails strict base64, or fails AEAD.
    mutations_rejected(&json, |b| {
        serde_json::from_slice::<envelope::Envelope>(b)
            .ok()
            .map(|e| envelope::open_record(&keys.ck, &ACC, &e).is_ok())
            .unwrap_or(false)
            && b != json.as_slice()
    });
    // 10,000 random envelopes: no panic, never accepted.
    let mut x = X(0x9e37_79b9_7f4a_7c15);
    for _ in 0..10_000 {
        let n = (x.next() % 400) as usize;
        let junk = x.bytes(n);
        if let Ok(e) = serde_json::from_slice::<envelope::Envelope>(&junk) {
            assert!(envelope::open_record(&keys.ck, &ACC, &e).is_err());
        }
        let e = envelope::Envelope {
            v: 1,
            c: "bookmarks".into(),
            id: envelope::b64(&x.bytes(16)),
            e: 1,
            k: envelope::KeyKind::Ak,
            n: envelope::b64(&x.bytes(24)),
            ct: envelope::b64(&x.bytes(n)),
        };
        assert!(envelope::open_record(&keys.ck, &ACC, &e).is_err());
    }
}

#[test]
fn mutated_directories_are_rejected() {
    let w = dir::tests::world();
    let mut d = w.ak_dir.clone();
    d.seq = 2;
    d.prev_hash = envelope::b64(&dir::hash(&w.ak_blob.bytes));
    let blob = dir::sign_ak(&d, &w.ak, None, None, &dir::tests::ACC).unwrap();
    let head = || dir::AkHead {
        account_id: dir::tests::ACC,
        epoch: 1,
        ak: zeroize::Zeroizing::new(w.ak),
        seq: 1,
        hash: dir::hash(&w.ak_blob.bytes),
        min_record_epoch: 1,
    };
    let me = dir::Opener::Device { id: "A", sk: &w.a.sk };
    assert!(matches!(dir::accept_ak(&head(), &blob, &me), Ok(dir::Outcome::Accepted(_))));
    mutations_rejected(&blob.bytes, |b| {
        let m = dir::Blob { bytes: b.to_vec(), ..blob.clone() };
        matches!(dir::accept_ak(&head(), &m, &me), Ok(dir::Outcome::Accepted(_)))
    });
    mutations_rejected(&blob.mac, |m| {
        let Ok(mac) = <[u8; 32]>::try_from(m) else { return false };
        matches!(dir::accept_ak(&head(), &dir::Blob { mac, ..blob.clone() }, &me), Ok(dir::Outcome::Accepted(_)))
    });
    let mut x = X(7);
    for _ in 0..10_000 {
        let n = (x.next() % 600) as usize;
        let junk = dir::Blob { bytes: x.bytes(n), mac: arr(&x.bytes(32)), chain: None, escrow_mac: None };
        assert!(!matches!(dir::accept_ak(&head(), &junk, &me), Ok(dir::Outcome::Accepted(_))));
    }
}

#[test]
fn mutated_wraps_boxes_and_codes_are_rejected() {
    let kek = [0x44; 32];
    let w = crypto::rr_wrap(&kek, &RR, WrapKind::Passphrase, &ACC, 1).unwrap();
    mutations_rejected(&w.ct, |ct| {
        crypto::rr_unwrap(&kek, &crypto::RrWrap { ct: ct.to_vec(), ..w.clone() }, WrapKind::Passphrase, &ACC, 1).is_ok()
    });
    mutations_rejected(&w.commit, |c| {
        let Ok(commit) = <[u8; 32]>::try_from(c) else { return false };
        crypto::rr_unwrap(&kek, &crypto::RrWrap { commit, ..w.clone() }, WrapKind::Passphrase, &ACC, 1).is_ok()
    });
    let b = sas::box_seal(&[9; 32], sas::Direction::AToN, b"keys").unwrap();
    mutations_rejected(&b, |m| sas::box_open(&[9; 32], sas::Direction::AToN, m).is_ok());
    let kp = crypto::KeyPair::generate().unwrap();
    let info = crypto::seal_info(SealKind::Ak, &ACC, 1, "x").unwrap();
    let s = crypto::seal(&kp.pk, &info, &AK).unwrap();
    mutations_rejected(&s.ct, |ct| crypto::open(&kp.sk, &info, &Sealed { enc: s.enc, ct: ct.to_vec() }).is_ok());
    // Random recovery-key strings: never panic; a valid parse needs the right check symbol.
    let mut x = X(99);
    let mut accepted = 0;
    for _ in 0..10_000 {
        let n = (x.next() % 60) as usize;
        let s: String = x
            .bytes(n)
            .iter()
            .map(|b| char::from(b"0123456789ABCDEFGHJKMNPQRSTVWXYZ-*~$=U kl"[(*b as usize) % 41]))
            .collect();
        if codes::parse_recovery_key(&s).is_ok() {
            accepted += 1;
        }
        let _ = codes::parse_user_code(&s);
    }
    assert_eq!(accepted, 0, "random strings are not valid recovery keys");
}
