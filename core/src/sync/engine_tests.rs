//! The engine against an in-memory server (S7 acceptance, PLAN §9): round trips of every AK
//! collection, the first-sync merge without duplicates, history as a union, device-local
//! settings that never leave, the 410 reconcile, offline and frozen runs, the bookmark tree
//! repair fixtures, and convergence under random concurrent edits (property tests).

use super::api::ApiError;
use super::client::{Api, SyncFail};
use super::coll;
use super::engine::{self, Report, RunOpts};
use super::envelope::{self, Envelope};
use super::memserver::Mem;
use super::state::{AkPin, SyncState};
use crate::storage::{now_ms, Store, BOOKMARKS_BAR, OTHER_BOOKMARKS};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

const ACC: &str = "0b1c2d3e-4f50-6172-8394-a5b6c7d8e9f0";
const AK: [u8; 32] = [7; 32];

struct Dev {
    store: Store,
    st: SyncState,
    name: String,
}

fn dev(id: &str, short: u16) -> Dev {
    let store = Store::open_in_memory().unwrap();
    store.sync_set_tracking(true).unwrap();
    let st = SyncState {
        account_id: ACC.into(),
        device_id: id.into(),
        dev_short: short,
        device_sk: Zeroizing::new([1; 32]),
        ak: Some(AkPin { epoch: 1, ak: Zeroizing::new(AK), seq: 1, hash: [0; 32], min_record_epoch: 1 }),
        keyset_gen: "g".into(),
        ..Default::default()
    };
    Dev { store, st, name: format!("Device {id}") }
}

fn try_sync(d: &mut Dev, mem: &Mem) -> Result<Report, SyncFail> {
    let api = Api { http: mem, token: "t", now_ms: now_ms() };
    let opts = RunOpts { device_name: &d.name, off: &[], max_rounds: 20 };
    engine::run(&api, &mut d.store, &mut d.st, &opts)
}

fn sync(d: &mut Dev, mem: &Mem) -> Report {
    try_sync(d, mem).unwrap()
}

/// Everything that must converge, in a comparable form.
#[derive(Debug, PartialEq, Eq)]
struct Snap {
    bookmarks: Vec<String>,
    history: BTreeMap<String, Vec<i64>>,
    pins: BTreeSet<(String, i64)>,
    allow: Vec<String>,
    homepage: String,
}

fn tree(s: &Store, parent: i64, depth: usize, out: &mut Vec<String>) {
    for b in s.children(parent).unwrap() {
        out.push(format!(
            "{}{} {} {}",
            "  ".repeat(depth),
            if b.is_folder { "+" } else { "-" },
            b.title,
            b.url.clone().unwrap_or_default()
        ));
        if b.is_folder {
            tree(s, b.id, depth + 1, out);
        }
    }
}

fn snap(d: &Dev) -> Snap {
    let mut bookmarks = Vec::new();
    for root in [BOOKMARKS_BAR, OTHER_BOOKMARKS] {
        bookmarks.push(format!("[{root}]"));
        tree(&d.store, root, 1, &mut bookmarks);
    }
    let c = d.store.sync_conn();
    let mut history = BTreeMap::new();
    let mut st =
        c.prepare("SELECT u.url, v.ts FROM urls u JOIN visits v ON v.url_id = u.id ORDER BY u.url, v.ts DESC").unwrap();
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap() {
        let (u, ts) = r.unwrap();
        let key = super::normalize::history_key(&u).unwrap_or(u);
        let e: &mut Vec<i64> = history.entry(key).or_default();
        if e.len() < 20 && !e.contains(&ts) {
            e.push(ts);
        }
    }
    let mut pins = BTreeSet::new();
    let mut st = c.prepare("SELECT url, state FROM tiles").unwrap();
    for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).unwrap() {
        pins.insert(r.unwrap());
    }
    Snap {
        bookmarks,
        history,
        pins,
        allow: d.store.allowlist().unwrap(),
        homepage: d.store.load_settings().unwrap().homepage,
    }
}

