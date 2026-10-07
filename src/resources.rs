//! Resources: document-wide settings that aren't attached to any entity.

use eframe::egui::{Pos2, pos2};
use serde::{Deserialize, Serialize};

/// Rounds `v` to the nearest multiple of `step`.
pub fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

pub fn snap_pos(p: Pos2, step: f32) -> Pos2 {
    pos2(snap(p.x, step), snap(p.y, step))
}

pub const DEFAULT_STEP: f32 = 4.0;

/// Which properties snap to the grid step. Each `*_step` method returns the
/// step to snap that property to, or 0 (no snapping) when it is turned off.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapSettings {
    pub step: f32,
    pub position: bool,
    pub width: bool,
    pub height: bool,
    pub radius: bool,
    pub font_size: bool,
    pub stroke: bool,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            step: DEFAULT_STEP,
            position: true,
            width: true,
            height: true,
            radius: true,
            font_size: false,
            stroke: false,
        }
    }
}

impl SnapSettings {
    fn when(&self, on: bool) -> f32 {
        if on { self.step } else { 0.0 }
    }

    pub fn position_step(&self) -> f32 {
        self.when(self.position)
    }

    pub fn width_step(&self) -> f32 {
        self.when(self.width)
    }

    pub fn height_step(&self) -> f32 {
        self.when(self.height)
    }

    pub fn radius_step(&self) -> f32 {
        self.when(self.radius)
    }

    pub fn font_size_step(&self) -> f32 {
        self.when(self.font_size)
    }

    pub fn stroke_step(&self) -> f32 {
        self.when(self.stroke)
    }
}

/// Background look: shimmer multipliers of the default, plus 100% reference dot and line contrast.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shimmer {
    pub speed: f32,
    /// Per-dot twinkle strength.
    #[serde(alias = "intensity")]
    pub twinkle: f32,
    /// Drifting wave strength across the canvas.
    pub wave: f32,
    /// Contrast ratio of the dots shown at 100% zoom, against the canvas.
    pub reference: f32,
    /// Contrast ratio of the grid and triangle lines shown at 100% zoom.
    pub reference_lines: f32,
    /// Dot diameter in screen pixels, the same at every zoom.
    pub dot_size: f32,
    /// Line width in screen pixels, the same at every zoom.
    pub line_width: f32,
}

impl Default for Shimmer {
    fn default() -> Self {
        Self {
            speed: 1.0,
            twinkle: 1.0,
            wave: 1.0,
            reference: 2.8,
            reference_lines: 1.8,
            dot_size: 2.0,
            line_width: 1.0,
        }
    }
}

impl Shimmer {
    /// True when the dots actually animate.
    pub fn active(&self) -> bool {
        self.speed > 0.0 && (self.twinkle > 0.0 || self.wave > 0.0)
    }
}

/// Canvas background pattern, sized by the snap step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Background {
    Grid,
    #[default]
    Dots,
    #[serde(alias = "HexLines")]
    TriangleLines,
    #[serde(alias = "HexDots")]
    TriangleDots,
}

impl Background {
    pub const ALL: [Background; 4] = [
        Background::Grid,
        Background::Dots,
        Background::TriangleLines,
        Background::TriangleDots,
    ];

    pub fn is_dots(self) -> bool {
        matches!(self, Background::Dots | Background::TriangleDots)
    }

    pub fn label(self) -> &'static str {
        match self {
            Background::Grid => "Grid",
            Background::Dots => "Dots",
            Background::TriangleLines => "Triangle lines",
            Background::TriangleDots => "Triangle dots",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snaps_to_nearest_multiple() {
        assert_eq!(snap(0.0, 4.0), 0.0);
        assert_eq!(snap(5.0, 4.0), 4.0);
        assert_eq!(snap(6.0, 4.0), 8.0);
        assert_eq!(snap(-5.0, 4.0), -4.0);
        assert_eq!(snap(101.0, 3.0), 102.0);
    }

    #[test]
    fn disabled_properties_do_not_snap() {
        let snap = SnapSettings::default();
        assert_eq!(snap.width_step(), DEFAULT_STEP);
        assert_eq!(snap.stroke_step(), 0.0);
        assert_eq!(super::snap(1.5, snap.stroke_step()), 1.5);
    }
}
