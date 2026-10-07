//! Number input scrubbed by dragging sideways; dragging further up or down changes the step.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align2, Color32, DragValue, FontId, Id, LayerId, Order, Pos2, Response, Ui, Widget,
    emath::Numeric, vec2,
};

/// Vertical distance, in points, between step tiers.
const BAND: f32 = 40.0;
/// Highest tier: 4× the base step.
const MAX_TIER: i32 = 2;
/// Lowest tier: 1/4 of the base step.
const MIN_TIER: i32 = -2;
/// Horizontal points dragged per step.
const PX_PER_STEP: f64 = 2.0;

type GetSet<'a> = Box<dyn FnMut(Option<f64>) -> f64 + 'a>;

pub struct Scrub<'a> {
    get_set: GetSet<'a>,
    speed: f64,
    snap: f64,
    range: RangeInclusive<f64>,
    prefix: String,
    suffix: String,
    max_decimals: Option<usize>,
}

impl<'a> Scrub<'a> {
    pub fn new(value: &'a mut f32) -> Self {
        Self::from_get_set(move |v| {
            if let Some(v) = v {
                *value = v as f32;
            }
            *value as f64
        })
    }

    pub fn from_get_set(get_set: impl FnMut(Option<f64>) -> f64 + 'a) -> Self {
        Self {
            get_set: Box::new(get_set),
            speed: 1.0,
            snap: 0.0,
            range: f64::MIN..=f64::MAX,
            prefix: String::new(),
            suffix: String::new(),
            max_decimals: None,
        }
    }

    /// Value change per step at the base tier.
    pub fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Values always land on a multiple of `step` (0 = no snapping).
    pub fn snap(mut self, step: f32) -> Self {
        self.snap = step as f64;
        self
    }

    pub fn range<N: Numeric>(mut self, range: RangeInclusive<N>) -> Self {
        self.range = range.start().to_f64()..=range.end().to_f64();
        self
    }

    pub fn prefix(mut self, prefix: impl ToString) -> Self {
        self.prefix = prefix.to_string();
        self
    }

    pub fn suffix(mut self, suffix: impl ToString) -> Self {
        self.suffix = suffix.to_string();
        self
    }

    pub fn max_decimals(mut self, n: usize) -> Self {
        self.max_decimals = Some(n);
        self
    }

    /// Lowest usable tier; snapped values only step down while the step stays a whole number.
    fn min_tier(&self) -> i32 {
        if self.max_decimals == Some(0) {
            return 0;
        }
        if self.snap <= 0.0 {
            return MIN_TIER;
        }
        let mut tier = 0;
        while tier > MIN_TIER && self.step(tier - 1).fract() == 0.0 {
            tier -= 1;
        }
        tier
    }

    /// Value change per step at `tier`: the base step doubled per tier up, halved per tier down.
    fn step(&self, tier: i32) -> f64 {
        let base = if self.snap > 0.0 {
            (self.speed / self.snap).ceil().max(1.0) * self.snap
        } else {
            self.speed
        };
        base * 2f64.powi(tier)
    }
}

fn snap(v: f64, step: f64) -> f64 {
    if step > 0.0 {
        (v / step).round() * step
    } else {
        v
    }
}

/// Tier for a pointer `dy` points above the press origin, with a dead zone around 0.
fn tier_at(dy: f32, min: i32) -> i32 {
    ((dy / BAND).trunc() as i32).clamp(min, MAX_TIER)
}

