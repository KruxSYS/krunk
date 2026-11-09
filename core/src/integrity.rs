//! Are the installed files the ones this release shipped? (Krunk 0.7.0)
//!
//! Two lists answer it:
//! - the engine's files (libcef.dll, the .pak/.dat/.bin files, locales/…) are built into
//!   Krunk from `engine-files.sha256` (made from the CEF distribution; `build-dist.sh` checks
//!   that the package matches it), so every copy can check its engine without a network;
//! - the update server publishes the release's whole file list (`files.json`, its SHA-256 in
//!   the signed manifest), which also covers Krunk.exe and Krunk.dll.
//!
//! A silent update that could not replace a locked `libcef.dll` used to leave the old engine
//! behind (0.6.0: an install still running the codec-less engine of 0.5.1). The check names
//! the files that differ and which engine `libcef.dll` is, and the browser offers a repair.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// The engine files of this release: `<sha256>  <relative path>` lines.
const ENGINE_FILES: &str = include_str!("../../engine-files.sha256");

/// `libcef.dll` of the CEF 156.0.15 "minimal" build Krunk shipped up to 0.5.1 (no H.264, AAC,
/// HEVC or AC-3 decoders).
pub const MINIMAL_LIBCEF_SHA256: &str = "07168eacd81bc09ab9ab895d39bc1a80913b09c844f0b5a4730f5f89a71133f5";

/// The largest `files.json` Krunk reads.
pub const MAX_FILES_JSON_BYTES: usize = 256 * 1024;

pub type Sha256 = [u8; 32];

/// Parses `sha256sum` output (`<64 hex>  <path>` per line; `*` binary marks allowed).
pub fn parse_sha256sums(text: &str) -> Result<BTreeMap<String, Sha256>, String> {
    let mut out = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (hex, path) = line.split_once(' ').ok_or_else(|| format!("line {}: no path", n + 1))?;
        let path = path.trim_start().trim_start_matches('*');
        let hash = parse_hex(hex).ok_or_else(|| format!("line {}: bad SHA-256", n + 1))?;
        let path = check_path(path).ok_or_else(|| format!("line {}: bad path", n + 1))?;
        out.insert(path, hash);
    }
    Ok(out)
}

/// The engine files built into this copy.
pub fn engine_files() -> BTreeMap<String, Sha256> {
    #[allow(clippy::expect_used)] // the list is checked by a test and by build-dist.sh
    parse_sha256sums(ENGINE_FILES).expect("engine-files.sha256")
}

/// A relative path inside the install folder, with forward slashes: no `..`, no root, no
/// drive, nothing odd.
fn check_path(p: &str) -> Option<String> {
    let p = p.replace('\\', "/");
    let ok = !p.is_empty()
        && p.len() <= 260
        && !p.starts_with('/')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
        && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-/ +()".contains(c));
    ok.then_some(p)
}

pub fn parse_hex(hex: &str) -> Option<Sha256> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn to_hex(h: &Sha256) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

/// The release's file list from the update server (`files.json`).
#[derive(Debug, Deserialize)]
struct FilesJson {
    version: String,
    files: BTreeMap<String, String>,
}

/// Reads `files.json` (its bytes were already checked against the signed manifest) and
/// insists it is for `version`.
pub fn parse_files_json(bytes: &[u8], version: &str) -> Result<BTreeMap<String, Sha256>, String> {
    if bytes.len() > MAX_FILES_JSON_BYTES {
        return Err("files.json is too large".into());
    }
    let f: FilesJson = serde_json::from_slice(bytes).map_err(|e| format!("files.json: {e}"))?;
    if f.version != version {
        return Err(format!("files.json is for {}, not {version}", f.version));
    }
    let mut out = BTreeMap::new();
    for (path, hex) in f.files {
        let p = check_path(&path).ok_or_else(|| format!("files.json: bad path {path:?}"))?;
        let h = parse_hex(&hex).ok_or_else(|| format!("files.json: bad SHA-256 for {p}"))?;
        out.insert(p, h);
    }
    if out.is_empty() {
        return Err("files.json lists no files".into());
    }
    Ok(out)
}

