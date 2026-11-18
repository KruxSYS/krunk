//! Themes on Android (0.2.0): the shared `appearance` settings (accent colour and the curated
//! styles of `core::style::PRESETS`) resolved into the final colours and shapes the Compose chrome
//! draws. Light, dark or private picks the base palette; the style decides how the toolbar is
//! filled, how round the address pill and the tab cards are and how the active tab card is marked.
//!
//! Everything a person has to read is checked here: text reaches 4.5:1 and icons 3:1 on whatever
//! they are drawn on (WCAG 2.x, `core::color::contrast`), for any accent and any gradient, because
//! colours that would not reach it are moved towards the text colour until they do.
use lb_core::color::{self, contrast, luminance, mix, parse_hex, to_hex, DARK_TEXT, DEFAULT_ACCENT, LIGHT_TEXT};
use lb_core::settings::{self as cs, Appearance};
use lb_core::style;

type Rgb = (u8, u8, u8);

/// WCAG AA for text, and for icons and other marks that are not text.
pub(crate) const TEXT_RATIO: f64 = color::AA;
pub(crate) const ICON_RATIO: f64 = 3.0;
/// How far the glass style tints the toolbar and the address pill with the accent.
const GLASS_TINT_LIGHT: f64 = 0.14;
const GLASS_TINT_DARK: f64 = 0.20;
/// The lock of a secure page (the same green as on Windows).
const SECURE: Rgb = (0x16, 0xa3, 0x4a);
const BLACK: Rgb = (0, 0, 0);
const WHITE: Rgb = (255, 255, 255);

/// How the toolbar is filled (`core::settings::BarStyle` without the tab strip's ombre).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ToolbarFill {
    /// The theme's raised surface.
    Solid,
    /// A gradient of two or three colours at an angle.
    Gradient,
    /// The surface tinted with the accent (Android does not blur).
    Glass,
}

/// The corners of the address pill and of the tab cards (`core::settings::TabShape`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ThemeShape {
    Rounded,
    Square,
    Pill,
}

/// How the active tab card is marked in the switcher (`core::settings::ActiveHighlight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ThemeHighlight {
    /// The card's title bar is filled with the accent.
    Fill,
    /// A line in the accent under the title bar.
    Underline,
    /// A border in the accent.
    Outline,
    /// Only the title is drawn in the accent.
    None,
}

/// The part of the shared `appearance` settings Android themes read. Android changes only
/// `preset` (through [`theme_with_preset`]) and `accent` ([`theme_with_accent`]); the other fields
/// say what the stored style looks like and are ignored when saving, so a look made by hand on
/// Windows survives an Android save.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ThemeAppearance {
    /// A style's id ([`theme_presets`]); empty for a look that is not one of the presets.
    pub preset: String,
    /// `#rrggbb`; empty for Krunk's blue.
    pub accent: String,
    pub toolbar_fill: ToolbarFill,
    /// Gradient colours as stored (`#rrggbb`; empty ones derive from the accent).
    pub gradient_from: String,
    pub gradient_via: String,
    pub gradient_to: String,
    /// CSS degrees: 0 runs upwards, 90 to the right.
    pub gradient_angle: u16,
    pub shape: ThemeShape,
    pub highlight: ThemeHighlight,
}

impl ThemeAppearance {
    pub(crate) fn from_core(a: &Appearance) -> ThemeAppearance {
        ThemeAppearance {
            preset: a.preset.clone(),
            accent: a.accent.clone(),
            toolbar_fill: match a.toolbar_style {
                cs::BarStyle::Solid => ToolbarFill::Solid,
                cs::BarStyle::Gradient | cs::BarStyle::Ombre => ToolbarFill::Gradient,
                cs::BarStyle::Glass => ToolbarFill::Glass,
            },
            gradient_from: a.gradient_from.clone(),
            gradient_via: a.gradient_via.clone(),
            gradient_to: a.gradient_to.clone(),
            gradient_angle: a.gradient_angle,
            shape: match a.tab_shape {
                cs::TabShape::Rounded => ThemeShape::Rounded,
                cs::TabShape::Square => ThemeShape::Square,
                cs::TabShape::Pill => ThemeShape::Pill,
            },
            highlight: match a.active_highlight {
                cs::ActiveHighlight::Fill => ThemeHighlight::Fill,
                cs::ActiveHighlight::Underline => ThemeHighlight::Underline,
                cs::ActiveHighlight::Outline => ThemeHighlight::Outline,
                cs::ActiveHighlight::None => ThemeHighlight::None,
            },
        }
    }

    /// Writes the accent, and the whole preset when its id changed (`Preset::apply`, as the
    /// Windows settings page does). Everything else in `a` keeps its value.
    pub(crate) fn apply_to_core(&self, a: &mut Appearance) {
        a.accent = self.accent.trim().to_string();
        if self.preset != a.preset {
            match style::preset(&self.preset) {
                Some(p) => p.apply(a),
                // Empty: the look is the user's own from now on. Unknown: validation rejects it.
                None => a.preset = self.preset.clone(),
            }
        }
    }

