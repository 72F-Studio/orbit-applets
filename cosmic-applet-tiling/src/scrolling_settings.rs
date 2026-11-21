// SPDX-License-Identifier: GPL-3.0-only

//! Orbit: pure helpers for the applet's Scrolling settings.

use cosmic_comp_config::ScrollingConfig;

/// One of the new-window width choices shown in the applet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidthChoice {
    /// Index into `ScrollingConfig::NEW_COLUMN_WIDTH_PRESETS`.
    Preset(usize),
    Custom,
}

/// Which choice a stored width belongs to. Values within half a percent of a
/// preset count as that preset, so float round-trips don't flip to Custom.
pub fn width_choice(width: f64) -> WidthChoice {
    ScrollingConfig::NEW_COLUMN_WIDTH_PRESETS
        .iter()
        .position(|preset| (preset - width).abs() < 0.005)
        .map_or(WidthChoice::Custom, WidthChoice::Preset)
}

/// Snap a slider value to whole percent inside the allowed range.
pub fn snap_width(value: f64) -> f64 {
    let value = if value.is_finite() { value } else { 0.5 };
    ((value * 100.0).round() / 100.0).clamp(ScrollingConfig::MIN_COLUMN_WIDTH, 1.0)
}

/// Snap a threshold slider value to whole percent inside the allowed range.
pub fn snap_threshold(value: f64) -> f64 {
    let value = if value.is_finite() { value } else { 0.6 };
    ((value * 100.0).round() / 100.0).clamp(0.1, 1.0)
}

/// "75%" for 0.75.
pub fn percent(value: f64) -> String {
    format!("{}%", (value * 100.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_map_to_their_index_and_everything_else_is_custom() {
        assert_eq!(width_choice(0.25), WidthChoice::Preset(0));
        assert_eq!(width_choice(0.5), WidthChoice::Preset(1));
        assert_eq!(width_choice(0.7501), WidthChoice::Preset(2));
        assert_eq!(width_choice(0.9), WidthChoice::Preset(3));
        assert_eq!(width_choice(0.66), WidthChoice::Custom);
        assert_eq!(width_choice(1.0), WidthChoice::Custom);
    }

    #[test]
    fn sliders_snap_to_whole_percent_and_stay_in_range() {
        assert_eq!(snap_width(0.6649), 0.66);
        assert_eq!(snap_width(0.1), 0.25);
        assert_eq!(snap_width(f64::NAN), 0.5);
        assert_eq!(snap_threshold(0.05), 0.1);
        assert_eq!(snap_threshold(1.4), 1.0);
    }

    #[test]
    fn percent_formats_whole_numbers() {
        assert_eq!(percent(0.25), "25%");
        assert_eq!(percent(0.666), "67%");
    }
}
