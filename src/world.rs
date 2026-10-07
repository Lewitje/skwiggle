//! The document as an ECS world: shapes are entities assembled from components.

use eframe::egui::{Rect, pos2, vec2};
use serde::Deserialize;

use crate::components::*;
use crate::ecs::{Entity, world};
use crate::paint::Paint;
use crate::resources::{Background, Shimmer, SnapSettings};

world! {
    components {
        names: Name,
        transforms: Transform,
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
