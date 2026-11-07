//! Unit tests of the Take state: pure input mapping, lane reads on a real file, and the
//! editor driven through headless egui passes (wheel, keys, drags and clicks).

use super::*;
use crate::ui::{Destructive, Gate, TakeFacts, gate};
use crate::wav::{WavFormat, WavReader, WavWriter};
use eframe::egui::epaint::ClippedShape;
use eframe::egui::{CentralPanel, MouseWheelSource, Pos2, RawInput, Shape, TouchPhase, pos2, vec2};

#[test]
fn handles_grab_within_six_points() {
    assert_eq!(hit_handle(105.0, 100.0, 300.0, Handle::End), Some(Handle::Start));
    assert_eq!(hit_handle(94.5, 100.0, 300.0, Handle::End), Some(Handle::Start));
    assert_eq!(hit_handle(306.5, 100.0, 300.0, Handle::Start), None);
    assert_eq!(hit_handle(296.0, 100.0, 300.0, Handle::Start), Some(Handle::End));
    assert_eq!(hit_handle(200.0, 100.0, 300.0, Handle::Start), None);
}

#[test]
fn close_handles_grab_the_nearer_one_and_ties_use_the_preference() {
    assert_eq!(hit_handle(101.0, 100.0, 104.0, Handle::End), Some(Handle::Start));
    assert_eq!(hit_handle(103.5, 100.0, 104.0, Handle::Start), Some(Handle::End));
    assert_eq!(hit_handle(102.0, 100.0, 100.0, Handle::Start), Some(Handle::Start));
    assert_eq!(hit_handle(102.0, 100.0, 100.0, Handle::End), Some(Handle::End));
}

fn wheel(unit: MouseWheelUnit, x: f32, y: f32, modifiers: Modifiers) -> Event {
    Event::MouseWheel { unit, delta: vec2(x, y), phase: TouchPhase::Move, source: MouseWheelSource::Unknown, modifiers }
}

#[test]
fn one_wheel_notch_is_one_zoom_step() {
    let up = wheel(MouseWheelUnit::Line, 0.0, 1.0, Modifiers::NONE);
    assert_eq!(wheel_notches(std::slice::from_ref(&up)), (1.0, 0.0));
    assert_eq!(zoom_factor(1.0), 1.25);
    assert_eq!(zoom_factor(-1.0), 0.8);
    assert_eq!(zoom_factor(2.0), 1.5625);
    // Two notches in one frame, and a precise device reporting points.
    let points = wheel(MouseWheelUnit::Point, 0.0, -40.0, Modifiers::NONE);
    assert_eq!(wheel_notches(&[up.clone(), up.clone()]), (2.0, 0.0));
    assert_eq!(wheel_notches(&[points]), (-1.0, 0.0));
}

#[test]
fn shift_wheel_and_horizontal_wheel_scroll() {
    // Wheel up (away from the user) with Shift goes back towards the start.
    assert_eq!(wheel_notches(&[wheel(MouseWheelUnit::Line, 0.0, 1.0, Modifiers::SHIFT)]), (0.0, -1.0));
    assert_eq!(wheel_notches(&[wheel(MouseWheelUnit::Line, 0.0, -2.0, Modifiers::SHIFT)]), (0.0, 2.0));
    assert_eq!(wheel_notches(&[wheel(MouseWheelUnit::Line, -1.0, 0.0, Modifiers::NONE)]), (0.0, 1.0));
    assert_eq!(wheel_notches(&[Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }]), (0.0, 0.0));
}

#[test]
fn arrows_nudge_one_frame_or_ten_ms() {
    assert_eq!(edit_key(Key::ArrowLeft, Modifiers::NONE, false, 48_000), Some(Edit::Nudge(-1)));
    assert_eq!(edit_key(Key::ArrowRight, Modifiers::NONE, true, 48_000), Some(Edit::Nudge(1)));
    assert_eq!(edit_key(Key::ArrowRight, Modifiers::SHIFT, false, 48_000), Some(Edit::Nudge(480)));
    assert_eq!(edit_key(Key::ArrowLeft, Modifiers::SHIFT, false, 44_100), Some(Edit::Nudge(-441)));
    assert_eq!(edit_key(Key::ArrowLeft, Modifiers::CTRL, false, 48_000), None);
}

#[test]
fn zero_fits_the_view() {
    assert_eq!(edit_key(Key::Num0, Modifiers::NONE, false, 48_000), Some(Edit::Fit));
    assert_eq!(edit_key(Key::Num0, Modifiers::NONE, true, 48_000), None);
    assert_eq!(edit_key(Key::Num0, Modifiers::COMMAND, false, 48_000), None);
    assert_eq!(edit_key(Key::Space, Modifiers::NONE, false, 48_000), None);
}

#[test]
fn playback_starts_at_the_cursor_only_inside_the_selection() {
    assert_eq!(play_start(500, 100, 1000, false), 500);
    assert_eq!(play_start(100, 100, 1000, false), 100);
    assert_eq!(play_start(1000, 100, 1000, false), 100);
    assert_eq!(play_start(50, 100, 1000, false), 100);
}

