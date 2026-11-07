//! The row of controls of a take: the handles' zero crossing and silence trim, then the variant's
//! lower button, pitch label, raise button and reverse. It sits above the bar, to the right of
//! the notice line, and is drawn with the painter like the bar.

use super::bar::{BarAction, GLYPH_SECS};
use super::theme::{self, RAISED, TEXT, TEXT_QUIET, Weight};
use crate::variant::{MAX_SEMITONES, Variant};
use eframe::egui::{Align2, Color32, Id, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};

/// Width of the row of controls, from the zero crossing to `rev`.
pub const ROW_WIDTH: f32 = 200.0;
const ROW_HEIGHT: f32 = 22.0;
const ICON_WIDTH: f32 = 24.0;
/// Space between the controls of the handles and those of the variant.
const ICONS_GAP: f32 = 12.0;
const STEP_WIDTH: f32 = 20.0;
const LABEL_WIDTH: f32 = 48.0;
const REVERSE_WIDTH: f32 = 42.0;
/// Space between the raise button and `rev`.
const REVERSE_GAP: f32 = 10.0;
/// How far the row ends from the right edge of the bar.
const EDGE: f32 = 2.0;
/// How much of its color a control that takes no clicks keeps.
const DIMMED: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    ZeroCrossing,
    TrimSilence,
    Down,
    /// The pitch label: it goes back to the original pitch.
    Label,
    Up,
    Reverse,
}

/// The controls, left to right.
const CONTROLS: [Control; 6] =
    [Control::ZeroCrossing, Control::TrimSilence, Control::Down, Control::Label, Control::Up, Control::Reverse];

impl Control {
    fn action(self) -> BarAction {
        match self {
            Control::ZeroCrossing => BarAction::ZeroCrossing,
            Control::TrimSilence => BarAction::TrimSilence,
            Control::Down => BarAction::PitchDown,
            Control::Label => BarAction::PitchReset,
            Control::Up => BarAction::PitchUp,
            Control::Reverse => BarAction::Reverse,
        }
    }

    fn tip(self) -> &'static str {
        match self {
            Control::ZeroCrossing => "Cruce por cero (Z)",
            Control::TrimSilence => "Recortar silencios (T)",
            Control::Down => "Bajar un semitono (\u{2212})",
            Control::Label => "Volver al tono original",
            Control::Up => "Subir un semitono (+)",
            Control::Reverse => "Reversa (I)",
        }
    }

    /// Whether a click does anything with `variant`: the steps stop at an octave.
    fn responds(self, variant: Variant) -> bool {
        match self {
            Control::Down => variant.semitones > -MAX_SEMITONES,
            Control::Up => variant.semitones < MAX_SEMITONES,
            Control::ZeroCrossing | Control::TrimSilence | Control::Label | Control::Reverse => true,
        }
    }
}

/// The row of controls: on the notice line above `bar`, against its right edge.
pub fn row_rect(bar: Rect) -> Rect {
    let right = bar.right() - EDGE;
    let center = bar.top() - super::NOTICE_ROOM / 2.0;
    Rect::from_min_max(
        pos2(right - ROW_WIDTH, center - ROW_HEIGHT / 2.0),
        pos2(right, center + ROW_HEIGHT / 2.0),
    )
}

/// Where each control of `CONTROLS` goes in `row`.
fn control_rects(row: Rect) -> [Rect; 6] {
    let at = |offset: f32, width: f32| {
        Rect::from_min_size(pos2(row.left() + offset, row.top()), vec2(width, row.height()))
    };
    let down = ICON_WIDTH * 2.0 + ICONS_GAP;
    let label = down + STEP_WIDTH;
    let up = label + LABEL_WIDTH;
    [
        at(0.0, ICON_WIDTH),
        at(ICON_WIDTH, ICON_WIDTH),
        at(down, STEP_WIDTH),
        at(label, LABEL_WIDTH),
        at(up, STEP_WIDTH),
        at(up + STEP_WIDTH + REVERSE_GAP, REVERSE_WIDTH),
    ]
}

/// Draws the controls above `bar`, with `enabled` off while something else holds the window.
/// Returns the action clicked this frame, if any.
pub fn show(ui: &mut Ui, bar: Rect, variant: Variant, enabled: bool) -> Option<BarAction> {
    let mut action = None;
    for (control, rect) in CONTROLS.into_iter().zip(control_rects(row_rect(bar))) {
        let live = enabled && control.responds(variant);
        if control_button(ui, control, rect, variant, live) {
            action = action.or(Some(control.action()));
        }
    }
    action
}

/// The variant after `action`: a step of a semitone, back to the original pitch, or the
/// reverse switched. Other actions leave it as it is.
pub fn after(action: BarAction, variant: Variant) -> Variant {
    match action {
        BarAction::PitchDown => variant.stepped(-1),
        BarAction::PitchUp => variant.stepped(1),
        BarAction::PitchReset => Variant { semitones: 0, ..variant },
        BarAction::Reverse => Variant { reversed: !variant.reversed, ..variant },
        _ => variant,
    }
}

