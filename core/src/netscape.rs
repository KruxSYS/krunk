//! Netscape bookmark file format (the HTML that every browser imports and
//! exports).
//!
//! On export, the bookmarks bar becomes a folder marked
//! `PERSONAL_TOOLBAR_FOLDER="true"` and "Other bookmarks" is written at the
//! top level, the way Firefox does it. On import, the toolbar folder's
//! contents go to the bar and everything else goes to "Other bookmarks".

use crate::bookmarks::{bookmarkable, BookmarkNode};
use crate::storage::{DbResult, Store, BOOKMARKS_BAR, OTHER_BOOKMARKS};

/// Inputs bigger than this are refused.
pub const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportNode {
    Folder { title: String, toolbar: bool, add_date: i64, children: Vec<ImportNode> },
    Link { title: String, url: String, add_date: i64 },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportStats {
    pub bookmarks: usize,
    pub folders: usize,
    pub skipped: usize,
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[..rest.len().min(12)].find(';');
        let decoded = end.and_then(|e| {
            let ent = &rest[1..e];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            c.map(|c| (c, e + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Exports the store's bookmark tree.
pub fn export(store: &Store) -> DbResult<String> {
    Ok(export_tree(&store.bookmark_tree()?))
}

/// Exports `[bar, other]` roots as produced by `Store::bookmark_tree`.
pub fn export_tree(roots: &[BookmarkNode]) -> String {
    let mut out = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file.\n     It will be read and overwritten.\n     DO NOT EDIT! -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
    );
    for root in roots {
        if root.item.id == BOOKMARKS_BAR {
            out.push_str(&format!(
                "    <DT><H3 ADD_DATE=\"{}\" PERSONAL_TOOLBAR_FOLDER=\"true\">{}</H3>\n    <DL><p>\n",
                root.item.added / 1000,
                escape(&root.item.title)
            ));
            write_children(&mut out, &root.children, 2);
            out.push_str("    </DL><p>\n");
        } else {
            write_children(&mut out, &root.children, 1);
        }
    }
    out.push_str("</DL><p>\n");
    out
}

fn write_children(out: &mut String, nodes: &[BookmarkNode], depth: usize) {
    let pad = "    ".repeat(depth);
    for n in nodes {
        let b = &n.item;
        if b.is_folder {
            out.push_str(&format!(
                "{pad}<DT><H3 ADD_DATE=\"{}\">{}</H3>\n{pad}<DL><p>\n",
                b.added / 1000,
                escape(&b.title)
            ));
            write_children(out, &n.children, depth + 1);
            out.push_str(&format!("{pad}</DL><p>\n"));
        } else if let Some(url) = &b.url {
            out.push_str(&format!(
                "{pad}<DT><A HREF=\"{}\" ADD_DATE=\"{}\">{}</A>\n",
                escape(url),
                b.added / 1000,
                escape(&b.title)
            ));
        }
    }
}

/// Parses attributes of a tag body such as `A HREF="x" ADD_DATE=1`.
fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    // Skip the tag name.
    let mut i = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
            i += 1;
        }
        let name = tag[start..i].to_ascii_uppercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            if !name.is_empty() {
                out.push((name, String::new()));
            }
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
            let q = b[i];
            let vs = i + 1;
            i = vs;
            while i < b.len() && b[i] != q {
                i += 1;
            }
            let v = &tag[vs..i];
            i = (i + 1).min(b.len());
            v
        } else {
            let vs = i;
            while i < b.len() && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            &tag[vs..i]
        };
        out.push((name, unescape(value)));
    }
    out
}

fn attr<'a>(a: &'a [(String, String)], name: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

/// Text up to the closing tag `</name>` (case-insensitive), with any nested
/// tags removed. Returns the text and the remaining input.
fn text_until<'a>(input: &'a str, name: &str) -> (String, &'a str) {
    let upper = input.to_ascii_uppercase();
    let close = format!("</{name}");
    let end = upper.find(&close).unwrap_or(input.len());
    let raw = &input[..end];
    let rest = &input[end..];
    let rest = rest.find('>').map_or("", |i| &rest[i + 1..]);
    let mut text = String::new();
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    (unescape(text.trim()), rest)
}

