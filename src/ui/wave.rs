//! Waveform area and drawing: one lane per channel, linear scale from −1 to +1; the trim
//! handles, the playback cursor, the cut markers and the bar that shows the visible part.
//!
//! The selection is drawn in white at 85 % and what lies outside it at 20 %, like Orc's
//! dimmed covers at rest.
//!
//! ±1.0 reaches `FULL_SCALE` of the lane's half height; the headroom above it shows
//! samples beyond full scale (possible in float) in red, clipped at the lane edge.

use super::theme;
use crate::peaks::PeakCache;
use crate::selection::{Handle, Selection};
use crate::view::{View, lane_bounds};
use eframe::egui::{
    Align, Color32, Label, Layout, Mesh, Painter, Rect, Response, Sense, Shape, Stroke, Ui,
    WidgetText, pos2, vec2,
};

/// The waveform inside the selection, and outside it.
const WAVE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 217);
const WAVE_OUTSIDE: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 51);
const FULL_SCALE: f32 = 0.9;
/// The light veil over the selected band.
const VEIL: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 18);
const HANDLE_W: f32 = 3.0;
const PLAYHEAD: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 153);
const TRACK: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 20);
const THUMB: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 90);
const TAG: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 235);
/// Size of the triangle on top of a cut marker.
const CUT_HEAD: f32 = 5.0;

/// Lays out the central area: `lines` at the bottom, the rest for the waveform.
pub fn area(ui: &mut Ui, lines: Vec<WidgetText>, sense: Sense) -> (Rect, Response) {
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        for line in lines.into_iter().rev() {
            ui.add(Label::new(line).wrap());
        }
        ui.add_space(4.0);
        let rect = ui.available_rect_before_wrap();
        (rect, ui.allocate_rect(rect, sense))
    })
    .inner
}

/// Min/max columns of the view's range for a waveform `width` points wide, one list per
/// channel. With fewer than `PeakCache::BASE_BIN` frames per column, the level-0 bins of the
/// whole take are returned instead (spread over the width): right only for a fitted view.
pub fn peak_lanes(peaks: &PeakCache, view: &View, width: f32) -> Vec<Vec<(f32, f32)>> {
    let n = width.round().max(1.0) as usize;
    let (from, to) = (view.from.max(0.0) as u64, view.to.ceil() as u64);
    (0..peaks.channels())
        .map(|ch| peaks.columns(ch, from, to, n).unwrap_or_else(|| peaks.bins(0, ch)))
        .collect()
}

/// One rectangle per channel inside `rect`, as laid out by `lane_bounds`.
pub fn lane_rects(rect: Rect, channels: usize) -> Vec<Rect> {
    lane_bounds(rect.top(), rect.height(), channels)
        .into_iter()
        .map(|(top, bottom)| Rect::from_x_y_ranges(rect.x_range(), top..=bottom))
        .collect()
}

fn y_of(v: f32, lane: Rect) -> f32 {
    let half = lane.height() / 2.0 * FULL_SCALE;
    (lane.center().y - v * half).clamp(lane.top(), lane.bottom())
}

fn draw_lane_bg(painter: &Painter, lane: Rect) {
    painter.rect_filled(lane, 2.0, theme::LANE);
    painter.hline(lane.x_range(), y_of(0.0, lane), Stroke::new(1.0, theme::HAIRLINE));
}

/// The waveform colour at `x`: bright inside `selected` (an x range), dim outside it;
/// everything is bright without a selection.
fn wave_color(x: f32, selected: Option<(f32, f32)>) -> Color32 {
    match selected {
        Some((a, b)) if x < a || x >= b => WAVE_OUTSIDE,
        _ => WAVE,
    }
}

/// Adds the bar covering values `lo..=hi` between `x.0` and `x.1`, at least `px` tall.
fn add_bar(mesh: &mut Mesh, lane: Rect, x: (f32, f32), (lo, hi): (f32, f32), px: f32, color: Color32) {
    let top = y_of(hi, lane);
    let bottom = y_of(lo, lane).max(top + px);
    mesh.add_colored_rect(Rect::from_min_max(pos2(x.0, top), pos2(x.1, bottom)), color);
}

