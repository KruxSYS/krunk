use super::*;
use lb_core::recovery::Phrase;
use lb_core::vault::{NoWrap, VaultSync};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

const PW: &str = "correct horse battery";
const FIXTURE_PW: &str = "fixture password";
/// Some time in 2027 (ms).
const T0: i64 = 1_800_000_000_000;
const MINUTE: i64 = 60_000;

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> TestDir {
        let p = std::env::temp_dir().join(format!("krunk-ffi-vault-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TestDir(p)
    }
    /// The vault folder: not there until `create` makes it.
    fn dir(&self) -> PathBuf {
        self.0.join("krunk-vault")
    }
    fn file(&self) -> PathBuf {
        self.dir().join(VAULT_FILE)
    }
    fn state_file(&self) -> PathBuf {
        self.dir().join(STATE_FILE)
    }
    fn open(&self) -> Arc<KrunkVault> {
        KrunkVault::with_wrapper(self.dir(), Box::new(NoWrap), KdfParams::insecure_for_tests())
    }
    fn open_with(&self, wrapper: BoxedWrapper) -> Arc<KrunkVault> {
        KrunkVault::with_wrapper(self.dir(), wrapper, KdfParams::insecure_for_tests())
    }
    /// A vault file from elsewhere put in place.
    fn install(&self, source: &Path) {
        std::fs::create_dir_all(self.dir()).unwrap();
        std::fs::copy(source, self.file()).unwrap();
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Switches {
    fail_protect: AtomicBool,
    fail_unprotect: AtomicBool,
}

/// A wrapper that really changes the bytes (XOR) and can be made to fail, as a key store that
/// lost its key does.
struct Flaky {
    name: &'static str,
    switches: Arc<Switches>,
}

impl Flaky {
    fn boxed(name: &'static str) -> (BoxedWrapper, Arc<Switches>) {
        let switches = Arc::new(Switches::default());
        (Box::new(Flaky { name, switches: switches.clone() }), switches)
    }
    fn sharing(name: &'static str, switches: &Arc<Switches>) -> BoxedWrapper {
        Box::new(Flaky { name, switches: switches.clone() })
    }
}

fn xor(data: &[u8]) -> Vec<u8> {
    data.iter().map(|b| b ^ 0x5a).collect()
}

impl KeyWrapper for Flaky {
    fn name(&self) -> &'static str {
        self.name
    }
    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        if self.switches.fail_protect.load(Ordering::SeqCst) {
            return Err("test key store: cannot protect".into());
        }
        Ok(xor(data))
    }
    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        if self.switches.fail_unprotect.load(Ordering::SeqCst) {
            return Err("test key store: key gone".into());
        }
        Ok(Zeroizing::new(xor(data)))
    }
}

fn json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn s(text: &str) -> String {
    text.to_string()
}

fn add(v: &KrunkVault, url: &str, user: &str, password: &str, now: i64) -> String {
    v.save_login(s(url), s(user), s(password), now).unwrap()
}

fn users(matches: &[VaultMatch]) -> Vec<(&str, bool)> {
    matches.iter().map(|m| (m.username.as_str(), m.exact)).collect()
}

// ------------------------------------------------------------------------------ create, unlock, lock

#[test]
fn create_makes_an_unlocked_vault_and_never_replaces_one() {
    let dir = TestDir::new("create");
    let v = dir.open();
    assert_eq!(vault_min_password_len(), 8);
    assert_eq!(v.state(), VaultState::None);
    assert_eq!(v.summaries(), Err(VaultFfiError::NoVault));
    assert_eq!(v.unlock(s(PW), T0), Err(VaultFfiError::NoVault));
    assert_eq!(v.unlock_with_key(vec![0; 32]), Err(VaultFfiError::NoVault));
    assert!(!v.has_hint(s("https://bank.example/")));

    assert_eq!(v.create(s("7 chars"), T0), Err(VaultFfiError::WeakPassword));
    assert_eq!(v.state(), VaultState::None);
    assert!(!dir.file().exists());

    v.create(s("8 chars!"), T0).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    assert!(dir.file().is_file());
    assert_eq!(v.summaries().unwrap(), []);
    assert_eq!(json(&dir.file())["wrap"], "none");

    let before = std::fs::read(dir.file()).unwrap();
    assert_eq!(v.create(s("another password"), T0), Err(VaultFfiError::AlreadyExists));
    assert_eq!(v.create(s("short"), T0), Err(VaultFfiError::AlreadyExists));
    // Locked, or seen by a second object: the file is still never replaced.
    v.lock();
    assert_eq!(v.create(s("another password"), T0), Err(VaultFfiError::AlreadyExists));
    assert_eq!(dir.open().create(s("another password"), T0), Err(VaultFfiError::AlreadyExists));
    assert_eq!(std::fs::read(dir.file()).unwrap(), before);
}

#[test]
fn lock_drops_the_vault_and_every_entry_call_says_locked() {
    let dir = TestDir::new("lock");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let id = add(&v, "https://bank.example/login", "alice", "s3cret-one", T0);
    v.lock();
    assert_eq!(v.state(), VaultState::Locked);
    let locked = Err(VaultFfiError::Locked);
    assert_eq!(v.summaries(), locked);
    assert_eq!(v.search(s("bank")), Err(VaultFfiError::Locked));
    assert_eq!(v.entry(id.clone()), Err(VaultFfiError::Locked));
    assert_eq!(v.reveal(id.clone()), Err(VaultFfiError::Locked));
    assert_eq!(v.save_login(s("https://x.example/"), s("u"), s("p"), T0), Err(VaultFfiError::Locked));
    assert_eq!(v.upsert(Some(id.clone()), s("https://x.example/"), s("u"), None, None, T0), Err(VaultFfiError::Locked));
    assert_eq!(v.delete(id.clone()), Err(VaultFfiError::Locked));
    assert_eq!(v.matches_for(s("https://bank.example/")), Err(VaultFfiError::Locked));
    assert_eq!(v.for_http_auth(s("https://bank.example/")), Err(VaultFfiError::Locked));
    assert_eq!(v.secret_for_fill(id.clone(), s("https://bank.example/")), Err(VaultFfiError::Locked));
    assert_eq!(v.save_decision(s("https://bank.example/"), s("alice"), s("x")), Err(VaultFfiError::Locked));
    assert_eq!(v.unlock_key(), Err(VaultFfiError::Locked));
    assert_eq!(v.change_password(s(PW), s("new password 123")), Err(VaultFfiError::Locked));

    v.unlock(s(PW), T0).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    assert_eq!(v.reveal(id).unwrap(), "s3cret-one");
    // A second object over the same folder starts locked and opens the same vault.
    let again = dir.open();
    assert_eq!(again.state(), VaultState::Locked);
    again.unlock(s(PW), T0).unwrap();
    assert_eq!(again.summaries().unwrap().len(), 1);
}

