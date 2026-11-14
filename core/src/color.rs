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
    /// 0.16.0, for the shell only ([`ThemeColors::for_shell`]): the private tabs' title colours.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private: Option<Box<PrivateColors>>,
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

/// A theme's own colours that can lie behind a private tab's title (`ui/shared/common.css`; a
/// test keeps the two in step).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeBase {
    /// `--bg`: the window and the tab strip.
    pub bg: &'static str,
    /// `--bg-raised`: the ghost of a tab dragged in from another window.
    pub raised: &'static str,
    /// `--bg-hover`: a hovered tab on a solid or ombre strip.
    pub hover: &'static str,
    /// `--accent`: the glass style's backdrop.
    pub accent: &'static str,
    /// `--private-bg`: the active private tab.
    pub private_bg: &'static str,
}

pub const LIGHT_THEME: ThemeBase =
    ThemeBase { bg: "#f3f4f6", raised: "#ffffff", hover: "#e5e7eb", accent: "#2563eb", private_bg: "#ede9fe" };
pub const DARK_THEME: ThemeBase =
    ThemeBase { bg: "#1f2023", raised: "#2b2d31", hover: "#3a3c42", accent: "#60a5fa", private_bg: "#3b2a5c" };

/// A private tab's title (0.16.0): the first of these that reaches [`AA`] on everything drawn
/// behind it. Krunk's private purple (`--private`) first, so it stays wherever it reads; then
/// lighter purples, darker ones, and plain text colours.
pub const PRIVATE_CANDIDATES: [&str; 11] = [
    "#6d28d9", "#a78bfa", "#c4b5fd", "#ddd6fe", "#ede9fe", "#5b21b6", "#4c1d95", "#2e1065", LIGHT_TEXT, DARK_TEXT,
    "#000000",
];
/// A hovered private tab on a gradient or glass strip is veiled with black or white at this
/// opacity: the one that moves the strip away from the title, so hovering never costs contrast.
pub const PRIVATE_HOVER_VEIL: f64 = 0.15;
/// The glass strip (shell.css): `--glass-strip` is the strip colour at this opacity over the
/// blurred backdrop, which the filter saturates by [`GLASS_SATURATE`].
pub const GLASS_STRIP: f64 = 0.46;
pub const GLASS_SATURATE: f64 = 1.5;
/// The backdrop behind glass bars (shell.css, `body`): three soft spots of the accent, each
/// with its hue turned by so many degrees and at most this opaque, over the window colour.
pub const GLASS_SPOTS: [(f64, f64); 3] = [(0.0, 0.55), (40.0, 0.42), (-50.0, 0.36)];
/// Colours taken between two neighbours of a gradient, or between the window colour and a
/// spot (the darkest point of a blend can lie between its ends).
const BLEND_STEPS: u8 = 8;

type Rgb = (u8, u8, u8);

/// The lowest contrast of `text` over `backgrounds`.
fn worst_contrast(text: Rgb, backgrounds: &[Rgb]) -> f64 {
    backgrounds.iter().map(|b| contrast(*b, text)).fold(f64::INFINITY, f64::min)
}

/// The first of [`PRIVATE_CANDIDATES`] that reaches [`AA`] on every one of `backgrounds`; when
/// none does (a strip that runs from dark to light), the one with the best worst case.
pub fn private_text(backgrounds: &[(u8, u8, u8)]) -> &'static str {
    let worst = |c: &&str| worst_contrast(parse_hex(c).unwrap_or_default(), backgrounds);
    PRIVATE_CANDIDATES
        .iter()
        .find(|c| worst(c) >= AA)
        .or_else(|| PRIVATE_CANDIDATES.iter().max_by(|a, b| worst(a).total_cmp(&worst(b))))
        .copied()
        .unwrap_or(DARK_TEXT)
}

