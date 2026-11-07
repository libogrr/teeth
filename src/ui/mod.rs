//! Main window: the Ready, Recording and Take states, notices, the confirmation before
//! losing an unsaved take, recovery of an unsaved take at startup and cleanup at exit.

pub mod bar;
mod pending;
mod source;
pub mod splash;
pub mod theme;
mod take;
mod take_row;
pub mod wave;

use crate::capture::{CAPTURE_FAILED, CaptureSource, process_loopback_supported};
use crate::dragout::{drag_file, samples_dir};
use crate::files::{now_local, orphans, temp_dir};
use crate::recorder::Recorder;
use crate::recorder::writer_loop::EndReason;
use crate::session::{Session, SessionError};
use crate::settings::Settings;
use crate::sources;
use crate::text::fmt_time;
use crate::variant::Variant;
use crate::view::View;
use crate::win::default_output_name;
use bar::{BarAction, BarMode, BarState, SourceView, shortcut};
use splash::Splash;
use eframe::egui::{
    self, Align2, Color32, Galley, Id, Order, Rect, RichText, Sense, Ui, UiBuilder, ViewportCommand, pos2,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use pending::Pending;
use source::{Choice, SourcePicker};
use take::{TakeEditor, TakeEvent};
use theme::DANGER as RED;
use wave::peak_lanes;

/// Smallest window: the take bar needs this width for its buttons and the time of a take
/// under ten hours.
pub const MIN_WINDOW: [f32; 2] = [780.0, 300.0];
/// Repaint interval while a recording is starting or running.
const LIVE_REPAINT: Duration = Duration::from_millis(33);
/// The same while minimized: it only has to notice when the recording or playback ends.
const HIDDEN_REPAINT: Duration = Duration::from_millis(100);
const DEVICE_REFRESH: Duration = Duration::from_secs(2);
/// Room between the content and the floating bar, where the notice line goes.
pub(crate) const NOTICE_ROOM: f32 = 26.0;
/// Space between the notice line and the variant controls on its right.
const CONTROLS_GAP: f32 = 12.0;
const BACKLOG_WARN_SECS: f32 = 10.0;
const READY_HINT: &str = "Pulsa R para grabar";
const PREPARING: &str = "Preparando…";
const ONLY_SILENCE: &str = "La selección es solo silencio.";
const OPENING: &str = "Abriendo…";
const DROP_HERE: &str = "Suelta para abrir";
const STOP_RECORDING_FIRST: &str = "Para la grabación antes de abrir un archivo.";
/// How long after a drag of the tab a drop of its sample is that same drag.
const OWN_DROP_WINDOW: Duration = Duration::from_secs(2);
/// How long a notice takes to appear and to go.
const NOTICE_IN_SECS: f32 = 0.15;
const NOTICE_OUT_SECS: f32 = 0.30;
/// How long the hint of the empty window takes to appear.
const READY_SECS: f32 = 0.25;
/// How long the veil over a take takes to appear when a new recording starts.
const VEIL_SECS: f32 = 0.12;
/// How long closing the window waits for a recording that is still starting.
const PREPARING_TIMEOUT: Duration = Duration::from_secs(12);
const SYSTEM_NAME: &str = "Todo el sistema";
/// Shown while recording, and in Ready once known, when the capture follows the volume.
const VOLUME_DEPENDENT: &str =
    "Esta versión de Windows no permite grabar antes del volumen: la toma depende del volumen de Windows.";

enum Mode {
    Ready,
    Recording(Recorder),
    Take(Box<TakeEditor>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoticeKind {
    Error,
    Warning,
    Info,
}

struct Notice {
    text: String,
    kind: NoticeKind,
    shown: Instant,
}

impl Notice {
    fn info(text: impl Into<String>) -> Option<Notice> {
        Some(Notice { text: text.into(), kind: NoticeKind::Info, shown: Instant::now() })
    }

    fn error(text: impl Into<String>) -> Option<Notice> {
        Some(Notice { text: text.into(), kind: NoticeKind::Error, shown: Instant::now() })
    }
}

/// How long an information notice stays before it fades.
const INFO_LIFETIME: Duration = Duration::from_secs(4);

/// Whether a notice of `kind` shown `age` ago is still on screen. Information fades after
/// four seconds; errors stay until the next action and warnings while their cause lasts.
fn notice_visible(kind: NoticeKind, age: Duration) -> bool {
    kind != NoticeKind::Info || age < INFO_LIFETIME
}

/// Requests that would lose an unsaved take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destructive {
    Record,
    Discard,
    Close,
    /// Opening a file dropped on the window.
    Open,
}

/// What a destructive request does in the current state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Run,
    Ask,
    Ignore,
}

/// The open take, as far as confirmations care.
#[derive(Clone, Copy)]
struct TakeFacts {
    saved: bool,
    playing: bool,
}

pub struct TeethApp {
    mode: Mode,
    /// The logo blinking on black while the app opens; `None` once it is over.
    splash: Option<Splash>,
    /// The splash screen's silhouette, loaded on its first frame.
    splash_mark: Option<egui::TextureHandle>,
    /// The last notice line, kept while it fades out.
    shown_notice: Option<(String, NoticeKind)>,
    /// The variant of the last take, kept while the controls fade out.
    shown_variant: Variant,
    /// A recording that is starting on its own thread. Meanwhile the mode stays as it was,
    /// so a failure leaves the open take untouched.
    preparing: Option<Pending<Recorder>>,
    /// A file opening on its own thread, and where it is. Meanwhile the mode stays as it was,
    /// so a failure leaves the open take untouched.
    opening: Option<(Pending<Session>, PathBuf)>,
    /// The dropped file waiting for `¿Descartar la toma sin guardar?`.
    to_open: Option<PathBuf>,
    /// The sample last dragged out of the tab, and when that drag ended.
    own_drag: Option<(PathBuf, Instant)>,
    notice: Option<Notice>,
    /// The request waiting for `¿Descartar la toma sin guardar?`.
    confirm: Option<Destructive>,
    /// Set once the user agreed to close: the next close request goes through.
    allow_close: bool,
    /// Default output device name; `None` when there is none.
    device: Option<String>,
    /// Last device lookup; `None` forces one on the next pass.
    device_checked: Option<Instant>,
    /// Whether the last recording was captured before the Windows volume; `None` until
    /// the first recording starts.
    volume_independent: Option<bool>,
    settings: Settings,
    window_size: Option<[f32; 2]>,
    window_pos: Option<[f32; 2]>,
    /// Executable of the app this session records; `None` is the whole system.
    source: Option<String>,
    /// How the source is shown: the app's name, or «Todo el sistema».
    source_name: String,
    picker: SourcePicker,
    /// Whether Windows can record a single app at all.
    single_app: bool,
}

impl TeethApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, settings: Settings) -> TeethApp {
        let mut app = TeethApp {
            mode: Mode::Ready,
            splash: Some(Splash::new()),
            splash_mark: None,
            shown_notice: None,
            shown_variant: Variant::default(),
            preparing: None,
            opening: None,
            to_open: None,
            own_drag: None,
            notice: None,
            confirm: None,
            allow_close: false,
            device: None,
            device_checked: None,
            volume_independent: None,
            settings,
            window_size: None,
            window_pos: None,
            source: None,
            source_name: SYSTEM_NAME.into(),
            picker: SourcePicker::default(),
            single_app: process_loopback_supported(),
        };
        // The remembered app is used only if it is running; the setting stays either way.
        if app.single_app
            && let Some(exe) = app.settings.source.clone()
            && sources::resolve(&exe).is_some()
        {
            app.source_name = sources::app_name(&exe);
            app.source = Some(exe);
        }
        app.refresh_device();
        app.recover_orphan();
        app
    }

    /// Paints the splash screen while it lasts; see `splash_pass`. It ends at once for
    /// whatever the user must see: a question, a file being opened, or one held over the
    /// window.
    fn show_splash(&mut self, ctx: &egui::Context) -> bool {
        let files = ctx.input(|i| !i.raw.hovered_files.is_empty() || !i.raw.dropped_files.is_empty());
        let end = self.confirm.is_some() || self.opening.is_some() || files;
        splash_pass(ctx, &mut self.splash, &mut self.splash_mark, &mut self.notice, end)
    }

    /// Opens the newest take that can be opened, of those left in the temp folder. Orphans
    /// without audio are deleted silently; one that fails is kept and reported, and the next
    /// newest is tried, so a damaged file never blocks the others.
    fn recover_orphan(&mut self) {
        let (session, failed) = Session::recover_first(orphans(&temp_dir()));
        let recovered = session.is_some();
        if let Some(session) = session {
            self.enter_take(session);
        }
        self.notice = recovery_notice(recovered, &failed);
    }

    fn refresh_device(&mut self) {
        self.device = default_output_name();
        self.device_checked = Some(Instant::now());
    }

    /// Remembers where the window is and how big, unless it is minimized or maximized.
    fn remember_window(&mut self, ctx: &egui::Context) {
        let (inner, outer, minimized, maximized) = ctx.input(|i| {
            let v = i.viewport();
            (v.inner_rect, v.outer_rect, v.minimized, v.maximized)
        });
        if minimized == Some(true) || maximized == Some(true) {
            return;
        }
        if let Some(r) = inner
            && r.width() > 0.0
            && r.height() > 0.0
        {
            self.window_size = Some([r.width(), r.height()]);
        }
        if let Some(r) = outer {
            self.window_pos = Some([r.left(), r.top()]);
        }
    }

    fn enter_take(&mut self, session: Session) {
        self.mode = Mode::Take(Box::new(TakeEditor::new(session)));
    }

    fn take_facts(&self) -> Option<TakeFacts> {
        match &self.mode {
            Mode::Take(t) => Some(TakeFacts { saved: t.saved(), playing: t.playing() }),
            _ => None,
        }
    }

    /// Moves the recorder out and leaves Ready; any other mode is left untouched.
    fn take_recorder(&mut self) -> Option<Recorder> {
        match std::mem::replace(&mut self.mode, Mode::Ready) {
            Mode::Recording(rec) => Some(rec),
            other => {
                self.mode = other;
                None
            }
        }
    }

    /// Moves the take out and leaves Ready; any other mode is left untouched.
    fn take_editor(&mut self) -> Option<TakeEditor> {
        match std::mem::replace(&mut self.mode, Mode::Ready) {
            Mode::Take(take) => Some(*take),
            other => {
                self.mode = other;
                None
            }
        }
    }

    fn handle(&mut self, action: BarAction, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.preparing.is_some() || self.opening.is_some() {
            return;
        }
        if self.notice.as_ref().is_some_and(|n| n.kind == NoticeKind::Error) {
            self.notice = None;
        }
        picker_on_action(&mut self.picker, action);
        match action {
            BarAction::Record => self.request(Destructive::Record, ctx),
            BarAction::Stop => self.stop_recording(),
            BarAction::Discard => self.request(Destructive::Discard, ctx),
            BarAction::Play => self.toggle_play(),
            BarAction::Save => self.save(frame),
            BarAction::OpenSource if self.picker.is_open() => self.picker.close(),
            BarAction::OpenSource => self.picker.open(),
            BarAction::Rewind => self.rewind(),
            BarAction::Loop => {
                if let Mode::Take(take) = &mut self.mode {
                    take.toggle_loop();
                }
            }
            BarAction::Download => self.download(),
            BarAction::ZeroCrossing => self.snap_to_zero(),
            BarAction::TrimSilence => self.trim_silence(),
            BarAction::PitchDown | BarAction::PitchUp | BarAction::PitchReset | BarAction::Reverse => {
                self.change_variant(action);
            }
        }
    }

    /// Takes the file dropped on the window this pass, if any. Of several, the first.
    fn receive_drop(&mut self, ctx: &egui::Context) {
        let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) else {
            return;
        };
        if is_own_drop(&path, self.own_drag.as_ref(), Instant::now()) {
            self.own_drag = None;
            return;
        }
        let recording = matches!(self.mode, Mode::Recording(_)) || self.preparing.is_some();
        let waiting = self.confirm.is_some() || self.opening.is_some();
        let same_file = matches!(&self.mode, Mode::Take(take) if take.holds_file(&path));
        match drop_outcome(recording, waiting, same_file) {
            DropOutcome::Open => {
                self.to_open = Some(path);
                self.request(Destructive::Open, ctx);
            }
            DropOutcome::Refuse => self.notice = Notice::info(STOP_RECORDING_FIRST),
            DropOutcome::Ignore => {}
        }
    }

    /// Opens the file on its own thread: a big one takes a while to read, and the window keeps
    /// painting. `poll_opening` picks up the result.
    fn start_opening(&mut self, path: PathBuf) {
        if let Mode::Take(take) = &mut self.mode {
            take.stop_playback();
        }
        let target = path.clone();
        let work = move || Session::open_existing(&target).map_err(|e| open_error_text(&target, &e.to_string()));
        let lost = open_error_text(&path, "el hilo que lo abre terminó de forma inesperada");
        match Pending::spawn("file-open", &lost, work) {
            Ok(pending) => {
                self.notice = None;
                self.opening = Some((pending, path));
            }
            Err(e) => self.notice = Notice::error(e),
        }
    }

    /// Takes the file that was opening, once it is ready or has failed. The take that was open
    /// closes only now, so a failure leaves it as it was.
    fn poll_opening(&mut self) {
        let Some(result) = self.opening.as_ref().and_then(|(pending, _)| pending.poll()) else { return };
        let Some((_, path)) = self.opening.take() else { return };
        match result {
            Err(e) => self.notice = Notice::error(e),
            Ok(session) => {
                let previous = std::mem::replace(&mut self.mode, Mode::Ready);
                self.enter_take(session);
                if let Mode::Take(take) = &mut self.mode {
                    take.set_sample_prefix(&prefix_of(&path));
                }
                self.notice = None;
                if let Mode::Take(take) = previous {
                    self.close_take(*take);
                }
            }
        }
    }

    /// Moves the handles of the open take to their nearest zero crossing.
    fn snap_to_zero(&mut self) {
        let Mode::Take(take) = &mut self.mode else { return };
        if let Err(e) = take.snap_to_zero() {
            self.notice = Notice::error(e);
        }
    }

    /// Moves the handles of the open take to the sound, or says that there is none.
    fn trim_silence(&mut self) {
        let Mode::Take(take) = &mut self.mode else { return };
        match take.trim_silence() {
            Ok(true) => {}
            Ok(false) => self.notice = Notice::info(ONLY_SILENCE),
            Err(e) => self.notice = Notice::error(e),
        }
    }

    /// Applies a variant control or key to the open take; without one they do nothing.
    fn change_variant(&mut self, action: BarAction) {
        let Mode::Take(take) = &mut self.mode else { return };
        let variant = take_row::after(action, take.variant());
        if let Err(e) = take.set_variant(variant) {
            self.notice = Notice::error(e);
        }
    }

    /// Runs the request, asks first when it would lose an unsaved take, or ignores it.
    fn request(&mut self, action: Destructive, ctx: &egui::Context) {
        match gate(action, self.take_facts(), self.device.is_some()) {
            Gate::Run => self.run(action, ctx),
            Gate::Ask => self.confirm = Some(action),
            Gate::Ignore => {}
        }
    }

    fn run(&mut self, action: Destructive, ctx: &egui::Context) {
        match action {
            Destructive::Record => self.start_recording(),
            Destructive::Discard => self.discard_take(),
            Destructive::Open => {
                if let Some(path) = self.to_open.take() {
                    self.start_opening(path);
                }
            }
            Destructive::Close => {
                self.discard_take();
                self.allow_close = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    /// Starts the capture on its own thread: it takes a moment, and the window keeps
    /// painting. `poll_preparing` picks up the result.
    fn start_recording(&mut self) {
        let playing = self.take_facts().is_some_and(|t| t.playing);
        if matches!(self.mode, Mode::Recording(_)) || self.preparing.is_some() || self.device.is_none() || playing {
            return;
        }
        let source = match &self.source {
            None => CaptureSource::SystemExceptSelf,
            Some(exe) => match sources::resolve(exe) {
                Some(pid) => CaptureSource::Process(pid),
                None => {
                    self.notice = Notice::error(app_gone_text(&self.source_name));
                    self.choose(None);
                    return;
                }
            },
        };
        let dir = temp_dir();
        let lost = format!("{CAPTURE_FAILED} (el hilo que la inicia terminó de forma inesperada).");
        match Pending::spawn("recording-start", &lost, move || Recorder::start(&dir, source)) {
            Ok(pending) => {
                self.notice = None;
                self.preparing = Some(pending);
            }
            Err(e) => self.notice = Notice::error(e),
        }
    }

    /// Takes the recording that was starting, once it is ready or has failed.
    fn poll_preparing(&mut self) {
        let Some(result) = self.preparing.as_ref().and_then(Pending::poll) else { return };
        self.preparing = None;
        match result {
            Err(e) => {
                let app = self.source.as_ref().map(|_| self.source_name.as_str());
                self.notice = Notice::error(start_error_text(app, &e));
            }
            Ok(rec) => {
                self.volume_independent = Some(rec.volume_independent());
                let previous = std::mem::replace(&mut self.mode, Mode::Recording(rec));
                self.notice = None;
                if let Mode::Take(take) = previous {
                    self.close_take(*take);
                }
            }
        }
    }

    /// Stops the recording (by the user, or after it ended by itself) and opens the take.
    fn stop_recording(&mut self) {
        let Some(rec) = self.take_recorder() else { return };
        self.device_checked = None;
        let take = match rec.stop() {
            Ok(take) => take,
            Err(e) => {
                self.notice = Notice::error(e);
                return;
            }
        };
        let ended = end_text(&take.end);
        match Session::from_recording(take) {
            Ok(session) => {
                self.enter_take(session);
                // The source cannot change while recording: it is the one recorded.
                if let (Mode::Take(take), Some(_)) = (&mut self.mode, &self.source) {
                    take.set_sample_prefix(&self.source_name);
                }
                self.notice = ended.and_then(Notice::error);
            }
            Err(SessionError::Empty) => self.notice = empty_take_notice(ended),
            Err(e) => self.notice = Notice::error(format!("No se pudo abrir la toma: {e}")),
        }
    }

    fn discard_take(&mut self) {
        let Some(take) = self.take_editor() else { return };
        self.notice = None;
        self.close_take(take);
    }

    fn close_take(&mut self, take: TakeEditor) {
        if let Err(e) = take.close() {
            self.notice = Notice::error(format!("No se pudo borrar el archivo temporal: {e}"));
        }
    }

    fn toggle_play(&mut self) {
        if let Mode::Take(take) = &mut self.mode {
            match take.toggle_play() {
                Ok(()) => self.notice = None,
                Err(e) => self.notice = Notice::error(e),
            }
        }
    }

    fn rewind(&mut self) {
        if let Mode::Take(take) = &mut self.mode
            && let Err(e) = take.rewind()
        {
            self.notice = Notice::error(e);
        }
    }

    /// Writes the selection into the samples folder, as a drag does, and says where.
    fn download(&mut self) {
        let Mode::Take(take) = &mut self.mode else { return };
        self.notice = match take.download(&samples_dir(), &now_local()) {
            Ok(path) => Notice::info(saved_sample_text(&path)),
            Err(e) => Notice::error(e),
        };
    }

    /// Saves the selection; on success remembers the folder at once.
    fn save(&mut self, frame: &eframe::Frame) {
        let Mode::Take(take) = &mut self.mode else { return };
        match take.save(frame, self.settings.last_folder.as_deref()) {
            Ok(None) => {}
            Ok(Some(dest)) => {
                self.notice = None;
                self.settings.last_folder = dest.parent().map(|p| p.to_path_buf());
                let _ = self.settings.save_to(&Settings::path());
            }
            Err(e) => self.notice = Notice::error(e),
        }
    }

    /// A close request with an unsaved take is cancelled and turned into the question.
    fn intercept_close(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        let ask = gate(Destructive::Close, self.take_facts(), self.device.is_some()) == Gate::Ask;
        if !requested || self.allow_close || !ask {
            return;
        }
        ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        self.confirm = Some(Destructive::Close);
        // A close from the taskbar may come while minimized: show the question.
        if ctx.input(|i| i.viewport().minimized == Some(true)) {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
    }

    /// Makes `exe` (or the whole system) this session's source.
    fn choose(&mut self, exe: Option<String>) {
        self.source_name = exe.as_deref().map_or_else(|| SYSTEM_NAME.into(), sources::app_name);
        self.source = exe;
    }

    /// A choice in the list: the new source, remembered for the next start.
    fn pick(&mut self, choice: Choice) {
        let exe = match choice {
            Choice::System => None,
            Choice::App(exe) => Some(exe),
        };
        self.settings.source = exe.clone();
        let _ = self.settings.save_to(&Settings::path());
        self.choose(exe);
    }

    fn bar_state(&self, icon: Option<egui::TextureId>) -> BarState {
        let device = self.device.clone().unwrap_or_default();
        let (mode, playing, time, device) = match &self.mode {
            _ if self.preparing.is_some() => (BarMode::Preparing, false, fmt_time(0, 1), device),
            Mode::Ready => (BarMode::Ready, false, fmt_time(0, 1), device),
            Mode::Recording(rec) => {
                let time = fmt_time(rec.status().frames, rec.format().sample_rate());
                (BarMode::Recording, false, time, rec.device_name().to_string())
            }
            Mode::Take(t) => (BarMode::Take, t.playing(), t.time_text(), device),
        };
        let looping = matches!(&self.mode, Mode::Take(t) if t.looping());
        let recording = mode == BarMode::Recording;
        let can_record =
            matches!(mode, BarMode::Ready | BarMode::Take) && !playing && self.device.is_some() && self.opening.is_none();
        let source = SourceView {
            name: self.source_name.clone(),
            icon,
            locked: recording || mode == BarMode::Preparing,
            available: self.single_app,
            system: self.source.is_none(),
        };
        BarState { mode, playing, looping, can_record, time, source, device }
    }

    fn central(&mut self, ui: &mut Ui, interactive: bool, veiled: bool) -> Option<TakeEvent> {
        let empty = matches!(self.mode, Mode::Ready) && !veiled;
        let ready = theme::transition(ui.ctx(), Id::unique("ready-hint"), empty, READY_SECS, 0.0);
        match &mut self.mode {
            Mode::Ready => ready_view(ui, self.device.is_some(), self.preparing.is_some(), ready),
            Mode::Recording(rec) => recording_view(ui, rec),
            Mode::Take(take) => return take.show(ui, interactive),
        }
        None
    }

    /// Writes the selection, with its variant, into the samples folder and drags that file
    /// with Windows drag and drop. Dropping the whole take as it was recorded somewhere counts
    /// as saving it; a part or a variant of it does not.
    fn drag_out(&mut self, ctx: &egui::Context) {
        let Mode::Take(take) = &mut self.mode else { return };
        self.notice = None;
        let prefix = take.sample_prefix().to_string();
        let sample = take.export_selection(&samples_dir(), &prefix, &now_local());
        let own_drag = &mut self.own_drag;
        let dragged = sample.and_then(|path| {
            let result = drag_file(&path);
            *own_drag = Some((path, Instant::now()));
            result
        });
        match dragged {
            Ok(dropped) if drag_saves_take(dropped, take.sample_is_the_take()) => take.mark_saved(),
            Ok(_) => {}
            Err(e) => self.notice = Notice::error(e),
        }
        // Windows ran the drag in its own loop and swallowed the button release: forget the
        // press so egui does not keep dragging.
        ctx.stop_dragging();
        ctx.input_mut(|i| i.pointer = Default::default());
    }

    /// The one notice line above the bar: an error first, then a warning that still
    /// applies, then information that has not faded.
    fn notice_line(&self) -> Option<(String, NoticeKind)> {
        let error = self.notice.as_ref().filter(|n| n.kind == NoticeKind::Error).map(|n| (n.text.clone(), n.kind));
        let info = self
            .notice
            .as_ref()
            .filter(|n| n.kind != NoticeKind::Error && notice_visible(n.kind, n.shown.elapsed()))
            .map(|n| (n.text.clone(), n.kind));
        let volume = match &self.mode {
            Mode::Recording(rec) => Some(rec.volume_independent()),
            Mode::Ready => self.volume_independent,
            Mode::Take(_) => None,
        };
        let backlog = match &self.mode {
            Mode::Recording(rec) => Some(rec.status().backlog_secs).filter(|s| *s > BACKLOG_WARN_SECS),
            _ => None,
        };
        let warning = volume_notice(volume).map(str::to_string).or(backlog.map(backlog_text));
        pick_notice(error, info, warning)
    }
}

impl eframe::App for TeethApp {
    /// Black from the first frame, before any panel paints.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    /// Runs every pass, also while the window is hidden: device refresh, automatic end of
    /// a recording, the end of playback, close requests and repaint scheduling.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.remember_window(ctx);
        if self.device_checked.is_none_or(|t| t.elapsed() >= DEVICE_REFRESH) {
            self.refresh_device();
        }
        if matches!(&self.mode, Mode::Recording(rec) if rec.has_ended()) {
            self.stop_recording();
        }
        self.poll_preparing();
        self.poll_opening();
        self.receive_drop(ctx);
        if let Mode::Take(take) = &mut self.mode
            && let Some(e) = take.poll()
        {
            self.notice = Notice::error(e);
        }
        if let Some(n) = &self.notice
            && n.kind == NoticeKind::Info
        {
            match INFO_LIFETIME.checked_sub(n.shown.elapsed()) {
                Some(left) => ctx.request_repaint_after(left),
                None => self.notice = None,
            }
        }
        self.intercept_close(ctx);
        let pace = match &self.mode {
            _ if self.preparing.is_some() || self.opening.is_some() => Some(LIVE_REPAINT),
            Mode::Recording(_) => Some(LIVE_REPAINT),
            Mode::Take(t) if t.playing() => Some(t.paint_pace()),
            _ => None,
        };
        let minimized = ctx.input(|i| i.viewport().minimized == Some(true));
        ctx.request_repaint_after(repaint_interval(pace, minimized));
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let splash_up = self.show_splash(&ctx);
        let icon = match self.source.clone() {
            Some(exe) => self.picker.icon(&ctx, &exe, || sources::app_path(&exe)),
            None => None,
        };
        let state = self.bar_state(icon);
        let recording = state.mode == BarMode::Recording;
        let interactive =
            !splash_up && self.confirm.is_none() && self.preparing.is_none() && self.opening.is_none();
        let hovering = !recording && self.confirm.is_none() && ctx.input(|i| !i.raw.hovered_files.is_empty());
        let veil_says = veil_text(
            self.preparing.is_some() && matches!(self.mode, Mode::Take(_)),
            self.opening.is_some(),
            hovering,
        );
        // As the bar sees it: not while a recording is starting.
        let in_take = state.mode == BarMode::Take;
        let full = ui.max_rect();
        let bar_rect = bar::bar_rect(full);
        let content = Rect::from_min_max(
            pos2(full.left() + bar::MARGIN, full.top() + bar::MARGIN),
            pos2(full.right() - bar::MARGIN, bar_rect.top() - NOTICE_ROOM),
        );
        let event = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let mut content_ui = ui.new_child(UiBuilder::new().max_rect(content));
                self.central(&mut content_ui, interactive, veil_says.is_some())
            })
            .inner;
        if let Some(TakeEvent::DragOut { .. }) = event {
            self.drag_out(&ctx);
        }
        let veil = veil_says.is_some();
        let veil = theme::transition(&ctx, Id::unique("window-veil"), veil, VEIL_SECS, 0.0);
        if let Some(text) = veil_says.filter(|_| veil > 0.0) {
            window_veil(ui, content, text, veil);
        }
        let line = self.notice_line();
        let fade = theme::transition(&ctx, Id::unique("notice-fade"), line.is_some(), NOTICE_IN_SECS, NOTICE_OUT_SECS);
        if line.is_some() {
            self.shown_notice = line;
        }
        if fade > 0.0
            && let Some((text, kind)) = &self.shown_notice
        {
            let color = match kind {
                NoticeKind::Error => theme::DANGER,
                NoticeKind::Warning => theme::WARNING,
                NoticeKind::Info => theme::TEXT,
            }
            .gamma_multiply(fade);
            let galley = notice_galley(ui.painter(), text, color, notice_width(bar_rect.width(), in_take));
            let top = bar_rect.top() - NOTICE_ROOM / 2.0 - galley.size().y / 2.0;
            let rect = Rect::from_min_size(pos2(bar_rect.left() + 2.0, top), galley.size());
            let cut = galley.elided;
            ui.painter().galley(rect.min, galley, color);
            if cut {
                ui.interact(rect, Id::unique("notice"), Sense::hover()).on_hover_text(text.as_str());
            }
        }
        let clicked = egui::Area::new(Id::unique("bar"))
            .order(Order::Foreground)
            .fixed_pos(bar_rect.min)
            .show(&ctx, |ui| {
                ui.set_min_size(bar_rect.size());
                bar::show(ui, bar_rect, &state)
            })
            .inner;
        let variant = match &self.mode {
            Mode::Take(take) => take.variant(),
            _ => self.shown_variant,
        };
        self.shown_variant = variant;
        let controls_t = theme::transition(&ctx, Id::unique("take-row-fade"), in_take, bar::TAKE_SECS, bar::TAKE_SECS);
        let row_click = if controls_t > 0.0 {
            let row = take_row::row_rect(bar_rect);
            egui::Area::new(Id::unique("take-row"))
                .order(Order::Foreground)
                .fixed_pos(row.min)
                .show(&ctx, |ui| {
                    ui.set_min_size(row.size());
                    ui.multiply_opacity(controls_t);
                    take_row::show(ui, bar_rect, variant, interactive && in_take)
                })
                .inner
        } else {
            None
        };
        if let Some(choice) = self.picker.show(&ctx, bar::source_rect(bar_rect), self.source.as_deref()) {
            self.pick(choice);
        }
        let key = if interactive { shortcut(&ctx, recording).filter(|action| key_applies(*action, in_take)) } else { None };
        if let Some(action) = clicked.or(row_click).or(key) {
            self.handle(action, &ctx, frame);
            ctx.request_repaint();
        }
        if let Some(pending) = self.confirm {
            match confirm_modal(&ctx) {
                Some(true) => {
                    self.confirm = None;
                    self.run(pending, &ctx);
                }
                Some(false) => self.confirm = None,
                None => {}
            }
        }
    }

    /// Stops the sound. A recording in progress is finished and left in the temp folder,
    /// like an unsaved take, so the next start recovers it; a saved take is closed.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(Ok(rec)) = self.preparing.take().and_then(|p| p.wait(PREPARING_TIMEOUT)) {
            let _ = rec.stop();
        }
        match std::mem::replace(&mut self.mode, Mode::Ready) {
            Mode::Recording(rec) => {
                let _ = rec.stop();
            }
            Mode::Take(take) => (*take).exit(),
            Mode::Ready => {}
        }
        if self.window_size.is_some() {
            self.settings.window_size = self.window_size;
        }
        if self.window_pos.is_some() {
            self.settings.window_pos = self.window_pos;
        }
        let _ = self.settings.save_to(&Settings::path());
    }
}

