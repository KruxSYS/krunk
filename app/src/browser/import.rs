//! Import from other browsers (the host side of `lb_core::import`): the profiles on this
//! Windows user, copies of their files in a temporary folder (the browsers keep the
//! originals locked), parsing and decryption on a worker thread, merging on the UI thread.
//! Passwords need the Krunk vault set up and unlocked: a locked vault opens the unlock
//! prompt and the import continues afterwards. Nothing secret is logged.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use lb_core::import::profiles::{self, Family, Profile};
use lb_core::import::{chromium, csv, firefox, ImportError, Login, Unprotect, Visit};
use lb_core::netscape::ImportNode;
use lb_core::protocol::{Event, Request, UnlockPurpose};
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use zeroize::Zeroizing;

use super::Browser;
use crate::ffi::dialogs;
use crate::ffi::secure::{self, Dpapi};
use crate::log::{info, warning};
use crate::paths;

/// A CSV export is small; anything bigger is not one.
const MAX_CSV_BYTES: u64 = 20 * 1024 * 1024;

/// An import request that waits for the vault to be unlocked.
struct PendingImport {
    tab: Option<TabId>,
    req_id: Option<u32>,
    request: Request,
}

#[derive(Default)]
pub(super) struct ImportHost {
    pending: RefCell<Option<PendingImport>>,
    jobs: std::cell::Cell<u32>,
}

/// Chrome's own DPAPI blobs (no extra entropy).
struct PlainDpapi;

impl Unprotect for PlainDpapi {
    fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
        secure::unprotect_plain(blob)
    }
}

/// What the worker thread read from the copies.
struct Parsed {
    history: Option<Vec<Visit>>,
    bookmarks: Option<Vec<ImportNode>>,
    logins: Option<ParsedLogins>,
}

/// Logins read, rows protected by Chrome (app-bound), and the rows that could not be used by
/// reason (0.7.0, R25).
struct ParsedLogins {
    logins: Vec<Login>,
    app_bound: usize,
    skipped: Vec<(String, usize)>,
    /// What the source holds (0.7.2, R12): for the log line, and the reason a 0 is shown with
    /// when nothing was there.
    source: Option<String>,
    empty_because: Option<&'static str>,
    /// Ciphers and key sizes of the fields that could not be decrypted (0.7.4), shown too.
    detail: Option<String>,
}

fn family_id(f: Family) -> &'static str {
    match f {
        Family::Firefox => "firefox",
        Family::Chrome => "chrome",
        Family::Edge => "edge",
        Family::Brave => "brave",
    }
}

/// Copies `name` (and a SQLite write-ahead log or journal next to it) from `from` to `to`.
fn copy_file(from: &Path, to: &Path, name: &str) -> Result<PathBuf, String> {
    let src = from.join(name);
    let dst = to.join(name);
    std::fs::copy(&src, &dst).map_err(|e| format!("cannot read {name}: {e} (close the browser and try again)"))?;
    for suffix in ["-wal", "-journal"] {
        let extra = from.join(format!("{name}{suffix}"));
        if extra.is_file() {
            let _ = std::fs::copy(&extra, to.join(format!("{name}{suffix}")));
        }
    }
    Ok(dst)
}

