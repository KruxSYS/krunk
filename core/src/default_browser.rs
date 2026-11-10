//! Krunk as the default browser (0.12.1). An installed Krunk registers itself with Windows as a
//! browser (per user, under HKCU): a ProgID that opens links and pages with Krunk, a
//! `StartMenuInternet` client with its capabilities, and an entry in `RegisteredApplications`.
//! Windows then lists Krunk under Settings → Apps → Default apps, where the user makes it the
//! default (Windows 10 and 11 let no program do that by itself). The installer writes the same
//! values (installer/krunk.nsi) and removes them on uninstall.
//!
//! Windows starts the browser with `"…\Krunk.exe" --single-argument %1`: everything after the
//! switch is one address, exactly as Windows passed it (a file path may hold spaces, and a link
//! can never smuggle in a switch). [`launch_targets`] reads that back and [`launch_url`]
//! decides what Krunk opens from it.

use url::Url;

/// The ProgID links and pages open with.
pub const PROG_ID: &str = "KrunkHTML";
/// Krunk's AppUserModelID (app/src/ffi/appid.rs sets it on the process and shortcuts).
pub const APP_USER_MODEL_ID: &str = "Krunk.Browser";
/// Krunk's name in `RegisteredApplications` and under `StartMenuInternet`.
pub const APP_NAME: &str = "Krunk";
/// The engine's own switch (base::CommandLine): the rest of the command line is one argument.
pub const SINGLE_ARGUMENT: &str = "--single-argument";
/// The URL schemes Krunk asks to be the default for.
pub const SCHEMES: [&str; 2] = ["http", "https"];
/// The file types Krunk offers to open (web pages).
pub const FILE_TYPES: [&str; 5] = [".htm", ".html", ".shtml", ".xht", ".xhtml"];
/// Windows Settings at Krunk's own page of Default apps (Windows 11); [`SETTINGS_FALLBACK`]
/// where that page cannot be opened.
pub const SETTINGS_URI: &str = "ms-settings:defaultapps?registeredAppUser=Krunk";
pub const SETTINGS_FALLBACK: &str = "ms-settings:defaultapps";
/// The longest address taken from the command line.
pub const MAX_LAUNCH_LEN: usize = 32 * 1024;

const CLIENT_KEY: &str = "Software\\Clients\\StartMenuInternet\\Krunk";
const DESCRIPTION: &str = "Krunk, a fast and private web browser";

/// One registry value under HKEY_CURRENT_USER; `name` "" is the key's default value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    pub key: String,
    pub name: &'static str,
    pub data: String,
}

fn value(key: impl Into<String>, name: &'static str, data: impl Into<String>) -> RegValue {
    RegValue { key: key.into(), name, data: data.into() }
}

/// The ProgID's open command for Krunk at `exe`.
pub fn open_command(exe: &str) -> String {
    format!("\"{exe}\" {SINGLE_ARGUMENT} %1")
}

/// The key that holds [`open_command`] (Krunk compares it to see whether it is registered).
pub fn open_command_key() -> String {
    format!("Software\\Classes\\{PROG_ID}\\shell\\open\\command")
}

/// Every value that registers Krunk at `exe` as a browser, in writing order.
pub fn registration(exe: &str) -> Vec<RegValue> {
    let icon = format!("{exe},0");
    let prog = format!("Software\\Classes\\{PROG_ID}");
    let caps = format!("{CLIENT_KEY}\\Capabilities");
    let mut v = vec![
        value(&prog, "", "Krunk HTML Document"),
        value(&prog, "AppUserModelId", APP_USER_MODEL_ID),
        value(format!("{prog}\\Application"), "ApplicationName", APP_NAME),
        value(format!("{prog}\\Application"), "ApplicationIcon", &icon),
        value(format!("{prog}\\Application"), "ApplicationCompany", APP_NAME),
        value(format!("{prog}\\Application"), "ApplicationDescription", DESCRIPTION),
        value(format!("{prog}\\Application"), "AppUserModelId", APP_USER_MODEL_ID),
        value(format!("{prog}\\DefaultIcon"), "", &icon),
        value(open_command_key(), "", open_command(exe)),
        value(CLIENT_KEY, "", APP_NAME),
        value(format!("{CLIENT_KEY}\\DefaultIcon"), "", &icon),
        value(format!("{CLIENT_KEY}\\shell\\open\\command"), "", format!("\"{exe}\"")),
        value(&caps, "ApplicationName", APP_NAME),
        value(&caps, "ApplicationIcon", &icon),
        value(&caps, "ApplicationDescription", DESCRIPTION),
        value(format!("{caps}\\StartMenu"), "StartMenuInternet", APP_NAME),
    ];
    for s in SCHEMES {
        v.push(value(format!("{caps}\\URLAssociations"), s, PROG_ID));
    }
    for t in FILE_TYPES {
        v.push(value(format!("{caps}\\FileAssociations"), t, PROG_ID));
        // "Open with" lists Krunk for the type (the value's data is not used).
        v.push(RegValue {
            key: format!("Software\\Classes\\{t}\\OpenWithProgids"),
            name: PROG_ID,
            data: String::new(),
        });
    }
    // Last: Windows lists Krunk once this points at complete capabilities.
    v.push(value("Software\\RegisteredApplications", APP_NAME, caps));
    v
}

