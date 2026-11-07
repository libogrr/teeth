//! Orc's visual language for egui: colors, Inter, and the motion curves.
//!
//! The screen is a dark room where only the take sounds: a black floor, white as the only
//! ink, and color only for state (red to record and for errors, amber for cuts and
//! warnings). Hover enters fast and leaves slow; a press compresses and springs back.

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id,
    Stroke, TextFormat, TextStyle, text::LayoutJob,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::BLACK;
pub const LANE: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
pub const SURFACE: Color32 = Color32::from_rgb(0x0d, 0x0d, 0x0d);
pub const RAISED: Color32 = Color32::from_rgb(0x16, 0x16, 0x16);
pub const TEXT: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 235);
pub const TEXT_SUPPORT: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 217);
pub const TEXT_QUIET: Color32 = Color32::from_rgba_unmultiplied_const(235, 235, 245, 153);
pub const HAIRLINE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 20);
pub const GLASS_EDGE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 36);
pub const DANGER: Color32 = Color32::from_rgb(0xef, 0x44, 0x44);
pub const WARNING: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);

/// Corner radius of panels, the bar, lists and the dialog.
pub const RADIUS: u8 = 8;

/// Hover enters in 150 ms and leaves in 300 ms.
const HOVER_IN: f32 = 0.150;
const HOVER_OUT: f32 = 0.300;
/// A released press springs back to rest in 350 ms.
const SPRING: f32 = 0.350;

const MEDIUM: &str = "inter-medium";
const SEMIBOLD: &str = "inter-semibold";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
}

pub fn font(size: f32, weight: Weight) -> FontId {
    let family = match weight {
        Weight::Regular => FontFamily::Proportional,
        Weight::Medium => FontFamily::Name(MEDIUM.into()),
        Weight::SemiBold => FontFamily::Name(SEMIBOLD.into()),
    };
    FontId::new(size, family)
}

/// The window and taskbar icon decoded from `png`; `None` when it cannot be read, so the
/// window opens with the default icon instead of not opening.
pub fn window_icon(png: &[u8]) -> Option<Arc<eframe::egui::IconData>> {
    eframe::icon_data::from_png_bytes(png).ok().map(Arc::new)
}

/// Loads Inter and the dark visuals.
pub fn install(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 3] = [
        ("inter", include_bytes!("../../assets/fonts/Inter-Regular.ttf")),
        (MEDIUM, include_bytes!("../../assets/fonts/Inter-Medium.ttf")),
        (SEMIBOLD, include_bytes!("../../assets/fonts/Inter-SemiBold.ttf")),
    ];
    for (name, bytes) in faces {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay behind Inter for the symbols Inter lacks.
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    for name in [MEDIUM, SEMIBOLD] {
        let mut chain = vec![name.to_string()];
        chain.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), chain);
    }
    ctx.set_fonts(fonts);

    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.0));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = BG;
        v.faint_bg_color = LANE;
        v.override_text_color = Some(TEXT);
        v.window_stroke = Stroke::new(1.0, HAIRLINE);
        v.window_corner_radius = CornerRadius::same(RADIUS);
        v.menu_corner_radius = CornerRadius::same(RADIUS);
        v.warn_fg_color = WARNING;
        v.error_fg_color = DANGER;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(RADIUS);
        }
        v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.hovered.weak_bg_fill = RAISED;
        v.widgets.active.weak_bg_fill = RAISED;
    });
}

pub fn quint_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(5)
}

/// Moves a hover progress `p` (0..1) linearly towards 1 when `rising`, else towards 0:
/// 150 ms for the whole way in, 300 ms for the whole way out.
pub fn step_progress(p: f32, rising: bool, dt: f32) -> f32 {
    if rising {
        (p + dt / HOVER_IN).min(1.0)
    } else {
        (p - dt / HOVER_OUT).max(0.0)
    }
}

/// Eased hover amount (0..1) of the widget `id`, kept in egui's memory between frames.
/// Jumps straight to its end when Windows animations are off.
pub fn hover_t(ctx: &Context, id: Id, hovered: bool) -> f32 {
    if !animations_enabled() {
        return if hovered { 1.0 } else { 0.0 };
    }
    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    let p = ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let next = step_progress(p, hovered, dt);
    ctx.data_mut(|d| d.insert_temp(id, next));
    if next != if hovered { 1.0 } else { 0.0 } {
        ctx.request_repaint();
    }
    quint_out(next)
}