fn parse_profile(profile: &Profile, types: &[String], primary_password: &str, temp: &Path) -> Result<Parsed, String> {
    let wants = |t: &str| types.iter().any(|x| x == t);
    std::fs::create_dir_all(temp).map_err(|e| format!("cannot create a temporary folder: {e}"))?;
    let mut parsed = Parsed { history: None, bookmarks: None, logins: None };
    match profile.family {
        Family::Firefox => {
            if wants("history") || wants("bookmarks") {
                let places = copy_file(&profile.path, temp, "places.sqlite")?;
                if wants("history") {
                    parsed.history = Some(firefox::read_history(&places).map_err(|e| e.to_string())?);
                }
                if wants("bookmarks") {
                    parsed.bookmarks = Some(firefox::read_bookmarks(&places).map_err(|e| e.to_string())?);
                }
            }
            if wants("passwords") {
                copy_file(&profile.path, temp, "key4.db")?;
                copy_file(&profile.path, temp, "logins.json")?;
                let got = firefox::read_logins(temp, primary_password).map_err(|e| match e {
                    ImportError::WrongPassword => "wrong Firefox primary password".to_string(),
                    other => other.to_string(),
                })?;
                let skipped = got.reasons.iter().map(|(r, n)| (r.to_string(), *n)).collect();
                let f = got.found;
                let source = format!(
                    "Firefox profile {}: {} entries in logins.json, {} key(s) in key4.db{}, primary password {}",
                    profile.path.display(),
                    f.entries,
                    f.keys,
                    if f.unusable_keys > 0 {
                        format!(" ({} could not be unlocked)", f.unusable_keys)
                    } else {
                        String::new()
                    },
                    if f.primary_password { "set" } else { "not set" }
                );
                let detail = Some(got.failure_detail().to_string()).filter(|d| !d.is_empty());
                let source = match &detail {
                    Some(d) => format!("{source}; not decrypted: {d}"),
                    None => source,
                };
                let empty_because = (f.entries == 0).then_some("Firefox has no saved logins in this profile");
                parsed.logins = Some(ParsedLogins {
                    logins: got.logins,
                    app_bound: 0,
                    skipped,
                    source: Some(source),
                    empty_because,
                    detail,
                });
            }
        }
        _ => {
            if wants("history") {
                let history = copy_file(&profile.path, temp, "History")?;
                parsed.history = Some(chromium::read_history(&history).map_err(|e| e.to_string())?);
            }
            if wants("bookmarks") {
                let json = std::fs::read_to_string(profile.path.join("Bookmarks"))
                    .map_err(|e| format!("cannot read Bookmarks: {e}"))?;
                parsed.bookmarks = Some(chromium::read_bookmarks(&json).map_err(|e| e.to_string())?);
            }
            if wants("passwords") {
                let user_data = profile.user_data.clone().ok_or("no User Data folder")?;
                let state = std::fs::read_to_string(user_data.join("Local State"))
                    .map_err(|e| format!("cannot read Local State: {e}"))?;
                let keys = chromium::local_state_keys(&state).map_err(|e| e.to_string())?;
                let login_data = copy_file(&profile.path, temp, "Login Data")?;
                let got = chromium::read_logins(&login_data, &keys, &PlainDpapi).map_err(|e| e.to_string())?;
                let skipped =
                    if got.skipped > 0 { vec![("unreadable rows".to_string(), got.skipped)] } else { Vec::new() };
                parsed.logins = Some(ParsedLogins {
                    logins: got.logins,
                    app_bound: got.app_bound,
                    skipped,
                    source: None,
                    empty_because: None,
                    detail: None,
                });
            }
        }
    }
    Ok(parsed)
}

impl Browser {
    fn import_profiles(&self) -> Vec<Profile> {
        let roaming = paths::roaming_app_data().unwrap_or_default();
        let local = paths::local_app_data().unwrap_or_default();
        profiles::detect(&roaming, &local)
    }