/// When to paint again by itself, given the `pace` of what moves on screen, if anything does.
fn repaint_interval(pace: Option<Duration>, minimized: bool) -> Duration {
    match (pace, minimized) {
        (Some(pace), false) => pace,
        (Some(_), true) => HIDDEN_REPAINT,
        (None, _) => DEVICE_REFRESH,
    }
}

/// Whether a request runs, asks first or does nothing. `take` describes the open take.
/// Recording is off without an output device, and while the take plays (the
/// endpoint-loopback fallback would capture the playback).
fn gate(action: Destructive, take: Option<TakeFacts>, has_device: bool) -> Gate {
    match (action, take) {
        (Destructive::Record, _) if !has_device => Gate::Ignore,
        (Destructive::Record, Some(t)) if t.playing => Gate::Ignore,
        (Destructive::Discard, None) => Gate::Ignore,
        (_, Some(t)) if !t.saved => Gate::Ask,
        _ => Gate::Run,
    }
}

/// `Some(true)` for `Descartar`; `Some(false)` for `Cancelar`, Escape or a click outside.
fn confirm_modal(ctx: &egui::Context) -> Option<bool> {
    let frame = egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0, theme::HAIRLINE))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(32));
    let modal = egui::Modal::new(egui::Id::unique("confirm-discard")).frame(frame).show(ctx, |ui| {
        ui.set_width(400.0 - 64.0);
        ui.label(RichText::new("¿Descartar la toma sin guardar?").font(theme::font(15.0, theme::Weight::Medium)));
        ui.add_space(20.0);
        ui.horizontal(|ui| {
            let discard = ui.button(RichText::new("Descartar").color(RED)).clicked();
            let cancel = ui.button(RichText::new("Cancelar").color(theme::TEXT_SUPPORT)).clicked();
            (discard || cancel).then_some(discard)
        })
        .inner
    });
    modal.inner.or_else(|| modal.should_close().then_some(false))
}

