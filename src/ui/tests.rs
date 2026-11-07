//! Unit tests of the window shell: end texts, notices and confirmation gating.

use super::*;

#[test]
fn end_texts_match_the_spec() {
    assert_eq!(end_text(&EndReason::Stopped), None);
    assert_eq!(
        end_text(&EndReason::DeviceChanged).unwrap(),
        "Grabación detenida: el dispositivo de salida cambió. Se conservó lo grabado."
    );
    assert_eq!(
        end_text(&EndReason::DeviceLost).unwrap(),
        "Grabación detenida: el dispositivo de salida dejó de estar disponible. Se conservó lo grabado."
    );
    assert_eq!(
        end_text(&EndReason::DiskError("disco lleno".into())).unwrap(),
        "Grabación detenida: no se pudo escribir en el disco (disco lleno). Se conservó lo grabado."
    );
    assert_eq!(
        end_text(&EndReason::CaptureError("0x88890004".into())).unwrap(),
        "Grabación detenida: error de audio (0x88890004). Se conservó lo grabado."
    );
}

#[test]
fn volume_notice_only_when_the_capture_follows_the_volume() {
    assert_eq!(volume_notice(None), None);
    assert_eq!(volume_notice(Some(true)), None);
    assert_eq!(
        volume_notice(Some(false)),
        Some("Esta versión de Windows no permite grabar antes del volumen: la toma depende del volumen de Windows.")
    );
}

#[test]
fn backlog_text_shows_whole_seconds() {
    assert_eq!(backlog_text(12.7), "El disco no da abasto: la escritura va 12 s por detrás.");
}

const UNSAVED: Option<TakeFacts> = Some(TakeFacts { saved: false, playing: false });
const SAVED: Option<TakeFacts> = Some(TakeFacts { saved: true, playing: false });
const PLAYING_UNSAVED: Option<TakeFacts> = Some(TakeFacts { saved: false, playing: true });
const PLAYING_SAVED: Option<TakeFacts> = Some(TakeFacts { saved: true, playing: true });

#[test]
fn losing_an_unsaved_take_asks_first() {
    for action in [Destructive::Record, Destructive::Discard, Destructive::Close] {
        assert_eq!(gate(action, UNSAVED, true), Gate::Ask, "{action:?}");
    }
    assert_eq!(gate(Destructive::Discard, PLAYING_UNSAVED, true), Gate::Ask);
    assert_eq!(gate(Destructive::Close, PLAYING_UNSAVED, true), Gate::Ask);
}

#[test]
fn a_saved_take_or_no_take_runs_without_asking() {
    for action in [Destructive::Record, Destructive::Discard, Destructive::Close] {
        assert_eq!(gate(action, SAVED, true), Gate::Run, "{action:?}");
    }
    assert_eq!(gate(Destructive::Record, None, true), Gate::Run);
    assert_eq!(gate(Destructive::Close, None, true), Gate::Run);
    assert_eq!(gate(Destructive::Discard, None, true), Gate::Ignore);
}

#[test]
fn recording_is_off_while_playing() {
    assert_eq!(gate(Destructive::Record, PLAYING_UNSAVED, true), Gate::Ignore);
    assert_eq!(gate(Destructive::Record, PLAYING_SAVED, true), Gate::Ignore);
    assert_eq!(gate(Destructive::Discard, PLAYING_SAVED, true), Gate::Run);
}

#[test]
fn recording_without_an_output_device_is_ignored() {
    for take in [None, UNSAVED, SAVED, PLAYING_UNSAVED] {
        assert_eq!(gate(Destructive::Record, take, false), Gate::Ignore);
    }
    // Discarding and closing do not need a device.
    assert_eq!(gate(Destructive::Discard, UNSAVED, false), Gate::Ask);
    assert_eq!(gate(Destructive::Close, UNSAVED, false), Gate::Ask);
    assert_eq!(gate(Destructive::Discard, SAVED, false), Gate::Run);
}

#[test]
fn an_empty_take_shows_why_the_recording_ended() {
    let n = empty_take_notice(None).unwrap();
    assert_eq!((n.text.as_str(), n.kind), ("La toma está vacía.", NoticeKind::Info));
    let lost = end_text(&EndReason::DeviceLost);
    let n = empty_take_notice(lost.clone()).unwrap();
    assert_eq!((Some(n.text), n.kind), (lost, NoticeKind::Error));
}

