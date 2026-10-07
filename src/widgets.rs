//! Tile widgets for the floating entity toolbar.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Layout, Painter, Popup, PopupCloseBehavior, Pos2,
    Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2, pos2, vec2,
};
use egui::widgets::color_picker::{self, Alpha};

use crate::components::{Anchor, color};
use crate::glass;
use crate::paint::{Paint, PaintCache};
use crate::paint_editor;
use crate::scrub::Scrub;

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

/// Glass uniform slots for the floating bars; 0 and 1 are the side panels.
pub const ENTITY_SLOT: usize = 2;
pub const TOOLS_SLOT: usize = 3;
pub const STATS_SLOT: usize = 4;

/// The toolbar pill over liquid glass, or the flat `bar` when glass can't render.
pub fn glass_bar<R>(
    ui: &mut Ui,
    glass: Option<glass::Settings>,
    slot: usize,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let Some(mut settings) = glass else {
        return bar().show(ui, add_contents).inner;
    };
    let radius = TILE_RADIUS + 10.0;
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::new()
        .corner_radius(radius)
        .inner_margin(10.0)
        .show(ui, add_contents);
    let rect = inner.response.rect;
    // Same look as the side panels; only the shape differs, as the bevel must fit the short pill.
    settings.radius = radius;
    settings.bevel = settings.bevel.min(rect.height() / 2.0);
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    let panel = glass::Panel {
        slot,
        rect,
        settings,
        backdrop: None,
    };
    ui.painter().set(
        background,
        egui::Shape::Vec(vec![
            shadow.as_shape(rect, CornerRadius::from(radius)).into(),
            panel.shape(),
        ]),
    );
    inner.inner
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
    Rotation,
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
            Glyph::Rotation => ([false; 4], false, Some("°")),
        }
    }
}

/// A square number tile: drag sideways to adjust, click to type.
pub fn number_tile(ui: &mut Ui, glyph: Glyph, value: Scrub<'_>, hint: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    tile_style(child.style_mut());
    let resp = child.add(value);
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

/// A 3×3 grid of dots picking which point stays put when the size changes.
pub fn anchor_grid(ui: &mut Ui, anchor: &mut Anchor, size: f32) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let gap = size * 0.08;
    let cell = (size - gap * 2.0) / 3.0;
    let hover = resp.hover_pos();
    for row in 0..3u8 {
        for col in 0..3u8 {
            let min = rect.min + vec2(col as f32, row as f32) * (cell + gap);
            let r = Rect::from_min_size(min, Vec2::splat(cell));
            let on = anchor.col == col && anchor.row == row;
            let hot = hover.is_some_and(|p| r.expand(gap / 2.0).contains(p));
            if resp.clicked() && hot {
                *anchor = Anchor { col, row };
                resp.mark_changed();
            }
            let radius = cell * 0.3;
            let p = ui.painter();
            if on {
                p.rect_filled(r, radius, BRIGHT);
            } else {
                let c = if hot { MID } else { DIM };
                p.rect_stroke(r, radius, Stroke::new(1.5, c), StrokeKind::Inside);
            }
        }
    }
    resp.on_hover_text("Anchor point")
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

/// What an arrange button does, drawn as its icon.
#[derive(Clone, Copy)]
pub enum ArrangeIcon {
    /// Align along x (`true`) or y at 0 start, 0.5 centre, 1 end.
    Align(bool, f32),
    /// Even spacing along x (`true`) or y.
    Distribute(bool),
}

/// A small icon button for aligning or distributing the selection.
pub fn arrange_button(ui: &mut Ui, icon: ArrangeIcon, enabled: bool, hint: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
    let resp = if enabled { resp } else { resp.on_disabled_hover_text(hint) };
    let p = ui.painter();
    let ink = match (enabled, resp.hovered()) {
        (false, _) => DIM,
        (true, true) => BRIGHT,
        (true, false) => MID,
    };
    if enabled && resp.hovered() {
        p.rect_filled(rect, 6.0, Color32::from_white_alpha(10));
    }
    let r = rect.shrink(7.0);
    // Icons are drawn for the x axis; y swaps the coordinates.
    let (ArrangeIcon::Align(horizontal, _) | ArrangeIcon::Distribute(horizontal)) = icon;
    let map = |u: f32, v: f32| {
        let (x, y) = if horizontal { (u, v) } else { (v, u) };
        pos2(r.min.x + x * r.width(), r.min.y + y * r.height())
    };
    let bar = |u0: f32, u1: f32, v0: f32, v1: f32| {
        p.rect_filled(Rect::from_two_pos(map(u0, v0), map(u1, v1)), 1.0, ink);
    };
    match icon {
        ArrangeIcon::Align(_, at) => {
            p.line_segment([map(at, -0.15), map(at, 1.15)], Stroke::new(1.5, ink));
            for (len, v) in [(0.9, 0.1), (0.55, 0.6)] {
                let u0 = at * (1.0 - len);
                bar(u0, u0 + len, v, v + 0.3);
            }
        }
        ArrangeIcon::Distribute(_) => {
            for u in [0.0, 0.42, 0.84] {
                bar(u, u + 0.16, 0.1, 0.9);
            }
        }
    }
    resp.on_hover_text(hint)
}

/// Icons for the bottom tools bar.
#[derive(Clone, Copy)]
pub enum ToolIcon {
    Move,
    Frame,
    Rect,
    Ellipse,
    Text,
}

/// A tile picking a drawing tool, lit while `selected`.
pub fn tool_tile(ui: &mut Ui, icon: ToolIcon, selected: bool, hint: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(TILE), Sense::click());
    let p = ui.painter();
    let hovered = resp.hovered();
    if selected {
        p.rect_filled(rect, TILE_RADIUS, Color32::from_white_alpha(28));
    } else if hovered {
        p.rect_filled(rect, TILE_RADIUS, Color32::from_white_alpha(8));
    }
    let ink = if selected || hovered { BRIGHT } else { MID };
    let stroke = Stroke::new(1.5, ink);
    let r = Rect::from_center_size(rect.center(), Vec2::splat(18.0));
    match icon {
        ToolIcon::Move => {
            let pts = [(0.15, 0.0), (0.15, 0.85), (0.38, 0.64), (0.55, 1.0), (0.68, 0.94), (0.52, 0.58), (0.85, 0.58)];
            let pts = pts.map(|(x, y)| pos2(r.min.x + x * r.width(), r.min.y + y * r.height()));
            p.add(egui::Shape::closed_line(pts.to_vec(), stroke));
        }
        ToolIcon::Frame => {
            let (a, b) = (0.3, 0.7);
            for t in [a, b] {
                let x = r.min.x + t * r.width();
                let y = r.min.y + t * r.height();
                p.line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], stroke);
                p.line_segment([pos2(r.min.x, y), pos2(r.max.x, y)], stroke);
            }
        }
        ToolIcon::Rect => {
            p.rect_stroke(r.shrink(1.0), 2.0, stroke, StrokeKind::Middle);
        }
        ToolIcon::Ellipse => {
            p.circle_stroke(r.center(), r.width() / 2.0 - 1.0, stroke);
        }
        ToolIcon::Text => {
            p.text(r.center(), Align2::CENTER_CENTER, "T", FontId::proportional(20.0), ink);
        }
    }
    resp.on_hover_text(hint)
}