/// Scale of an element that grows by `amount` when fully hovered; it stays still when
/// Windows animations are off.
pub fn growth(animated: bool, amount: f32, hover: f32) -> f32 {
    if animated { 1.0 + amount * hover } else { 1.0 }
}

/// `growth` under the current Windows setting.
pub fn hover_scale(amount: f32, hover: f32) -> f32 {
    growth(animations_enabled(), amount, hover)
}

/// Scale of a pressable element: `min` while pressed; after release it springs back to
/// 1, passing it briefly, and rests at 1 from 350 ms on.
pub fn press_scale(pressed: bool, since_release: Option<f32>, min: f32) -> f32 {
    if pressed {
        return min;
    }
    match since_release {
        Some(t) if t < SPRING => min + (1.0 - min) * egui::emath::easing::back_out(t / SPRING),
        _ => 1.0,
    }
}

/// `press_scale` for the widget `id`, remembering when it was released. Without Windows
/// animations it stays at 1.
pub fn press(ctx: &Context, id: Id, pressed: bool, min: f32) -> f32 {
    if !animations_enabled() {
        return 1.0;
    }
    let now = ctx.input(|i| i.time);
    let key = id.with("released");
    let was_pressed = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    ctx.data_mut(|d| d.insert_temp(id, pressed));
    if was_pressed && !pressed {
        ctx.data_mut(|d| d.insert_temp(key, now));
    }
    let since = ctx.data(|d| d.get_temp::<f64>(key)).map(|t| (now - t) as f32);
    let scale = press_scale(pressed, since, min);
    if scale != 1.0 && !pressed {
        ctx.request_repaint();
    }
    scale
}

/// Paints `text` with every digit in a cell as wide as the widest digit, so times do not
/// shift as they count (egui cannot turn on Inter's tabular figures). Returns the rect used.
pub fn tabular(
    painter: &egui::Painter,
    anchor: egui::Pos2,
    align: egui::Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> egui::Rect {
    let (widths, height) = painter.fonts_mut(|f| {
        let digit = ('0'..='9').map(|d| f.glyph_width(&font, d)).fold(0.0_f32, f32::max);
        let widths: Vec<f32> =
            text.chars().map(|ch| if ch.is_ascii_digit() { digit } else { f.glyph_width(&font, ch) }).collect();
        (widths, f.row_height(&font))
    });
    let rect = align.anchor_size(anchor, egui::vec2(widths.iter().sum(), height));
    let mut x = rect.left();
    for (ch, w) in text.chars().zip(widths) {
        let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, height));
        painter.text(cell.center(), egui::Align2::CENTER_CENTER, ch, font.clone(), color);
        x += w;
    }
    rect
}

/// `text` as one row in which every digit sits centred in a cell as wide as the widest
/// digit, for a time that changes while it is on screen.
pub fn tabular_job(painter: &egui::Painter, text: &str, font: FontId, color: Color32) -> LayoutJob {
    painter.fonts_mut(|f| {
        let cell = ('0'..='9').map(|d| f.glyph_width(&font, d)).fold(0.0_f32, f32::max);
        let mut job = LayoutJob::default();
        let mut after_previous = 0.0;
        for ch in text.chars() {
            let side = if ch.is_ascii_digit() { (cell - f.glyph_width(&font, ch)) / 2.0 } else { 0.0 };
            let format = TextFormat { font_id: font.clone(), color, ..Default::default() };
            job.append(ch.encode_utf8(&mut [0; 4]), after_previous + side, format);
            after_previous = side;
        }
        job
    })
}

/// Whether Windows shows animations (Accessibility → Visual effects), so a change there
/// applies at once.
pub fn animations_enabled() -> bool {
    crate::win::client_area_animation()
}

/// Whether things may move on their way to where they go: Windows allows it, and egui's
/// animation time has not been set to zero.
pub fn animated(ctx: &Context) -> bool {
    animations_enabled() && ctx.global_style().animation_time > 0.0
}

/// `value` one step closer to `target`, `dt` seconds later: fast at first, then slowing
/// down, and there in `secs`. The curve of everything that goes from one state to another.
pub fn approach(value: f32, target: f32, dt: f32, secs: f32) -> f32 {
    if secs <= 0.0 {
        return target;
    }
    let next = value + (target - value) * (1.0 - (-4.0 * dt / secs).exp());
    if (target - next).abs() < 0.004 { target } else { next }
}