#[test]
fn recovery_notice_reports_failures_and_success() {
    assert!(recovery_notice(false, &[]).is_none());
    let n = recovery_notice(true, &[]).unwrap();
    assert_eq!((n.text.as_str(), n.kind), ("Se recuperó una toma sin guardar", NoticeKind::Info));
    let failed = [(
        std::path::PathBuf::from(r"C:\Temp\Teeth\toma-1.wav"),
        SessionError::Wav(crate::wav::WavError::NotWav),
    )];
    let lost = r"No se pudo recuperar C:\Temp\Teeth\toma-1.wav: el archivo no es un WAV válido";
    let n = recovery_notice(true, &failed).unwrap();
    assert_eq!(n.kind, NoticeKind::Error);
    assert_eq!(n.text, format!("{lost}\nSe recuperó una toma sin guardar"));
    let n = recovery_notice(false, &failed).unwrap();
    assert_eq!((n.text.as_str(), n.kind), (lost, NoticeKind::Error));
}

#[test]
fn information_fades_after_four_seconds_and_errors_stay() {
    assert!(notice_visible(NoticeKind::Info, Duration::from_millis(3_900)));
    assert!(!notice_visible(NoticeKind::Info, Duration::from_millis(4_000)));
    assert!(notice_visible(NoticeKind::Error, Duration::from_secs(600)));
    assert!(notice_visible(NoticeKind::Warning, Duration::from_secs(600)));
}

#[test]
fn ready_hint_matches_the_spec() {
    assert_eq!(READY_HINT, "Pulsa R para grabar");
}

#[test]
fn ready_says_what_is_going_on() {
    assert_eq!(ready_text(true, false).0, "Pulsa R para grabar");
    assert_eq!(ready_text(true, true).0, "Preparando…");
    assert_eq!(ready_text(false, false).0, "No hay dispositivo de salida de audio.");
}

#[test]
fn it_repaints_at_the_pace_of_what_moves_on_screen() {
    let pace = Duration::from_millis(12);
    assert_eq!(repaint_interval(Some(pace), false), pace);
    assert_eq!(repaint_interval(Some(pace), true), HIDDEN_REPAINT);
    assert_eq!(repaint_interval(None, false), DEVICE_REFRESH);
    assert_eq!(repaint_interval(None, true), DEVICE_REFRESH);
}

#[test]
fn source_errors_match_the_spec() {
    assert_eq!(app_gone_text("Google Chrome"), "Google Chrome ya no está abierta. Elige otra fuente.");
    assert_eq!(app_capture_error_text("Google Chrome", "0x80070005"), "No se pudo grabar solo Google Chrome (0x80070005).");
}

#[test]
fn capture_details_come_from_the_parentheses() {
    assert_eq!(capture_detail("No se pudo iniciar la captura de audio (Activación: 0x8000). "), "Activación: 0x8000");
    assert_eq!(capture_detail("algo raro"), "algo raro");
}

#[test]
fn recording_closes_the_source_picker() {
    let mut picker = SourcePicker::default();
    picker.open_with(Vec::new());
    assert!(picker.is_open());
    picker_on_action(&mut picker, BarAction::Play);
    assert!(picker.is_open());
    picker_on_action(&mut picker, BarAction::Record);
    assert!(!picker.is_open());
}

#[test]
fn errors_win_then_fresh_information_then_warnings() {
    let error = Some(("e".to_string(), NoticeKind::Error));
    let info = Some(("i".to_string(), NoticeKind::Info));
    let warning = Some("w".to_string());
    assert_eq!(pick_notice(error.clone(), info.clone(), warning.clone()), error);
    assert_eq!(pick_notice(None, info.clone(), warning.clone()), info);
    assert_eq!(pick_notice(None, None, warning), Some(("w".to_string(), NoticeKind::Warning)));
    assert_eq!(pick_notice(None, None, None), None);
}

#[test]
fn a_downloaded_sample_names_its_file() {
    let path = std::path::Path::new(r"C:\Users\x\Documents\Teeth\Samples\Zen 2026-11-01 14-32-05.wav");
    assert_eq!(saved_sample_text(path), "Guardado en Samples: Zen 2026-11-01 14-32-05.wav");
}

#[test]
fn only_a_whole_take_dropped_somewhere_counts_as_saved() {
    assert!(drag_saves_take(true, true));
    assert!(!drag_saves_take(false, true), "cancelled, or dropped back on Teeth");
    assert!(!drag_saves_take(true, false), "a part or a variant of the take is not the take");
}

