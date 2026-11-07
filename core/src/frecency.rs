//! "Frecency" ranking for the new-tab page: visit count weighted by how
//! recent the visits are (the Firefox approach, simplified).
//!
//! Score of a URL = `visit_count × mean(bucket weight of its last 10 visits)`.
//! Tiles are one per host (the best-scoring URL of that host), pinned tiles
//! come first in pin order, and tiles the user removed are hidden.

use crate::storage::{DbResult, Store};
use crate::urlclean::host_of;
use rusqlite::params;
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::HashMap;

const DAY_MS: i64 = 86_400_000;
/// Visits sampled per URL.
pub const SAMPLE_VISITS: u32 = 10;

/// Weight of a visit by its age.
pub fn age_weight(age_ms: i64) -> f64 {
    let days = age_ms.max(0) / DAY_MS;
    match days {
        0..=3 => 100.0,
        4..=14 => 70.0,
        15..=31 => 50.0,
        32..=90 => 30.0,
        _ => 10.0,
    }
}

/// Frecency score of a URL.
pub fn score(visit_count: i64, recent_visits: &[i64], now: i64) -> f64 {
    if visit_count <= 0 || recent_visits.is_empty() {
        return 0.0;
    }
    let mean = recent_visits.iter().map(|&ts| age_weight(now - ts)).sum::<f64>() / recent_visits.len() as f64;
    visit_count as f64 * mean
}

/// Input to [`rank`]: one URL from history.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub url: String,
    pub title: String,
    pub visit_count: i64,
    pub recent_visits: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub url: String,
    pub title: String,
    pub pinned: bool,
    /// Same-origin URL of the host's cached favicon, if known.
    pub favicon: Option<String>,
    #[serde(skip)]
    pub score: f64,
}

/// Ranks candidates into at most `n` tiles, one per host, best first.
pub fn rank(candidates: &[Candidate], now: i64, n: usize) -> Vec<Tile> {
    let mut best: HashMap<String, Tile> = HashMap::new();
    for c in candidates {
        let s = score(c.visit_count, &c.recent_visits, now);
        if s <= 0.0 {
            continue;
        }
        let host = host_of(&c.url).unwrap_or_else(|| c.url.clone());
        let host = host.strip_prefix("www.").map(str::to_string).unwrap_or(host);
        let replace = best.get(&host).is_none_or(|t| s > t.score);
        if replace {
            let title = if c.title.is_empty() { host.clone() } else { c.title.clone() };
            best.insert(host, Tile { url: c.url.clone(), title, pinned: false, favicon: None, score: s });
        }
    }
    let mut tiles: Vec<Tile> = best.into_values().collect();
    tiles.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.url.cmp(&b.url)));
    tiles.truncate(n);
    tiles
}

const TILE_PINNED: i64 = 1;
const TILE_HIDDEN: i64 = 2;