/// The one notice to show: an error, else information that has not faded (it lasts only
/// 4 s), else a warning that still applies.
fn pick_notice(
    error: Option<(String, NoticeKind)>,
    info: Option<(String, NoticeKind)>,
    warning: Option<String>,
) -> Option<(String, NoticeKind)> {
    error.or(info).or(warning.map(|w| (w, NoticeKind::Warning)))
}

/// Whether the key of `action` does anything: the variant keys need a take, so that out of
/// one they do not even dismiss an error notice.
fn key_applies(action: BarAction, in_take: bool) -> bool {
    in_take
        || !matches!(
            action,
            BarAction::PitchDown
                | BarAction::PitchUp
                | BarAction::PitchReset
                | BarAction::Reverse
                | BarAction::ZeroCrossing
                | BarAction::TrimSilence
        )
}

/// How wide the notice line may be above a bar `bar_width` wide: the bar's width less the
/// 2 points it starts and ends inside, and in a take the variant controls with a gap.
fn notice_width(bar_width: f32, in_take: bool) -> f32 {
    let width = bar_width - 4.0;
    if in_take { width - take_row::ROW_WIDTH - CONTROLS_GAP } else { width }
}

/// The notice's text on one line of at most `max_w`; what does not fit is cut.
fn notice_galley(painter: &egui::Painter, text: &str, color: Color32, max_w: f32) -> Arc<Galley> {
    bar::one_line(painter, text, theme::font(13.0, theme::Weight::Regular), color, max_w)
}