#[test]
fn a_wrong_password_is_refused_and_slows_the_next_attempt() {
    let dir = TestDir::new("rate");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    v.lock();
    assert_eq!(v.retry_wait_ms(T0), 0);

    let delays = [1_000u64, 2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000];
    let mut now = T0;
    for (n, delay) in delays.into_iter().enumerate() {
        assert_eq!(v.unlock(s("wrong password"), now), Err(VaultFfiError::WrongPassword), "attempt {n}");
        assert_eq!(v.state(), VaultState::Locked);
        assert_eq!(v.retry_wait_ms(now), delay);
        // While the delay runs nothing is tried: not even the right password gets in, and the
        // refusal does not count as another failure.
        let early = now + i64::try_from(delay).unwrap() - 1;
        assert_eq!(v.unlock(s(PW), early), Err(VaultFfiError::RateLimited { wait_ms: 1 }));
        assert_eq!(v.retry_wait_ms(early), 1);
        if n == 2 {
            // Kept across a restart of the app.
            let restarted = dir.open();
            assert_eq!(restarted.retry_wait_ms(now), delay);
            assert_eq!(restarted.unlock(s(PW), early), Err(VaultFfiError::RateLimited { wait_ms: 1 }));
        }
        now += i64::try_from(delay).unwrap();
        assert_eq!(v.retry_wait_ms(now), 0);
    }
    // A clock set back does not shorten the wait.
    assert_eq!(v.unlock(s("wrong password"), now), Err(VaultFfiError::WrongPassword));
    assert_eq!(v.retry_wait_ms(now - 5 * MINUTE), 60_000);

    // The side file holds the count and nothing of the passwords tried.
    let state = std::fs::read_to_string(dir.state_file()).unwrap();
    assert_eq!(json(&dir.state_file())["rate"]["failures"], 9);
    assert!(!state.contains("wrong") && !state.contains("horse"));

    // Success resets it, also on disk.
    now += 60_000;
    v.unlock(s(PW), now).unwrap();
    assert_eq!(v.retry_wait_ms(now), 0);
    assert_eq!(json(&dir.state_file())["rate"]["failures"], 0);
    v.lock();
    assert_eq!(v.unlock(s("wrong password"), now), Err(VaultFfiError::WrongPassword));
    assert_eq!(dir.open().retry_wait_ms(now), 1_000);
}

#[test]
fn change_password_swaps_the_password_and_drops_the_biometric_blob() {
    let dir = TestDir::new("change");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let id = add(&v, "https://bank.example/", "alice", "s3cret-one", T0);
    assert_eq!(v.bio_blob(), None);
    v.set_bio_blob(Some(vec![0, 1, 2, 255])).unwrap();
    assert_eq!(v.bio_blob(), Some(vec![0, 1, 2, 255]));
    assert_eq!(dir.open().bio_blob(), Some(vec![0, 1, 2, 255]), "kept in state.json");

    let before = std::fs::read(dir.file()).unwrap();
    assert_eq!(v.change_password(s("not the password"), s("new password 123")), Err(VaultFfiError::WrongPassword));
    assert_eq!(v.change_password(s(PW), s("short")), Err(VaultFfiError::WeakPassword));
    assert_eq!(std::fs::read(dir.file()).unwrap(), before, "a refused change leaves the file alone");
    assert_eq!(v.bio_blob(), Some(vec![0, 1, 2, 255]), "and the blob");

    v.change_password(s(PW), s("new password 123")).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    assert_eq!(v.bio_blob(), None);
    assert_eq!(dir.open().bio_blob(), None);
    assert!(json(&dir.state_file()).get("bio").is_none());
    v.lock();
    assert_eq!(v.unlock(s(PW), T0), Err(VaultFfiError::WrongPassword));
    v.unlock(s("new password 123"), T0 + 1_000).unwrap();
    assert_eq!(v.reveal(id).unwrap(), "s3cret-one");

    v.set_bio_blob(Some(vec![9])).unwrap();
    v.set_bio_blob(None).unwrap();
    assert_eq!(v.bio_blob(), None);
}

#[test]
fn the_unlock_key_opens_the_vault_until_the_password_changes() {
    let dir = TestDir::new("key");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let id = add(&v, "https://bank.example/", "alice", "s3cret-one", T0);
    let key = v.unlock_key().unwrap();
    assert_eq!(key.len(), 32);
    v.lock();

    let mut wrong = key.clone();
    wrong[31] ^= 0x80;
    assert_eq!(v.unlock_with_key(wrong), Err(VaultFfiError::WrongPassword));
    assert_eq!(v.unlock_with_key(key[..31].to_vec()), Err(VaultFfiError::WrongPassword));
    assert_eq!(v.unlock_with_key(Vec::new()), Err(VaultFfiError::WrongPassword));
    assert_eq!(v.state(), VaultState::Locked);
    assert_eq!(v.retry_wait_ms(T0), 0, "the key path is not rate limited (the key store gates it)");

    v.unlock_with_key(key.clone()).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    assert_eq!(v.reveal(id.clone()).unwrap(), "s3cret-one");
    // A vault opened by key saves like any other.
    add(&v, "https://mail.example/", "bob", "s3cret-two", T0);
    assert_eq!(v.unlock_key().unwrap(), key);

    v.change_password(s(PW), s("new password 123")).unwrap();
    assert_ne!(v.unlock_key().unwrap(), key);
    v.lock();
    assert_eq!(v.unlock_with_key(key), Err(VaultFfiError::WrongPassword));
    v.unlock(s("new password 123"), T0).unwrap();
    assert_eq!(v.summaries().unwrap().len(), 2);
}

