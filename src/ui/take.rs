//! Take state: waveform with sample-exact trim handles, zoom and scroll, playback with a
//! moving cursor, the drag tab that takes the selection out, and saving through the
//! Windows dialog.
//!
//! Where a press starts decides the gesture: on the tab it drags the selection out, near a
//! handle it moves that edge, anywhere else on the waveform it sweeps a new selection (a
//! click there places the cursor).

use super::theme::{self, Weight};
use super::wave;
use crate::dragout::{self, Exports};
use crate::files::{LocalTime, now_local, suggested_name};
use crate::player::Player;
use crate::selection::{Handle, Selection};
use crate::session::Session;
use crate::text::{fmt_peak, fmt_time};
use crate::variant::Variant;
use crate::view::{View, glide};
use eframe::egui::{
    Color32, Context, CornerRadius, CursorIcon, Event, Id, Key, Modifiers, MouseWheelUnit,
    PointerButton, Pos2, Rect, Response, Sense, Ui, pos2, vec2,
};
use rfd::FileDialog;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Zoom per wheel notch.
const ZOOM_STEP: f64 = 1.25;
/// Shift + wheel scrolls this fraction of the visible width per notch.
const SCROLL_STEP: f32 = 0.1;
/// Wheel deltas in points (precise devices) per notch; egui's own line height for scrolling.
const POINTS_PER_NOTCH: f32 = 40.0;
/// A press closer than this to a handle grabs it.
const GRAB_PX: f32 = 6.0;
/// Height of the bar under the waveform that shows the visible part, and the gap above it.
const OVERVIEW_H: f32 = 4.0;
const OVERVIEW_GAP: f32 = 3.0;
/// How far below that bar a press still takes it, because the bar is thin.
const OVERVIEW_REACH: f32 = 6.0;
/// Height of the row above the lanes that holds the drag tab.
pub(crate) const TAB_H: f32 = 24.0;
/// A tab narrower than this is widened and centred on its selection.
const TAB_MIN_W: f32 = 64.0;

/// What the editor asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TakeEvent {
    /// The tab was dragged: take frames `[start, end)` out to wherever it is dropped.
    DragOut { start: u64, end: u64 },
}

/// The part of the waveform a press lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Tab,
    Handle(Handle),
    Wave,
}

/// The gesture in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Handle(Handle),
    /// A new selection from the frame where the press started.
    Sweep { anchor: u64 },
    Tab,
}

/// Peak of the selection. Reading it may read the file, so it is cached per selection.
struct SelectionPeak {
    range: (u64, u64),
    peak: f32,
}

/// What is drawn for the visible range: min/max columns, or single samples when zoomed in
/// past one frame per pixel. `lanes[ch]` holds channel `ch`.
enum Lanes {
    Columns(Vec<Vec<(f32, f32)>>),
    Samples(Vec<Vec<f32>>),
}

/// Lanes of the last drawn range, kept while the view does not change.
struct LaneCache {
    /// First and end frame of the visible range, and the number of columns.
    key: (u64, u64, usize),
    lanes: Lanes,
}

struct Playback {
    player: Player,
    /// Where the cursor goes back to when the range has been heard.
    origin: u64,
}

/// The window paints between these two intervals while the take sounds: never less often
/// than the time needs, and never more often than any screen shows.
const PAINT_SLOWEST: Duration = Duration::from_millis(33);
const PAINT_FASTEST: Duration = Duration::from_millis(8);
/// How long a new take takes to settle: the tab comes down, the lanes make room for it.
const REVEAL_SECS: f32 = 0.28;
/// A difference this big between the cursor drawn and the player is a jump, not drift.
const CURSOR_JUMP_SECS: f64 = 0.05;
/// How much of its drift the cursor corrects on each paint.
const CURSOR_CORRECTION: f64 = 0.15;

/// Editing keys of the Take state; the action keys (R, Space, Ctrl+S, Supr) are global.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edit {
    Nudge(i64),
    Fit,
}

