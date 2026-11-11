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
}

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
    },
    Preset {
        id: "glass",
        label: "Glass",
        tab_style: BarStyle::Glass,
        toolbar_style: BarStyle::Glass,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Rounded,
        // 0.15.2: the glow alone marks the active tab.
        active_highlight: ActiveHighlight::None,
        accent_glow: true,
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
    },
    // 0.15.2
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
    },
    Preset {
        id: "frost",
        label: "Frost",
        tab_style: BarStyle::Glass,
        toolbar_style: BarStyle::Glass,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Pill,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: false,
    },
    Preset {
        id: "neon",
        label: "Neon",
        tab_style: BarStyle::Solid,
        toolbar_style: BarStyle::Solid,
        gradient: ["", "", ""],
        gradient_angle: 90,
        tab_shape: TabShape::Square,
        active_highlight: ActiveHighlight::Outline,
        accent_glow: true,
    },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl Preset {
    /// Copies the preset into `a` and remembers its id.
    pub fn apply(&self, a: &mut Appearance) {
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
        // Ids are unique, labels set, and ombre only on the tab strip.
        for (i, p) in PRESETS.iter().enumerate() {
            assert!(!p.label.is_empty() && PRESETS[i + 1..].iter().all(|q| q.id != p.id));
            assert_ne!(p.toolbar_style, BarStyle::Ombre, "{}", p.id);
        }
    }
}
