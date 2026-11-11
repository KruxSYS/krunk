//! Long userscripts and macros saved in parts (0.15.0).
//!
//! A script too long for one answer goes in several `save_userscript` / `save_macro` calls:
//! `more: true` appends the call's `source` to a draft (a new one without `draft`), and the call
//! with the draft id and without `more` adds the last part and hands the whole source to the
//! normal save, which checks it (metadata block, syntax) only then. Drafts live with the
//! conversation, at most [`MAX_DRAFTS`] at once, each at most [`MAX_DRAFT_BYTES`].

use super::ScriptKind;

/// The size cap of a script (a userscript's; a macro may be a little larger).
pub const MAX_DRAFT_BYTES: usize = crate::userscripts::MAX_SOURCE_BYTES;
/// Drafts kept at once; starting another drops the oldest.
pub const MAX_DRAFTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Draft {
    id: String,
    kind: ScriptKind,
    source: String,
    parts: u32,
}

/// What a part added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub draft: String,
    /// Parts so far, this one included.
    pub parts: u32,
    /// Bytes so far.
    pub bytes: usize,
}

/// One conversation's drafts.
#[derive(Debug, Default)]
pub struct Drafts {
    list: Vec<Draft>,
    next: u32,
}

fn kind_name(k: ScriptKind) -> &'static str {
    match k {
        ScriptKind::Userscript => "userscript",
        ScriptKind::Macro => "macro",
    }
}

fn too_big(bytes: usize) -> String {
    format!(
        "the script would be larger than {} KB ({bytes} bytes so far): nothing was added; make it shorter",
        MAX_DRAFT_BYTES / 1000
    )
}

impl Drafts {
    fn find(&mut self, kind: ScriptKind, id: &str) -> Result<usize, String> {
        let i = self.list.iter().position(|d| d.id == id).ok_or_else(|| {
            "there is no such draft (drafts end when they are saved); start again without draft".to_string()
        })?;
        if self.list[i].kind != kind {
            return Err(format!("that draft is a {} draft", kind_name(self.list[i].kind)));
        }
        Ok(i)
    }

    /// Appends `part` to draft `draft` (a new draft when `None`).
    pub fn append(&mut self, kind: ScriptKind, draft: Option<&str>, part: &str) -> Result<Progress, String> {
        let i = match draft {
            Some(id) => self.find(kind, id)?,
            None => {
                if part.len() > MAX_DRAFT_BYTES {
                    return Err(too_big(0));
                }
                if self.list.len() >= MAX_DRAFTS {
                    self.list.remove(0);
                }
                self.next += 1;
                self.list.push(Draft { id: format!("draft-{}", self.next), kind, source: String::new(), parts: 0 });
                self.list.len() - 1
            }
        };
        let d = &mut self.list[i];
        if d.source.len() + part.len() > MAX_DRAFT_BYTES {
            return Err(too_big(d.source.len()));
        }
        d.source.push_str(part);
        d.parts += 1;
        Ok(Progress { draft: d.id.clone(), parts: d.parts, bytes: d.source.len() })
    }

    /// Adds the last part and takes the whole source out (the draft ends). Too large: the draft
    /// stays as it was.
    pub fn finish(&mut self, kind: ScriptKind, draft: &str, part: &str) -> Result<String, String> {
        let i = self.find(kind, draft)?;
        if self.list[i].source.len() + part.len() > MAX_DRAFT_BYTES {
            return Err(too_big(self.list[i].source.len()));
        }
        let mut d = self.list.remove(i);
        d.source.push_str(part);
        Ok(d.source)
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_join_in_order() {
        let mut d = Drafts::default();
        let p = d.append(ScriptKind::Userscript, None, "a").unwrap();
        assert_eq!(p, Progress { draft: "draft-1".into(), parts: 1, bytes: 1 });
        let p = d.append(ScriptKind::Userscript, Some("draft-1"), "bb").unwrap();
        assert_eq!((p.parts, p.bytes), (2, 3));
        assert_eq!(d.finish(ScriptKind::Userscript, "draft-1", "c").unwrap(), "abbc");
        assert!(d.is_empty());
        assert!(d.finish(ScriptKind::Userscript, "draft-1", "c").unwrap_err().contains("no such draft"));
    }

    #[test]
    fn kinds_and_ids_are_checked() {
        let mut d = Drafts::default();
        d.append(ScriptKind::Macro, None, "x").unwrap();
        assert!(d.append(ScriptKind::Userscript, Some("draft-1"), "y").unwrap_err().contains("macro draft"));
        assert!(d.finish(ScriptKind::Userscript, "draft-1", "y").is_err());
        assert!(d.append(ScriptKind::Macro, Some("draft-9"), "y").is_err());
        assert_eq!(d.finish(ScriptKind::Macro, "draft-1", "z").unwrap(), "xz");
    }

    #[test]
    fn size_cap() {
        let mut d = Drafts::default();
        let big = "x".repeat(MAX_DRAFT_BYTES - 10);
        d.append(ScriptKind::Userscript, None, &big).unwrap();
        // Too much: refused, nothing added.
        let e = d.append(ScriptKind::Userscript, Some("draft-1"), &"y".repeat(11)).unwrap_err();
        assert!(e.contains("200 KB") && e.contains("nothing was added"), "{e}");
        assert!(d.finish(ScriptKind::Userscript, "draft-1", &"y".repeat(11)).is_err());
        assert_eq!(d.len(), 1, "the draft stays after a refused last part");
        let p = d.append(ScriptKind::Userscript, Some("draft-1"), &"y".repeat(10)).unwrap();
        assert_eq!(p.bytes, MAX_DRAFT_BYTES);
        assert_eq!(d.finish(ScriptKind::Userscript, "draft-1", "").unwrap().len(), MAX_DRAFT_BYTES);
        assert!(d.append(ScriptKind::Userscript, None, &"z".repeat(MAX_DRAFT_BYTES + 1)).is_err());
        assert!(d.is_empty());
    }

    #[test]
    fn oldest_draft_goes() {
        let mut d = Drafts::default();
        for _ in 0..=MAX_DRAFTS {
            d.append(ScriptKind::Userscript, None, "a").unwrap();
        }
        assert_eq!(d.len(), MAX_DRAFTS);
        assert!(d.append(ScriptKind::Userscript, Some("draft-1"), "b").is_err());
        assert!(d.append(ScriptKind::Userscript, Some("draft-2"), "b").is_ok());
        d.clear();
        assert!(d.is_empty());
    }
}