/// One open take with its playback and drawing caches.
pub struct TakeEditor {
    session: Session,
    playback: Option<Playback>,
    lanes: Option<LaneCache>,
    /// The view drawn and under the mouse, on its way to `session.view`, the view asked for.
    shown: View,
    /// Where the cursor is drawn: between the steps of the player while it sounds.
    shown_playhead: f64,
    /// How settled the take is on screen (0..1).
    reveal: f32,
    /// Width of the waveform when it was last drawn.
    wave_width: f32,
    /// The gesture in progress.
    drag: Option<Drag>,
    /// While the bar under the waveform is held: how far from the centre of the view, in
    /// frames, it was pressed.
    overview_grab: Option<f64>,
    peak: Option<SelectionPeak>,
    /// Samples already dragged out of this take.
    exports: Exports,
    /// Name prefix of this take's samples: the app it was recorded from, or «Teeth».
    sample_prefix: String,
    /// Whether listening repeats the selection until paused. Not kept between sessions.
    looping: bool,
    /// How the take is heard and exported. A sample with a variant is not the take.
    variant: Variant,
}

impl TakeEditor {
    pub fn new(session: Session) -> TakeEditor {
        TakeEditor {
            shown: session.view,
            shown_playhead: session.playhead as f64,
            reveal: 0.0,
            wave_width: 1.0,
            session,
            playback: None,
            lanes: None,
            drag: None,
            overview_grab: None,
            peak: None,
            exports: Exports::default(),
            sample_prefix: "Teeth".into(),
            looping: false,
            variant: Variant::default(),
        }
    }

    /// Marks the take as saved, as when the whole of it, as recorded and without a variant,
    /// has been handed out as a sample.
    pub fn mark_saved(&mut self) {
        self.session.take.saved = true;
    }

    /// The name prefix of this take's samples.
    pub fn sample_prefix(&self) -> &str {
        &self.sample_prefix
    }