#[test]
fn only_capture_failures_blame_the_chosen_app() {
    let capture = "No se pudo iniciar la captura de audio (Activación: 0x8000).";
    assert_eq!(start_error_text(Some("Zen"), capture), "No se pudo grabar solo Zen (Activación: 0x8000).");
    assert_eq!(start_error_text(None, capture), capture);
    let disk = "No se pudo crear el archivo temporal C:\\x.wav: disco lleno";
    assert_eq!(start_error_text(Some("Zen"), disk), disk);
}

#[test]
fn in_a_take_the_notice_leaves_room_for_the_controls() {
    assert_eq!(notice_width(752.0, false), 748.0);
    assert_eq!(notice_width(752.0, true), 748.0 - take_row::ROW_WIDTH - 12.0);
}

#[test]
fn variant_keys_do_nothing_outside_a_take() {
    let variant = [BarAction::PitchDown, BarAction::PitchUp, BarAction::PitchReset, BarAction::Reverse];
    for action in variant {
        assert!(!key_applies(action, false), "{action:?}");
        assert!(key_applies(action, true), "{action:?}");
    }
    for action in [BarAction::Loop, BarAction::Record] {
        assert!(key_applies(action, false), "{action:?}");
        assert!(key_applies(action, true), "{action:?}");
    }
}

#[test]
fn a_notice_stays_on_one_line_inside_its_width() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let (mut size, mut rows) = (egui::Vec2::ZERO, 0);
    let output = ctx.run_ui(Default::default(), |ui| {
        let long = format!("{}\nsegunda línea", "No se pudo recuperar C:\\una\\ruta\\muy\\larga.wav ".repeat(9));
        let galley = notice_galley(ui.painter(), &long, theme::TEXT, 500.0);
        size = galley.size();
        rows = galley.rows.len();
    });
    output.drop_without_applying_deltas();
    assert_eq!(rows, 1);
    assert!(size.x <= 500.0, "{size:?}");
}

#[test]
fn the_keys_of_the_handles_do_nothing_outside_a_take() {
    for action in [BarAction::ZeroCrossing, BarAction::TrimSilence] {
        assert!(!key_applies(action, false), "{action:?}");
        assert!(key_applies(action, true), "{action:?}");
    }
}

#[test]
fn a_silent_selection_says_so() {
    assert_eq!(ONLY_SILENCE, "La selección es solo silencio.");
}

#[test]
fn opening_a_file_asks_only_over_an_unsaved_take() {
    let take = |saved, playing| Some(TakeFacts { saved, playing });
    assert_eq!(gate(Destructive::Open, take(false, false), true), Gate::Ask);
    assert_eq!(gate(Destructive::Open, take(false, true), true), Gate::Ask);
    assert_eq!(gate(Destructive::Open, take(true, false), true), Gate::Run);
    assert_eq!(gate(Destructive::Open, take(true, true), true), Gate::Run);
    assert_eq!(gate(Destructive::Open, None, true), Gate::Run);
    // Opening needs no output device.
    assert_eq!(gate(Destructive::Open, None, false), Gate::Run);
    assert_eq!(gate(Destructive::Open, take(false, false), false), Gate::Ask);
}

#[test]
fn the_tab_dropped_on_its_own_window_is_not_opened() {
    let now = Instant::now();
    let own = (PathBuf::from("sample.wav"), now);
    assert!(is_own_drop(Path::new("sample.wav"), Some(&own), now + Duration::from_millis(100)));
    assert!(!is_own_drop(Path::new("otro.wav"), Some(&own), now));
    assert!(!is_own_drop(Path::new("sample.wav"), Some(&own), now + OWN_DROP_WINDOW));
    assert!(!is_own_drop(Path::new("sample.wav"), None, now));
}

#[test]
fn what_a_dropped_file_does_depends_on_the_window() {
    assert_eq!(drop_outcome(false, false, false), DropOutcome::Open);
    assert_eq!(drop_outcome(true, false, false), DropOutcome::Refuse);
    assert_eq!(drop_outcome(true, true, true), DropOutcome::Refuse);
    assert_eq!(drop_outcome(false, true, false), DropOutcome::Ignore);
    assert_eq!(drop_outcome(false, false, true), DropOutcome::Ignore, "the file of the open take");
}

#[test]
fn samples_of_an_opened_file_take_its_name() {
    assert_eq!(prefix_of(Path::new("C:/x/kick 808.wav")), "kick 808");
    assert_eq!(prefix_of(Path::new("sin extension")), "sin extension");
}