#[test]
fn reverse_starts_at_the_cursor_or_at_the_end_handle() {
    assert_eq!(play_start(500, 100, 1000, true), 500);
    assert_eq!(play_start(101, 100, 1000, true), 101);
    assert_eq!(play_start(1000, 100, 1000, true), 1000);
    assert_eq!(play_start(100, 100, 1000, true), 1000);
    assert_eq!(play_start(50, 100, 1000, true), 1000);
    assert_eq!(play_start(1500, 100, 1000, true), 1000);
}

#[test]
fn play_errors_use_the_spec_text() {
    assert_eq!(
        play_error_text("No se pudo iniciar la reproducción (el dispositivo de salida no responde)."),
        "No se pudo reproducir: el dispositivo de salida no responde"
    );
    assert_eq!(play_error_text("GetBuffer: error"), "No se pudo reproducir: GetBuffer: error");
}

/// A mono take whose frame `i` holds `i / 100_000`.
fn ramp_session(dir: &Path, frames: u32) -> Session {
    let path = dir.join("ramp.wav");
    let mut w = WavWriter::create(&path, &WavFormat::float32_extensible(48_000, 1)).unwrap();
    let bytes: Vec<u8> = (0..frames).flat_map(|i| (i as f32 / 100_000.0).to_le_bytes()).collect();
    w.write_audio(&bytes).unwrap();
    w.finish().unwrap();
    Session::open(&path, false).unwrap()
}

#[test]
fn far_views_use_peaks_near_views_read_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = ramp_session(dir.path(), 30_000);
    // 300 frames per column: the peak cache.
    let Lanes::Columns(far) = lanes_for(&mut s, (0, 30_000, 100)) else { panic!("expected columns") };
    assert_eq!(far[0].len(), 100);
    // 10 frames per column: exact min/max read from the file.
    let Lanes::Columns(near) = lanes_for(&mut s, (1_000, 2_000, 100)) else { panic!("expected columns") };
    assert_eq!(near[0][0], (0.01, 0.01009));
    assert_eq!(near[0][99], (0.0199, 0.01999));
    // Fewer frames than columns: single samples, one past the right edge.
    let Lanes::Samples(samples) = lanes_for(&mut s, (29_950, 30_000, 100)) else { panic!("expected samples") };
    assert_eq!(samples[0].len(), 50); // the take ends at 30_000
    let Lanes::Samples(samples) = lanes_for(&mut s, (10, 20, 100)) else { panic!("expected samples") };
    assert_eq!(samples[0].len(), 11);
    assert_eq!(samples[0][0], 0.0001);
    s.close().unwrap();
}

/// Runs the editor in a headless 800 × 300 window, one egui pass per call.
struct Harness {
    ctx: Context,
    time: f64,
    /// The waveform area of the last pass, tab row included.
    area: Rect,
    /// Every event the editor reported, in order.
    events: Vec<TakeEvent>,
}

impl Harness {
    fn new(editor: &mut TakeEditor) -> Harness {
        let mut h = Harness { ctx: Context::default(), time: 0.0, area: Rect::NOTHING, events: Vec::new() };
        // Everything lands at once, so a test can aim at what it just asked for.
        h.ctx.all_styles_mut(|s| s.animation_time = 0.0);
        h.pass(editor, vec![]);
        h
    }

    /// One pass with `events`; returns every text painted.
    fn pass(&mut self, editor: &mut TakeEditor, events: Vec<Event>) -> Vec<String> {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 300.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let mut area = Rect::NOTHING;
        let mut event = None;
        let output = self.ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| {
                area = ui.available_rect_before_wrap();
                event = editor.show(ui, true);
            });
        });
        self.area = area;
        self.events.extend(event);
        let painted = texts(&output.shapes);
        output.drop_without_applying_deltas();
        painted
    }

    /// Where `frame` is drawn in the current view, 20 points into the waveform below the
    /// tab row.
    fn at(&self, editor: &TakeEditor, frame: f64) -> Pos2 {
        pos2(self.area.left() + editor.session.view.x_at(frame, self.area.width()), self.area.top() + TAB_H + 20.0)
    }

    /// The middle of the drag tab, where it is visible.
    fn tab(&self, editor: &TakeEditor) -> Pos2 {
        let sel = &editor.session.selection;
        let (a, b) = (self.at(editor, sel.start as f64).x, self.at(editor, sel.end as f64).x);
        let wave = Rect::from_x_y_ranges(self.area.x_range(), self.area.y_range());
        tab_rect((a, b), wave).expect("the selection is visible").center()
    }

    /// The frame under `pos` in the current view.
    fn frame(&self, editor: &TakeEditor, pos: Pos2) -> f64 {
        editor.session.view.frame_at(pos.x - self.area.left(), self.area.width())
    }

    /// Presses at `from`, drags through `to` and releases there; returns the texts painted
    /// while the pointer was at `to`.
    fn drag(&mut self, editor: &mut TakeEditor, from: Pos2, to: Pos2) -> Vec<String> {
        self.pass(editor, vec![Event::PointerMoved(from)]);
        self.pass(editor, vec![button(from, true)]);
        self.pass(editor, vec![Event::PointerMoved(from + (to - from) / 2.0)]);
        let during = self.pass(editor, vec![Event::PointerMoved(to)]);
        self.pass(editor, vec![button(to, false)]);
        during
    }

    fn click(&mut self, editor: &mut TakeEditor, at: Pos2) {
        self.pass(editor, vec![Event::PointerMoved(at)]);
        self.pass(editor, vec![button(at, true)]);
        self.pass(editor, vec![button(at, false)]);
    }
}