    /// The core appearance with only what the colours depend on.
    fn gradient_source(&self) -> Appearance {
        Appearance {
            accent: self.accent.clone(),
            gradient_from: self.gradient_from.clone(),
            gradient_via: self.gradient_via.clone(),
            gradient_to: self.gradient_to.clone(),
            ..Appearance::default()
        }
    }
}

/// `appearance` with the style `id` chosen; unchanged for an id that is not a preset.
#[uniffi::export]
pub fn theme_with_preset(appearance: ThemeAppearance, id: String) -> ThemeAppearance {
    let Some(p) = style::preset(&id) else { return appearance };
    // The old style's id too: its own accent leaves with it (0.20.0).
    let mut a = Appearance { accent: appearance.accent, preset: appearance.preset, ..Appearance::default() };
    p.apply(&mut a);
    ThemeAppearance::from_core(&a)
}

/// `appearance` with the accent `color` (`#rrggbb`); Krunk's blue is stored as "no accent", so it
/// stays the lighter blue in the dark theme. Unchanged for anything that is not a colour.
#[uniffi::export]
pub fn theme_with_accent(appearance: ThemeAppearance, color: String) -> ThemeAppearance {
    let accent = match parse_hex(&color) {
        Some(c) if to_hex(c) == DEFAULT_ACCENT => String::new(),
        Some(c) => to_hex(c),
        None if color.trim().is_empty() => String::new(),
        None => return appearance,
    };
    ThemeAppearance { accent, ..appearance }
}

/// A theme as the chrome draws it. Colours are `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ResolvedTheme {
    /// The base palette is dark (always for private tabs).
    pub dark: bool,
    pub private: bool,
    /// The page behind everything.
    pub background: String,
    /// Cards and menus.
    pub surface_raised: String,
    /// Dialogs and pressed rows.
    pub surface_hover: String,
    pub surface_lowest: String,
    pub border: String,
    pub text: String,
    /// Secondary text; readable on all three surfaces and the address pill.
    pub text_muted: String,
    /// Error text (readable like `text_muted`) and what is drawn on an error fill.
    pub error: String,
    pub on_error: String,
    /// The accent as chosen, for filled areas, with the text drawn on it.
    pub accent: String,
    pub on_accent: String,
    /// The accent where it is text or a thin mark on a surface (switches, the progress line,
    /// text buttons): moved towards `text` until it is readable there.
    pub primary: String,
    pub on_primary: String,
    pub primary_container: String,
    pub on_primary_container: String,
    pub toolbar_fill: ToolbarFill,
    /// One colour, or the gradient's two or three stops.
    pub toolbar_colors: Vec<String>,
    /// CSS degrees for a gradient: 0 runs upwards, 90 to the right.
    pub toolbar_angle: u16,
    /// Text and icons on the toolbar.
    pub toolbar_text: String,
    /// The strip behind the status bar: the toolbar's fill for gradient and glass, else the
    /// background. `status_strip_dark` asks for light status-bar icons.
    pub status_strip: Vec<String>,
    pub status_strip_dark: bool,
    /// The address pill, its text and the lock of a secure page.
    pub pill: String,
    pub pill_text: String,
    pub secure: String,
    pub shape: ThemeShape,
    pub highlight: ThemeHighlight,
}

/// A base palette (ui/shared/common.css; the private one is Android's).
struct Palette {
    background: &'static str,
    raised: &'static str,
    hover: &'static str,
    lowest: &'static str,
    border: &'static str,
    text: &'static str,
    muted: &'static str,
    error: &'static str,
    /// Krunk's own accent in this palette, with its container pair.
    accent: &'static str,
    container: &'static str,
    on_container: &'static str,
}

const LIGHT: Palette = Palette {
    background: "#f3f4f6",
    raised: "#ffffff",
    hover: "#e5e7eb",
    lowest: "#ffffff",
    border: "#d1d5db",
    text: "#1f2937",
    muted: "#6b7280",
    error: "#dc2626",
    accent: DEFAULT_ACCENT,
    container: "#dbeafe",
    on_container: "#1e3a8a",
};

const DARK: Palette = Palette {
    background: "#1f2023",
    raised: "#2b2d31",
    hover: "#3a3c42",
    lowest: "#17181b",
    border: "#44474e",
    text: "#e5e7eb",
    muted: "#9ca3af",
    error: "#f87171",
    accent: "#60a5fa",
    container: "#1e3a8a",
    on_container: "#dbeafe",
};

/// Private tabs: dark, tinted with Krunk's private purple, the same in every theme.
const PRIVATE: Palette = Palette {
    background: "#1b1526",
    raised: "#2a2140",
    hover: "#3b2a5c",
    lowest: "#17181b",
    border: "#44474e",
    text: "#e5e7eb",
    muted: "#9ca3af",
    error: "#f87171",
    accent: "#c4b5fd",
    container: "#6d28d9",
    on_container: "#ede9fe",
};