/// Is Krunk the default browser, given the ProgIDs Windows opens http and https links with?
pub fn is_default(http: Option<&str>, https: Option<&str>) -> bool {
    let ours = |p: Option<&str>| p.is_some_and(|p| p.eq_ignore_ascii_case(PROG_ID));
    ours(http) && ours(https)
}

/// What Krunk was asked to open: `raw` is the process's command line as Windows passed it,
/// `args` the same split into arguments (the program first). With [`SINGLE_ARGUMENT`] the rest
/// of `raw` is one target; otherwise every argument that is not a switch is.
pub fn launch_targets(raw: &str, args: &[String]) -> Vec<String> {
    if let Some(one) = single_argument(raw) {
        return vec![one];
    }
    args.iter().skip(1).filter(|a| !a.starts_with('-') && !a.trim().is_empty()).cloned().collect()
}

fn single_argument(raw: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = raw[from..].find(SINGLE_ARGUMENT).map(|i| i + from) {
        let before_ok = i == 0 || raw[..i].ends_with(char::is_whitespace);
        let rest = &raw[i + SINGLE_ARGUMENT.len()..];
        if before_ok && (rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            let t = rest.trim();
            let t = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
            return (!t.is_empty()).then(|| t.to_string());
        }
        from = i + SINGLE_ARGUMENT.len();
    }
    None
}

/// The address Krunk opens for a launch target: a web address (http, https), a `file:` URL or
/// an absolute Windows path (as a `file:` URL). Anything else (`javascript:`, `data:`,
/// Krunk's own pages, relative paths, other schemes) is not opened.
pub fn launch_url(target: &str) -> Option<String> {
    let t = target.trim();
    if t.is_empty() || t.len() > MAX_LAUNCH_LEN || t.chars().any(char::is_control) {
        return None;
    }
    if let Some(rest) = t.strip_prefix("\\\\?\\") {
        return launch_url(rest).filter(|u| u.starts_with("file:"));
    }
    if is_windows_path(t) {
        return file_url(t);
    }
    let u = Url::parse(t).ok()?;
    match u.scheme() {
        "http" | "https" if u.host().is_some() => Some(u.into()),
        "file" => Some(u.into()),
        _ => None,
    }
}

fn is_windows_path(t: &str) -> bool {
    let b = t.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
        || t.starts_with("\\\\")
}