/// Which engine a `libcef.dll` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Flavor {
    /// The engine this release ships (Krunk's build with H.264/AAC since 0.5.2).
    Release,
    /// The old CEF "minimal" build: no H.264, AAC, HEVC or AC-3.
    Minimal,
    /// Something else (a different build, or a damaged file).
    Unknown,
    /// No libcef.dll next to Krunk.
    Missing,
}

pub fn flavor_of(libcef: Option<&Sha256>, expected: &BTreeMap<String, Sha256>) -> Flavor {
    match libcef {
        None => Flavor::Missing,
        Some(h) if expected.get("libcef.dll") == Some(h) => Flavor::Release,
        Some(h) if to_hex(h) == MINIMAL_LIBCEF_SHA256 => Flavor::Minimal,
        Some(_) => Flavor::Unknown,
    }
}

/// One file that is not what the list says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub path: String,
    /// "missing" or "changed".
    pub kind: String,
}

/// Compares the installed files (`actual(path)` = their SHA-256, `None` if missing) with a list.
pub fn compare(expected: &BTreeMap<String, Sha256>, actual: &mut dyn FnMut(&str) -> Option<Sha256>) -> Vec<Problem> {
    let mut out = Vec::new();
    for (path, want) in expected {
        match actual(path) {
            None => out.push(Problem { path: path.clone(), kind: "missing".into() }),
            Some(h) if &h != want => out.push(Problem { path: path.clone(), kind: "changed".into() }),
            Some(_) => {}
        }
    }
    out
}

/// Hashes are expensive (libcef.dll alone is ~300 MB): a file whose size and modification time
/// are unchanged keeps its last hash. Stored in the profile's kv table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HashCache {
    pub entries: BTreeMap<String, CachedHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedHash {
    pub size: u64,
    pub mtime: i64,
    pub sha256: String,
}

impl HashCache {
    pub fn from_json(json: &str) -> HashCache {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// The cached hash, if the file still has this size and time.
    pub fn get(&self, path: &str, size: u64, mtime: i64) -> Option<Sha256> {
        let e = self.entries.get(path)?;
        if e.size == size && e.mtime == mtime {
            parse_hex(&e.sha256)
        } else {
            None
        }
    }

    pub fn put(&mut self, path: &str, size: u64, mtime: i64, h: &Sha256) {
        self.entries.insert(path.to_string(), CachedHash { size, mtime, sha256: to_hex(h) });
    }
}

/// The SHA-256 of a file, read in 1 MiB pieces.
pub fn sha256_file(path: &Path) -> std::io::Result<Sha256> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

/// The installed files' hashes (`None`: missing or unreadable), reusing `cache` for files
/// whose size and modification time are unchanged (and updating it).
pub fn hash_installed<'a>(
    dir: &Path,
    paths: impl IntoIterator<Item = &'a String>,
    cache: &mut HashCache,
) -> BTreeMap<String, Option<Sha256>> {
    let mut out = BTreeMap::new();
    for p in paths {
        let full = dir.join(p);
        let h = std::fs::metadata(&full).ok().filter(|m| m.is_file()).and_then(|m| {
            let size = m.len();
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as i64);
            cache.get(p, size, mtime).or_else(|| {
                let h = sha256_file(&full).ok()?;
                cache.put(p, size, mtime, &h);
                Some(h)
            })
        });
        out.insert(p.clone(), h);
    }
    // Forget files that are no longer checked.
    cache.entries.retain(|k, _| out.contains_key(k));
    out
}