    /// Settings → the profiles found on this computer.
    pub(super) fn import_detect(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let list: Vec<serde_json::Value> = self
            .import_profiles()
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "browser": family_id(p.family),
                    "label": format!("{} – {}", p.family.name(), p.name),
                    "path": p.path.display().to_string(),
                    "isDefault": p.is_default,
                    "hasHistory": p.has_history,
                    "hasBookmarks": p.has_bookmarks,
                    "hasPasswords": p.has_passwords,
                })
            })
            .collect();
        self.reply(tab, Event::ok(req_id, list));
    }

    /// Passwords need the vault: set up and unlocked. A locked vault opens the unlock prompt
    /// and keeps the request; `true` means the import may go on now.
    fn import_vault_ready(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) -> bool {
        if !self.vault_exists() {
            self.reply(tab, Event::err(req_id, "Set up Krunk passwords on krunk://passwords first, then import."));
            return false;
        }
        if self.vault_unlocked() {
            return true;
        }
        if self.imports.pending.borrow().is_some() {
            self.reply(tab, Event::err(req_id, "Another import is waiting for the vault to be unlocked."));
            return false;
        }
        *self.imports.pending.borrow_mut() = Some(PendingImport { tab, req_id, request });
        *self.vaults.unlock_for.borrow_mut() =
            Some((tab.unwrap_or(0), "krunk://settings".to_string(), UnlockPurpose::Import));
        self.show_unlock_prompt(None);
        false
    }

    /// The vault was unlocked for an import: run it now.
    pub(super) fn after_unlock_import(&self) {
        // Take the request before the prompt goes: hiding it counts as a dismissal otherwise.
        let pending = self.imports.pending.borrow_mut().take();
        self.hide_popup();
        let Some(PendingImport { tab, req_id, request }) = pending else { return };
        match request {
            Request::ImportRun { profile, types, primary_password } => {
                self.import_run(tab, req_id, profile, types, primary_password)
            }
            Request::ImportCsv => self.import_csv(tab, req_id),
            _ => {}
        }
    }

    /// The unlock prompt for an import was dismissed.
    pub(super) fn import_unlock_dismissed(&self) {
        if let Some(p) = self.imports.pending.borrow_mut().take() {
            self.reply(p.tab, Event::err(p.req_id, "Krunk passwords stay locked; nothing was imported."));
        }
    }

    /// Imports `types` from the profile `profile_id` (an id from `import_detect`).
    pub(super) fn import_run(
        &self,
        tab: Option<TabId>,
        req_id: Option<u32>,
        profile_id: String,
        types: Vec<String>,
        primary_password: Option<Secret>,
    ) {
        let types: Vec<String> =
            types.into_iter().filter(|t| matches!(t.as_str(), "history" | "bookmarks" | "passwords")).collect();
        if types.is_empty() {
            self.reply(tab, Event::err(req_id, "Choose what to import."));
            return;
        }
        if types.iter().any(|t| t == "passwords") {
            let request = Request::ImportRun {
                profile: profile_id.clone(),
                types: types.clone(),
                primary_password: primary_password.clone(),
            };
            if !self.import_vault_ready(tab, req_id, request) {
                return;
            }
        }
        let Some(profile) = self.import_profiles().into_iter().find(|p| p.id == profile_id) else {
            self.reply(tab, Event::err(req_id, "That browser profile is no longer there."));
            return;
        };
        let password = primary_password.map(Secret::into_zeroizing).unwrap_or_default();
        let n = self.imports.jobs.get() + 1;
        self.imports.jobs.set(n);
        let temp = std::env::temp_dir().join(format!("Krunk-import-{}-{n}", std::process::id()));
        info!("import from {} ({}): {}", profile.family.name(), profile.name, types.join(", "));
        self.spawn_job(
            "import",
            move || {
                let r = parse_profile(&profile, &types, &password, &temp);
                let _ = std::fs::remove_dir_all(&temp);
                r
            },
            move |b, r| match r {
                Ok(parsed) => {
                    let result = b.import_merge(parsed);
                    b.reply(tab, Event::ok(req_id, result));
                }
                Err(e) => {
                    warning!("import failed: {e}");
                    b.reply(tab, Event::err(req_id, e));
                }
            },
        );
    }

    /// Passwords from a CSV file chosen in a native dialog.
    pub(super) fn import_csv(&self, tab: Option<TabId>, req_id: Option<u32>) {
        if !self.import_vault_ready(tab, req_id, Request::ImportCsv) {
            return;
        }
        let Some(w) = self.window() else { return };
        let Some(path) = dialogs::open_file(w.hwnd(), "Import passwords from a CSV file", ("CSV files", "*.csv"))
        else {
            self.reply(tab, Event::ok(req_id, ()));
            return;
        };
        let text = std::fs::metadata(&path)
            .map_err(|e| e.to_string())
            .and_then(|m| {
                if m.len() > MAX_CSV_BYTES {
                    Err("The file is too large for a password export.".to_string())
                } else {
                    Ok(())
                }
            })
            .and_then(|()| std::fs::read(&path).map_err(|e| e.to_string()))
            .map(|bytes| Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()));
        let parsed = match text.and_then(|t| csv::parse(&t).map_err(|e| e.to_string())) {
            Ok(p) => p,
            Err(e) => {
                warning!("CSV import failed: {e}");
                self.reply(tab, Event::err(req_id, format!("Import failed: {e}")));
                return;
            }
        };
        info!("import passwords from a CSV file ({} rows)", parsed.logins.len() + parsed.skipped);
        let result = self.import_merge(Parsed {
            history: None,
            bookmarks: None,
            logins: Some(ParsedLogins {
                source: None,
                empty_because: None,
                detail: None,
                logins: parsed.logins,
                app_bound: 0,
                skipped: if parsed.skipped > 0 {
                    vec![("rows without an address or password".to_string(), parsed.skipped)]
                } else {
                    Vec::new()
                },
            }),
        });
        self.reply(tab, Event::ok(req_id, result));
    }

    /// Merges what was read into Krunk's stores (UI thread) and describes the counts.
    fn import_merge(&self, parsed: Parsed) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        if let Some(visits) = parsed.history {
            match self.store.borrow_mut().import_visits(&visits) {
                Ok(c) => {
                    info!("import: history {} added, {} already there", c.added, c.skipped);
                    out.insert("history".into(), serde_json::json!({ "imported": c.added, "merged": c.skipped }));
                }
                Err(e) => {
                    warning!("import: history failed: {e}");
                    out.insert("history".into(), serde_json::json!({ "imported": 0, "error": e.to_string() }));
                }
            }
        }
        if let Some(nodes) = parsed.bookmarks {
            match self.store.borrow().import_bookmark_nodes(&nodes) {
                Ok(s) => {
                    info!("import: bookmarks {} added ({} folders), {} skipped", s.bookmarks, s.folders, s.skipped);
                    out.insert(
                        "bookmarks".into(),
                        serde_json::json!({ "imported": s.bookmarks, "folders": s.folders, "merged": s.skipped }),
                    );
                    self.send_bookmarks_bar();
                    self.send_nav();
                }
                Err(e) => {
                    warning!("import: bookmarks failed: {e}");
                    out.insert("bookmarks".into(), serde_json::json!({ "imported": 0, "error": e }));
                }
            }
        }
        if let Some(ParsedLogins { logins, app_bound, mut skipped, source, empty_because, detail }) = parsed.logins {
            let merged = match self.vaults.vault.borrow_mut().as_mut() {
                Some(v) => v.import_logins(logins, now_ms(), &Dpapi).map_err(|e| e.to_string()),
                None => Err("Krunk passwords are locked".to_string()),
            };
            match merged {
                Ok(c) => {
                    if c.invalid > 0 {
                        skipped.push(("not a website login".to_string(), c.invalid));
                    }
                    let total: usize = skipped.iter().map(|(_, n)| n).sum();
                    let reasons: Vec<String> = skipped.iter().map(|(r, n)| format!("{r}: {n}")).collect();
                    info!(
                        "import: passwords {} added, {} updated, {} already there, {} skipped ({}), {} app-bound{}",
                        c.added,
                        c.updated,
                        c.merged,
                        total,
                        reasons.join(", "),
                        app_bound,
                        source.map(|s| format!("; {s}")).unwrap_or_default()
                    );
                    // Never a silent 0 (0.7.2, R12): nothing there says so; skipped entries
                    // carry their reasons.
                    let why = (c.added + c.updated + c.merged == 0 && total == 0 && app_bound == 0)
                        .then(|| empty_because.unwrap_or("no saved logins were found"));
                    out.insert(
                        "passwords".into(),
                        serde_json::json!({
                            "imported": c.added, "updated": c.updated, "merged": c.merged, "skipped": total,
                            "skippedReasons": skipped.iter().map(|(r, n)| serde_json::json!({ "reason": r, "count": n })).collect::<Vec<_>>(),
                            "appBound": app_bound,
                            "why": why,
                            "detail": detail,
                        }),
                    );
                    self.touch_vault();
                }
                Err(e) => {
                    warning!("import: passwords failed: {e}");
                    out.insert(
                        "passwords".into(),
                        serde_json::json!({ "imported": 0, "error": e, "appBound": app_bound }),
                    );
                }
            }
        }
        serde_json::Value::Object(out)
    }
}