fn texts(shapes: &[ClippedShape]) -> Vec<String> {
    fn walk(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Text(t) => out.push(t.galley.text().to_owned()),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|s| walk(&s.shape, &mut out));
    out
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn line_wheel(notches: f32, modifiers: Modifiers) -> Event {
    wheel(MouseWheelUnit::Line, 0.0, notches, modifiers)
}

#[test]
fn the_wheel_zooms_around_the_pointer_and_shift_wheel_scrolls() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let p = h.at(&ed, 20_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(p), line_wheel(1.0, Modifiers::NONE)]);
    let v = ed.session.view;
    assert!((v.to - v.from - 24_000.0).abs() < 1e-6, "{v:?}");
    assert!((h.frame(&ed, p) - 20_000.0).abs() < 0.01, "{v:?}");
    // One notch away from the user with Shift: a tenth of the visible width back.
    h.pass(&mut ed, vec![line_wheel(1.0, Modifiers::SHIFT)]);
    assert!((ed.session.view.from - (v.from - 2_400.0)).abs() < 0.01, "{:?}", ed.session.view);
    assert!(!ed.session.view.is_fitted());
    h.pass(&mut ed, vec![key(Key::Num0, Modifiers::NONE)]);
    assert!(ed.session.view.is_fitted());
    ed.close().unwrap();
}

#[test]
fn arrows_move_the_active_handle() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    h.pass(&mut ed, vec![key(Key::ArrowRight, Modifiers::NONE)]);
    h.pass(&mut ed, vec![key(Key::ArrowRight, Modifiers::SHIFT)]);
    assert_eq!(ed.session.selection.start, 481);
    ed.close().unwrap();
}

#[test]
fn dragging_a_handle_moves_it_to_the_frame_under_the_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let from = h.at(&ed, 30_000.0) - vec2(2.0, 0.0);
    let to = h.at(&ed, 15_000.0);
    h.drag(&mut ed, from, to);
    assert_eq!(ed.session.selection.end, 15_000);
    assert_eq!(ed.session.selection.active, Handle::End);
    assert_eq!(ed.drag, None);
    ed.close().unwrap();
}

#[test]
fn clicks_place_the_cursor_or_activate_a_near_handle() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let quarter = h.at(&ed, 7_500.0);
    h.click(&mut ed, quarter);
    assert_eq!(ed.session.playhead, 7_500);
    assert_eq!(ed.session.selection.active, Handle::Start);
    let near_end = h.at(&ed, 30_000.0) - vec2(3.0, 0.0);
    h.click(&mut ed, near_end);
    assert_eq!(ed.session.selection.active, Handle::End);
    assert_eq!((ed.session.playhead, ed.session.selection.end), (7_500, 30_000));
    ed.close().unwrap();
}

#[test]
fn at_full_zoom_samples_are_drawn_and_a_drag_shows_the_sample_number() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let center = h.at(&ed, 15_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(center), line_wheel(40.0, Modifiers::NONE)]);
    let px_per_frame = 1.0 / ed.session.view.frames_per_px(h.area.width());
    assert!((px_per_frame - crate::view::MAX_PX_PER_FRAME).abs() < 1e-6);
    // Scroll to the end, where the end handle is: 3.92 frames (a tenth of the view) per notch.
    h.pass(&mut ed, vec![line_wheel(-5000.0, Modifiers::SHIFT)]);
    assert_eq!(ed.session.view.to, 30_000.0);
    assert!(matches!(ed.lanes, Some(LaneCache { lanes: Lanes::Samples(_), .. })));
    // 22 points left of the end at 20 points per frame: frame 29_998.9, rounded.
    let end = h.at(&ed, 30_000.0) - vec2(2.0, 0.0);
    let during = h.drag(&mut ed, end, end - vec2(20.0, 0.0));
    assert!(during.iter().any(|t| t == "00:00.624 · muestra 29999"), "{during:?}");
    assert_eq!(ed.session.selection.end, 29_999);
    ed.close().unwrap();
}

#[test]
fn tab_spans_the_selection() {
    let wave = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 300.0));
    assert_eq!(tab_rect((100.0, 400.0), wave), Some(Rect::from_min_max(pos2(100.0, 0.0), pos2(400.0, 24.0))));
}

#[test]
fn a_narrow_selection_gets_a_64_px_tab_centered_on_it() {
    let wave = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 300.0));
    assert_eq!(tab_rect((300.0, 310.0), wave), Some(Rect::from_min_max(pos2(273.0, 0.0), pos2(337.0, 24.0))));
}