/// How far the widget `id` is (0..1) on its way to `on`: `rise` seconds to turn on and `fall`
/// to turn off, kept in egui's memory between frames. Without animations it jumps.
pub fn transition(ctx: &Context, id: Id, on: bool, rise: f32, fall: f32) -> f32 {
    let target = if on { 1.0 } else { 0.0 };
    if !animated(ctx) {
        return target;
    }
    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    let value = ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
    let next = approach(value, target, dt, if on { rise } else { fall });
    ctx.data_mut(|d| d.insert_temp(id, next));
    if next != target {
        ctx.request_repaint();
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quint_out_starts_ends_and_rises() {
        assert_eq!(quint_out(0.0), 0.0);
        assert_eq!(quint_out(1.0), 1.0);
        assert!(quint_out(0.5) > 0.9);
    }

    #[test]
    fn hover_enters_in_150_ms_and_leaves_in_300_ms() {
        assert!((step_progress(0.0, true, 0.075) - 0.5).abs() < 1e-6);
        assert!((step_progress(1.0, false, 0.150) - 0.5).abs() < 1e-6);
        assert_eq!(step_progress(0.9, true, 1.0), 1.0);
        assert_eq!(step_progress(0.1, false, 1.0), 0.0);
    }

    #[test]
    fn press_compresses_then_springs_past_rest_and_settles() {
        assert_eq!(press_scale(true, None, 0.92), 0.92);
        assert_eq!(press_scale(false, Some(0.0), 0.92), 0.92);
        assert!((0..35).any(|ms| press_scale(false, Some(ms as f32 / 100.0), 0.92) > 1.0));
        assert_eq!(press_scale(false, Some(0.35), 0.92), 1.0);
        assert_eq!(press_scale(false, None, 0.92), 1.0);
    }

    #[test]
    fn approach_slows_down_and_lands_exactly_without_passing() {
        let (mut v, dt) = (0.0, 1.0 / 60.0);
        let first = approach(v, 1.0, dt, 0.2);
        assert!(first > 0.0 && first < 0.5, "{first}");
        for _ in 0..60 {
            let next = approach(v, 1.0, dt, 0.2);
            assert!(next >= v && next <= 1.0, "{v} then {next}");
            v = next;
        }
        assert_eq!(v, 1.0);
        assert_eq!(approach(1.0, 0.0, dt, 0.0), 0.0, "no time means at once");
    }

    #[test]
    fn a_transition_jumps_when_there_is_no_animation_time() {
        let ctx = Context::default();
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let id = Id::unique("prueba");
        let mut seen = Vec::new();
        for on in [true, false] {
            let output = ctx.run_ui(Default::default(), |_| seen.push(transition(&ctx, id, on, 0.2, 0.2)));
            output.drop_without_applying_deltas();
        }
        assert_eq!(seen, vec![1.0, 0.0]);
    }

    #[test]
    fn growing_on_hover_needs_animations() {
        assert_eq!(growth(true, 0.1, 1.0), 1.1);
        assert_eq!(growth(true, 0.1, 0.0), 1.0);
        assert_eq!(growth(false, 0.1, 1.0), 1.0);
    }

    #[test]
    fn tabular_text_is_one_row_as_wide_for_any_digits() {
        let ctx = Context::default();
        install(&ctx);
        let mut widths = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            for text in ["00:00.000", "11:11.111", "07:48.259"] {
                let job = tabular_job(ui.painter(), text, font(12.0, Weight::Regular), TEXT);
                let galley = ui.painter().layout_job(job);
                assert_eq!(galley.text(), text);
                widths.push(galley.size().x);
            }
        });
        output.drop_without_applying_deltas();
        // Only the space after the last digit and the pixel grid can change the width.
        assert!((widths[0] - widths[1]).abs() < 2.0 && (widths[0] - widths[2]).abs() < 2.0, "{widths:?}");
    }

    #[test]
    fn colors_match_the_spec() {
        assert_eq!(TEXT, Color32::from_rgba_unmultiplied(255, 255, 255, 235));
        assert_eq!(TEXT_QUIET, Color32::from_rgba_unmultiplied(235, 235, 245, 153));
        assert_eq!(SURFACE, Color32::from_rgb(0x0d, 0x0d, 0x0d));
        assert_eq!(DANGER, Color32::from_rgb(0xef, 0x44, 0x44));
        assert_eq!(WARNING, Color32::from_rgb(0xf5, 0x9e, 0x0b));
    }
}