// ------------------------------------------------------------------------------ entries

#[test]
fn entries_are_listed_searched_edited_and_deleted() {
    let dir = TestDir::new("entries");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let alice = add(&v, "https://Bank.example/login?next=1", "Alice", "s3cret-one", T0 + 1);
    let bob = add(&v, "https://mail.example:8443/", "bob", "s3cret-two", T0 + 2);
    let carol = v
        .upsert(None, s("https://shop.example/"), s("carol@bank"), Some(s("pw-3")), Some(s("a note")), T0 + 3)
        .unwrap();

    let list = v.summaries().unwrap();
    assert_eq!(
        list,
        [
            VaultEntrySummary {
                id: alice.clone(),
                origin: s("https://bank.example"),
                username: s("Alice"),
                has_note: false,
                created: T0 + 1,
                updated: T0 + 1,
            },
            VaultEntrySummary {
                id: bob.clone(),
                origin: s("https://mail.example:8443"),
                username: s("bob"),
                has_note: false,
                created: T0 + 2,
                updated: T0 + 2,
            },
            VaultEntrySummary {
                id: carol.clone(),
                origin: s("https://shop.example"),
                username: s("carol@bank"),
                has_note: true,
                created: T0 + 3,
                updated: T0 + 3,
            },
        ]
    );
    let found = |q: &str| -> Vec<String> { v.search(s(q)).unwrap().into_iter().map(|e| e.username).collect() };
    assert_eq!(found(""), ["Alice", "bob", "carol@bank"]);
    assert_eq!(found("  "), ["Alice", "bob", "carol@bank"]);
    assert_eq!(found("BANK"), ["Alice", "carol@bank"], "origin or user name, any case");
    assert_eq!(found("alice"), ["Alice"]);
    assert_eq!(found("8443"), ["bob"]);
    assert_eq!(found("s3cret"), Vec::<String>::new(), "passwords are not searched");
    assert_eq!(found("a note"), Vec::<String>::new(), "nor notes");

    // The detail has the note and no password; reveal has the password.
    let detail = v.entry(carol.clone()).unwrap();
    assert_eq!(
        (detail.origin.as_str(), detail.username.as_str(), detail.note.as_str()),
        ("https://shop.example", "carol@bank", "a note")
    );
    assert_eq!(v.reveal(carol.clone()).unwrap(), "pw-3");
    assert_eq!(v.entry(s("no-such-id")), Err(VaultFfiError::NotFound));
    assert_eq!(v.reveal(s("no-such-id")), Err(VaultFfiError::NotFound));

    // The same origin and user name again: the same entry with a new password.
    assert_eq!(add(&v, "https://bank.example/other", "Alice", "changed-one", T0 + 10), alice);
    assert_eq!(v.reveal(alice.clone()).unwrap(), "changed-one");
    assert_eq!(v.summaries().unwrap()[0].updated, T0 + 10);
    assert_eq!(v.summaries().unwrap()[0].created, T0 + 1);

    // Edit: nulls keep the password and the note.
    assert_eq!(
        v.upsert(Some(carol.clone()), s("https://shop.example/"), s("carol"), None, None, T0 + 11).unwrap(),
        carol
    );
    assert_eq!(v.reveal(carol.clone()).unwrap(), "pw-3");
    assert_eq!(v.entry(carol.clone()).unwrap().note, "a note");
    v.upsert(
        Some(carol.clone()),
        s("https://shop.example/"),
        s("carol"),
        Some(s("pw-4")),
        Some(String::new()),
        T0 + 12,
    )
    .unwrap();
    assert_eq!(v.reveal(carol.clone()).unwrap(), "pw-4");
    assert!(!v.summaries().unwrap()[2].has_note);

    // Refused input.
    assert!(matches!(
        v.upsert(None, s("https://new.example/"), s("u"), None, None, T0),
        Err(VaultFfiError::InvalidEntry { .. })
    ));
    assert!(matches!(v.save_login(s("about:blank"), s("u"), s("p"), T0), Err(VaultFfiError::InvalidEntry { .. })));
    assert!(matches!(
        v.save_login(s("https://new.example/"), s("u"), String::new(), T0),
        Err(VaultFfiError::InvalidEntry { .. })
    ));
    assert_eq!(
        v.upsert(Some(s("no-such-id")), s("https://new.example/"), s("u"), None, None, T0),
        Err(VaultFfiError::NotFound)
    );

    v.delete(bob.clone()).unwrap();
    assert_eq!(v.delete(bob), Err(VaultFfiError::NotFound));
    assert_eq!(found(""), ["Alice", "carol"]);

    // All of it is on disk, sealed.
    let text = std::fs::read_to_string(dir.file()).unwrap();
    for plain in ["changed-one", "pw-4", "Alice", "carol", "bank.example"] {
        assert!(!text.contains(plain), "{plain} in the vault file");
    }
    v.lock();
    v.unlock(s(PW), T0).unwrap();
    assert_eq!(found(""), ["Alice", "carol"]);
    assert_eq!(v.reveal(alice).unwrap(), "changed-one");
}