    /// Names this take's samples after the app it was recorded from.
    pub fn set_sample_prefix(&mut self, prefix: &str) {
        self.sample_prefix = prefix.to_string();
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    /// Changes how the take is heard and exported. A listening in progress goes on from
    /// where it is with the new variant, and keeps the origin it will go back to; one that
    /// has just ended is left for `poll`, so that it does not sound again.
    pub fn set_variant(&mut self, variant: Variant) -> Result<(), String> {
        if variant == self.variant {
            return Ok(());
        }
        self.variant = variant;
        let Some(origin) = self.playback.as_ref().filter(|pb| !pb.player.is_finished()).map(|pb| pb.origin) else {
            return Ok(());
        };
        self.stop_playback();
        let result = self.toggle_play();
        if let Some(pb) = &mut self.playback {
            pb.origin = origin;
        }
        result
    }

    /// Whether the sample of the selection is the take itself: all of it, as it was recorded.
    /// Only then does handing it out count as saving the take.
    pub fn sample_is_the_take(&self) -> bool {
        self.session.selection.is_full() && self.variant.is_faithful()
    }

    /// Writes the selection, with the variant, as a sample in `dir`, or hands out the one
    /// already written for the same range and variant.
    pub fn export_selection(&mut self, dir: &Path, source: &str, now: &LocalTime) -> Result<PathBuf, String> {
        let variant = self.variant();
        dragout::export(&mut self.session, &mut self.exports, dir, source, variant, now)
    }

    /// Writes the selection into `dir` as a sample, as a drag does (reusing the one already
    /// written for the same range and variant). Downloading the whole take as it was
    /// recorded counts as saving it.
    pub fn download(&mut self, dir: &Path, now: &LocalTime) -> Result<PathBuf, String> {
        let source = self.sample_prefix.clone();
        let path = self.export_selection(dir, &source, now)?;
        if self.sample_is_the_take() {
            self.mark_saved();
        }
        Ok(path)
    }

    pub fn saved(&self) -> bool {
        self.session.take.saved
    }

    pub fn playing(&self) -> bool {
        self.playback.is_some()
    }

    pub fn looping(&self) -> bool {
        self.looping
    }

    /// Turns the loop on or off; a playback in progress follows at once.
    pub fn toggle_loop(&mut self) {
        self.looping = !self.looping;
        if let Some(pb) = &self.playback {
            pb.player.set_looping(self.looping);
        }
    }

    /// Puts the cursor where listening starts to sound: the start handle, or the end handle
    /// in reverse. While listening, listening starts again there.
    pub fn rewind(&mut self) -> Result<(), String> {
        let playing = self.playing();
        self.stop_playback();
        let sel = self.session.selection;
        self.session.playhead = if self.variant.reversed { sel.end } else { sel.start };
        if playing { self.toggle_play() } else { Ok(()) }
    }

    /// Whether `path` is the file this take lives in.
    pub fn holds_file(&self, path: &Path) -> bool {
        self.session.holds_file(path)
    }

    /// Moves both handles to their nearest zero crossing.
    pub fn snap_to_zero(&mut self) -> Result<(), String> {
        self.session.snap_to_zero().map_err(|e| read_error_text(&e))
    }

    /// Moves the handles inward to the first and last sound of the selection. `Ok(false)`, with
    /// the handles where they were, when the selection is only silence.
    pub fn trim_silence(&mut self) -> Result<bool, String> {
        self.session.trim_silence().map_err(|e| read_error_text(&e))
    }

    /// Duration of the whole take, for the timer.
    pub fn duration_text(&self) -> String {
        fmt_time(self.session.reader.frames(), self.session.reader.format().sample_rate())
    }

    /// `cursor / total` for the bar.
    pub fn time_text(&self) -> String {
        let rate = self.session.reader.format().sample_rate();
        format!("{} / {}", fmt_time(self.session.playhead, rate), self.duration_text())
    }

    /// How often to paint while the take sounds: the cursor walks at the rate of the variant.
    pub fn paint_pace(&self) -> Duration {
        let rate = self.variant.rate(self.session.reader.format().sample_rate());
        cursor_pace(self.shown.frames_per_px(self.wave_width), rate)
    }

    /// Follows the playback; once it ends, puts the cursor back where it started and
    /// returns the error text if playback failed.
    pub fn poll(&mut self) -> Option<String> {
        let pb = self.playback.as_ref()?;
        if !pb.player.is_finished() {
            self.session.playhead = pb.player.position();
            return None;
        }
        let pb = self.playback.take()?;
        let error = pb.player.error();
        pb.player.stop();
        self.session.playhead = pb.origin;
        error.map(|e| play_error_text(&e))
    }

    /// Pauses (the cursor stays where the sound stopped) or starts listening from the
    /// cursor to the end handle, or from the start handle when the cursor is outside.
    /// While looping, the end handle leads back to the start handle until paused. In
    /// reverse it is the other way round: from the cursor back to the start handle, or from
    /// the end handle when the cursor is outside.
    pub fn toggle_play(&mut self) -> Result<(), String> {
        if self.playback.is_some() {
            self.stop_playback();
            return Ok(());
        }
        let (start, end) = (self.session.selection.start, self.session.selection.end);
        let from = play_start(self.session.playhead, start, end, self.variant.reversed);
        let player = Player::start(&self.session.take.path, from, end, start, self.looping, self.variant).map_err(|e| play_error_text(&e))?;
        self.session.playhead = from;
        self.playback = Some(Playback { player, origin: from });
        Ok(())
    }

    pub fn stop_playback(&mut self) {
        if let Some(pb) = self.playback.take() {
            self.session.playhead = pb.player.stop();
        }
    }

    /// Asks where to save the selection and saves it. `Ok(None)` when the dialog was
    /// cancelled; `Ok(Some(dest))` once the file is written.
    pub fn save(&mut self, parent: &eframe::Frame, last_folder: Option<&Path>) -> Result<Option<PathBuf>, String> {
        let mut dialog = FileDialog::new()
            .add_filter("WAV", &["wav"])
            .set_file_name(suggested_name(&now_local()))
            .set_parent(parent);
        if let Some(dir) = last_folder.filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        let Some(dest) = dialog.save_file() else { return Ok(None) };
        self.save_to(&dest)?;
        Ok(Some(dest))
    }

    /// Writes the selection to `dest` as it was recorded: a variant is for listening and
    /// for samples, never for the take.
    fn save_to(&mut self, dest: &Path) -> Result<(), String> {
        // The player reads the file that saving may move or replace.
        self.stop_playback();
        let result = self.session.save(dest);
        // A same-file save reloads the take, so nothing cached is valid any more.
        self.lanes = None;
        self.shown = self.session.view;
        result.map_err(|e| format!("No se pudo guardar: {e}"))
    }

    /// Discards or closes the take: stops the sound and deletes the temp file, if any.
    pub fn close(mut self) -> io::Result<()> {
        self.stop_playback();
        self.session.close()
    }

    /// At exit: a saved take is closed (its temp, if any, deleted); an unsaved one keeps
    /// its temp file so the next start recovers it.
    pub fn exit(mut self) {
        self.stop_playback();
        if self.saved() {
            let _ = self.session.close();
        }
    }

    /// Draws the take and, when `interactive`, handles keys, wheel and mouse. Returns what
    /// the window must do, if anything.
    pub fn show(&mut self, ui: &mut Ui, interactive: bool) -> Option<TakeEvent> {
        if interactive {
            self.keys(ui.ctx());
        } else {
            self.drag = None;
        }
        refresh_peak(&mut self.peak, &mut self.session);
        let sense = if interactive { Sense::click_and_drag() } else { Sense::hover() };
        let (area, response) = wave::area(ui, Vec::new(), sense);
        self.settle(ui.ctx());
        // A recording fills the area: from there the lanes make room for the tab and the bar
        // under them, so the waveform does not jump.
        let mut rect = area;
        rect.set_top((area.top() + TAB_H * self.reveal).min(area.bottom()));
        rect.set_bottom((area.bottom() - (OVERVIEW_H + OVERVIEW_GAP) * self.reveal).max(rect.top()));
        let bar = Rect::from_x_y_ranges(area.x_range(), area.bottom() - OVERVIEW_H..=area.bottom());
        let mut event = None;
        if interactive {
            let (changed, e) = self.pointer(ui, &response, rect);
            if changed {
                ui.ctx().request_repaint();
            }
            event = e;
            self.overview_bar(ui, area, rect, bar);
        }
        self.wave_width = rect.width();
        self.follow_view(ui.ctx(), rect.width());
        self.follow_cursor(ui.ctx());
        self.draw(ui, rect, bar);
        self.draw_tab(ui, &response, rect);
        event
    }

    fn settle(&mut self, ctx: &Context) {
        self.reveal = if theme::animated(ctx) {
            theme::approach(self.reveal, 1.0, ctx.input(|i| i.stable_dt).min(0.1), REVEAL_SECS)
        } else {
            1.0
        };
        if self.reveal < 1.0 {
            ctx.request_repaint();
        }
    }

    /// Moves the view drawn towards the view asked for.
    fn follow_view(&mut self, ctx: &Context, width: f32) {
        let target = self.session.view;
        self.shown = if theme::animated(ctx) {
            glide(self.shown, target, ctx.input(|i| i.stable_dt).min(0.1), width)
        } else {
            target
        };
        if self.shown != target {
            ctx.request_repaint();
        }
    }

    /// Moves the cursor drawn with the playback, or puts it where the cursor is.
    fn follow_cursor(&mut self, ctx: &Context) {
        let actual = self.session.playhead;
        self.shown_playhead = if self.playing() && theme::animated(ctx) {
            let velocity = self.variant.velocity(self.session.reader.format().sample_rate());
            follow_playhead(self.shown_playhead, actual, ctx.input(|i| i.stable_dt).min(0.1), velocity)
        } else {
            actual as f64
        };
    }

    fn keys(&mut self, ctx: &Context) {
        let rate = self.session.reader.format().sample_rate();
        let edits: Vec<Edit> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key { key, pressed: true, repeat, modifiers, .. } => {
                        edit_key(*key, *modifiers, *repeat, rate)
                    }
                    _ => None,
                })
                .collect()
        });
        for edit in edits {
            match edit {
                Edit::Nudge(delta) => self.session.selection.nudge(delta),
                Edit::Fit => self.session.view.fit(),
            }
        }
    }

    /// Wheel zoom and scroll, then the gesture of the press: drag the tab out, move a
    /// handle or sweep a new selection; a click activates a handle or places the cursor.
    /// Returns whether anything changed and the event to hand to the window.
    fn pointer(&mut self, ui: &Ui, response: &Response, rect: Rect) -> (bool, Option<TakeEvent>) {
        let width = rect.width();
        let s = &mut self.session;
        let mut changed = false;
        let mut event = None;
        if let Some(pos) = response.hover_pos() {
            let (zoom, scroll) = ui.input(|i| wheel_notches(&i.events));
            if zoom != 0.0 {
                let at = s.view.frame_at(pos.x - rect.left(), width);
                s.view.zoom(zoom_factor(zoom), at, width);
                changed = true;
            }
            if scroll != 0.0 {
                s.view.scroll_px(scroll * SCROLL_STEP * width, width);
                changed = true;
            }
        }
        let view = self.shown;
        let total = s.selection.total();
        let frame_of = |x: f32| view.frame_at(x - rect.left(), width).round().clamp(0.0, total as f64) as u64;
        let (sx, ex) = wave::selection_xs(rect, &view, &s.selection);
        let tab = tab_rect((sx, ex), tab_row(rect));

        if response.drag_started_by(PointerButton::Primary) {
            let (origin, delta) = ui.input(|i| (i.pointer.press_origin(), i.pointer.total_drag_delta()));
            let toward = if delta.is_some_and(|d| d.x < 0.0) { Handle::Start } else { Handle::End };
            self.drag = origin.map(|p| match zone_at(p, tab, sx, ex, toward) {
                Zone::Tab => {
                    event = Some(TakeEvent::DragOut { start: s.selection.start, end: s.selection.end });
                    Drag::Tab
                }
                Zone::Handle(h) => {
                    s.selection.active = h;
                    Drag::Handle(h)
                }
                Zone::Wave => Drag::Sweep { anchor: frame_of(p.x) },
            });
        }
        match (self.drag, response.interact_pointer_pos()) {
            (Some(drag), Some(p)) if response.dragged_by(PointerButton::Primary) => {
                let before = s.selection;
                match drag {
                    Drag::Handle(Handle::Start) => s.selection.set_start(frame_of(p.x)),
                    Drag::Handle(Handle::End) => s.selection.set_end(frame_of(p.x)),
                    Drag::Sweep { anchor } => sweep(&mut s.selection, anchor, frame_of(p.x)),
                    Drag::Tab => {}
                }
                changed |= s.selection != before;
            }
            _ => self.drag = None,
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            match zone_at(p, tab, sx, ex, s.selection.active) {
                Zone::Tab => {}
                Zone::Handle(h) => s.selection.active = h,
                Zone::Wave => {
                    if let Some(pb) = self.playback.take() {
                        pb.player.stop();
                    }
                    s.playhead = frame_of(p.x);
                }
            }
            changed = true;
        }
        let hovered = response.hover_pos().map(|p| zone_at(p, tab, sx, ex, Handle::Start));
        let cursor = match (self.drag, hovered) {
            (Some(Drag::Tab), _) => Some(CursorIcon::Grabbing),
            (Some(Drag::Handle(_)), _) | (None, Some(Zone::Handle(_))) => Some(CursorIcon::ResizeHorizontal),
            (Some(Drag::Sweep { .. }), _) | (None, Some(Zone::Wave)) => Some(CursorIcon::Crosshair),
            (None, Some(Zone::Tab)) => Some(CursorIcon::Grab),
            (None, None) => None,
        };
        if let Some(icon) = cursor {
            ui.ctx().set_cursor_icon(icon);
        }
        (changed, event)
    }

    /// The bar under the lanes of a zoomed take: pressing it brings that part of the take into
    /// view, and dragging it moves the view. The visible part of the bar stays held where it
    /// was pressed. The cuts and the cursor stay where they are.
    fn overview_bar(&mut self, ui: &Ui, area: Rect, lanes: Rect, bar: Rect) {
        if self.session.view.is_fitted() {
            self.overview_grab = None;
            return;
        }
        let hit = Rect::from_x_y_ranges(area.x_range(), lanes.bottom()..=area.bottom() + OVERVIEW_REACH);
        let response = ui.interact(hit, Id::unique("take-overview"), Sense::click_and_drag());
        let held = response.is_pointer_button_down_on();
        // A quick click can bring the press and the release in one pass.
        let Some(p) = response.interact_pointer_pos().filter(|_| held || response.clicked()) else {
            self.overview_grab = None;
            if response.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Grab);
            }
            return;
        };
        let view = &mut self.session.view;
        let frame = f64::from((p.x - bar.left()) / bar.width().max(1.0)) * view.total as f64;
        let on_thumb = (view.from..=view.to).contains(&frame);
        let grab = *self.overview_grab.get_or_insert(if on_thumb { frame - (view.from + view.to) / 2.0 } else { 0.0 });
        view.center_on(frame - grab);
        if !held {
            self.overview_grab = None;
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        ui.ctx().request_repaint();
    }

    /// The drag tab over the selection: the duration of the file that would come out of it,
    /// and a red dot with the peak on hover when the selection goes past 0 dBFS.
    fn draw_tab(&self, ui: &Ui, response: &Response, rect: Rect) {
        let view = self.shown;
        let sel = self.session.selection;
        let Some(tab) = tab_rect(wave::selection_xs(rect, &view, &sel), tab_row(rect)) else {
            return;
        };
        let hovered = response.hover_pos().is_some_and(|p| tab.contains(p));
        let lit = theme::hover_t(ui.ctx(), Id::unique("take-tab"), hovered || self.drag == Some(Drag::Tab));
        let alpha = (36.0 + 20.0 * lit) as u8;
        let mut painter = ui.painter_at(tab_row(rect).expand2(vec2(TAB_MIN_W, 0.0)));
        painter.multiply_opacity(self.reveal);
        let corners = CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 };
        painter.rect_filled(tab, corners, Color32::from_rgba_unmultiplied(255, 255, 255, alpha));

        let label = fmt_time(sel.frames(), self.variant.rate(self.session.reader.format().sample_rate()));
        let font = theme::font(12.0, Weight::Medium);
        let over = self.peak.as_ref().is_some_and(|p| p.peak > 1.0);
        let galley = painter.layout_job(theme::tabular_job(&painter, &label, font, theme::TEXT));
        let text_w = galley.size().x;
        let grip_w = 10.0;
        let dot_w = if over { 10.0 } else { 0.0 };
        let left = tab.center().x - (grip_w + 6.0 + text_w + dot_w) / 2.0;
        paint_grip(&painter, pos2(left + grip_w / 2.0, tab.center().y));
        let text_left = left + grip_w + 6.0;
        painter.galley(pos2(text_left, tab.center().y - galley.size().y / 2.0), galley, theme::TEXT);
        if let Some(p) = self.peak.as_ref().filter(|_| over) {
            painter.circle_filled(pos2(text_left + text_w + 7.0, tab.center().y), 3.0, theme::DANGER);
            if hovered {
                response.clone().on_hover_text_at_pointer(fmt_peak(p.peak));
            }
        }
    }

    fn draw(&mut self, ui: &Ui, rect: Rect, bar: Rect) {
        let width = rect.width();
        let n = width.round().max(1.0) as usize;
        let view = self.shown;
        let key = (view.from.max(0.0).floor() as u64, view.to.ceil() as u64, n);
        if self.lanes.as_ref().is_some_and(|c| c.key != key) {
            self.lanes = None;
        }
        let session = &mut self.session;
        let cache = self.lanes.get_or_insert_with(|| LaneCache { key, lanes: lanes_for(session, key) });
        let painter = ui.painter_at(rect.union(bar));
        let selected = Some(wave::selection_xs(rect, &view, &self.session.selection));
        match &cache.lanes {
            Lanes::Columns(lanes) => wave::draw_columns(&painter, rect, lanes, selected),
            Lanes::Samples(lanes) => wave::draw_samples(&painter, rect, lanes, &view, selected),
        }
        let sel = &self.session.selection;
        let mut marks = painter.clone();
        marks.multiply_opacity(self.reveal);
        wave::draw_marks(&marks, rect, &view, sel, self.shown_playhead);
        let cuts = &self.session.take.cuts;
        wave::draw_cuts(&painter, rect, &wave::visible_cut_xs(cuts, &view, width));
        if let Some(Drag::Handle(h)) = self.drag {
            let frame = if h == Handle::Start { sel.start } else { sel.end };
            let mut text = fmt_time(frame, self.session.reader.format().sample_rate());
            if view.frames_per_px(width) < 1.0 {
                text.push_str(&format!(" · muestra {frame}"));
            }
            wave::draw_tag(&painter, rect, rect.left() + view.x_at(frame as f64, width), text);
        }
        if !view.is_fitted() {
            wave::draw_overview(&painter, bar, &view, &self.session.take.cuts);
        }
    }
}