/// The plaintext of every record on the server (to check what left a device).
fn server_plaintexts(mem: &Mem) -> Vec<Value> {
    let acc = super::state::parse_account_id(ACC).unwrap();
    mem.0
        .borrow()
        .records
        .values()
        .map(|(_, env)| {
            let env: Envelope = serde_json::from_value(env.clone()).unwrap();
            let keys = envelope::collection_keys(&AK, &acc, &env.c, env.e);
            serde_json::to_value(envelope::open_record(&keys.ck, &acc, &env).unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn every_collection_round_trips_between_two_devices() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let mut b = dev("dev-b", 2);
    let f = a.store.add_folder(BOOKMARKS_BAR, "News").unwrap();
    a.store.add_bookmark(f, "Example", "https://example.com/").unwrap();
    a.store.add_bookmark(OTHER_BOOKMARKS, "Rust", "https://www.rust-lang.org/").unwrap();
    a.store.record_visit("https://example.com/a?utm_source=x", "A", 1_000).unwrap();
    a.store.record_visit("https://example.com/a?utm_source=x", "A", 2_000).unwrap();
    a.store.pin_site("https://pinned.example/", "Pinned").unwrap();
    a.store.allowlist_add("allowed.example", 5).unwrap();
    let mut s = a.store.load_settings().unwrap();
    s.homepage = "https://home.example/".into();
    a.store.save_settings(&s).unwrap();
    let ra = sync(&mut a, &mem);
    assert!(ra.pushed >= 7, "{ra:?}");
    assert_eq!(ra.queue_left, 0);
    let rb = sync(&mut b, &mem);
    assert!(
        rb.changed.bookmarks && rb.changed.history && rb.changed.settings && rb.changed.pins && rb.changed.allowlist,
        "{rb:?}"
    );
    let sb = snap(&b);
    assert_eq!(sb.homepage, "https://home.example/");
    assert!(
        sb.bookmarks.iter().any(|l| l.contains("+ News"))
            && sb.bookmarks.iter().any(|l| l.contains("- Example https://example.com/"))
    );
    assert_eq!(sb.history.get("https://example.com/a"), Some(&vec![2_000, 1_000]), "the normalized URL, both visits");
    assert!(sb.pins.contains(&("https://pinned.example/".into(), 1)));
    assert_eq!(sb.allow, vec!["allowed.example".to_string()]);
    // B changes things; A gets them.
    let rust = b.store.search_bookmarks("Rust").unwrap()[0].id;
    b.store.edit_bookmark(rust, "Rust lang", None).unwrap();
    b.store.allowlist_remove("allowed.example").unwrap();
    b.store.unpin_site("https://pinned.example/").unwrap();
    sync(&mut b, &mem);
    sync(&mut a, &mem);
    assert_eq!(snap(&a), snap(&b));
    // Nothing more to send when nothing changed.
    let again = sync(&mut a, &mem);
    assert_eq!((again.pushed, again.pulled), (0, 0), "{again:?}");
    // The tabs of the other device are there to show; this device's own aren't applied.
    a.store
        .kv_set(
            "session",
            r#"{"version":2,"windows":[{"tabs":[{"url":"https://open.example/","title":"Open"}],"active":0}]}"#,
        )
        .unwrap();
    sync(&mut a, &mem);
    sync(&mut b, &mem);
    let tabs = coll::remote_tabs(b.store.sync_conn()).unwrap();
    assert_eq!(tabs["dev-a"]["windows"][0]["tabs"][0]["url"], "https://open.example/");
    assert!(!tabs.contains_key("dev-b"));
    assert_eq!(coll::device_names(b.store.sync_conn()).unwrap()["dev-a"], "Device dev-a");
}

#[test]
fn a_device_local_setting_never_leaves_the_device() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let mut s = a.store.load_settings().unwrap();
    s.proxy = "proxy.example:3128".into();
    s.user_agent = "custom".into();
    s.homepage = "https://shared.example/".into();
    a.store.save_settings(&s).unwrap();
    sync(&mut a, &mem);
    let all = server_plaintexts(&mem);
    let text = serde_json::to_string(&all).unwrap();
    assert!(text.contains("https://shared.example/"));
    assert!(!text.contains("proxy.example") && !text.contains("\"custom\""), "device-local settings stay");
    assert!(all.iter().all(|p| p["data"]["key"] != "proxy" && p["data"]["key"] != "userAgent"));
}

#[test]
fn a_first_sync_merges_existing_bookmarks_without_duplicates() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let mut b = dev("dev-b", 2);
    for d in [&mut a, &mut b] {
        let f = d.store.add_folder(BOOKMARKS_BAR, "Work").unwrap();
        d.store.add_bookmark(f, "Mail", "https://mail.example/").unwrap();
        d.store.add_bookmark(BOOKMARKS_BAR, "News", "https://news.example/").unwrap();
    }
    b.store.add_bookmark(BOOKMARKS_BAR, "Only on B", "https://b.example/").unwrap();
    sync(&mut a, &mem);
    sync(&mut b, &mem);
    sync(&mut a, &mem);
    let sa = snap(&a);
    assert_eq!(sa, snap(&b));
    let count = |s: &Snap, what: &str| s.bookmarks.iter().filter(|l| l.contains(what)).count();
    assert_eq!(
        (count(&sa, "+ Work"), count(&sa, "- Mail"), count(&sa, "- News"), count(&sa, "Only on B")),
        (1, 1, 1, 1),
        "{:#?}",
        sa.bookmarks
    );
}

#[test]
fn history_is_a_union_of_visits() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let mut b = dev("dev-b", 2);
    a.store.record_visit("https://page.example/", "P", 1_000).unwrap();
    b.store.record_visit("https://page.example/", "P", 2_000).unwrap();
    sync(&mut a, &mem);
    sync(&mut b, &mem);
    sync(&mut a, &mem);
    assert_eq!(snap(&a).history["https://page.example/"], vec![2_000, 1_000]);
    assert_eq!(snap(&b).history["https://page.example/"], vec![2_000, 1_000]);
    // Deleting the page deletes it everywhere (a later visit elsewhere would keep it).
    a.store.delete_url("https://page.example/").unwrap();
    sync(&mut a, &mem);
    sync(&mut b, &mem);
    assert!(!snap(&b).history.contains_key("https://page.example/"));
}

