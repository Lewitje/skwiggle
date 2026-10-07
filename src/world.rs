//! The document as an ECS world: shapes are entities assembled from components.

use eframe::egui::{Pos2, Rect, emath::Rot2, pos2, vec2};
use serde::Deserialize;

use crate::components::*;
use crate::ecs::{Entity, world};
use crate::paint::Paint;
use crate::resources::{Background, Shimmer, SnapSettings};

world! {
    components {
        names: Name,
        transforms: Transform,
        anchors: Anchor,
        rotations: Rotation,
        fills: Fill,
        strokes: Stroke,
        radii: CornerRadius,
        ellipses: Ellipse,
        texts: Text,
        frames: Frame,
        parents: Parent,
        hidden: Hidden,
    }
    resources {
        snap: SnapSettings,
        background: Background,
        shimmer: Shimmer,
    }
}

impl World {
    /// A new, empty entity on top of the z-order.
    pub fn spawn(&mut self) -> Entity {
        self.next_id += 1;
        self.entities.push(self.next_id);
        self.next_id
    }

    /// Removes `e` alone; see `remove` to take its children too.
    pub fn despawn(&mut self, e: Entity) {
        self.entities.retain(|x| *x != e);
        self.strip(e);
    }

    /// Spawns a new entity with copies of all of `e`'s components.
    pub fn clone_entity(&mut self, e: Entity) -> Entity {
        let copy = self.spawn();
        self.clone_components(e, copy);
        copy
    }

    pub fn alive(&self, e: Entity) -> bool {
        self.entities.contains(&e)
    }

    /// Spawns a shape from its prefab.
    pub fn spawn_shape(&mut self, kind: ShapeKind, rect: Rect) -> Entity {
        let e = self.spawn();
        self.insert_bundle(e, kind, rect);
        e
    }

    /// Attaches a prefab's default components to `e`.
    fn insert_bundle(&mut self, e: Entity, kind: ShapeKind, rect: Rect) {
        self.names.insert(e, Name(format!("{} {e}", kind.label())));
        self.transforms.insert(e, Transform::from_rect(rect));
        let fill = match kind {
            ShapeKind::Text => [30, 30, 30, 255],
            ShapeKind::Frame => [255, 255, 255, 255],
            _ => [217, 217, 217, 255],
        };
        self.fills.insert(e, Fill(Paint::Solid(fill)));
        match kind {
            ShapeKind::Text => {
                let text = Text {
                    content: "Text".into(),
                    font_size: 24.0,
                };
                self.texts.insert(e, text);
            }
            ShapeKind::Rect | ShapeKind::Ellipse | ShapeKind::Frame => {
                self.strokes.insert(e, Stroke::default());
            }
        }
        match kind {
            ShapeKind::Rect => self.radii.insert(e, CornerRadius::default()),
            ShapeKind::Ellipse => self.ellipses.insert(e, Ellipse),
            ShapeKind::Frame => {
                self.radii.insert(e, CornerRadius::default());
                self.frames.insert(e, Frame { clip: true });
            }
            ShapeKind::Text => {}
        }
    }

    /// Which prefab an entity looks like, judged by its components.
    pub fn kind(&self, e: Entity) -> ShapeKind {
        if self.frames.has(e) {
            ShapeKind::Frame
        } else if self.texts.has(e) {
            ShapeKind::Text
        } else if self.ellipses.has(e) {
            ShapeKind::Ellipse
        } else {
            ShapeKind::Rect
        }
    }

    /// The entity's bounds, or `Rect::NOTHING` if it has no transform.
    pub fn rect(&self, e: Entity) -> Rect {
        self.transforms
            .get(e)
            .map_or(Rect::NOTHING, Transform::rect)
    }

    pub fn set_rect(&mut self, e: Entity, r: Rect) {
        if let Some(t) = self.transforms.get_mut(e) {
            *t = Transform::from_rect(r);
        }
    }

    /// Clockwise rotation in radians.
    pub fn angle(&self, e: Entity) -> f32 {
        self.rotations.get(e).map_or(0.0, |r| r.0.to_radians())
    }

    /// Corners of `rect` turned by `e`'s rotation, clockwise from top-left.
    pub fn corners_of(&self, e: Entity, rect: Rect) -> [Pos2; 4] {
        let rot = Rot2::from_angle(self.angle(e));
        let c = rect.center();
        [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()]
            .map(|p| c + rot * (p - c))
    }

    pub fn corners(&self, e: Entity) -> [Pos2; 4] {
        self.corners_of(e, self.rect(e))
    }

    /// Axis-aligned bounds of `rect` once turned by `e`'s rotation.
    pub fn bounds_of(&self, e: Entity, rect: Rect) -> Rect {
        if self.angle(e) == 0.0 || rect == Rect::NOTHING {
            return rect;
        }
        Rect::from_points(&self.corners_of(e, rect))
    }

    /// Axis-aligned bounds of the rotated shape.
    pub fn bounds(&self, e: Entity) -> Rect {
        self.bounds_of(e, self.rect(e))
    }

    /// Sets `e`'s rotation in degrees; its descendants turn with it about its centre.
    pub fn set_rotation(&mut self, e: Entity, degrees: f32) {
        let current = self.rotations.get(e).map_or(0.0, |r| r.0);
        let delta = normalize_degrees(degrees - current);
        if delta == 0.0 {
            return;
        }
        let pivot = self.rect(e).center();
        let rot = Rot2::from_angle(delta.to_radians());
        for id in self.with_descendants(&[e]) {
            if id != e
                && let Some(t) = self.transforms.get_mut(id)
            {
                let c = t.rect().center();
                let moved = pivot + rot * (c - pivot) - c;
                t.x += moved.x;
                t.y += moved.y;
            }
            let turned = normalize_degrees(self.rotations.get(id).map_or(0.0, |r| r.0) + delta);
            if turned == 0.0 {
                self.rotations.remove(id);
            } else {
                self.rotations.insert(id, Rotation(turned));
            }
        }
    }