/// Reads what the visible range `[from, to)` drawn as `n` columns needs: the peak cache
/// when columns hold at least 256 frames, otherwise the samples from the file.
fn lanes_for(session: &mut Session, (from, to, n): (u64, u64, usize)) -> Lanes {
    let channels = session.peaks.channels();
    let peaks: Option<Vec<_>> = (0..channels).map(|ch| session.peaks.columns(ch, from, to, n)).collect();
    if let Some(columns) = peaks {
        return Lanes::Columns(columns);
    }
    if to.saturating_sub(from) >= n as u64 {
        let raw = session.read_raw(from, to).unwrap_or_default();
        Lanes::Columns(wave::raw_columns(&raw, channels, n))
    } else {
        // One more frame, so the line reaches past the right edge.
        let raw = session.read_raw(from, (to + 1).min(session.reader.frames())).unwrap_or_default();
        Lanes::Samples(wave::deinterleave(&raw, channels))
    }
}

/// The row above the lanes `rect` where the tab sits, as wide as the lanes.
fn tab_row(rect: Rect) -> Rect {
    Rect::from_x_y_ranges(rect.x_range(), (rect.top() - TAB_H)..=rect.bottom())
}

/// The tab over a selection drawn from `sel.0` to `sel.1` (screen x): `TAB_H` tall on the
/// top edge of `wave`, cut to its width, at least `TAB_MIN_W` wide and centred on the
/// visible part; `None` when nothing of the selection is visible.
pub fn tab_rect(sel: (f32, f32), wave: Rect) -> Option<Rect> {
    let (mut left, mut right) = (sel.0.max(wave.left()), sel.1.min(wave.right()));
    if right <= left {
        return None;
    }
    if right - left < TAB_MIN_W {
        let c = (left + right) / 2.0;
        (left, right) = (c - TAB_MIN_W / 2.0, c + TAB_MIN_W / 2.0);
    }
    Some(Rect::from_min_max(pos2(left, wave.top()), pos2(right, wave.top() + TAB_H)))
}