impl Widget for Scrub<'_> {
    fn ui(mut self, ui: &mut Ui) -> Response {
        // DragValue's own drag also reacts to vertical motion, so ignore its sets while dragging.
        let id = ui.next_auto_id();
        let dragging = ui.ctx().is_being_dragged(id);
        let (snap_step, range) = (self.snap, self.range.clone());
        let get_set = &mut self.get_set;
        let mut drag = DragValue::from_get_set(|v| match v {
            Some(v) if !dragging => get_set(Some(snap(v, snap_step))),
            _ => get_set(None),
        })
        .speed(self.speed)
        .range(range.clone())
        .prefix(&self.prefix)
        .suffix(&self.suffix)
        .custom_parser(crate::calc::parse);
        if let Some(n) = self.max_decimals {
            drag = drag.max_decimals(n);
        }
        let mut resp = ui.add(drag);

        if !resp.dragged() {
            return resp;
        }
        let (origin, pos) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()));
        let (Some(origin), Some(pos)) = (origin, pos) else {
            return resp;
        };
        let min = self.min_tier();
        let tier = tier_at(origin.y - pos.y, min);
        let step = self.step(tier);

        // Precise value and leftover sub-step drag, kept across frames.
        let key = id.with("scrub");
        let value = (self.get_set)(None);
        let (mut precise, mut frac) = if resp.drag_started() {
            (value, 0.0)
        } else {
            ui.data(|d| d.get_temp(key)).unwrap_or((value, 0.0))
        };
        frac += resp.drag_delta().x as f64;
        let n = (frac / PX_PER_STEP).trunc();
        frac -= n * PX_PER_STEP;
        if n != 0.0 {
            precise = (precise + n * step).clamp(*range.start(), *range.end());
            precise = (precise * 1e6).round() / 1e6;
            // Finer tiers snap to their own step, coarser ones to the global step.
            let grid = if snap_step > 0.0 { step.min(snap_step) } else { 0.0 };
            (self.get_set)(Some(snap(precise, grid)));
            resp.mark_changed();
        }
        ui.data_mut(|d| d.insert_temp(key, (precise, frac)));

        let steps: Vec<_> = (min..=MAX_TIER).map(|t| (t, self.step(t))).collect();
        paint_ladder(ui.ctx(), key, origin, &steps, tier);
        resp
    }
}

/// Step labels stacked beside the press origin at each tier's height; the active one lit.
fn paint_ladder(ctx: &egui::Context, id: Id, origin: Pos2, steps: &[(i32, f64)], active: i32) {
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, id));
    let font = FontId::proportional(11.0);
    for &(tier, step) in steps {
        // Centre of the tier's band; tier 0 sits on the origin.
        let dy = if tier == 0 {
            0.0
        } else {
            (tier.abs() as f32 + 0.5) * BAND * tier.signum() as f32
        };
        let on = tier == active;
        let text = format!(
            "±{}",
            egui::emath::format_with_decimals_in_range(step, 0..=4)
        );
        let fg = if on {
            Color32::from_gray(20)
        } else {
            Color32::from_gray(170)
        };
        let galley = painter.layout_no_wrap(text, font.clone(), fg);
        let size = galley.size() + vec2(10.0, 4.0);
        let rect =
            Align2::RIGHT_CENTER.anchor_size(Pos2::new(origin.x - 14.0, origin.y - dy), size);
        let bg = if on {
            Color32::from_gray(235)
        } else {
            Color32::from_black_alpha(200)
        };
        painter.rect_filled(rect, size.y / 2.0, bg);
        painter.galley(rect.center() - galley.size() / 2.0, galley, fg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiers(snap_step: f32) -> Vec<f64> {
        let mut v = 0.0;
        let s = Scrub::new(&mut v).snap(snap_step);
        (s.min_tier()..=MAX_TIER).map(|t| s.step(t)).collect()
    }

    #[test]
    fn snapped_steps_down_only_to_whole_numbers() {
        assert_eq!(tiers(4.0), [1.0, 2.0, 4.0, 8.0, 16.0]);
        assert_eq!(tiers(2.0), [1.0, 2.0, 4.0, 8.0]);
        assert_eq!(tiers(6.0), [3.0, 6.0, 12.0, 24.0]);
        assert_eq!(tiers(3.0), [3.0, 6.0, 12.0]);
    }
}