#[test]
fn logins_are_offered_and_filled_only_where_they_belong() {
    let dir = TestDir::new("match");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let alice = add(&v, "https://example.com/", "alice", "pw-alice", T0);
    let bob = add(&v, "https://accounts.example.com/", "bob", "pw-bob", T0);
    let carl = add(&v, "http://plain.example/", "carl", "pw-carl", T0);
    let dora = add(&v, "https://example.com:8443/", "dora", "pw-dora", T0);
    let offered = |url: &str| v.matches_for(s(url)).unwrap();

    // The page's own origin.
    assert_eq!(users(&offered("https://example.com/login?x=1")), [("alice", true)]);
    assert_eq!(offered("https://example.com/")[0].id, alice);
    assert_eq!(offered("https://example.com/")[0].origin, "https://example.com");
    // A subdomain with its own login gets only that one, not the whole domain's.
    assert_eq!(users(&offered("https://accounts.example.com/")), [("bob", true)]);
    // A subdomain without one: the same registrable domain's, https only, marked as not exact.
    assert_eq!(users(&offered("https://shop.example.com/")), [("alice", false), ("bob", false), ("dora", false)]);
    // An https login is never offered to an http page, whatever the host.
    assert_eq!(offered("http://example.com/"), []);
    assert_eq!(offered("http://shop.example.com/"), []);
    // An http login: its exact origin only; not its subdomains, not the https site.
    assert_eq!(users(&offered("http://plain.example/form")), [("carl", true)]);
    assert_eq!(offered("http://sub.plain.example/"), []);
    assert_eq!(offered("https://plain.example/"), []);
    assert_eq!(offered("http://plain.example:8080/"), []);
    // Look-alikes and pages without an origin.
    assert_eq!(offered("https://example.com.evil.example/"), []);
    assert_eq!(offered("https://notexample.com/"), []);
    assert_eq!(offered("about:blank"), []);
    assert_eq!(offered("not a url"), []);

    // The fill checks the page again, by the entry's own origin.
    let fill = |id: &String, page: &str| v.secret_for_fill(id.clone(), s(page));
    assert_eq!(
        fill(&alice, "https://example.com/login"),
        Ok(VaultSecret { username: s("alice"), password: s("pw-alice") })
    );
    assert_eq!(fill(&alice, "https://shop.example.com/").unwrap().password, "pw-alice");
    assert_eq!(fill(&alice, "http://example.com/"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&alice, "https://example.com.evil.example/"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&alice, "https://other.example/"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&alice, "about:blank"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&alice, ""), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&carl, "http://plain.example/"), Ok(VaultSecret { username: s("carl"), password: s("pw-carl") }));
    assert_eq!(fill(&carl, "https://plain.example/"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&carl, "http://sub.plain.example/"), Err(VaultFfiError::NotAllowed));
    assert_eq!(fill(&bob, "https://accounts.example.com/").unwrap().username, "bob");
    assert_eq!(fill(&s("no-such-id"), "https://example.com/"), Err(VaultFfiError::NotFound));

    // HTTP sign-in: the exact origin only, never the registrable domain.
    let auth = |url: &str| v.for_http_auth(s(url)).unwrap();
    assert_eq!(users(&auth("https://example.com/protected")), [("alice", true)]);
    assert_eq!(users(&auth("https://example.com:8443/")), [("dora", true)]);
    assert_eq!(auth("https://example.com:8443/")[0].id, dora);
    assert_eq!(auth("https://shop.example.com/"), []);
    assert_eq!(auth("http://example.com/"), []);
    assert_eq!(users(&auth("http://plain.example/")), [("carl", true)]);
    assert_eq!(auth("nonsense"), []);
}

#[test]
fn save_decision_is_new_update_or_unchanged() {
    let dir = TestDir::new("decision");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let alice = add(&v, "https://example.com/", "alice", "pw-alice", T0);
    let decide = |url: &str, user: &str, pw: &str| v.save_decision(s(url), s(user), s(pw)).unwrap();
    assert_eq!(decide("https://example.com/login", "alice", "pw-alice"), SaveDecision::Unchanged);
    assert_eq!(decide("https://example.com/login", "alice", "pw-new"), SaveDecision::Update { id: alice });
    assert_eq!(decide("https://example.com/login", "bob", "pw-alice"), SaveDecision::New);
    assert_eq!(decide("https://example.com/login", "Alice", "pw-alice"), SaveDecision::New, "user names are exact");
    // Another origin is another login, also within the same domain.
    assert_eq!(decide("https://shop.example.com/", "alice", "pw-alice"), SaveDecision::New);
    assert_eq!(decide("http://example.com/", "alice", "pw-alice"), SaveDecision::New);
    // Asking changes nothing.
    assert_eq!(v.summaries().unwrap().len(), 1);
}

// ------------------------------------------------------------------------------ hints, lock timer

#[test]
fn hints_say_which_sites_have_logins_while_locked() {
    let dir = TestDir::new("hints");
    let (wrapper, switches) = Flaky::boxed("xor-test");
    let v = dir.open_with(wrapper);
    v.create(s(PW), T0).unwrap();
    let alice = add(&v, "https://bank.example/login", "alice", "pw-alice", T0);
    add(&v, "http://plain.example/", "carl", "pw-carl", T0);

    let check = |v: &KrunkVault, bank: bool| {
        assert_eq!(v.has_hint(s("https://bank.example/account")), bank);
        assert_eq!(v.has_hint(s("https://login.bank.example/")), bank, "same registrable domain, https");
        assert!(!v.has_hint(s("http://bank.example/")));
        assert!(v.has_hint(s("http://plain.example/x")));
        assert!(!v.has_hint(s("http://sub.plain.example/")));
        assert!(!v.has_hint(s("https://plain.example/")));
        assert!(!v.has_hint(s("https://other.example/")));
        assert!(!v.has_hint(s("about:blank")));
    };
    check(&v, true);
    v.lock();
    assert_eq!(v.state(), VaultState::Locked);
    check(&v, true);
    // After a restart, still locked.
    let restarted = dir.open_with(Flaky::sharing("xor-test", &switches));
    assert_eq!(restarted.state(), VaultState::Locked);
    check(&restarted, true);

    // The side file names no site, no user and no key in the clear.
    let state = std::fs::read_to_string(dir.state_file()).unwrap();
    for plain in ["bank", "plain.example", "alice", "carl"] {
        assert!(!state.contains(plain), "{plain} in state.json");
    }
    let parsed = json(&dir.state_file());
    assert_eq!(parsed["hintTags"].as_array().unwrap().len(), 3, "two origins and one https domain");
    assert!(parsed["hintKey"].is_string());

    // Every change rewrites the tags.
    v.unlock(s(PW), T0).unwrap();
    v.delete(alice).unwrap();
    v.lock();
    check(&v, false);
    v.unlock(s(PW), T0).unwrap();
    v.upsert(None, s("https://bank.example/"), s("alice"), Some(s("pw")), None, T0).unwrap();
    v.lock();
    check(&v, true);

    // The device key fails: no hints, and no error.
    switches.fail_unprotect.store(true, Ordering::SeqCst);
    let keyless = dir.open_with(Flaky::sharing("xor-test", &switches));
    assert!(!keyless.has_hint(s("https://bank.example/account")));
    assert!(!keyless.has_hint(s("http://plain.example/x")));
    switches.fail_unprotect.store(false, Ordering::SeqCst);

    // No side file (deleted, or damaged): no hints until the next unlock writes them again.
    std::fs::write(dir.state_file(), b"{ not json").unwrap();
    let fresh = dir.open_with(Flaky::sharing("xor-test", &switches));
    assert!(!fresh.has_hint(s("https://bank.example/account")));
    fresh.unlock(s(PW), T0).unwrap();
    fresh.lock();
    check(&fresh, true);
}

