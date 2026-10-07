use std::path::PathBuf;

use eframe::egui::{
    self, Color32, CursorIcon, FontId, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Sense,
    Stroke, StrokeKind, Vec2, pos2, vec2,
};

use crate::calc;
use crate::components::{Anchor, Hidden, Parent, ShapeKind, Transform};
use egui::emath::Rot2;
use crate::contrast;
use crate::glass;
use crate::ecs::Entity;
use crate::paint::PaintCache;
use crate::paint_editor;
use crate::resources::{Background, Shimmer, SnapSettings, snap, snap_pos};
use crate::scrub::Scrub;
use crate::systems::render;
use crate::widgets::{self, Glyph, number_tile};
use crate::world::World;

const ACCENT: Color32 = Color32::from_rgb(13, 153, 255);
const GUIDE: Color32 = Color32::from_rgb(242, 72, 34);
/// How close (in screen pixels) an edge must be to snap to another shape.
const GUIDE_SNAP: f32 = 6.0;
const HANDLE_SIZE: f32 = 8.0;
/// How far outside a corner (in screen pixels) dragging rotates instead of resizing.
const ROTATE_ZONE: f32 = 20.0;
/// Shift-drag rotation snaps to multiples of this many degrees.
const ROTATE_SNAP: f32 = 15.0;
/// Screen gap between the entity toolbar and its entity, clearing frame labels and the size badge.
const TOOLBAR_GAP: f32 = 30.0;
const MAX_UNDO: usize = 200;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Frame,
    Rect,
    Ellipse,
    Text,
}

const TOOLS: [Tool; 5] = [
    Tool::Select,
    Tool::Frame,
    Tool::Rect,
    Tool::Ellipse,
    Tool::Text,
];

impl Tool {
    /// The shape kind a drawing tool creates.
    fn shape_kind(self) -> Option<ShapeKind> {
        match self {
            Tool::Frame => Some(ShapeKind::Frame),
            Tool::Rect => Some(ShapeKind::Rect),
            Tool::Ellipse => Some(ShapeKind::Ellipse),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tool::Select => "Move",
            Tool::Frame => "Frame",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
        }
    }

    fn icon(self) -> widgets::ToolIcon {
        match self {
            Tool::Select => widgets::ToolIcon::Move,
            Tool::Frame => widgets::ToolIcon::Frame,
            Tool::Rect => widgets::ToolIcon::Rect,
            Tool::Ellipse => widgets::ToolIcon::Ellipse,
            Tool::Text => widgets::ToolIcon::Text,
        }
    }

    fn key(self) -> Key {
        match self {
            Tool::Select => Key::V,
            Tool::Frame => Key::F,
            Tool::Rect => Key::R,
            Tool::Ellipse => Key::O,
            Tool::Text => Key::T,
        }
    }
}

/// An alignment line: `at` on one axis, spanning `from..to` on the other.
struct Guide {
    vertical: bool,
    at: f32,
    from: f32,
    to: f32,
    /// Extensions out to an off-line neighbour are drawn faint and dashed.
    faint: bool,
}

/// A measured distance from `a` to `b` along x (`horizontal`) or y, drawn at `cross`.
struct Gap {
    horizontal: bool,
    a: f32,
    b: f32,
    cross: f32,
}

/// Snapping feedback for the shape being moved, resized or drawn.
struct Alignment {
    /// The snapped rect.
    rect: Rect,
    guides: Vec<Guide>,
    gaps: Vec<Gap>,
    /// Shapes it lines up with exactly.
    aligned: Vec<Entity>,
}

impl Default for Alignment {
    fn default() -> Self {
        Self {
            rect: Rect::NOTHING,
            guides: Vec::new(),
            gaps: Vec::new(),
            aligned: Vec::new(),
        }
    }
}

/// Which edges (min, centre, max) are moving, for x then y.
type EdgeMask = [[bool; 3]; 2];
const ALL_EDGES: EdgeMask = [[true; 3]; 2];

/// A resize handle, as a direction from the center: each component is -1, 0 or 1.
type Handle = (i8, i8);

enum Drag {
    None,
    Pan,
    Move {
        start: Pos2,
        originals: Vec<(Entity, Rect)>,
    },
    Resize {
        id: Entity,
        handle: Handle,
        start: Pos2,
        original: Rect,
    },
    Create {
        id: Entity,
        start: Pos2,
    },
    Rotate {
        id: Entity,
        /// World-space pivot, the shape's centre.
        center: Pos2,
        /// Pointer angle (radians) when the drag began.
        start: f32,
        /// Rotation (degrees) when the drag began.
        original: f32,
    },
    Marquee {
        start: Pos2,
        base: Vec<Entity>,
    },
}

pub struct SkwiggleApp {
    world: World,
    selection: Vec<Entity>,
    tool: Tool,
    /// Screen-space offset of the world origin from the canvas's top-left.
    pan: Vec2,
    zoom: f32,
    drag: Drag,
    undo: Vec<World>,
    redo: Vec<World>,
    path: Option<PathBuf>,
    editing_text: Option<Entity>,
    focus_text: bool,
    /// True while a run of edits in the properties panel shares one undo entry.
    panel_edit: bool,
    /// Timestamps of frames painted in the last second, for the FPS readout.
    frame_times: std::collections::VecDeque<f64>,
    /// Window title last sent to the OS.
    title: String,
    alignment: Alignment,
    /// Textures for gradient and image fills.
    paints: PaintCache,
    /// True while a panel value is being dragged, so the canvas shows guides for it.
    scrubbing: bool,
    /// Entity toolbar placement, held still while the pointer is down.
    toolbar_pin: Option<(Vec<Entity>, Pos2, egui::Align2)>,
    /// Shimmer animation time, advanced by `shimmer.speed` each frame.
    shimmer_clock: f32,
    /// Widths of the floating layers and properties panels.
    panel_widths: [f32; 2],
    glass: glass::Settings,
    /// Whether glass rendered last frame; the floating toolbars fall back to flat otherwise.
    glass_live: bool,
    #[cfg(target_os = "macos")]
    menu: Option<std::rc::Rc<crate::menu::NativeMenu>>,
}

impl Default for SkwiggleApp {
    fn default() -> Self {
        Self {
            world: World::default(),
            selection: Vec::new(),
            tool: Tool::Select,
            pan: vec2(100.0, 100.0),
            zoom: 1.0,
            drag: Drag::None,
            undo: Vec::new(),
            redo: Vec::new(),
            path: None,
            editing_text: None,
            focus_text: false,
            panel_edit: false,
            title: String::new(),
            frame_times: Default::default(),
            alignment: Alignment::default(),
            paints: PaintCache::default(),
            scrubbing: false,
            toolbar_pin: None,
            shimmer_clock: 0.0,
            panel_widths: [220.0, 260.0],
            glass: glass::Settings::default(),
            glass_live: false,
            #[cfg(target_os = "macos")]
            menu: None,
        }
    }
}

impl SkwiggleApp {
    pub fn new(ctx: &egui::Context) -> Self {
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;
        Self {
            #[cfg(target_os = "macos")]
            menu: crate::menu::NativeMenu::install(ctx).map(std::rc::Rc::new),
            ..Self::default()
        }
    }

    /// Runs commands picked from the native menu bar.
    fn menu_commands(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            use crate::menu::Command;
            let Some(menu) = &self.menu else { return };
            menu.set_history(!self.undo.is_empty(), !self.redo.is_empty());
            let picked = menu.poll();
            let typing = ctx.egui_wants_keyboard_input();
            for c in picked {
                match c {
                    Command::New => self.new_doc(),
                    Command::Open => self.open(),
                    Command::Save => self.save(false),
                    Command::SaveAs => self.save(true),
                    // Text fields keep ⌘Z for their own undo.
                    Command::Undo if !typing => self.undo(),
                    Command::Redo if !typing => self.redo(),
                    Command::Undo | Command::Redo => {}
                }
            }
        }
    }

    /// Shows the open file's name in the window title.
    fn sync_title(&mut self, ctx: &egui::Context) {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|n| n.to_string_lossy());
        let title = match name {
            Some(n) => format!("{n} — Skwiggle"),
            None => "Skwiggle".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Clears to a blank document, keeping app-level state like the menu bar.
    fn reset(&mut self) {
        #[cfg(target_os = "macos")]
        let menu = self.menu.take();
        *self = Self::default();
        #[cfg(target_os = "macos")]
        {
            self.menu = menu;
        }
    }
}

impl eframe::App for SkwiggleApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        calc::begin_frame(ui.ctx());
        let now = ui.input(|i| i.time);
        self.frame_times.push_back(now);
        while self.frame_times.front().is_some_and(|&t| now - t > 1.0) {
            self.frame_times.pop_front();
        }
        self.menu_commands(ui.ctx());
        self.shortcuts(ui.ctx());

        self.sync_title(ui.ctx());
        let canvas = egui::CentralPanel::no_frame()
            .show(ui, |ui| self.canvas(ui))
            .response;
        self.floating_panels(ui.ctx(), frame, canvas.rect, canvas.layer_id);
        self.tools_bar(ui.ctx(), canvas.rect);

        // A run of direct edits ends once the pointer and keyboard are let go.
        let ctx = ui.ctx();
        if !ctx.input(|i| i.pointer.any_down()) && !ctx.egui_wants_keyboard_input() {
            self.panel_edit = false;
        }
        if self.scrubbing && !ctx.input(|i| i.pointer.any_down()) {
            self.scrubbing = false;
            self.alignment = Alignment::default();
        }
        self.paints.end_frame();
        calc::show_preview(ctx);
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