#[test]
fn the_tab_is_clipped_to_the_view_and_vanishes_off_screen() {
    let wave = Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 300.0));
    assert_eq!(tab_rect((-500.0, 200.0), wave), Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 24.0))));
    assert_eq!(tab_rect((900.0, 1_200.0), wave), None);
}

#[test]
fn zones_prefer_tab_then_handles_then_wave() {
    let tab = Some(Rect::from_min_max(pos2(100.0, 0.0), pos2(400.0, 24.0)));
    assert_eq!(zone_at(pos2(250.0, 10.0), tab, 100.0, 400.0, Handle::Start), Zone::Tab);
    assert_eq!(zone_at(pos2(103.0, 150.0), tab, 100.0, 400.0, Handle::Start), Zone::Handle(Handle::Start));
    assert_eq!(zone_at(pos2(250.0, 150.0), tab, 100.0, 400.0, Handle::Start), Zone::Wave);
}

fn near(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= 1
}

#[test]
fn sweeping_right_selects_from_press_to_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let (from, to) = (h.at(&ed, 5_000.0), h.at(&ed, 15_000.0));
    h.drag(&mut ed, from, to);
    let sel = ed.session.selection;
    assert!(near(sel.start, 5_000) && near(sel.end, 15_000), "{sel:?}");
    assert_eq!(sel.active, Handle::End);
    ed.close().unwrap();
}

#[test]
fn sweeping_left_normalizes_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let (from, to) = (h.at(&ed, 15_000.0), h.at(&ed, 5_000.0));
    h.drag(&mut ed, from, to);
    let sel = ed.session.selection;
    assert!(near(sel.start, 5_000) && near(sel.end, 15_000), "{sel:?}");
    assert_eq!(sel.active, Handle::Start);
    ed.close().unwrap();
}

#[test]
fn a_click_on_the_wave_moves_the_playhead_and_keeps_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let before = ed.session.selection;
    let at = h.at(&ed, 10_000.0);
    h.click(&mut ed, at);
    assert!(near(ed.session.playhead, 10_000));
    assert_eq!(ed.session.selection, before);
    ed.close().unwrap();
}

#[test]
fn a_click_on_the_tab_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let (from, to) = (h.at(&ed, 5_000.0), h.at(&ed, 15_000.0));
    h.drag(&mut ed, from, to);
    let (sel, playhead) = (ed.session.selection, ed.session.playhead);
    let tab = h.tab(&ed);
    h.click(&mut ed, tab);
    assert_eq!((ed.session.selection, ed.session.playhead), (sel, playhead));
    assert!(h.events.is_empty());
    ed.close().unwrap();
}

#[test]
fn dragging_the_tab_reports_the_whole_selection_even_when_zoomed() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    let (from, to) = (h.at(&ed, 5_000.0), h.at(&ed, 25_000.0));
    h.drag(&mut ed, from, to);
    let sel = ed.session.selection;
    let middle = h.at(&ed, 15_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(middle), line_wheel(8.0, Modifiers::NONE)]);
    assert!(ed.session.view.from > sel.start as f64 && ed.session.view.to < sel.end as f64);
    let tab = h.tab(&ed);
    h.drag(&mut ed, tab, tab + vec2(0.0, 120.0));
    assert_eq!(h.events, vec![TakeEvent::DragOut { start: sel.start, end: sel.end }]);
    assert_eq!(ed.session.selection, sel);
    ed.close().unwrap();
}

#[test]
fn the_tab_shows_the_selection_duration() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    assert!(h.pass(&mut ed, vec![]).iter().any(|t| t == "00:00.625"));
    h.pass(&mut ed, vec![key(Key::ArrowRight, Modifiers::NONE)]);
    h.pass(&mut ed, vec![key(Key::ArrowRight, Modifiers::SHIFT)]);
    // 30_000 − 481 frames at 48 kHz.
    assert!(h.pass(&mut ed, vec![]).iter().any(|t| t == "00:00.614"));
    ed.close().unwrap();
}

#[test]
fn mark_saved_makes_the_take_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    assert!(!ed.saved());
    ed.mark_saved();
    assert!(ed.saved());
    ed.close().unwrap();
}

#[test]
fn samples_take_the_source_the_take_was_recorded_from() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    assert_eq!(ed.sample_prefix(), "Teeth");
    ed.set_sample_prefix("Zen");
    assert_eq!(ed.sample_prefix(), "Zen");
    ed.close().unwrap();
}

#[test]
fn the_view_drawn_glides_to_the_view_asked_for() {
    if !theme::animations_enabled() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    let mut h = Harness::new(&mut ed);
    h.ctx.all_styles_mut(|s| s.animation_time = 1.0 / 12.0);
    let p = h.at(&ed, 20_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(p), line_wheel(1.0, Modifiers::NONE)]);
    assert_ne!(ed.shown, ed.session.view, "it starts on its way");
    for _ in 0..30 {
        h.pass(&mut ed, vec![]);
    }
    assert_eq!(ed.shown, ed.session.view, "and it lands there");
    ed.close().unwrap();
}

