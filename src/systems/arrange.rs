//! Arranging several entities relative to each other: aligning edges and spacing them evenly.

use eframe::egui::{Rect, Vec2, vec2};

use crate::ecs::Entity;
use crate::world::World;

impl World {
    /// Moves `ids` and everything nested in them by `d`.
    pub fn translate(&mut self, ids: &[Entity], d: Vec2) {
        for id in self.with_descendants(ids) {
            if let Some(t) = self.transforms.get_mut(id) {
                t.x += d.x;
                t.y += d.y;
            }
        }
    }

    /// Lines up the top-level `ids` along x (`horizontal`) or y at `at`: 0 start, 0.5 centre, 1 end of their combined bounds.
    pub fn align(&mut self, ids: &[Entity], horizontal: bool, at: f32) {
        let roots = self.topmost(ids);
        let all = roots.iter().fold(Rect::NOTHING, |b, &e| b.union(self.bounds(e)));
        let axis = |r: Rect| {
            if horizontal {
                r.min.x + r.width() * at
            } else {
                r.min.y + r.height() * at
            }
        };
        let target = axis(all);
        for e in roots {
            let shift = target - axis(self.bounds(e));
            self.translate(&[e], along(horizontal, shift));
        }
    }

    /// Spaces the top-level `ids` so the gaps between neighbours along x (`horizontal`) or y are equal; the outermost stay put.
    pub fn distribute(&mut self, ids: &[Entity], horizontal: bool) {
        let mut items: Vec<(Entity, f32, f32)> = self
            .topmost(ids)
            .into_iter()
            .map(|e| {
                let r = self.bounds(e);
                if horizontal {
                    (e, r.min.x, r.width())
                } else {
                    (e, r.min.y, r.height())
                }
            })
            .collect();
        if items.len() < 3 {
            return;
        }
        items.sort_by(|a, b| a.1.total_cmp(&b.1));
        let start = items[0].1;
        let end = items.iter().map(|(_, min, len)| min + len).fold(f32::MIN, f32::max);
        let total: f32 = items.iter().map(|(_, _, len)| len).sum();
        let gap = (end - start - total) / (items.len() - 1) as f32;
        let mut cursor = start;
        for (e, min, len) in items {
            self.translate(&[e], along(horizontal, cursor - min));
            cursor += len + gap;
        }
    }
}

fn along(horizontal: bool, d: f32) -> Vec2 {
    if horizontal { vec2(d, 0.0) } else { vec2(0.0, d) }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{pos2, vec2};

    use super::*;
    use crate::components::ShapeKind;

    fn rects(specs: &[(f32, f32, f32, f32)]) -> (World, Vec<Entity>) {
        let mut w = World::default();
        let ids = specs
            .iter()
            .map(|&(x, y, wd, h)| {
                w.spawn_shape(ShapeKind::Rect, Rect::from_min_size(pos2(x, y), vec2(wd, h)))
            })
            .collect();
        (w, ids)
    }

    #[test]
    fn aligns_tops_centres_and_ends() {
        let (mut w, ids) = rects(&[(0.0, 10.0, 20.0, 20.0), (50.0, 40.0, 20.0, 40.0)]);
        w.align(&ids, false, 0.0);
        assert_eq!((w.rect(ids[0]).min.y, w.rect(ids[1]).min.y), (10.0, 10.0));
        w.align(&ids, false, 0.5);
        assert_eq!((w.rect(ids[0]).center().y, w.rect(ids[1]).center().y), (30.0, 30.0));
        w.align(&ids, true, 1.0);
        assert_eq!((w.rect(ids[0]).max.x, w.rect(ids[1]).max.x), (70.0, 70.0));
    }

    #[test]
    fn distributes_with_equal_gaps_keeping_the_ends() {
        let (mut w, ids) = rects(&[
            (0.0, 0.0, 10.0, 10.0),
            (12.0, 0.0, 30.0, 10.0),
            (90.0, 0.0, 10.0, 10.0),
        ]);
        w.distribute(&ids, true);
        let xs: Vec<f32> = ids.iter().map(|&e| w.rect(e).min.x).collect();
        assert_eq!(xs, vec![0.0, 35.0, 90.0]);
    }
}
