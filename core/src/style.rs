//! Tab and toolbar styles (0.15.0): the curated presets behind `appearance.preset`. Choosing a
//! preset copies its values into the user's appearance settings; the user can change them after.

use crate::settings::{ActiveHighlight, Appearance, BarStyle, TabShape};

/// A curated look. Empty colours keep the user's/theme's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub tab_style: BarStyle,
    pub toolbar_style: BarStyle,
    pub gradient: [&'static str; 3],
    pub gradient_angle: u16,
    pub tab_shape: TabShape,
    pub active_highlight: ActiveHighlight,
    pub accent_glow: bool,
    /// 0.20.0: colours a style brings along (Neon is not neon without them).
    pub colors: PresetColors,
}

/// A style's own colours (`#rrggbb`). An empty one leaves the user's colour alone, except that
/// a colour the previous style had set is cleared again (so Neon's cyan does not stay behind).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetColors {
    pub accent: &'static str,
    /// `Appearance::active_outline_color`.
    pub outline: &'static str,
    /// The tab titles' colours. No built-in style sets them: they would also colour private
    /// tabs' titles, which have their own readable colours.
    pub tab_text: &'static str,
    pub active_tab_text: &'static str,
}

const NO_COLORS: PresetColors = PresetColors { accent: "", outline: "", tab_text: "", active_tab_text: "" };

