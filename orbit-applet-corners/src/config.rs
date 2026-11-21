// SPDX-License-Identifier: GPL-3.0-only

//! What each control writes.
//!
//! The dock and the top panel each keep their own `border_radius` in
//! cosmic-panel's config. cosmic-panel watches those files and animates.
//!
//! Everything else reads one of the theme's shared radius tokens. Settings
//! rewrites all of them whenever Roundness changes, and rebuilds the theme from
//! its Builder on any other appearance change, so a pinned token is written to
//! the Dark and Light themes and to both Builders.

use cosmic::cosmic_theme::{
    CornerRadii, DARK_THEME_BUILDER_ID, DARK_THEME_ID, LIGHT_THEME_BUILDER_ID, LIGHT_THEME_ID,
};
use cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};
use serde::{Deserialize, Serialize};

pub const APPLET_CONFIG_ID: &str = "com.studio72f.OrbitCorners";

/// The slider's range. 12 to 24 is the GNOME and macOS look.
pub const SLIDER_MAX: u32 = 32;
/// What Settings writes for Roundness: Round. Panels and buttons clamp it to
/// half their height, which gives the pill.
pub const PILL: u32 = 160;
/// Where the slider lands when there's no earlier value under the pill.
pub const DEFAULT_RADIUS: u32 = 16;

/// Every config that holds the theme's corner radii.
pub const THEME_CONFIG_IDS: [&str; 4] = [
    DARK_THEME_ID,
    LIGHT_THEME_ID,
    DARK_THEME_BUILDER_ID,
    LIGHT_THEME_BUILDER_ID,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Dock,
    Panel,
    /// `radius_xl`: buttons, toggles, sliders, search fields.
    Buttons,
    /// `radius_m`: popups, menus, dropdowns, dialogs.
    Popups,
    /// `radius_s`: windows, text fields, lists, Orbit's window overlays.
    Windows,
    /// `radius_l`: applet popups and tooltips.
    AppletPopups,
    /// `radius_xs`: cards and small details.
    Cards,
}

impl Target {
    pub const ALL: [Target; 7] = [
        Target::Dock,
        Target::Panel,
        Target::Buttons,
        Target::Popups,
        Target::Windows,
        Target::AppletPopups,
        Target::Cards,
    ];
    pub const BARS: [Target; 2] = [Target::Dock, Target::Panel];
    pub const THEME: [Target; 5] = [
        Target::Buttons,
        Target::Popups,
        Target::Windows,
        Target::AppletPopups,
        Target::Cards,
    ];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap()
    }

    /// The cosmic-panel config for a bar; `None` for a theme token.
    pub fn bar_config_id(self) -> Option<&'static str> {
        match self {
            Target::Dock => Some("com.system76.CosmicPanel.Dock"),
            Target::Panel => Some("com.system76.CosmicPanel.Panel"),
            _ => None,
        }
    }

    /// Only shapes that are wider than tall can be a pill.
    pub fn allows_pill(self) -> bool {
        matches!(self, Target::Dock | Target::Panel | Target::Buttons)
    }
}

/// A theme token's value, as one number. Settings always writes all four
/// corners the same.
pub fn token(radii: &CornerRadii, target: Target) -> Option<u32> {
    let corners = match target {
        Target::Buttons => radii.radius_xl,
        Target::Popups => radii.radius_m,
        Target::Windows => radii.radius_s,
        Target::AppletPopups => radii.radius_l,
        Target::Cards => radii.radius_xs,
        Target::Dock | Target::Panel => return None,
    };
    Some(corners[0].max(0.0).round() as u32)
}

pub fn set_token(radii: &mut CornerRadii, target: Target, value: u32) {
    let v = [value as f32; 4];
    match target {
        Target::Buttons => radii.radius_xl = v,
        Target::Popups => radii.radius_m = v,
        Target::Windows => radii.radius_s = v,
        Target::AppletPopups => radii.radius_l = v,
        Target::Cards => radii.radius_xs = v,
        Target::Dock | Target::Panel => {}
    }
}

/// Only `border_radius`, so writing it never touches a bar's other keys.
/// The version matches `CosmicPanelConfig`'s, which stores its keys under `v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, CosmicConfigEntry)]
#[version = 1]
pub struct BarCorners {
    pub border_radius: u32,
}

impl Default for BarCorners {
    fn default() -> Self {
        // cosmic-panel's own default.
        Self { border_radius: 8 }
    }
}

/// Only `corner_radii` of a theme, for watching. The version matches
/// `cosmic_theme::Theme::VERSION` (checked in a test).
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize, CosmicConfigEntry)]
#[version = 2]
pub struct ThemeCorners {
    pub corner_radii: CornerRadii,
}

