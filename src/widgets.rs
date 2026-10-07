//! Tile widgets for the floating entity toolbar.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, DragValue, FontId, Layout, Painter, Popup,
    PopupCloseBehavior, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui, UiBuilder,
    Vec2, pos2, vec2,
};
use egui::widgets::color_picker::{self, Alpha};

use crate::components::color;
use crate::paint::{Paint, PaintCache};
use crate::paint_editor;

pub const TILE: f32 = 44.0;
const TILE_RADIUS: f32 = 10.0;
const BAR_BG: Color32 = Color32::from_gray(46);
const DIM: Color32 = Color32::from_gray(90);
const MID: Color32 = Color32::from_gray(140);
const BRIGHT: Color32 = Color32::from_gray(235);

/// The rounded pill the tiles sit in.
pub fn bar() -> egui::Frame {
    egui::Frame::new()
        .fill(BAR_BG)
        .corner_radius(TILE_RADIUS + 10.0)
        .inner_margin(10.0)
        .shadow(egui::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(90),
        })
}

/// What a tile edits, shown by which parts of its outline light up.
#[derive(Clone, Copy)]
pub enum Glyph {
    X,
    Y,
    Width,
    Height,
    Radius,
    /// Lit all round, drawn at (roughly) the border's width.
    Border(f32),
    FontSize,
}

impl Glyph {
    /// Lit sides (left, top, right, bottom), lit corners, and a corner caption.
    fn parts(self) -> ([bool; 4], bool, Option<&'static str>) {
        match self {
            Glyph::X => ([true, false, false, false], false, Some("X")),
            Glyph::Y => ([false, true, false, false], false, Some("Y")),
            Glyph::Width => ([true, false, true, false], false, None),
            Glyph::Height => ([false, true, false, true], false, None),
            Glyph::Radius => ([false; 4], true, None),
            Glyph::Border(_) => ([true; 4], true, None),
            Glyph::FontSize => ([false; 4], false, Some("T")),
        }
    }
}

/// A square number tile: drag sideways to adjust, click to type.
pub fn number_tile(ui: &mut Ui, glyph: Glyph, value: DragValue<'_>, hint: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    tile_style(child.style_mut());
    let resp = child.add(value.custom_parser(crate::calc::parse));
    let active = resp.hovered() || resp.dragged() || resp.has_focus();
    outline(ui.painter(), rect, glyph, active);
    resp.on_hover_text(hint)
}

/// Strips the DragValue's own frame so only our outline shows.
fn tile_style(style: &mut egui::Style) {
    style.drag_value_text_style = TextStyle::Heading;
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(15.0));
    style.spacing.interact_size = Vec2::splat(TILE);
    style.spacing.button_padding = vec2(4.0, 0.0);
    let v = &mut style.visuals;
    v.override_text_color = Some(BRIGHT);
    v.text_edit_bg_color = Some(Color32::TRANSPARENT);
    v.extreme_bg_color = Color32::TRANSPARENT;
    v.selection.stroke = Stroke::NONE;
    for (w, bg) in [
        (&mut v.widgets.inactive, Color32::TRANSPARENT),
        (&mut v.widgets.hovered, Color32::from_white_alpha(8)),
        (&mut v.widgets.active, Color32::from_white_alpha(12)),
    ] {
        w.bg_fill = bg;
        w.weak_bg_fill = bg;
        w.bg_stroke = Stroke::NONE;
        w.corner_radius = CornerRadius::same(TILE_RADIUS as u8);
        w.expansion = 0.0;
    }
}