#[test]
fn the_cursor_moves_with_the_clock_between_the_steps_of_the_player() {
    // The player reports every 10 ms, in steps of 480 frames; the window paints at 60 Hz.
    let (rate, dt) = (48_000.0, 1.0 / 60.0);
    let (mut shown, mut last) = (0.0, 0.0);
    for paint in 1..240 {
        let now = f64::from(paint) * f64::from(dt);
        let reported = (now * 100.0).floor() as u64 * 480;
        shown = follow_playhead(shown, reported, dt, rate);
        assert!(shown > last, "paint {paint}: the cursor stopped or went back");
        assert!((shown - now * 48_000.0).abs() < 480.0, "paint {paint}: {shown} is far from the sound");
        last = shown;
    }
}

#[test]
fn the_window_paints_as_often_as_the_cursor_moves_a_pixel() {
    // A long take fitted: more than 33 ms per pixel, so 30 times a second is plenty.
    assert_eq!(cursor_pace(4_800.0, 48_000), Duration::from_millis(33));
    // 12 ms per pixel.
    assert_eq!(cursor_pace(576.0, 48_000), Duration::from_millis(12));
    // Zoomed in, the screen's own rate sets the limit.
    assert_eq!(cursor_pace(1.0, 48_000), Duration::from_millis(8));
}

#[test]
fn the_cursor_follows_a_jump_at_once() {
    // The loop went back to its start.
    assert_eq!(follow_playhead(90_000.0, 10_000, 1.0 / 60.0, 48_000.0), 10_000.0);
}

#[test]
fn the_reversed_cursor_walks_back_and_follows_the_loop_at_once() {
    let (velocity, dt) = (-40_363.0, 1.0 / 60.0);
    let shown = follow_playhead(100_000.0, 100_000, dt, velocity);
    assert!(shown < 100_000.0);
    assert_eq!(follow_playhead(10_000.0, 90_000, dt, velocity), 90_000.0);
}

#[test]
fn a_new_take_settles_into_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    Harness::new(&mut ed);
    assert_eq!(ed.reveal, 1.0, "at once without animation time");
    ed.close().unwrap();
    if !theme::animations_enabled() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    let mut h = Harness::new(&mut ed);
    ed.reveal = 0.0;
    h.ctx.all_styles_mut(|s| s.animation_time = 1.0 / 12.0);
    h.pass(&mut ed, vec![]);
    assert!(ed.reveal > 0.0 && ed.reveal < 1.0, "{}", ed.reveal);
    for _ in 0..60 {
        h.pass(&mut ed, vec![]);
    }
    assert_eq!(ed.reveal, 1.0);
    ed.close().unwrap();
}

#[test]
fn rewind_puts_the_cursor_on_the_start_handle() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.session.selection.set_start(5_000);
    ed.session.playhead = 20_000;
    ed.rewind().unwrap();
    assert_eq!(ed.session.playhead, 5_000);
    assert!(!ed.playing());
    ed.close().unwrap();
}

#[test]
fn rewind_in_reverse_goes_to_where_it_starts_to_sound() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.session.selection.set_start(5_000);
    ed.session.selection.set_end(20_000);
    ed.session.playhead = 10_000;
    ed.set_variant(Variant { semitones: 0, reversed: true }).unwrap();
    ed.rewind().unwrap();
    assert_eq!(ed.session.playhead, 20_000);
    assert!(!ed.playing());
    ed.close().unwrap();
}

#[test]
fn a_variant_changed_as_listening_ends_does_not_sound_again() {
    let dir = tempfile::tempdir().unwrap();
    let session = ramp_session(dir.path(), 1_000);
    let path = session.take.path.clone();
    let mut ed = TakeEditor::new(session);
    // An empty range is finished from the start and needs no device.
    let player = Player::start(&path, 7, 7, 7, false, Variant::default()).unwrap();
    ed.playback = Some(Playback { player, origin: 7 });
    ed.session.playhead = 300;
    ed.set_variant(Variant { semitones: 0, reversed: true }).unwrap();
    assert!(ed.poll().is_none());
    assert!(!ed.playing());
    assert_eq!(ed.session.playhead, 7);
    ed.close().unwrap();
}

#[test]
fn a_variant_set_while_paused_only_changes_the_variant() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.session.playhead = 12_345;
    let variant = Variant { semitones: 5, reversed: true };
    ed.set_variant(variant).unwrap();
    assert_eq!(ed.variant(), variant);
    assert!(!ed.playing());
    assert_eq!(ed.session.playhead, 12_345);
    ed.close().unwrap();
}