/// What a file dropped on the window does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropOutcome {
    Open,
    /// Not while recording: the notice says why.
    Refuse,
    Ignore,
}

/// `same_file` is the file of the open take: reopening it could lose it, because closing the
/// take deletes its temp file.
fn drop_outcome(recording: bool, waiting: bool, same_file: bool) -> DropOutcome {
    if recording {
        DropOutcome::Refuse
    } else if waiting || same_file {
        DropOutcome::Ignore
    } else {
        DropOutcome::Open
    }
}

/// Whether `path` is the sample of a drag of the tab that has just ended. Windows hands it to
/// the window too when the tab is dropped back on it.
fn is_own_drop(path: &Path, own_drag: Option<&(PathBuf, Instant)>, now: Instant) -> bool {
    own_drag.is_some_and(|(sample, ended)| sample == path && now.duration_since(*ended) < OWN_DROP_WINDOW)
}

/// The name of an opened file, as the prefix of its samples.
fn prefix_of(path: &Path) -> String {
    path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `No se pudo abrir {nombre}: {detalle}`.
fn open_error_text(path: &Path, detail: &str) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    format!("No se pudo abrir {name}: {detail}")
}

/// What the veil over the content says, if there is one: a recording starting over a take, a
/// file opening, or a file on its way to being dropped.
fn veil_text(preparing_over_take: bool, opening: bool, hovering: bool) -> Option<&'static str> {
    if preparing_over_take {
        Some(PREPARING)
    } else if opening {
        Some(OPENING)
    } else if hovering {
        Some(DROP_HERE)
    } else {
        None
    }
}