/// One control, as the quiet buttons of the bar: transparent at rest, a raised pill and 1.1 on
/// hover, 0.9 pressed. `rev` lights up with `variant`: a white fill and black text. A control
/// that is not `live` takes no clicks and is dimmed. Returns whether it was clicked.
fn control_button(ui: &mut Ui, control: Control, rect: Rect, variant: Variant, live: bool) -> bool {
    let id = Id::unique(("take-row", control as u8));
    let response = ui.interact(rect, id, if live { Sense::click() } else { Sense::hover() });
    let ctx = ui.ctx().clone();
    let hover = theme::hover_t(&ctx, id.with("hover"), live && response.hovered());
    let pressed = live && response.is_pointer_button_down_on();
    let scale = theme::press(&ctx, id.with("press"), pressed, 0.9) * theme::hover_scale(0.1, hover);
    let fade = |name: &str, on: bool| theme::transition(&ctx, id.with(name), on, GLYPH_SECS, GLYPH_SECS);
    let dim = DIMMED + (1.0 - DIMMED) * fade("live", live);
    // `rev` lights up with the reverse; the label turns to the main text with a pitch.
    let lit = if control == Control::Reverse { fade("on", variant.reversed) } else { 0.0 };
    let pitched = if control == Control::Label { fade("pitched", variant.semitones != 0) } else { 0.0 };
    let color = TEXT_QUIET.lerp_to_gamma(TEXT, hover.max(pitched)).lerp_to_gamma(Color32::BLACK, lit).gamma_multiply(dim);

    let p = ui.painter();
    let c = rect.center();
    let pill = Rect::from_center_size(c, rect.size() * scale);
    if hover > 0.0 {
        p.rect_filled(pill, pill.height() / 2.0, RAISED.gamma_multiply(hover));
    }
    if lit > 0.0 {
        p.rect_filled(pill, pill.height() / 2.0, Color32::WHITE.gamma_multiply(lit * dim));
    }
    let font = theme::font(12.0, Weight::Regular);
    match control {
        Control::ZeroCrossing => paint_zero_crossing(p, c, scale, color),
        Control::TrimSilence => paint_trim(p, c, scale, color),
        Control::Down | Control::Up => paint_step(p, c, scale, control == Control::Up, color),
        Control::Label => {
            theme::tabular(p, c, Align2::CENTER_CENTER, &variant.label(), font, color);
        }
        Control::Reverse => {
            p.text(c, Align2::CENTER_CENTER, "rev", font, color);
        }
    }
    live && response.on_hover_text(control.tip()).clicked()
}

/// A cycle of a wave crossing its axis, about 18 points across, centred on `c`.
fn paint_zero_crossing(p: &Painter, c: Pos2, scale: f32, color: Color32) {
    let at = |x: f32, y: f32| pos2(c.x + x * scale, c.y + y * scale);
    p.line_segment([at(-10.0, 0.0), at(10.0, 0.0)], Stroke::new(1.0, color.gamma_multiply(0.45)));
    let wave = (-9..=9).map(|i| {
        let x = i as f32;
        at(x, -(x / 9.0 * std::f32::consts::PI).sin() * 5.0)
    });
    p.add(Shape::line(wave.collect(), Stroke::new(1.6, color)));
}

/// Two handles closing in on what is left between them, about 18 points across, centred on `c`.
fn paint_trim(p: &Painter, c: Pos2, scale: f32, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let at = |x: f32, y: f32| pos2(c.x + x * scale, c.y + y * scale);
    for side in [-1.0, 1.0] {
        let handle = vec![at(side * 9.0, -6.0), at(side * 5.0, -6.0), at(side * 5.0, 6.0), at(side * 9.0, 6.0)];
        p.add(Shape::line(handle, stroke));
    }
    p.line_segment([at(-2.5, 0.0), at(2.5, 0.0)], stroke);
}