#[test]
fn the_inactivity_lock_counts_from_the_last_use() {
    let dir = TestDir::new("autolock");
    let v = dir.open();
    assert!(!v.autolock_due(T0 + 99 * MINUTE, 15), "no vault: nothing to lock");
    v.create(s(PW), T0).unwrap();
    assert!(!v.autolock_due(T0 + 15 * MINUTE - 1, 15));
    assert!(v.autolock_due(T0 + 15 * MINUTE, 15));
    assert!(v.autolock_due(T0 + MINUTE, 0), "0 counts as one minute");
    assert_eq!(v.state(), VaultState::Unlocked, "the caller locks");

    v.touch(T0 + 10 * MINUTE);
    assert!(!v.autolock_due(T0 + 24 * MINUTE, 15));
    assert!(v.autolock_due(T0 + 25 * MINUTE, 15));
    // A call that carries a time is a use at that time.
    add(&v, "https://bank.example/", "alice", "pw", T0 + 20 * MINUTE);
    assert!(!v.autolock_due(T0 + 34 * MINUTE, 15));
    assert!(v.autolock_due(T0 + 35 * MINUTE, 15));
    // One without a time (a listing) is a use by the time of the next check.
    v.summaries().unwrap();
    assert!(!v.autolock_due(T0 + 36 * MINUTE, 15));
    assert!(!v.autolock_due(T0 + 50 * MINUTE, 15));
    assert!(v.autolock_due(T0 + 51 * MINUTE, 15));

    v.lock();
    assert!(!v.autolock_due(T0 + 99 * MINUTE, 15), "locked already");
    v.unlock(s(PW), T0 + 100 * MINUTE).unwrap();
    assert!(!v.autolock_due(T0 + 114 * MINUTE, 15));
    assert!(v.autolock_due(T0 + 115 * MINUTE, 15));
}

// ------------------------------------------------------------------------------ the device key

#[test]
fn a_lost_device_key_is_key_lost_and_the_file_stays_as_it_is() {
    let dir = TestDir::new("keylost");
    let (wrapper, switches) = Flaky::boxed("xor-test");
    let v = dir.open_with(wrapper);
    v.create(s(PW), T0).unwrap();
    let id = add(&v, "https://bank.example/", "alice", "s3cret-one", T0);
    let key = v.unlock_key().unwrap();
    v.lock();
    let before = std::fs::read(dir.file()).unwrap();

    switches.fail_unprotect.store(true, Ordering::SeqCst);
    let lost = VaultFfiError::KeyLost { detail: s("test key store: key gone") };
    assert_eq!(v.unlock(s(PW), T0), Err(lost.clone()));
    assert_eq!(v.unlock_with_key(key), Err(lost));
    assert_eq!(v.state(), VaultState::Locked);
    assert_eq!(std::fs::read(dir.file()).unwrap(), before, "byte for byte");
    assert_eq!(v.retry_wait_ms(T0), 0, "not a wrong password");
    // The password is still checked first, so a typo is told apart from a lost key.
    assert_eq!(v.unlock(s("wrong password"), T0), Err(VaultFfiError::WrongPassword));
    assert_eq!(std::fs::read(dir.file()).unwrap(), before);

    // A vault file written under another wrapper (a file from Windows, say) is the same case.
    let other = dir.open();
    assert!(matches!(other.unlock(s(PW), T0 + 5_000), Err(VaultFfiError::KeyLost { .. })));
    assert_eq!(std::fs::read(dir.file()).unwrap(), before);

    // The key store works again: so does the vault.
    switches.fail_unprotect.store(false, Ordering::SeqCst);
    v.unlock(s(PW), T0 + 5_000).unwrap();
    assert_eq!(v.reveal(id).unwrap(), "s3cret-one");

    // A key store that fails while saving: the change is refused, the file and the open vault keep
    // what they had.
    switches.fail_protect.store(true, Ordering::SeqCst);
    let before = std::fs::read(dir.file()).unwrap();
    assert!(matches!(v.save_login(s("https://new.example/"), s("u"), s("p"), T0), Err(VaultFfiError::KeyLost { .. })));
    assert_eq!(std::fs::read(dir.file()).unwrap(), before);
}

#[test]
fn a_key_store_that_cannot_protect_leaves_no_vault_behind() {
    let dir = TestDir::new("create-fails");
    let (wrapper, switches) = Flaky::boxed("xor-test");
    switches.fail_protect.store(true, Ordering::SeqCst);
    let v = dir.open_with(wrapper);
    assert_eq!(v.create(s(PW), T0), Err(VaultFfiError::KeyLost { detail: s("test key store: cannot protect") }));
    assert_eq!(v.state(), VaultState::None);
    assert!(!dir.file().exists());
    let left: Vec<_> =
        std::fs::read_dir(dir.dir()).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(left.is_empty(), "left behind: {left:?}");
    assert_eq!(v.summaries(), Err(VaultFfiError::NoVault));

    switches.fail_protect.store(false, Ordering::SeqCst);
    v.create(s(PW), T0).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    assert_eq!(json(&dir.file())["wrap"], "xor-test");
}