/// Min/max columns spread evenly across `rect`; `lanes[ch][i]` is column `i` of channel `ch`.
/// Columns inside `selected` (screen x) are bright and the rest dim; the part of a bar
/// beyond ±1.0 is drawn in red.
pub fn draw_columns(painter: &Painter, rect: Rect, lanes: &[Vec<(f32, f32)>], selected: Option<(f32, f32)>) {
    let painter = painter.with_clip_rect(rect);
    let px = 1.0 / painter.pixels_per_point();
    for (lane, cols) in lane_rects(rect, lanes.len()).into_iter().zip(lanes) {
        draw_lane_bg(&painter, lane);
        if cols.is_empty() {
            continue;
        }
        let w = rect.width() / cols.len() as f32;
        let mut mesh = Mesh::default();
        for (i, &(lo, hi)) in cols.iter().enumerate() {
            if lo.is_nan() || hi.is_nan() || lo > hi {
                continue;
            }
            let x0 = rect.left() + i as f32 * w;
            let x = (x0, x0 + w.max(px));
            if lo <= 1.0 && hi >= -1.0 {
                let color = wave_color((x.0 + x.1) / 2.0, selected);
                add_bar(&mut mesh, lane, x, (lo.max(-1.0), hi.min(1.0)), px, color);
            }
            if hi > 1.0 {
                add_bar(&mut mesh, lane, x, (lo.max(1.0), hi), px, theme::DANGER);
            }
            if lo < -1.0 {
                add_bar(&mut mesh, lane, x, (lo, hi.min(-1.0)), px, theme::DANGER);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
}

/// Single samples as points joined by straight lines, for views with less than one frame
/// per pixel. `lanes[ch][i]` is frame `view.from.floor() + i` of channel `ch`.
/// Samples beyond ±1.0, and the segments that touch them, are drawn in red; the rest are
/// bright inside `selected` (screen x) and dim outside it.
pub fn draw_samples(painter: &Painter, rect: Rect, lanes: &[Vec<f32>], view: &View, selected: Option<(f32, f32)>) {
    let painter = painter.with_clip_rect(rect);
    let first = view.from.floor();
    let spacing = (1.0 / view.frames_per_px(rect.width()).max(f64::MIN_POSITIVE)) as f32;
    let radius = (spacing * 0.15).min(2.5);
    for (lane, samples) in lane_rects(rect, lanes.len()).into_iter().zip(lanes) {
        draw_lane_bg(&painter, lane);
        let at = |i: usize, v: f32| {
            pos2(rect.left() + view.x_at(first + i as f64, rect.width()), y_of(v, lane))
        };
        let color = |over: bool, x: f32| if over { theme::DANGER } else { wave_color(x, selected) };
        for (i, pair) in samples.windows(2).enumerate() {
            let over = pair[0].abs() > 1.0 || pair[1].abs() > 1.0;
            let (a, b) = (at(i, pair[0]), at(i + 1, pair[1]));
            painter.line_segment([a, b], Stroke::new(1.0, color(over, (a.x + b.x) / 2.0)));
        }
        if radius >= 1.0 {
            for (i, &v) in samples.iter().enumerate() {
                let p = at(i, v);
                painter.circle_filled(p, radius, color(v.abs() > 1.0, p.x));
            }
        }
    }
}

/// Min/max of `n` equal columns of the interleaved `samples`, one list per channel, split
/// like `PeakCache::columns`. With fewer frames than columns, each column takes one frame.
pub fn raw_columns(samples: &[f32], channels: usize, n: usize) -> Vec<Vec<(f32, f32)>> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = samples.len() / channels;
    let mut lanes = vec![Vec::with_capacity(n); channels];
    if frames == 0 {
        return lanes;
    }
    for c in 0..n {
        let a = c * frames / n;
        let b = ((c + 1) * frames / n).clamp(a + 1, frames);
        for (ch, lane) in lanes.iter_mut().enumerate() {
            let column = samples[a * channels..b * channels].iter().skip(ch).step_by(channels);
            lane.push(column.fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v))));
        }
    }
    lanes
}