/// Whether a finished drag saves the take: only the whole of it, as it was recorded, dropped
/// somewhere.
fn drag_saves_take(dropped: bool, whole_and_faithful: bool) -> bool {
    dropped && whole_and_faithful
}

/// Closes the source list when a recording starts or stops.
fn picker_on_action(picker: &mut SourcePicker, action: BarAction) {
    if matches!(action, BarAction::Record | BarAction::Stop) {
        picker.close();
    }
}

/// `Guardado en Samples: {nombre}`, with the sample's file name.
fn saved_sample_text(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    format!("Guardado en Samples: {name}")
}

fn app_gone_text(name: &str) -> String {
    format!("{name} ya no está abierta. Elige otra fuente.")
}

fn app_capture_error_text(name: &str, detail: &str) -> String {
    format!("No se pudo grabar solo {name} ({detail}).")
}

/// What a failed start says: a capture that fails with an app chosen names the app, and
/// anything else (the disk, a thread) is not the app's fault.
fn start_error_text(app: Option<&str>, e: &str) -> String {
    match app {
        Some(name) if e.starts_with(CAPTURE_FAILED) => app_capture_error_text(name, capture_detail(e)),
        _ => e.to_string(),
    }
}

/// The detail inside the parentheses of a capture error, or the whole text.
fn capture_detail(e: &str) -> &str {
    match (e.find('('), e.rfind(')')) {
        (Some(a), Some(b)) if a < b => &e[a + 1..b],
        _ => e,
    }
}