/// The tile's rounded outline, dim except for the parts the glyph lights up.
fn outline(painter: &Painter, rect: Rect, glyph: Glyph, active: bool) {
    let (sides, corners, caption) = glyph.parts();
    let lit_width = match glyph {
        Glyph::Border(w) => w.clamp(1.5, 4.0),
        _ => 2.0,
    };
    let dim = Stroke::new(1.5, if active { MID } else { DIM });
    let lit = Stroke::new(lit_width, BRIGHT);
    let r = rect.shrink(lit_width / 2.0);
    let k = TILE_RADIUS;
    let side_lines = [
        [pos2(r.min.x, r.min.y + k), pos2(r.min.x, r.max.y - k)],
        [pos2(r.min.x + k, r.min.y), pos2(r.max.x - k, r.min.y)],
        [pos2(r.max.x, r.min.y + k), pos2(r.max.x, r.max.y - k)],
        [pos2(r.min.x + k, r.max.y), pos2(r.max.x - k, r.max.y)],
    ];
    for (line, on) in side_lines.into_iter().zip(sides) {
        painter.line_segment(line, if on { lit } else { dim });
    }
    // Corner arcs, clockwise from top-left (angles in screen space, y down).
    let arcs = [
        (pos2(r.min.x + k, r.min.y + k), 180.0),
        (pos2(r.max.x - k, r.min.y + k), 270.0),
        (pos2(r.max.x - k, r.max.y - k), 0.0),
        (pos2(r.min.x + k, r.max.y - k), 90.0),
    ];
    for (center, start) in arcs {
        painter.line(arc(center, k, start), if corners { lit } else { dim });
    }
    if let Some(text) = caption {
        painter.text(
            rect.min + vec2(6.0, 4.0),
            Align2::LEFT_TOP,
            text,
            FontId::proportional(9.0),
            MID,
        );
    }
}

fn arc(center: Pos2, radius: f32, start_deg: f32) -> Vec<Pos2> {
    (0..=8)
        .map(|i| {
            let a = (start_deg + i as f32 * 90.0 / 8.0).to_radians();
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// Overlapping swatches: the solid front square is the fill, the outlined one behind is the border.
pub fn color_pair(
    ui: &mut Ui,
    fill: &mut Paint,
    border: Option<&mut [u8; 4]>,
    fill_hint: &str,
    solid_only: bool,
    cache: &mut PaintCache,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::hover());
    let id = ui.id().with("color_pair");
    let size = Vec2::splat(TILE * 0.7);
    let front = match border {
        Some(_) => Rect::from_min_size(pos2(rect.min.x, rect.max.y - size.y), size),
        None => Rect::from_center_size(rect.center(), size),
    };

    if let Some(border) = border {
        let back = Rect::from_min_size(pos2(rect.max.x - size.x, rect.min.y), size);
        let resp = ui.interact(back, id.with("border"), Sense::click());
        let ring = if resp.hovered() { 3.0 } else { 2.0 };
        let p = ui.painter();
        p.rect_filled(back, 8.0, BAR_BG);
        p.rect_stroke(
            back,
            8.0,
            Stroke::new(ring, color(*border)),
            StrokeKind::Inside,
        );
        color_popup(&resp.on_hover_text("Border color"), border);
    }

    let resp = ui.interact(front, id.with("fill"), Sense::click());
    paint_editor::draw_swatch(ui, front, 8.0, fill, cache, resp.hovered());
    paint_editor::popup(&resp.on_hover_text(fill_hint), fill, solid_only, cache);
}

/// Toggles clipping; the square spilling out of the tile is cut off when on.
pub fn clip_tile(ui: &mut Ui, clip: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::click());
    if resp.clicked() {
        *clip = !*clip;
        resp.mark_changed();
    }
    let active = resp.hovered();
    let glyph = if *clip {
        Glyph::Border(1.5)
    } else {
        Glyph::Radius
    };
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, TILE_RADIUS, Color32::from_white_alpha(8));
    }
    outline(painter, rect, glyph, active);
    let inner = Rect::from_min_size(rect.center() - Vec2::splat(4.0), Vec2::splat(TILE * 0.55));
    let (p, fill) = if *clip {
        let clip_rect = rect.shrink(3.0).intersect(painter.clip_rect());
        (painter.with_clip_rect(clip_rect), BRIGHT)
    } else {
        (painter.clone(), MID)
    };
    p.rect_filled(inner, 4.0, fill);
    let state = if *clip { "on" } else { "off" };
    resp.on_hover_text(format!("Clip content: {state}"))
}

/// A color picker that opens from `resp` and edits `color` in place.
fn color_popup(resp: &Response, value: &mut [u8; 4]) {
    let mut c = color(*value);
    let mut changed = false;
    Popup::menu(resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().slider_width = 275.0;
            changed = color_picker::color_picker_color32(ui, &mut c, Alpha::OnlyBlend);
        });
    if changed {
        *value = c.to_srgba_unmultiplied();
    }
}
