//! Floating glass bar: the source on the left; rewind, record, listen and loop in the
//! middle; the time, download and discard on the right; and the keys that do the same.
//! Icons are shapes drawn with the painter, no icon font.

use super::theme::{
    self, DANGER, GLASS_EDGE, HAIRLINE, RADIUS, RAISED, SURFACE, TEXT, TEXT_QUIET, TEXT_SUPPORT,
    Weight,
};
use eframe::egui::{
    Align2, Color32, Context, CornerRadius, Event, FontId, Galley, Id, Key, Modifiers, Painter,
    Pos2, Rect, Sense, Shadow, Shape, Stroke, StrokeKind, TextureId, Ui, pos2,
    text::LayoutJob, vec2,
};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarAction {
    Record,
    Stop,
    Play,
    Save,
    Discard,
    OpenSource,
    Rewind,
    Loop,
    Download,
    /// The variant controls above the bar and their keys.
    PitchDown,
    PitchUp,
    PitchReset,
    Reverse,
    ZeroCrossing,
    TrimSilence,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarMode {
    Ready,
    /// A recording is starting.
    Preparing,
    Recording,
    Take,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Record,
    Stop,
    Play,
    Pause,
    Discard,
    Rewind,
    Loop,
    Download,
    /// Record while the recording starts: it takes no clicks.
    Preparing,
}

/// What the source block shows.
pub struct SourceView {
    pub name: String,
    pub icon: Option<TextureId>,
    /// While recording the source cannot change.
    pub locked: bool,
    /// False on a Windows without process loopback: the selector does not open.
    pub available: bool,
    /// «Todo el sistema»: a speaker instead of an app icon.
    pub system: bool,
}

pub struct BarState {
    pub mode: BarMode,
    pub playing: bool,
    /// The loop button is lit.
    pub looping: bool,
    pub can_record: bool,
    pub time: String,
    pub source: SourceView,
    pub device: String,
}

pub const BAR_H: f32 = 66.0;
pub const MARGIN: f32 = 14.0;
const PAD: f32 = 14.0;
const RECORD: f32 = 42.0;
const PLAY: f32 = 38.0;
const QUIET: f32 = 32.0;
const ICON_BOX: f32 = 36.0;
const GAP: f32 = 14.0;
/// How long the bar takes to go to the buttons of a take and back.
pub(super) const TAKE_SECS: f32 = 0.22;
/// How far the buttons on the right slide in from.
const SLIDE_IN: f32 = 10.0;
/// How often the ring beats a frame: recordings last for hours, and this is plenty.
const PULSE_PAINT: Duration = Duration::from_millis(33);
/// How long one glyph takes to fade into another.
pub(super) const GLYPH_SECS: f32 = 0.12;
const UNAVAILABLE: &str = "Esta versión de Windows no permite grabar una sola aplicación.";

/// The buttons each state shows, left to right.
pub fn buttons(mode: BarMode, playing: bool) -> Vec<Button> {
    match mode {
        BarMode::Ready => vec![Button::Record],
        BarMode::Preparing => vec![Button::Preparing],
        BarMode::Recording => vec![Button::Stop],
        BarMode::Take => {
            let listen = if playing { Button::Pause } else { Button::Play };
            vec![Button::Rewind, Button::Record, listen, Button::Loop, Button::Download, Button::Discard]
        }
    }
}

/// The buttons that sit on the right, after the time.
const RIGHT: [Button; 2] = [Button::Download, Button::Discard];

/// Where each button of the state goes in `bar`, left to right: the group in the middle
/// centred, the others against the right edge. Also returns where the time ends.
fn layout(bar: Rect, mode: BarMode, playing: bool) -> (Vec<(Button, Rect)>, f32) {
    let (right, center): (Vec<Button>, Vec<Button>) =
        buttons(mode, playing).into_iter().partition(|b| RIGHT.contains(b));
    let at = |left: f32, size: f32| Rect::from_min_size(pos2(left, bar.center().y - size / 2.0), vec2(size, size));
    let mut placed = Vec::new();
    let mut x = bar.center().x - group_width(&center) / 2.0;
    for button in center {
        let size = size_of_button(button);
        placed.push((button, at(x, size)));
        x += size + GAP;
    }
    let mut time_right = bar.right() - PAD;
    let mut tail = Vec::new();
    for button in right.into_iter().rev() {
        let r = at(time_right - QUIET, QUIET);
        tail.push((button, r));
        time_right = r.left() - GAP;
    }
    placed.extend(tail.into_iter().rev());
    (placed, time_right)
}

/// One button as it is drawn while the bar moves between states.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Slot {
    button: Button,
    rect: Rect,
    opacity: f32,
    interactive: bool,
}