/// Why a recording stopped by itself; `None` when the user stopped it.
fn end_text(end: &EndReason) -> Option<String> {
    Some(match end {
        EndReason::Stopped => return None,
        EndReason::DeviceChanged => {
            "Grabación detenida: el dispositivo de salida cambió. Se conservó lo grabado.".to_string()
        }
        EndReason::DeviceLost => "Grabación detenida: el dispositivo de salida dejó de estar \
                                  disponible. Se conservó lo grabado."
            .to_string(),
        EndReason::DiskError(d) => {
            format!("Grabación detenida: no se pudo escribir en el disco ({d}). Se conservó lo grabado.")
        }
        EndReason::CaptureError(d) => {
            format!("Grabación detenida: error de audio ({d}). Se conservó lo grabado.")
        }
    })
}

/// Notice for a recording that held no audio: why it ended when it stopped by itself,
/// otherwise `La toma está vacía.`
fn empty_take_notice(ended: Option<String>) -> Option<Notice> {
    match ended {
        Some(reason) => Notice::error(reason),
        None => Notice::info("La toma está vacía."),
    }
}

/// Paints the splash screen while it lasts. `true` when it was up at the start of this frame,
/// so the app underneath takes none of this frame's keys, not even the one that skipped it:
/// an R pressed to skip it does not start a recording. A key, a click on it or `end` ends it.
/// When it ends, its texture is freed and information notices start their time.
fn splash_pass(
    ctx: &egui::Context,
    splash: &mut Option<Splash>,
    mark: &mut Option<egui::TextureHandle>,
    notice: &mut Option<Notice>,
    end: bool,
) -> bool {
    let Some(shown) = splash else { return false };
    if mark.is_none() {
        let Some(image) = splash::mark_image(include_bytes!("../../assets/icon/teeth-mark.png")) else {
            *splash = None;
            return false;
        };
        let options = egui::TextureOptions::LINEAR.with_mipmap_mode(Some(egui::TextureFilter::Linear));
        *mark = Some(ctx.load_texture("teeth-mark", image, options));
    }
    let key = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
    let now = ctx.input(|i| i.time);
    match (shown.frame(now, theme::animated(ctx), key || end), mark.as_ref()) {
        (Some(frame), Some(texture)) => {
            if splash::show(ctx, texture, frame) {
                shown.frame(now, true, true);
            }
            ctx.request_repaint();
        }
        _ => {
            *splash = None;
            *mark = None;
            restart_after_splash(notice);
        }
    }
    true
}

