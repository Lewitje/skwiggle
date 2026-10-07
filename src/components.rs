//! Components: the building blocks shape entities are assembled from.

use eframe::egui::{Color32, Rect, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::ecs::Entity;
use crate::paint::Paint;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Name(pub String);

/// World-space bounding box.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Transform {
    pub fn from_rect(r: Rect) -> Self {
        Self {
            x: r.min.x,
            y: r.min.y,
            w: r.width(),
            h: r.height(),
        }
    }

    pub fn rect(&self) -> Rect {
        Rect::from_min_size(pos2(self.x, self.y), vec2(self.w, self.h))
    }
}

/// Clockwise turn in degrees about the bounding box's centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rotation(pub f32);

/// The point that stays put when width or height is edited; each axis is 0 (start), 1 (middle) or 2 (end).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub col: u8,
    pub row: u8,
}

impl Anchor {
    /// Fraction of a size change that shifts the position on each axis.
    pub fn factor(self) -> (f32, f32) {
        (self.col as f32 / 2.0, self.row as f32 / 2.0)
    }
}

/// What the shape is filled with. Text only uses its solid color.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "FillRepr")]
pub struct Fill(pub Paint);

/// Fills used to be a bare sRGBA array; both forms load.
#[derive(Deserialize)]
#[serde(untagged)]
enum FillRepr {
    Rgba([u8; 4]),
    Paint(Paint),
}

impl From<FillRepr> for Fill {
    fn from(r: FillRepr) -> Self {
        match r {
            FillRepr::Rgba(c) => Fill(Paint::Solid(c)),
            FillRepr::Paint(p) => Fill(p),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: [u8; 4],
    pub width: f32,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            color: [0, 0, 0, 255],
            width: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CornerRadius(pub f32);

/// Draws and hit-tests as an ellipse rather than a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub content: String,
    pub font_size: f32,
}

/// A container other entities can sit in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Hide children outside the frame's bounds.
    pub clip: bool,
}

/// The frame this entity sits in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Parent(pub Entity);

/// Skipped by rendering and picking.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hidden;

/// A prefab: the bundle of components a drawing tool spawns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Text,
    Frame,
}

impl ShapeKind {
    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Rect => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::Text => "Text",
            ShapeKind::Frame => "Frame",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ShapeKind::Rect => "▭",
            ShapeKind::Ellipse => "○",
            ShapeKind::Text => "T",
            ShapeKind::Frame => "#",
        }
    }
}

pub fn color(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_load_from_bare_colors_and_paints() {
        let old: Fill = serde_json::from_str("[1,2,3,4]").unwrap();
        assert_eq!(old, Fill(Paint::Solid([1, 2, 3, 4])));
        let new = Fill(Paint::image());
        let json = serde_json::to_string(&new).unwrap();
        assert_eq!(serde_json::from_str::<Fill>(&json).unwrap(), new);
    }
}