/// What a press at `pos` grabs: the tab first, then a handle within `GRAB_PX`, else the
/// waveform.
pub fn zone_at(pos: Pos2, tab: Option<Rect>, start_x: f32, end_x: f32, prefer: Handle) -> Zone {
    if tab.is_some_and(|t| t.contains(pos)) {
        return Zone::Tab;
    }
    match hit_handle(pos.x, start_x, end_x, prefer) {
        Some(h) => Zone::Handle(h),
        None => Zone::Wave,
    }
}

/// Selects from `anchor` to `pointer` in either direction, at least one frame wide; the
/// handle that follows the pointer becomes active.
fn sweep(sel: &mut Selection, anchor: u64, pointer: u64) {
    let total = sel.total();
    let (mut start, mut end) = if pointer >= anchor { (anchor, pointer) } else { (pointer, anchor) };
    if end == start {
        if end < total { end += 1 } else { start -= 1 }
    }
    sel.start = start;
    sel.end = end;
    sel.active = if pointer >= anchor { Handle::End } else { Handle::Start };
}

/// Six dots in two rows: the handhold of the tab.
fn paint_grip(painter: &eframe::egui::Painter, c: Pos2) {
    for row in [-2.0, 2.0] {
        for col in [-3.5, 0.0, 3.5] {
            painter.circle_filled(pos2(c.x + col, c.y + row), 1.1, theme::TEXT_SUPPORT);
        }
    }
}

