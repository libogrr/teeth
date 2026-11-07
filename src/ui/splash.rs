//! The screen Teeth opens with: the clean silhouette of the logo on black, blinking twice.
//! Asked for by the user on 7 November 2026, in the style of TIDAL. It covers the window
//! while the app starts underneath; a click or a key skips it.

use eframe::egui::{
    self, Align2, Color32, ColorImage, Context, Id, LayerId, Order, Rect, Sense, TextureHandle, pos2, vec2,
};

/// One blink of the mark: from dark to white and back. Slow, as the user asked.
const BLINK_SECS: f64 = 1.3;
/// How long the mark blinks: two blinks.
const BLINKS_SECS: f64 = 2.0 * BLINK_SECS;
/// How long the black takes to fade and show the app.
const FADE_SECS: f64 = 0.35;
/// How long the mark shows, still, when Windows animations are off.
const STILL_SECS: f64 = 0.8;
/// Height of the mark as a share of the window's height, and its limits in points: half of
/// the first version's, as the user asked.
const MARK_SHARE: f32 = 0.125;
const MARK_MIN: f32 = 32.0;
const MARK_MAX: f32 = 56.0;

/// Opacities of the black backdrop and of the mark, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplashFrame {
    pub backdrop: f32,
    pub mark: f32,
}

/// The splash screen `t` seconds after it first showed; `None` once it is over. Without
/// `animated` the mark shows still and the app appears at once.
pub fn opacity(t: f64, animated: bool) -> Option<SplashFrame> {
    if !animated {
        return (t < STILL_SECS).then_some(SplashFrame { backdrop: 1.0, mark: 1.0 });
    }
    if t < BLINKS_SECS {
        let mark = 0.5 - 0.5 * (std::f64::consts::TAU * t / BLINK_SECS).cos();
        return Some(SplashFrame { backdrop: 1.0, mark: mark as f32 });
    }
    let fade = (t - BLINKS_SECS) / FADE_SECS;
    (fade < 1.0).then_some(SplashFrame { backdrop: (1.0 - fade) as f32, mark: 0.0 })
}

/// The splash screen's clock: it starts on the first frame it is asked about.
#[derive(Debug, Default)]
pub struct Splash {
    start: Option<f64>,
    skipped: bool,
}

impl Splash {
    pub fn new() -> Splash {
        Splash::default()
    }

    /// What to paint at `now`, in seconds; `None` once it is over or `skip` was asked once.
    pub fn frame(&mut self, now: f64, animated: bool, skip: bool) -> Option<SplashFrame> {
        self.skipped |= skip;
        if self.skipped {
            return None;
        }
        let start = *self.start.get_or_insert(now);
        opacity(now - start, animated)
    }
}

/// The white silhouette from its PNG; `None` when it cannot be read, and then the app opens
/// without a splash screen.
pub fn mark_image(png: &[u8]) -> Option<ColorImage> {
    let icon = eframe::icon_data::from_png_bytes(png).ok()?;
    let size = [icon.width as usize, icon.height as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, &icon.rgba))
}

/// Height of the mark, in points, in a window `window` points high.
pub fn mark_height(window: f32) -> f32 {
    (window * MARK_SHARE).clamp(MARK_MIN, MARK_MAX)
}

/// Paints the splash screen over the whole window, above the bar, and catches the pointer so
/// nothing underneath reacts. `true` when it was clicked.
pub fn show(ctx: &Context, mark: &TextureHandle, frame: SplashFrame) -> bool {
    let screen = ctx.content_rect();
    // Painted straight on a layer: a new Area hides its first frame, and the app would show.
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("splash-paint")));
    painter.rect_filled(screen, 0.0, Color32::BLACK.gamma_multiply(frame.backdrop));
    if frame.mark > 0.0 {
        let [w, h] = mark.size();
        let height = mark_height(screen.height());
        let size = vec2(height * w as f32 / h as f32, height);
        let rect = Align2::CENTER_CENTER.align_size_within_rect(size, screen);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(mark.id(), rect, uv, Color32::WHITE.gamma_multiply(frame.mark));
    }
    egui::Area::new(Id::unique("splash-catch"))
        .order(Order::Tooltip)
        .fixed_pos(screen.min)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_min_size(screen.size());
            ui.interact(screen, Id::unique("splash-click"), Sense::click()).clicked()
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn at(t: f64) -> SplashFrame {
        opacity(t, true).unwrap()
    }

    #[test]
    fn the_mark_blinks_twice_on_black_and_then_the_black_fades() {
        assert!(close(at(0.0).backdrop, 1.0) && close(at(0.0).mark, 0.0));
        assert!(close(at(0.65).mark, 1.0) && close(at(0.65).backdrop, 1.0));
        assert!(close(at(1.3).mark, 0.0));
        assert!(close(at(1.95).mark, 1.0));
        let fading = at(2.6 + 0.175);
        assert!(close(fading.backdrop, 0.5) && close(fading.mark, 0.0), "{fading:?}");
        assert!(opacity(3.0, true).is_none());
    }

    #[test]
    fn the_mark_is_an_eighth_of_the_window_high_within_its_limits() {
        assert!(close(mark_height(380.0), 47.5));
        assert!(close(mark_height(300.0), 37.5));
        assert!(close(mark_height(200.0), 32.0));
        assert!(close(mark_height(1000.0), 56.0));
    }

    #[test]
    fn without_animations_the_mark_stays_still_and_then_goes() {
        let still = opacity(0.5, false).unwrap();
        assert!(close(still.backdrop, 1.0) && close(still.mark, 1.0));
        assert!(opacity(0.81, false).is_none());
    }

    #[test]
    fn time_counts_from_the_first_frame_and_a_skip_ends_it_for_good() {
        let mut splash = Splash::new();
        let first = splash.frame(100.0, true, false).unwrap();
        assert!(close(first.backdrop, 1.0) && close(first.mark, 0.0));
        assert!(close(splash.frame(100.65, true, false).unwrap().mark, 1.0));
        assert!(splash.frame(100.5, true, true).is_none());
        assert!(splash.frame(100.6, true, false).is_none());
    }
}