impl Store {
    /// The new-tab grid: pinned tiles, then the top frecency sites.
    pub fn top_sites(&self, n: usize, now: i64) -> DbResult<Vec<Tile>> {
        let mut pinned: Vec<Tile> = {
            let mut stmt =
                self.conn.prepare_cached("SELECT url, title FROM tiles WHERE state = ?1 ORDER BY position")?;
            let rows = stmt.query_map([TILE_PINNED], |r| {
                Ok(Tile { url: r.get(0)?, title: r.get(1)?, pinned: true, favicon: None, score: f64::INFINITY })
            })?;
            rows.collect::<DbResult<_>>()?
        };
        let excluded: Vec<String> = {
            let mut stmt = self.conn.prepare_cached("SELECT url FROM tiles")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<DbResult<_>>()?
        };
        // Candidate pool: URLs visited in the last 90 days plus the most
        // visited ones overall, bounded so this stays fast.
        let (ids, mut candidates): (Vec<i64>, Vec<Candidate>) = {
            let mut stmt = self.conn.prepare_cached(
                "SELECT id, url, title, visit_count FROM urls
                 WHERE visit_count > 0
                 ORDER BY (last_visit > ?1) DESC, visit_count DESC, last_visit DESC LIMIT 400",
            )?;
            let rows = stmt.query_map(params![now - 90 * DAY_MS], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    Candidate { url: r.get(1)?, title: r.get(2)?, visit_count: r.get(3)?, recent_visits: Vec::new() },
                ))
            })?;
            rows.collect::<DbResult<Vec<_>>>()?.into_iter().unzip()
        };
        for (c, visits) in candidates.iter_mut().zip(self.recent_visits(&ids, SAMPLE_VISITS)?) {
            c.recent_visits = visits;
        }
        let pinned_hosts: Vec<Option<String>> = pinned.iter().map(|t| host_of(&t.url)).collect();
        candidates.retain(|c| {
            !excluded.contains(&c.url) && (host_of(&c.url).is_none() || !pinned_hosts.contains(&host_of(&c.url)))
        });
        let room = n.saturating_sub(pinned.len());
        pinned.truncate(n);
        pinned.extend(rank(&candidates, now, room));
        for t in &mut pinned {
            if let Some(h) = host_of(&t.url) {
                t.favicon = self.favicon_url(&h)?;
            }
        }
        Ok(pinned)
    }

    /// Pins a tile at the end of the pinned list.
    pub fn pin_site(&self, url: &str, title: &str) -> DbResult<()> {
        let pos: i64 = self.conn.query_row(
            "SELECT coalesce(max(position), -1) + 1 FROM tiles WHERE state = ?1",
            [TILE_PINNED],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO tiles(url, title, state, position) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(url) DO UPDATE SET title = excluded.title, state = excluded.state, position = excluded.position",
            params![url, title, TILE_PINNED, pos],
        )?;
        Ok(())
    }

    pub fn unpin_site(&self, url: &str) -> DbResult<()> {
        self.conn.execute("DELETE FROM tiles WHERE url = ?1 AND state = ?2", params![url, TILE_PINNED])?;
        Ok(())
    }

    /// The pinned tiles only (the "own list" mode of the new tab page).
    pub fn pinned_tiles(&self, n: usize) -> DbResult<Vec<Tile>> {
        let mut stmt = self.conn.prepare_cached("SELECT url, title FROM tiles WHERE state = ?1 ORDER BY position")?;
        let rows = stmt.query_map([TILE_PINNED], |r| {
            Ok(Tile { url: r.get(0)?, title: r.get(1)?, pinned: true, favicon: None, score: f64::INFINITY })
        })?;
        let mut tiles: Vec<Tile> = rows.collect::<DbResult<_>>()?;
        tiles.truncate(n);
        for t in &mut tiles {
            if let Some(h) = host_of(&t.url) {
                t.favicon = self.favicon_url(&h)?;
            }
        }
        Ok(tiles)
    }

    /// The pinned tiles in this order (URLs not pinned are ignored; a URL that is a
    /// most-visited tile is pinned by the reorder, keeping its title from history).
    pub fn reorder_tiles(&self, urls: &[String]) -> DbResult<()> {
        for (i, url) in urls.iter().enumerate() {
            let title: String = self
                .conn
                .query_row("SELECT title FROM urls WHERE url = ?1", [url], |r| r.get(0))
                .optional()?
                .unwrap_or_default();
            self.conn.execute(
                "INSERT INTO tiles(url, title, state, position) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(url) DO UPDATE SET state = excluded.state, position = excluded.position,
                 title = CASE WHEN tiles.title = '' THEN excluded.title ELSE tiles.title END",
                params![url, title, TILE_PINNED, i as i64],
            )?;
        }
        Ok(())
    }

    /// Changes a pinned tile's address and title (its position stays).
    pub fn edit_tile(&self, url: &str, new_url: &str, title: &str) -> DbResult<bool> {
        if url != new_url {
            self.conn.execute("DELETE FROM tiles WHERE url = ?1", [new_url])?;
        }
        let n = self.conn.execute(
            "UPDATE tiles SET url = ?2, title = ?3 WHERE url = ?1 AND state = ?4",
            params![url, new_url, title, TILE_PINNED],
        )?;
        Ok(n > 0)
    }

    /// Removes a tile from the grid (history itself is kept).
    pub fn hide_site(&self, url: &str) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO tiles(url, title, state) VALUES (?1, '', ?2)
             ON CONFLICT(url) DO UPDATE SET state = excluded.state",
            params![url, TILE_HIDDEN],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_list_reorder_and_edit() {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://a.example/", "A site", 1_700_000_000_000).unwrap();
        s.pin_site("https://p.example/", "P").unwrap();
        s.pin_site("https://q.example/", "Q").unwrap();
        // A most-visited tile dragged into the order gets pinned with its history title.
        s.reorder_tiles(&["https://q.example/".into(), "https://a.example/".into(), "https://p.example/".into()])
            .unwrap();
        let urls: Vec<String> = s.pinned_tiles(10).unwrap().into_iter().map(|t| t.url).collect();
        assert_eq!(urls, vec!["https://q.example/", "https://a.example/", "https://p.example/"]);
        assert_eq!(s.pinned_tiles(10).unwrap()[1].title, "A site");
        assert_eq!(s.pinned_tiles(2).unwrap().len(), 2);
        assert!(s.edit_tile("https://p.example/", "https://p2.example/", "P2").unwrap());
        let t = s.pinned_tiles(10).unwrap();
        assert_eq!((t[2].url.as_str(), t[2].title.as_str()), ("https://p2.example/", "P2"));
        assert!(!s.edit_tile("https://nope.example/", "https://x.example/", "X").unwrap());
        // The own list ignores history: top_sites would add A's neighbours, pinned_tiles does not.
        assert!(s.pinned_tiles(10).unwrap().iter().all(|t| t.pinned));
    }

    const NOW: i64 = 1_000 * DAY_MS;

    fn c(url: &str, count: i64, ages_days: &[i64]) -> Candidate {
        Candidate {
            url: url.into(),
            title: String::new(),
            visit_count: count,
            recent_visits: ages_days.iter().map(|d| NOW - d * DAY_MS).collect(),
        }
    }

    #[test]
    fn weights_decay_with_age() {
        assert!(age_weight(0) > age_weight(10 * DAY_MS));
        assert!(age_weight(10 * DAY_MS) > age_weight(20 * DAY_MS));
        assert!(age_weight(20 * DAY_MS) > age_weight(60 * DAY_MS));
        assert!(age_weight(60 * DAY_MS) > age_weight(365 * DAY_MS));
        assert_eq!(score(0, &[NOW], NOW), 0.0);
    }

    #[test]
    fn recency_beats_raw_count() {
        // 30 visits a year ago vs 5 visits today: 30*10=300 < 5*100=500.
        let old = c("https://old.example/", 30, &[365; 10]);
        let fresh = c("https://fresh.example/", 5, &[0, 0, 1, 1, 2]);
        let tiles = rank(&[old, fresh], NOW, 10);
        assert_eq!(tiles[0].url, "https://fresh.example/");
        assert_eq!(tiles[1].url, "https://old.example/");
    }

    #[test]
    fn count_breaks_ties_at_equal_recency() {
        let a = c("https://a.example/", 3, &[1, 1, 1]);
        let b = c("https://b.example/", 9, &[1, 1, 1]);
        let tiles = rank(&[a, b], NOW, 10);
        assert_eq!(
            tiles.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(),
            ["https://b.example/", "https://a.example/"]
        );
    }

    #[test]
    fn one_tile_per_host_and_limit() {
        let cands = vec![
            c("https://news.example/a", 2, &[0, 0]),
            c("https://news.example/", 8, &[0, 0, 0]),
            c("https://www.news.example/b", 1, &[0]),
            c("https://x.example/", 1, &[0]),
            c("https://y.example/", 1, &[400]),
        ];
        let tiles = rank(&cands, NOW, 2);
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].url, "https://news.example/");
        assert_eq!(tiles[1].url, "https://x.example/");
        assert_eq!(tiles[0].title, "news.example");
    }

    #[test]
    fn store_top_sites_with_pins_and_hidden() {
        let s = Store::open_in_memory().unwrap();
        for (url, n, age) in [
            ("https://a.example/", 10, 1),
            ("https://b.example/", 5, 1),
            ("https://c.example/", 2, 1),
            ("https://d.example/", 1, 200),
        ] {
            for i in 0..n {
                s.record_visit(url, &url[8..9].to_uppercase(), NOW - age * DAY_MS - i).unwrap();
            }
        }
        let urls = |t: Vec<Tile>| t.into_iter().map(|t| t.url).collect::<Vec<_>>();
        assert_eq!(
            urls(s.top_sites(8, NOW).unwrap()),
            ["https://a.example/", "https://b.example/", "https://c.example/", "https://d.example/"]
        );
        s.pin_site("https://d.example/", "D").unwrap();
        s.hide_site("https://b.example/").unwrap();
        s.set_favicon("a.example", b"\x89PNG\r\n\x1a\n", 1).unwrap();
        let tiles = s.top_sites(3, NOW).unwrap();
        assert!(tiles[0].pinned);
        assert_eq!(urls(tiles.clone()), ["https://d.example/", "https://a.example/", "https://c.example/"]);
        assert_eq!(tiles[1].favicon.as_deref(), Some("/favicon?host=a.example"));
        assert_eq!(tiles[2].favicon, None);
        s.unpin_site("https://d.example/").unwrap();
        assert_eq!(
            urls(s.top_sites(8, NOW).unwrap()),
            ["https://a.example/", "https://c.example/", "https://d.example/"]
        );
    }
}