/// Recomputes the selection peak only when the selection changed.
fn refresh_peak(cache: &mut Option<SelectionPeak>, session: &mut Session) {
    let range = (session.selection.start, session.selection.end);
    if cache.as_ref().is_some_and(|c| c.range == range) {
        return;
    }
    *cache = Some(SelectionPeak { range, peak: session.selection_peak() });
}

/// How often to paint so that a cursor moving through `frames_per_px` is drawn on every pixel.
/// Presenting a frame costs far more than preparing it, so frames that would show the same
/// picture are not worth painting.
fn cursor_pace(frames_per_px: f64, rate: u32) -> Duration {
    Duration::from_secs_f64(frames_per_px / f64::from(rate.max(1))).clamp(PAINT_FASTEST, PAINT_SLOWEST)
}

/// Where to draw the cursor of a playback `dt` seconds later: it advances with the clock, at
/// `velocity` frames per second (negative in reverse), and leans towards `actual`, what the
/// player reports in steps of the device's period. A jump (the loop going back, a restart)
/// is followed at once.
fn follow_playhead(shown: f64, actual: u64, dt: f32, velocity: f64) -> f64 {
    let predicted = shown + f64::from(dt) * velocity;
    let drift = actual as f64 - predicted;
    if drift.abs() > CURSOR_JUMP_SECS * velocity.abs() {
        actual as f64
    } else {
        predicted + drift * CURSOR_CORRECTION
    }
}