/// A minus, or a plus when `plus`, about 9 points across, centred on `c`.
fn paint_step(p: &Painter, c: Pos2, scale: f32, plus: bool, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    let at = |x: f32, y: f32| pos2(c.x + x * scale, c.y + y * scale);
    p.line_segment([at(-4.5, 0.0), at(4.5, 0.0)], stroke);
    if plus {
        p.line_segment([at(0.0, -4.5), at(0.0, 4.5)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::bar;
    use eframe::egui::{Context, Event, Modifiers, PointerButton, RawInput, Shape};

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0))
    }

    /// Where the controls are in the usual window, in the order cruce por cero, recortar
    /// silencios, Bajar, etiqueta, Subir and `rev`.
    fn controls() -> [Rect; 6] {
        control_rects(row_rect(bar::bar_rect(screen())))
    }

    fn pitched(semitones: i8) -> Variant {
        Variant { semitones, reversed: false }
    }

    /// Hovers, presses and releases at `at` over headless passes of the controls; returns
    /// the action of any pass.
    fn click(variant: Variant, enabled: bool, at: Pos2) -> Option<BarAction> {
        let bar = bar::bar_rect(screen());
        let ctx = Context::default();
        theme::install(&ctx);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut action = None;
        let passes = [vec![], vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]];
        for (i, events) in passes.into_iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(screen()),
                time: Some(i as f64 / 60.0),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| action = action.or(show(ui, bar, variant, enabled)));
            output.drop_without_applying_deltas();
        }
        action
    }

    /// The text the controls paint for `variant`, without the spaces.
    fn painted(variant: Variant) -> String {
        let bar = bar::bar_rect(screen());
        let ctx = Context::default();
        theme::install(&ctx);
        let input = RawInput { screen_rect: Some(screen()), ..Default::default() };
        let output = ctx.run_ui(input, |ui| {
            show(ui, bar, variant, true);
        });
        let text: String = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        text.split_whitespace().collect()
    }

    #[test]
    fn clicking_each_control_gives_its_action() {
        let variant = pitched(-3);
        let [zero, trim, down, label, up, reverse] = controls();
        assert_eq!(click(variant, true, zero.center()), Some(BarAction::ZeroCrossing));
        assert_eq!(click(variant, true, trim.center()), Some(BarAction::TrimSilence));
        assert_eq!(click(variant, true, down.center()), Some(BarAction::PitchDown));
        assert_eq!(click(variant, true, label.center()), Some(BarAction::PitchReset));
        assert_eq!(click(variant, true, up.center()), Some(BarAction::PitchUp));
        assert_eq!(click(variant, true, reverse.center()), Some(BarAction::Reverse));
    }

    #[test]
    fn the_limits_take_no_clicks() {
        let [_, _, down, _, up, _] = controls();
        assert_eq!(click(pitched(-12), true, down.center()), None);
        assert_eq!(click(pitched(-12), true, up.center()), Some(BarAction::PitchUp));
        assert_eq!(click(pitched(12), true, up.center()), None);
        assert_eq!(click(pitched(12), true, down.center()), Some(BarAction::PitchDown));
    }

    #[test]
    fn nothing_responds_while_disabled() {
        for control in controls() {
            assert_eq!(click(pitched(-3), false, control.center()), None);
        }
    }

    #[test]
    fn each_action_changes_the_variant() {
        let v = Variant { semitones: -3, reversed: true };
        assert_eq!(after(BarAction::PitchDown, v).semitones, -4);
        assert_eq!(after(BarAction::PitchUp, v).semitones, -2);
        assert_eq!(after(BarAction::PitchReset, v), Variant { semitones: 0, reversed: true });
        assert_eq!(after(BarAction::Reverse, v), Variant { semitones: -3, reversed: false });
        assert_eq!(after(BarAction::Play, v), v);
    }

    #[test]
    fn the_row_fits_above_the_bar_in_the_smallest_window() {
        let bar = bar::bar_rect(Rect::from_min_size(Pos2::ZERO, crate::ui::MIN_WINDOW.into()));
        let row = row_rect(bar);
        assert_eq!((row.width(), row.right()), (ROW_WIDTH, bar.right() - 2.0));
        assert!(row.height() <= crate::ui::NOTICE_ROOM && row.bottom() <= bar.top());
        assert_eq!(row.center().y, bar.top() - crate::ui::NOTICE_ROOM / 2.0);
    }

    #[test]
    fn the_controls_fill_the_row_in_order() {
        let row = row_rect(bar::bar_rect(screen()));
        let [zero, trim, down, label, up, reverse] = controls();
        assert_eq!(row.width(), 200.0);
        assert_eq!([zero.width(), trim.width(), down.width(), label.width(), up.width(), reverse.width()], [24.0, 24.0, 20.0, 48.0, 20.0, 42.0]);
        assert_eq!((zero.left(), reverse.right()), (row.left(), row.right()));
        assert_eq!(trim.left(), zero.right());
        assert_eq!(down.left() - trim.right(), 12.0);
        assert_eq!((label.left(), up.left()), (down.right(), label.right()));
        assert_eq!(reverse.left() - up.right(), 10.0);
        for control in [zero, trim, down, label, up, reverse] {
            assert_eq!((control.top(), control.bottom()), (row.top(), row.bottom()));
        }
    }

    #[test]
    fn the_label_says_the_pitch_and_rev_is_always_there() {
        assert_eq!(painted(pitched(0)), "0strev");
        assert_eq!(painted(pitched(-3)), "\u{2212}3strev");
        assert_eq!(painted(pitched(12)), "+12strev");
    }

    #[test]
    fn the_hover_texts_match_the_spec() {
        assert_eq!(Control::ZeroCrossing.tip(), "Cruce por cero (Z)");
        assert_eq!(Control::TrimSilence.tip(), "Recortar silencios (T)");
        assert_eq!(Control::Down.tip(), "Bajar un semitono (\u{2212})");
        assert_eq!(Control::Label.tip(), "Volver al tono original");
        assert_eq!(Control::Up.tip(), "Subir un semitono (+)");
        assert_eq!(Control::Reverse.tip(), "Reversa (I)");
    }
}
