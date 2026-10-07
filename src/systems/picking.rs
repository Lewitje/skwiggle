//! Hit testing: which entity is under a world-space point.

use eframe::egui::{Pos2, emath::Rot2};

use crate::ecs::Entity;
use crate::world::World;

impl World {
    /// Whether `p` lands on `e`'s geometry.
    pub fn contains(&self, e: Entity, p: Pos2) -> bool {
        let r = self.rect(e);
        // Test in the shape's own unrotated frame.
        let p = r.center() + Rot2::from_angle(-self.angle(e)) * (p - r.center());
        if !self.ellipses.has(e) {
            return r.contains(p);
        }
        let c = r.center();
        let rx = (r.width() / 2.0).max(0.001);
        let ry = (r.height() / 2.0).max(0.001);
        let (dx, dy) = ((p.x - c.x) / rx, (p.y - c.y) / ry);
        dx * dx + dy * dy <= 1.0
    }

    /// Topmost visible entity under `p`, ignoring clipped-away parts.
    pub fn hit_test(&self, p: Pos2) -> Option<Entity> {
        self.hit(p, |_| true)
    }

    /// Topmost visible frame under `p`, skipping `exclude`.
    pub fn frame_at(&self, p: Pos2, exclude: &[Entity]) -> Option<Entity> {
        self.hit(p, |e| self.frames.has(e) && !exclude.contains(&e))
    }

    fn hit(&self, p: Pos2, accept: impl Fn(Entity) -> bool) -> Option<Entity> {
        self.paint_order()
            .into_iter()
            .rev()
            .find(|&(e, clip)| {
                clip.is_none_or(|c| c.contains(p)) && self.contains(e, p) && accept(e)
            })
            .map(|(e, _)| e)
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui::pos2;

    use crate::systems::hierarchy::tests::frame_with_child;

    #[test]
    fn clipped_children_only_hit_inside_frame() {
        let (w, f, c) = frame_with_child(true);
        assert_eq!(w.hit_test(pos2(75.0, 75.0)), Some(c));
        assert_eq!(w.hit_test(pos2(10.0, 10.0)), Some(f));
        assert_eq!(w.hit_test(pos2(125.0, 125.0)), None);
        let (w, _, c) = frame_with_child(false);
        assert_eq!(w.hit_test(pos2(125.0, 125.0)), Some(c));
    }
}