/// `C:\a b\x.html` → `file:///C:/a%20b/x.html`; `\\server\share\x.html` → `file://server/share/x.html`.
fn file_url(path: &str) -> Option<String> {
    let p = path.replace('\\', "/").replace('%', "%25");
    let mut u = if let Some(unc) = p.strip_prefix("//") {
        let (host, rest) = unc.split_once('/')?;
        let mut u = Url::parse(&format!("file://{host}/")).ok()?;
        u.set_path(rest);
        u
    } else {
        let mut u = Url::parse("file:///").ok()?;
        u.set_path(&format!("/{p}"));
        u
    };
    u.set_query(None);
    u.set_fragment(None);
    Some(u.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn single_argument_takes_the_rest_verbatim() {
        let raw = r#""C:\Users\me\AppData\Local\Programs\Krunk\Krunk.exe" --single-argument C:\My Pages\a b.html"#;
        let args = s(&["Krunk.exe", "--single-argument", "C:\\My", "Pages\\a", "b.html"]);
        assert_eq!(launch_targets(raw, &args), s(&["C:\\My Pages\\a b.html"]));
        // A link can't smuggle a switch in: it's part of the one argument.
        let raw = r#""Krunk.exe" --single-argument https://a.example/?x=1 --type=renderer"#;
        assert_eq!(launch_targets(raw, &[]), s(&["https://a.example/?x=1 --type=renderer"]));
        // Quoted by whoever started Krunk.
        assert_eq!(
            launch_targets(r#"Krunk.exe --single-argument "https://a.example/""#, &[]),
            s(&["https://a.example/"])
        );
        // Nothing after the switch: nothing to open.
        assert!(launch_targets("Krunk.exe --single-argument   ", &s(&["Krunk.exe", "--single-argument"])).is_empty());
        // Only the switch itself counts.
        assert_eq!(
            launch_targets(
                "Krunk.exe --single-argumentx https://b.example/",
                &s(&["Krunk.exe", "--single-argumentx", "https://b.example/"])
            ),
            s(&["https://b.example/"])
        );
    }

    #[test]
    fn plain_arguments_skip_switches() {
        let args = s(&["Krunk.exe", "--wait-for-pid=12", "https://a.example/", "-x", "https://b.example/"]);
        assert_eq!(
            launch_targets("Krunk.exe --wait-for-pid=12 https://a.example/ -x https://b.example/", &args),
            s(&["https://a.example/", "https://b.example/"])
        );
        assert!(launch_targets("Krunk.exe", &s(&["Krunk.exe"])).is_empty());
    }

    #[test]
    fn launch_urls() {
        assert_eq!(launch_url("https://a.example/x?y=1#z").as_deref(), Some("https://a.example/x?y=1#z"));
        assert_eq!(launch_url("HTTP://A.example").as_deref(), Some("http://a.example/"));
        assert_eq!(launch_url("file:///C:/x.html").as_deref(), Some("file:///C:/x.html"));
        assert_eq!(launch_url("C:\\My Pages\\a b.html").as_deref(), Some("file:///C:/My%20Pages/a%20b.html"));
        assert_eq!(launch_url("C:\\x\\100% #1?.html").as_deref(), Some("file:///C:/x/100%25%20%231%3F.html"));
        assert_eq!(launch_url("D:/site/index.htm").as_deref(), Some("file:///D:/site/index.htm"));
        assert_eq!(launch_url("\\\\nas\\share\\a.html").as_deref(), Some("file://nas/share/a.html"));
        assert_eq!(launch_url("\\\\?\\C:\\long\\a.html").as_deref(), Some("file:///C:/long/a.html"));
        for bad in [
            "",
            "   ",
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "krunk://settings",
            "chrome://gpu",
            "about:blank",
            "mailto:a@b.example",
            "relative\\page.html",
            "example.com",
            "https://a.example/\u{7}",
            "\\\\?\\https://a.example/",
        ] {
            assert_eq!(launch_url(bad), None, "{bad}");
        }
        assert_eq!(launch_url(&format!("https://a.example/{}", "x".repeat(MAX_LAUNCH_LEN))), None);
    }

    #[test]
    fn registration_values() {
        let exe = "C:\\Users\\me\\AppData\\Local\\Programs\\Krunk\\Krunk.exe";
        let reg = registration(exe);
        let get = |k: &str, n: &str| reg.iter().find(|v| v.key == k && v.name == n).map(|v| v.data.as_str());
        assert_eq!(get(&open_command_key(), ""), Some(format!("\"{exe}\" --single-argument %1").as_str()));
        assert_eq!(
            get("Software\\Clients\\StartMenuInternet\\Krunk\\Capabilities\\URLAssociations", "https"),
            Some("KrunkHTML")
        );
        assert_eq!(
            get("Software\\Clients\\StartMenuInternet\\Krunk\\Capabilities\\FileAssociations", ".html"),
            Some("KrunkHTML")
        );
        assert_eq!(get("Software\\Classes\\.htm\\OpenWithProgids", "KrunkHTML"), Some(""));
        assert_eq!(get("Software\\Classes\\KrunkHTML", "AppUserModelId"), Some("Krunk.Browser"));
        // RegisteredApplications comes last and points at the capabilities.
        let last = reg.last().unwrap();
        assert_eq!((last.key.as_str(), last.name), ("Software\\RegisteredApplications", "Krunk"));
        assert_eq!(last.data, "Software\\Clients\\StartMenuInternet\\Krunk\\Capabilities");
        // The deep link names the same registered application.
        assert!(SETTINGS_URI.ends_with(&format!("registeredAppUser={APP_NAME}")));
        // No value twice.
        let mut keys: Vec<_> = reg.iter().map(|v| (v.key.to_lowercase(), v.name.to_lowercase())).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), reg.len());
    }

    #[test]
    fn default_needs_both_schemes() {
        assert!(is_default(Some("KrunkHTML"), Some("krunkhtml")));
        assert!(!is_default(Some("KrunkHTML"), Some("ChromeHTML")));
        assert!(!is_default(Some("MSEdgeHTM"), Some("KrunkHTML")));
        assert!(!is_default(None, Some("KrunkHTML")));
    }
}