    pub fn visible(&self, e: Entity) -> bool {
        !self.hidden.has(e)
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let json = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Self::from_json(&json)
    }

    /// Parses a saved world, upgrading pre-ECS files.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if value.get("shapes").is_some() {
            let old: LegacyDocument = serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(old.into_world())
        } else {
            serde_json::from_value(value).map_err(|e| e.to_string())
        }
    }
}

/// Wraps degrees into -180..=180, rounding off float noise.
pub fn normalize_degrees(d: f32) -> f32 {
    let d = (d + 180.0).rem_euclid(360.0) - 180.0;
    let d = if d == -180.0 { 180.0 } else { d };
    (d * 1000.0).round() / 1000.0
}

/// The pre-ECS file format: one flat struct per shape.
#[derive(Deserialize)]
struct LegacyDocument {
    shapes: Vec<LegacyShape>,
    next_id: Entity,
    #[serde(default)]
    snap: SnapSettings,
    #[serde(default)]
    background: Background,
}

#[derive(Deserialize)]
struct LegacyShape {
    id: Entity,
    name: String,
    kind: ShapeKind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    fill: [u8; 4],
    stroke: [u8; 4],
    stroke_width: f32,
    corner_radius: f32,
    text: String,
    font_size: f32,
    visible: bool,
    #[serde(default)]
    parent: Option<Entity>,
    #[serde(default)]
    clip_content: bool,
}

impl LegacyDocument {
    fn into_world(self) -> World {
        let mut w = World {
            next_id: self.next_id,
            snap: self.snap,
            background: self.background,
            ..Default::default()
        };
        for s in self.shapes {
            let e = s.id;
            w.entities.push(e);
            w.next_id = w.next_id.max(e);
            w.insert_bundle(
                e,
                s.kind,
                Rect::from_min_size(pos2(s.x, s.y), vec2(s.w, s.h)),
            );
            w.names.insert(e, Name(s.name));
            w.fills.insert(e, Fill(Paint::Solid(s.fill)));
            if let Some(st) = w.strokes.get_mut(e) {
                *st = Stroke {
                    color: s.stroke,
                    width: s.stroke_width,
                };
            }
            if let Some(r) = w.radii.get_mut(e) {
                r.0 = s.corner_radius;
            }
            if let Some(t) = w.texts.get_mut(e) {
                t.content = s.text;
                t.font_size = s.font_size;
            }
            if let Some(f) = w.frames.get_mut(e) {
                f.clip = s.clip_content;
            }
            if let Some(p) = s.parent {
                w.parents.insert(e, Parent(p));
            }
            if !s.visible {
                w.hidden.insert(e, Hidden);
            }
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_files_without_snap_get_defaults() {
        let w = World::from_json(r#"{"shapes":[],"next_id":0}"#).unwrap();
        assert_eq!(w.snap, SnapSettings::default());
        assert_eq!(w.background, Background::Dots);
    }

    #[test]
    fn legacy_shapes_become_entities() {
        let json = r#"{"next_id":2,"shapes":[
            {"id":1,"name":"F","kind":"Frame","x":0,"y":0,"w":100,"h":100,"fill":[1,2,3,4],
             "stroke":[0,0,0,255],"stroke_width":2,"corner_radius":8,"text":"","font_size":24,
             "visible":true,"clip_content":false},
            {"id":2,"name":"T","kind":"Text","x":10,"y":10,"w":50,"h":20,"fill":[0,0,0,255],
             "stroke":[0,0,0,255],"stroke_width":0,"corner_radius":0,"text":"hi","font_size":12,
             "visible":false,"parent":1}]}"#;
        let w = World::from_json(json).unwrap();
        assert_eq!(w.entities, vec![1, 2]);
        assert_eq!(w.kind(1), ShapeKind::Frame);
        assert_eq!(w.frames.get(1), Some(&Frame { clip: false }));
        assert_eq!(w.radii.get(1), Some(&CornerRadius(8.0)));
        assert_eq!(w.texts.get(2).unwrap().content, "hi");
        assert!(!w.strokes.has(2) && !w.visible(2));
        assert_eq!(w.parents.get(2), Some(&Parent(1)));
    }

    #[test]
    fn rotating_a_frame_turns_its_children_about_its_centre() {
        let mut w = World::default();
        let f = w.spawn_shape(ShapeKind::Frame, Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0)));
        let c = w.spawn_shape(ShapeKind::Rect, Rect::from_min_size(pos2(70.0, 40.0), vec2(20.0, 20.0)));
        w.parents.insert(c, Parent(f));
        w.set_rotation(f, 90.0);
        assert_eq!(w.rotations.get(c), Some(&Rotation(90.0)));
        let centre = w.rect(c).center();
        assert!((centre - pos2(50.0, 80.0)).length() < 1e-3);
        w.set_rotation(f, 0.0);
        assert!(!w.rotations.has(c) && !w.rotations.has(f));
        assert!((w.rect(c).center() - pos2(80.0, 50.0)).length() < 1e-3);
    }

    #[test]
    fn round_trips_through_json() {
        let mut w = World::default();
        w.spawn_shape(
            ShapeKind::Ellipse,
            Rect::from_min_size(pos2(1.0, 2.0), vec2(3.0, 4.0)),
        );
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(World::from_json(&json).unwrap(), w);
    }
}