/// Parses a Netscape bookmark file into a tree.
pub fn parse(html: &str) -> Result<Vec<ImportNode>, String> {
    if html.len() > MAX_IMPORT_BYTES {
        return Err("file too large".into());
    }
    // Stack of open lists; each list after the first belongs to the last
    // folder of the list below it.
    let mut stack: Vec<Vec<ImportNode>> = vec![Vec::new()];
    let mut seen_root_dl = false;
    let mut folder_pending = false;
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        if rest.starts_with("!--") {
            rest = rest.find("-->").map_or("", |i| &rest[i + 3..]);
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        rest = &rest[gt + 1..];
        let name = tag.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_uppercase();
        match name.as_str() {
            "DL" => {
                if folder_pending {
                    if stack.len() > MAX_DEPTH {
                        return Err("folders nested too deeply".into());
                    }
                    stack.push(Vec::new());
                    folder_pending = false;
                } else if !seen_root_dl {
                    seen_root_dl = true;
                } else {
                    // A list without a folder header: treat as part of the current list.
                    stack.push(Vec::new());
                    if let Some(top) = stack.iter_mut().rev().nth(1) {
                        top.push(ImportNode::Folder {
                            title: String::new(),
                            toolbar: false,
                            add_date: 0,
                            children: Vec::new(),
                        });
                    }
                }
            }
            "/DL" => {
                folder_pending = false;
                if stack.len() > 1 {
                    let children = stack.pop().unwrap_or_default();
                    if let Some(ImportNode::Folder { children: c, .. }) = stack.last_mut().and_then(|l| l.last_mut()) {
                        *c = children;
                    }
                }
            }
            "H3" => {
                let a = attrs(tag);
                let (title, r) = text_until(rest, "H3");
                rest = r;
                let toolbar = attr(&a, "PERSONAL_TOOLBAR_FOLDER").is_some_and(|v| v.eq_ignore_ascii_case("true"));
                let add_date = attr(&a, "ADD_DATE").and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(top) = stack.last_mut() {
                    top.push(ImportNode::Folder { title, toolbar, add_date, children: Vec::new() });
                }
                folder_pending = true;
            }
            "A" => {
                let a = attrs(tag);
                let (title, r) = text_until(rest, "A");
                rest = r;
                folder_pending = false;
                if let Some(url) = attr(&a, "HREF") {
                    let add_date = attr(&a, "ADD_DATE").and_then(|v| v.parse().ok()).unwrap_or(0);
                    if let Some(top) = stack.last_mut() {
                        top.push(ImportNode::Link { title, url: url.to_string(), add_date });
                    }
                }
            }
            _ => {}
        }
    }
    // Close any unterminated lists.
    while stack.len() > 1 {
        let children = stack.pop().unwrap_or_default();
        if let Some(ImportNode::Folder { children: c, .. }) = stack.last_mut().and_then(|l| l.last_mut()) {
            *c = children;
        }
    }
    Ok(stack.pop().unwrap_or_default())
}

