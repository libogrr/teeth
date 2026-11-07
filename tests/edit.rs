use teeth::selection::{Handle, Selection};
use teeth::text::{cuts_text, fmt_peak, fmt_time, info_line};
use teeth::view::{MAX_PX_PER_FRAME, View, glide, lane_bounds};
use teeth::wav::WavFormat;

#[test]
fn selection_clamps_and_keeps_one_sample() {
    let mut s = Selection::new(100);
    assert!(s.is_full());
    s.set_start(150);
    assert_eq!(s.start, 99);
    s.set_end(10);
    assert_eq!(s.end, 100); // no puede bajar de start + 1
    s.active = Handle::End;
    s.nudge(-1000);
    assert_eq!(s.end, 100);
    s.set_start(0);
    s.nudge(-1000);
    assert_eq!(s.end, 1);
    s.active = Handle::Start;
    s.nudge(-5);
    assert_eq!(s.start, 0);
    assert_eq!(s.frames(), 1);
}

#[test]
fn view_zoom_limits_and_anchor() {
    let mut v = View::new(48_000);
    v.zoom(1e9, 24_000.0, 800.0);
    assert!((v.frames_per_px(800.0) - 1.0 / MAX_PX_PER_FRAME).abs() < 1e-9);
    assert!((v.frame_at(400.0, 800.0) - 24_000.0).abs() < 1.0);
    v.zoom(1e-9, 24_000.0, 800.0);
    assert!(v.is_fitted());
}

#[test]
fn a_gliding_view_lands_exactly_on_its_target() {
    let shown = View::new(1_000_000);
    let mut target = shown;
    target.zoom(8.0, 250_000.0, 800.0);
    let mut v = shown;
    let mut steps = 0;
    while v != target {
        v = glide(v, target, 1.0 / 60.0, 800.0);
        steps += 1;
        assert!(steps < 60, "it never lands: {v:?}");
    }
    assert!(steps > 2, "it should take a few frames, took {steps}");
}

#[test]
fn a_gliding_view_keeps_the_frame_under_the_pointer() {
    let shown = View::new(1_000_000);
    let mut target = shown;
    target.zoom(4.0, 300_000.0, 800.0);
    let pointer = shown.x_at(300_000.0, 800.0);
    let mid = glide(shown, target, 1.0 / 60.0, 800.0);
    assert_ne!(mid, target);
    assert!((mid.frame_at(pointer, 800.0) - 300_000.0).abs() < 1.0, "{mid:?}");
}

#[test]
fn a_view_at_rest_does_not_move() {
    let v = View::new(48_000);
    assert_eq!(glide(v, v, 1.0 / 60.0, 800.0), v);
}

#[test]
fn view_scroll_stays_inside() {
    let mut v = View::new(10_000);
    v.zoom(10.0, 0.0, 1000.0);
    v.scroll_px(-500.0, 1000.0);
    assert_eq!(v.from, 0.0);
    v.scroll_px(1e9, 1000.0);
    assert_eq!(v.to, 10_000.0);
}

#[test]
fn fitted_view_follows_growing_total() {
    let mut v = View::new(100);
    v.set_total(5_000);
    assert_eq!((v.from, v.to), (0.0, 5_000.0));
}

#[test]
fn lanes_fit_eight_channels() {
    let l = lane_bounds(10.0, 300.0, 8);
    assert_eq!(l.len(), 8);
    assert_eq!(l[0].0, 10.0);
    assert!((l[7].1 - 310.0).abs() < 0.01);
    assert!(l.windows(2).all(|w| (w[1].0 - w[0].1 - 4.0).abs() < 0.01));
}

#[test]
fn texts() {
    assert_eq!(fmt_time(592_560, 48_000), "00:12.345");
    assert_eq!(fmt_time(48_000 * 3_725, 48_000), "1:02:05.000");
    assert_eq!(fmt_peak(0.5), "pico −6,0 dBFS");
    assert_eq!(fmt_peak(1.0), "pico 0,0 dBFS");
    assert_eq!(fmt_peak(2.0), "pico +6,0 dBFS");
    assert_eq!(fmt_peak(0.0), "pico −∞ dBFS");
    assert_eq!(fmt_peak(f32::NAN), "pico −∞ dBFS");
    assert_eq!(fmt_peak(-1.0), "pico −∞ dBFS");
    assert_eq!(
        info_line(&WavFormat::float32_extensible(48_000, 2), 0.5, 592_560),
        "48000 Hz · 32-bit float · 2 ch · pico −6,0 dBFS · 00:12.345"
    );
    assert_eq!(cuts_text(1), "Toma con 1 corte");
    assert_eq!(cuts_text(3), "Toma con 3 cortes");
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let mut v = View::new(1);
    v.zoom(2.0, 0.0, 0.0);
    v.scroll_px(10.0, 0.0);
    assert!(v.frames_per_px(0.0).is_finite());
    assert!(v.frame_at(5.0, 0.0).is_finite());
    assert!(v.x_at(0.5, 0.0).is_finite());
    v.zoom(f64::NAN, 0.0, 100.0);
    assert!(v.from.is_finite() && v.to.is_finite());
    assert!(lane_bounds(0.0, 100.0, 0).is_empty());
    assert_eq!(fmt_peak(0.9999), "pico 0,0 dBFS");
}

#[test]
fn small_scrolls_never_leave_the_take() {
    // En medio: desplazamiento exacto.
    let mut v = View::new(10_000);
    v.zoom(10.0, 0.0, 1000.0); // span 1000, from 0
    v.scroll_px(100.0, 1000.0); // 100 px = 100 frames
    assert_eq!((v.from, v.to), (100.0, 1100.0));

    // Cerca del final: se ajusta a total.
    v.from = 8_500.0;
    v.to = 9_500.0;
    v.scroll_px(800.0, 1000.0);
    assert_eq!((v.from, v.to), (9_000.0, 10_000.0));

    // Ajustada: no cambia.
    let mut f = View::new(10_000);
    f.scroll_px(10.0, 1000.0);
    f.scroll_px(-10.0, 1000.0);
    assert_eq!((f.from, f.to), (0.0, 10_000.0));
    assert!(f.is_fitted());
}

#[test]
fn centring_the_view_keeps_the_zoom_and_stays_inside_the_take() {
    let mut v = View::new(1_000);
    v.from = 100.0;
    v.to = 300.0;
    v.center_on(500.0);
    assert_eq!((v.from, v.to), (400.0, 600.0));
    v.center_on(50.0);
    assert_eq!((v.from, v.to), (0.0, 200.0));
    v.center_on(950.0);
    assert_eq!((v.from, v.to), (800.0, 1_000.0));
    v.center_on(f64::NAN);
    assert_eq!((v.from, v.to), (800.0, 1_000.0));
}
