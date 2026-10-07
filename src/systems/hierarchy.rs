//! Parent/child queries and structural edits: nesting, draw order, removal and duplication.

use std::collections::HashMap;

use eframe::egui::{Rect, Vec2};

use crate::components::Parent;
use crate::ecs::Entity;
use crate::world::World;

impl World {
    /// The frame `e` sits in, ignoring parents that no longer exist.
    pub fn parent(&self, e: Entity) -> Option<Entity> {
        self.parents.get(e).map(|p| p.0).filter(|p| self.alive(*p))
    }

    /// Children per parent, in z-order. `None` holds the roots.
    pub fn tree(&self) -> HashMap<Option<Entity>, Vec<Entity>> {
        let mut map: HashMap<Option<Entity>, Vec<Entity>> = HashMap::new();
        for &e in &self.entities {
            map.entry(self.parent(e)).or_default().push(e);
        }
        map
    }

    /// Visible entities back to front, each with the world-space clip from its ancestor frames.
    pub fn paint_order(&self) -> Vec<(Entity, Option<Rect>)> {
        fn walk(
            w: &World,
            tree: &HashMap<Option<Entity>, Vec<Entity>>,
            parent: Option<Entity>,
            clip: Option<Rect>,
            out: &mut Vec<(Entity, Option<Rect>)>,
        ) {
            for &e in tree.get(&parent).into_iter().flatten() {
                if !w.visible(e) {
                    continue;
                }
                out.push((e, clip));
                if let Some(frame) = w.frames.get(e) {
                    let inner = match (frame.clip, clip) {
                        (true, Some(c)) => Some(c.intersect(w.rect(e))),
                        (true, None) => Some(w.rect(e)),
                        (false, c) => c,
                    };
                    walk(w, tree, Some(e), inner, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(self, &self.tree(), None, None, &mut out);
        out
    }

    /// `ids` plus everything nested inside them.
    pub fn with_descendants(&self, ids: &[Entity]) -> Vec<Entity> {
        let mut out: Vec<Entity> = ids.to_vec();
        let mut i = 0;
        while i < out.len() {
            let id = out[i];
            for (e, p) in self.parents.iter() {
                if p.0 == id && !out.contains(&e) {
                    out.push(e);
                }
            }
            i += 1;
        }
        out
    }

    /// Of `ids`, those without an ancestor also in `ids`.
    pub fn topmost(&self, ids: &[Entity]) -> Vec<Entity> {
        ids.iter()
            .copied()
            .filter(|&e| {
                let mut p = self.parent(e);
                while let Some(pid) = p {
                    if ids.contains(&pid) {
                        return false;
                    }
                    p = self.parent(pid);
                }
                true
            })
            .collect()
    }

    /// Moves `e` into `parent`, on top of its new siblings.
    pub fn set_parent(&mut self, e: Entity, parent: Option<Entity>) {
        let Some(i) = self.entities.iter().position(|x| *x == e) else {
            return;
        };
        self.entities.remove(i);
        self.entities.push(e);
        match parent {
            Some(p) => self.parents.insert(e, Parent(p)),
            None => {
                self.parents.remove(e);
            }
        }
    }

    /// Despawns `ids` and everything nested inside them.
    pub fn remove(&mut self, ids: &[Entity]) {
        for e in self.with_descendants(ids) {
            self.despawn(e);
        }
    }

    /// Copies `ids` with their children, returning the new top-level entities.
    pub fn duplicate(&mut self, ids: &[Entity], offset: Vec2) -> Vec<Entity> {
        let roots = self.topmost(ids);
        let all = self.with_descendants(&roots);
        // Copy in z-order so the copies stack like the originals.
        let originals: Vec<Entity> = self
            .entities
            .iter()
            .copied()
            .filter(|e| all.contains(e))
            .collect();
        let remap: HashMap<Entity, Entity> = originals
            .iter()
            .map(|&e| (e, self.clone_entity(e)))
            .collect();
        for (&old, &new) in &remap {
            if roots.contains(&old) {
                if let Some(n) = self.names.get_mut(new) {
                    n.0 = format!("{} copy", n.0);
                }
            } else if let Some(p) = self.parents.get_mut(new) {
                p.0 = remap[&p.0];
            }
            if let Some(t) = self.transforms.get_mut(new) {
                t.x += offset.x;
                t.y += offset.y;
            }
        }
        roots.iter().map(|e| remap[e]).collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use eframe::egui::{pos2, vec2};

    use super::*;
    use crate::components::ShapeKind;

    pub fn frame_with_child(clip: bool) -> (World, Entity, Entity) {
        let mut w = World::default();
        let f = w.spawn_shape(
            ShapeKind::Frame,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0)),
        );
        w.frames.get_mut(f).unwrap().clip = clip;
        let c = w.spawn_shape(
            ShapeKind::Rect,
            Rect::from_min_size(pos2(50.0, 50.0), vec2(100.0, 100.0)),
        );
        w.set_parent(c, Some(f));
        (w, f, c)
    }

    #[test]
    fn remove_and_duplicate_include_children() {
        let (mut w, f, _) = frame_with_child(true);
        let copies = w.duplicate(&[f], vec2(10.0, 0.0));
        assert_eq!(w.entities.len(), 4);
        let copy_child = w.tree()[&Some(copies[0])][0];
        assert_eq!(w.transforms.get(copy_child).unwrap().x, 60.0);
        w.remove(&[f]);
        assert_eq!(w.entities.len(), 2);
        assert_eq!(w.transforms.iter().count(), 2);
    }
}