/// Playback starts at the cursor when it is inside the selection, else at the start handle.
/// Reversed, the cursor counts as inside when something sounds before it (it may sit on the
/// end handle, where a loop passes), and else playback starts at the end handle.
fn play_start(playhead: u64, start: u64, end: u64, reversed: bool) -> u64 {
    if reversed {
        if (start + 1..=end).contains(&playhead) { playhead } else { end }
    } else if (start..end).contains(&playhead) {
        playhead
    } else {
        start
    }
}

/// `No se pudo leer la toma: {detalle}`.
fn read_error_text(e: &io::Error) -> String {
    format!("No se pudo leer la toma: {e}")
}

/// `No se pudo reproducir: {detalle}`. A start failure from the player already reads
/// "No se pudo iniciar la reproducción ({detalle})."; only its detail is kept.
fn play_error_text(e: &str) -> String {
    let detail = e
        .strip_prefix("No se pudo iniciar la reproducción (")
        .and_then(|d| d.strip_suffix(")."))
        .unwrap_or(e);
    format!("No se pudo reproducir: {detail}")
}

/// The handle a press at `x` grabs: the nearer one closer than `GRAB_PX`, or `prefer`
/// when both are about equally close (handles drawn on top of each other).
fn hit_handle(x: f32, start_x: f32, end_x: f32, prefer: Handle) -> Option<Handle> {
    let (ds, de) = ((x - start_x).abs(), (x - end_x).abs());
    match (ds < GRAB_PX, de < GRAB_PX) {
        (false, false) => None,
        (true, false) => Some(Handle::Start),
        (false, true) => Some(Handle::End),
        (true, true) if (ds - de).abs() < 0.5 => Some(prefer),
        (true, true) => Some(if ds < de { Handle::Start } else { Handle::End }),
    }
}