impl SkwiggleApp {
    /// Runs UI that edits the world directly, folding a run of edits (e.g. one drag) into one undo step.
    fn coalesce_edits(&mut self, f: impl FnOnce(&mut Self)) {
        let before = self.world.clone();
        f(self);
        let moved = self
            .selection
            .iter()
            .any(|&e| before.rect(e) != self.world.rect(e));
        if moved {
            self.scrubbing = true;
        }
        // Commands that checkpoint themselves have already pushed `before`.
        if self.world != before && !self.panel_edit && self.undo.last() != Some(&before) {
            self.undo.push(before);
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.panel_edit = true;
        }
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.world.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.world, prev));
            self.after_history_change();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.world, next));
            self.after_history_change();
        }
    }

    fn after_history_change(&mut self) {
        let world = &self.world;
        self.selection.retain(|id| world.alive(*id));
        self.editing_text = None;
        self.drag = Drag::None;
        self.alignment = Alignment::default();
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let sel = std::mem::take(&mut self.selection);
        self.world.remove(&sel);
    }

    fn duplicate_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let step = self.world.snap.position_step();
        let offset = snap(10.0, step).max(step);
        self.selection = self.world.duplicate(&self.selection, Vec2::splat(offset));
    }

    /// Moves the selection by `steps` grid steps, landing on the grid; or by
    /// that many pixels when position snapping is off.
    fn nudge(&mut self, steps: Vec2) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let step = self.world.snap.position_step();
        let distance = steps * step.max(1.0);
        // Children ride along with their frame by the same (snapped) offset.
        for root in self.world.topmost(&self.selection) {
            let r = self.world.rect(root).min;
            let d = vec2(
                snap(r.x + distance.x, step) - r.x,
                snap(r.y + distance.y, step) - r.y,
            );
            for id in self.world.with_descendants(&[root]) {
                if let Some(t) = self.world.transforms.get_mut(id) {
                    t.x += d.x;
                    t.y += d.y;
                }
            }
        }
    }

    /// Moves the selection to the top (`true`) or bottom of the z-order.
    fn reorder(&mut self, to_front: bool) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let (picked, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.world.entities)
            .into_iter()
            .partition(|e| self.selection.contains(e));
        self.world.entities = if to_front {
            rest.into_iter().chain(picked).collect()
        } else {
            picked.into_iter().chain(rest).collect()
        };
    }

    fn new_doc(&mut self) {
        self.reset();
    }

    fn open(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Skwiggle", &["skw"])
            .pick_file()
        else {
            return;
        };
        match World::load(&path) {
            Ok(doc) => {
                self.reset();
                self.world = doc;
                self.path = Some(path);
            }
            Err(e) => error_dialog("Couldn’t open the file", &e.to_string()),
        }
    }

    fn save(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new()
                .add_filter("Skwiggle", &["skw"])
                .set_file_name("Untitled.skw")
                .save_file()
            {
                Some(p) => p,
                None => return,
            },
        };
        match self.world.save(&path) {
            Ok(()) => self.path = Some(path),
            Err(e) => error_dialog("Couldn’t save the file", &e.to_string()),
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;

        // Shortcuts that should work even while a text field has focus.
        if ctx.input_mut(|i| i.consume_key(cmd, Key::S)) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::O)) {
            self.open();
        }

        // Everything else is suppressed while typing (TextEdit handles Escape itself).
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        if ctx.input_mut(|i| i.consume_key(cmd_shift, Key::Z)) {
            self.redo();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Z)) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::D)) {
            self.duplicate_selection();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::A)) {
            self.selection = self.world.entities.clone();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::Num0)) {
            self.zoom = 1.0;
        }

        let (delete, escape, front, back, arrows, shift) = ctx.input(|i| {
            let steps = if i.modifiers.shift { 10.0 } else { 1.0 };
            let mut d = Vec2::ZERO;
            for (key, dir) in [
                (Key::ArrowLeft, vec2(-1.0, 0.0)),
                (Key::ArrowRight, vec2(1.0, 0.0)),
                (Key::ArrowUp, vec2(0.0, -1.0)),
                (Key::ArrowDown, vec2(0.0, 1.0)),
            ] {
                if i.key_pressed(key) {
                    d += dir * steps;
                }
            }
            (
                i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace),
                i.key_pressed(Key::Escape),
                i.key_pressed(Key::CloseBracket),
                i.key_pressed(Key::OpenBracket),
                d,
                i.modifiers.shift,
            )
        });
        if delete {
            self.delete_selection();
        }
        if escape {
            self.selection.clear();
            self.tool = Tool::Select;
        }
        if front || back {
            self.reorder(front);
        }
        if arrows != Vec2::ZERO {
            self.nudge(arrows);
        }
        if !shift && !ctx.input(|i| i.modifiers.command) {
            for t in TOOLS {
                if ctx.input(|i| i.key_pressed(t.key())) {
                    self.tool = t;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

impl SkwiggleApp {
    /// Glass panels floating over the canvas, which shows through blurred.
    fn floating_panels(
        &mut self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        canvas: Rect,
        canvas_layer: egui::LayerId,
    ) {
        const MARGIN: f32 = 8.0;
        let bg = canvas_bg(ctx.global_style().visuals.dark_mode);
        let mut backdrop = glass::Backdrop::capture(ctx, frame.wgpu_render_state(), canvas_layer, bg);
        // Without wgpu the shader can't draw the glass, so fall back to a flat tint.
        let flat = backdrop.is_none();
        self.glass_live = !flat;
        let inner = canvas.shrink(MARGIN);
        for (slot, left) in [(0, true), (1, false)] {
            let max_w = (inner.width() * 0.45).max(120.0);
            let w = self.panel_widths[slot].clamp(120.0, max_w);
            let rect = if left {
                Rect::from_min_size(inner.min, vec2(w, inner.height()))
            } else {
                Rect::from_min_size(pos2(inner.max.x - w, inner.min.y), vec2(w, inner.height()))
            };
            let id = egui::Id::new(("glass_panel", slot));
            egui::Area::new(id)
                .order(egui::Order::Middle)
                .fixed_pos(rect.min)
                .constrain(false)
                .show(ctx, |ui| {
                    ui.set_min_size(rect.size());
                    ui.set_max_size(rect.size());
                    let painter = ui.painter();
                    painter.add(
                        glass::Panel {
                            slot,
                            rect,
                            settings: self.glass,
                            backdrop: backdrop.take(),
                        }
                        .shape(),
                    );
                    if flat {
                        painter.rect(
                            rect,
                            self.glass.radius,
                            Color32::from_black_alpha(90),
                            Stroke::new(1.0, Color32::from_white_alpha(28)),
                            StrokeKind::Inside,
                        );
                    }

                    // Drag the inner edge to resize.
                    let edge = if left { rect.max.x } else { rect.min.x };
                    let handle = Rect::from_x_y_ranges(edge - 4.0..=edge + 4.0, rect.y_range());
                    let resp = ui.interact(handle, id.with("resize"), Sense::drag());
                    if resp.hovered() || resp.dragged() {
                        ui.set_cursor_icon(CursorIcon::ResizeHorizontal);
                    }
                    if resp.dragged() {
                        let dx = resp.drag_delta().x;
                        self.panel_widths[slot] = w + if left { dx } else { -dx };
                    }

                    let content = rect.shrink2(vec2(12.0, 10.0));
                    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(content));
                    ui.set_clip_rect(content);
                    if left {
                        self.layers_panel(&mut ui);
                    } else {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(&mut ui, |ui| {
                                self.coalesce_edits(|app| app.properties_panel(ui))
                            });
                    }
                });
        }
        let right = inner.max.x - self.panel_widths[1].clamp(120.0, (inner.width() * 0.45).max(120.0));
        self.stats(ctx, pos2(right - MARGIN, inner.min.y));
    }

    /// A small FPS and entity count readout, its top-right corner at `anchor`.
    fn stats(&self, ctx: &egui::Context, anchor: Pos2) {
        let glass = self.glass_live.then_some(self.glass);
        let text = format!("{} fps · {} entities", self.frame_times.len(), self.world.entities.len());
        egui::Area::new(egui::Id::new("stats"))
            .order(egui::Order::Middle)
            .fixed_pos(anchor)
            .pivot(egui::Align2::RIGHT_TOP)
            .interactable(false)
            .show(ctx, |ui| {
                widgets::glass_bar(ui, glass, widgets::STATS_SLOT, |ui| {
                    ui.label(egui::RichText::new(text).monospace().small().color(Color32::from_gray(200)));
                });
            });
    }

    /// The bottom-centre bar of creation tools.
    fn tools_bar(&mut self, ctx: &egui::Context, canvas: Rect) {
        const MARGIN: f32 = 16.0;
        let glass = self.glass_live.then_some(self.glass);
        egui::Area::new(egui::Id::new("tools_bar"))
            .fixed_pos(pos2(canvas.center().x, canvas.max.y - MARGIN))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .constrain_to(canvas)
            .show(ctx, |ui| {
                widgets::glass_bar(ui, glass, widgets::TOOLS_SLOT, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for t in TOOLS {
                            let hint = format!("{} ({:?})", t.label(), t.key());
                            if widgets::tool_tile(ui, t.icon(), self.tool == t, &hint).clicked() {
                                self.tool = t;
                            }
                        }
                    });
                });
            });
    }

    /// Sliders for tuning the floating panels' glass look.
    fn glass_settings(&mut self, ui: &mut egui::Ui) {
        let g = &mut self.glass;
        let slider = |ui: &mut egui::Ui, v: &mut f32, range, text: &str, hint: &str| {
            ui.add(egui::Slider::new(v, range).text(text).custom_parser(calc::parse))
                .on_hover_text(hint);
        };
        slider(ui, &mut g.tint, 0.0..=1.2, "Tint", "Backdrop brightness; 1 is untinted");
        slider(ui, &mut g.saturation, 0.0..=2.5, "Saturation", "Backdrop colour boost");
        slider(ui, &mut g.frost, 0.0..=1.0, "Frost", "Mix of sharp (0) and blurred (1) backdrop");
        slider(ui, &mut g.blur_spread, 0.0..=8.0, "Blur spread", "Width of each blur pass");
        ui.add(egui::Slider::new(&mut g.blur_passes, 1..=8).text("Blur passes"))
            .on_hover_text("More passes give a smoother, wider blur");
        slider(ui, &mut g.blur_scale, 0.1..=1.0, "Blur resolution", "Lower is cheaper and blurrier");
        slider(ui, &mut g.bevel, 0.0..=80.0, "Bevel", "Width of the curved edge, in points");
        slider(ui, &mut g.refraction, 0.0..=2.0, "Refraction", "How far the edge bends the canvas");
        slider(ui, &mut g.chroma, 0.0..=0.5, "Chromatic", "Colour split along the edge");
        slider(ui, &mut g.fresnel, 0.0..=4.0, "Edge glow", "Brightening of the curved edge");
        slider(ui, &mut g.rim, 0.0..=2.0, "Rim light", "Strength of the thin specular edge");
        slider(ui, &mut g.hairline, 0.0..=0.5, "Hairline", "Faint line just inside the rim");
        slider(ui, &mut g.light_angle, -180.0..=180.0, "Light angle", "Degrees clockwise from up");
        slider(ui, &mut g.radius, 0.0..=40.0, "Corner radius", "Panel corner radius, in points");
        let changed = *g != glass::Settings::default();
        if reset_button(ui, changed, "Restore default glass settings") {
            self.glass = glass::Settings::default();
        }
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        // Truncate long names with "…" so the panel can shrink back after a resize.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.heading("Layers");
        ui.separator();
        let mut clicked = None;
        let mut toggle_visibility = None;
        // Topmost first, children indented under their frame.
        let tree = self.world.tree();
        let mut rows = Vec::new();
        let mut stack: Vec<(Entity, usize)> = tree
            .get(&None)
            .into_iter()
            .flatten()
            .map(|&e| (e, 0))
            .collect();
        while let Some((e, depth)) = stack.pop() {
            rows.push((e, depth));
            stack.extend(
                tree.get(&Some(e))
                    .into_iter()
                    .flatten()
                    .map(|&c| (c, depth + 1)),
            );
        }
        // Only lay out the rows in view, so huge documents stay fast.
        let row_h = ui.spacing().interact_size.y;
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for &(e, depth) in &rows[range] {
                    let w = &self.world;
                    let size = vec2(ui.available_width(), row_h);
                    ui.allocate_ui_with_layout(size, layout, |ui| {
                        ui.set_min_height(row_h);
                        ui.add_space(depth as f32 * 14.0);
                        let eye = if w.visible(e) { "◉" } else { "○" };
                        if ui
                            .small_button(eye)
                            .on_hover_text("Toggle visibility")
                            .clicked()
                        {
                            toggle_visibility = Some(e);
                        }
                        let selected = self.selection.contains(&e);
                        let name = w.names.get(e).map_or("", |n| n.0.as_str());
                        let label = format!("{}  {}", w.kind(e).icon(), name);
                        if ui.selectable_label(selected, label).clicked() {
                            clicked = Some(e);
                        }
                    });
                }
            });
        if let Some(id) = toggle_visibility {
            self.checkpoint();
            if self.world.hidden.remove(id).is_none() {
                self.world.hidden.insert(id, Hidden);
            }
        }
        if let Some(id) = clicked {
            if ui.input(|i| i.modifiers.shift) {
                toggle(&mut self.selection, id);
            } else {
                self.selection = vec![id];
            }
        }
    }

    fn properties_panel(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.heading("Design");
        ui.separator();

        egui::CollapsingHeader::new("Snap")
            .default_open(true)
            .show(ui, |ui| {
                let snap = &mut self.world.snap;
                ui.horizontal(|ui| {
                    ui.label("Step");
                    ui.add(Scrub::new(&mut snap.step).range(1.0..=256.0).speed(0.1))
                        .on_hover_text("Checked properties snap to multiples of this");
                });
                ui.checkbox(&mut snap.position, "Position (move & nudge)");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut snap.width, "Width");
                    ui.checkbox(&mut snap.height, "Height");
                });
                ui.checkbox(&mut snap.radius, "Corner radius");
                ui.checkbox(&mut snap.font_size, "Font size");
                ui.checkbox(&mut snap.stroke, "Stroke width");
                let changed = self.world.snap != SnapSettings::default();
                if reset_button(ui, changed, "Restore default snap settings") {
                    self.checkpoint();
                    self.world.snap = SnapSettings::default();
                }
            });
        egui::CollapsingHeader::new("Background")
            .default_open(true)
            .show(ui, |ui| {
                for bg in Background::ALL {
                    ui.radio_value(&mut self.world.background, bg, bg.label());
                }
                if self.world.background.is_dots() {
                    let sh = &mut self.world.shimmer;
                    ui.add(
                        egui::Slider::new(&mut sh.dot_size, 1.0..=6.0)
                            .text("Dot size")
                            .custom_parser(calc::parse),
                    );
                    ui.add(
                        egui::Slider::new(&mut sh.speed, 0.0..=3.0)
                            .text("Shimmer speed")
                            .custom_parser(calc::parse),
                    );
                    ui.add(
                        egui::Slider::new(&mut sh.twinkle, 0.0..=1.5)
                            .text("Twinkle")
                            .custom_parser(calc::parse),
                    );
                    ui.add(
                        egui::Slider::new(&mut sh.wave, 0.0..=1.5)
                            .text("Wave")
                            .custom_parser(calc::parse),
                    );
                    ui.add(
                        egui::Slider::new(&mut sh.reference, 1.6..=4.5)
                            .text("100% dots")
                            .custom_parser(calc::parse),
                    )
                    .on_hover_text("Contrast of the dots visible at 100% zoom");
                } else {
                    let sh = &mut self.world.shimmer;
                    ui.add(
                        egui::Slider::new(&mut sh.line_width, 0.5..=4.0)
                            .text("Line width")
                            .custom_parser(calc::parse),
                    );
                    ui.add(
                        egui::Slider::new(&mut sh.reference_lines, 1.3..=3.5)
                            .text("100% lines")
                            .custom_parser(calc::parse),
                    )
                    .on_hover_text("Contrast of the lines visible at 100% zoom");
                }
                // Keeps the chosen pattern; only its settings go back to default.
                let changed = self.world.shimmer != Shimmer::default();
                if reset_button(ui, changed, "Restore default background settings") {
                    self.checkpoint();
                    self.world.shimmer = Shimmer::default();
                }
            });
        egui::CollapsingHeader::new("Glass")
            .default_open(false)
            .show(ui, |ui| self.glass_settings(ui));
        ui.separator();

        let snap = self.world.snap.clone();
        match self.selection.as_slice() {
            [] => {
                ui.weak("Nothing selected");
            }
            &[id] => {
                let kind = self.world.kind(id);
                let rotation = self.world.rotations.get(id).map_or(0.0, |r| r.0);
                let mut new_rotation = rotation;
                let shift = ui.input(|i| i.modifiers.shift);
                let w = &mut self.world;
                let paints = &mut self.paints;
                // One row group per component the entity has.
                egui::Grid::new("props")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        if let Some(n) = w.names.get_mut(id) {
                            ui.label("Name");
                            ui.text_edit_singleline(&mut n.0);
                            ui.end_row();
                        }

                        let mut anchor = w.anchors.get(id).copied().unwrap_or_default();
                        if let Some(t) = w.transforms.get_mut(id) {
                            ui.label("Position");
                            ui.horizontal(|ui| {
                                ui.add(snapped(&mut t.x, snap.position_step()).prefix("X "));
                                ui.add(snapped(&mut t.y, snap.position_step()).prefix("Y "));
                            });
                            ui.end_row();

                            ui.label("Size");
                            ui.horizontal(|ui| {
                                let width = anchored_size(t, true, anchor, snap.width_step());
                                ui.add(width.prefix("W "));
                                let height = anchored_size(t, false, anchor, snap.height_step());
                                ui.add(height.prefix("H "));
                            });
                            ui.end_row();

                            ui.label("Rotation");
                            ui.add(rotation_input(&mut new_rotation, shift));
                            ui.end_row();

                            ui.label("Anchor");
                            if widgets::anchor_grid(ui, &mut anchor, 36.0).changed() {
                                w.anchors.insert(id, anchor);
                            }
                            ui.end_row();
                        }

                        if let Some(f) = w.fills.get_mut(id) {
                            ui.label(match kind {
                                ShapeKind::Text => "Color",
                                ShapeKind::Frame => "Background",
                                _ => "Fill",
                            });
                            let solid_only = kind == ShapeKind::Text;
                            paint_editor::swatch(
                                ui,
                                vec2(40.0, 20.0),
                                &mut f.0,
                                solid_only,
                                paints,
                            );
                            ui.end_row();
                        }

                        if let Some(t) = w.texts.get_mut(id) {
                            ui.label("Font size");
                            let step = snap.font_size_step();
                            ui.add(snapped(&mut t.font_size, step).range(step.max(1.0)..=512.0));
                            ui.end_row();

                            ui.label("Text");
                            ui.text_edit_multiline(&mut t.content);
                            ui.end_row();
                        }

                        if let Some(st) = w.strokes.get_mut(id) {
                            ui.label("Stroke");
                            ui.horizontal(|ui| {
                                ui.color_edit_button_srgba_unmultiplied(&mut st.color);
                                let step = snap.stroke_step();
                                ui.add(snapped(&mut st.width, step).range(0.0..=100.0).speed(1.0));
                            });
                            ui.end_row();
                        }

                        if let Some(f) = w.frames.get_mut(id) {
                            ui.label("Content");
                            ui.checkbox(&mut f.clip, "Clip content")
                                .on_hover_text("Hide children outside the frame");
                            ui.end_row();
                        }

                        if let Some(r) = w.radii.get_mut(id) {
                            ui.label("Radius");
                            ui.add(snapped(&mut r.0, snap.radius_step()).range(0.0..=1000.0));
                            ui.end_row();
                        }
                    });
                if new_rotation != rotation {
                    self.world.set_rotation(id, new_rotation);
                }
            }
            many => {
                ui.label(format!("{} layers selected", many.len()));
                self.arrange_controls(ui);
            }
        }

        if !self.selection.is_empty() {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Bring to front").on_hover_text("]").clicked() {
                    self.reorder(true);
                }
                if ui.button("Send to back").on_hover_text("[").clicked() {
                    self.reorder(false);
                }
            });
            if ui.button("Delete").clicked() {
                self.delete_selection();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

impl SkwiggleApp {
    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + self.pan + p.to_vec2() * self.zoom
    }

    fn to_world(&self, origin: Pos2, p: Pos2) -> Pos2 {
        ((p - origin - self.pan) / self.zoom).to_pos2()
    }

    fn rect_to_screen(&self, origin: Pos2, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(origin, r.min), self.to_screen(origin, r.max))
    }

    fn handles(r: Rect) -> impl Iterator<Item = (Handle, Pos2)> {
        [
            (-1, -1),
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
        ]
        .into_iter()
        .map(move |(hx, hy): Handle| {
            let x = egui::lerp(r.min.x..=r.max.x, (hx as f32 + 1.0) / 2.0);
            let y = egui::lerp(r.min.y..=r.max.y, (hy as f32 + 1.0) / 2.0);
            ((hx, hy), pos2(x, y))
        })
    }

    /// Screen-space handles of `id`, turned with its rotation.
    fn rotated_handles(&self, origin: Pos2, id: Entity) -> Vec<(Handle, Pos2)> {
        let r = self.rect_to_screen(origin, self.world.rect(id));
        let rot = Rot2::from_angle(self.world.angle(id));
        Self::handles(r)
            .map(|(h, p)| (h, r.center() + rot * (p - r.center())))
            .collect()
    }

    /// Screen-space outline of `id`, turned with its rotation.
    fn screen_corners(&self, origin: Pos2, id: Entity) -> Vec<Pos2> {
        self.world
            .corners(id)
            .iter()
            .map(|p| self.to_screen(origin, *p))
            .collect()
    }

    /// The resize handle of the (single) selected shape under the screen point.
    fn handle_at(&self, origin: Pos2, screen: Pos2) -> Option<(Entity, Handle)> {
        let [id] = self.selection.as_slice() else {
            return None;
        };
        self.rotated_handles(origin, *id)
            .into_iter()
            .find(|(_, p)| (*p - screen).length() <= HANDLE_SIZE)
            .map(|(h, _)| (*id, h))
    }

    /// The (single) selected shape, when the screen point is just outside one of its corners.
    fn rotate_zone_at(&self, origin: Pos2, screen: Pos2) -> Option<Entity> {
        let &[id] = self.selection.as_slice() else {
            return None;
        };
        if self.editing_text.is_some() {
            return None;
        }
        let r = self.rect_to_screen(origin, self.world.rect(id));
        let local = r.center() + Rot2::from_angle(-self.world.angle(id)) * (screen - r.center());
        let near_corner = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
            .iter()
            .any(|c| (*c - local).length() <= ROTATE_ZONE);
        (near_corner && !r.contains(local)).then_some(id)
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let origin = resp.rect.min;
        let bg = canvas_bg(ui.visuals().dark_mode);
        painter.rect_filled(resp.rect, 0.0, bg);

        // --- Zoom & pan with wheel / trackpad --------------------------------
        let (scroll, zoom_delta, hover, shift, space) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
                i.modifiers.shift,
                i.key_down(Key::Space),
            )
        });
        if resp.hovered() {
            if zoom_delta != 1.0 {
                if let Some(p) = hover {
                    let anchor = self.to_world(origin, p);
                    self.zoom = (self.zoom * zoom_delta).clamp(0.1, 10.0);
                    self.pan = p - origin - anchor.to_vec2() * self.zoom;
                }
            } else {
                self.pan += scroll;
            }
        }

        if self.world.background.is_dots()
            && self.world.shimmer.active()
            && grid_zoom_fade(self.zoom) > 0.0
        {
            // Advancing a clock (not scaling time) keeps speed changes from jumping the phase.
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            self.shimmer_clock += dt * self.world.shimmer.speed;
            ui.ctx().request_repaint();
        }
        self.draw_grid(&painter, resp.rect, bg);

        // --- Pointer interaction ---------------------------------------------
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let world = pointer.map(|p| self.to_world(origin, p));
        let space_pan = space && !ui.ctx().egui_wants_keyboard_input();

        if resp.drag_started() {
            let press = ui
                .input(|i| i.pointer.press_origin())
                .or(pointer)
                .unwrap_or(origin);
            let start = self.to_world(origin, press);
            let snapped_start = snap_pos(start, self.world.snap.position_step());
            let pan = resp.dragged_by(PointerButton::Middle) || space_pan;
            self.drag = if pan {
                Drag::Pan
            } else if !resp.dragged_by(PointerButton::Primary) {
                Drag::None
            } else {
                match self.tool {
                    Tool::Select => self.begin_select_drag(origin, press, start, shift),
                    Tool::Rect | Tool::Ellipse | Tool::Frame => {
                        let kind = self.tool.shape_kind().unwrap();
                        self.checkpoint();
                        let start = snapped_start;
                        let id = self.add_shape(kind, Rect::from_min_size(start, Vec2::ZERO));
                        self.selection = vec![id];
                        Drag::Create { id, start }
                    }
                    Tool::Text => Drag::None,
                }
            };
        }

        if resp.dragged()
            && let Some(wp) = world
        {
            let mods = ui.input(|i| i.modifiers);
            self.update_drag(wp, resp.drag_delta(), mods);
        }

        if resp.drag_stopped() {
            match self.drag {
                Drag::Create { id, start } => {
                    // A drag shorter than half a step leaves a degenerate shape.
                    let r = self.world.rect(id);
                    if r.width() < 1.0 || r.height() < 1.0 {
                        self.world.set_rect(id, self.default_rect(start));
                    } else {
                        self.adopt_enclosed(id);
                    }
                    self.tool = Tool::Select;
                }
                Drag::Move { .. } => {
                    if let Some(wp) = world {
                        self.reparent_selection(wp);
                    }
                }
                _ => {}
            }
            self.drag = Drag::None;
            self.alignment = Alignment::default();
        }

        if resp.clicked()
            && let Some(p) = pointer
        {
            self.click(origin, p, shift);
        }

        if resp.double_clicked()
            && self.tool == Tool::Select
            && let Some(id) = world.and_then(|wp| self.world.hit_test(wp))
            && self.world.texts.has(id)
        {
            self.start_text_edit(id);
        }

        // --- Cursor ----------------------------------------------------------
        if resp.hovered() {
            let icon = match (&self.drag, self.tool) {
                (Drag::Pan, _) => CursorIcon::Grabbing,
                _ if space_pan => CursorIcon::Grab,
                (_, Tool::Rect | Tool::Ellipse | Tool::Frame) => CursorIcon::Crosshair,
                (_, Tool::Text) => CursorIcon::Text,
                (Drag::Resize { id, handle, .. }, _) => {
                    resize_cursor(*handle, self.world.angle(*id))
                }
                (Drag::Rotate { .. }, _) => CursorIcon::Crosshair,
                _ => match pointer.and_then(|p| self.handle_at(origin, p)) {
                    Some((id, h)) => resize_cursor(h, self.world.angle(id)),
                    None if pointer.and_then(|p| self.rotate_zone_at(origin, p)).is_some() => {
                        CursorIcon::Crosshair
                    }
                    None => CursorIcon::Default,
                },
            };
            ui.ctx().set_cursor_icon(icon);
        }

        // --- Paint -----------------------------------------------------------
        render::layout_text(&mut self.world, &painter, self.zoom);
        let (pan, zoom) = (self.pan, self.zoom);
        let to_screen = |r: Rect| (r * zoom).translate(origin.to_vec2() + pan);
        render::render(
            &self.world,
            &painter,
            resp.rect,
            zoom,
            to_screen,
            self.editing_text,
            &mut self.paints,
        );
        if self.scrubbing && ui.input(|i| i.pointer.any_down()) {
            self.alignment = self.selection_alignment();
        }
        self.draw_overlays(&painter, origin, world.filter(|_| resp.hovered()));
        self.text_editor(ui, origin);
        self.coalesce_edits(|app| app.entity_toolbar(ui, origin, resp.rect));
    }

    /// Guides and gaps for the selection as it stands, without snapping it.
    fn selection_alignment(&self) -> Alignment {
        let roots = self.world.topmost(&self.selection);
        let bounds = roots
            .iter()
            .fold(Rect::NOTHING, |b, &e| b.union(self.world.bounds(e)));
        let exclude = self.world.with_descendants(&roots);
        align(&self.world, bounds, &exclude, ALL_EDGES, 0.0)
    }

    /// A floating WYSIWYG editor above the selected entity, with a control per visual component.
    /// Where the floating toolbar sits for the current selection, if it's on screen.
    fn toolbar_placement(
        &mut self,
        ui: &egui::Ui,
        origin: Pos2,
        canvas: Rect,
    ) -> Option<(Pos2, egui::Align2)> {
        if self.selection.is_empty() || !matches!(self.drag, Drag::None) {
            return None;
        }
        let bounds = self
            .selection
            .iter()
            .fold(Rect::NOTHING, |b, &e| b.union(self.world.bounds(e)));
        let r = self.rect_to_screen(origin, bounds);
        let held = ui.input(|i| i.pointer.any_down());
        Some(match &self.toolbar_pin {
            // Stay put mid-edit so scrubbing a value can't fling the toolbar away.
            Some((pinned, anchor, pivot)) if held && *pinned == self.selection => (*anchor, *pivot),
            _ => {
                if !r.intersects(canvas) {
                    self.toolbar_pin = None;
                    return None;
                }
                // Sit above the entity, or below it when there's no room at the top.
                let placement = if r.min.y - TOOLBAR_GAP - 40.0 > canvas.min.y {
                    (
                        pos2(r.center().x, r.min.y - TOOLBAR_GAP),
                        egui::Align2::CENTER_BOTTOM,
                    )
                } else {
                    (
                        pos2(r.center().x, r.max.y + TOOLBAR_GAP),
                        egui::Align2::CENTER_TOP,
                    )
                };
                self.toolbar_pin = Some((self.selection.clone(), placement.0, placement.1));
                placement
            }
        })
    }

    /// The floating toolbar: per-component controls for one entity, arrange controls for several.
    fn entity_toolbar(&mut self, ui: &egui::Ui, origin: Pos2, canvas: Rect) {
        let Some((anchor, pivot)) = self.toolbar_placement(ui, origin, canvas) else {
            return;
        };
        let &[id] = self.selection.as_slice() else {
            self.selection_toolbar(ui, anchor, pivot, canvas);
            return;
        };
        let kind = self.world.kind(id);
        let glass = self.glass_live.then_some(self.glass);
        let snap = self.world.snap.clone();
        let rotation = self.world.rotations.get(id).map_or(0.0, |r| r.0);
        let mut new_rotation = rotation;
        let shift = ui.input(|i| i.modifiers.shift);
        let w = &mut self.world;
        let paints = &mut self.paints;
        egui::Area::new(egui::Id::new("entity_toolbar"))
            .fixed_pos(anchor)
            .pivot(pivot)
            .constrain_to(canvas)
            .show(ui.ctx(), |ui| {
                widgets::glass_bar(ui, glass, widgets::ENTITY_SLOT, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let mut anchor = w.anchors.get(id).copied().unwrap_or_default();
                        if let Some(t) = w.transforms.get_mut(id) {
                            let ps = snap.position_step();
                            number_tile(ui, Glyph::X, snapped(&mut t.x, ps), "X");
                            number_tile(ui, Glyph::Y, snapped(&mut t.y, ps), "Y");
                            let width = anchored_size(t, true, anchor, snap.width_step());
                            number_tile(ui, Glyph::Width, width, "Width");
                            // Text height follows its content.
                            if kind != ShapeKind::Text {
                                let h = anchored_size(t, false, anchor, snap.height_step());
                                number_tile(ui, Glyph::Height, h, "Height");
                            }
                            let r = rotation_input(&mut new_rotation, shift);
                            number_tile(ui, Glyph::Rotation, r, "Rotation (⇧ snaps to 15°)");
                        }
                        if let Some(c) = w.radii.get_mut(id) {
                            let v = snapped(&mut c.0, snap.radius_step()).range(0.0..=1000.0);
                            number_tile(ui, Glyph::Radius, v, "Corner radius");
                        }
                        if let Some(st) = w.strokes.get_mut(id) {
                            let step = snap.stroke_step();
                            let glyph = Glyph::Border(st.width);
                            let v = snapped(&mut st.width, step).range(0.0..=100.0).speed(1.0);
                            number_tile(ui, glyph, v, "Border width");
                        }
                        if let Some(t) = w.texts.get_mut(id) {
                            let step = snap.font_size_step();
                            let v = snapped(&mut t.font_size, step).range(step.max(1.0)..=512.0);
                            number_tile(ui, Glyph::FontSize, v, "Font size");
                        }
                        if let Some(f) = w.fills.get_mut(id) {
                            let hint = match kind {
                                ShapeKind::Text => "Text color",
                                ShapeKind::Frame => "Background color",
                                _ => "Fill color",
                            };
                            let border = w.strokes.get_mut(id).map(|s| &mut s.color);
                            let solid_only = kind == ShapeKind::Text;
                            widgets::color_pair(ui, &mut f.0, border, hint, solid_only, paints);
                        }
                        if let Some(f) = w.frames.get_mut(id) {
                            widgets::clip_tile(ui, &mut f.clip);
                        }
                        if w.transforms.has(id)
                            && widgets::anchor_grid(ui, &mut anchor, widgets::TILE).changed()
                        {
                            w.anchors.insert(id, anchor);
                        }
                    });
                });
            });
        if new_rotation != rotation {
            self.world.set_rotation(id, new_rotation);
        }
    }

    /// Arrange controls in the properties panel for a multi-selection.
    fn arrange_controls(&mut self, ui: &mut egui::Ui) {
        let roots = self.world.topmost(&self.selection);
        ui.label("Align");
        let mut action = ui.horizontal(|ui| align_buttons(ui, roots.len())).inner;
        action = action.or(ui.horizontal(|ui| space_buttons(ui, roots.len())).inner);
        if let Some(icon) = action {
            self.arrange(icon);
        }
    }

    /// The floating toolbar for several selected entities.
    fn selection_toolbar(&mut self, ui: &egui::Ui, anchor: Pos2, pivot: egui::Align2, canvas: Rect) {
        let n = self.world.topmost(&self.selection).len();
        let glass = self.glass_live.then_some(self.glass);
        let action = egui::Area::new(egui::Id::new("selection_toolbar"))
            .fixed_pos(anchor)
            .pivot(pivot)
            .constrain_to(canvas)
            .show(ui.ctx(), |ui| {
                widgets::glass_bar(ui, glass, widgets::ENTITY_SLOT, |ui| {
                    ui.horizontal(|ui| {
                        let a = align_buttons(ui, n);
                        ui.separator();
                        a.or(space_buttons(ui, n))
                    })
                    .inner
                })
            })
            .inner;
        if let Some(icon) = action {
            self.arrange(icon);
        }
    }

    /// Aligns or spaces out the selection's top-level entities.
    fn arrange(&mut self, icon: widgets::ArrangeIcon) {
        let roots = self.world.topmost(&self.selection);
        self.checkpoint();
        match icon {
            widgets::ArrangeIcon::Align(horizontal, at) => self.world.align(&roots, horizontal, at),
            widgets::ArrangeIcon::Distribute(horizontal) => {
                self.world.distribute(&roots, horizontal)
            }
        }
    }

    fn begin_select_drag(&mut self, origin: Pos2, press: Pos2, start: Pos2, shift: bool) -> Drag {
        if self.handle_at(origin, press).is_none()
            && let Some(id) = self.rotate_zone_at(origin, press)
        {
            self.checkpoint();
            let center = self.world.rect(id).center();
            let d = start - center;
            return Drag::Rotate {
                id,
                center,
                start: d.y.atan2(d.x),
                original: self.world.rotations.get(id).map_or(0.0, |r| r.0),
            };
        }
        if let Some((id, handle)) = self.handle_at(origin, press) {
            self.checkpoint();
            let original = self.world.rect(id);
            return Drag::Resize {
                id,
                handle,
                start,
                original,
            };
        }
        match self.pick(origin, press) {
            Some(id) => {
                if !self.selection.contains(&id) {
                    if shift {
                        self.selection.push(id);
                    } else {
                        self.selection = vec![id];
                    }
                }
                self.checkpoint();
                let moved = self
                    .world
                    .with_descendants(&self.world.topmost(&self.selection));
                let originals = moved.iter().map(|&id| (id, self.world.rect(id))).collect();
                Drag::Move { start, originals }
            }
            None => Drag::Marquee {
                start,
                base: if shift {
                    self.selection.clone()
                } else {
                    Vec::new()
                },
            },
        }
    }

    /// A 100×100 shape centred on `center`, aligned to the grid.
    fn default_rect(&self, center: Pos2) -> Rect {
        let snap_cfg = &self.world.snap;
        let (ws, hs) = (snap_cfg.width_step(), snap_cfg.height_step());
        let size = vec2(snap(100.0, ws).max(ws), snap(100.0, hs).max(hs));
        Rect::from_min_size(
            snap_pos(center - size / 2.0, snap_cfg.position_step()),
            size,
        )
    }

    fn update_drag(&mut self, wp: Pos2, screen_delta: Vec2, mods: Modifiers) {
        let shift = mods.shift;
        let ps = self.world.snap.position_step();
        let (ws, hs) = (self.world.snap.width_step(), self.world.snap.height_step());
        match &self.drag {
            Drag::None => {}
            Drag::Pan => self.pan += screen_delta,
            Drag::Move { start, originals } => {
                // Snap the selection's bounding box, keeping relative offsets intact.
                let roots = self.world.topmost(&self.selection);
                let bounds = originals
                    .iter()
                    .filter(|(id, _)| roots.contains(id))
                    .fold(Rect::NOTHING, |b, (id, r)| {
                        b.union(self.world.bounds_of(*id, *r))
                    });
                let d = snap_pos(bounds.min + (wp - *start), ps) - bounds.min;
                // Alignment with other shapes wins over the grid when close enough.
                let moved: Vec<Entity> = originals.iter().map(|(id, _)| *id).collect();
                let a = align(
                    &self.world,
                    bounds.translate(d),
                    &moved,
                    ALL_EDGES,
                    GUIDE_SNAP / self.zoom,
                );
                let d = a.rect.min - bounds.min;
                self.alignment = a;
                for (id, r) in originals {
                    self.world.set_rect(*id, r.translate(d));
                }
            }
            Drag::Resize {
                id,
                handle,
                start,
                original,
            } => {
                // Like Figma: ⇧ keeps proportions, ⌥ resizes from the centre, ⌘ skips snapping.
                let (alt, free) = (mods.alt, mods.command);
                let angle = self.world.angle(*id);
                let rot = Rot2::from_angle(angle);
                let d = rot.inverse() * (wp - *start);
                let (hx, hy) = (handle.0 as f32, handle.1 as f32);
                let grow = if alt { 2.0 } else { 1.0 };
                let step = |s: f32| if free { 0.0 } else { s };
                // Signed sizes, negative once dragged past the fixed side.
                let mut size = vec2(
                    snap(original.width() + hx * d.x * grow, step(ws)),
                    snap(original.height() + hy * d.y * grow, step(hs)),
                );
                if shift {
                    let o = original.size().max(Vec2::splat(1e-3));
                    let (sx, sy) = (size.x / o.x, size.y / o.y);
                    let m = match handle {
                        (0, _) => sy.abs(),
                        (_, 0) => sx.abs(),
                        _ => sx.abs().max(sy.abs()),
                    };
                    let sign = |s: f32, h: i8| if h == 0 { 1.0 } else { s.signum() };
                    size = vec2(o.x * m * sign(sx, handle.0), o.y * m * sign(sy, handle.1));
                }
                // The point that stays put, as a fraction of the rect: opposite side, or centre with ⌥.
                let f = if alt {
                    Vec2::splat(0.5)
                } else {
                    vec2(1.0 - hx, 1.0 - hy) / 2.0
                };
                let fixed = original.min + original.size() * f;
                let min = fixed - size * f;
                let r = Rect::from_two_pos(min, min + size);
                // Turned shapes spin about their own centre, so shift back to keep the fixed point still.
                let c = original.center() - r.center();
                let r = r.translate(c - rot * c);
                if angle != 0.0 || free {
                    self.world.set_rect(*id, r);
                    self.alignment = Alignment::default();
                    return;
                }
                // Only the dragged edges snap to other shapes, and not when proportions are held.
                let edge = |h: i8, upright: bool| match (h, alt) {
                    (0, _) => [false; 3],
                    (_, true) => [true, false, true],
                    _ => dragged_edge(h, upright),
                };
                let mask = [edge(handle.0, size.x >= 0.0), edge(handle.1, size.y >= 0.0)];
                let reach = if shift || alt { 0.0 } else { GUIDE_SNAP / self.zoom };
                let exclude = self.world.with_descendants(&[*id]);
                let a = align(&self.world, r, &exclude, mask, reach);
                self.world.set_rect(*id, a.rect);
                self.alignment = a;
            }
            &Drag::Rotate {
                id,
                center,
                start,
                original,
            } => {
                let d = wp - center;
                let mut deg = original + (d.y.atan2(d.x) - start).to_degrees();
                if shift {
                    deg = (deg / ROTATE_SNAP).round() * ROTATE_SNAP;
                }
                self.world.set_rotation(id, deg);
            }
            Drag::Create { id, start } => {
                let size = wp - *start;
                let mut end = *start + vec2(snap(size.x, ws), snap(size.y, hs));
                if shift {
                    let d = end - *start;
                    let m = d.x.abs().max(d.y.abs());
                    end = *start + vec2(m * d.x.signum(), m * d.y.signum());
                }
                let mut r = Rect::from_two_pos(*start, end);
                // The corner under the pointer snaps; skipped for squares so they stay square.
                self.alignment = if shift {
                    Alignment {
                        rect: r,
                        ..Default::default()
                    }
                } else {
                    let mask = [
                        dragged_edge(1, end.x >= start.x),
                        dragged_edge(1, end.y >= start.y),
                    ];
                    align(&self.world, r, &[*id], mask, GUIDE_SNAP / self.zoom)
                };
                r = self.alignment.rect;
                self.world.set_rect(*id, r);
            }
            Drag::Marquee { start, base } => {
                let m = Rect::from_two_pos(*start, wp);
                let mut sel = base.clone();
                let w = &self.world;
                for &e in &w.entities {
                    let root = w.parent(e).is_none();
                    if root && w.visible(e) && m.intersects(w.bounds(e)) && !sel.contains(&e) {
                        sel.push(e);
                    }
                }
                self.selection = sel;
            }
        }
    }

    fn click(&mut self, origin: Pos2, screen: Pos2, shift: bool) {
        let wp = self.to_world(origin, screen);
        match self.tool {
            Tool::Select => match self.pick(origin, screen) {
                Some(id) if shift => toggle(&mut self.selection, id),
                Some(id) => self.selection = vec![id],
                None if shift => {}
                None => self.selection.clear(),
            },
            Tool::Rect | Tool::Ellipse | Tool::Frame => {
                let kind = self.tool.shape_kind().unwrap();
                self.checkpoint();
                let id = self.add_shape(kind, self.default_rect(wp));
                self.selection = vec![id];
                self.tool = Tool::Select;
            }
            Tool::Text => {
                self.checkpoint();
                let min = snap_pos(wp, self.world.snap.position_step());
                let id =
                    self.add_shape(ShapeKind::Text, Rect::from_min_size(min, vec2(200.0, 30.0)));
                if let Some(t) = self.world.texts.get_mut(id) {
                    t.content.clear();
                }
                self.selection = vec![id];
                self.tool = Tool::Select;
                self.editing_text = Some(id);
                self.focus_text = true;
            }
        }
    }

    /// Adds a shape inside whichever frame is under its top-left corner.
    fn add_shape(&mut self, kind: ShapeKind, rect: Rect) -> Entity {
        let parent = self.world.frame_at(rect.min, &[]);
        let id = self.world.spawn_shape(kind, rect);
        if let Some(p) = parent {
            self.world.parents.insert(id, Parent(p));
        }
        id
    }

    /// A newly drawn frame takes in the sibling shapes it fully covers.
    fn adopt_enclosed(&mut self, id: Entity) {
        let w = &self.world;
        if !w.frames.has(id) {
            return;
        }
        let (rect, parent) = (w.rect(id), w.parent(id));
        let enclosed: Vec<Entity> = w
            .entities
            .iter()
            .copied()
            .filter(|&e| e != id && w.parent(e) == parent && rect.contains_rect(w.rect(e)))
            .collect();
        for child in enclosed {
            self.world.set_parent(child, Some(id));
        }
    }

    /// After a move, drops the selection into the frame under the pointer (or onto the canvas).
    fn reparent_selection(&mut self, wp: Pos2) {
        let moved = self.world.topmost(&self.selection);
        let exclude = self.world.with_descendants(&moved);
        let target = self.world.frame_at(wp, &exclude);
        for id in moved {
            if self.world.parent(id) != target {
                self.world.set_parent(id, target);
            }
        }
    }

    /// Screen rect of a top-level frame's name label, which selects the frame when clicked.
    fn frame_label_rect(&self, origin: Pos2, e: Entity) -> Option<Rect> {
        let w = &self.world;
        if !w.frames.has(e) || !w.visible(e) || w.parent(e).is_some() {
            return None;
        }
        let r = self.rect_to_screen(origin, w.bounds(e));
        Some(Rect::from_min_max(
            pos2(r.min.x, r.min.y - 18.0),
            pos2(r.max.x, r.min.y - 2.0),
        ))
    }

    /// The shape under a screen point, counting frame labels.
    fn pick(&self, origin: Pos2, screen: Pos2) -> Option<Entity> {
        self.world
            .entities
            .iter()
            .rev()
            .copied()
            .find(|&e| {
                self.frame_label_rect(origin, e)
                    .is_some_and(|r| r.contains(screen))
            })
            .or_else(|| self.world.hit_test(self.to_world(origin, screen)))
    }

    fn start_text_edit(&mut self, id: Entity) {
        self.checkpoint();
        self.selection = vec![id];
        self.editing_text = Some(id);
        self.focus_text = true;
    }

    /// An in-place text editor drawn over the text shape being edited.
    fn text_editor(&mut self, ui: &mut egui::Ui, origin: Pos2) {
        let Some(id) = self.editing_text else { return };
        if !self.world.texts.has(id) {
            self.editing_text = None;
            return;
        }
        let zoom = self.zoom;
        let rect = self.rect_to_screen(origin, self.world.rect(id));
        let fill = self
            .world
            .fills
            .get(id)
            .map_or(Color32::BLACK, |f| f.0.solid_color());
        let Some(t) = self.world.texts.get_mut(id) else {
            return;
        };
        let edit = egui::TextEdit::multiline(&mut t.content)
            .font(FontId::proportional(t.font_size * zoom))
            .text_color(fill)
            .frame(egui::Frame::NONE)
            .margin(Vec2::ZERO)
            .desired_width(rect.width())
            .desired_rows(1);
        let resp = ui.put(rect.expand2(vec2(0.0, 2.0)), edit);
        let empty = t.content.trim().is_empty();
        if std::mem::take(&mut self.focus_text) {
            resp.request_focus();
        } else if resp.lost_focus() {
            self.editing_text = None;
            // Drop text layers that ended up empty.
            if empty {
                self.world.despawn(id);
                self.selection.retain(|s| *s != id);
            }
        }
    }

    fn draw_grid(&self, painter: &Painter, rect: Rect, bg: Color32) {
        let zoom_fade = grid_zoom_fade(self.zoom);
        if zoom_fade == 0.0 {
            return;
        }
        // Canvas-pinned pattern; when too dense, every 2nd line stays and the rest crossfade out.
        const MIN_PX: f32 = 8.0;
        let step = self.world.snap.step.max(1.0);
        // 2:1 isometric triangles have vertical sides of 2 steps, diagonals rising 1 per 2 across.
        let base = match self.world.background {
            Background::Grid | Background::Dots => step,
            Background::TriangleLines | Background::TriangleDots => 2.0 * step,
        };
        // Spacing of the dots fully shown at 100%; these stay highlighted as a zoom reference.
        let mut reference = base;
        while reference < 2.0 * MIN_PX {
            reference *= 2.0;
        }
        let mut s = base;
        // Never coarser than the reference, so the 100% dots and lines always show.
        while s * self.zoom < MIN_PX && s < reference {
            s *= 2.0;
        }
        let minor = ((s * self.zoom - MIN_PX) / MIN_PX).clamp(0.0, 1.0);
        let (line_ink, dot_ink) = (contrast::ink(bg, 1.3), contrast::ink(bg, 1.6));
        let reference_ink = contrast::ink(bg, self.world.shimmer.reference);
        let fade =
            |c: Color32, major: bool| c.gamma_multiply(zoom_fade * if major { 1.0 } else { minor });
        let multiple = |v: f32, d: f32| (v / d - (v / d).round()).abs() < 1e-3;
        let reference_line_ink = contrast::ink(bg, self.world.shimmer.reference_lines);
        // `at` is the line's offset, so lines on the 100% spacing get the reference ink.
        let line = |major: bool, at: f32| {
            let on_ref = multiple(at, reference);
            let ink = if on_ref { reference_line_ink } else { line_ink };
            Stroke::new(self.world.shimmer.line_width, fade(ink, major || on_ref))
        };
        let dot_r = self.world.shimmer.dot_size / 2.0;

        let origin = rect.min;
        let tl = self.to_world(origin, rect.min);
        let br = self.to_world(origin, rect.max);
        let screen = |p: Pos2| self.to_screen(origin, p);
        let is_dots = self.world.background.is_dots() && self.world.shimmer.active();
        let time = self.shimmer_clock;
        let (twinkle_depth, wave_depth) = (
            0.325 * self.world.shimmer.twinkle,
            0.325 * self.world.shimmer.wave,
        );
        // Shimmer calms near the cursor and the selection (screen-space rects and point).
        let hover = painter
            .ctx()
            .pointer_hover_pos()
            .filter(|p| rect.contains(*p));
        let calm_rects: Vec<Rect> = if is_dots {
            let sel = self
                .selection
                .iter()
                .map(|id| self.rect_to_screen(origin, self.world.bounds(*id)));
            sel.chain(hover.map(Rect::from_pos)).collect()
        } else {
            Vec::new()
        };
        // Dims by up to 65% by default: drifting waves plus a per-dot twinkle, each with its own strength.
        let shimmer = |p: Pos2| {
            let sp = screen(p);
            let d = calm_rects
                .iter()
                .map(|r| r.distance_to_pos(sp))
                .fold(f32::MAX, f32::min);
            let t = (d / 160.0).clamp(0.0, 1.0);
            let amp = 0.2 + 0.8 * t * t * (3.0 - 2.0 * t);
            let hash = (p.x.round() as i64).wrapping_mul(73_856_093)
                ^ (p.y.round() as i64).wrapping_mul(19_349_663);
            let phase = hash.rem_euclid(1000) as f32 / 1000.0 * std::f32::consts::TAU;
            // Three waves at odd angles and speeds, through a slowly wobbling warp: drifting blotches.
            let warp = vec2(
                (p.y * 0.006 + time * 0.31).sin(),
                (p.x * 0.005 - time * 0.27).sin(),
            ) * 40.0;
            let q = p + warp;
            let wave = ((q.x * 0.0184 + q.y * 0.0078) + time * 0.9).sin()
                + ((q.x * -0.0035 + q.y * 0.0283) - time * 0.7).sin()
                + ((q.x * -0.0145 + q.y * -0.0050) + time * 0.5).sin();
            let wave = (wave / 2.0).clamp(-1.0, 1.0);
            let twinkle = (time * 2.0 + phase).sin();
            let dim = (wave_depth * (1.0 - wave) + twinkle_depth * (1.0 - twinkle)) / 2.0;
            if !is_dots {
                return 1.0;
            }
            (1.0 - amp * dim).max(0.0)
        };
        // Triangle lattices shift odd columns down by half a spacing.
        let on_reference = |p: Pos2| {
            let col = (p.x / reference).round() as i64;
            let shift = match self.world.background {
                Background::TriangleDots if col.rem_euclid(2) == 1 => reference / 2.0,
                _ => 0.0,
            };
            multiple(p.x, reference) && multiple(p.y - shift, reference)
        };
        let dot = |p: Pos2, major: bool| {
            let on_ref = on_reference(p);
            let ink = if on_ref { reference_ink } else { dot_ink };
            let c = fade(ink, major || on_ref).gamma_multiply(shimmer(p));
            painter.circle_filled(screen(p), dot_r, c);
        };
        let even = |i: i64| i.rem_euclid(2) == 0;
        // Indices of multiples of `s` covering `lo..=hi`.
        let range = |lo: f32, hi: f32| (lo / s).floor() as i64..=(hi / s).ceil() as i64;
        let at = |i: i64| i as f32 * s;
        match self.world.background {
            Background::Grid => {
                for i in range(tl.x, br.x) {
                    painter.vline(
                        screen(pos2(at(i), 0.0)).x,
                        rect.y_range(),
                        line(even(i), at(i)),
                    );
                }
                for j in range(tl.y, br.y) {
                    painter.hline(
                        rect.x_range(),
                        screen(pos2(0.0, at(j))).y,
                        line(even(j), at(j)),
                    );
                }
            }
            Background::Dots => {
                for i in range(tl.x, br.x) {
                    for j in range(tl.y, br.y) {
                        dot(pos2(at(i), at(j)), even(i) && even(j));
                    }
                }
            }
            Background::TriangleLines => {
                for i in range(tl.x, br.x) {
                    painter.vline(
                        screen(pos2(at(i), 0.0)).x,
                        rect.y_range(),
                        line(even(i), at(i)),
                    );
                }
                // Lines y = ±x/2 + c, clipped to the visible x span.
                for (k, lo, hi) in [
                    (0.5, tl.y - br.x / 2.0, br.y - tl.x / 2.0),
                    (-0.5, tl.y + tl.x / 2.0, br.y + br.x / 2.0),
                ] {
                    for j in range(lo, hi) {
                        let a = screen(pos2(tl.x, k * tl.x + at(j)));
                        let b = screen(pos2(br.x, k * br.x + at(j)));
                        painter.line_segment([a, b], line(even(j), at(j)));
                    }
                }
            }
            Background::TriangleDots => {
                // Odd columns sit half a triangle lower; the coarser lattice shifts every other even column.
                for i in range(tl.x, br.x) {
                    let shift = if even(i) { 0.0 } else { s / 2.0 };
                    let coarse_shift = (!even(i.div_euclid(2))) as i64;
                    for j in range(tl.y - s, br.y) {
                        let major = even(i) && even(j - coarse_shift);
                        dot(pos2(at(i), at(j) + shift), major);
                    }
                }
            }
        }
    }

    fn draw_overlays(&self, painter: &Painter, origin: Pos2, hover_world: Option<Pos2>) {
        let outline = Stroke::new(1.5, ACCENT);

        // Pulse the shapes we're aligned with, and draw the guide lines.
        let al = &self.alignment;
        if !al.aligned.is_empty() {
            let ctx = painter.ctx();
            let t = ctx.input(|i| i.time) as f32;
            let pulse = 0.5 + 0.5 * (t * std::f32::consts::TAU * 1.5).sin();
            for id in &al.aligned {
                let r = self.rect_to_screen(origin, self.world.bounds(*id));
                painter.rect_filled(r, 0.0, GUIDE.gamma_multiply(0.08 + 0.12 * pulse));
                let stroke =
                    Stroke::new(1.0 + 2.0 * pulse, GUIDE.gamma_multiply(0.4 + 0.6 * pulse));
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Outside);
            }
            ctx.request_repaint();
        }
        let faint = Stroke::new(1.0, GUIDE.gamma_multiply(0.4));
        let solid = Stroke::new(1.0, GUIDE);
        for g in &al.guides {
            let (a, b) = if g.vertical {
                (pos2(g.at, g.from), pos2(g.at, g.to))
            } else {
                (pos2(g.from, g.at), pos2(g.to, g.at))
            };
            let (a, b) = (self.to_screen(origin, a), self.to_screen(origin, b));
            if g.faint {
                painter.extend(egui::Shape::dashed_line(&[a, b], faint, 4.0, 3.0));
                continue;
            }
            painter.line_segment([a, b], solid);
            for p in [a, b] {
                let x = 3.0;
                painter.line_segment([p + vec2(-x, -x), p + vec2(x, x)], solid);
                painter.line_segment([p + vec2(-x, x), p + vec2(x, -x)], solid);
            }
        }
        for g in &al.gaps {
            let pt = |along: f32| {
                let p = if g.horizontal {
                    pos2(along, g.cross)
                } else {
                    pos2(g.cross, along)
                };
                self.to_screen(origin, p)
            };
            let (p, q) = (pt(g.a), pt(g.b));
            let stroke = solid;
            painter.line_segment([p, q], stroke);
            let tick = if g.horizontal {
                vec2(0.0, 3.0)
            } else {
                vec2(3.0, 0.0)
            };
            for e in [p, q] {
                painter.line_segment([e - tick, e + tick], stroke);
            }
            let gap = (g.b - g.a).abs();
            let text = if gap.fract().abs() < 0.05 {
                format!("{gap:.0}px")
            } else {
                format!("{gap:.1}px")
            };
            let galley = painter.layout_no_wrap(text, FontId::proportional(10.0), Color32::WHITE);
            let pad = vec2(4.0, 1.0);
            // Sit the label beside the marker so it doesn't cover it.
            let offset = if g.horizontal {
                vec2(0.0, -(galley.size().y / 2.0 + pad.y + 4.0))
            } else {
                vec2(galley.size().x / 2.0 + pad.x + 4.0, 0.0)
            };
            let pill = Rect::from_center_size(p.lerp(q, 0.5) + offset, galley.size() + pad * 2.0);
            painter.rect_filled(pill, 3.0, GUIDE);
            painter.galley(pill.min + pad, galley, Color32::WHITE);
        }

        // Names above top-level frames.
        for &e in &self.world.entities {
            if let Some(r) = self.frame_label_rect(origin, e) {
                let selected = self.selection.contains(&e);
                let name = self.world.names.get(e).map_or("", |n| n.0.as_str());
                let c = if selected { ACCENT } else { Color32::GRAY };
                let label = painter.with_clip_rect(r.intersect(painter.clip_rect()));
                label.text(
                    r.left_bottom(),
                    egui::Align2::LEFT_BOTTOM,
                    name,
                    FontId::proportional(11.0),
                    c,
                );
            }
        }

        // Hover highlight.
        if matches!(self.drag, Drag::None)
            && self.tool == Tool::Select
            && let Some(id) = hover_world.and_then(|p| self.pick(origin, self.to_screen(origin, p)))
            && !self.selection.contains(&id)
        {
            painter.add(egui::Shape::closed_line(self.screen_corners(origin, id), outline));
        }

        // Selection outlines.
        let mut bounds = Rect::NOTHING;
        for id in &self.selection {
            let corners = self.screen_corners(origin, *id);
            bounds = bounds.union(Rect::from_points(&corners));
            painter.add(egui::Shape::closed_line(corners, outline));
        }

        // Resize handles for a single selection.
        if let [id] = self.selection.as_slice()
            && self.editing_text.is_none()
        {
            let rot = Rot2::from_angle(self.world.angle(*id));
            for (_, p) in self.rotated_handles(origin, *id) {
                let h = Rect::from_center_size(p, Vec2::splat(HANDLE_SIZE));
                let quad = [h.left_top(), h.right_top(), h.right_bottom(), h.left_bottom()]
                    .map(|c| p + rot * (c - p));
                painter.add(egui::Shape::convex_polygon(quad.to_vec(), Color32::WHITE, outline));
            }
        }

        // Size badge under the selection, or the angle while rotating.
        if bounds != Rect::NOTHING {
            {
                let size = match self.selection.as_slice() {
                    [id] => self.world.rect(*id).size(),
                    _ => bounds.size() / self.zoom,
                };
                let text = match self.drag {
                    Drag::Rotate { id, .. } => {
                        format!("{:.0}°", self.world.rotations.get(id).map_or(0.0, |r| r.0))
                    }
                    _ => format!("{:.0} × {:.0}", size.x, size.y),
                };
                let galley =
                    painter.layout_no_wrap(text, FontId::proportional(11.0), Color32::WHITE);
                let pad = vec2(5.0, 2.0);
                let badge = Rect::from_center_size(
                    pos2(
                        bounds.center().x,
                        bounds.max.y + 8.0 + galley.size().y / 2.0 + pad.y,
                    ),
                    galley.size() + pad * 2.0,
                );
                painter.rect_filled(badge, 3.0, ACCENT);
                painter.galley(badge.min + pad, galley, Color32::WHITE);
            }
        }

        // Marquee.
        if let (Drag::Marquee { start, .. }, Some(end)) = (&self.drag, hover_world) {
            let r = self.rect_to_screen(origin, Rect::from_two_pos(*start, end));
            painter.rect_filled(r, 0.0, ACCENT.gamma_multiply(0.1));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
        }
    }
}