/// CSS `saturate(s)`: the filter's matrix, on sRGB values as CSS filters work.
fn saturate((r, g, b): Rgb, s: f64) -> Rgb {
    let (r, g, b) = (f64::from(r), f64::from(g), f64::from(b));
    let ch = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    (
        ch((0.213 + 0.787 * s) * r + (0.715 - 0.715 * s) * g + (0.072 - 0.072 * s) * b),
        ch((0.213 - 0.213 * s) * r + (0.715 + 0.285 * s) * g + (0.072 - 0.072 * s) * b),
        ch((0.213 - 0.213 * s) * r + (0.715 - 0.715 * s) * g + (0.072 + 0.928 * s) * b),
    )
}

/// `from`, `to` and the colours between them (as a CSS gradient blends, per sRGB channel).
fn blend(from: Rgb, to: Rgb) -> impl Iterator<Item = Rgb> {
    (0..=BLEND_STEPS).map(move |k| mix(from, to, f64::from(k) / f64::from(BLEND_STEPS)))
}

/// What the shell draws behind an inactive private tab (shell.css): the gradient of a gradient
/// strip, the glass strip over its backdrop, otherwise the strip's colour. A private tab takes
/// no part in an ombre: it lies flat on the strip.
fn private_strip(a: &crate::settings::Appearance, theme: &ThemeBase) -> Vec<Rgb> {
    use crate::settings::BarStyle::{Glass, Gradient};
    let window = parse_hex(&a.background_color).or_else(|| parse_hex(theme.bg)).unwrap_or_default();
    // The backdrop of glass bars shows wherever nothing opaque covers it.
    let backdrop = || -> Vec<Rgb> {
        let accent = parse_hex(&a.accent).or_else(|| parse_hex(theme.accent)).unwrap_or_default();
        GLASS_SPOTS
            .iter()
            .flat_map(|(turn, alpha)| blend(window, mix(window, rotate_hue(accent, *turn), *alpha)))
            .collect()
    };
    match a.tab_style {
        Gradient => {
            let stops: Vec<Rgb> = gradient_stops(a).iter().filter_map(|s| parse_hex(s)).collect();
            stops.windows(2).flat_map(|w| blend(w[0], w[1])).collect()
        }
        Glass => {
            let strip = parse_hex(&a.strip_color).unwrap_or(window);
            backdrop().into_iter().map(|c| mix(saturate(c, GLASS_SATURATE), strip, GLASS_STRIP)).collect()
        }
        _ => match parse_hex(&a.strip_color) {
            Some(strip) => vec![strip],
            None if a.toolbar_style == Glass => backdrop(),
            None => vec![window],
        },
    }
}

/// The veil of a hovered private tab on a gradient or glass strip ([`PRIVATE_HOVER_VEIL`]):
/// black or white, whichever leaves `text` the better contrast. `None` on the other strips,
/// where a hovered tab is filled with the theme's hover colour.
fn private_hover_veil(a: &crate::settings::Appearance, text: Rgb, strip: &[Rgb]) -> Option<Rgb> {
    use crate::settings::BarStyle::{Glass, Gradient};
    let veiled = |v: Rgb| -> Vec<Rgb> { strip.iter().map(|c| mix(*c, v, PRIVATE_HOVER_VEIL)).collect() };
    let (black, white) = ((0, 0, 0), (255, 255, 255));
    matches!(a.tab_style, Gradient | Glass).then(|| {
        if worst_contrast(text, &veiled(black)) >= worst_contrast(text, &veiled(white)) {
            black
        } else {
            white
        }
    })
}

/// What the shell draws behind a hovered private tab.
fn private_hover(theme: &ThemeBase, strip: &[Rgb], veil: Option<Rgb>) -> Vec<Rgb> {
    match veil {
        Some(v) => strip.iter().map(|c| mix(*c, v, PRIVATE_HOVER_VEIL)).collect(),
        None => vec![parse_hex(theme.hover).unwrap_or_default()],
    }
}

/// The private tabs' title colours in one theme (0.16.0), each the first of
/// [`PRIVATE_CANDIDATES`] that reaches [`AA`] on what the shell draws behind that state (a
/// hovered tab keeps the resting colour where that does). The mask icon takes the title's
/// colour. A window in the background uses the same colours.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivateTabColors {
    /// An inactive private tab, on the strip.
    pub fg: String,
    /// The same tab under the mouse.
    pub hover_fg: String,
    /// Its fill then, as a CSS colour: a veil on a gradient or glass strip; empty = the
    /// theme's hover colour.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hover_bg: String,
    /// The active private tab, on its purple fill (`--private-bg`).
    pub active_fg: String,
    /// A private tab dragged in from another window (its ghost, on `--bg-raised`).
    pub ghost_fg: String,
}