fn rgb(hex: &str) -> Rgb {
    parse_hex(hex).unwrap_or_default()
}

/// The lowest contrast of `fg` over all of `on`.
fn worst(fg: Rgb, on: &[Rgb]) -> f64 {
    on.iter().map(|c| contrast(fg, *c)).fold(f64::INFINITY, f64::min)
}

/// The text for something drawn over all of `on`: the core's choice (`foreground_on_all`) when it
/// reaches the text ratio, else the best of the core's dark and light text and pure black.
fn best_text(on: &[Rgb]) -> Rgb {
    let core = color::foreground_on_all(on).map(rgb).unwrap_or(rgb(DARK_TEXT));
    if worst(core, on) >= TEXT_RATIO {
        return core;
    }
    [rgb(DARK_TEXT), rgb(LIGHT_TEXT), BLACK]
        .into_iter()
        .max_by(|a, b| worst(*a, on).total_cmp(&worst(*b, on)))
        .unwrap_or(core)
}

/// `colour` moved towards `towards` just far enough to reach `ratio` on every one of `on`; the
/// best plain text colour when even `towards` does not get there.
fn ensure(colour: Rgb, towards: Rgb, on: &[Rgb], ratio: f64) -> Rgb {
    (0..=50).map(|step| mix(colour, towards, f64::from(step) / 50.0)).find(|c| worst(*c, on) >= ratio).unwrap_or_else(
        || {
            let text = best_text(on);
            // Black or white always reaches the icon ratio; the text ratio needs the better one.
            if worst(text, on) >= ratio {
                text
            } else {
                [BLACK, WHITE].into_iter().max_by(|a, b| worst(*a, on).total_cmp(&worst(*b, on))).unwrap_or(text)
            }
        },
    )
}

/// A gradient with the text drawn on it. A gradient no single text colour is readable on (say
/// black to white, made by hand on Windows) is toned towards black or white until one is.
fn readable_gradient(stops: Vec<Rgb>) -> (Vec<Rgb>, Rgb) {
    let fg = best_text(&stops);
    if worst(fg, &stops) >= TEXT_RATIO {
        return (stops, fg);
    }
    let towards = if luminance(fg) > 0.5 { BLACK } else { WHITE };
    for step in 1..=20 {
        let toned: Vec<Rgb> = stops.iter().map(|s| mix(*s, towards, f64::from(step) / 20.0)).collect();
        let fg = best_text(&toned);
        if worst(fg, &toned) >= TEXT_RATIO {
            return (toned, fg);
        }
    }
    (vec![towards; stops.len()], best_text(&[towards]))
}

/// The colours and shapes for `appearance` in the light or dark theme. Private tabs get the
/// fixed private palette with a plain toolbar whatever the style (only its shape and highlight
/// carry over), so they look different from every theme.
#[uniffi::export]
pub fn resolve_theme(appearance: ThemeAppearance, dark: bool, private: bool) -> ResolvedTheme {
    let p = if private {
        &PRIVATE
    } else if dark {
        &DARK
    } else {
        &LIGHT
    };
    let dark = dark || private;
    let custom_accent = if private { None } else { parse_hex(&appearance.accent) };
    let accent = custom_accent.unwrap_or(rgb(p.accent));
    let (background, raised, hover, text) = (rgb(p.background), rgb(p.raised), rgb(p.hover), rgb(p.text));

    let toolbar_fill = if private { ToolbarFill::Solid } else { appearance.toolbar_fill };
    let (toolbar, toolbar_text, pill) = match toolbar_fill {
        ToolbarFill::Solid => (vec![raised], text, hover),
        ToolbarFill::Glass => {
            let tint = if dark { GLASS_TINT_DARK } else { GLASS_TINT_LIGHT };
            let bar = mix(raised, accent, tint);
            (vec![bar], ensure(text, best_text(&[bar]), &[bar], TEXT_RATIO), mix(hover, accent, tint))
        }
        ToolbarFill::Gradient => {
            let stops = color::gradient_stops(&appearance.gradient_source()).iter().map(|s| rgb(s)).collect();
            let (stops, fg) = readable_gradient(stops);
            (stops, fg, raised)
        }
    };
    let status_strip = if toolbar_fill == ToolbarFill::Solid { vec![background] } else { toolbar.clone() };
    let status_strip_dark = if toolbar_fill == ToolbarFill::Solid { dark } else { luminance(toolbar_text) > 0.5 };

    // Everything coloured text is drawn on.
    let surfaces = [background, raised, hover, pill];
    let primary = ensure(accent, text, &surfaces, TEXT_RATIO);
    let error = ensure(rgb(p.error), text, &surfaces, TEXT_RATIO);
    let (container, on_container) = match custom_accent {
        Some(_) => {
            let container = mix(raised, accent, if dark { 0.35 } else { 0.16 });
            (container, ensure(mix(accent, text, 0.6), text, &[container], TEXT_RATIO))
        }
        None => (rgb(p.container), rgb(p.on_container)),
    };
    let hex = |c: Rgb| to_hex(c);
    ResolvedTheme {
        dark,
        private,
        background: hex(background),
        surface_raised: hex(raised),
        surface_hover: hex(hover),
        surface_lowest: p.lowest.to_string(),
        border: p.border.to_string(),
        text: hex(text),
        text_muted: hex(ensure(rgb(p.muted), text, &surfaces, TEXT_RATIO)),
        error: hex(error),
        on_error: hex(best_text(&[error])),
        accent: hex(accent),
        on_accent: hex(best_text(&[accent])),
        primary: hex(primary),
        on_primary: hex(best_text(&[primary])),
        primary_container: hex(container),
        on_primary_container: hex(on_container),
        toolbar_fill,
        toolbar_colors: toolbar.iter().map(|c| hex(*c)).collect(),
        toolbar_angle: if toolbar_fill == ToolbarFill::Gradient { appearance.gradient_angle } else { 90 },
        toolbar_text: hex(toolbar_text),
        status_strip: status_strip.iter().map(|c| hex(*c)).collect(),
        status_strip_dark,
        pill: hex(pill),
        pill_text: hex(ensure(text, best_text(&[pill]), &[pill], TEXT_RATIO)),
        secure: hex(ensure(SECURE, text, &[pill], ICON_RATIO)),
        shape: appearance.shape,
        highlight: appearance.highlight,
    }
}

