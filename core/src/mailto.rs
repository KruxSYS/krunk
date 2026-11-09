//! Email links (Krunk 0.11.3): where a `mailto:` link goes. The engine never handles them
//! (Krunk refuses navigations to schemes it does not know); the host opens the user's choice
//! from Settings → General: the system's email app, or a webmail site in a new tab.

use serde::{Deserialize, Serialize};

/// Where email links open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MailHandler {
    /// The app Windows opens `mailto:` with (Outlook, Thunderbird, Mail, …).
    #[default]
    System,
    Gmail,
    Outlook,
    Yahoo,
    Proton,
    /// `Settings::mail_custom_url`: a web address with `%s` where the link goes.
    Custom,
}

/// What to do with an email link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailTarget {
    /// Hand the `mailto:` address to Windows.
    System(String),
    /// Open this web address in a new tab.
    Web(String),
}

/// Longest `mailto:` address passed on (a page could build a huge one).
pub const MAX_MAILTO_LEN: usize = 8192;

/// Is this an email link Krunk passes on?
pub fn is_mailto(url: &str) -> bool {
    url.len() <= MAX_MAILTO_LEN
        && url.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("mailto:"))
        && !url.chars().any(|c| c.is_control())
}

/// Is `t` a usable custom template: an http(s) address with `%s`?
pub fn valid_template(t: &str) -> bool {
    let t = t.trim();
    t.len() <= 2048
        && t.contains("%s")
        && url::Url::parse(&t.replace("%s", "x")).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

/// Where the email link `mailto` goes with the user's choice.
pub fn target(handler: MailHandler, custom: &str, mailto: &str) -> MailTarget {
    let enc = || url::form_urlencoded::byte_serialize(mailto.as_bytes()).collect::<String>();
    let web = |template: &str| MailTarget::Web(template.replace("%s", &enc()));
    match handler {
        MailHandler::System => MailTarget::System(mailto.to_string()),
        MailHandler::Gmail => web("https://mail.google.com/mail/?extsrc=mailto&url=%s"),
        MailHandler::Outlook => web("https://outlook.live.com/mail/0/deeplink/compose?mailtouri=%s"),
        MailHandler::Yahoo => web("https://compose.mail.yahoo.com/?mailto=%s"),
        MailHandler::Proton => web("https://mail.proton.me/u/0/inbox#mailto=%s"),
        MailHandler::Custom if valid_template(custom) => web(custom.trim()),
        MailHandler::Custom => MailTarget::System(mailto.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_links_go_where_the_user_chose() {
        let m = "mailto:a@b.example?subject=Hi%20there";
        assert!(is_mailto(m) && is_mailto("MAILTO:x@y.z"));
        assert!(!is_mailto("https://a.example/") && !is_mailto("mailto:a@b\n.example") && !is_mailto("mail"));
        assert!(!is_mailto(&format!("mailto:{}", "a".repeat(MAX_MAILTO_LEN))));
        assert_eq!(target(MailHandler::System, "", m), MailTarget::System(m.into()));
        assert_eq!(
            target(MailHandler::Gmail, "", m),
            MailTarget::Web(
                "https://mail.google.com/mail/?extsrc=mailto&url=mailto%3Aa%40b.example%3Fsubject%3DHi%2520there"
                    .into()
            )
        );
        assert!(
            matches!(target(MailHandler::Outlook, "", m), MailTarget::Web(u) if u.starts_with("https://outlook.live.com/") && u.contains("mailto%3Aa%40b.example"))
        );
        assert_eq!(
            target(MailHandler::Custom, " https://mail.example/compose?to=%s ", "mailto:a@b.example"),
            MailTarget::Web("https://mail.example/compose?to=mailto%3Aa%40b.example".into())
        );
        // A broken custom address falls back to the system's app.
        assert_eq!(target(MailHandler::Custom, "javascript:%s", m), MailTarget::System(m.into()));
        assert!(
            valid_template("https://x.example/?u=%s")
                && !valid_template("https://x.example/")
                && !valid_template("ftp://x/%s")
        );
    }
}