/// [`PrivateTabColors`] for both themes: the shell follows Windows' theme by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrivateColors {
    pub light: PrivateTabColors,
    pub dark: PrivateTabColors,
}

impl PrivateTabColors {
    pub fn from_appearance(a: &crate::settings::Appearance, theme: &ThemeBase) -> PrivateTabColors {
        let strip = private_strip(a, theme);
        let fg = private_text(&strip);
        // The user's own title colour (0.15.2) wins in the shell: the veil suits that one.
        let shown = parse_hex(&a.tab_text_color).or_else(|| parse_hex(fg)).unwrap_or_default();
        let veil = private_hover_veil(a, shown, &strip);
        let on = |c: &str| private_text(&[parse_hex(c).unwrap_or_default()]).to_string();
        // Under the mouse the title keeps its colour where that still reads.
        let hover = private_hover(theme, &strip, veil);
        let keeps = worst_contrast(parse_hex(fg).unwrap_or_default(), &hover) >= AA;
        PrivateTabColors {
            fg: fg.to_string(),
            hover_fg: if keeps { fg } else { private_text(&hover) }.to_string(),
            hover_bg: veil.map(|(r, g, b)| format!("rgba({r}, {g}, {b}, {PRIVATE_HOVER_VEIL})")).unwrap_or_default(),
            active_fg: on(theme.private_bg),
            ghost_fg: on(theme.raised),
        }
    }
}