/// Information shown while the splash screen covered it, such as the recovered take, starts
/// its time when the splash screen ends, so it is not gone before it is seen.
fn restart_after_splash(notice: &mut Option<Notice>) {
    if let Some(n) = notice
        && n.kind == NoticeKind::Info
    {
        n.shown = Instant::now();
    }
}

/// Notice after startup recovery: one line per orphan that could not be opened, then
/// whether a take was recovered.
fn recovery_notice(recovered: bool, failed: &[(PathBuf, SessionError)]) -> Option<Notice> {
    let mut lines: Vec<String> =
        failed.iter().map(|(p, e)| format!("No se pudo recuperar {}: {e}", p.display())).collect();
    if recovered {
        lines.push("Se recuperó una toma sin guardar".to_string());
    }
    match (failed.is_empty(), recovered) {
        (true, false) => None,
        (true, true) => Notice::info(lines.join("\n")),
        (false, _) => Notice::error(lines.join("\n")),
    }
}

/// The volume warning, only when the capture is known to follow the Windows volume.
fn volume_notice(volume_independent: Option<bool>) -> Option<&'static str> {
    (volume_independent == Some(false)).then_some(VOLUME_DEPENDENT)
}

fn backlog_text(secs: f32) -> String {
    format!("El disco no da abasto: la escritura va {} s por detrás.", secs as u64)
}