/// A mono take of `frames` frames of silence.
fn silent_session(dir: &Path, frames: u64) -> Session {
    let path = dir.join("silence.wav");
    let mut w = WavWriter::create(&path, &WavFormat::float32_extensible(48_000, 1)).unwrap();
    w.write_silence(frames).unwrap();
    w.finish().unwrap();
    Session::open(&path, false).unwrap()
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn changing_the_variant_while_it_sounds_goes_on_from_there() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(silent_session(dir.path(), 96_000));
    ed.session.playhead = 48_000;
    ed.toggle_play().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    // The same variant does not restart: a restart stops first, which writes the cursor.
    ed.set_variant(Variant::default()).unwrap();
    assert!(ed.playing());
    assert_eq!(ed.session.playhead, 48_000);
    assert_eq!(ed.playback.as_ref().map(|pb| pb.origin), Some(48_000));

    let heard = ed.playback.as_ref().unwrap().player.position();
    assert!(heard > 48_000, "it has been sounding for 100 ms: {heard}");
    let reversed = Variant { semitones: -3, reversed: true };
    ed.set_variant(reversed).unwrap();
    assert!(ed.playing());
    assert_eq!(ed.variant(), reversed);
    assert_eq!(ed.playback.as_ref().map(|pb| pb.origin), Some(48_000), "the origin is where listening began");
    let from = ed.session.playhead;
    assert!(from >= heard && from - heard < 24_000, "it goes on from where it was: {heard} -> {from}");
    std::thread::sleep(Duration::from_millis(200));
    assert!(ed.poll().is_none());
    assert!(ed.playing());
    assert!(ed.session.playhead < from, "the cursor walks back: {from} -> {}", ed.session.playhead);
    ed.close().unwrap();
}

#[test]
fn the_loop_starts_off_and_toggles() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    assert!(!ed.looping());
    ed.toggle_loop();
    assert!(ed.looping());
    ed.toggle_loop();
    assert!(!ed.looping());
    ed.close().unwrap();
}

const NOW: LocalTime = LocalTime { year: 2026, month: 11, day: 1, hour: 14, minute: 32, second: 5 };

#[test]
fn download_writes_the_selection_into_the_folder_once() {
    let dir = tempfile::tempdir().unwrap();
    let samples = dir.path().join("Samples");
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.set_sample_prefix("Zen");
    ed.session.selection.set_start(10_000);
    let path = ed.download(&samples, &NOW).unwrap();
    assert_eq!(path, samples.join("Zen 2026-11-01 14-32-05.wav"));
    assert_eq!(crate::wav::WavReader::open(&path).unwrap().frames(), 20_000);
    assert!(!ed.saved(), "a part of the take is not the take");
    let later = LocalTime { second: 9, ..NOW };
    assert_eq!(ed.download(&samples, &later).unwrap(), path);
    ed.close().unwrap();
}

#[test]
fn downloading_the_whole_take_saves_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    ed.download(&dir.path().join("Samples"), &NOW).unwrap();
    assert!(ed.saved());
    ed.close().unwrap();
}

#[test]
fn a_new_take_starts_without_a_variant() {
    let dir = tempfile::tempdir().unwrap();
    let ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    assert_eq!(ed.variant(), Variant::default());
    ed.close().unwrap();
}

#[test]
fn the_sample_is_the_take_only_when_whole_and_faithful() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    assert!(ed.sample_is_the_take());
    ed.variant = Variant { semitones: -3, reversed: false };
    assert!(!ed.sample_is_the_take(), "pitched");
    ed.variant = Variant { semitones: 0, reversed: true };
    assert!(!ed.sample_is_the_take(), "reversed");
    ed.variant = Variant::default();
    ed.session.selection.set_start(100);
    assert!(!ed.sample_is_the_take(), "a part");
    ed.close().unwrap();
}

#[test]
fn downloading_the_whole_take_with_a_variant_does_not_save_it() {
    for v in [Variant { semitones: -3, reversed: false }, Variant { semitones: 0, reversed: true }] {
        let dir = tempfile::tempdir().unwrap();
        let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
        ed.variant = v;
        ed.download(&dir.path().join("Samples"), &NOW).unwrap();
        assert!(!ed.saved(), "{v:?}");
        let facts = TakeFacts { saved: ed.saved(), playing: false };
        assert_eq!(gate(Destructive::Discard, Some(facts), true), Gate::Ask);
        ed.close().unwrap();
    }
}

#[test]
fn a_variant_does_not_unsave_a_saved_take() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 1_000));
    ed.mark_saved();
    ed.variant = Variant { semitones: 5, reversed: true };
    ed.download(&dir.path().join("Samples"), &NOW).unwrap();
    assert!(ed.saved());
    ed.close().unwrap();
}

#[test]
fn saving_with_a_variant_writes_the_faithful_audio() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.variant = Variant { semitones: -3, reversed: true };
    ed.session.selection.set_start(10_000);
    let dest = dir.path().join("saved.wav");
    ed.save_to(&dest).unwrap();
    let mut got = WavReader::open(&dest).unwrap();
    assert_eq!(got.format().sample_rate(), 48_000);
    // The bytes of `data` are those of frames [10_000, 30_000) of the take, in their order.
    let mut original = WavReader::open(&dir.path().join("ramp.wav")).unwrap();
    assert_eq!(got.read_frames(0, 20_000).unwrap(), original.read_frames(10_000, 20_000).unwrap());
    assert_eq!(got.frames(), 20_000);
    assert_eq!(ed.variant(), Variant { semitones: -3, reversed: true });
    ed.close().unwrap();
}