impl ThemeColors {
    /// For the shell: [`ThemeColors::from_appearance`] and the private tabs' title colours.
    pub fn for_shell(a: &crate::settings::Appearance) -> ThemeColors {
        let private = PrivateColors {
            light: PrivateTabColors::from_appearance(a, &LIGHT_THEME),
            dark: PrivateTabColors::from_appearance(a, &DARK_THEME),
        };
        ThemeColors { private: Some(Box::new(private)), ..ThemeColors::from_appearance(a) }
    }

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
        ThemeColors {
            accent,
            accent_fg,
            toolbar,
            toolbar_fg,
            strip,
            strip_fg,
            background,
            background_fg,
            gradient,
            private: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == ThemeColors::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Appearance, BarStyle};

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
        assert_eq!(with_gradient, 27);
    }

    // ---- private tabs (0.16.0) ----

    /// `--fg-muted` (common.css): what 0.15.3 drew an inactive private tab's title with while
    /// the window was in the background.
    const MUTED: [&str; 2] = ["#6b7280", "#9ca3af"];
    const THEMES: [(&str, &ThemeBase); 2] = [("light", &LIGHT_THEME), ("dark", &DARK_THEME)];

    /// One state of a private tab: what is behind its title, the title's colour, and the same
    /// for 0.15.3 (always `--private`, or `--fg-muted` in a background window).
    struct State {
        /// The key in `ui/dev/private-tab-colors.json` (the focused window's states).
        key: &'static str,
        name: &'static str,
        behind: Vec<Rgb>,
        title: String,
        old_behind: Vec<Rgb>,
        old_title: &'static str,
    }

    impl State {
        fn ratio(&self) -> f64 {
            worst_contrast(parse_hex(&self.title).unwrap(), &self.behind)
        }
        fn old_ratio(&self) -> f64 {
            worst_contrast(parse_hex(self.old_title).unwrap(), &self.old_behind)
        }
        /// The colour behind the title that it reads worst on.
        fn worst_behind(&self) -> Rgb {
            let t = parse_hex(&self.title).unwrap();
            *self.behind.iter().min_by(|a, b| contrast(**a, t).total_cmp(&contrast(**b, t))).unwrap()
        }
    }

    /// Every state of a private tab under `a`, as shell.css draws it.
    fn private_states(a: &Appearance, dark: bool) -> Vec<State> {
        let theme = THEMES[usize::from(dark)].1;
        let c = PrivateTabColors::from_appearance(a, theme);
        let rgb = |s: &str| parse_hex(s).unwrap();
        let strip = private_strip(a, theme);
        let veil = private_hover_veil(a, rgb(&c.fg), &strip);
        assert_eq!(veil.is_some(), !c.hover_bg.is_empty());
        let hover = private_hover(theme, &strip, veil);
        // 0.15.3 under the mouse: the theme's hover colour; on a gradient or glass strip a
        // tint of the title's own colour.
        let old_hover = |text: &str| -> Vec<Rgb> {
            match veil {
                Some(_) => strip.iter().map(|s| mix(*s, rgb(text), 0.15)).collect(),
                None => vec![rgb(theme.hover)],
            }
        };
        let (purple, muted) = (PRIVATE_CANDIDATES[0], MUTED[usize::from(dark)]);
        let fill = vec![rgb(theme.private_bg)];
        let state = |key, name, behind: &[Rgb], title: &str, old_behind: Vec<Rgb>, old_title| State {
            key,
            name,
            behind: behind.to_vec(),
            title: title.to_string(),
            old_behind,
            old_title,
        };
        vec![
            state("inactive", "inactive", &strip, &c.fg, strip.clone(), purple),
            state("hover", "hovered", &hover, &c.hover_fg, old_hover(purple), purple),
            state("active", "active", &fill, &c.active_fg, fill.clone(), purple),
            state("", "inactive, window in the background", &strip, &c.fg, strip.clone(), muted),
            state("", "hovered, window in the background", &hover, &c.hover_fg, old_hover(muted), muted),
            state("", "active, window in the background", &fill, &c.active_fg, fill.clone(), purple),
            state(
                "ghost",
                "dragged in from another window",
                &[rgb(theme.raised)],
                &c.ghost_fg,
                vec![rgb(theme.raised)],
                purple,
            ),
        ]
    }

    fn preset_looks() -> Vec<(String, Appearance)> {
        crate::style::PRESETS
            .iter()
            .map(|p| {
                let mut a = Appearance::default();
                p.apply(&mut a);
                (p.label.to_string(), a)
            })
            .collect()
    }

    /// Looks that are not presets: purple where the private purple used to vanish, a dark
    /// strip in the light theme, glass over a purple accent, and a gradient no single colour
    /// reads on.
    fn other_looks() -> Vec<(String, Appearance)> {
        let look = |name: &str, edit: &dyn Fn(&mut Appearance)| {
            let mut a = Appearance::default();
            edit(&mut a);
            (name.to_string(), a)
        };
        vec![
            look("Purple tab strip (#6d28d9)", &|a| a.strip_color = "#6d28d9".into()),
            look("Dark tab strip (#0b1220)", &|a| a.strip_color = "#0b1220".into()),
            look("Lavender window (#ede9fe)", &|a| a.background_color = "#ede9fe".into()),
            look("Glass, purple accent (#7c3aed)", &|a| {
                crate::style::preset("glass").unwrap().apply(a);
                a.accent = "#7c3aed".into();
            }),
            look("Solid tabs, glass toolbar, purple accent (#7c3aed)", &|a| {
                a.toolbar_style = BarStyle::Glass;
                a.accent = "#7c3aed".into();
            }),
            look("Midnight, purple accent (#7c3aed)", &|a| {
                crate::style::preset("midnight").unwrap().apply(a);
                a.accent = "#7c3aed".into();
            }),
            look("Gradient from a purple accent (#7c3aed)", &|a| {
                a.tab_style = BarStyle::Gradient;
                a.toolbar_style = BarStyle::Gradient;
                a.accent = "#7c3aed".into();
            }),
        ]
    }

    #[test]
    fn theme_base_matches_common_css() {
        let css = include_str!("../../ui/shared/common.css");
        let (light, dark) = css.split_once(":root.dark {").expect("the dark theme's block");
        let var = |block: &str, name: &str| -> String {
            let at = block.find(&format!("--{name}: ")).unwrap_or_else(|| panic!("--{name}"));
            block[at + name.len() + 4..].split(';').next().unwrap().trim().to_string()
        };
        for (block, t, muted) in [(light, &LIGHT_THEME, MUTED[0]), (dark, &DARK_THEME, MUTED[1])] {
            assert_eq!(var(block, "bg"), t.bg);
            assert_eq!(var(block, "bg-raised"), t.raised);
            assert_eq!(var(block, "bg-hover"), t.hover);
            assert_eq!(var(block, "accent"), t.accent);
            assert_eq!(var(block, "private-bg"), t.private_bg);
            assert_eq!(var(block, "fg-muted"), muted);
        }
        assert_eq!(var(light, "private"), PRIVATE_CANDIDATES[0]);
        assert_eq!(LIGHT_THEME.accent, DEFAULT_ACCENT);
    }

    #[test]
    fn private_text_is_the_first_candidate_that_reaches_aa() {
        let on = |c: &str| private_text(&[parse_hex(c).unwrap()]);
        // The private purple stays wherever it reads.
        assert_eq!(on("#f3f4f6"), "#6d28d9");
        assert_eq!(on("#ffffff"), "#6d28d9");
        assert_eq!(on("#ede9fe"), "#6d28d9");
        // Dark surfaces: a lighter purple; purple ones: white or a pale purple, not purple.
        assert_eq!(on("#1f2023"), "#a78bfa");
        assert_eq!(on("#0f172a"), "#a78bfa");
        assert_ne!(on("#6d28d9"), "#6d28d9");
        assert!(contrast(parse_hex(on("#6d28d9")).unwrap(), parse_hex("#6d28d9").unwrap()) >= AA);
        // Mid-tone surfaces: a darker purple before plain black.
        assert_eq!(on("#0ea5e9"), "#2e1065");
        // Every single colour has a candidate that reaches AA (black or white always does on
        // one colour, except in a narrow mid-luminance band: then the best there is).
        for v in (0..=255u8).step_by(5) {
            for bg in [(v, v, v), (v, 255 - v, v.wrapping_mul(7)), (255 - v, v / 2, v)] {
                let got = worst_contrast(parse_hex(private_text(&[bg])).unwrap(), &[bg]);
                let best = PRIVATE_CANDIDATES.iter().map(|c| contrast(parse_hex(c).unwrap(), bg)).fold(0.0, f64::max);
                assert!(got >= AA || got == best, "{}: {got:.2}, best {best:.2}", to_hex(bg));
                assert!(got >= 4.4, "{}: {got:.2}", to_hex(bg));
            }
        }
        // A strip from black to white: nothing reaches AA on both ends; the best worst case.
        let both = [(0, 0, 0), (255, 255, 255)];
        let got = worst_contrast(parse_hex(private_text(&both)).unwrap(), &both);
        assert!(PRIVATE_CANDIDATES.iter().all(|c| worst_contrast(parse_hex(c).unwrap(), &both) <= got));
        assert!(PRIVATE_CANDIDATES.iter().all(|c| parse_hex(c).is_some()));
    }

    #[test]
    fn private_titles_reach_aa_on_every_preset_theme_and_state() {
        let looks = preset_looks();
        assert_eq!(looks.len(), 20);
        let mut checked = 0;
        for (name, a) in &looks {
            for dark in [false, true] {
                let states = private_states(a, dark);
                assert_eq!(states.len(), 7);
                for s in states {
                    assert!(!s.behind.is_empty());
                    let r = s.ratio();
                    assert!(
                        r >= AA,
                        "{name}, {}, {}: {} on {} is {r:.2}",
                        THEMES[usize::from(dark)].0,
                        s.name,
                        s.title,
                        to_hex(s.worst_behind())
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 20 * 2 * 7);
        // The other looks reach it too, except the gradient no colour reads on from end to
        // end: there the title is the best of the candidates.
        for (name, a) in other_looks() {
            for dark in [false, true] {
                for s in private_states(&a, dark) {
                    let t = parse_hex(&s.title).unwrap();
                    let best = PRIVATE_CANDIDATES
                        .iter()
                        .all(|c| worst_contrast(parse_hex(c).unwrap(), &s.behind) <= worst_contrast(t, &s.behind));
                    assert!(
                        s.ratio() >= AA || (name.starts_with("Gradient") && best),
                        "{name}, {}: {:.2}",
                        s.name,
                        s.ratio()
                    );
                }
            }
        }
    }

    #[test]
    fn styles_that_read_keep_the_private_purple() {
        // Classic in the light theme: every state as before 0.16.0.
        let c = PrivateTabColors::from_appearance(&Appearance::default(), &LIGHT_THEME);
        let purple = "#6d28d9".to_string();
        assert_eq!(
            c,
            PrivateTabColors {
                fg: purple.clone(),
                hover_fg: purple.clone(),
                hover_bg: String::new(),
                active_fg: purple.clone(),
                ghost_fg: purple.clone(),
            }
        );
        // So do the other solid and ombre styles there (a private tab lies flat on the strip).
        for (name, a) in preset_looks() {
            if matches!(a.tab_style, BarStyle::Solid | BarStyle::Ombre) {
                assert_eq!(PrivateTabColors::from_appearance(&a, &LIGHT_THEME), c, "{name}");
            }
        }
        // The dark theme never could: a lighter purple on the strip, under the mouse and on
        // the active tab's fill.
        let d = PrivateTabColors::from_appearance(&Appearance::default(), &DARK_THEME);
        assert_eq!((d.fg.as_str(), d.hover_fg.as_str(), d.hover_bg.as_str()), ("#a78bfa", "#c4b5fd", ""));
        assert_eq!((d.active_fg.as_str(), d.ghost_fg.as_str()), ("#a78bfa", "#a78bfa"));
    }

    #[test]
    fn private_colours_follow_the_strip() {
        let colours = |edit: &dyn Fn(&mut Appearance)| {
            let mut a = Appearance::default();
            edit(&mut a);
            (PrivateTabColors::from_appearance(&a, &LIGHT_THEME), PrivateTabColors::from_appearance(&a, &DARK_THEME))
        };
        // A custom strip colour is the same in both themes; the window colour shows without one.
        let (l, d) = colours(&|a| a.strip_color = "#0b1220".into());
        assert_eq!((l.fg.as_str(), d.fg.as_str()), ("#a78bfa", "#a78bfa"));
        // (the hovered tab is filled with the theme's hover colour, whatever the strip)
        assert_eq!((l.hover_fg.as_str(), d.hover_fg.as_str()), ("#6d28d9", "#c4b5fd"));
        let (l, _) = colours(&|a| a.background_color = "#0b1220".into());
        assert_eq!(l.fg, "#a78bfa");
        let (l, _) = colours(&|a| {
            a.background_color = "#0b1220".into();
            a.strip_color = "#ffffff".into();
        });
        assert_eq!(l.fg, "#6d28d9");
        // An ombre strip: private tabs are not part of it.
        let (l, d) = colours(&|a| crate::style::preset("sunset").unwrap().apply(a));
        assert_eq!((l.fg.as_str(), d.fg.as_str(), l.hover_bg.as_str()), ("#6d28d9", "#a78bfa", ""));
        // A gradient strip: one colour for every point of it, the same in both themes, and a
        // veil under the mouse that darkens under light text and lightens under dark text.
        let (l, d) = colours(&|a| crate::style::preset("midnight").unwrap().apply(a));
        assert_eq!(l, PrivateTabColors { active_fg: l.active_fg.clone(), ghost_fg: l.ghost_fg.clone(), ..d.clone() });
        assert_eq!(
            (d.fg.as_str(), d.hover_fg.as_str(), d.hover_bg.as_str()),
            ("#c4b5fd", "#c4b5fd", "rgba(0, 0, 0, 0.15)")
        );
        let (l, _) = colours(&|a| crate::style::preset("ocean").unwrap().apply(a));
        assert_eq!((l.fg.as_str(), l.hover_bg.as_str()), ("#2e1065", "rgba(255, 255, 255, 0.15)"));
        // The user's own title colour decides the veil (the shell draws that colour).
        let (l, _) = colours(&|a| {
            crate::style::preset("ocean").unwrap().apply(a);
            a.tab_text_color = "#ffffff".into();
        });
        assert_eq!((l.fg.as_str(), l.hover_bg.as_str()), ("#2e1065", "rgba(0, 0, 0, 0.15)"));
        // Glass: the strip over its accent backdrop, per theme and accent.
        let (l, d) = colours(&|a| crate::style::preset("glass").unwrap().apply(a));
        assert!(!l.hover_bg.is_empty() && !d.hover_bg.is_empty() && l.fg != d.fg);
        // A gradient's darkest point can lie between two stops.
        let between = blend((255, 0, 0), (0, 0, 255)).map(luminance).fold(f64::INFINITY, f64::min);
        assert!(between < luminance((0, 0, 255)));
        assert_eq!(saturate((128, 128, 128), 1.5), (128, 128, 128));
        assert_eq!(saturate((200, 100, 50), 1.0), (200, 100, 50));
    }

    #[test]
    fn private_colours_on_the_wire() {
        // Only the shell gets them; the pages' colours stay empty without custom colours.
        let a = Appearance::default();
        assert!(ThemeColors::from_appearance(&a).private.is_none());
        let c = ThemeColors::for_shell(&a);
        assert!(!c.is_empty());
        assert_eq!(ThemeColors { private: None, ..c.clone() }, ThemeColors::from_appearance(&a));
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(
            json["private"],
            serde_json::json!({
                "light": { "fg": "#6d28d9", "hoverFg": "#6d28d9", "activeFg": "#6d28d9", "ghostFg": "#6d28d9" },
                "dark": { "fg": "#a78bfa", "hoverFg": "#c4b5fd", "activeFg": "#a78bfa", "ghostFg": "#a78bfa" },
            })
        );
        let mut ocean = Appearance::default();
        crate::style::preset("ocean").unwrap().apply(&mut ocean);
        let json = serde_json::to_value(ThemeColors::for_shell(&ocean)).unwrap();
        assert_eq!(json["private"]["dark"]["hoverBg"], "rgba(255, 255, 255, 0.15)");
        assert!(json["gradient"]["fg"].is_string());
        assert_eq!(serde_json::from_value::<ThemeColors>(json).unwrap(), ThemeColors::for_shell(&ocean));
    }

    /// `docs/private-tab-contrast.md` (the measured table, evidence for the release) and
    /// `ui/dev/private-tab-colors.json` (what the host sends for each look; the UI tests draw
    /// the shell with it and measure the pixels). Regenerate both with
    /// `LB_UPDATE_FIXTURES=1 cargo test -p core --lib color::`.
    #[test]
    fn private_tab_contrast_table_is_current() {
        // Never rounded up: 4.499 must not read as 4.50.
        let floor = |r: f64| (r * 100.0).floor() / 100.0;
        let mut md = String::from(
            "# Private tab titles: contrast on every style\n\n\
             Krunk 0.16.0 chooses the colour of a private tab's title (and of its mask icon) for contrast\n\
             with what is drawn behind it: the first of\n\n",
        );
        md += &PRIVATE_CANDIDATES.iter().map(|c| format!("`{c}`")).collect::<Vec<_>>().join(", ");
        md +=
            "\n\nthat reaches 4.5:1 (WCAG 2 contrast ratio, AA for normal text) on every colour behind that state of\n\
               the tab; when none does, the one with the best worst case (`core/src/color.rs`,\n\
               `PrivateTabColors`). 0.15.3 drew `#6d28d9` everywhere, and the theme's muted grey on inactive\n\
               tabs while the window was in the background.\n\n\
               This file is generated: `LB_UPDATE_FIXTURES=1 cargo test -p core --lib color::` writes it, and\n\
               the test `private_tab_contrast_table_is_current` fails when it is stale. The UI test\n\
               `ui/dev/test-0160-private.cjs` draws the shell with the same colours and measures the pixels.\n\n\
               - **Behind the title:** the colour the 0.16.0 title reads worst on, of all the colours that can\n\
               \x20 lie behind it in that state (a gradient strip: every point of the gradient; a glass strip:\n\
               \x20 the strip over every shade of its accent backdrop; under the mouse on those two: the same\n\
               \x20 under a 15 % veil of black or white, whichever keeps the title readable). On a solid or\n\
               \x20 ombre strip a private tab lies flat on the strip's colour and a hovered one is filled with\n\
               \x20 the theme's hover colour.\n\
               - **Ratios** are each title's worst case over those colours, rounded down. The 0.15.3 ratio\n\
               \x20 is measured on what 0.15.3 drew (under the mouse on a gradient or glass strip: a 15 % tint\n\
               \x20 of the title's own colour).\n\
               - A window in the background uses the colours of the focused one since 0.16.0.\n\n";
        // The active tab's fill and the ghost's surface are the theme's, the same on every
        // style: the strip is where the styles differ.
        let columns = "| On the strip, 0.15.3 | On the strip, 0.16.0 | Every state, 0.15.3 | Every state, 0.16.0 |\n\
                       |---|---:|---:|---:|---:|\n";
        let mut summary = format!(
            "## Worst case per style\n\nThe lowest ratio over both themes: on the strip (inactive and hovered, \
             focused window and\nbackground window), and over every state (the active private tab's fill and the \
             ghost of a\ndragged tab are the theme's own, the same on every style).\n\n| Style {columns}"
        );
        let mut tables = String::new();
        let mut cases = Vec::new();
        for (heading, looks, presets) in
            [("Built-in styles", preset_looks(), true), ("Other looks", other_looks(), false)]
        {
            tables += &format!(
                "## {heading}\n\n| Style | Theme | State | Behind the title | 0.15.3 title | Ratio | 0.16.0 title | Ratio |\n\
                 |---|---|---|---|---|---:|---|---:|\n"
            );
            if !presets {
                summary += "\n## Worst case of other looks\n\nCustom colours are not styles; these show the rule on \
                            purple and dark strips, and its limit: no\nsingle colour reaches 4.5:1 on a gradient \
                            that runs through mid-tones of very different\nlightness, so the title gets the best \
                            there is.\n\n| Look ";
                summary += columns;
            }
            for (name, a) in looks {
                // [on the strip, every state] x [0.15.3, 0.16.0]
                let mut worst = [[f64::INFINITY; 2]; 2];
                let mut ratios = serde_json::Map::new();
                for (dark, (theme, _)) in THEMES.iter().enumerate() {
                    let mut per_state = serde_json::Map::new();
                    for s in private_states(&a, dark == 1) {
                        tables += &format!(
                            "| {name} | {theme} | {} | `{}` | `{}` | {:.2} | `{}` | {:.2} |\n",
                            s.name,
                            to_hex(s.worst_behind()),
                            s.old_title,
                            floor(s.old_ratio()),
                            s.title,
                            floor(s.ratio()),
                        );
                        let on_strip = s.name.starts_with("inactive") || s.name.starts_with("hovered");
                        for w in worst.iter_mut().skip(usize::from(!on_strip)) {
                            *w = [w[0].min(s.old_ratio()), w[1].min(s.ratio())];
                        }
                        if !s.key.is_empty() {
                            per_state.insert(s.key.to_string(), floor(s.ratio()).into());
                        }
                    }
                    ratios.insert((*theme).to_string(), per_state.into());
                }
                summary += &format!(
                    "| {name} | {:.2} | {:.2} | {:.2} | {:.2} |\n",
                    floor(worst[0][0]),
                    floor(worst[0][1]),
                    floor(worst[1][0]),
                    floor(worst[1][1])
                );
                cases.push(serde_json::json!({
                    "name": name,
                    "appearance": a,
                    "colors": ThemeColors::for_shell(&a),
                    "ratios": ratios,
                }));
            }
            tables += "\n";
        }
        let md = md + &summary + "\n" + tables.trim_end() + "\n";
        let json = serde_json::to_string_pretty(&cases).unwrap() + "\n";
        for (path, want) in [
            (concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/private-tab-contrast.md"), md),
            (concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/dev/private-tab-colors.json"), json),
        ] {
            if std::env::var_os("LB_UPDATE_FIXTURES").is_some() {
                std::fs::write(path, &want).unwrap();
            }
            let have = std::fs::read_to_string(path).unwrap_or_default();
            assert!(have == want, "{path} is stale; run with LB_UPDATE_FIXTURES=1");
        }
    }
}
