//! The fill editor popup: solid color, gradient with draggable stops, or image.

use eframe::egui::{
    self, Button, Color32, DragValue, Popup, PopupCloseBehavior, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui::widgets::color_picker::{self, Alpha};

use crate::components::color;
use crate::paint::{
    Gradient, GradientKind, ImageFit, ImagePaint, Outline, Paint, PaintCache, Stop,
};

const WIDTH: f32 = 260.0;

/// A swatch showing `paint` that opens the editor when clicked.
pub fn swatch(
    ui: &mut Ui,
    size: Vec2,
    paint: &mut Paint,
    solid_only: bool,
    cache: &mut PaintCache,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    draw_swatch(ui, rect, 4.0, paint, cache, resp.hovered());
    popup(&resp, paint, solid_only, cache);
    resp
}

/// Paints a swatch with an outline so fills close to the background stay visible.
pub fn draw_swatch(
    ui: &Ui,
    rect: Rect,
    radius: f32,
    paint: &Paint,
    cache: &mut PaintCache,
    hovered: bool,
) {
    let painter = ui.painter();
    crate::paint::fill(painter, cache, rect, Outline::Rect(radius), paint, 1.0);
    let edge = if hovered {
        Color32::from_gray(140)
    } else {
        Color32::from_white_alpha(40)
    };
    painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Outside);
}

/// Opens the fill editor from `resp`; `solid_only` hides gradients and images (e.g. for text).
pub fn popup(resp: &Response, paint: &mut Paint, solid_only: bool, cache: &mut PaintCache) {
    Popup::menu(resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().slider_width = WIDTH;
            editor(ui, paint, solid_only, cache);
        });
}

fn editor(ui: &mut Ui, paint: &mut Paint, solid_only: bool, cache: &mut PaintCache) {
    if !solid_only {
        let mode = match paint {
            Paint::Solid(_) => 0,
            Paint::Gradient(_) => 1,
            Paint::Image(_) => 2,
        };
        ui.horizontal(|ui| {
            for (i, label) in ["Solid", "Gradient", "Image"].into_iter().enumerate() {
                if ui.selectable_label(mode == i, label).clicked() && mode != i {
                    *paint = match i {
                        0 => paint.solid(),
                        1 => paint.gradient(),
                        _ => Paint::image(),
                    };
                }
            }
        });
        ui.separator();
    }
    match paint {
        Paint::Solid(c) => color_picker(ui, c),
        Paint::Gradient(g) => gradient_editor(ui, g, cache),
        Paint::Image(img) => image_editor(ui, img, cache),
    }
}

fn color_picker(ui: &mut Ui, rgba: &mut [u8; 4]) {
    let mut c = color(*rgba);
    if color_picker::color_picker_color32(ui, &mut c, Alpha::OnlyBlend) {
        *rgba = c.to_srgba_unmultiplied();
    }
}

