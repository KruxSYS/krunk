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

/// Krunk's own accent (common.css, light theme). Empty gradient colours derive from the user's
/// accent, or from this one while the accent is the theme's (0.15.0; the same in light and dark,
/// so a Windows theme switch needs no new colours).
pub const DEFAULT_ACCENT: &str = "#2563eb";
/// How far an empty last gradient colour turns from the first one (HSL hue, degrees).
pub const GRADIENT_HUE_STEP: f64 = 40.0;
/// The inactive tabs of an ombre strip are veiled with black at this opacity (shell.css
/// `--ombre-veil` uses the same value), so the active tab shows the gradient at full strength.
pub const OMBRE_VEIL: f64 = 0.22;

/// `#rrggbb` for the channels.
pub fn to_hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `a` moved towards `b` by `t` (0 = `a`, 1 = `b`), per sRGB channel like CSS `color-mix`.
pub fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f64) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

/// `c` with its hue turned by `degrees` (HSL; saturation and lightness stay).
pub fn rotate_hue((r, g, b): (u8, u8, u8), degrees: f64) -> (u8, u8, u8) {
    let (r, g, b) = (f64::from(r) / 255.0, f64::from(g) / 255.0, f64::from(b) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        let v = (l * 255.0).round() as u8;
        return (v, v, v);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = (h + degrees).rem_euclid(360.0);
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let ch = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (ch(r1), ch(g1), ch(b1))
}

/// The text colour for a surface made of several colours (a gradient): dark or light, whichever
/// has the better worst-case contrast over all of them. `None` without colours.
pub fn foreground_on_all(colours: &[(u8, u8, u8)]) -> Option<&'static str> {
    let worst = |text: &str| {
        let t = parse_hex(text).unwrap_or_default();
        colours.iter().map(|c| contrast(*c, t)).fold(f64::INFINITY, f64::min)
    };
    if colours.is_empty() {
        return None;
    }
    Some(if worst(DARK_TEXT) >= worst(LIGHT_TEXT) { DARK_TEXT } else { LIGHT_TEXT })
}

/// The gradient of the gradient and ombre styles (0.15.0): two or three `#rrggbb` stops. Empty
/// colours derive: the first from the accent (or Krunk's default), the last from the first with
/// its hue turned by [`GRADIENT_HUE_STEP`]; an empty middle colour means two stops.
pub fn gradient_stops(a: &crate::settings::Appearance) -> Vec<String> {
    let from = parse_hex(&a.gradient_from)
        .or_else(|| parse_hex(&a.accent))
        .or_else(|| parse_hex(DEFAULT_ACCENT))
        .unwrap_or_default();
    let to = parse_hex(&a.gradient_to).unwrap_or_else(|| rotate_hue(from, GRADIENT_HUE_STEP));
    let mut stops = vec![to_hex(from)];
    if let Some(via) = parse_hex(&a.gradient_via) {
        stops.push(to_hex(via));
    }
    stops.push(to_hex(to));
    stops
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
    /// 0.15.0, only while a bar uses the gradient or ombre style (boxed: `Event` stays small).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient: Option<Box<GradientColors>>,
}