/// The applet's own settings. `None` follows Settings' Roundness; `Some` is a
/// radius the user pinned, which the applet puts back if Settings overwrites it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, CosmicConfigEntry)]
#[version = 1]
pub struct CornersConfig {
    pub dock: Option<u32>,
    pub panel: Option<u32>,
    pub buttons: Option<u32>,
    pub popups: Option<u32>,
    pub windows: Option<u32>,
    pub applet_popups: Option<u32>,
    pub cards: Option<u32>,
}

impl CornersConfig {
    fn slot(&mut self, target: Target) -> &mut Option<u32> {
        match target {
            Target::Dock => &mut self.dock,
            Target::Panel => &mut self.panel,
            Target::Buttons => &mut self.buttons,
            Target::Popups => &mut self.popups,
            Target::Windows => &mut self.windows,
            Target::AppletPopups => &mut self.applet_popups,
            Target::Cards => &mut self.cards,
        }
    }

    pub fn pin(&self, target: Target) -> Option<u32> {
        *self.clone().slot(target)
    }

    pub fn set_pin(&mut self, target: Target, value: Option<u32>) {
        *self.slot(target) = value;
    }
}

pub fn is_pill(radius: u32) -> bool {
    radius > SLIDER_MAX
}

/// Marks drawn on every slider: the GNOME and macOS range.
pub const SLIDER_MARKS: [u32; 2] = [12, 24];

/// Where a radius sits on a slider. Shapes that allow a pill get one extra
/// notch past `SLIDER_MAX`, and that notch is the pill.
pub fn slider_pos(radius: u32) -> u32 {
    radius.min(SLIDER_MAX + 1)
}

pub fn slider_top(target: Target) -> u32 {
    if target.allows_pill() {
        SLIDER_MAX + 1
    } else {
        SLIDER_MAX
    }
}

pub fn from_slider(target: Target, pos: u32) -> u32 {
    if target.allows_pill() && pos > SLIDER_MAX {
        PILL
    } else {
        pos.min(SLIDER_MAX)
    }
}

/// The radius to write when a config changes from outside, or `None` to
/// leave it alone.
pub fn reapply(pin: Option<u32>, current: u32) -> Option<u32> {
    pin.filter(|&pinned| pinned != current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_above_the_slider_read_as_pill() {
        assert!(!is_pill(0));
        assert!(!is_pill(SLIDER_MAX));
        assert!(is_pill(SLIDER_MAX + 1));
        assert!(is_pill(PILL));
    }

    #[test]
    fn the_last_notch_is_the_pill_only_where_allowed() {
        assert_eq!(slider_top(Target::Dock), SLIDER_MAX + 1);
        assert_eq!(slider_top(Target::Windows), SLIDER_MAX);
        assert_eq!(from_slider(Target::Dock, SLIDER_MAX + 1), PILL);
        assert_eq!(from_slider(Target::Windows, SLIDER_MAX + 1), SLIDER_MAX);
        assert_eq!(from_slider(Target::Buttons, 16), 16);
        assert_eq!(slider_pos(PILL), SLIDER_MAX + 1);
        assert_eq!(slider_pos(16), 16);
    }

    #[test]
    fn only_a_changed_pinned_value_is_rewritten() {
        assert_eq!(reapply(None, 160), None);
        assert_eq!(reapply(Some(16), 16), None);
        assert_eq!(reapply(Some(16), 160), Some(16));
    }

    #[test]
    fn pins_are_per_target() {
        let mut c = CornersConfig::default();
        c.set_pin(Target::Dock, Some(16));
        c.set_pin(Target::Windows, Some(12));
        assert_eq!(c.pin(Target::Dock), Some(16));
        assert_eq!(c.pin(Target::Windows), Some(12));
        assert_eq!(c.pin(Target::Panel), None);
        assert_eq!(c.pin(Target::Buttons), None);
    }

    #[test]
    fn tokens_round_trip_and_leave_the_others_alone() {
        let mut radii = CornerRadii::default();
        set_token(&mut radii, Target::Popups, 12);
        assert_eq!(token(&radii, Target::Popups), Some(12));
        assert_eq!(radii.radius_m, [12.0; 4]);
        assert_eq!(token(&radii, Target::Buttons), Some(160));
        assert_eq!(token(&radii, Target::Windows), Some(8));
        assert_eq!(token(&radii, Target::Dock), None);
    }

    #[test]
    fn theme_version_matches_libcosmic() {
        assert_eq!(ThemeCorners::VERSION, cosmic::cosmic_theme::Theme::VERSION);
    }

    #[test]
    fn every_target_has_a_slot() {
        for (i, t) in Target::ALL.iter().enumerate() {
            assert_eq!(t.index(), i);
        }
        assert_eq!(Target::BARS.len() + Target::THEME.len(), Target::ALL.len());
    }
}