/// Left/centre/right (or top/middle/bottom) of a rect along one axis.
fn edges(r: Rect, vertical: bool) -> [f32; 3] {
    if vertical {
        [r.left(), r.center().x, r.right()]
    } else {
        [r.top(), r.center().y, r.bottom()]
    }
}

/// The mask row for a resize handle direction: -1 drags min, 1 drags max (swapped when flipped).
fn dragged_edge(dir: i8, upright: bool) -> [bool; 3] {
    match (dir, upright) {
        (0, _) => [false; 3],
        (1, true) | (-1, false) => [false, false, true],
        _ => [true, false, false],
    }
}

/// Shifts the masked edges of `r` by `d` on one axis; a moving centre means the whole rect moves.
fn shift_edges(r: Rect, mask: [bool; 3], d: f32, x_axis: bool) -> Rect {
    let (mut min, mut max) = if x_axis {
        (r.min.x, r.max.x)
    } else {
        (r.min.y, r.max.y)
    };
    if mask[1] || (mask[0] && mask[2]) {
        min += d;
        max += d;
    } else if mask[0] {
        min += d;
    } else if mask[2] {
        max += d;
    }
    if x_axis {
        Rect::from_x_y_ranges(min..=max, r.y_range())
    } else {
        Rect::from_x_y_ranges(r.x_range(), min..=max)
    }
}