impl Store {
    /// Imports a Netscape bookmark file. Exact duplicates (same folder and
    /// URL) and non-bookmarkable URLs (`javascript:`, `place:`) are skipped.
    pub fn import_bookmarks(&self, html: &str) -> Result<ImportStats, String> {
        let nodes = parse(html)?;
        let mut stats = ImportStats::default();
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for node in &nodes {
            match node {
                ImportNode::Folder { toolbar: true, children, .. } => {
                    self.import_into(BOOKMARKS_BAR, children, &mut stats).map_err(|e| e.to_string())?
                }
                other => self
                    .import_into(OTHER_BOOKMARKS, std::slice::from_ref(other), &mut stats)
                    .map_err(|e| e.to_string())?,
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(stats)
    }

    fn import_into(&self, parent: i64, nodes: &[ImportNode], stats: &mut ImportStats) -> DbResult<()> {
        for node in nodes {
            match node {
                ImportNode::Link { title, url, add_date } => {
                    let exists = self.children(parent)?.iter().any(|b| b.url.as_deref() == Some(url.as_str()));
                    if exists || !bookmarkable(url) {
                        stats.skipped += 1;
                        continue;
                    }
                    self.add_bookmark_at(parent, title, url, add_date.saturating_mul(1000))?;
                    stats.bookmarks += 1;
                }
                ImportNode::Folder { title, add_date, children, .. } => {
                    let existing = self.children(parent)?.into_iter().find(|b| b.is_folder && &b.title == title);
                    let id = match existing {
                        Some(f) => f.id,
                        None => {
                            stats.folders += 1;
                            self.add_folder_at(parent, title, add_date.saturating_mul(1000))?
                        }
                    };
                    self.import_into(id, children, stats)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(store: &Store) -> Vec<String> {
        fn walk(nodes: &[BookmarkNode], depth: usize, out: &mut Vec<String>) {
            for n in nodes {
                let b = &n.item;
                out.push(format!(
                    "{}{}|{}|{}",
                    "  ".repeat(depth),
                    b.title,
                    b.url.clone().unwrap_or_default(),
                    b.added
                ));
                walk(&n.children, depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(&store.bookmark_tree().unwrap(), 0, &mut out);
        out
    }

    #[test]
    fn round_trip() {
        let src = Store::open_in_memory().unwrap();
        src.add_bookmark_at(BOOKMARKS_BAR, "Rust & <Friends>", "https://rust-lang.org/?a=1&b=\"2\"", 1_700_000_000_000)
            .unwrap();
        let f = src.add_folder_at(BOOKMARKS_BAR, "Dev 'tools'", 1_600_000_000_000).unwrap();
        let sub = src.add_folder_at(f, "Nested", 1_600_000_001_000).unwrap();
        src.add_bookmark_at(sub, "Déjà vu ✓", "https://example.com/é", 1_600_000_002_000).unwrap();
        src.add_bookmark_at(OTHER_BOOKMARKS, "Other one", "https://other.example/", 1_500_000_000_000).unwrap();
        let o = src.add_folder_at(OTHER_BOOKMARKS, "Reading", 1_500_000_001_000).unwrap();
        src.add_bookmark_at(o, "Article", "https://news.example/a", 1_500_000_002_000).unwrap();

        let html = export(&src).unwrap();
        assert!(html.contains("PERSONAL_TOOLBAR_FOLDER=\"true\""));
        assert!(html.contains("Rust &amp; &lt;Friends&gt;"));

        let dst = Store::open_in_memory().unwrap();
        let stats = dst.import_bookmarks(&html).unwrap();
        assert_eq!(stats, ImportStats { bookmarks: 4, folders: 3, skipped: 0 });
        assert_eq!(shape(&src), shape(&dst));

        // Importing the same file again adds nothing.
        let again = dst.import_bookmarks(&html).unwrap();
        assert_eq!(again.bookmarks, 0);
        assert_eq!(again.skipped, 4);
        assert_eq!(shape(&src), shape(&dst));
    }

    #[test]
    fn imports_chrome_style_file() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1" LAST_MODIFIED="2" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://a.example/" ADD_DATE="10" ICON="data:image/png;base64,AAA">A &#38; B</A>
        <DT><A HREF="javascript:alert(1)">Bookmarklet</A>
    </DL><p>
    <DT><H3 ADD_DATE="3">Other bookmarks</H3>
    <DL><p>
        <dt><a href='https://c.example/' add_date=30>C</a>
    </DL><p>
</DL><p>"#;
        let s = Store::open_in_memory().unwrap();
        let stats = s.import_bookmarks(html).unwrap();
        assert_eq!(stats.bookmarks, 2);
        assert_eq!(stats.skipped, 1);
        let bar = s.children(BOOKMARKS_BAR).unwrap();
        assert_eq!(bar[0].title, "A & B");
        assert_eq!(bar[0].added, 10_000);
        let other = s.children(OTHER_BOOKMARKS).unwrap();
        assert_eq!(other[0].title, "Other bookmarks");
        assert_eq!(s.children(other[0].id).unwrap()[0].url.as_deref(), Some("https://c.example/"));
    }

    #[test]
    fn tolerates_garbage() {
        assert_eq!(parse("").unwrap(), vec![]);
        assert!(parse("<DL><DT><H3>x</H3><DL><DT><A HREF=\"https://x/\">y").is_ok());
        let deep = "<DL>".to_string() + &"<DT><H3>f</H3><DL>".repeat(100);
        assert!(parse(&deep).is_err());
        assert_eq!(unescape("&amp;&bogus;&#x41;&#66;&"), "&&bogus;AB&");
    }
}