/// One of Krunk's styles with what a preview of it draws.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ThemePreset {
    pub id: String,
    /// The style's name, the same as on Windows ("Classic", "Sunset", ...).
    pub label: String,
    pub theme: ResolvedTheme,
}

/// Every style (`core::style::PRESETS`, in the order Windows lists them), resolved with `accent`
/// (empty = Krunk's blue) in the light or dark theme.
#[uniffi::export]
pub fn theme_presets(accent: String, dark: bool) -> Vec<ThemePreset> {
    style::PRESETS
        .iter()
        .map(|p| {
            let mut a = Appearance { accent: accent.clone(), ..Appearance::default() };
            p.apply(&mut a);
            ThemePreset {
                id: p.id.to_string(),
                label: p.label.to_string(),
                theme: resolve_theme(ThemeAppearance::from_core(&a), dark, false),
            }
        })
        .collect()
}

/// An accent colour offered in the settings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AccentSwatch {
    /// `#rrggbb`
    pub color: String,
    pub name: String,
}

/// The accents the settings offer: Krunk's blue first, then the ones the Windows settings page
/// offers.
#[uniffi::export]
pub fn accent_swatches() -> Vec<AccentSwatch> {
    [
        (DEFAULT_ACCENT, "Krunk blue"),
        ("#7c3aed", "Violet"),
        ("#db2777", "Pink"),
        ("#dc2626", "Red"),
        ("#ea580c", "Orange"),
        ("#16a34a", "Green"),
        ("#0d9488", "Teal"),
    ]
    .into_iter()
    .map(|(color, name)| AccentSwatch { color: color.to_string(), name: name.to_string() })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_settings, KrunkStore, SettingsError};
    use lb_core::settings::Settings;

    /// The swatches, colours neither black nor white text is comfortable on, and the extremes.
    const ACCENTS: &[&str] = &[
        "", "#2563eb", "#7c3aed", "#db2777", "#dc2626", "#ea580c", "#16a34a", "#0d9488", "#facc15", "#808080",
        "#777777", "#111827", "#f3f4f6", "#000000", "#ffffff", "#00ff00", "#ff00ff",
    ];

    fn of_preset(id: &str, accent: &str) -> ThemeAppearance {
        let a = theme_with_preset(default_settings().appearance, id.into());
        assert_eq!(a.preset, id);
        ThemeAppearance { accent: accent.into(), ..a }
    }

    fn c(hex: &str) -> Rgb {
        parse_hex(hex).unwrap_or_else(|| panic!("not a colour: {hex:?}"))
    }

    /// Every pair the chrome draws, with the ratio it needs.
    fn assert_readable(t: &ResolvedTheme, what: &str) {
        let need = |fg: &str, bg: &str, ratio: f64, pair: &str| {
            let r = contrast(c(fg), c(bg));
            assert!(r >= ratio, "{what}: {pair} {fg} on {bg} is {r:.2}, needs {ratio}");
        };
        // Text on the surfaces: plain, secondary, accent-coloured (text buttons, selected
        // tabs, links) and errors, on the page, on cards and menus, in dialogs and in the pill.
        for (bg, name) in [
            (&t.background, "background"),
            (&t.surface_raised, "raised"),
            (&t.surface_hover, "hover"),
            (&t.pill, "pill"),
        ] {
            need(&t.text, bg, TEXT_RATIO, &format!("text/{name}"));
            need(&t.text_muted, bg, TEXT_RATIO, &format!("muted/{name}"));
            need(&t.primary, bg, TEXT_RATIO, &format!("primary/{name}"));
            need(&t.error, bg, TEXT_RATIO, &format!("error/{name}"));
        }
        need(&t.pill_text, &t.pill, TEXT_RATIO, "pill text");
        // Text on fills.
        need(&t.on_accent, &t.accent, TEXT_RATIO, "on accent");
        need(&t.on_primary, &t.primary, TEXT_RATIO, "on primary");
        need(&t.on_primary_container, &t.primary_container, TEXT_RATIO, "on primary container");
        need(&t.on_error, &t.error, TEXT_RATIO, "on error");
        // The toolbar: its icons need 3:1, the tab counter's number is text.
        assert!(!t.toolbar_colors.is_empty() && !t.status_strip.is_empty(), "{what}");
        for stop in &t.toolbar_colors {
            need(&t.toolbar_text, stop, TEXT_RATIO, "toolbar");
        }
        // Icons and marks: the lock in the pill, and the active card's outline or underline
        // (drawn in `primary`) against the page and the card.
        need(&t.secure, &t.pill, ICON_RATIO, "lock");
        need(&t.primary, &t.background, ICON_RATIO, "mark/background");
        need(&t.primary, &t.surface_raised, ICON_RATIO, "mark/card");
        // Status-bar icons are light exactly when the strip is dark.
        for stop in &t.status_strip {
            let (light, dark) = (contrast(WHITE, c(stop)), contrast(BLACK, c(stop)));
            let (used, other) = if t.status_strip_dark { (light, dark) } else { (dark, light) };
            assert!(used >= ICON_RATIO, "{what}: status icons on {stop}: {used:.2} (the other: {other:.2})");
        }
    }

    #[test]
    fn every_preset_is_readable_in_light_dark_and_private_with_any_accent() {
        let mut checked = 0;
        for p in style::PRESETS {
            for accent in ACCENTS {
                for (dark, private) in [(false, false), (true, false), (false, true), (true, true)] {
                    let t = resolve_theme(of_preset(p.id, accent), dark, private);
                    assert_readable(&t, &format!("{} accent {accent:?} dark {dark} private {private}", p.id));
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, style::PRESETS.len() * ACCENTS.len() * 4);
        assert_eq!(style::PRESETS.len(), 20);
    }

    #[test]
    fn the_previews_list_every_core_preset_in_order() {
        for dark in [false, true] {
            let list = theme_presets(String::new(), dark);
            let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
            assert_eq!(ids, style::PRESETS.iter().map(|p| p.id).collect::<Vec<_>>());
            assert_eq!(list[0].label, "Classic");
            for p in &list {
                let chosen = theme_with_preset(default_settings().appearance, p.id.clone());
                assert_eq!(p.theme, resolve_theme(chosen, dark, false), "{}", p.id);
                assert_eq!(p.theme.dark, dark);
            }
            // No two previews are the same picture: fill, colours, shape or highlight differ.
            for (i, a) in list.iter().enumerate() {
                for b in &list[i + 1..] {
                    assert_ne!(a.theme, b.theme, "{} and {} look the same", a.id, b.id);
                }
            }
        }
        // The accent reaches the previews, except where the style brings its own (0.20.0).
        let pink = theme_presets("#db2777".into(), false);
        for p in &pink {
            let own = style::preset(&p.id).unwrap().colors.accent;
            assert_eq!(p.theme.accent, if own.is_empty() { "#db2777" } else { own }, "{}", p.id);
        }
        // Leaving such a style takes its accent away again.
        let neon = theme_with_preset(default_settings().appearance, "neon".into());
        assert_eq!(neon.accent, "#22f5ff");
        assert_eq!(theme_with_preset(neon, "classic".into()).accent, "");
    }

    #[test]
    fn the_style_maps_to_fill_shape_and_highlight() {
        let t = |id: &str, dark: bool| resolve_theme(of_preset(id, ""), dark, false);
        // Classic is the plain palette (the look before themes), light and dark.
        let classic = t("classic", false);
        assert_eq!(classic.toolbar_fill, ToolbarFill::Solid);
        assert_eq!((classic.background.as_str(), classic.text.as_str()), ("#f3f4f6", "#1f2937"));
        assert_eq!(classic.toolbar_colors, vec!["#ffffff"]);
        assert_eq!((classic.pill.as_str(), classic.toolbar_text.as_str()), ("#e5e7eb", "#1f2937"));
        assert_eq!((classic.accent.as_str(), classic.on_accent.as_str()), ("#2563eb", "#ffffff"));
        assert_eq!((classic.primary_container.as_str(), classic.on_primary_container.as_str()), ("#dbeafe", "#1e3a8a"));
        assert_eq!((classic.status_strip.clone(), classic.status_strip_dark), (vec!["#f3f4f6".to_string()], false));
        assert_eq!((classic.shape, classic.highlight), (ThemeShape::Rounded, ThemeHighlight::Fill));
        let classic_dark = t("classic", true);
        assert_eq!((classic_dark.background.as_str(), classic_dark.accent.as_str()), ("#1f2023", "#60a5fa"));
        assert_eq!(classic_dark.toolbar_colors, vec!["#2b2d31"]);
        assert!(classic_dark.status_strip_dark && classic_dark.dark);
        // No preset is the same as the default look without one.
        assert_eq!(resolve_theme(default_settings().appearance, false, false), classic);

        // Gradient: the preset's stops and angle, text from the core's choice, a raised pill.
        let sunset = t("sunset", false);
        assert_eq!(sunset.toolbar_fill, ToolbarFill::Gradient);
        assert_eq!(sunset.toolbar_colors, vec!["#f97316", "#ec4899", "#8b5cf6"]);
        assert_eq!(sunset.toolbar_angle, 90);
        assert_eq!(sunset.status_strip, sunset.toolbar_colors);
        assert_eq!(sunset.pill, "#ffffff");
        // The core's dark text is 4.2:1 on the violet end; black is used so the counter is readable.
        assert_eq!(color::foreground_on_all(&[c("#f97316"), c("#ec4899"), c("#8b5cf6")]), Some(DARK_TEXT));
        assert_eq!(sunset.toolbar_text, "#000000");
        assert!(!sunset.status_strip_dark);
        // The same gradient in the dark theme: only the base palette changes.
        let sunset_dark = t("sunset", true);
        assert_eq!(sunset_dark.toolbar_colors, sunset.toolbar_colors);
        assert_eq!((sunset_dark.pill.as_str(), sunset_dark.background.as_str()), ("#2b2d31", "#1f2023"));

        let ocean = t("ocean", false);
        assert_eq!(ocean.toolbar_colors, vec!["#0ea5e9", "#14b8a6"]);
        assert_eq!(
            (ocean.toolbar_angle, ocean.shape, ocean.highlight),
            (135, ThemeShape::Pill, ThemeHighlight::Outline)
        );
        assert_eq!(ocean.toolbar_text, DARK_TEXT);
        let midnight = t("midnight", false);
        assert_eq!(midnight.toolbar_text, LIGHT_TEXT);
        // A dark gradient in the light theme: the status-bar icons follow the strip, not the theme.
        assert!(midnight.status_strip_dark && !midnight.dark);
        assert_eq!(midnight.highlight, ThemeHighlight::Underline);
        // Ember's last colour derives from its first (the core's rule).
        assert_eq!(t("ember", false).toolbar_colors.len(), 3);

        // Glass: the surface and the pill tinted with the accent, no gradient.
        let glass = t("glass", false);
        assert_eq!(glass.toolbar_fill, ToolbarFill::Glass);
        assert_eq!(glass.toolbar_colors, vec![to_hex(mix(WHITE, c("#2563eb"), GLASS_TINT_LIGHT))]);
        assert_eq!(glass.status_strip, glass.toolbar_colors);
        assert_ne!(glass.pill, classic.pill);
        assert_eq!(glass.highlight, ThemeHighlight::None);
        let green_glass = resolve_theme(of_preset("glass", "#16a34a"), true, false);
        assert_eq!(green_glass.toolbar_colors, vec![to_hex(mix(c("#2b2d31"), c("#16a34a"), GLASS_TINT_DARK))]);
        assert_eq!(green_glass.accent, "#16a34a");

        let mono = t("mono", false);
        assert_eq!(
            (mono.shape, mono.highlight, mono.toolbar_fill),
            (ThemeShape::Square, ThemeHighlight::Underline, ToolbarFill::Solid)
        );
        assert_eq!(
            (t("neon", false).shape, t("neon", false).highlight),
            (ThemeShape::Rounded, ThemeHighlight::Outline)
        );
    }

    #[test]
    fn the_accent_is_kept_for_fills_and_made_readable_as_text() {
        // A light accent in the light theme: fills keep it (with dark text on them), text and
        // thin marks get a darker variant that reaches 4.5:1 on every surface.
        let t = resolve_theme(of_preset("classic", "#facc15"), false, false);
        assert_eq!((t.accent.as_str(), t.on_accent.as_str()), ("#facc15", DARK_TEXT));
        assert_ne!(t.primary, "#facc15");
        assert!(contrast(c(&t.primary), c("#e5e7eb")) >= TEXT_RATIO);
        assert!(contrast(c("#facc15"), c("#e5e7eb")) < 2.0);
        // An accent that is readable already stays as it is.
        let violet = resolve_theme(of_preset("classic", "#7c3aed"), false, false);
        assert_eq!(violet.primary, "#7c3aed");
        // The same accent in the dark theme is lightened instead.
        let violet_dark = resolve_theme(of_preset("classic", "#7c3aed"), true, false);
        assert!(luminance(c(&violet_dark.primary)) > luminance(c("#7c3aed")));
        assert_eq!(violet_dark.accent, "#7c3aed");
        // Krunk's own blue moves only a little (it was 4.1:1 on dialogs).
        let own = resolve_theme(of_preset("classic", ""), false, false);
        let (r, g, b) = c(&own.primary);
        let (r0, g0, b0) = c(DEFAULT_ACCENT);
        assert!(r.abs_diff(r0) <= 8 && g.abs_diff(g0) <= 12 && b.abs_diff(b0) <= 30, "{}", own.primary);
        // A mid grey no dark or light text reaches 4.5:1 on gets black.
        let grey = resolve_theme(of_preset("classic", "#777777"), false, false);
        assert_eq!(color::foreground("#777777"), Some(LIGHT_TEXT));
        assert!(contrast(c(LIGHT_TEXT), c("#777777")) < TEXT_RATIO);
        assert_eq!(grey.on_accent, "#000000");
        // Something that is not a colour counts as no accent.
        assert_eq!(resolve_theme(of_preset("classic", "blue"), false, false), own);
    }

    #[test]
    fn private_tabs_look_the_same_in_every_theme_and_unlike_all_of_them() {
        let base = resolve_theme(of_preset("classic", ""), false, true);
        assert!(base.dark && base.private);
        assert_eq!((base.background.as_str(), base.accent.as_str()), ("#1b1526", "#c4b5fd"));
        assert_eq!(base.toolbar_colors, vec!["#2a2140"]);
        assert_eq!(base.status_strip, vec!["#1b1526"]);
        for p in style::PRESETS {
            for accent in ACCENTS {
                for dark in [false, true] {
                    let private = resolve_theme(of_preset(p.id, accent), dark, true);
                    // Only the shape and the highlight follow the style.
                    let same_palette =
                        ResolvedTheme { shape: base.shape, highlight: base.highlight, ..private.clone() };
                    assert_eq!(same_palette, base, "{} {accent}", p.id);
                    assert_eq!(private.toolbar_fill, ToolbarFill::Solid);
                    let normal = resolve_theme(of_preset(p.id, accent), dark, false);
                    assert_ne!(normal.background, private.background);
                    assert_ne!(normal.toolbar_colors, private.toolbar_colors);
                    assert_ne!(normal.pill, private.pill);
                    assert!(!normal.toolbar_colors.contains(&private.toolbar_colors[0]));
                }
            }
        }
    }

    #[test]
    fn gradients_made_by_hand_stay_readable() {
        let hand = |from: &str, via: &str, to: &str, accent: &str| ThemeAppearance {
            preset: String::new(),
            accent: accent.into(),
            toolbar_fill: ToolbarFill::Gradient,
            gradient_from: from.into(),
            gradient_via: via.into(),
            gradient_to: to.into(),
            gradient_angle: 45,
            shape: ThemeShape::Rounded,
            highlight: ThemeHighlight::Fill,
        };
        // Black to white: no text colour works, so the gradient is toned until one does.
        for dark in [false, true] {
            let t = resolve_theme(hand("#000000", "", "#ffffff", ""), dark, false);
            assert_eq!(t.toolbar_colors.len(), 2);
            assert_ne!(t.toolbar_colors, vec!["#000000", "#ffffff"]);
            assert_eq!(t.toolbar_angle, 45);
            assert_readable(&t, "black to white");
        }
        // Empty colours derive from the accent (the core's rule); a sweep of accents and stops.
        for accent in ACCENTS {
            for (from, via, to) in
                [("", "", ""), ("", "#808080", ""), ("#ff0000", "", "#0000ff"), ("#777777", "#ffffff", "")]
            {
                for dark in [false, true] {
                    let t = resolve_theme(hand(from, via, to, accent), dark, false);
                    assert_readable(&t, &format!("hand-made {from:?} {via:?} {to:?} accent {accent:?}"));
                }
            }
        }
        // A readable gradient is left alone.
        let t = resolve_theme(hand("#0f172a", "", "#312e81", ""), false, false);
        assert_eq!(t.toolbar_colors, vec!["#0f172a", "#312e81"]);
    }

    #[test]
    fn choosing_an_accent() {
        let a = default_settings().appearance;
        assert_eq!(theme_with_accent(a.clone(), "#DB2777".into()).accent, "#db2777");
        assert_eq!(theme_with_accent(a.clone(), "#abc".into()).accent, "#aabbcc");
        // Krunk's blue is "no accent" (so the dark theme keeps its lighter blue).
        let pink = theme_with_accent(a.clone(), "#db2777".into());
        assert_eq!(theme_with_accent(pink.clone(), "#2563EB".into()).accent, "");
        assert_eq!(theme_with_accent(pink.clone(), " ".into()).accent, "");
        assert_eq!(theme_with_accent(pink.clone(), "pink".into()), pink);
        // The first swatch is that blue; all are colours with a name, none twice.
        let swatches = accent_swatches();
        assert_eq!(swatches[0].color, DEFAULT_ACCENT);
        assert_eq!(swatches.len(), 7);
        for (i, s) in swatches.iter().enumerate() {
            assert_eq!(parse_hex(&s.color).map(to_hex).as_deref(), Some(s.color.as_str()));
            assert!(!s.name.is_empty() && swatches[i + 1..].iter().all(|o| o.color != s.color && o.name != s.name));
        }
        // An unknown style changes nothing; a known one keeps the accent.
        assert_eq!(theme_with_preset(pink.clone(), "nope".into()), pink);
        let ocean = theme_with_preset(pink, "ocean".into());
        assert_eq!((ocean.preset.as_str(), ocean.accent.as_str()), ("ocean", "#db2777"));
        assert_eq!(
            (ocean.toolbar_fill, ocean.gradient_angle, ocean.shape),
            (ToolbarFill::Gradient, 135, ThemeShape::Pill)
        );
    }

    #[test]
    fn saving_goes_through_the_preset_and_keeps_what_android_does_not_know() {
        let store = KrunkStore::in_memory().unwrap();
        // Desktop-only appearance fields already stored.
        let mut core = Settings::default();
        core.appearance.ui_font_px = 15;
        core.appearance.tab_radius = 3;
        core.appearance.toolbar_color = "#0b1220".into();
        core.appearance.show_zoom_chip = false;
        core.appearance.active_outline_width = 4;
        store.kv_set("settings".into(), serde_json::to_string(&core).unwrap()).unwrap();

        let mut s = store.load_settings().unwrap();
        assert_eq!(s.appearance, default_settings().appearance);
        s.appearance = theme_with_accent(theme_with_preset(s.appearance, "sunset".into()), "#16a34a".into());
        store.save_settings(s.clone()).unwrap();
        // What Kotlin holds after the save is what a load returns.
        assert_eq!(store.load_settings().unwrap(), s);

        let raw = || -> Settings { serde_json::from_str(&store.kv_get("settings".into()).unwrap().unwrap()).unwrap() };
        let a = raw().appearance;
        // The whole preset, as `Preset::apply` writes it on Windows (the tab strip's ombre too).
        let mut expected = core.appearance.clone();
        style::preset("sunset").unwrap().apply(&mut expected);
        expected.accent = "#16a34a".into();
        assert_eq!(a, expected);
        assert_eq!((a.tab_style, a.accent_glow), (cs::BarStyle::Ombre, true));
        assert_eq!((a.ui_font_px, a.tab_radius, a.toolbar_color.as_str()), (15, 3, "#0b1220"));
        assert!(!a.show_zoom_chip && a.active_outline_width == 4);
        let json = store.kv_get("settings".into()).unwrap().unwrap();
        assert!(json.contains("\"preset\":\"sunset\"") && json.contains("\"toolbarStyle\":\"gradient\""), "{json}");

        // The style was then changed by hand on Windows (same preset id): an Android save that
        // only changes the accent leaves it alone, and the theme shows the changed gradient.
        let mut tweaked = raw();
        tweaked.appearance.gradient_from = "#111111".into();
        tweaked.appearance.gradient_angle = 200;
        tweaked.appearance.tab_style = cs::BarStyle::Glass;
        store.kv_set("settings".into(), serde_json::to_string(&tweaked).unwrap()).unwrap();
        let mut s = store.load_settings().unwrap();
        assert_eq!((s.appearance.gradient_from.as_str(), s.appearance.gradient_angle), ("#111111", 200));
        assert_eq!(resolve_theme(s.appearance.clone(), false, false).toolbar_angle, 200);
        s.appearance = theme_with_accent(s.appearance, "#db2777".into());
        store.save_settings(s).unwrap();
        let a = raw().appearance;
        assert_eq!((a.gradient_from.as_str(), a.gradient_angle, a.tab_style), ("#111111", 200, cs::BarStyle::Glass));
        assert_eq!(a.accent, "#db2777");

        // Choosing another style replaces the hand-made values.
        let mut s = store.load_settings().unwrap();
        s.appearance = theme_with_preset(s.appearance, "mono".into());
        store.save_settings(s).unwrap();
        let a = raw().appearance;
        assert_eq!((a.preset.as_str(), a.tab_shape, a.rounded_tabs), ("mono", cs::TabShape::Square, false));
        assert!(a.gradient_from.is_empty() && a.gradient_angle == 90 && a.tab_style == cs::BarStyle::Solid);
        assert_eq!((a.accent.as_str(), a.ui_font_px), ("#db2777", 15));

        // The core validates: an unknown style or a bad colour is rejected and nothing is written.
        let mut bad = store.load_settings().unwrap();
        bad.appearance.preset = "nope".into();
        bad.appearance.accent = "red".into();
        match store.save_settings(bad).unwrap_err() {
            SettingsError::Validation { errors } => {
                let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert_eq!(fields, vec!["appearance.accent", "appearance.preset"]);
            }
            other => panic!("expected a validation error, got {other:?}"),
        }
        assert_eq!(raw().appearance, a);
    }
}