fn gradient_editor(ui: &mut Ui, g: &mut Gradient, cache: &mut PaintCache) {
    let id = ui.id().with("gradient_stops");
    let mut selected: usize = ui.data(|d| d.get_temp(id)).unwrap_or(0);

    ui.horizontal(|ui| {
        ui.selectable_value(&mut g.kind, GradientKind::Linear, "Linear");
        ui.selectable_value(&mut g.kind, GradientKind::Radial, "Radial");
        if g.kind == GradientKind::Linear {
            ui.add(
                DragValue::new(&mut g.angle)
                    .suffix("°")
                    .speed(1.0)
                    .custom_parser(crate::calc::parse),
            )
            .on_hover_text("Angle");
            g.angle = g.angle.rem_euclid(360.0);
        }
        if ui.button("⇄").on_hover_text("Reverse").clicked() {
            for s in &mut g.stops {
                s.pos = 1.0 - s.pos;
            }
        }
    });
    ui.add_space(4.0);

    // The stop bar always shows the stops left to right, whatever the angle.
    let (bar, bar_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    checker(ui, bar);
    let preview = Paint::Gradient(Gradient {
        kind: GradientKind::Linear,
        angle: 0.0,
        stops: g.stops.clone(),
    });
    crate::paint::fill(ui.painter(), cache, bar, Outline::Rect(0.0), &preview, 1.0);
    let bar_resp = bar_resp.on_hover_text("Click to add a stop");
    let pos_at = |x: f32| ((x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
    if bar_resp.clicked()
        && let Some(p) = bar_resp.interact_pointer_pos()
    {
        let pos = pos_at(p.x);
        g.stops.push(Stop {
            pos,
            color: g.sample(pos),
        });
        selected = g.stops.len() - 1;
    }

    // Draggable stop handles under the bar.
    let (row, _) = ui.allocate_exact_size(vec2(bar.width(), 18.0), Sense::hover());
    selected = selected.min(g.stops.len() - 1);
    for (i, stop) in g.stops.iter_mut().enumerate() {
        let x = egui::lerp(bar.x_range(), stop.pos);
        let handle = Rect::from_center_size(pos2(x, row.center().y + 2.0), vec2(12.0, 12.0));
        let resp = ui.interact(handle, id.with(i), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            selected = i;
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            stop.pos = pos_at(p.x);
        }
        let painter = ui.painter();
        let ring = if i == selected {
            Color32::WHITE
        } else {
            Color32::from_gray(120)
        };
        painter.line_segment(
            [pos2(x, bar.max.y), pos2(x, handle.min.y)],
            Stroke::new(1.0, ring),
        );
        painter.rect_filled(handle, 3.0, color(stop.color));
        painter.rect_stroke(handle, 3.0, Stroke::new(2.0, ring), StrokeKind::Outside);
    }

    ui.horizontal(|ui| {
        let stop = &mut g.stops[selected];
        ui.label("Stop");
        ui.add(
            DragValue::from_get_set(|v| {
                if let Some(v) = v {
                    stop.pos = (v as f32 / 100.0).clamp(0.0, 1.0);
                }
                (stop.pos * 100.0) as f64
            })
            .range(0.0..=100.0)
            .max_decimals(0)
            .suffix("%")
            .custom_parser(crate::calc::parse),
        );
        let can_remove = g.stops.len() > 2;
        if ui.add_enabled(can_remove, Button::new("Remove")).clicked() {
            g.stops.remove(selected);
            selected = selected.saturating_sub(1);
        }
    });
    color_picker(ui, &mut g.stops[selected].color);
    ui.data_mut(|d| d.insert_temp(id, selected));
}

fn image_editor(ui: &mut Ui, img: &mut ImagePaint, cache: &mut PaintCache) {
    let (preview, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    checker(ui, preview);
    crate::paint::fill(
        ui.painter(),
        cache,
        preview,
        Outline::Rect(0.0),
        &Paint::Image(img.clone()),
        1.0,
    );

    ui.horizontal(|ui| {
        let label = if img.path.is_empty() {
            "Choose image…"
        } else {
            "Replace…"
        };
        if ui.button(label).clicked()
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
                .pick_file()
        {
            img.path = path.display().to_string();
            cache.reload(&img.path);
        }
        let name = std::path::Path::new(&img.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ui.add(egui::Label::new(egui::RichText::new(name).weak()).truncate());
    });
    if !img.path.is_empty() && !cache.loads(ui.ctx(), &img.path) {
        ui.colored_label(ui.visuals().error_fg_color, "Couldn't load this image");
    }

    ui.horizontal(|ui| {
        for fit in ImageFit::ALL {
            ui.selectable_value(&mut img.fit, fit, fit.label());
        }
    });
    if img.fit == ImageFit::Tile {
        ui.horizontal(|ui| {
            ui.label("Scale");
            ui.add(
                DragValue::from_get_set(|v| {
                    if let Some(v) = v {
                        img.scale = (v as f32 / 100.0).max(0.01);
                    }
                    (img.scale * 100.0) as f64
                })
                .range(1.0..=1000.0)
                .max_decimals(0)
                .suffix("%")
                .custom_parser(crate::calc::parse),
            );
        });
    }
    ui.add(
        egui::Slider::new(&mut img.opacity, 0.0..=1.0)
            .text("Opacity")
            .custom_parser(crate::calc::parse),
    );
}

/// A checkerboard so transparency reads as transparent.
fn checker(ui: &Ui, rect: Rect) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0.0, Color32::from_gray(200));
    let s = 6.0;
    let (cols, rows) = (
        (rect.width() / s).ceil() as i32,
        (rect.height() / s).ceil() as i32,
    );
    for y in 0..rows {
        for x in (y % 2..cols).step_by(2) {
            let min = rect.min + vec2(x as f32, y as f32) * s;
            painter.rect_filled(
                Rect::from_min_size(min, Vec2::splat(s)),
                0.0,
                Color32::from_gray(150),
            );
        }
    }
}