/// Zoom notches (wheel without Shift; positive zooms in) and scroll notches (Shift + wheel
/// or a horizontal wheel; positive moves towards the end) in this frame's raw wheel events.
/// Raw events are unsmoothed, so one notch of a plain wheel is exactly one step.
fn wheel_notches(events: &[Event]) -> (f32, f32) {
    let (mut zoom, mut scroll) = (0.0, 0.0);
    for e in events {
        if let Event::MouseWheel { unit, delta, modifiers, .. } = e {
            let d = match unit {
                MouseWheelUnit::Point => *delta / POINTS_PER_NOTCH,
                MouseWheelUnit::Line | MouseWheelUnit::Page => *delta,
            };
            if modifiers.shift {
                scroll -= d.x + d.y;
            } else {
                zoom += d.y;
                scroll -= d.x;
            }
        }
    }
    (zoom, scroll)
}

fn zoom_factor(notches: f32) -> f64 {
    ZOOM_STEP.powf(f64::from(notches))
}

/// ← → move the active handle one frame, 10 ms with Shift (held keys repeat); 0 fits.
fn edit_key(key: Key, modifiers: Modifiers, repeat: bool, rate: u32) -> Option<Edit> {
    let sign = match key {
        Key::ArrowLeft => -1,
        Key::ArrowRight => 1,
        Key::Num0 if modifiers.is_none() && !repeat => return Some(Edit::Fit),
        _ => return None,
    };
    if modifiers.is_none() {
        Some(Edit::Nudge(sign))
    } else if modifiers.shift_only() {
        Some(Edit::Nudge(sign * i64::from(rate / 100).max(1)))
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
