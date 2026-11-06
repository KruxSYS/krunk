//! Finding the other browsers' profiles on this machine. Pure path logic over the two
//! app-data roots the host passes in (`%APPDATA%` and `%LOCALAPPDATA%` on Windows), so the
//! tests run anywhere with a temporary tree.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Firefox,
    Chrome,
    Edge,
    Brave,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Firefox => "Firefox",
            Family::Chrome => "Google Chrome",
            Family::Edge => "Microsoft Edge",
            Family::Brave => "Brave",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Family::Firefox => "firefox",
            Family::Chrome => "chrome",
            Family::Edge => "edge",
            Family::Brave => "brave",
        }
    }

    /// The Chromium family's "User Data" folder under `%LOCALAPPDATA%`.
    fn user_data(self) -> Option<&'static str> {
        match self {
            Family::Firefox => None,
            Family::Chrome => Some("Google/Chrome/User Data"),
            Family::Edge => Some("Microsoft/Edge/User Data"),
            Family::Brave => Some("BraveSoftware/Brave-Browser/User Data"),
        }
    }
}

/// One profile that can be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Stable id for the settings page: `family:name`.
    pub id: String,
    pub family: Family,
    /// The profile's display name ("default-release", "Person 1").
    pub name: String,
    /// The profile directory (with places.sqlite / History inside).
    pub path: PathBuf,
    /// Chromium only: the "User Data" directory with `Local State`.
    pub user_data: Option<PathBuf>,
    /// The browser's default profile.
    pub is_default: bool,
    pub has_passwords: bool,
    pub has_history: bool,
    pub has_bookmarks: bool,
}

/// Every profile of every supported browser under the two roots.
pub fn detect(roaming: &Path, local: &Path) -> Vec<Profile> {
    let mut out = firefox_profiles(roaming);
    for family in [Family::Chrome, Family::Edge, Family::Brave] {
        out.extend(chromium_profiles(local, family));
    }
    out
}

/// Firefox profiles from `Mozilla/Firefox/profiles.ini`.
pub fn firefox_profiles(roaming: &Path) -> Vec<Profile> {
    let base = roaming.join("Mozilla").join("Firefox");
    let Ok(ini) = std::fs::read_to_string(base.join("profiles.ini")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut install_default: Option<String> = None;
    let mut section = String::new();
    let mut cur: Option<(String, String, bool, bool)> = None; // name, path, relative, default
    let flush = |cur: &mut Option<(String, String, bool, bool)>, out: &mut Vec<Profile>| {
        if let Some((name, path, relative, default)) = cur.take() {
            let dir = if relative {
                base.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
            } else {
                PathBuf::from(&path)
            };
            if dir.is_dir() {
                let name = if name.is_empty() {
                    dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()
                } else {
                    name
                };
                out.push(profile(Family::Firefox, name, dir, None, default, &path));
            }
        }
    };
    for line in ini.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            flush(&mut cur, &mut out);
            section = name.to_string();
            if section.starts_with("Profile") {
                cur = Some((String::new(), String::new(), true, false));
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if section.starts_with("Install") && k == "Default" {
            install_default = Some(v.to_string());
        }
        if let Some(c) = cur.as_mut() {
            match k {
                "Name" => c.0 = v.to_string(),
                "Path" => c.1 = v.to_string(),
                "IsRelative" => c.2 = v != "0",
                "Default" => c.3 = v == "1",
                _ => {}
            }
        }
    }
    flush(&mut cur, &mut out);
    if let Some(d) = install_default {
        for p in &mut out {
            if p.id.ends_with(&format!(":{}", d.rsplit('/').next().unwrap_or(&d)))
                || p.path.ends_with(d.replace('/', std::path::MAIN_SEPARATOR_STR))
            {
                p.is_default = true;
            }
        }
    }
    out
}

/// Profiles of one Chromium-family browser from its `Local State`.
pub fn chromium_profiles(local: &Path, family: Family) -> Vec<Profile> {
    let Some(rel) = family.user_data() else { return Vec::new() };
    let user_data = local.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    let Ok(state) = std::fs::read_to_string(user_data.join("Local State")) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&state) else { return Vec::new() };
    let last_used = json.pointer("/profile/last_used").and_then(|v| v.as_str()).unwrap_or("Default").to_string();
    let mut out = Vec::new();
    if let Some(cache) = json.pointer("/profile/info_cache").and_then(|v| v.as_object()) {
        let mut keys: Vec<&String> = cache.keys().collect();
        keys.sort();
        for key in keys {
            let dir = user_data.join(key);
            if !dir.is_dir() {
                continue;
            }
            let shown = cache[key].get("name").and_then(|v| v.as_str()).unwrap_or(key);
            let name = if shown == key || shown.is_empty() { key.clone() } else { format!("{shown} ({key})") };
            out.push(profile(family, name, dir, Some(user_data.clone()), key == &last_used, key));
        }
    } else if user_data.join("Default").is_dir() {
        out.push(profile(
            family,
            "Default".into(),
            user_data.join("Default"),
            Some(user_data.clone()),
            true,
            "Default",
        ));
    }
    out
}