/// Where each button goes when the bar is `take_t` (0..1) of the way from one button in the
/// middle to the buttons of a take, and where the time ends. The middle button slides to its
/// place among them; the others come out of it, or from the right edge, fading in, and take
/// clicks only in a take.
fn slots(bar: Rect, mode: BarMode, playing: bool, take_t: f32) -> (Vec<Slot>, f32) {
    let main = match mode {
        BarMode::Ready | BarMode::Take => Button::Record,
        BarMode::Preparing => Button::Preparing,
        BarMode::Recording => Button::Stop,
    };
    let (places, time_take) = layout(bar, BarMode::Take, playing);
    let (alone, time_alone) = layout(bar, BarMode::Ready, false);
    let middle = alone[0].1;
    let between = |from: Rect, to: Rect| Rect::from_min_max(from.min.lerp(to.min, take_t), from.max.lerp(to.max, take_t));
    let mut out = Vec::new();
    for (button, place) in places {
        if button == Button::Record {
            out.push(Slot { button: main, rect: between(middle, place), opacity: 1.0, interactive: true });
        } else if take_t > 0.0 {
            let from = if RIGHT.contains(&button) {
                place.translate(vec2(SLIDE_IN, 0.0))
            } else {
                Rect::from_center_size(middle.center(), place.size())
            };
            out.push(Slot { button, rect: between(from, place), opacity: take_t, interactive: mode == BarMode::Take });
        }
    }
    (out, time_alone + (time_take - time_alone) * take_t)
}

/// Width of buttons laid side by side `GAP` apart.
fn group_width(buttons: &[Button]) -> f32 {
    buttons.iter().map(|b| size_of_button(*b)).sum::<f32>() + GAP * buttons.len().saturating_sub(1) as f32
}

/// The bar floats `MARGIN` away from the window's sides and bottom.
pub fn bar_rect(window: Rect) -> Rect {
    Rect::from_min_max(
        pos2(window.left() + MARGIN, window.bottom() - MARGIN - BAR_H),
        pos2(window.right() - MARGIN, window.bottom() - MARGIN),
    )
}

/// The source block of the bar in `bar`: its icon and the text beside it.
pub fn source_rect(bar: Rect) -> Rect {
    let icon_box =
        Rect::from_min_size(pos2(bar.left() + PAD, bar.center().y - ICON_BOX / 2.0), vec2(ICON_BOX, ICON_BOX));
    let text_left = icon_box.right() + 10.0;
    // It ends before the widest middle group, so it does not change with the state.
    let middle: Vec<Button> =
        buttons(BarMode::Take, false).into_iter().filter(|b| !RIGHT.contains(b)).collect();
    let max_w = (bar.center().x - group_width(&middle) / 2.0 - GAP - text_left).max(40.0);
    Rect::from_min_max(icon_box.min, pos2(text_left + max_w, icon_box.bottom()))
}

/// Draws the bar in `rect`; returns the action clicked this frame, if any.
pub fn show(ui: &mut Ui, rect: Rect, state: &BarState) -> Option<BarAction> {
    paint_glass(ui.painter(), rect);
    let mut action = source_block(ui, rect, state);
    let ctx = ui.ctx().clone();
    let take_t = theme::transition(&ctx, Id::unique("bar-take"), state.mode == BarMode::Take, TAKE_SECS, TAKE_SECS);
    let (placed, time_right) = slots(rect, state.mode, state.playing, take_t);
    for Slot { button, rect: r, opacity, interactive } in placed {
        let outer = ui.opacity();
        ui.multiply_opacity(opacity);
        let clicked = match button {
            Button::Record => circle_button(ui, r, button, interactive && state.can_record),
            Button::Preparing => circle_button(ui, r, button, false),
            Button::Stop | Button::Play | Button::Pause => circle_button(ui, r, button, interactive),
            Button::Loop => quiet_button(ui, r, button, interactive, state.looping),
            Button::Rewind | Button::Download | Button::Discard => quiet_button(ui, r, button, interactive, false),
        };
        ui.set_opacity(outer);
        if clicked {
            action = action.or(Some(action_of(button)));
        }
    }
    let recording = theme::transition(&ctx, Id::unique("bar-time"), state.mode == BarMode::Recording, TAKE_SECS, TAKE_SECS);
    let color = TEXT_QUIET.lerp_to_gamma(TEXT, recording);
    let font = theme::font(12.0, Weight::Regular);
    theme::tabular(ui.painter(), pos2(time_right, rect.center().y), Align2::RIGHT_CENTER, &state.time, font, color);
    action
}