#[test]
fn below_the_horizon_a_full_reconcile_deletes_nothing() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let mut b = dev("dev-b", 2);
    a.store.add_bookmark(BOOKMARKS_BAR, "One", "https://one.example/").unwrap();
    sync(&mut a, &mem);
    sync(&mut b, &mem);
    // Later records, then the server purges below B's cursor.
    a.store.add_bookmark(BOOKMARKS_BAR, "Two", "https://two.example/").unwrap();
    sync(&mut a, &mem);
    b.store.add_bookmark(BOOKMARKS_BAR, "Local on B", "https://local-b.example/").unwrap();
    mem.0.borrow_mut().horizon = 1_000;
    let r = sync(&mut b, &mem);
    assert!(r.full, "{r:?}");
    let sb = snap(&b);
    assert!(sb.bookmarks.iter().any(|l| l.contains("Two")) && sb.bookmarks.iter().any(|l| l.contains("Local on B")));
    sync(&mut a, &mem);
    assert_eq!(snap(&a), sb);
}

#[test]
fn offline_keeps_the_queue_and_frozen_pushes_nothing() {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    a.store.add_bookmark(BOOKMARKS_BAR, "Queued", "https://q.example/").unwrap();
    mem.0.borrow_mut().offline = true;
    let e = try_sync(&mut a, &mem).unwrap_err();
    assert!(matches!(e, SyncFail::Api(ApiError::Offline(_))), "{e:?}");
    assert!(super::local::queue_len(a.store.sync_conn()).unwrap() > 0, "the change waits");
    mem.0.borrow_mut().offline = false;
    mem.0.borrow_mut().frozen = true;
    let r = sync(&mut a, &mem);
    assert!(r.frozen && r.pushed == 0 && r.queue_left > 0, "{r:?}");
    mem.0.borrow_mut().frozen = false;
    let r = sync(&mut a, &mem);
    assert!(r.pushed > 0 && r.queue_left == 0, "{r:?}");
}

// ---- Tree repair fixtures (PROTOCOL §12 item 12) ----------------------------------------------

