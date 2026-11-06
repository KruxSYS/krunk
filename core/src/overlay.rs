//! The native overlays' pure parts: the link-status label at the bottom of the page and the
//! "Press Esc" hint pill. The host draws them into layered, click-through windows (the
//! app's `ffi::overlay`); here are the colours, the placement, the text elision and the
//! fades, so they can be tested without Windows.

use crate::layout::Rect;

/// A colour with alpha (straight, not premultiplied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

/// Colours of an overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgba,
    pub fg: Rgba,
    pub border: Rgba,
    pub keycap_bg: Rgba,
    pub keycap_border: Rgba,
}

/// The status label follows the theme (Chrome's: a light grey box, or a dark one).
pub fn status_palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: Rgba(41, 42, 45, 255),
            fg: Rgba(232, 234, 237, 255),
            border: Rgba(94, 94, 94, 255),
            keycap_bg: Rgba(60, 60, 64, 255),
            keycap_border: Rgba(110, 110, 110, 255),
        }
    } else {
        Palette {
            bg: Rgba(241, 243, 244, 255),
            fg: Rgba(32, 33, 36, 255),
            border: Rgba(199, 199, 199, 255),
            keycap_bg: Rgba(255, 255, 255, 255),
            keycap_border: Rgba(160, 160, 160, 255),
        }
    }
}

/// The hint pill is dark and semi-transparent in both themes (it floats over a page).
pub fn hint_palette() -> Palette {
    Palette {
        bg: Rgba(32, 33, 36, 217),
        fg: Rgba(255, 255, 255, 255),
        border: Rgba(32, 33, 36, 0),
        keycap_bg: Rgba(255, 255, 255, 38),
        keycap_border: Rgba(255, 255, 255, 120),
    }
}

/// Shortens `text` to fit `max_width`, keeping its start and end and putting "…" in the
/// middle (an address keeps its host and its file name). `measure` gives the width of a
/// string in the font used. Never returns an empty string for a non-empty text.
pub fn elide_middle(text: &str, max_width: f64, measure: impl Fn(&str) -> f64) -> String {
    if measure(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    // Binary search on how many characters survive (head gets 3/5, tail 2/5).
    let build = |keep: usize| -> String {
        let head = keep * 3 / 5;
        let tail = keep - head;
        let mut s: String = chars[..head].iter().collect();
        s.push('…');
        s.extend(chars[n - tail..].iter());
        s
    };
    let (mut lo, mut hi) = (0usize, n.saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if measure(&build(mid)) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo == 0 {
        // Not even one character fits with the ellipsis: show the first one anyway.
        return chars.first().map(|c| c.to_string()).unwrap_or_default();
    }
    build(lo)
}

/// Where the status label goes: the bottom-left corner of the page area, or the bottom-right
/// corner while the cursor is within `margin` of the label's left-side place (so the label
/// never sits under the mouse). Returns the rect and whether it is on the right.
pub fn status_rect(page: Rect, width: i32, height: i32, cursor: Option<(i32, i32)>, margin: i32) -> (Rect, bool) {
    let width = width.min(page.width.max(1));
    let left = Rect { x: page.x, y: page.y + page.height - height, width, height };
    let near = cursor.is_some_and(|(cx, cy)| {
        cx >= left.x - margin && cx < left.x + left.width + margin && cy >= left.y - margin && cy < left.y + left.height
    });
    if near {
        (Rect { x: page.x + page.width - width, ..left }, true)
    } else {
        (left, false)
    }
}

/// Where the hint pill goes: centred horizontally in `area`, `top_gap` below its top.
pub fn hint_rect(area: Rect, width: i32, height: i32, top_gap: i32) -> Rect {
    Rect { x: area.x + (area.width - width) / 2, y: area.y + top_gap, width, height }
}

/// Per-pixel coverage (0 = outside, 255 = inside) of a rounded rectangle of `width` x
/// `height` with corner `radius`, anti-aliased on the edge. Row-major, top-down.
pub fn rounded_mask(width: i32, height: i32, radius: f64) -> Vec<u8> {
    let (w, h) = (width.max(0) as usize, height.max(0) as usize);
    let r = radius.max(0.0).min(f64::from(width.max(1)) / 2.0).min(f64::from(height.max(1)) / 2.0);
    let mut mask = vec![0u8; w * h];
    let (fw, fh) = (f64::from(width), f64::from(height));
    for y in 0..h {
        for x in 0..w {
            // Signed distance from the pixel centre to the rounded rectangle's edge.
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let qx = (px - fw / 2.0).abs() - (fw / 2.0 - r);
            let qy = (py - fh / 2.0).abs() - (fh / 2.0 - r);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
            let inside = qx.max(qy).min(0.0);
            let dist = outside + inside - r;
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            mask[y * w + x] = (coverage * 255.0).round() as u8;
        }
    }
    mask
}

/// A pixel of a 32-bit premultiplied BGRA bitmap: `colour` under `coverage` (the mask) and
/// the layer's own `alpha`.
pub fn premultiplied(colour: (u8, u8, u8), coverage: u8, alpha: u8) -> [u8; 4] {
    let a = u32::from(coverage) * u32::from(alpha) / 255;
    let mul = |c: u8| (u32::from(c) * a / 255) as u8;
    [mul(colour.2), mul(colour.1), mul(colour.0), a as u8]
}

/// A piece of the hint: plain text or a key cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Run {
    Text(String),
    Key(String),
}

/// Splits "Press [Esc] to show your cursor" into text runs and key caps.
pub fn hint_runs(text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        if let Some(close) = rest[open..].find(']') {
            if open > 0 {
                runs.push(Run::Text(rest[..open].to_string()));
            }
            runs.push(Run::Key(rest[open + 1..open + close].to_string()));
            rest = &rest[open + close + 1..];
        } else {
            break;
        }
    }
    if !rest.is_empty() {
        runs.push(Run::Text(rest.to_string()));
    }
    runs
}