/// Snaps the masked edges of `moving` to other visible shapes' edges within `snap` world units, and gathers the
/// guides plus the distance to the nearest shape on each side.
fn align(world: &World, moving: Rect, exclude: &[Entity], mask: EdgeMask, snap: f32) -> Alignment {
    let others: Vec<(Entity, Rect)> = world
        .paint_order()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| !exclude.contains(e))
        .map(|e| (e, world.bounds(e)))
        .collect();
    let moving_edges = |r: Rect, axis: usize| {
        let e = edges(r, axis == 0);
        (0..3).filter(move |i| mask[axis][*i]).map(move |i| e[i])
    };

    // Closest edge pair on each axis snaps when very close.
    let mut rect = moving;
    for axis in 0..2 {
        let vertical = axis == 0;
        let best = others
            .iter()
            .flat_map(|(_, r)| edges(*r, vertical))
            .flat_map(|b| moving_edges(moving, axis).map(move |a| b - a))
            .filter(|d| d.abs() <= snap)
            .min_by(|x, y| x.abs().total_cmp(&y.abs()));
        if let Some(d) = best {
            rect = shift_edges(rect, mask[axis], d, vertical);
        }
    }

    let mut out = Alignment {
        rect,
        ..Default::default()
    };
    for (id, r) in &others {
        for axis in 0..2 {
            let vertical = axis == 0;
            for a in moving_edges(rect, axis) {
                if !edges(*r, vertical).iter().any(|b| (a - b).abs() < 0.01) {
                    continue;
                }
                let (from, to) = if vertical {
                    (rect.top().min(r.top()), rect.bottom().max(r.bottom()))
                } else {
                    (rect.left().min(r.left()), rect.right().max(r.right()))
                };
                out.guides.push(Guide {
                    vertical,
                    at: a,
                    from,
                    to,
                    faint: false,
                });
                if !out.aligned.contains(id) {
                    out.aligned.push(*id);
                }
            }
        }
    }

    // Distance from each side to the nearest edge beyond it, preferring shapes level with us.
    for (horizontal, dir) in [(true, -1.0), (true, 1.0), (false, -1.0), (false, 1.0)] {
        let side = match (horizontal, dir < 0.0) {
            (true, true) => rect.left(),
            (true, false) => rect.right(),
            (false, true) => rect.top(),
            (false, false) => rect.bottom(),
        };
        let level = |r: &Rect| {
            if horizontal {
                r.top() < rect.bottom() && r.bottom() > rect.top()
            } else {
                r.left() < rect.right() && r.right() > rect.left()
            }
        };
        let nearest = |only_level: bool| {
            others
                .iter()
                .filter(|(_, r)| !only_level || level(r))
                .flat_map(|(_, r)| {
                    let (lo, hi) = if horizontal {
                        (r.left(), r.right())
                    } else {
                        (r.top(), r.bottom())
                    };
                    [(*r, lo), (*r, hi)]
                })
                .map(|(r, e)| (r, e, (e - side) * dir))
                .filter(|(_, _, d)| *d >= 0.5)
                .min_by(|x, y| x.2.total_cmp(&y.2))
        };
        let Some((r, edge, _)) = nearest(true).or_else(|| nearest(false)) else {
            continue;
        };
        let cross = if horizontal {
            rect.center().y
        } else {
            rect.center().x
        };
        out.gaps.push(Gap {
            horizontal,
            a: side,
            b: edge,
            cross,
        });
        // An off-line neighbour gets a dashed extension of its edge out to the marker.
        if !level(&r) {
            let (lo, hi) = if horizontal {
                (r.top(), r.bottom())
            } else {
                (r.left(), r.right())
            };
            out.guides.push(Guide {
                vertical: horizontal,
                at: edge,
                from: lo.min(cross),
                to: hi.max(cross),
                faint: true,
            });
        }
    }
    out
}