fn action_of(button: Button) -> BarAction {
    match button {
        Button::Record | Button::Preparing => BarAction::Record,
        Button::Stop => BarAction::Stop,
        Button::Play | Button::Pause => BarAction::Play,
        Button::Discard => BarAction::Discard,
        Button::Rewind => BarAction::Rewind,
        Button::Loop => BarAction::Loop,
        Button::Download => BarAction::Download,
    }
}

fn size_of_button(button: Button) -> f32 {
    match button {
        Button::Record | Button::Stop | Button::Preparing => RECORD,
        Button::Play | Button::Pause => PLAY,
        Button::Discard | Button::Rewind | Button::Loop | Button::Download => QUIET,
    }
}

fn tip(button: Button) -> &'static str {
    match button {
        Button::Record => "Grabar (R)",
        Button::Stop => "Parar (R)",
        Button::Play => "Escuchar (Espacio)",
        Button::Pause => "Pausar (Espacio)",
        Button::Discard => "Descartar (Supr)",
        Button::Rewind => "Al inicio (Inicio)",
        Button::Loop => "Bucle (L)",
        Button::Download => "Guardar en Samples",
        Button::Preparing => super::PREPARING,
    }
}

/// Opaque near-black glass: a soft shadow under it, a hairline around it and a lighter
/// line where light catches its top edge.
fn paint_glass(p: &Painter, rect: Rect) {
    let radius = CornerRadius::same(RADIUS);
    let shadow = Shadow { offset: [0, 14], blur: 30, spread: 0, color: Color32::from_black_alpha(180) };
    p.add(shadow.as_shape(rect, radius));
    p.rect(rect, radius, SURFACE, Stroke::new(1.0, HAIRLINE), StrokeKind::Inside);
    let top = rect.top() + 0.5;
    let inset = f32::from(RADIUS);
    p.line_segment([pos2(rect.left() + inset, top), pos2(rect.right() - inset, top)], Stroke::new(1.0, GLASS_EDGE));
}

/// The source block: icon, name with an arrow, and the output device under it.
fn source_block(ui: &mut Ui, bar: Rect, state: &BarState) -> Option<BarAction> {
    let s = &state.source;
    let block = source_rect(bar);
    let icon_box = Rect::from_min_size(block.min, vec2(ICON_BOX, ICON_BOX));
    let text_left = icon_box.right() + 10.0;
    let max_w = block.right() - text_left;
    let open = !s.locked && s.available;
    let sense = if open { Sense::click() } else { Sense::hover() };
    let response = ui.interact(block, Id::unique("bar-source"), sense);
    let hover = theme::hover_t(ui.ctx(), Id::unique("bar-source-hover"), open && response.hovered());

    let p = ui.painter();
    let edge = HAIRLINE.lerp_to_gamma(GLASS_EDGE, hover);
    p.rect(icon_box, CornerRadius::same(RADIUS), RAISED, Stroke::new(1.0, edge), StrokeKind::Inside);
    match s.icon {
        Some(texture) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.image(texture, Rect::from_center_size(icon_box.center(), vec2(20.0, 20.0)), uv, Color32::WHITE);
        }
        None if s.system => paint_speaker(p, icon_box.center(), TEXT_SUPPORT),
        None => paint_app(p, icon_box.center(), TEXT_SUPPORT),
    }

    let name = one_line(p, &s.name, theme::font(13.0, Weight::Medium), TEXT, max_w - 16.0);
    let name_rect = Rect::from_min_size(pos2(text_left, bar.center().y - 2.0 - name.size().y), name.size());
    p.galley(name_rect.min, name, TEXT);
    if open {
        paint_chevron(p, pos2(name_rect.right() + 8.0, name_rect.center().y), TEXT_QUIET);
    }
    let device = one_line(p, &state.device, theme::font(12.0, Weight::Regular), TEXT_QUIET, max_w);
    p.galley(pos2(text_left, bar.center().y + 2.0), device, TEXT_QUIET);

    let response = if s.available { response } else { response.on_hover_text(UNAVAILABLE) };
    (open && response.clicked()).then_some(BarAction::OpenSource)
}

/// One line of text cut to `max_w`.
pub(super) fn one_line(p: &Painter, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_w.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    p.layout_job(job)
}

