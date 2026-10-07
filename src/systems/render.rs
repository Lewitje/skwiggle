//! Render systems: fit text entities to their content, then paint every visible entity.

use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Vec2, emath::Rot2};
use eframe::egui::layers::ShapeIdx;
use eframe::epaint::{Mesh, Shape, Tessellator};

use crate::components::color;
use crate::ecs::Entity;
use crate::paint::{self, Outline, PaintCache};
use crate::world::World;

/// Text entities grow vertically to fit their content.
pub fn layout_text(world: &mut World, painter: &Painter, zoom: f32) {
    for (e, text) in world.texts.iter() {
        let fill = world
            .fills
            .get(e)
            .map_or(Color32::BLACK, |f| f.0.solid_color());
        let Some(t) = world.transforms.get_mut(e) else {
            continue;
        };
        let font = FontId::proportional(text.font_size * zoom);
        let galley = painter.layout(text.content.clone(), font, fill, t.w * zoom);
        t.h = (galley.size().y / zoom).max(text.font_size);
    }
}

/// Paints entities back to front; `editing` is the text entity drawn by the in-place editor instead.
pub fn render(
    world: &World,
    painter: &Painter,
    clip: Rect,
    zoom: f32,
    to_screen: impl Fn(Rect) -> Rect,
    editing: Option<Entity>,
    cache: &mut PaintCache,
) {
    for (e, frame_clip) in world.paint_order() {
        // Children of clipping frames paint only inside the frame.
        let clip = match frame_clip {
            Some(c) => to_screen(c).intersect(clip),
            None => clip,
        };
        let r = to_screen(world.rect(e));
        if !clip.is_positive() || !to_screen(world.bounds(e)).intersects(clip) {
            continue;
        }
        let painter = &painter.with_clip_rect(clip);
        let angle = world.angle(e);
        let first = painter.add(Shape::Noop);
        paint_entity(world, painter, e, r, zoom, editing, cache);
        if angle != 0.0 {
            let end = painter.add(Shape::Noop);
            rotate_shapes(painter, first.0 + 1..end.0, angle, r.center());
        }
    }
}

/// Paints one entity, unrotated, into screen rect `r`.
fn paint_entity(
    world: &World,
    painter: &Painter,
    e: Entity,
    r: Rect,
    zoom: f32,
    editing: Option<Entity>,
    cache: &mut PaintCache,
) {
    {
        let fill = world.fills.get(e).map(|f| &f.0);

        if let Some(text) = world.texts.get(e) {
            if Some(e) != editing {
                let font = FontId::proportional(text.font_size * zoom);
                let c = fill.map_or(Color32::BLACK, |p| p.solid_color());
                let galley = painter.layout(text.content.clone(), font, c, r.width());
                painter.galley(r.min, galley, c);
            }
            return;
        }

        let stroke = world
            .strokes
            .get(e)
            .filter(|s| s.width > 0.0)
            .map(|s| egui::Stroke::new(s.width * zoom, color(s.color)));
        let outline = if world.ellipses.has(e) {
            Outline::Ellipse
        } else {
            Outline::Rect(world.radii.get(e).map_or(0.0, |c| c.0 * zoom))
        };
        if let Some(fill) = fill {
            paint::fill(painter, cache, r, outline, fill, zoom);
        }
        match (outline, stroke) {
            (Outline::Rect(radius), Some(stroke)) => {
                paint::stroke_rounded_inside(painter, r, radius, stroke);
            }
            (Outline::Ellipse, Some(stroke)) => {
                let inset = r.size() / 2.0 - Vec2::splat(stroke.width / 2.0);
                painter.add(egui::Shape::ellipse_stroke(
                    r.center(),
                    inset.max(Vec2::ZERO),
                    stroke,
                ));
            }
            (_, None) => {}
        }
    }
}

/// Turns the painter's shapes in `range` by `angle` about `pivot`: text by its angle, the rest as meshes.
fn rotate_shapes(painter: &Painter, range: std::ops::Range<usize>, angle: f32, pivot: Pos2) {
    let ctx = painter.ctx();
    let rot = Rot2::from_angle(angle);
    let options = ctx.tessellation_options(|o| *o);
    let tex_size = ctx.fonts(|f| f.font_image_size());
    let mut tess = Tessellator::new(ctx.pixels_per_point(), options, tex_size, Vec::new());
    ctx.graphics_mut(|g| {
        let list = g.entry(painter.layer_id());
        for i in range {
            list.mutate_shape(ShapeIdx(i), |cs| {
                let shape = std::mem::replace(&mut cs.shape, Shape::Noop);
                cs.shape = match shape {
                    Shape::Text(mut t) => {
                        t.pos = pivot + rot * (t.pos - pivot);
                        t.angle += angle;
                        Shape::Text(t)
                    }
                    other => {
                        let mut mesh = match &other {
                            Shape::Rect(r) => r
                                .brush
                                .as_ref()
                                .map_or_else(Mesh::default, |b| Mesh::with_texture(b.fill_texture_id)),
                            _ => Mesh::default(),
                        };
                        tess.tessellate_shape(other, &mut mesh);
                        mesh.rotate(rot, pivot);
                        Shape::mesh(mesh)
                    }
                };
            });
        }
    });
}
