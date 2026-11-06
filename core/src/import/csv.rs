//! Password CSV files: the Chrome/Edge/Brave export (`name,url,username,password,note`) and the
//! Firefox export (`url,username,password,httpRealm,formActionOrigin,guid,timeCreated,
//! timeLastUsed,timePasswordChanged`). RFC 4180 quoting, CRLF or LF, an optional BOM. Columns
//! are found by header name, so other exports with the same names work too.

use zeroize::Zeroizing;

use super::{ImportError, Login};

/// Logins read from a CSV file and how many rows were unusable.
pub struct CsvLogins {
    pub logins: Vec<Login>,
    pub skipped: usize,
}

/// Parses a password CSV export.
pub fn parse(text: &str) -> Result<CsvLogins, ImportError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = records(text)?;
    let mut rows = rows.into_iter().filter(|r| !(r.len() == 1 && r[0].trim().is_empty()));
    let header = rows.next().ok_or_else(|| ImportError::Format("the CSV file is empty".into()))?;
    let col = |names: &[&str]| header.iter().position(|h| names.iter().any(|n| h.trim().eq_ignore_ascii_case(n)));
    let url = col(&["url", "login_uri", "website", "web site", "hostname"]);
    let username = col(&["username", "login_username", "user name", "login"]);
    let password = col(&["password", "login_password"]);
    let (Some(url), Some(username), Some(password)) = (url, username, password) else {
        return Err(ImportError::Format("the CSV file has no url, username and password columns".into()));
    };
    let note = col(&["note", "notes", "extra"]);
    let created = col(&["timeCreated", "time_created", "created"]);
    let mut out = CsvLogins { logins: Vec::new(), skipped: 0 };
    for row in rows {
        let get = |i: Option<usize>| i.and_then(|i| row.get(i)).map(|s| s.trim()).unwrap_or("");
        let url = get(Some(url)).to_string();
        let password = get(Some(password));
        if password.is_empty() || crate::urlclean::origin_of(&url).is_none() {
            out.skipped += 1;
            continue;
        }
        let created_ms = get(created).parse::<i64>().ok().filter(|t| *t > 0);
        out.logins.push(Login {
            url,
            username: get(Some(username)).to_string(),
            password: Zeroizing::new(password.to_string()),
            note: Zeroizing::new(get(note).to_string()),
            created_ms,
        });
    }
    Ok(out)
}

/// RFC 4180 records: fields separated by commas, quoted fields may hold commas, quotes
/// (doubled) and line breaks.
fn records(text: &str) -> Result<Vec<Vec<String>>, ImportError> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut after_quote = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    after_quote = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !after_quote => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                after_quote = false;
            }
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                after_quote = false;
            }
            _ if after_quote => return Err(ImportError::Format("malformed CSV quoting".into())),
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(ImportError::Format("unterminated quoted CSV field".into()));
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::testing::fixture;

    #[test]
    fn chrome_export() {
        let text = std::fs::read_to_string(fixture("chrome.csv")).unwrap();
        let got = parse(&text).unwrap();
        assert_eq!(got.skipped, 1, "the row without a URL");
        assert_eq!(got.logins.len(), 2);
        assert_eq!(got.logins[0].url, "https://login.example/");
        assert_eq!(got.logins[0].username, "csv-alice");
        assert_eq!(got.logins[0].password.as_str(), "Csv-pass-alice-1");
        assert_eq!(got.logins[1].password.as_str(), "Csv,pass \"bob\" 2");
        assert_eq!(got.logins[1].note.as_str(), "two lines\nof note");
        assert_eq!(got.logins[1].created_ms, None);
    }

    #[test]
    fn firefox_export() {
        let text = std::fs::read_to_string(fixture("firefox.csv")).unwrap();
        let got = parse(&text).unwrap();
        assert_eq!(got.skipped, 0);
        assert_eq!(got.logins.len(), 2);
        assert_eq!(got.logins[0].url, "https://login.example");
        assert_eq!(got.logins[1].username, "csv-carol");
        assert_eq!(got.logins[1].created_ms, Some(1_700_000_000_000));
    }

    #[test]
    fn quoting_bom_and_line_endings() {
        let text = "\u{feff}url,username,password\r\n\"https://a.example\",\"a,b\",\"x\"\"y\"\r\n\r\nhttps://b.example,bob,pw\n";
        let got = parse(text).unwrap();
        assert_eq!(got.logins.len(), 2);
        assert_eq!(got.logins[0].username, "a,b");
        assert_eq!(got.logins[0].password.as_str(), "x\"y");
        assert_eq!(got.logins[1].username, "bob");
    }

    #[test]
    fn bad_files_are_errors_not_panics() {
        assert!(matches!(parse(""), Err(ImportError::Format(_))));
        assert!(matches!(parse("name,site\nx,y\n"), Err(ImportError::Format(_))));
        assert!(matches!(parse("url,username,password\n\"unterminated,a,b\n"), Err(ImportError::Format(_))));
        assert!(matches!(parse("url,username,password\n\"a\"b,c,d\n"), Err(ImportError::Format(_))));
        // Empty passwords and non-URLs are skipped, not fatal.
        let got = parse("url,username,password\nhttps://a.example,u,\nnot a url,u,p\n").unwrap();
        assert_eq!((got.logins.len(), got.skipped), (0, 2));
    }
}