pub const PRESETS: &[Preset] = &[
    Preset {
        id: "classic",
        label: "Classic",
        tab_style: BarStyle::Solid,
        toolbar_style: BarStyle::Solid,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "sunset",
        label: "Sunset",
        tab_style: BarStyle::Ombre,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#f97316", "#ec4899", "#8b5cf6"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: true,
        colors: NO_COLORS,
    },
    Preset {
        id: "ocean",
        label: "Ocean",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#0ea5e9", "", "#14b8a6"],
        gradient_angle: 135,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "glass",
        label: "Glass",
        tab_style: BarStyle::Glass,
        toolbar_style: BarStyle::Glass,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::None,
        accent_glow: true,
        colors: NO_COLORS,
    },
    Preset {
        id: "mono",
        label: "Mono",
        tab_style: BarStyle::Solid,
        toolbar_style: BarStyle::Solid,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Square,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "aurora",
        label: "Aurora",
        tab_style: BarStyle::Ombre,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#22d3ee", "#a78bfa", "#f472b6"],
        gradient_angle: 120,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::None,
        accent_glow: true,
        colors: NO_COLORS,
    },
    Preset {
        id: "midnight",
        label: "Midnight",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#0f172a", "#1e3a8a", "#312e81"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: true,
        colors: NO_COLORS,
    },
    Preset {
        id: "forest",
        label: "Forest",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#14532d", "#166534", "#0f766e"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "ember",
        label: "Ember",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#7f1d1d", "#c2410c", ""],
        gradient_angle: 135,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: true,
        colors: NO_COLORS,
    },
    Preset {
        id: "candy",
        label: "Candy",
        tab_style: BarStyle::Ombre,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#f9a8d4", "#c4b5fd", "#93c5fd"],
        gradient_angle: 90,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "frost",
        label: "Frost",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#dbeafe", "#f0f9ff", "#bae6fd"],
        gradient_angle: 100,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: true,
        colors: PresetColors { accent: "#38bdf8", outline: "#ffffff", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "neon",
        label: "Neon",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#0a0a14", "#170a2e", "#0a0a14"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: true,
        colors: PresetColors { accent: "#22f5ff", outline: "#ff2bd6", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "synthwave",
        label: "Synthwave",
        tab_style: BarStyle::Ombre,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#2e1065", "#a21caf", "#be185d"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: true,
        colors: PresetColors { accent: "#f472b6", outline: "#22d3ee", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "matrix",
        label: "Matrix",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#020a04", "#06200e", "#020a04"],
        gradient_angle: 90,
        tab_shape: TabShape::Square,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: true,
        colors: PresetColors { accent: "#22c55e", outline: "", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "graphite",
        label: "Graphite",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#111827", "#374151", "#1f2937"],
        gradient_angle: 90,
        tab_shape: TabShape::Square,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: false,
        colors: NO_COLORS,
    },
    Preset {
        id: "citrus",
        label: "Citrus",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#fde047", "", "#fb923c"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: false,
        colors: PresetColors { accent: "#ea580c", outline: "", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "mint",
        label: "Mint",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#d1fae5", "#a7f3d0", "#99f6e4"],
        gradient_angle: 120,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: false,
        colors: PresetColors { accent: "#059669", outline: "", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "sakura",
        label: "Sakura",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#fce7f3", "#fff1f2", "#fbcfe8"],
        gradient_angle: 90,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: true,
        colors: PresetColors { accent: "#ec4899", outline: "", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "coffee",
        label: "Coffee",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#3f2a1d", "#6b4423", "#8a5a36"],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Fill,
        accent_glow: false,
        colors: PresetColors { accent: "#d6a05c", outline: "", tab_text: "", active_tab_text: "" },
    },
    Preset {
        id: "royal",
        label: "Royal",
        tab_style: BarStyle::Gradient,
        toolbar_style: BarStyle::Gradient,
        gradient: ["#3b0764", "#6b21a8", "#4c1d95"],
        gradient_angle: 135,
        tab_shape: TabShape::Rounded,
        active_highlight: ActiveHighlight::Underline,
        accent_glow: true,
        colors: PresetColors { accent: "#fbbf24", outline: "", tab_text: "", active_tab_text: "" },
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl Preset {
    /// Copies the preset into `a` and remembers its id.
    pub fn apply(&self, a: &mut Appearance) {
        let old = preset(&a.preset).map_or(NO_COLORS, |p| p.colors);
        let set = |cur: &mut String, old: &str, new: &str| {
            if !new.is_empty() {
                *cur = new.to_string();
            } else if !old.is_empty() && cur.eq_ignore_ascii_case(old) {
                cur.clear();
            }
        };
        set(&mut a.accent, old.accent, self.colors.accent);
        set(&mut a.active_outline_color, old.outline, self.colors.outline);
        set(&mut a.tab_text_color, old.tab_text, self.colors.tab_text);
        set(&mut a.active_tab_text_color, old.active_tab_text, self.colors.active_tab_text);
        a.preset = self.id.to_string();
        a.tab_style = self.tab_style;
        a.toolbar_style = self.toolbar_style;
        a.gradient_from = self.gradient[0].to_string();
        a.gradient_via = self.gradient[1].to_string();
        a.gradient_to = self.gradient[2].to_string();
        a.gradient_angle = self.gradient_angle;
        a.tab_shape = self.tab_shape;
        // Older versions read only `roundedTabs`.
        a.rounded_tabs = self.tab_shape != TabShape::Square;
        a.active_highlight = self.active_highlight;
        a.accent_glow = self.accent_glow;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn every_preset_is_valid_in_settings() {
        for p in PRESETS {
            let mut s = Settings::default();
            p.apply(&mut s.appearance);
            assert_eq!(s.validate(), Ok(()), "preset {}", p.id);
        }
        assert!(preset("nope").is_none());
    }

    #[test]
    fn applying_a_preset_copies_every_style_value_and_keeps_the_rest() {
        let mut a = Appearance {
            accent: "#16a34a".into(),
            gradient_via: "#123456".into(),
            tab_radius: 3,
            ui_font_px: 15,
            ..Default::default()
        };
        preset("sunset").unwrap().apply(&mut a);
        assert_eq!(a.preset, "sunset");
        assert_eq!((a.tab_style, a.toolbar_style), (BarStyle::Ombre, BarStyle::Gradient));
        assert_eq!([a.gradient_from.as_str(), &a.gradient_via, &a.gradient_to], ["#f97316", "#ec4899", "#8b5cf6"]);
        assert!(a.accent_glow && a.rounded_tabs);
        // Colours, the font and the user's radius are not part of a style.
        assert_eq!((a.accent.as_str(), a.ui_font_px, a.tab_radius), ("#16a34a", 15, 3));
        // An empty preset colour clears the user's (it derives from the accent again).
        preset("ocean").unwrap().apply(&mut a);
        assert!(a.gradient_via.is_empty() && a.gradient_angle == 135 && a.tab_shape == TabShape::Pill);
        preset("mono").unwrap().apply(&mut a);
        assert!(!a.rounded_tabs && a.active_highlight == ActiveHighlight::Underline && !a.accent_glow);
        // 0.20.0: a style's own colours come with it and leave with it; the user's stay.
        preset("neon").unwrap().apply(&mut a);
        assert_eq!((a.accent.as_str(), a.active_outline_color.as_str()), ("#22f5ff", "#ff2bd6"));
        preset("matrix").unwrap().apply(&mut a);
        assert_eq!((a.accent.as_str(), a.active_outline_color.as_str()), ("#22c55e", ""));
        preset("frost").unwrap().apply(&mut a);
        assert_eq!((a.accent.as_str(), a.active_outline_color.as_str()), ("#38bdf8", "#ffffff"));
        a.accent = "#16a34a".into();
        preset("classic").unwrap().apply(&mut a);
        assert_eq!((a.accent.as_str(), a.active_outline_color.as_str()), ("#16a34a", ""));
        // Ids are unique, labels set, and ombre only on the tab strip.
        for (i, p) in PRESETS.iter().enumerate() {
            assert!(!p.label.is_empty() && PRESETS[i + 1..].iter().all(|q| q.id != p.id));
            assert_ne!(p.toolbar_style, BarStyle::Ombre, "{}", p.id);
        }
    }
}