/// The hint's opacity `elapsed_ms` after it appeared: a fade in over `fade_ms`, full
/// opacity, then a fade out in the last `fade_ms` of `total_ms`. `None` once it is over.
/// With `fade_ms` 0 (reduced motion) it is fully opaque for `total_ms` and then gone.
pub fn fade_alpha(elapsed_ms: u32, total_ms: u32, fade_ms: u32) -> Option<u8> {
    if elapsed_ms >= total_ms {
        return None;
    }
    if fade_ms == 0 {
        return Some(255);
    }
    let f = f64::from(fade_ms);
    let t = f64::from(elapsed_ms);
    let level = if t < f {
        t / f
    } else if t > f64::from(total_ms) - f {
        (f64::from(total_ms) - t) / f
    } else {
        1.0
    };
    Some((level.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(s: &str) -> f64 {
        s.chars().count() as f64 * 7.0
    }

    #[test]
    fn elision_keeps_both_ends() {
        let url = "https://www.example.com/a/very/long/path/that/goes/on/and/on/index.html";
        assert_eq!(elide_middle(url, 10_000.0, width), url);
        let short = elide_middle(url, 30.0 * 7.0, width);
        assert!(short.chars().count() <= 30 && short.contains('…'), "{short}");
        assert!(short.starts_with("https://www.exam") && short.ends_with("index.html"), "{short}");
        assert_eq!(elide_middle("abc", 0.0, width), "a", "never empty");
        assert_eq!(elide_middle("", 10.0, width), "");
        // Multi-byte characters are cut on character boundaries.
        let s = elide_middle("ÄÖÜ€€€€€€€€€€€€€€ÄÖÜ", 8.0 * 7.0, width);
        assert!(s.chars().count() <= 8 && s.contains('…'), "{s}");
    }

    #[test]
    fn status_label_moves_away_from_the_mouse() {
        let page = Rect { x: 0, y: 100, width: 1000, height: 600 };
        let (r, right) = status_rect(page, 300, 22, None, 20);
        assert_eq!((r.x, r.y, r.width, r.height, right), (0, 678, 300, 22, false));
        // The cursor far away: still left.
        assert!(!status_rect(page, 300, 22, Some((800, 300)), 20).1);
        // Near the label (within the margin above it): right side.
        let (r, right) = status_rect(page, 300, 22, Some((150, 665)), 20);
        assert_eq!((r.x, right), (700, true));
        // Just outside the margin: left.
        assert!(!status_rect(page, 300, 22, Some((150, 650)), 20).1);
        // Wider than the page: clamped.
        assert_eq!(status_rect(page, 5000, 22, None, 20).0.width, 1000);
        assert_eq!(hint_rect(page, 200, 40, 24), Rect { x: 400, y: 124, width: 200, height: 40 });
    }

    #[test]
    fn rounded_mask_is_inside_and_soft_at_corners() {
        let m = rounded_mask(20, 10, 4.0);
        assert_eq!(m.len(), 200);
        assert_eq!(m[5 * 20 + 10], 255, "centre is inside");
        assert_eq!(m[0], 0, "the corner pixel is outside");
        assert!(m[1] > 0 && m[1] < 255 || m[20] > 0, "the corner edge is anti-aliased");
        assert_eq!(m[5 * 20], 255, "the straight edge is inside");
        assert!(rounded_mask(0, 0, 3.0).is_empty());
        assert_eq!(rounded_mask(4, 4, 100.0).len(), 16, "a radius larger than the box is clamped");
    }

    #[test]
    fn premultiplied_pixels() {
        assert_eq!(premultiplied((255, 128, 0), 255, 255), [0, 128, 255, 255]);
        assert_eq!(premultiplied((255, 255, 255), 255, 128), [128, 128, 128, 128]);
        assert_eq!(premultiplied((255, 255, 255), 0, 255), [0, 0, 0, 0]);
    }

    #[test]
    fn hint_runs_and_fade() {
        assert_eq!(
            hint_runs("Press [Esc] to show your cursor"),
            vec![Run::Text("Press ".into()), Run::Key("Esc".into()), Run::Text(" to show your cursor".into())]
        );
        assert_eq!(hint_runs("plain"), vec![Run::Text("plain".into())]);
        assert_eq!(hint_runs("[F11]"), vec![Run::Key("F11".into())]);
        assert_eq!(fade_alpha(0, 3000, 150), Some(0));
        assert_eq!(fade_alpha(75, 3000, 150), Some(128));
        assert_eq!(fade_alpha(1500, 3000, 150), Some(255));
        assert_eq!(fade_alpha(2925, 3000, 150), Some(128));
        assert_eq!(fade_alpha(3000, 3000, 150), None);
        assert_eq!(fade_alpha(10, 3000, 0), Some(255), "reduced motion: no fade");
        assert_eq!(fade_alpha(3001, 3000, 0), None);
    }
}
