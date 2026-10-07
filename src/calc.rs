//! Arithmetic in number fields: `2 + 3`, `100 / 3`, `(4 + 4) * 2`, `-5 + 2^3`.

use std::cell::{Cell, RefCell};

use eframe::egui::{self, Align2, Context, Id, Order, vec2};

thread_local! {
    static CTX: RefCell<Option<Context>> = const { RefCell::new(None) };
    /// The field being typed into and its latest text.
    static EDIT: RefCell<Option<(Id, String)>> = const { RefCell::new(None) };
    /// True once the edited field has been unfocused for a frame without committing.
    static LOST: Cell<bool> = const { Cell::new(false) };
}

/// Call once per frame so `parse` can see which field has focus.
pub fn begin_frame(ctx: &Context) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx.clone()));
    // The field gets one frame to commit after losing focus; drop it after that (e.g. Escape).
    let Some(id) = EDIT.with(|e| e.borrow().as_ref().map(|(id, _)| *id)) else {
        return;
    };
    if ctx.memory(|m| m.has_focus(id)) {
        LOST.set(false);
    } else if LOST.replace(true) {
        EDIT.with(|e| *e.borrow_mut() = None);
        LOST.set(false);
    }
}

/// Number-field parser: holds off while typing, then evaluates on Enter or blur.
pub fn parse(text: &str) -> Option<f64> {
    let focused = CTX.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|ctx| ctx.memory(|m| m.focused()))
    });
    let pending = EDIT.with(|e| e.borrow().clone());
    match (focused, pending) {
        // The field lost focus with this text: commit it.
        (f, Some((id, last))) if f != Some(id) && last == text => {
            EDIT.with(|e| *e.borrow_mut() = None);
            eval(text)
        }
        // Still typing: remember the text for the preview, don't change the value yet.
        (Some(id), _) => {
            EDIT.with(|e| *e.borrow_mut() = Some((id, text.to_owned())));
            None
        }
        _ => eval(text),
    }
}

/// Shows `2 + 3 = 5` above the focused field once its text has an operator.
pub fn show_preview(ctx: &Context) {
    let Some((id, text)) = EDIT.with(|e| e.borrow().clone()) else {
        return;
    };
    if !ctx.memory(|m| m.has_focus(id)) {
        return;
    }
    // A leading minus is just a negative number, not a calculation.
    let has_op = text
        .trim_start()
        .chars()
        .skip(1)
        .any(|c| "+-*/^×÷−".contains(c));
    let Some(rect) = ctx.read_response(id).map(|r| r.rect) else {
        return;
    };
    if !has_op {
        return;
    }
    let result = eval(&text).map_or("…".to_owned(), format_result);
    egui::Area::new(Id::new("calc_preview"))
        .order(Order::Tooltip)
        .fixed_pos(rect.center_top() - vec2(0.0, 6.0))
        .pivot(Align2::CENTER_BOTTOM)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(format!("{} = {result}", text.trim()));
            });
        });
}

/// Up to 4 decimals, keeping at least one: 5.0, 33.3333.
fn format_result(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_owned()
    }
}

/// Evaluates `text` as a calculation; `None` if it isn't one.
pub fn eval(text: &str) -> Option<f64> {
    // Accept typographic operators and ignore units like px, ° or %.
    let cleaned: String = text
        .chars()
        .filter_map(|c| match c {
            '−' => Some('-'),
            '×' => Some('*'),
            '÷' => Some('/'),
            c if c.is_alphabetic() || "°%=".contains(c) || c.is_whitespace() => None,
            c => Some(c),
        })
        .collect();
    let mut p = Parser {
        s: cleaned.as_bytes(),
        i: 0,
    };
    let v = p.expr()?;
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.i += hit as usize;
        hit
    }

    // expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat(b'+') {
                v += self.term()?;
            } else if self.eat(b'-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    // term := unary (('*' | '/') unary)*
    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat(b'*') {
                v *= self.unary()?;
            } else if self.eat(b'/') {
                v /= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    // unary := ('-' | '+') unary | power
    fn unary(&mut self) -> Option<f64> {
        if self.eat(b'-') {
            return Some(-self.unary()?);
        }
        if self.eat(b'+') {
            return self.unary();
        }
        self.power()
    }

    // power := atom ('^' unary)?  (right-associative)
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat(b'^') {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    // atom := number | '(' expr ')'
    fn atom(&mut self) -> Option<f64> {
        if self.eat(b'(') {
            let v = self.expr()?;
            return self.eat(b')').then_some(v);
        }
        let start = self.i;
        while matches!(self.peek(), Some(b'0'..=b'9' | b'.' | b',')) {
            self.i += 1;
        }
        let num = std::str::from_utf8(&self.s[start..self.i]).ok()?;
        num.replace(',', ".").parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_numbers() {
        assert_eq!(eval("42"), Some(42.0));
        assert_eq!(eval("-3.5"), Some(-3.5));
        assert_eq!(eval(" 7 "), Some(7.0));
    }

    #[test]
    fn arithmetic_with_precedence() {
        assert_eq!(eval("2 + 3"), Some(5.0));
        assert_eq!(eval("2 + 3 * 4"), Some(14.0));
        assert_eq!(eval("(2 + 3) * 4"), Some(20.0));
        assert_eq!(eval("10 - 4 - 3"), Some(3.0));
        assert_eq!(eval("100 / 4 / 5"), Some(5.0));
        assert_eq!(eval("2^3^2"), Some(512.0));
        assert_eq!(eval("-2^2"), Some(-4.0));
    }

    #[test]
    fn units_and_symbols() {
        assert_eq!(eval("90° + 45"), Some(135.0));
        assert_eq!(eval("50% * 2"), Some(100.0));
        assert_eq!(eval("12px × 2"), Some(24.0));
        assert_eq!(eval("10 − 3"), Some(7.0));
        assert_eq!(eval("1,5 * 2"), Some(3.0));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(eval(""), None);
        assert_eq!(eval("2 +"), None);
        assert_eq!(eval("(2 + 3"), None);
        assert_eq!(eval("1 / 0"), None);
        assert_eq!(eval("1..2"), None);
    }

    #[test]
    fn trailing_equals_is_ignored() {
        assert_eq!(eval("2 + 3 ="), Some(5.0));
    }

    #[test]
    fn formats_results() {
        assert_eq!(format_result(5.0), "5.0");
        assert_eq!(format_result(100.0 / 3.0), "33.3333");
        assert_eq!(format_result(2.5), "2.5");
    }
}