/// The six align buttons, enabled for 2+ layers; returns the one clicked.
fn align_buttons(ui: &mut egui::Ui, layers: usize) -> Option<widgets::ArrangeIcon> {
    use widgets::ArrangeIcon::Align;
    ui.spacing_mut().item_spacing.x = 2.0;
    let enabled = layers >= 2;
    let mut clicked = None;
    for (icon, hint) in [
        (Align(true, 0.0), "Align left"),
        (Align(true, 0.5), "Align horizontal centres"),
        (Align(true, 1.0), "Align right"),
        (Align(false, 0.0), "Align top"),
        (Align(false, 0.5), "Align vertical centres"),
        (Align(false, 1.0), "Align bottom"),
    ] {
        if widgets::arrange_button(ui, icon, enabled, hint).clicked() && enabled {
            clicked = Some(icon);
        }
    }
    clicked
}

/// The two even-spacing buttons, enabled for 3+ layers; returns the one clicked.
fn space_buttons(ui: &mut egui::Ui, layers: usize) -> Option<widgets::ArrangeIcon> {
    use widgets::ArrangeIcon::Distribute;
    ui.spacing_mut().item_spacing.x = 2.0;
    let enabled = layers >= 3;
    let mut clicked = None;
    for (icon, hint) in [
        (Distribute(true), "Space evenly horizontally"),
        (Distribute(false), "Space evenly vertically"),
    ] {
        let hint = if enabled { hint } else { "Select 3 or more layers to space evenly" };
        if widgets::arrange_button(ui, icon, enabled, hint).clicked() && enabled {
            clicked = Some(icon);
        }
    }
    clicked
}

