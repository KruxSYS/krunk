//! Colours chosen in Settings → Appearance (Krunk 0.7.0, R3): the text drawn on them must stay
//! readable, so every custom colour gets the light or dark foreground with the better WCAG
//! contrast (relative luminance, contrast ratio as in WCAG 2.x). Both the shell and Krunk's
//! pages use the pairs computed here.

use serde::{Deserialize, Serialize};

/// The dark and light text Krunk draws on a custom colour.
pub const DARK_TEXT: &str = "#111827";
pub const LIGHT_TEXT: &str = "#ffffff";
/// WCAG AA for normal text.
pub const AA: f64 = 4.5;

/// `#rrggbb` (or `#rgb`) to its channels.
pub fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => h.to_string(),
        _ => return None,
    };
    if !full.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(&full, 16).ok()?;
    Some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// WCAG relative luminance, 0 (black) to 1 (white).
pub fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let lin = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio between two colours, 1 to 21.
pub fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// The text colour for `background`: dark or light, whichever contrasts more. `None` for an
/// empty or invalid colour (the theme's own text applies).
pub fn foreground(background: &str) -> Option<&'static str> {
    let bg = parse_hex(background)?;
    let dark = contrast(bg, parse_hex(DARK_TEXT)?);
    let light = contrast(bg, parse_hex(LIGHT_TEXT)?);
    Some(if dark >= light { DARK_TEXT } else { LIGHT_TEXT })
}

/// The custom colours and their text colours, for the shell and the pages (empty = the
/// theme's own).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThemeColors {
    pub accent: String,
    pub accent_fg: String,
    pub toolbar: String,
    pub toolbar_fg: String,
    pub strip: String,
    pub strip_fg: String,
    pub background: String,
    pub background_fg: String,
}

impl ThemeColors {
    pub fn from_appearance(a: &crate::settings::Appearance) -> ThemeColors {
        let pair = |c: &str| -> (String, String) {
            match foreground(c) {
                Some(fg) => (c.trim().to_ascii_lowercase(), fg.to_string()),
                None => (String::new(), String::new()),
            }
        };
        let (accent, accent_fg) = pair(&a.accent);
        let (toolbar, toolbar_fg) = pair(&a.toolbar_color);
        let (strip, strip_fg) = pair(&a.strip_color);
        let (background, background_fg) = pair(&a.background_color);
        ThemeColors { accent, accent_fg, toolbar, toolbar_fg, strip, strip_fg, background, background_fg }
    }

    pub fn is_empty(&self) -> bool {
        *self == ThemeColors::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#2563eb"), Some((0x25, 0x63, 0xeb)));
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex(" #ABCDEF "), Some((0xab, 0xcd, 0xef)));
        assert_eq!(parse_hex("2563eb"), None);
        assert_eq!(parse_hex("#12345g"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn wcag_values() {
        assert!((contrast((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast((119, 119, 119), (255, 255, 255)) - 4.48).abs() < 0.01);
        assert!((luminance((255, 255, 255)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_foreground_always_meets_aa_where_possible() {
        // Every preset accent and a sweep of greys and hues: the chosen text reaches AA, except
        // for the narrow mid-luminance band where neither black nor white can (then the better).
        let mut colours: Vec<String> = [
            "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#16a34a", "#0d9488", "#1f2023", "#f3f4f6",
            "#ffffff",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        for v in (0..=255).step_by(15) {
            colours.push(format!("#{v:02x}{v:02x}{v:02x}"));
            colours.push(format!("#{v:02x}{:02x}{:02x}", 255 - v, (v * 7) % 256));
        }
        for c in &colours {
            let bg = parse_hex(c).unwrap();
            let fg = parse_hex(foreground(c).unwrap()).unwrap();
            let other = parse_hex(if foreground(c) == Some(DARK_TEXT) { LIGHT_TEXT } else { DARK_TEXT }).unwrap();
            let ratio = contrast(bg, fg);
            assert!(ratio >= contrast(bg, other), "{c}: picked the worse text");
            assert!(ratio >= 4.0, "{c}: contrast {ratio:.2}");
        }
        assert_eq!(foreground("#ffffff"), Some(DARK_TEXT));
        assert_eq!(foreground("#1f2023"), Some(LIGHT_TEXT));
        assert_eq!(foreground("#2563eb"), Some(LIGHT_TEXT));
        assert_eq!(foreground(""), None);
    }

    #[test]
    fn colours_from_appearance() {
        let a = crate::settings::Appearance {
            accent: "#FFE066".into(),
            toolbar_color: "#0b1220".into(),
            strip_color: String::new(),
            background_color: "nope".into(),
            ..Default::default()
        };
        let c = ThemeColors::from_appearance(&a);
        assert_eq!((c.accent.as_str(), c.accent_fg.as_str()), ("#ffe066", DARK_TEXT));
        assert_eq!((c.toolbar.as_str(), c.toolbar_fg.as_str()), ("#0b1220", LIGHT_TEXT));
        assert!(c.strip.is_empty() && c.strip_fg.is_empty() && c.background.is_empty());
        assert!(ThemeColors::from_appearance(&Default::default()).is_empty());
    }
}