/// Splits interleaved samples into one list per channel.
pub fn deinterleave(samples: &[f32], channels: usize) -> Vec<Vec<f32>> {
    (0..channels).map(|ch| samples.iter().skip(ch).step_by(channels).copied().collect()).collect()
}

/// Screen x range of the selection in `rect`, unclipped.
pub fn selection_xs(rect: Rect, view: &View, sel: &Selection) -> (f32, f32) {
    let x = |frame: u64| rect.left() + view.x_at(frame as f64, rect.width());
    (x(sel.start), x(sel.end))
}

/// A light veil over the selected band, both handles as white lines at full height and
/// the playback cursor.
pub fn draw_marks(painter: &Painter, rect: Rect, view: &View, sel: &Selection, playhead: f64) {
    let painter = painter.with_clip_rect(rect);
    let (sx, ex) = selection_xs(rect, view, sel);
    let band = Rect::from_x_y_ranges(sx.max(rect.left())..=ex.min(rect.right()), rect.y_range());
    if band.width() > 0.0 {
        painter.rect_filled(band, 0.0, VEIL);
    }
    for (handle, hx) in [(Handle::Start, sx), (Handle::End, ex)] {
        painter.vline(hx, rect.y_range(), Stroke::new(HANDLE_W, handle_color(sel.active == handle)));
    }
    let px = rect.left() + view.x_at(playhead, rect.width());
    painter.vline(px, rect.y_range(), Stroke::new(2.0, PLAYHEAD));
}

/// The handle the arrows move is pure white; the other one is dimmer.
pub fn handle_color(active: bool) -> Color32 {
    if active { Color32::WHITE } else { Color32::from_rgba_unmultiplied(255, 255, 255, 140) }
}

/// x of each cut inside the view's `[from, to)`, relative to the waveform's left edge.
pub fn visible_cut_xs(cuts: &[u64], view: &View, width: f32) -> Vec<f32> {
    cuts.iter()
        .filter(|&&c| (c as f64) >= view.from && (c as f64) < view.to)
        .map(|&c| view.x_at(c as f64, width))
        .collect()
}

/// x of each cut on a bar `width` wide that stands for the whole take of `total` frames.
pub fn overview_cut_xs(cuts: &[u64], total: u64, width: f32) -> Vec<f32> {
    let total = total.max(1) as f64;
    cuts.iter().map(|&c| (c as f64 / total * f64::from(width)) as f32).collect()
}

/// A cut: an amber line at full height with a small triangle on top. `xs` are relative to
/// `rect.left()`.
pub fn draw_cuts(painter: &Painter, rect: Rect, xs: &[f32]) {
    let painter = painter.with_clip_rect(rect);
    for &x in xs {
        let x = rect.left() + x;
        painter.vline(x, rect.y_range(), Stroke::new(2.0, theme::WARNING));
        let head = vec![
            pos2(x - CUT_HEAD, rect.top()),
            pos2(x + CUT_HEAD, rect.top()),
            pos2(x, rect.top() + CUT_HEAD * 1.4),
        ];
        painter.add(Shape::convex_polygon(head, theme::WARNING, Stroke::NONE));
    }
}

/// A small label at the top of `rect`, right of `x` (left of it near the right edge).
pub fn draw_tag(painter: &Painter, rect: Rect, x: f32, text: String) {
    let galley = painter.layout_job(theme::tabular_job(painter, &text, theme::font(12.0, theme::Weight::Regular), TAG));
    let pad = vec2(4.0, 2.0);
    let size = galley.size() + 2.0 * pad;
    let left = if x + 6.0 + size.x <= rect.right() { x + 6.0 } else { x - 6.0 - size.x };
    let bg = Rect::from_min_size(pos2(left, rect.top() + 4.0), size);
    painter.rect_filled(bg, 3.0, Color32::from_black_alpha(210));
    painter.galley(bg.min + pad, galley, TAG);
}