/// A white circle for what sounds. Hover grows it to 1.06 and widens its glow; a press
/// compresses it to 0.92 and springs back. Stop carries the beating red ring. Record and stop
/// are one circle whose glyph fades into the other, and so are play and pause.
fn circle_button(ui: &mut Ui, rect: Rect, button: Button, enabled: bool) -> bool {
    let (first, second) = match button {
        Button::Play | Button::Pause => (Button::Play, Button::Pause),
        _ => (Button::Record, Button::Stop),
    };
    let id = Id::unique(("bar-circle", first as u8));
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, id, sense);
    let ctx = ui.ctx().clone();
    let hover = theme::hover_t(&ctx, id.with("hover"), enabled && response.hovered());
    let pressed = enabled && response.is_pointer_button_down_on();
    let scale = theme::press(&ctx, id.with("press"), pressed, 0.92) * theme::hover_scale(0.06, hover);
    let c = rect.center();
    let r = rect.width() / 2.0 * scale;
    let p = ui.painter();
    match button {
        Button::Stop => paint_pulse(&ctx, p, c, r, DANGER, 1.0),
        Button::Preparing => paint_pulse(&ctx, p, c, r, Color32::WHITE, 1.4),
        _ => {}
    }
    if enabled {
        p.circle_filled(c, r + 2.0 + 4.0 * hover, Color32::from_white_alpha((12.0 + 12.0 * hover) as u8));
    }
    let lit = theme::transition(&ctx, id.with("lit"), enabled || button == Button::Preparing, GLYPH_SECS, GLYPH_SECS);
    p.circle_filled(c, r, Color32::from_white_alpha((64.0 + 191.0 * lit) as u8));
    let t = theme::transition(&ctx, id.with("glyph"), button == second, GLYPH_SECS, GLYPH_SECS);
    paint_glyph(p, c, scale, first, 1.0 - t);
    paint_glyph(p, c, scale, second, t);
    enabled && response.on_hover_text(tip(button)).clicked()
}

/// A quiet round button: transparent at rest, a raised fill and 1.1 on hover, 0.9 pressed.
/// `on` lights it: a white fill and a black glyph.
fn quiet_button(ui: &mut Ui, rect: Rect, button: Button, enabled: bool, on: bool) -> bool {
    let id = Id::unique(("bar-quiet", button as u8));
    let response = ui.interact(rect, id, if enabled { Sense::click() } else { Sense::hover() });
    let ctx = ui.ctx().clone();
    let hover = theme::hover_t(&ctx, id.with("hover"), response.hovered());
    let pressed = response.is_pointer_button_down_on();
    let scale = theme::press(&ctx, id.with("press"), pressed, 0.9) * theme::hover_scale(0.1, hover);
    let c = rect.center();
    let p = ui.painter();
    let r = rect.width() / 2.0 * scale;
    if hover > 0.0 {
        p.circle_filled(c, r, RAISED.gamma_multiply(hover));
    }
    let lit = theme::transition(&ctx, id.with("on"), on, GLYPH_SECS, GLYPH_SECS);
    if lit > 0.0 {
        p.circle_filled(c, r, Color32::WHITE.gamma_multiply(lit));
    }
    let color = TEXT_QUIET.lerp_to_gamma(TEXT, hover).lerp_to_gamma(Color32::BLACK, lit);
    paint_line_glyph(p, c, scale, button, color);
    enabled && response.on_hover_text(tip(button)).clicked()
}

/// The line icons of the quiet buttons, about 12 points across.
fn paint_line_glyph(p: &Painter, c: Pos2, scale: f32, button: Button, color: Color32) {
    let stroke = Stroke::new(1.8, color);
    let at = |x: f32, y: f32| pos2(c.x + x * scale, c.y + y * scale);
    match button {
        Button::Discard => {
            p.line_segment([at(-5.0, -5.0), at(5.0, 5.0)], stroke);
            p.line_segment([at(-5.0, 5.0), at(5.0, -5.0)], stroke);
        }
        Button::Rewind => {
            // A bar and a triangle pointing at it.
            p.line_segment([at(-5.0, -5.5), at(-5.0, 5.5)], stroke);
            let arrow = vec![at(-3.0, 0.0), at(5.5, -5.5), at(5.5, 5.5)];
            p.add(Shape::convex_polygon(arrow, color, Stroke::NONE));
        }
        Button::Loop => paint_loop(p, at, stroke),
        Button::Download => {
            // An arrow down onto a line.
            p.line_segment([at(0.0, -6.0), at(0.0, 2.0)], stroke);
            p.line_segment([at(-4.0, -1.5), at(0.0, 2.5)], stroke);
            p.line_segment([at(4.0, -1.5), at(0.0, 2.5)], stroke);
            p.line_segment([at(-5.5, 6.0), at(5.5, 6.0)], stroke);
        }
        Button::Record | Button::Stop | Button::Play | Button::Pause | Button::Preparing => {}
    }
}