#[test]
fn the_texts_of_opening_a_file_match_the_design() {
    assert_eq!(OPENING, "Abriendo…");
    assert_eq!(DROP_HERE, "Suelta para abrir");
    assert_eq!(STOP_RECORDING_FIRST, "Para la grabación antes de abrir un archivo.");
    assert_eq!(
        open_error_text(Path::new("C:/x/kick.wav"), "el archivo no es un WAV válido"),
        "No se pudo abrir kick.wav: el archivo no es un WAV válido"
    );
}

#[test]
fn the_veil_says_what_is_going_on() {
    assert_eq!(veil_text(true, false, false), Some(PREPARING));
    assert_eq!(veil_text(false, true, false), Some(OPENING));
    assert_eq!(veil_text(false, false, true), Some(DROP_HERE));
    assert_eq!(veil_text(false, true, true), Some(OPENING));
    assert_eq!(veil_text(false, false, false), None);
}

#[test]
fn an_info_notice_starts_its_time_when_the_splash_ends() {
    let mut notice = Notice::info("Se recuperó una toma sin guardar");
    let n = notice.as_mut().unwrap();
    n.shown = n.shown.checked_sub(Duration::from_secs(3)).unwrap();
    restart_after_splash(&mut notice);
    assert!(notice.unwrap().shown.elapsed() < Duration::from_secs(1));
}

#[test]
fn an_error_notice_keeps_its_time_after_the_splash() {
    let mut notice = Notice::error("No se pudo recuperar");
    let n = notice.as_mut().unwrap();
    let earlier = n.shown.checked_sub(Duration::from_secs(3)).unwrap();
    n.shown = earlier;
    restart_after_splash(&mut notice);
    assert_eq!(notice.unwrap().shown, earlier);
}

/// One headless pass of the splash screen over a window of 860 × 380, with `under`, a
/// clickable area covering the window below it, like the bar. Returns whether the splash
/// was up and whether `under` was clicked.
fn splash_frame(
    ctx: &egui::Context,
    i: usize,
    events: Vec<egui::Event>,
    splash: &mut Option<Splash>,
    mark: &mut Option<egui::TextureHandle>,
    notice: &mut Option<Notice>,
    end: bool,
) -> (bool, bool) {
    let screen = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(860.0, 380.0));
    let input = egui::RawInput { screen_rect: Some(screen), time: Some(i as f64 / 60.0), events, ..Default::default() };
    let (mut up, mut clicked) = (false, false);
    ctx.run_ui(input, |ui| {
        let ctx = ui.ctx().clone();
        clicked = egui::Area::new(Id::unique("under"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(&ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.interact(screen, Id::unique("under-click"), Sense::click()).clicked()
            })
            .inner;
        up = splash_pass(&ctx, splash, mark, notice, end);
    })
    .drop_without_applying_deltas();
    (up, clicked)
}

fn key_r() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::R,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn a_key_that_skips_the_splash_does_not_reach_the_app_and_frees_its_texture() {
    let ctx = egui::Context::default();
    let (mut splash, mut mark, mut notice) = (Some(Splash::new()), None, None);
    assert!(splash_frame(&ctx, 0, vec![], &mut splash, &mut mark, &mut notice, false).0);
    assert!(splash.is_some() && mark.is_some());
    assert!(splash_frame(&ctx, 1, vec![key_r()], &mut splash, &mut mark, &mut notice, false).0);
    assert!(splash.is_none(), "the key ends the splash");
    assert!(mark.is_none(), "its texture is freed");
    assert!(!splash_frame(&ctx, 2, vec![key_r()], &mut splash, &mut mark, &mut notice, false).0);
}

#[test]
fn a_click_on_the_splash_ends_it_and_does_not_reach_what_is_under_it() {
    let ctx = egui::Context::default();
    let (mut splash, mut mark, mut notice) = (Some(Splash::new()), None, None);
    let at = pos2(430.0, 300.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let passes = [vec![], vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], vec![]];
    let mut under = false;
    for (i, events) in passes.into_iter().enumerate() {
        under |= splash_frame(&ctx, i, events, &mut splash, &mut mark, &mut notice, false).1;
    }
    assert!(!under, "the click went through the splash");
    assert!(splash.is_none(), "the click ends the splash");
}

#[test]
fn a_question_ends_the_splash_so_it_can_be_seen() {
    let ctx = egui::Context::default();
    let (mut splash, mut mark, mut notice) = (Some(Splash::new()), None, None);
    splash_frame(&ctx, 0, vec![], &mut splash, &mut mark, &mut notice, false);
    assert!(splash.is_some());
    assert!(splash_frame(&ctx, 1, vec![], &mut splash, &mut mark, &mut notice, true).0);
    assert!(splash.is_none() && mark.is_none());
}
