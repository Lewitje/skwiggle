//! WCAG contrast helpers for picking colours that stay readable on any background.

use eframe::egui::Color32;

/// Relative luminance of an sRGB colour, 0 (black) to 1 (white).
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG contrast ratio between two colours, 1 (identical) to 21 (black on white).
pub fn ratio(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The colour closest to `bg` (keeping its hue) that reaches `target` contrast against it.
pub fn ink(bg: Color32, target: f32) -> Color32 {
    // Above this luminance black contrasts more than white.
    let toward = if luminance(bg) > 0.179 {
        Color32::BLACK
    } else {
        Color32::WHITE
    };
    let mix = |t: f32| {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color32::from_rgb(
            m(bg.r(), toward.r()),
            m(bg.g(), toward.g()),
            m(bg.b(), toward.b()),
        )
    };
    // Binary search for the smallest mix that's contrasty enough.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        if ratio(mix(mid), bg) >= target {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_max() {
        assert!((ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
    }

    #[test]
    fn ink_hits_target_without_overshooting() {
        for bg in [
            Color32::from_gray(245),
            Color32::from_gray(30),
            Color32::from_gray(128),
            Color32::from_rgb(40, 60, 200),
            Color32::from_rgb(255, 230, 120),
        ] {
            let r = ratio(ink(bg, 1.4), bg);
            assert!((1.4..1.5).contains(&r), "{bg:?} gave {r}");
        }
    }

    #[test]
    fn ink_goes_dark_on_light_and_light_on_dark() {
        let light = Color32::from_gray(245);
        let dark = Color32::from_gray(30);
        assert!(luminance(ink(light, 1.4)) < luminance(light));
        assert!(luminance(ink(dark, 1.4)) > luminance(dark));
    }
}