/// Two arrows chasing each other round a rounded rectangle, the usual repeat sign.
/// `at` maps glyph coordinates to the screen.
fn paint_loop(p: &Painter, at: impl Fn(f32, f32) -> Pos2, stroke: Stroke) {
    use std::f32::consts::{FRAC_PI_2, PI};
    use std::iter::once;
    let at = &at;
    let (w, h, r) = (6.5, 4.5, 2.5);
    let corner = |cx: f32, cy: f32, from: f32| {
        (0..=4).map(move |i| {
            let a = from + i as f32 * FRAC_PI_2 / 4.0;
            at(cx + r * a.cos(), cy + r * a.sin())
        })
    };
    // Up the left side and right along the top; down the right side and left along the bottom.
    let top = once(at(-w, 1.5)).chain(corner(-w + r, -h + r, PI)).chain(once(at(2.5, -h)));
    let bottom = once(at(w, -1.5)).chain(corner(w - r, h - r, 0.0)).chain(once(at(-2.5, h)));
    p.add(Shape::line(top.collect(), stroke));
    p.add(Shape::line(bottom.collect(), stroke));
    let right = vec![at(2.0, -h - 3.0), at(5.5, -h), at(2.0, -h + 3.0)];
    let left = vec![at(-5.5, h), at(-2.0, h - 3.0), at(-2.0, h + 3.0)];
    p.add(Shape::convex_polygon(right, stroke.color, Stroke::NONE));
    p.add(Shape::convex_polygon(left, stroke.color, Stroke::NONE));
}

/// A soft ring that beats every `period` seconds while something goes on: red while
/// recording. Still when Windows animations are off.
fn paint_pulse(ctx: &Context, p: &Painter, c: Pos2, r: f32, color: Color32, period: f64) {
    let (grow, alpha) = if theme::animations_enabled() {
        let t = ((ctx.input(|i| i.time) % period) / period) as f32;
        ctx.request_repaint_after(PULSE_PAINT);
        (4.0 + 8.0 * theme::quint_out(t), 0.55 * (1.0 - t))
    } else {
        (5.0, 0.45)
    };
    p.circle_stroke(c, r + grow, Stroke::new(2.0, color.gamma_multiply(alpha)));
}

fn paint_glyph(p: &Painter, c: Pos2, scale: f32, button: Button, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let black = Color32::BLACK.gamma_multiply(alpha);
    match button {
        Button::Record | Button::Preparing => {
            p.circle_filled(c, 7.0 * scale, DANGER.gamma_multiply(alpha));
        }
        Button::Stop => {
            p.rect_filled(Rect::from_center_size(c, vec2(12.0, 12.0) * scale), 2.5, black);
        }
        Button::Play => {
            let s = 7.0 * scale;
            let pts = vec![pos2(c.x - 0.55 * s, c.y - s), pos2(c.x + s, c.y), pos2(c.x - 0.55 * s, c.y + s)];
            p.add(Shape::convex_polygon(pts, black, Stroke::NONE));
        }
        Button::Pause => {
            for dx in [-3.5, 3.5] {
                let bar = Rect::from_center_size(pos2(c.x + dx * scale, c.y), vec2(3.5, 13.0) * scale);
                p.rect_filled(bar, 1.0, black);
            }
        }
        Button::Discard | Button::Rewind | Button::Loop | Button::Download => {}
    }
}

fn paint_chevron(p: &Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.5, color);
    p.line_segment([pos2(c.x - 3.5, c.y - 1.75), pos2(c.x, c.y + 1.75)], stroke);
    p.line_segment([pos2(c.x, c.y + 1.75), pos2(c.x + 3.5, c.y - 1.75)], stroke);
}