#[test]
fn destroy_removes_the_vault_and_its_side_file() {
    let dir = TestDir::new("destroy");
    let v = dir.open();
    v.destroy().unwrap();
    assert_eq!(v.state(), VaultState::None);

    v.create(s(PW), T0).unwrap();
    add(&v, "https://bank.example/", "alice", "s3cret-one", T0);
    v.set_bio_blob(Some(vec![1, 2, 3])).unwrap();
    v.lock();
    assert_eq!(v.unlock(s("wrong password"), T0), Err(VaultFfiError::WrongPassword));
    assert!(dir.file().exists() && dir.state_file().exists());

    // Locked (the key-lost screen never has the vault open) or unlocked: gone either way.
    v.destroy().unwrap();
    assert_eq!(v.state(), VaultState::None);
    assert!(!dir.file().exists() && !dir.state_file().exists());
    assert_eq!(v.bio_blob(), None);
    assert_eq!(v.retry_wait_ms(T0), 0);
    assert!(!v.has_hint(s("https://bank.example/")));
    assert_eq!(v.summaries(), Err(VaultFfiError::NoVault));

    // A new vault starts empty.
    v.create(s("another password"), T0).unwrap();
    assert_eq!(v.summaries().unwrap(), []);
    v.destroy().unwrap();
    assert_eq!(v.state(), VaultState::None);
    assert_eq!(v.summaries(), Err(VaultFfiError::NoVault));
}

/// A wrapper that presses "Lock now" while an unlock is at work (the wrapper is called in the
/// middle of it, outside the object's lock).
struct LocksMeanwhile(Mutex<Option<Arc<KrunkVault>>>);

impl KeyWrapper for LocksMeanwhile {
    fn name(&self) -> &'static str {
        "none"
    }
    fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        Ok(data.to_vec())
    }
    fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        if let Some(v) = self.0.lock().unwrap().as_ref() {
            assert_eq!(v.state(), VaultState::Locked, "state() answers during an unlock");
            v.lock();
        }
        Ok(Zeroizing::new(data.to_vec()))
    }
}

#[test]
fn a_lock_during_an_unlock_wins() {
    let dir = TestDir::new("lock-race");
    let first = dir.open();
    first.create(s(PW), T0).unwrap();
    let key = first.unlock_key().unwrap();
    drop(first);

    let wrapper = Arc::new(LocksMeanwhile(Mutex::new(None)));
    struct Shared(Arc<LocksMeanwhile>);
    impl KeyWrapper for Shared {
        fn name(&self) -> &'static str {
            self.0.name()
        }
        fn protect(&self, data: &[u8]) -> Result<Vec<u8>, String> {
            self.0.protect(data)
        }
        fn unprotect(&self, data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            self.0.unprotect(data)
        }
    }
    let v = dir.open_with(Box::new(Shared(wrapper.clone())));
    v.unlock(s(PW), T0).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
    v.lock();

    *wrapper.0.lock().unwrap() = Some(v.clone());
    assert_eq!(v.unlock(s(PW), T0), Err(VaultFfiError::Locked));
    assert_eq!(v.state(), VaultState::Locked);
    assert_eq!(v.unlock_with_key(key), Err(VaultFfiError::Locked));
    assert_eq!(v.state(), VaultState::Locked);
    assert_eq!(v.retry_wait_ms(T0), 0, "the password was right");
    // (The wrapper and the vault hold each other: undone so both are freed.)
    *wrapper.0.lock().unwrap() = None;
    v.unlock(s(PW), T0).unwrap();
    assert_eq!(v.state(), VaultState::Unlocked);
}

// ------------------------------------------------------------------------------ the exported path

/// What Kotlin's Keystore wrapper is to the library: an implementation of the exported trait.
#[derive(Default)]
struct ForeignXor {
    missing: AtomicBool,
    throws: AtomicBool,
}

impl VaultKeyWrapper for ForeignXor {
    fn protect(&self, data: Vec<u8>) -> Result<Vec<u8>, WrapError> {
        Ok(xor(&data))
    }
    fn unprotect(&self, data: Vec<u8>) -> Result<Vec<u8>, WrapError> {
        if self.throws.load(Ordering::SeqCst) {
            // What UniFFI makes of an exception the Kotlin side did not declare.
            return Err(uniffi::UnexpectedUniFFICallbackError::new("java.lang.IllegalStateException").into());
        }
        if self.missing.load(Ordering::SeqCst) {
            return Err(WrapError::KeyMissing { detail: s("alias krunk.vault.wrap.v1") });
        }
        Ok(xor(&data))
    }
}

#[test]
fn the_exported_constructor_wraps_with_the_foreign_key_wrapper() {
    let dir = TestDir::new("foreign");
    let wrapper = Arc::new(ForeignXor::default());
    let v = KrunkVault::new(dir.dir().to_string_lossy().into_owned(), wrapper.clone());
    assert_eq!(v.state(), VaultState::None);
    v.create(s(PW), T0).unwrap();
    let id = add(&v, "https://bank.example/", "alice", "s3cret-one", T0);

    // The real cost and the Android wrapper name are in the file.
    let file = json(&dir.file());
    assert_eq!(file["wrap"], "android-keystore");
    assert_eq!(file["format"], 1);
    assert_eq!(
        (file["kdf"]["m_kib"].as_u64(), file["kdf"]["t"].as_u64(), file["kdf"]["p"].as_u64()),
        (Some(65_536), Some(3), Some(1))
    );
    // What the wrapper returned is what is stored: the core's own wrapper reads it back.
    v.lock();
    let core_view =
        Vault::open(&dir.file(), PW, &Flaky { name: "android-keystore", switches: Arc::default() }).unwrap();
    assert_eq!(core_view.entries()[0].password.as_str(), "s3cret-one");
    assert!(matches!(Vault::open(&dir.file(), PW, &NoWrap), Err(VaultError::Wrap(_))), "not without the device key");
    drop(core_view);

    assert!(v.has_hint(s("https://bank.example/")));
    wrapper.missing.store(true, Ordering::SeqCst);
    assert_eq!(
        v.unlock(s(PW), T0),
        Err(VaultFfiError::KeyLost { detail: s("device key missing: alias krunk.vault.wrap.v1") })
    );
    wrapper.throws.store(true, Ordering::SeqCst);
    assert_eq!(
        v.unlock(s(PW), T0),
        Err(VaultFfiError::KeyLost { detail: s("device key failed: java.lang.IllegalStateException") })
    );
    wrapper.throws.store(false, Ordering::SeqCst);
    wrapper.missing.store(false, Ordering::SeqCst);
    v.unlock(s(PW), T0).unwrap();
    assert_eq!(v.reveal(id).unwrap(), "s3cret-one");
}

