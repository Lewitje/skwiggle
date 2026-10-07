//! Render systems: fit text entities to their content, then paint every visible entity.

use eframe::egui::{self, Color32, FontId, Painter, Rect, Vec2};

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
        if !clip.is_positive() || !r.intersects(clip) {
            continue;
        }
        let painter = &painter.with_clip_rect(clip);
        let fill = world.fills.get(e).map(|f| &f.0);

        if let Some(text) = world.texts.get(e) {
            if Some(e) != editing {
                let font = FontId::proportional(text.font_size * zoom);
                let c = fill.map_or(Color32::BLACK, |p| p.solid_color());
                let galley = painter.layout(text.content.clone(), font, c, r.width());
                painter.galley(r.min, galley, c);
            }
            continue;
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