/// A speaker: a small box, its cone and two sound arcs.
pub(super) fn paint_speaker(p: &Painter, c: Pos2, color: Color32) {
    p.rect_filled(Rect::from_center_size(pos2(c.x - 5.0, c.y), vec2(4.0, 6.0)), 1.0, color);
    let cone = vec![pos2(c.x - 3.0, c.y - 3.0), pos2(c.x + 2.0, c.y - 7.0), pos2(c.x + 2.0, c.y + 7.0), pos2(c.x - 3.0, c.y + 3.0)];
    p.add(Shape::convex_polygon(cone, color, Stroke::NONE));
    let stroke = Stroke::new(1.4, color);
    for radius in [4.0_f32, 7.5] {
        let arc: Vec<Pos2> = (-4..=4)
            .map(|i| {
                let a = i as f32 * 0.17;
                pos2(c.x + 3.0 + radius * a.cos(), c.y + radius * a.sin())
            })
            .collect();
        p.add(Shape::line(arc, stroke));
    }
}

/// A generic application: a rounded square with a title strip.
pub(super) fn paint_app(p: &Painter, c: Pos2, color: Color32) {
    let r = Rect::from_center_size(c, vec2(16.0, 16.0));
    let stroke = Stroke::new(1.4, color);
    p.rect_stroke(r, 3.0, stroke, StrokeKind::Inside);
    p.line_segment([pos2(r.left(), r.top() + 4.5), pos2(r.right(), r.top() + 4.5)], stroke);
}

/// The action keys: R records or stops, Space listens or pauses, Inicio rewinds, L loops,
/// Ctrl+S saves and Supr discards; - and + lower and raise the pitch (+ also with Mayúsculas,
/// and = for keyboards where + needs it) and I reverses. Held keys do not repeat, and other
/// modifier combinations do nothing.
fn key_action(key: Key, modifiers: Modifiers, repeat: bool, recording: bool) -> Option<BarAction> {
    if repeat {
        return None;
    }
    match key {
        Key::R if modifiers.is_none() => Some(if recording { BarAction::Stop } else { BarAction::Record }),
        Key::Space if modifiers.is_none() => Some(BarAction::Play),
        Key::S if modifiers.command_only() => Some(BarAction::Save),
        Key::Delete if modifiers.is_none() => Some(BarAction::Discard),
        Key::Home if modifiers.is_none() => Some(BarAction::Rewind),
        Key::L if modifiers.is_none() => Some(BarAction::Loop),
        Key::Minus if modifiers.is_none() => Some(BarAction::PitchDown),
        Key::Plus if modifiers.is_none() || modifiers == Modifiers::SHIFT => Some(BarAction::PitchUp),
        Key::Equals if modifiers.is_none() => Some(BarAction::PitchUp),
        Key::I if modifiers.is_none() => Some(BarAction::Reverse),
        Key::Z if modifiers.is_none() => Some(BarAction::ZeroCrossing),
        Key::T if modifiers.is_none() => Some(BarAction::TrimSilence),
        _ => None,
    }
}