/// The thin bar under the waveform: which part of the take the view shows, and a tick for
/// every cut so the ones outside the view can be found.
pub fn draw_overview(painter: &Painter, bar: Rect, view: &View, cuts: &[u64]) {
    let total = view.total.max(1) as f64;
    let at = |frame: f64| bar.left() + bar.width() * (frame / total) as f32;
    let x1 = at(view.to).min(bar.right());
    let x0 = at(view.from).min(x1 - 3.0).max(bar.left());
    painter.rect_filled(bar, 2.0, TRACK);
    painter.rect_filled(Rect::from_x_y_ranges(x0..=x1.max(x0 + 3.0), bar.y_range()), 2.0, THUMB);
    for x in overview_cut_xs(cuts, view.total, bar.width()) {
        let tick = Rect::from_center_size(pos2(bar.left() + x, bar.center().y), vec2(2.0, bar.height()));
        painter.rect_filled(tick, 0.0, theme::WARNING);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_active_handle_is_brighter_than_the_other() {
        assert_eq!(handle_color(true), Color32::WHITE);
        assert_eq!(handle_color(false), Color32::from_rgba_unmultiplied(255, 255, 255, 140));
    }

    #[test]
    fn only_cuts_inside_the_view_are_drawn() {
        let mut view = View::new(1_000);
        view.from = 100.0;
        view.to = 300.0;
        assert_eq!(visible_cut_xs(&[50, 100, 200, 300, 900], &view, 200.0), vec![0.0, 100.0]);
    }

    #[test]
    fn overview_marks_every_cut_on_the_whole_take() {
        assert_eq!(overview_cut_xs(&[0, 500, 1_000], 1_000, 100.0), vec![0.0, 50.0, 100.0]);
    }

    #[test]
    fn short_takes_draw_the_bins_there_are() {
        let mut peaks = PeakCache::new(2);
        peaks.push(&vec![0.5; 2 * 1000]);
        let lanes = peak_lanes(&peaks, &View::new(peaks.frames()), 800.0);
        assert_eq!(lanes.len(), 2);
        assert_eq!(lanes[0].len(), 4); // 1000 frames = 3 full bins + 1 partial
    }

    #[test]
    fn long_takes_draw_one_column_per_point() {
        let mut peaks = PeakCache::new(1);
        peaks.push(&vec![0.25; 256 * 1000]);
        let lanes = peak_lanes(&peaks, &View::new(peaks.frames()), 800.0);
        assert_eq!(lanes[0].len(), 800);
        assert!(lanes[0].iter().all(|&c| c == (0.25, 0.25)));
    }

    #[test]
    fn an_empty_recording_has_empty_lanes() {
        let peaks = PeakCache::new(2);
        let lanes = peak_lanes(&peaks, &View::new(0), 800.0);
        assert_eq!(lanes, vec![Vec::<(f32, f32)>::new(); 2]);
    }

    #[test]
    fn raw_columns_hold_the_exact_min_and_max_per_channel() {
        // 6 stereo frames into 3 columns of 2 frames.
        let samples = [0.1, -0.1, 0.5, -0.2, -0.3, 0.0, 0.2, 0.9, 1.5, -1.2, 0.0, 0.3];
        let lanes = raw_columns(&samples, 2, 3);
        assert_eq!(lanes[0], vec![(0.1, 0.5), (-0.3, 0.2), (0.0, 1.5)]);
        assert_eq!(lanes[1], vec![(-0.2, -0.1), (0.0, 0.9), (-1.2, 0.3)]);
    }

    #[test]
    fn raw_columns_split_uneven_frames_like_the_peak_cache() {
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        // Mono, 10 frames, 4 columns: [0,2) [2,5) [5,7) [7,10).
        assert_eq!(raw_columns(&samples, 1, 4)[0], vec![(0.0, 1.0), (2.0, 4.0), (5.0, 6.0), (7.0, 9.0)]);
    }

    #[test]
    fn raw_columns_of_nothing_are_empty() {
        assert_eq!(raw_columns(&[], 2, 100), vec![Vec::<(f32, f32)>::new(); 2]);
        assert!(raw_columns(&[0.5], 0, 10).is_empty());
    }

    #[test]
    fn deinterleave_splits_channels_in_order() {
        let lanes = deinterleave(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0], 2);
        assert_eq!(lanes, vec![vec![1.0, 2.0, 3.0], vec![-1.0, -2.0, -3.0]]);
    }
}