/// The gradient of the gradient and ombre styles (0.15.0) with the text drawn on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GradientColors {
    /// [`gradient_stops`]
    pub stops: Vec<String>,
    /// The text on the gradient.
    pub fg: String,
    /// The text on the veiled (inactive) tabs of an ombre strip.
    pub ombre_fg: String,
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
        use crate::settings::BarStyle::{Gradient, Ombre};
        let gradient =
            (matches!(a.tab_style, Gradient | Ombre) || matches!(a.toolbar_style, Gradient | Ombre)).then(|| {
                let stops: Vec<(u8, u8, u8)> = gradient_stops(a).iter().filter_map(|s| parse_hex(s)).collect();
                // An inactive ombre tab: the veil, lighter while hovered (half the veil).
                let veiled: Vec<(u8, u8, u8)> = stops
                    .iter()
                    .flat_map(|s| [mix(*s, (0, 0, 0), OMBRE_VEIL), mix(*s, (0, 0, 0), OMBRE_VEIL / 2.0)])
                    .collect();
                Box::new(GradientColors {
                    stops: stops.iter().map(|s| to_hex(*s)).collect(),
                    fg: foreground_on_all(&stops).unwrap_or(DARK_TEXT).to_string(),
                    ombre_fg: foreground_on_all(&veiled).unwrap_or(DARK_TEXT).to_string(),
                })
            });
        ThemeColors { accent, accent_fg, toolbar, toolbar_fg, strip, strip_fg, background, background_fg, gradient }
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

    #[test]
    fn mixing_and_hue_rotation() {
        assert_eq!(mix((0, 0, 0), (255, 255, 255), 0.5), (128, 128, 128));
        assert_eq!(mix((10, 20, 30), (0, 0, 0), 0.0), (10, 20, 30));
        assert_eq!(rotate_hue((255, 0, 0), 120.0), (0, 255, 0));
        assert_eq!(rotate_hue((255, 0, 0), -120.0), (0, 0, 255));
        assert_eq!(rotate_hue((128, 128, 128), 90.0), (128, 128, 128));
        // Krunk's blue turns violet (near the Settings placeholder #7c3aed).
        let (r, g, b) = rotate_hue(parse_hex(DEFAULT_ACCENT).unwrap(), GRADIENT_HUE_STEP);
        assert!(r > g && b > r, "{}", to_hex((r, g, b)));
        assert_eq!(to_hex((1, 0xab, 255)), "#01abff");
    }

    #[test]
    fn gradient_stops_derive_empty_colours() {
        let mut a = crate::settings::Appearance::default();
        let violet = rotate_hue(parse_hex(DEFAULT_ACCENT).unwrap(), GRADIENT_HUE_STEP);
        assert_eq!(gradient_stops(&a), vec![DEFAULT_ACCENT.to_string(), to_hex(violet)]);
        a.accent = "#DB2777".into();
        assert_eq!(gradient_stops(&a)[0], "#db2777");
        a.gradient_from = "#f97316".into();
        a.gradient_via = "#ec4899".into();
        a.gradient_to = "#8b5cf6".into();
        assert_eq!(gradient_stops(&a), vec!["#f97316", "#ec4899", "#8b5cf6"]);
        a.gradient_via.clear();
        a.gradient_from = "bad".into();
        assert_eq!(gradient_stops(&a), vec!["#db2777", "#8b5cf6"]);
    }

    #[test]
    fn text_on_a_gradient_is_the_better_one_everywhere() {
        use crate::settings::{Appearance, BarStyle};
        assert_eq!(foreground_on_all(&[]), None);
        assert_eq!(foreground_on_all(&[(255, 255, 255), (250, 204, 21)]), Some(DARK_TEXT));
        assert_eq!(foreground_on_all(&[(17, 24, 39), (37, 99, 235)]), Some(LIGHT_TEXT));
        // Solid and glass styles carry no gradient (the default stays empty on the wire).
        let mut a = Appearance::default();
        assert!(ThemeColors::from_appearance(&a).gradient.is_none());
        a.toolbar_style = BarStyle::Glass;
        a.tab_style = BarStyle::Glass;
        assert!(ThemeColors::from_appearance(&a).is_empty());
        // Every preset and a sweep of accents: the chosen text is the better of the two over the
        // whole gradient, and stays readable (3:1 at the worst stop) on the gradient and on the
        // veiled ombre tabs.
        let mut looks: Vec<Appearance> = Vec::new();
        for p in crate::style::PRESETS {
            let mut a = Appearance::default();
            p.apply(&mut a);
            looks.push(a);
        }
        for accent in [
            "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#16a34a", "#0d9488", "#facc15", "#111827",
            "#f3f4f6",
        ] {
            looks.push(Appearance {
                accent: accent.into(),
                tab_style: BarStyle::Ombre,
                toolbar_style: BarStyle::Gradient,
                ..Default::default()
            });
        }
        let worst = |fg: &str, cs: &[(u8, u8, u8)]| {
            let f = parse_hex(fg).unwrap();
            cs.iter().map(|s| contrast(*s, f)).fold(f64::INFINITY, f64::min)
        };
        let other = |fg: &str| if fg == DARK_TEXT { LIGHT_TEXT } else { DARK_TEXT };
        let mut with_gradient = 0;
        for a in &looks {
            let c = ThemeColors::from_appearance(a);
            let styled =
                [a.tab_style, a.toolbar_style].iter().any(|s| matches!(s, BarStyle::Gradient | BarStyle::Ombre));
            assert_eq!(c.gradient.is_some(), styled, "{}", a.preset);
            let Some(g) = c.gradient else { continue };
            with_gradient += 1;
            let stops: Vec<_> = g.stops.iter().map(|s| parse_hex(s).unwrap()).collect();
            assert!((2..=3).contains(&stops.len()));
            let w = worst(&g.fg, &stops);
            assert!(w >= worst(other(&g.fg), &stops), "{:?}: the worse text", g.stops);
            assert!(w >= 3.0, "{:?}: {w:.2}", g.stops);
            let veiled: Vec<_> = stops.iter().map(|s| mix(*s, (0, 0, 0), OMBRE_VEIL)).collect();
            assert!(worst(&g.ombre_fg, &veiled) >= 3.0, "{:?}: ombre text", g.stops);
        }
        // On the wire: `colors.gradient {stops, fg, ombreFg}`, absent without a gradient style.
        let json = serde_json::to_value(ThemeColors::from_appearance(&looks[1])).unwrap();
        assert_eq!(json["gradient"]["stops"], serde_json::json!(["#f97316", "#ec4899", "#8b5cf6"]));
        assert!(json["gradient"]["fg"].is_string() && json["gradient"]["ombreFg"].is_string());
        assert!(serde_json::to_value(ThemeColors::default()).unwrap().get("gradient").is_none());
        assert_eq!(with_gradient, 12);
    }
}
