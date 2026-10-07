//! Paints (solid, gradient, image) and drawing them into shapes via cached textures.

use std::collections::HashMap;

use eframe::egui::{
    Color32, ColorImage, Context, Mesh, Painter, Pos2, Rect, TextureHandle, TextureId,
    TextureOptions, TextureWrapMode, Vec2, epaint, pos2, vec2,
};
use serde::{Deserialize, Serialize};

use crate::components::color;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    /// Unmultiplied sRGBA.
    Solid([u8; 4]),
    Gradient(Gradient),
    Image(ImagePaint),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    /// Direction of a linear gradient in degrees; 0 runs left to right, 90 top to bottom.
    pub angle: f32,
    pub stops: Vec<Stop>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientKind {
    Linear,
    Radial,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    /// 0..=1 along the gradient.
    pub pos: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImagePaint {
    pub path: String,
    pub fit: ImageFit,
    /// Tile size as a multiple of the image's pixel size.
    pub scale: f32,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageFit {
    /// Cover the shape, cropping the overflow.
    Fill,
    /// Show the whole image inside the shape.
    Fit,
    Stretch,
    Tile,
}

impl ImageFit {
    pub const ALL: [ImageFit; 4] = [
        ImageFit::Fill,
        ImageFit::Fit,
        ImageFit::Stretch,
        ImageFit::Tile,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ImageFit::Fill => "Fill",
            ImageFit::Fit => "Fit",
            ImageFit::Stretch => "Stretch",
            ImageFit::Tile => "Tile",
        }
    }
}

impl Paint {
    /// One color standing in for the paint, e.g. for text.
    pub fn solid_color(&self) -> Color32 {
        match self {
            Paint::Solid(c) => color(*c),
            Paint::Gradient(g) => g.stops.first().map_or(Color32::GRAY, |s| color(s.color)),
            Paint::Image(_) => Color32::GRAY,
        }
    }

    /// Converts to another kind of paint, carrying the color over where it can.
    pub fn solid(&self) -> Paint {
        Paint::Solid(self.solid_color().to_srgba_unmultiplied())
    }

    pub fn gradient(&self) -> Paint {
        let c = self.solid_color().to_srgba_unmultiplied();
        let faded = [c[0], c[1], c[2], 0];
        Paint::Gradient(Gradient {
            kind: GradientKind::Linear,
            angle: 90.0,
            stops: vec![
                Stop { pos: 0.0, color: c },
                Stop {
                    pos: 1.0,
                    color: faded,
                },
            ],
        })
    }

    pub fn image() -> Paint {
        Paint::Image(ImagePaint {
            path: String::new(),
            fit: ImageFit::Fill,
            scale: 1.0,
            opacity: 1.0,
        })
    }
}

impl Gradient {
    /// The color at `t` (0..=1), blending between the surrounding stops.
    pub fn sample(&self, t: f32) -> [u8; 4] {
        let mut stops = self.stops.clone();
        stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        let (Some(first), Some(last)) = (stops.first(), stops.last()) else {
            return [0; 4];
        };
        if t <= first.pos {
            return first.color;
        }
        if t >= last.pos {
            return last.color;
        }
        let i = stops
            .iter()
            .position(|s| s.pos > t)
            .unwrap_or(stops.len() - 1);
        let (a, b) = (stops[i - 1], stops[i]);
        let f = (t - a.pos) / (b.pos - a.pos).max(1e-6);
        std::array::from_fn(|k| {
            (a.color[k] as f32 + (b.color[k] as f32 - a.color[k] as f32) * f).round() as u8
        })
    }

    /// Where a point in the unit square falls along the gradient.
    fn t_at(&self, uv: Vec2) -> f32 {
        let d = uv - vec2(0.5, 0.5);
        match self.kind {
            GradientKind::Linear => {
                // Spans corner to corner, like CSS.
                let a = self.angle.to_radians();
                let dir = vec2(a.cos(), a.sin());
                0.5 + d.dot(dir) / (dir.x.abs() + dir.y.abs())
            }
            GradientKind::Radial => d.length() * 2.0,
        }
    }
}

/// The geometry a paint fills.
#[derive(Clone, Copy)]
pub enum Outline {
    Rect(f32),
    Ellipse,
}

/// GPU textures for gradients and images, rebuilt only when a paint changes.
#[derive(Default)]
pub struct PaintCache {
    gradients: HashMap<String, (TextureHandle, bool)>,
    /// Keyed by (path, tiled); `None` remembers a failed load.
    images: HashMap<(String, bool), Option<(TextureHandle, Vec2)>>,
}

const GRADIENT_RES: usize = 128;

impl PaintCache {
    fn gradient(&mut self, ctx: &Context, g: &Gradient) -> TextureId {
        let key = format!("{g:?}");
        let entry = self.gradients.entry(key).or_insert_with(|| {
            let mut pixels = Vec::with_capacity(GRADIENT_RES * GRADIENT_RES * 4);
            for y in 0..GRADIENT_RES {
                for x in 0..GRADIENT_RES {
                    let uv = (vec2(x as f32, y as f32) + Vec2::splat(0.5)) / GRADIENT_RES as f32;
                    pixels.extend(g.sample(g.t_at(uv)));
                }
            }
            let img = ColorImage::from_rgba_unmultiplied([GRADIENT_RES; 2], &pixels);
            (
                ctx.load_texture("gradient", img, TextureOptions::LINEAR),
                false,
            )
        });
        entry.1 = true;
        entry.0.id()
    }

    /// The texture and image size. Clamped textures get a transparent 1px border so `Fit` can sample past the edge.
    fn image(&mut self, ctx: &Context, path: &str, tiled: bool) -> Option<(TextureId, Vec2)> {
        let entry = self
            .images
            .entry((path.to_owned(), tiled))
            .or_insert_with(|| load_image(ctx, path, tiled));
        entry.as_ref().map(|(t, size)| (t.id(), *size))
    }

    /// Whether `path` loads as an image.
    pub fn loads(&mut self, ctx: &Context, path: &str) -> bool {
        self.image(ctx, path, false).is_some()
    }

    /// Drops gradient textures that weren't drawn since the last call.
    pub fn end_frame(&mut self) {
        self.gradients.retain(|_, (_, used)| std::mem::take(used));
    }

    /// Forgets a failed or stale image so it is loaded again.
    pub fn reload(&mut self, path: &str) {
        self.images.retain(|(p, _), _| p != path);
    }
}

fn load_image(ctx: &Context, path: &str, tiled: bool) -> Option<(TextureHandle, Vec2)> {
    let img = image::open(path).ok()?;
    // Keep huge photos from blowing up GPU memory.
    let img = if img.width().max(img.height()) > 2048 {
        img.resize(2048, 2048, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let size = vec2(w as f32, h as f32);
    let (image, options) = if tiled {
        let options = TextureOptions {
            wrap_mode: TextureWrapMode::Repeat,
            ..TextureOptions::LINEAR
        };
        (
            ColorImage::from_rgba_unmultiplied([w, h], rgba.as_raw()),
            options,
        )
    } else {
        let mut padded = vec![0u8; (w + 2) * (h + 2) * 4];
        for y in 0..h {
            let src = &rgba.as_raw()[y * w * 4..(y + 1) * w * 4];
            let at = ((y + 1) * (w + 2) + 1) * 4;
            padded[at..at + w * 4].copy_from_slice(src);
        }
        let img = ColorImage::from_rgba_unmultiplied([w + 2, h + 2], &padded);
        (img, TextureOptions::LINEAR)
    };
    Some((ctx.load_texture(path, image, options), size))
}

/// Outline of a rounded rect; egui's own corner radius rounds to whole pixels and caps at 255.
pub fn rounded_rect(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.clamp(0.0, rect.width().min(rect.height()) / 2.0);
    if r <= 0.0 {
        return vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    // Roughly 3px per arc segment keeps big corners smooth and small ones cheap.
    let n = ((r * std::f32::consts::FRAC_PI_2 / 3.0).ceil() as usize).clamp(2, 64);
    let corners = [
        (pos2(rect.max.x - r, rect.min.y + r), -90.0f32),
        (pos2(rect.max.x - r, rect.max.y - r), 0.0),
        (pos2(rect.min.x + r, rect.max.y - r), 90.0),
        (pos2(rect.min.x + r, rect.min.y + r), 180.0),
    ];
    let mut pts = Vec::with_capacity(4 * (n + 1));
    for (c, start) in corners {
        for i in 0..=n {
            let a = (start + 90.0 * i as f32 / n as f32).to_radians();
            pts.push(c + vec2(a.cos(), a.sin()) * r);
        }
    }
    pts
}

/// Fills a rounded rect with a solid colour.
pub fn fill_rounded(painter: &Painter, rect: Rect, radius: f32, color: Color32) {
    if radius <= 0.0 {
        painter.rect_filled(rect, 0.0, color);
    } else {
        painter.add(epaint::Shape::convex_polygon(
            rounded_rect(rect, radius),
            color,
            epaint::Stroke::NONE,
        ));
    }
}

/// Strokes a rounded rect with the stroke fully inside `rect`.
pub fn stroke_rounded_inside(painter: &Painter, rect: Rect, radius: f32, stroke: epaint::Stroke) {
    let inset = rect.shrink(stroke.width / 2.0);
    let r = (radius - stroke.width / 2.0).max(0.0);
    painter.add(epaint::Shape::closed_line(rounded_rect(inset, r), stroke));
}

/// Fills `rect` (screen space) with `paint`; `zoom` scales tiled images.
pub fn fill(
    painter: &Painter,
    cache: &mut PaintCache,
    rect: Rect,
    outline: Outline,
    paint: &Paint,
    zoom: f32,
) {
    let ctx = painter.ctx();
    match paint {
        Paint::Solid(c) => match outline {
            Outline::Rect(radius) => fill_rounded(painter, rect, radius, color(*c)),
            Outline::Ellipse => {
                painter.add(epaint::Shape::ellipse_filled(
                    rect.center(),
                    rect.size() / 2.0,
                    color(*c),
                ));
            }
        },
        Paint::Gradient(g) => {
            let tex = cache.gradient(ctx, g);
            textured(
                painter,
                rect,
                outline,
                tex,
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        Paint::Image(img) => {
            let tiled = img.fit == ImageFit::Tile;
            let Some((tex, size)) = cache.image(ctx, &img.path, tiled) else {
                missing_image(painter, rect, outline);
                return;
            };
            let uv = image_uv(img, size, rect.size(), zoom);
            let tint = Color32::WHITE.gamma_multiply(img.opacity.clamp(0.0, 1.0));
            textured(painter, rect, outline, tex, uv, tint);
        }
    }
}

/// Which part of the image texture maps onto a shape of `shape` screen size.
fn image_uv(img: &ImagePaint, size: Vec2, shape: Vec2, zoom: f32) -> Rect {
    let unit = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    let visible = match img.fit {
        ImageFit::Tile => {
            let tile = size * img.scale.max(0.01) * zoom;
            return Rect::from_min_size(Pos2::ZERO, shape / tile);
        }
        ImageFit::Stretch => return pad(unit, size),
        ImageFit::Fill => shape / (shape / size).max_elem(),
        ImageFit::Fit => shape / (shape / size).min_elem(),
    };
    // Image-space window centered on the image, as a fraction of it.
    let frac = visible / size;
    pad(Rect::from_center_size(pos2(0.5, 0.5), frac), size)
}

/// Maps image-space uv onto the padded texture.
fn pad(uv: Rect, size: Vec2) -> Rect {
    let total = size + Vec2::splat(2.0);
    let map = |p: Pos2| ((p.to_vec2() * size + Vec2::splat(1.0)) / total).to_pos2();
    Rect::from_min_max(map(uv.min), map(uv.max))
}

fn textured(
    painter: &Painter,
    rect: Rect,
    outline: Outline,
    tex: TextureId,
    uv: Rect,
    tint: Color32,
) {
    match outline {
        Outline::Rect(radius) if radius <= 0.0 => {
            let shape = epaint::RectShape::filled(rect, 0.0, tint).with_texture(tex, uv);
            painter.add(shape);
        }
        Outline::Rect(radius) => {
            textured_fan(painter, rect, rounded_rect(rect, radius), tex, uv, tint)
        }
        Outline::Ellipse => {
            let n = 96;
            let ring = (0..n)
                .map(|i| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    rect.center() + vec2(a.cos(), a.sin()) * rect.size() / 2.0
                })
                .collect();
            textured_fan(painter, rect, ring, tex, uv, tint);
        }
    }
}

/// A triangle fan over a convex `ring`; uv is affine in position so the texture maps exactly.
fn textured_fan(
    painter: &Painter,
    rect: Rect,
    ring: Vec<Pos2>,
    tex: TextureId,
    uv: Rect,
    tint: Color32,
) {
    let mut mesh = Mesh::with_texture(tex);
    let vertex = |p: Pos2| {
        let f = (p - rect.min) / rect.size().max(Vec2::splat(1e-3));
        epaint::Vertex {
            pos: p,
            uv: uv.min + uv.size() * f,
            color: tint,
        }
    };
    mesh.vertices.push(vertex(rect.center()));
    let n = ring.len() as u32;
    mesh.vertices.extend(ring.into_iter().map(vertex));
    for i in 1..=n {
        mesh.add_triangle(0, i, i % n + 1);
    }
    painter.add(mesh);
}

/// A grey hatched placeholder for an image that hasn't been chosen or failed to load.
fn missing_image(painter: &Painter, rect: Rect, outline: Outline) {
    let base = Color32::from_gray(200);
    match outline {
        Outline::Rect(radius) => fill_rounded(painter, rect, radius, base),
        Outline::Ellipse => {
            painter.add(epaint::Shape::ellipse_filled(
                rect.center(),
                rect.size() / 2.0,
                base,
            ));
        }
    }
    let hatch = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let stroke = epaint::Stroke::new(1.0, Color32::from_gray(170));
    let step = 10.0;
    let mut x = rect.min.x - rect.height();
    while x < rect.max.x {
        hatch.line_segment(
            [pos2(x, rect.max.y), pos2(x + rect.height(), rect.min.y)],
            stroke,
        );
        x += step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounded_rect_keeps_large_radii_and_stays_inside() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        let pts = rounded_rect(r, 280.5);
        assert!(pts.iter().all(|p| r.expand(1e-3).contains(*p)));
        // The top edge starts exactly one radius in, unrounded and uncapped.
        assert!((pts.last().unwrap().x - 280.5).abs() < 1e-3);
        // Radii beyond half the short side clamp to a pill.
        let pill = rounded_rect(r, 10_000.0);
        assert!((pill.last().unwrap().x - 300.0).abs() < 1e-3);
    }

    fn bw() -> Gradient {
        Gradient {
            kind: GradientKind::Linear,
            angle: 0.0,
            stops: vec![
                Stop {
                    pos: 1.0,
                    color: [255, 255, 255, 255],
                },
                Stop {
                    pos: 0.0,
                    color: [0, 0, 0, 255],
                },
            ],
        }
    }

    #[test]
    fn samples_between_unsorted_stops() {
        let g = bw();
        assert_eq!(g.sample(-1.0), [0, 0, 0, 255]);
        assert_eq!(g.sample(0.5), [128, 128, 128, 255]);
        assert_eq!(g.sample(2.0), [255, 255, 255, 255]);
    }

    #[test]
    fn linear_gradient_spans_the_box() {
        let g = bw();
        assert_eq!(g.t_at(vec2(0.0, 0.3)), 0.0);
        assert_eq!(g.t_at(vec2(1.0, 0.7)), 1.0);
        let diag = Gradient {
            angle: 45.0,
            ..bw()
        };
        assert!((diag.t_at(vec2(1.0, 1.0)) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn fill_crops_and_fit_letterboxes() {
        let img = ImagePaint {
            path: String::new(),
            fit: ImageFit::Fill,
            scale: 1.0,
            opacity: 1.0,
        };
        let (size, shape) = (vec2(200.0, 100.0), vec2(100.0, 100.0));
        // Fill: a square window from the middle half of a 2:1 image.
        let uv = image_uv(&img, size, shape, 1.0);
        assert!((uv.width() * 202.0 - 100.0).abs() < 1e-3);
        // Fit: the full width shows, with room above and below.
        let fit = ImagePaint {
            fit: ImageFit::Fit,
            ..img
        };
        let uv = image_uv(&fit, size, shape, 1.0);
        assert!((uv.width() * 202.0 - 200.0).abs() < 1e-3);
        assert!(uv.height() * 102.0 > 100.0);
    }
}