/// Pushes raw bookmark records (as a misbehaving or older device might) and returns B's tree.
fn tree_after(records: &[(&str, &str, &str, &str, &str)]) -> Vec<String> {
    let mem = Mem::new();
    let mut a = dev("dev-a", 1);
    let acc = super::state::parse_account_id(ACC).unwrap();
    let keys = envelope::collection_keys(&AK, &acc, "bookmarks", 1);
    for (i, (guid, parent, kind, title, sort)) in records.iter().enumerate() {
        let data = serde_json::json!({"guid": guid, "parent_guid": parent, "kind": kind, "title": title,
            "url": if *kind == "bookmark" { Value::String(format!("https://{title}.example/")) } else { Value::Null },
            "sort": sort, "added": 1, "modified": 1, "v_pos": 1, "v_text": 1});
        let plain = envelope::RecordPlain { ver: 100 + i as u64, dev: 9, deleted: false, data };
        let id = envelope::record_id(&keys.ik, guid.as_bytes());
        let env = envelope::seal_record(&keys.ck, &acc, "bookmarks", &id, 1, &plain, None).unwrap();
        let mut s = mem.0.borrow_mut();
        s.head += 1;
        let seq = s.head;
        s.records.insert(("bookmarks".into(), env.id.clone()), (seq, serde_json::to_value(&env).unwrap()));
    }
    sync(&mut a, &mem);
    let mut out = Vec::new();
    for root in [BOOKMARKS_BAR, OTHER_BOOKMARKS] {
        out.push(format!("[{root}]"));
        tree(&a.store, root, 1, &mut out);
    }
    out
}

const BAR: &str = "01010101010101010101010101010101";
const OTHER: &str = "02020202020202020202020202020202";
fn g(n: u8) -> String {
    format!("{n:02x}").repeat(16)
}

#[test]
fn tree_repair_fixtures() {
    let (g1, g2, g3, g4, g5) = (g(0x11), g(0x22), g(0x33), g(0x44), g(0x55));
    // 1. An orphan (unknown parent) goes to Other bookmarks.
    let t = tree_after(&[(&g1, &g(0x99), "bookmark", "orphan", "V")]);
    assert_eq!(t, ["[1]", "[2]", "  - orphan https://orphan.example/"]);
    // 2. Siblings by (sort, guid): a lower sort first, equal sorts by guid.
    let t = tree_after(&[
        (&g3, BAR, "bookmark", "c", "V"),
        (&g1, BAR, "bookmark", "a", "W"),
        (&g2, BAR, "bookmark", "b", "V"),
    ]);
    assert_eq!(t, ["[1]", "  - b https://b.example/", "  - c https://c.example/", "  - a https://a.example/", "[2]"]);
    // 3. A two-folder cycle: the lower guid moves to Other bookmarks.
    let t = tree_after(&[(&g1, &g2, "folder", "f1", "V"), (&g2, &g1, "folder", "f2", "V")]);
    assert_eq!(t, ["[1]", "[2]", "  + f1 ", "    + f2 "]);
    // 4. A three-folder cycle.
    let t = tree_after(&[
        (&g1, &g3, "folder", "f1", "V"),
        (&g2, &g1, "folder", "f2", "V"),
        (&g3, &g2, "folder", "f3", "V"),
    ]);
    assert_eq!(t, ["[1]", "[2]", "  + f1 ", "    + f2 ", "      + f3 "]);
    // 5. A folder that is its own parent.
    let t = tree_after(&[(&g1, &g1, "folder", "self", "V")]);
    assert_eq!(t, ["[1]", "[2]", "  + self "]);
    // 6. A record for a root is ignored (roots are fixed on every device).
    let t = tree_after(&[(BAR, OTHER, "folder", "evil", "V"), (&g1, BAR, "bookmark", "kept", "V")]);
    assert_eq!(t, ["[1]", "  - kept https://kept.example/", "[2]"]);
    // 7. A parent that is a bookmark, not a folder: the child goes to Other bookmarks.
    let t = tree_after(&[(&g1, BAR, "bookmark", "notafolder", "V"), (&g2, &g1, "bookmark", "child", "V")]);
    assert_eq!(t, ["[1]", "  - notafolder https://notafolder.example/", "[2]", "  - child https://child.example/"]);
    // 8. A child that arrives before its parent finds it once the parent arrives.
    let t = tree_after(&[(&g2, &g1, "bookmark", "early", "V"), (&g1, BAR, "folder", "late", "V")]);
    assert_eq!(t, ["[1]", "  + late ", "    - early https://early.example/", "[2]"]);
    // 9. A nested tree in the right order.
    let t = tree_after(&[
        (&g1, OTHER, "folder", "top", "V"),
        (&g2, &g1, "folder", "mid", "V"),
        (&g3, &g2, "bookmark", "leaf", "V"),
        (&g4, &g1, "bookmark", "first", "A"),
    ]);
    assert_eq!(
        t,
        [
            "[1]",
            "[2]",
            "  + top ",
            "    - first https://first.example/",
            "    + mid ",
            "      - leaf https://leaf.example/"
        ]
    );
    // 10. Invalid sort keys sort as the default; a bookmark without a usable URL is skipped.
    let t = tree_after(&[(&g5, BAR, "bookmark", "bad sort", "!!"), (&g4, BAR, "bookmark", "good", "A")]);
    assert_eq!(
        t,
        ["[1]", "  - good https://good.example/", "  - bad sort https://bad sort.example/", "[2]"]
            .iter()
            .filter(|l| !l.contains("bad sort"))
            .copied()
            .collect::<Vec<_>>()
    );
}