#[test]
fn the_tab_shows_the_length_of_the_file_that_will_come_out() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 288_000));
    let mut h = Harness::new(&mut ed);
    assert!(h.pass(&mut ed, vec![]).iter().any(|t| t == "00:06.000"));
    let before = ed.time_text();
    ed.set_variant(Variant { semitones: -3, reversed: false }).unwrap();
    // 288_000 frames at 40_363 Hz.
    assert!(h.pass(&mut ed, vec![]).iter().any(|t| t == "00:07.135"));
    assert_eq!(ed.time_text(), before, "the bar keeps the time of the take");
    ed.close().unwrap();
}

#[test]
fn the_window_paints_at_the_pace_of_the_variant() {
    let dir = tempfile::tempdir().unwrap();
    // Fitted, 10 s is some 13 ms per pixel at 48 kHz, and the screen's 8 ms at 96 kHz.
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 480_000));
    let mut h = Harness::new(&mut ed);
    let faithful = ed.paint_pace();
    assert_eq!(faithful, cursor_pace(ed.shown.frames_per_px(ed.wave_width), 48_000));
    ed.set_variant(Variant { semitones: 12, reversed: false }).unwrap();
    h.pass(&mut ed, vec![]);
    assert_eq!(ed.paint_pace(), cursor_pace(ed.shown.frames_per_px(ed.wave_width), 96_000));
    assert!(ed.paint_pace() < faithful);
    ed.close().unwrap();
}

#[test]
fn trimming_goes_through_the_editor() {
    let dir = tempfile::tempdir().unwrap();
    // The ramp reaches -60 dBFS at frame 100.
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    assert_eq!(ed.trim_silence(), Ok(true));
    assert_eq!((ed.session.selection.start, ed.session.selection.end), (100, 30_000));
    assert!(!ed.sample_is_the_take(), "a part of the take is not the take");
    ed.close().unwrap();
}

#[test]
fn a_selection_of_only_silence_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.session.selection.set_end(50);
    assert_eq!(ed.trim_silence(), Ok(false));
    assert_eq!((ed.session.selection.start, ed.session.selection.end), (0, 50));
    ed.close().unwrap();
}

#[test]
fn snapping_keeps_the_whole_take_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    assert_eq!(ed.snap_to_zero(), Ok(()));
    assert!(ed.sample_is_the_take());
    ed.close().unwrap();
}

#[test]
fn a_failed_read_says_what_failed() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 30_000));
    ed.session.selection.set_start(500);
    ed.session.selection.set_end(20_000);
    ed.session.reader.release();
    let starts = |r: Result<(), String>| r.is_err_and(|e| e.starts_with("No se pudo leer la toma: "));
    assert!(starts(ed.snap_to_zero()));
    assert!(starts(ed.trim_silence().map(|_| ())));
    ed.close().unwrap();
}

#[test]
fn the_editor_knows_the_file_of_its_take() {
    let dir = tempfile::tempdir().unwrap();
    let session = ramp_session(dir.path(), 1_000);
    let path = session.take.path.clone();
    let ed = TakeEditor::new(session);
    assert!(ed.holds_file(&path));
    assert!(!ed.holds_file(&dir.path().join("otro.wav")));
    ed.close().unwrap();
}

/// A take of 300 000 frames zoomed in around frame 200 000: 53 000 frames across or so.
fn zoomed_in(ed: &mut TakeEditor, h: &mut Harness) {
    let aim = h.at(ed, 200_000.0);
    h.pass(ed, vec![Event::PointerMoved(aim), line_wheel(8.0, Modifiers::NONE)]);
    h.pass(ed, vec![]);
}

/// The middle of the overview bar under `frame` of a take of 300 000 frames.
fn on_bar(h: &Harness, frame: f64) -> Pos2 {
    pos2(h.area.left() + h.area.width() * (frame / 300_000.0) as f32, h.area.bottom() - OVERVIEW_H / 2.0)
}

#[test]
fn pressing_the_overview_brings_that_part_into_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    ed.session.selection.set_start(5_000);
    h.pass(&mut ed, vec![]);
    zoomed_in(&mut ed, &mut h);
    let (selection, playhead) = (ed.session.selection, ed.session.playhead);
    let span = ed.session.view.to - ed.session.view.from;
    let at = on_bar(&h, 60_000.0);
    h.click(&mut ed, at);
    let v = ed.session.view;
    assert!(((v.from + v.to) / 2.0 - 60_000.0).abs() < 1.0, "{v:?}");
    assert!(((v.to - v.from) - span).abs() < 1e-6, "the zoom stays");
    assert_eq!(ed.session.selection, selection, "the cuts stay");
    assert_eq!(ed.session.playhead, playhead, "the cursor stays");
    ed.close().unwrap();
}

