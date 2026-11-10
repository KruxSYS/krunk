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
        active_highlight: ActiveHighlight::Underline,
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
}