#[test]
fn nothing_secret_is_printed_or_kept_in_the_side_file() {
    let dir = TestDir::new("secrets");
    let v = dir.open();
    v.create(s(PW), T0).unwrap();
    let id =
        v.upsert(None, s("https://bank.example/"), s("alice"), Some(s("s3cret-one")), Some(s("pin 4711")), T0).unwrap();
    v.set_bio_blob(Some(b"opaque".to_vec())).unwrap();
    let secret = v.secret_for_fill(id.clone(), s("https://bank.example/")).unwrap();
    let printed = format!("{secret:?} {:?} {v:?}", v.entry(id).unwrap());
    assert!(printed.contains("alice") && printed.contains("<redacted>"));
    for plain in ["s3cret-one", "pin 4711", PW] {
        assert!(!printed.contains(plain), "{plain} printed");
    }
    assert!(!format!("{}", VaultFfiError::RateLimited { wait_ms: 1_500 }).is_empty());
    let state = std::fs::read_to_string(dir.state_file()).unwrap();
    for plain in ["s3cret-one", "pin 4711", PW, "alice", "bank.example"] {
        assert!(!state.contains(plain), "{plain} in state.json");
    }
    let keys: BTreeSet<String> = json(&dir.state_file()).as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["bio", "hintKey", "hintTags", "rate"].map(String::from).into());
}

// ------------------------------------------------------------------------------ file formats

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/vault")
}

/// The recovery phrase of the frozen fixtures.
fn fixture_phrase() -> Phrase {
    Phrase::from_entropy([7; 32])
}

/// Writes a fixture, but never over one that is there (the committed ones are frozen, and the
/// platform ones get replaced by files made on the platforms) unless `KRUNK_REGEN_FIXTURES=1`.
fn write_fixture(from: &Path, to: &Path) {
    assert!(
        !to.exists() || std::env::var("KRUNK_REGEN_FIXTURES").as_deref() == Ok("1"),
        "{} exists; set KRUNK_REGEN_FIXTURES=1 to replace it",
        to.display()
    );
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap();
}

/// One-off: `vault-format4.json` = the frozen format-3 fixture plus a sync part.
///   cargo test -p krunk_ffi -- --ignored generate_the_format_4_fixture
#[test]
#[ignore = "writes a committed fixture"]
fn generate_the_format_4_fixture() {
    let dir = TestDir::new("gen-f4");
    dir.install(&fixtures().join("vault-format3.json"));
    let mut v = Vault::open(&dir.file(), FIXTURE_PW, &NoWrap).unwrap();
    let part = VaultSync {
        state: Zeroizing::new(b"fixture sync state".to_vec()),
        dirty: vec![s("0123456789abcdef0123456789abcdef")],
    };
    v.set_sync_part(Some(part), &NoWrap).unwrap();
    drop(v);
    assert_eq!(json(&dir.file())["format"], 4);
    write_fixture(&dir.file(), &fixtures().join("vault-format4.json"));
}

#[test]
fn vaults_of_formats_2_3_and_4_keep_everything_this_app_does_not_show() {
    for format in [2u64, 3, 4] {
        let dir = TestDir::new(&format!("format{format}"));
        let source = fixtures().join(format!("vault-format{format}.json"));
        dir.install(&source);
        assert_eq!(json(&dir.file())["format"], format);

        // What the core sees before: the reference.
        let (card, recovery, sync, alice) = {
            let v = Vault::open(&dir.file(), FIXTURE_PW, &NoWrap).unwrap();
            assert_eq!(v.cards().len(), 1, "format {format}");
            assert_eq!(v.recovery_created().is_some(), format >= 3);
            assert_eq!(v.sync_part().is_some(), format >= 4);
            let alice = v.entries().iter().find(|e| e.username == "alice").unwrap().id.clone();
            (v.cards()[0].clone(), v.recovery_created(), v.sync_part().cloned(), alice)
        };

        // Through the Android object: add a login, edit one, lock.
        let v = dir.open();
        assert_eq!(v.state(), VaultState::Locked);
        v.unlock(s(FIXTURE_PW), T0).unwrap();
        assert_eq!(v.reveal(alice.clone()).unwrap(), "s3cret-one");
        let carol = add(&v, "https://new.example/", "carol", "s3cret-three", T0 + 1);
        let edited = v
            .upsert(
                Some(alice.clone()),
                s("https://bank.example/"),
                s("alice"),
                Some(s("changed-one")),
                Some(s("edited on Android")),
                T0 + 2,
            )
            .unwrap();
        assert_eq!(edited, alice);
        v.lock();
        assert_eq!(json(&dir.file())["format"], format, "the format number stays");
        assert_eq!(json(&dir.file())["wrap"], "none");

        // What the core sees after.
        let after = Vault::open(&dir.file(), FIXTURE_PW, &NoWrap).unwrap();
        let logins: BTreeSet<(&str, &str)> =
            after.entries().iter().map(|e| (e.username.as_str(), e.password.as_str())).collect();
        assert_eq!(logins, [("alice", "changed-one"), ("bob", "s3cret-two"), ("carol", "s3cret-three")].into());
        assert_eq!(after.entry(&alice).unwrap().note.as_str(), "edited on Android");
        let kept = &after.cards()[0];
        assert_eq!(after.cards().len(), 1);
        assert_eq!(
            (
                &kept.id,
                kept.number.as_str(),
                kept.exp_month,
                kept.exp_year,
                &kept.name,
                &kept.nickname,
                kept.address.as_str(),
                kept.created,
                kept.updated
            ),
            (
                &card.id,
                card.number.as_str(),
                card.exp_month,
                card.exp_year,
                &card.name,
                &card.nickname,
                card.address.as_str(),
                card.created,
                card.updated
            ),
            "format {format}: the card"
        );
        assert_eq!(after.recovery_created(), recovery, "format {format}: the recovery slot");
        match (after.sync_part(), &sync) {
            (None, None) => {}
            (Some(now), Some(before)) => {
                assert_eq!(now.state, before.state, "the sync state");
                // The two entries changed here are queued for sync's next push, after what was
                // queued already: that is the core's rule for a vault that syncs.
                let mut queue = before.dirty.clone();
                queue.extend([carol.clone(), alice.clone()]);
                assert_eq!(now.dirty, queue);
            }
            _ => panic!("format {format}: the sync part came or went"),
        }
        drop(after);

        if format >= 3 {
            // The phrase still recovers the vault (on a copy: recovery rewrites the file).
            let copy = TestDir::new(&format!("format{format}-recover"));
            copy.install(&dir.file());
            let wrong = Phrase::from_entropy([8; 32]);
            assert_eq!(
                Vault::recover(&copy.file(), &wrong, "recovered password", &NoWrap).unwrap_err(),
                VaultError::WrongPhrase
            );
            let recovered = Vault::recover(&copy.file(), &fixture_phrase(), "recovered password", &NoWrap).unwrap();
            assert_eq!(recovered.entries().len(), 3);
            assert_eq!(recovered.cards().len(), 1);
            assert_eq!(recovered.recovery_created(), recovery);
            assert_eq!(recovered.sync_part().map(|p| p.state.clone()), sync.as_ref().map(|p| p.state.clone()));
            drop(recovered);
            assert_eq!(json(&copy.file())["format"], format);
            let reopened = copy.open();
            reopened.unlock(s("recovered password"), T0).unwrap();
            assert_eq!(reopened.reveal(alice.clone()).unwrap(), "changed-one");
        }
    }
}