#[test]
fn dragging_the_thumb_moves_the_view_and_keeps_the_cuts() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    ed.session.selection.set_start(5_000);
    h.pass(&mut ed, vec![]);
    zoomed_in(&mut ed, &mut h);
    let (selection, playhead) = (ed.session.selection, ed.session.playhead);
    let before = ed.session.view;
    // Off the centre of the thumb: it stays held where it was pressed, without a jump.
    let press = on_bar(&h, before.from + (before.to - before.from) * 0.8);
    h.pass(&mut ed, vec![Event::PointerMoved(press)]);
    h.pass(&mut ed, vec![button(press, true)]);
    let held = ed.session.view;
    assert!((held.from - before.from).abs() < 1.0, "{held:?}");
    let to = press - vec2(100.0, 0.0);
    let frames = 100.0 / f64::from(h.area.width()) * 300_000.0;
    h.pass(&mut ed, vec![Event::PointerMoved(to)]);
    h.pass(&mut ed, vec![button(to, false)]);
    let moved = ed.session.view;
    assert!((before.from - moved.from - frames).abs() < 1.0, "{before:?} {moved:?}");
    assert_eq!(ed.session.selection, selection, "the cuts stay");
    assert_eq!(ed.session.playhead, playhead, "the cursor stays");
    // Let go, the pointer moves the view no more.
    h.pass(&mut ed, vec![Event::PointerMoved(to - vec2(50.0, 0.0))]);
    assert_eq!(ed.session.view, moved);
    ed.close().unwrap();
}

#[test]
fn dragging_past_the_ends_of_the_overview_stops_at_the_ends_of_the_take() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    zoomed_in(&mut ed, &mut h);
    let span = ed.session.view.to - ed.session.view.from;
    let press = on_bar(&h, 200_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(press)]);
    h.pass(&mut ed, vec![button(press, true)]);
    h.pass(&mut ed, vec![Event::PointerMoved(pos2(h.area.left() - 50.0, press.y))]);
    assert_eq!(ed.session.view.from, 0.0);
    assert!((ed.session.view.to - span).abs() < 1e-6);
    h.pass(&mut ed, vec![Event::PointerMoved(pos2(h.area.right() + 50.0, press.y))]);
    assert_eq!(ed.session.view.to, 300_000.0);
    h.pass(&mut ed, vec![button(pos2(h.area.right() + 50.0, press.y), false)]);
    ed.close().unwrap();
}

#[test]
fn the_strip_between_the_lanes_and_the_bar_counts_as_the_bar() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    zoomed_in(&mut ed, &mut h);
    let selection = ed.session.selection;
    let at = pos2(on_bar(&h, 60_000.0).x, h.area.bottom() - OVERVIEW_H - OVERVIEW_GAP / 2.0);
    h.click(&mut ed, at);
    let v = ed.session.view;
    assert!(((v.from + v.to) / 2.0 - 60_000.0).abs() < 1.0, "{v:?}");
    assert_eq!(ed.session.selection, selection);
    ed.close().unwrap();
}

#[test]
fn a_press_on_the_lanes_of_a_zoomed_take_still_marks_and_places() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    zoomed_in(&mut ed, &mut h);
    let view = ed.session.view;
    let at = h.at(&ed, 200_000.0);
    h.click(&mut ed, at);
    assert!(near(ed.session.playhead, 200_000));
    assert_eq!(ed.session.view, view);
    let (from, to) = (h.at(&ed, 195_000.0), h.at(&ed, 205_000.0));
    h.drag(&mut ed, from, to);
    let sel = ed.session.selection;
    assert!(near(sel.start, 195_000) && near(sel.end, 205_000), "{sel:?}");
    ed.close().unwrap();
}

#[test]
fn without_zoom_there_is_no_bar_to_press() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    let view = ed.session.view;
    let at = on_bar(&h, 60_000.0);
    h.click(&mut ed, at);
    assert_eq!(ed.session.view, view);
    assert!(near(ed.session.playhead, 60_000), "{}", ed.session.playhead);
    ed.close().unwrap();
}

#[test]
fn a_click_so_quick_that_press_and_release_arrive_together_still_moves_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut ed = TakeEditor::new(ramp_session(dir.path(), 300_000));
    let mut h = Harness::new(&mut ed);
    zoomed_in(&mut ed, &mut h);
    let at = on_bar(&h, 60_000.0);
    h.pass(&mut ed, vec![Event::PointerMoved(at)]);
    h.pass(&mut ed, vec![button(at, true), button(at, false)]);
    let v = ed.session.view;
    assert!(((v.from + v.to) / 2.0 - 60_000.0).abs() < 1.0, "{v:?}");
    // The next press on the thumb starts over: it does not keep this one's grip.
    let press = on_bar(&h, v.from + (v.to - v.from) * 0.8);
    h.pass(&mut ed, vec![Event::PointerMoved(press)]);
    h.pass(&mut ed, vec![button(press, true)]);
    assert!((ed.session.view.from - v.from).abs() < 1.0, "{:?}", ed.session.view);
    h.pass(&mut ed, vec![button(press, false)]);
    ed.close().unwrap();
}