fn profile(
    family: Family,
    name: String,
    path: PathBuf,
    user_data: Option<PathBuf>,
    is_default: bool,
    key: &str,
) -> Profile {
    let (pw, hist, bm) = match family {
        Family::Firefox => (path.join("logins.json"), path.join("places.sqlite"), path.join("places.sqlite")),
        _ => (path.join("Login Data"), path.join("History"), path.join("Bookmarks")),
    };
    Profile {
        id: format!("{}:{}", family.id(), key.rsplit('/').next().unwrap_or(key)),
        family,
        name,
        has_passwords: pw.is_file(),
        has_history: hist.is_file(),
        has_bookmarks: bm.is_file(),
        path,
        user_data,
        is_default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);
    impl Tree {
        fn new(name: &str) -> Tree {
            let p = std::env::temp_dir().join(format!("lb-import-profiles-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tree(p)
        }
        fn file(&self, rel: &str, content: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn firefox_profiles_from_profiles_ini() {
        let t = Tree::new("ff");
        t.file(
            "roaming/Mozilla/Firefox/profiles.ini",
            "[Install308046B0AF4A39CB]\nDefault=Profiles/abcd.default-release\nLocked=1\n\n[Profile1]\nName=default\nIsRelative=1\nPath=Profiles/wxyz.default\nDefault=1\n\n[Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/abcd.default-release\n\n[Profile2]\nName=gone\nIsRelative=1\nPath=Profiles/missing\n\n[General]\nStartWithLastProfile=1\nVersion=2\n",
        );
        t.file("roaming/Mozilla/Firefox/Profiles/abcd.default-release/logins.json", "{}");
        t.file("roaming/Mozilla/Firefox/Profiles/abcd.default-release/places.sqlite", "");
        t.file("roaming/Mozilla/Firefox/Profiles/wxyz.default/times.json", "{}");
        let got = firefox_profiles(&t.0.join("roaming"));
        assert_eq!(got.len(), 2, "{got:?}");
        let rel = got.iter().find(|p| p.name == "default-release").unwrap();
        assert!(rel.is_default && rel.has_passwords && rel.has_history && rel.has_bookmarks);
        assert_eq!(rel.id, "firefox:abcd.default-release");
        let old = got.iter().find(|p| p.name == "default").unwrap();
        assert!(!old.has_passwords && !old.has_history);
        assert!(detect(&t.0.join("roaming"), &t.0.join("local")).len() == 2);
    }

    #[test]
    fn chromium_profiles_from_local_state() {
        let t = Tree::new("cr");
        t.file(
            "local/Google/Chrome/User Data/Local State",
            r#"{"profile":{"info_cache":{"Default":{"name":"Person 1"},"Profile 1":{"name":"Work"},"Profile 9":{"name":"Deleted"}},"last_used":"Profile 1"}}"#,
        );
        t.file("local/Google/Chrome/User Data/Default/Login Data", "");
        t.file("local/Google/Chrome/User Data/Default/History", "");
        t.file("local/Google/Chrome/User Data/Profile 1/Bookmarks", "{}");
        t.file("local/Microsoft/Edge/User Data/Local State", r#"{"profile":{}}"#);
        t.file("local/Microsoft/Edge/User Data/Default/History", "");
        let got = detect(&t.0.join("roaming"), &t.0.join("local"));
        let ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["chrome:Default", "chrome:Profile 1", "edge:Default"]);
        assert_eq!(got[0].name, "Person 1 (Default)");
        assert!(got[0].has_passwords && got[0].has_history && !got[0].has_bookmarks && !got[0].is_default);
        assert!(got[1].is_default && got[1].has_bookmarks);
        assert_eq!(got[1].family, Family::Chrome);
        assert_eq!(got[2].family.name(), "Microsoft Edge");
        assert!(got[2].user_data.as_ref().unwrap().ends_with("User Data"));
    }

    #[test]
    fn nothing_installed() {
        let t = Tree::new("none");
        assert!(detect(&t.0, &t.0).is_empty());
    }
}