/// The line in the middle of the empty window, and its color.
fn ready_text(has_device: bool, preparing: bool) -> (&'static str, Color32) {
    match (preparing, has_device) {
        (true, _) => (PREPARING, theme::TEXT_QUIET),
        (false, true) => (READY_HINT, theme::TEXT_QUIET),
        (false, false) => ("No hay dispositivo de salida de audio.", theme::WARNING),
    }
}

fn ready_view(ui: &mut Ui, has_device: bool, preparing: bool, opacity: f32) {
    let (rect, _) = wave::area(ui, Vec::new(), Sense::hover());
    let (text, color) = ready_text(has_device, preparing);
    let font = theme::font(13.0, theme::Weight::Regular);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, font, color.gamma_multiply(opacity));
}

/// Dims the content and says `text` over it.
fn window_veil(ui: &Ui, content: Rect, text: &str, opacity: f32) {
    let painter = ui.painter();
    painter.rect_filled(content, 0.0, Color32::from_black_alpha(170).gamma_multiply(opacity));
    let font = theme::font(13.0, theme::Weight::Regular);
    let color = theme::TEXT_SUPPORT.gamma_multiply(opacity);
    painter.text(content.center(), Align2::CENTER_CENTER, text, font, color);
}

fn recording_view(ui: &mut Ui, rec: &Recorder) {
    let (rect, _) = wave::area(ui, Vec::new(), Sense::hover());
    let (lanes, view) = {
        let peaks = rec.shared().peaks.lock().unwrap_or_else(|e| e.into_inner());
        let mut view = View::new(0);
        view.set_total(peaks.frames());
        (peak_lanes(&peaks, &view, rect.width()), view)
    };
    let painter = ui.painter_at(rect);
    wave::draw_columns(&painter, rect, &lanes, None);
    let cuts = rec.shared().cuts.lock().unwrap_or_else(|e| e.into_inner()).clone();
    wave::draw_cuts(&painter, rect, &wave::visible_cut_xs(&cuts, &view, rect.width()));
}

#[cfg(test)]
mod tests;