/// A number input whose value always lands on a multiple of `step` (0 = no snapping).
fn snapped(value: &mut f32, step: f32) -> Scrub<'_> {
    Scrub::new(value).snap(step)
}

/// A degrees input that snaps to 15° steps while shift is held.
fn rotation_input(degrees: &mut f32, shift: bool) -> Scrub<'_> {
    let step = if shift { ROTATE_SNAP } else { 0.0 };
    Scrub::new(degrees).snap(step).suffix("°").speed(0.5)
}

/// A width (or height) input that grows and shrinks around `anchor`.
fn anchored_size(t: &mut Transform, horizontal: bool, anchor: Anchor, step: f32) -> Scrub<'_> {
    let (fx, fy) = anchor.factor();
    let f = if horizontal { fx } else { fy };
    Scrub::from_get_set(move |v| {
        let (pos, len) = if horizontal {
            (&mut t.x, &mut t.w)
        } else {
            (&mut t.y, &mut t.h)
        };
        if let Some(v) = v {
            *pos += (*len - v as f32) * f;
            *len = v as f32;
        }
        *len as f64
    })
    .snap(step)
    .range(0.0..=f32::MAX)
}

/// The whole background fades out from 70% zoom, gone by 30%, as it gets too crowded.
fn grid_zoom_fade(zoom: f32) -> f32 {
    ((zoom - 0.3) / 0.4).clamp(0.0, 1.0)
}