// ------------------------------------------------------------------------------ platform files

const PLATFORM_LOGINS: [(&str, &str, &str); 2] =
    [("https://bank.example", "alice", "s3cret-one"), ("https://mail.example:8443", "bob", "s3cret-two")];

fn platform(name: &str) -> PathBuf {
    fixtures().join("platform").join(name)
}

/// One-off: the STAND-INS for the platform files, made by the core on Linux (see the README in
/// the fixture folder). The real ones come from a Windows VM and from the Android emulator.
///   cargo test -p krunk_ffi -- --ignored generate_the_platform_stand_ins
#[test]
#[ignore = "writes committed fixtures"]
fn generate_the_platform_stand_ins() {
    for (name, order, base) in [
        ("vault-windows-inner.json", [0, 1], 1_794_000_000_000i64),
        ("vault-android-inner.json", [1, 0], 1_794_100_000_000),
    ] {
        let dir = TestDir::new(&format!("gen-{name}"));
        std::fs::create_dir_all(dir.dir()).unwrap();
        // The cost both apps use for a new vault.
        let mut v = Vault::create(&dir.file(), FIXTURE_PW, KdfParams::default(), &NoWrap).unwrap();
        for (n, i) in order.into_iter().enumerate() {
            let (origin, user, password) = PLATFORM_LOGINS[i];
            v.save_login(origin, user, Zeroizing::new(s(password)), base + 1_000 * i64::try_from(n).unwrap(), &NoWrap)
                .unwrap();
        }
        drop(v);
        write_fixture(&dir.file(), &platform(name));
    }
}

/// The logins of the vault file `name`, opened through the Android object without a device
/// wrap. Ids, times and the order of the entries are the file's own business.
fn platform_logins(name: &str) -> BTreeSet<(String, String, String)> {
    let dir = TestDir::new(&format!("platform-{name}"));
    dir.install(&platform(name));
    let v = dir.open();
    v.unlock(s(FIXTURE_PW), T0).unwrap();
    let logins = v
        .summaries()
        .unwrap()
        .into_iter()
        .map(|e| {
            let password = v.reveal(e.id).unwrap();
            (e.origin, e.username, password)
        })
        .collect();
    v.lock();
    assert_eq!(std::fs::read(dir.file()).unwrap(), std::fs::read(platform(name)).unwrap(), "reading changes nothing");
    logins
}

#[test]
fn vaults_written_on_windows_and_on_android_hold_the_same_logins() {
    let expected: BTreeSet<(String, String, String)> =
        PLATFORM_LOGINS.iter().map(|(o, u, p)| (s(o), s(u), s(p))).collect();
    assert_eq!(platform_logins("vault-windows-inner.json"), expected, "Windows");
    assert_eq!(platform_logins("vault-android-inner.json"), expected, "Android");
}

#[test]
fn platform_vaults_as_written_do_not_open_off_their_machine() {
    for (written, inner) in [
        ("vault-windows-dpapi.json", "vault-windows-inner.json"),
        ("vault-android-keystore.json", "vault-android-inner.json"),
    ] {
        if !platform(written).exists() {
            // Not made yet (see the README): nothing to check for this platform.
            eprintln!("skipped: {written} is not in the fixture folder");
            continue;
        }
        let (as_written, twin) = (json(&platform(written)), json(&platform(inner)));
        let wrap = as_written["wrap"].as_str().unwrap().to_string();
        assert_ne!(wrap, "none", "{written}");
        assert_eq!(twin["wrap"], "none", "{inner}");
        // Only the platform wrap differs: everything sealed under the password and the vault key
        // is the same text in both files.
        for part in ["format", "kdf", "check", "manifest", "entries"] {
            assert_eq!(as_written[part], twin[part], "{written}: {part}");
        }
        assert_ne!(as_written["vaultKey"], twin["vaultKey"], "{written}");

        let dir = TestDir::new(&format!("platform-{written}"));
        dir.install(&platform(written));
        // This machine's wrapper of that name cannot unwrap another machine's blob.
        let (wrapper, switches) = Flaky::boxed(Box::leak(wrap.into_boxed_str()));
        switches.fail_unprotect.store(true, Ordering::SeqCst);
        let v = dir.open_with(wrapper);
        assert_eq!(v.state(), VaultState::Locked);
        assert!(matches!(v.unlock(s(FIXTURE_PW), T0), Err(VaultFfiError::KeyLost { .. })), "{written}");
        assert_eq!(v.state(), VaultState::Locked);
        // Nor does a build with another wrapper (or none).
        assert!(matches!(dir.open().unlock(s(FIXTURE_PW), T0), Err(VaultFfiError::KeyLost { .. })), "{written}");
        assert_eq!(
            std::fs::read(dir.file()).unwrap(),
            std::fs::read(platform(written)).unwrap(),
            "{written}: untouched"
        );
    }
}