// ---- Convergence under random concurrent edits ---------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn random_op(d: &mut Dev, r: &mut Rng, step: usize) {
    let folders: Vec<i64> = {
        let mut all = vec![BOOKMARKS_BAR, OTHER_BOOKMARKS];
        let mut st = d.store.sync_conn().prepare("SELECT id FROM bookmarks WHERE is_folder = 1 AND id > 2").unwrap();
        all.extend(st.query_map([], |x| x.get::<_, i64>(0)).unwrap().map(Result::unwrap));
        all
    };
    let items: Vec<i64> = {
        let mut st = d.store.sync_conn().prepare("SELECT id FROM bookmarks WHERE id > 2").unwrap();
        st.query_map([], |x| x.get::<_, i64>(0)).unwrap().map(Result::unwrap).collect()
    };
    let url = format!("https://site{}.example/", r.below(12));
    match r.below(10) {
        0 | 1 => {
            let _ = d.store.add_bookmark(folders[r.below(folders.len())], &format!("b{step}"), &url);
        }
        2 => {
            let _ = d.store.add_folder(folders[r.below(folders.len())], &format!("f{step}"));
        }
        3 if !items.is_empty() => {
            let _ = d.store.edit_bookmark(items[r.below(items.len())], &format!("renamed{step}"), None);
        }
        4 if !items.is_empty() => {
            let _ =
                d.store.move_bookmark(items[r.below(items.len())], folders[r.below(folders.len())], r.below(4) as u32);
        }
        5 if !items.is_empty() => {
            let _ = d.store.delete_bookmark(items[r.below(items.len())]);
        }
        6 => {
            let _ = d.store.record_visit(&url, "t", 1_000_000 + (r.below(50) as i64) * 1000 + step as i64);
        }
        7 => {
            if r.below(2) == 0 {
                let _ = d.store.pin_site(&url, "p");
            } else {
                let _ = d.store.unpin_site(&url);
            }
        }
        8 => {
            let host = format!("h{}.example", r.below(5));
            if r.below(2) == 0 {
                let _ = d.store.allowlist_add(&host, 1);
            } else {
                let _ = d.store.allowlist_remove(&host);
            }
        }
        _ => {
            let mut s = d.store.load_settings().unwrap();
            s.homepage = format!("https://home{}.example/", r.below(4));
            d.store.save_settings(&s).unwrap();
        }
    }
}

#[test]
fn random_concurrent_edits_converge() {
    let seeds: u64 = std::env::var("KRUNK_SYNC_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(500);
    for seed in 1..=seeds {
        let mem = Mem::new();
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let n = 2 + (seed % 2) as usize;
        let mut devs: Vec<Dev> = (0..n).map(|i| dev(&format!("dev-{i}"), i as u16 + 1)).collect();
        for step in 0..30 {
            let i = r.below(n);
            if r.below(4) == 0 {
                sync(&mut devs[i], &mem);
            } else {
                random_op(&mut devs[i], &mut r, step);
            }
        }
        // Everyone syncs until nothing moves.
        for _ in 0..4 {
            for d in devs.iter_mut() {
                sync(d, &mem);
            }
        }
        let first = snap(&devs[0]);
        for (i, d) in devs.iter().enumerate().skip(1) {
            let s = snap(d);
            assert_eq!(first, s, "seed {seed}: device 0 and {i} differ");
        }
    }
}