/// A "Reset" button, greyed out when there's nothing to reset.
fn reset_button(ui: &mut egui::Ui, enabled: bool, hint: &str) -> bool {
    ui.add_enabled(enabled, egui::Button::new("Reset"))
        .on_hover_text(hint)
        .clicked()
}

fn toggle(selection: &mut Vec<Entity>, id: Entity) {
    if let Some(i) = selection.iter().position(|s| *s == id) {
        selection.remove(i);
    } else {
        selection.push(id);
    }
}

/// The resize cursor closest to the handle's direction once turned by `angle`.
fn resize_cursor(handle: Handle, angle: f32) -> CursorIcon {
    let dir = Rot2::from_angle(angle) * vec2(handle.0 as f32, handle.1 as f32);
    // Eighth-turns from pointing right, folded to a half-turn since cursors are two-way.
    let octant = ((dir.y.atan2(dir.x) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(4);
    match octant {
        0 => CursorIcon::ResizeHorizontal,
        1 => CursorIcon::ResizeNwSe,
        2 => CursorIcon::ResizeVertical,
        _ => CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_rect(min: Pos2, size: Vec2) -> (World, Entity) {
        let mut world = World::default();
        let id = world.spawn_shape(ShapeKind::Rect, Rect::from_min_size(min, size));
        (world, id)
    }

    #[test]
    fn snaps_to_nearby_edges_and_reports_aligned_shape() {
        let (doc, other) = doc_with_rect(pos2(0.0, 0.0), vec2(100.0, 50.0));
        // Left edge 3px off, tops 200px apart: snaps x only.
        let moving = Rect::from_min_size(pos2(3.0, 200.0), vec2(40.0, 40.0));
        let a = align(&doc, moving, &[], ALL_EDGES, GUIDE_SNAP);
        assert_eq!(a.rect.min, pos2(0.0, 200.0));
        assert_eq!(a.aligned, vec![other]);
        let mut solid = a.guides.iter().filter(|g| !g.faint);
        assert!(solid.clone().count() > 0 && solid.all(|g| g.vertical && g.at == 0.0));
        // Too far to snap: no alignment, but still a distance to the shape above.
        let far = moving.translate(vec2(200.0, 0.0));
        let a = align(&doc, far, &[], ALL_EDGES, GUIDE_SNAP);
        assert!(a.aligned.is_empty() && a.guides.iter().all(|g| g.faint));
        assert_eq!(a.gaps.len(), 2);
    }

    #[test]
    fn measures_to_nearest_shape_on_each_side_however_far() {
        let (doc, _) = doc_with_rect(pos2(0.0, 0.0), vec2(100.0, 50.0));
        // Off to the lower right: nearest on the left is 200px away, above is 10px.
        let moving = Rect::from_min_size(pos2(300.0, 60.0), vec2(40.0, 40.0));
        let a = align(&doc, moving, &[], ALL_EDGES, GUIDE_SNAP);
        assert_eq!(a.rect, moving);
        assert!(a.aligned.is_empty());
        let gaps: Vec<_> = a.gaps.iter().map(|g| (g.horizontal, g.b - g.a)).collect();
        assert_eq!(gaps, vec![(true, -200.0), (false, -10.0)]);
        // Neither neighbour is level with it, so both get dashed extensions.
        assert_eq!(a.guides.iter().filter(|g| g.faint).count(), 2);
    }

    #[test]
    fn resize_snaps_only_the_dragged_edge_and_measures_padding() {
        let (doc, _) = doc_with_rect(pos2(0.0, 0.0), vec2(200.0, 100.0));
        // Text inside the rect, its right edge being dragged to 3px short of 184.
        let (min, max) = (pos2(16.0, 20.0), pos2(181.0, 60.0));
        let target = Rect::from_min_max(min, max);
        let mask = [dragged_edge(1, true), dragged_edge(0, true)];
        let a = align(&doc, target, &[], mask, GUIDE_SNAP);
        // 19px from the rect's right edge: too far to snap.
        assert_eq!(a.rect, target);
        let gaps: Vec<_> = a.gaps.iter().map(|g| (g.b - g.a).abs()).collect();
        assert_eq!(gaps, vec![16.0, 19.0, 20.0, 40.0]);
        // Within snap distance of the right edge: only the right edge moves.
        let near = Rect::from_min_max(min, pos2(197.0, 60.0));
        let a = align(&doc, near, &[], mask, GUIDE_SNAP);
        assert_eq!(a.rect, Rect::from_min_max(min, pos2(200.0, 60.0)));
    }
}

fn canvas_bg(dark: bool) -> Color32 {
    if dark {
        Color32::from_gray(30)
    } else {
        Color32::from_gray(245)
    }
}

fn error_dialog(title: &str, detail: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(detail)
        .show();
}