/// The result shown on krunk://gpu and used for the repair prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Report {
    /// The check ran (it runs once per start, in the background).
    pub checked: bool,
    pub flavor: Option<Flavor>,
    /// The first 16 hex digits of the installed libcef.dll's SHA-256.
    pub libcef: String,
    pub engine_files: usize,
    pub problems: Vec<Problem>,
    /// Problems found against the update server's list (Krunk.exe, Krunk.dll included).
    pub release_problems: Vec<Problem>,
    pub release_checked: bool,
}

impl Report {
    pub fn healthy(&self) -> bool {
        self.problems.is_empty() && self.release_problems.is_empty() && self.flavor != Some(Flavor::Minimal)
    }

    /// One sentence for the prompt and the page.
    pub fn summary(&self) -> String {
        let mut all: Vec<&Problem> = self.problems.iter().chain(&self.release_problems).collect();
        all.dedup_by(|a, b| a.path == b.path);
        match (self.flavor, all.len()) {
            (Some(Flavor::Minimal), _) => {
                "libcef.dll is the old engine without H.264/AAC (Krunk 0.5.1 or earlier): an update did not replace it"
                    .into()
            }
            (_, 0) => "The installed files match this release".into(),
            (_, 1) => format!("{} is {}", all[0].path, all[0].kind),
            (_, n) => format!("{n} files don't match this release (first: {} is {})", all[0].path, all[0].kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> Sha256 {
        [b; 32]
    }

    #[test]
    fn the_built_in_list_is_the_codec_engine() {
        let files = engine_files();
        assert!(files.len() > 30, "{}", files.len());
        assert_eq!(
            to_hex(&files["libcef.dll"]),
            "d9f4549be3fc0ecace086b345c63e4f3ef822d5bc80d923c73674c05948e5eb9",
            "libcef.dll must be the self-built CEF 156.0.15 with codecs and Krunk's patches (cef.lock)"
        );
        assert!(files.contains_key("locales/en-US.pak") && files.contains_key("snapshot_blob.bin"));
        // Krunk's own files are not engine files (they change with every build).
        assert!(!files.contains_key("Krunk.exe") && !files.contains_key("Krunk.dll"));
    }

    #[test]
    fn sums_parse_and_reject_bad_lines() {
        let ok = format!("{}  libcef.dll\n{} *locales/de.pak\n\n", "ab".repeat(32), "cd".repeat(32));
        let m = parse_sha256sums(&ok).unwrap();
        assert_eq!(m["libcef.dll"], [0xab; 32]);
        assert_eq!(m["locales/de.pak"], [0xcd; 32]);
        assert!(parse_sha256sums("abc  x").is_err());
        assert!(parse_sha256sums(&format!("{}  ../evil.dll", "ab".repeat(32))).is_err());
        assert!(parse_sha256sums(&format!("{}  C:/x.dll", "ab".repeat(32))).is_err());
        assert!(parse_sha256sums(&format!("{}  /abs.dll", "ab".repeat(32))).is_err());
    }

    #[test]
    fn files_json_must_match_the_version() {
        let j = format!(
            r#"{{"version":"0.7.0","files":{{"Krunk.dll":"{}","locales\\en-US.pak":"{}"}}}}"#,
            "11".repeat(32),
            "22".repeat(32)
        );
        let m = parse_files_json(j.as_bytes(), "0.7.0").unwrap();
        assert_eq!(m.len(), 2);
        assert!(m.contains_key("locales/en-US.pak"));
        assert!(parse_files_json(j.as_bytes(), "0.7.1").unwrap_err().contains("not 0.7.1"));
        assert!(parse_files_json(br#"{"version":"0.7.0","files":{}}"#, "0.7.0").is_err());
        let bad = format!(r#"{{"version":"0.7.0","files":{{"../x":"{}"}}}}"#, "11".repeat(32));
        assert!(parse_files_json(bad.as_bytes(), "0.7.0").is_err());
    }

    #[test]
    fn flavors() {
        let mut expected = BTreeMap::new();
        expected.insert("libcef.dll".to_string(), h(1));
        assert_eq!(flavor_of(Some(&h(1)), &expected), Flavor::Release);
        assert_eq!(flavor_of(Some(&parse_hex(MINIMAL_LIBCEF_SHA256).unwrap()), &expected), Flavor::Minimal);
        assert_eq!(flavor_of(Some(&h(9)), &expected), Flavor::Unknown);
        assert_eq!(flavor_of(None, &expected), Flavor::Missing);
    }

    #[test]
    fn compare_names_missing_and_changed_files() {
        let mut expected = BTreeMap::new();
        expected.insert("a.dll".to_string(), h(1));
        expected.insert("b.pak".to_string(), h(2));
        expected.insert("c.bin".to_string(), h(3));
        let mut actual = |p: &str| match p {
            "a.dll" => Some(h(1)),
            "b.pak" => Some(h(7)),
            _ => None,
        };
        let p = compare(&expected, &mut actual);
        assert_eq!(
            p,
            vec![
                Problem { path: "b.pak".into(), kind: "changed".into() },
                Problem { path: "c.bin".into(), kind: "missing".into() }
            ]
        );
    }

    #[test]
    fn the_cache_follows_size_and_time() {
        let mut c = HashCache::default();
        c.put("libcef.dll", 10, 5, &h(4));
        let c = HashCache::from_json(&c.to_json());
        assert_eq!(c.get("libcef.dll", 10, 5), Some(h(4)));
        assert_eq!(c.get("libcef.dll", 11, 5), None);
        assert_eq!(c.get("libcef.dll", 10, 6), None);
        assert_eq!(c.get("other", 10, 5), None);
        assert_eq!(HashCache::from_json("garbage"), HashCache::default());
    }

    #[test]
    fn installed_files_are_hashed_once() {
        let dir = std::env::temp_dir().join(format!("krunk-integrity-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("locales")).unwrap();
        std::fs::write(dir.join("libcef.dll"), b"engine").unwrap();
        std::fs::write(dir.join("locales/de.pak"), b"de").unwrap();
        let paths: Vec<String> = vec!["libcef.dll".into(), "locales/de.pak".into(), "gone.bin".into()];
        let mut cache = HashCache::default();
        let got = hash_installed(&dir, &paths, &mut cache);
        let engine: Sha256 = sha2::Sha256::digest(b"engine").into();
        assert_eq!(got["libcef.dll"], Some(engine));
        assert_eq!(got["gone.bin"], None);
        assert_eq!(cache.entries.len(), 2);
        // A cached hash is used while size and time are unchanged (even if it is wrong).
        let e = cache.entries.get_mut("libcef.dll").unwrap();
        e.sha256 = "00".repeat(32);
        assert_eq!(hash_installed(&dir, &paths, &mut cache)["libcef.dll"], Some([0; 32]));
        // A rewritten file of another size is hashed again.
        std::fs::write(dir.join("libcef.dll"), b"engine v2").unwrap();
        let v2: Sha256 = sha2::Sha256::digest(b"engine v2").into();
        assert_eq!(hash_installed(&dir, &paths, &mut cache)["libcef.dll"], Some(v2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_summaries() {
        let mut r = Report { checked: true, flavor: Some(Flavor::Release), ..Report::default() };
        assert!(r.healthy());
        assert_eq!(r.summary(), "The installed files match this release");
        r.flavor = Some(Flavor::Minimal);
        assert!(!r.healthy());
        assert!(r.summary().contains("without H.264/AAC"));
        r.flavor = Some(Flavor::Release);
        r.problems.push(Problem { path: "libcef.dll".into(), kind: "changed".into() });
        assert_eq!(r.summary(), "libcef.dll is changed");
        r.release_problems.push(Problem { path: "Krunk.dll".into(), kind: "missing".into() });
        assert!(r.summary().starts_with("2 files don't match"));
    }
}