/// The bar action whose key went down this frame, if any.
pub fn shortcut(ctx: &Context, recording: bool) -> Option<BarAction> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::Key { key, pressed: true, repeat, modifiers, .. } => {
                key_action(*key, *modifiers, *repeat, recording)
            }
            _ => None,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_keys_map_to_bar_actions() {
        let none = Modifiers::NONE;
        assert_eq!(key_action(Key::R, none, false, false), Some(BarAction::Record));
        assert_eq!(key_action(Key::R, none, false, true), Some(BarAction::Stop));
        assert_eq!(key_action(Key::Space, none, false, false), Some(BarAction::Play));
        assert_eq!(key_action(Key::S, Modifiers::COMMAND, false, false), Some(BarAction::Save));
        assert_eq!(key_action(Key::Delete, none, false, false), Some(BarAction::Discard));
        assert_eq!(key_action(Key::Home, none, false, false), Some(BarAction::Rewind));
        assert_eq!(key_action(Key::L, none, false, false), Some(BarAction::Loop));
    }

    #[test]
    fn variant_keys_map_to_bar_actions() {
        let none = Modifiers::NONE;
        assert_eq!(key_action(Key::Minus, none, false, false), Some(BarAction::PitchDown));
        assert_eq!(key_action(Key::Plus, none, false, false), Some(BarAction::PitchUp));
        assert_eq!(key_action(Key::Plus, Modifiers::SHIFT, false, false), Some(BarAction::PitchUp));
        assert_eq!(key_action(Key::Equals, none, false, false), Some(BarAction::PitchUp));
        assert_eq!(key_action(Key::I, none, false, false), Some(BarAction::Reverse));
        assert_eq!(key_action(Key::Minus, none, true, false), None);
        assert_eq!(key_action(Key::Plus, none, true, false), None);
        assert_eq!(key_action(Key::I, none, true, false), None);
        assert_eq!(key_action(Key::Minus, Modifiers::CTRL, false, false), None);
        assert_eq!(key_action(Key::I, Modifiers::SHIFT, false, false), None);
    }

    #[test]
    fn the_keys_of_the_handles_map_to_bar_actions() {
        let none = Modifiers::NONE;
        assert_eq!(key_action(Key::Z, none, false, false), Some(BarAction::ZeroCrossing));
        assert_eq!(key_action(Key::T, none, false, false), Some(BarAction::TrimSilence));
        assert_eq!(key_action(Key::Z, none, true, false), None);
        assert_eq!(key_action(Key::T, none, true, false), None);
        assert_eq!(key_action(Key::Z, Modifiers::CTRL, false, false), None);
        assert_eq!(key_action(Key::T, Modifiers::SHIFT, false, false), None);
    }

    #[test]
    fn each_mode_shows_its_buttons() {
        assert_eq!(buttons(BarMode::Ready, false), vec![Button::Record]);
        assert_eq!(buttons(BarMode::Recording, false), vec![Button::Stop]);
        assert_eq!(buttons(BarMode::Preparing, false), vec![Button::Preparing]);
        let take = |listen| vec![Button::Rewind, Button::Record, listen, Button::Loop, Button::Download, Button::Discard];
        assert_eq!(buttons(BarMode::Take, false), take(Button::Play));
        assert_eq!(buttons(BarMode::Take, true), take(Button::Pause));
    }

    /// Width of the time of a take under ten hours, as the bar paints it.
    fn time_width() -> f32 {
        let ctx = Context::default();
        theme::install(&ctx);
        let mut width = 0.0;
        let output = ctx.run_ui(Default::default(), |ui| {
            let font = theme::font(12.0, Weight::Regular);
            let text = "9:59:59.999 / 9:59:59.999";
            width = theme::tabular(ui.painter(), Pos2::ZERO, Align2::LEFT_TOP, text, font, TEXT).width();
        });
        output.drop_without_applying_deltas();
        width
    }

    #[test]
    fn the_take_bar_fits_the_smallest_window_in_order() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, crate::ui::MIN_WINDOW.into()));
        let (placed, time_right) = layout(bar, BarMode::Take, false);
        let order: Vec<Button> = placed.iter().map(|(b, _)| *b).collect();
        assert_eq!(order, buttons(BarMode::Take, false));
        let rect = |b| placed.iter().find(|(p, _)| *p == b).map(|(_, r)| *r).unwrap();
        let time_left = time_right - time_width();
        assert!(source_rect(bar).right() <= rect(Button::Rewind).left(), "source reaches ⏮");
        assert!(rect(Button::Loop).right() < time_left, "the time overlaps the loop button");
        assert!(time_right < rect(Button::Download).left());
        assert!(rect(Button::Download).right() < rect(Button::Discard).left());
        assert!(rect(Button::Discard).right() <= bar.right() - PAD);
        for b in [Button::Rewind, Button::Loop, Button::Download, Button::Discard] {
            assert_eq!(rect(b).size(), vec2(QUIET, QUIET), "{b:?}");
        }
    }

    #[test]
    fn at_rest_the_slots_are_the_layout_of_the_state() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let (take, time) = slots(bar, BarMode::Take, false, 1.0);
        let (places, take_time) = layout(bar, BarMode::Take, false);
        assert_eq!(time, take_time);
        let got: Vec<(Button, Rect)> = take.iter().map(|s| (s.button, s.rect)).collect();
        assert_eq!(got, places);
        assert!(take.iter().all(|s| s.opacity == 1.0 && s.interactive));

        let (ready, time) = slots(bar, BarMode::Ready, false, 0.0);
        assert_eq!(ready.len(), 1);
        assert_eq!((ready[0].button, ready[0].rect.center()), (Button::Record, bar.center()));
        assert_eq!(time, bar.right() - PAD);
    }

    #[test]
    fn the_buttons_of_a_take_come_out_of_the_middle_one_and_fade_in() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let (places, _) = layout(bar, BarMode::Take, false);
        let place = |b| places.iter().find(|(p, _)| *p == b).map(|(_, r)| *r).unwrap();
        let (mid, _) = slots(bar, BarMode::Take, false, 0.5);
        let slot = |b| *mid.iter().find(|s| s.button == b).unwrap();
        let record = slot(Button::Record);
        assert_eq!(record.opacity, 1.0);
        assert_eq!(record.rect.center().x, (bar.center().x + place(Button::Record).center().x) / 2.0);
        let rewind = slot(Button::Rewind);
        assert_eq!(rewind.opacity, 0.5);
        assert_eq!(rewind.rect.center().x, (bar.center().x + place(Button::Rewind).center().x) / 2.0);
        assert!(slot(Button::Discard).rect.left() > place(Button::Discard).left());
        // Leaving a take they still show, but take no clicks.
        let (leaving, _) = slots(bar, BarMode::Recording, false, 0.5);
        assert_eq!(leaving.iter().find(|s| s.interactive).map(|s| s.button), Some(Button::Stop));
        assert!(leaving.iter().any(|s| s.button == Button::Rewind));
    }

    fn state(mode: BarMode) -> BarState {
        BarState {
            mode,
            playing: false,
            looping: false,
            can_record: true,
            time: "00:00.000".into(),
            source: SourceView { name: "Zen".into(), icon: None, locked: false, available: true, system: false },
            device: "Auriculares".into(),
        }
    }

    /// Hovers, presses and releases at `at` over headless passes of the bar; returns the
    /// action of any pass.
    fn click(state: &BarState, at: Pos2) -> Option<BarAction> {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0));
        let bar = bar_rect(screen);
        let ctx = Context::default();
        theme::install(&ctx);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: eframe::egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut action = None;
        let passes = [vec![], vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]];
        for (i, events) in passes.into_iter().enumerate() {
            let input = eframe::egui::RawInput {
                screen_rect: Some(screen),
                time: Some(i as f64 / 60.0),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| action = action.or(show(ui, bar, state)));
            output.drop_without_applying_deltas();
        }
        action
    }

    #[test]
    fn clicking_each_take_button_gives_its_action() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let take = state(BarMode::Take);
        let (placed, _) = layout(bar, BarMode::Take, false);
        for (button, rect) in placed {
            assert_eq!(click(&take, rect.center()), Some(action_of(button)), "{button:?}");
        }
    }

    #[test]
    fn nothing_can_be_clicked_while_preparing() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let mut preparing = state(BarMode::Preparing);
        preparing.source.locked = true;
        assert_eq!(click(&preparing, bar.center()), None);
    }

    #[test]
    fn record_does_nothing_when_it_cannot_record() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let mut ready = state(BarMode::Ready);
        ready.can_record = false;
        assert_eq!(click(&ready, bar.center()), None);
    }

    #[test]
    fn the_source_opens_unless_it_is_locked_or_unavailable() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let at = source_rect(bar).left_center() + vec2(20.0, 0.0);
        let mut s = state(BarMode::Ready);
        assert_eq!(click(&s, at), Some(BarAction::OpenSource));
        s.source.locked = true;
        assert_eq!(click(&s, at), None);
        s.source.locked = false;
        s.source.available = false;
        assert_eq!(click(&s, at), None);
    }

    #[test]
    fn record_stays_centred_when_it_is_alone() {
        let bar = bar_rect(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 380.0)));
        let (placed, time_right) = layout(bar, BarMode::Ready, false);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].1.center(), bar.center());
        assert_eq!(time_right, bar.right() - PAD);
    }

    #[test]
    fn the_bar_floats_with_a_margin() {
        let r = bar_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(860.0, 380.0)));
        assert_eq!(r, Rect::from_min_max(pos2(14.0, 380.0 - 14.0 - 66.0), pos2(846.0, 366.0)));
    }

    #[test]
    fn action_keys_ignore_repeats_and_other_modifiers() {
        assert_eq!(key_action(Key::R, Modifiers::NONE, true, false), None);
        assert_eq!(key_action(Key::Space, Modifiers::NONE, true, false), None);
        assert_eq!(key_action(Key::R, Modifiers::CTRL, false, false), None);
        assert_eq!(key_action(Key::S, Modifiers::NONE, false, false), None);
        assert_eq!(key_action(Key::S, Modifiers::COMMAND | Modifiers::SHIFT, false, false), None);
        assert_eq!(key_action(Key::Delete, Modifiers::SHIFT, false, false), None);
        assert_eq!(key_action(Key::ArrowLeft, Modifiers::NONE, false, false), None);
        assert_eq!(key_action(Key::Home, Modifiers::NONE, true, false), None);
        assert_eq!(key_action(Key::L, Modifiers::NONE, true, false), None);
        assert_eq!(key_action(Key::Home, Modifiers::SHIFT, false, false), None);
        assert_eq!(key_action(Key::L, Modifiers::CTRL, false, false), None);
    }
}
