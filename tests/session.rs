mod common;

use common::{XorShift, f32_bytes};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use teeth::peaks::PeakCache;
use teeth::recorder::RecordedTake;
use teeth::recorder::writer_loop::EndReason;
use teeth::session::{Session, SessionError};
use teeth::settings::Settings;
use teeth::wav::{WavError, WavFormat, WavReader, WavWriter};

fn fmt() -> WavFormat {
    WavFormat::float32_extensible(48_000, 2)
}

fn samples(frames: usize, seed: u64) -> Vec<f32> {
    let mut g = XorShift::new(seed);
    (0..frames * 2).map(|_| g.next_f32()).collect()
}

/// Finished WAV with `frames` stereo frames.
fn make_wav(path: &Path, frames: usize, seed: u64) {
    let mut w = WavWriter::create(path, &fmt()).unwrap();
    w.write_audio(&f32_bytes(&samples(frames, seed))).unwrap();
    w.finish().unwrap();
}

fn peaks_of(frames: usize, seed: u64) -> PeakCache {
    let mut p = PeakCache::new(2);
    p.push(&samples(frames, seed));
    p
}

fn take(path: &Path, frames: u64, peaks: PeakCache, end: EndReason) -> RecordedTake {
    RecordedTake { path: path.to_path_buf(), format: fmt(), frames, discontinuities: 0, cuts: Vec::new(), peaks, end }
}

fn data(path: &Path, start: u64, count: u64) -> Vec<u8> {
    WavReader::open(path).unwrap().read_frames(start, count).unwrap()
}

fn temp_session(dir: &Path, frames: usize) -> (Session, PathBuf) {
    let tmp = dir.join("toma-temp.wav");
    make_wav(&tmp, frames, 1);
    let rec = take(&tmp, frames as u64, peaks_of(frames, 1), EndReason::Stopped);
    (Session::from_recording(rec).unwrap(), tmp)
}

#[test]
fn empty_recording_is_rejected_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().join("empty.wav");
    WavWriter::create(&tmp, &fmt()).unwrap().finish().unwrap();
    let rec = take(&tmp, 0, PeakCache::new(2), EndReason::Stopped);
    assert!(matches!(Session::from_recording(rec), Err(SessionError::Empty)));
    assert!(!tmp.exists());
}

#[test]
fn saving_full_temp_take_moves_file() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tmp) = temp_session(dir.path(), 1000);
    let before = data(&tmp, 0, 1000);
    let dest = dir.path().join("out.wav");
    s.save(&dest).unwrap();
    assert_eq!(data(&dest, 0, 1000), before);
    assert!(!tmp.exists());
    assert_eq!(s.take.path, dest);
    assert!(!s.take.is_temp);
    assert!(s.take.saved);
    assert_eq!(s.reader.frames(), 1000);
}

#[test]
fn failed_full_save_keeps_the_temp_take() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tmp) = temp_session(dir.path(), 1000);
    let before = data(&tmp, 0, 1000);
    let dest = dir.path().join("no_existe").join("out.wav");
    assert!(s.save(&dest).is_err());
    assert_eq!(data(&tmp, 0, 1000), before);
    assert_eq!(s.take.path, tmp);
    assert!(s.take.is_temp && !s.take.saved);
    assert_eq!(s.reader.read_frames(0, 1000).unwrap(), before, "reader reopened on the temp");
}

#[test]
fn saving_trim_keeps_temp_and_writes_range() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tmp) = temp_session(dir.path(), 1000);
    let expected = data(&tmp, 100, 800);
    s.selection.set_start(100);
    s.selection.set_end(900);
    let dest = dir.path().join("trim.wav");
    s.save(&dest).unwrap();
    assert_eq!(WavReader::open(&dest).unwrap().frames(), 800);
    assert_eq!(data(&dest, 0, 800), expected);
    assert_eq!(s.take.path, tmp);
    assert!(s.take.is_temp && s.take.saved);
}

#[test]
fn closing_saved_temp_take_deletes_temp() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, tmp) = temp_session(dir.path(), 1000);
    s.selection.set_start(100);
    s.selection.set_end(900);
    let dest = dir.path().join("trim.wav");
    s.save(&dest).unwrap();
    s.close().unwrap();
    assert!(!tmp.exists());
    assert!(dest.exists());
}

#[test]
fn closing_saved_moved_take_keeps_file() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _tmp) = temp_session(dir.path(), 1000);
    let dest = dir.path().join("out.wav");
    s.save(&dest).unwrap();
    s.close().unwrap();
    assert!(dest.exists());
}

#[test]
fn saving_trim_over_itself_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("saved.wav");
    make_wav(&p, 100, 2);
    let expected = data(&p, 10, 10);
    let mut s = Session::open(&p, false).unwrap();
    s.selection.set_start(10);
    s.selection.set_end(20);
    s.playhead = 5;
    let path = s.take.path.clone();
    s.save(&path).unwrap();
    assert_eq!(data(&p, 0, 10), expected);
    assert!(s.selection.is_full());
    assert_eq!(s.reader.frames(), 10);
    assert_eq!(s.peaks.frames(), 10);
    assert_eq!(s.playhead, 0);
    assert!(s.take.saved);
    let parts = std::fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
        .count();
    assert_eq!(parts, 0);
}

#[test]
fn recover_reads_all_frames_of_stale_temp() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("crash.wav");
    {
        let mut w = WavWriter::create(&p, &fmt()).unwrap();
        w.write_audio(&f32_bytes(&samples(200, 3))).unwrap();
        w.update_header().unwrap();
        w.write_audio(&f32_bytes(&samples(100, 4))).unwrap();
    }
    let s = Session::recover(&p).unwrap();
    assert_eq!(s.reader.frames(), 300);
    assert_eq!(s.peaks.frames(), 300);
    assert!(!s.take.saved);
    assert!(s.take.is_temp);
}

#[test]
fn recover_of_zero_frame_orphan_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("orphan.wav");
    drop(WavWriter::create(&p, &fmt()).unwrap());
    assert!(matches!(Session::recover(&p), Err(SessionError::Empty)));
    assert!(!p.exists());
}

#[test]
fn recover_of_a_file_too_short_for_a_header_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    for (name, len) in [("zero.wav", 0usize), ("short.wav", 43)] {
        let p = dir.path().join(name);
        std::fs::write(&p, vec![0x52; len]).unwrap();
        assert!(matches!(Session::recover(&p), Err(SessionError::Empty)), "{name}");
        assert!(!p.exists(), "{name}");
    }
}

#[test]
fn recover_keeps_an_unreadable_file_that_may_hold_audio() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("garbage.wav");
    std::fs::write(&p, vec![0x55; 44]).unwrap();
    assert!(matches!(Session::recover(&p), Err(SessionError::Wav(_))));
    assert!(p.exists());
}

#[test]
fn recover_first_skips_orphans_that_fail() {
    let dir = tempfile::tempdir().unwrap();
    let base = SystemTime::now() - Duration::from_secs(1000);
    let at = |p: &Path, secs: u64| {
        std::fs::File::options().write(true).open(p).unwrap().set_modified(base + Duration::from_secs(secs)).unwrap();
    };
    let good = dir.path().join("good.wav");
    make_wav(&good, 500, 9);
    at(&good, 0);
    let empty = dir.path().join("empty.wav");
    drop(WavWriter::create(&empty, &fmt()).unwrap());
    at(&empty, 100);
    let bad = dir.path().join("bad.wav");
    std::fs::write(&bad, vec![0x55; 200]).unwrap();
    at(&bad, 200);
    let zero = dir.path().join("zero.wav");
    std::fs::write(&zero, b"").unwrap();
    at(&zero, 300);

    let (session, failed) = Session::recover_first(teeth::files::orphans(dir.path()));
    let session = session.expect("the good orphan opens");
    assert_eq!(session.take.path, good);
    assert_eq!(session.reader.frames(), 500);
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, bad);
    assert!(matches!(failed[0].1, SessionError::Wav(_)));
    assert!(bad.exists(), "a file that may hold audio is never deleted");
    assert!(!zero.exists() && !empty.exists(), "files without audio are deleted");
}

#[test]
fn recover_first_with_nothing_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.wav");
    std::fs::write(&bad, vec![0x55; 200]).unwrap();
    let (session, failed) = Session::recover_first(vec![bad.clone()]);
    assert!(session.is_none());
    assert_eq!(failed.len(), 1);
    assert!(bad.exists());
    let (session, failed) = Session::recover_first(Vec::new());
    assert!(session.is_none() && failed.is_empty());
}

#[test]
fn error_texts_are_in_spanish() {
    assert_eq!(SessionError::Empty.to_string(), "la toma está vacía");
    let io = std::io::Error::other("sin espacio");
    assert_eq!(SessionError::Io(io).to_string(), "error de lectura/escritura: sin espacio");
    assert_eq!(SessionError::Wav(WavError::NotWav).to_string(), "el archivo no es un WAV válido");
}

#[test]
fn disk_error_recording_repairs_stale_header_and_rebuilds_peaks() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("diskerr.wav");
    {
        let mut w = WavWriter::create(&p, &fmt()).unwrap();
        w.write_audio(&f32_bytes(&samples(200, 5))).unwrap();
        w.update_header().unwrap();
        w.write_audio(&f32_bytes(&samples(100, 6))).unwrap();
    }
    // Peaks in memory claim 250 frames, more than the file ended up holding.
    let rec = take(&p, 250, peaks_of(250, 5), EndReason::DiskError("full".into()));
    let s = Session::from_recording(rec).unwrap();
    assert_eq!(s.reader.frames(), 300);
    assert_eq!(s.peaks.frames(), 300);
    assert_eq!(s.selection.end, 300);
}

#[test]
fn clean_recording_keeps_given_peaks() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("clean.wav");
    make_wav(&p, 400, 7);
    let rec = take(&p, 400, peaks_of(400, 7), EndReason::Stopped);
    let s = Session::from_recording(rec).unwrap();
    assert_eq!(s.peaks.frames(), 400);
    assert_eq!(s.reader.frames(), 400);
}

#[test]
fn selection_peak_matches_raw_maximum() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _tmp) = temp_session(dir.path(), 1000);
    s.selection.set_start(300);
    s.selection.set_end(700);
    let raw = s.read_raw(300, 700).unwrap();
    let expected = raw.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert_eq!(raw.len(), 800);
    assert_eq!(s.selection_peak(), expected);
}

#[test]
fn settings_roundtrip_and_bad_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("sub").join("settings.json");
    let st = Settings { last_folder: Some(PathBuf::from("C:/Music")), window_size: Some([900.0, 500.0]), window_pos: Some([40.0, 60.0]), source: None };
    st.save_to(&p).unwrap();
    assert_eq!(Settings::load_from(&p), st);
    std::fs::write(&p, "{no json").unwrap();
    assert_eq!(Settings::load_from(&p), Settings::default());
}

#[test]
fn a_saved_position_is_used_only_while_the_title_bar_can_be_reached() {
    let screen = [0.0, 0.0, 1456.0, 819.0];
    let at = |pos| Settings { window_pos: Some(pos), window_size: Some([860.0, 380.0]), ..Settings::default() };
    assert_eq!(at([100.0, 50.0]).restorable_position(screen), Some([100.0, 50.0]));
    assert_eq!(at([1300.0, 700.0]).restorable_position(screen), Some([1300.0, 700.0]));
    assert_eq!(at([-700.0, 50.0]).restorable_position(screen), Some([-700.0, 50.0]));
    // On a monitor that is gone, or with the title bar out of reach.
    assert_eq!(at([5000.0, 50.0]).restorable_position(screen), None);
    assert_eq!(at([-900.0, 50.0]).restorable_position(screen), None);
    assert_eq!(at([100.0, 2000.0]).restorable_position(screen), None);
    assert_eq!(at([100.0, -200.0]).restorable_position(screen), None);
    assert_eq!(at([f32::NAN, 50.0]).restorable_position(screen), None);
    assert_eq!(Settings::default().restorable_position(screen), None);
    // A monitor on the left of the main one makes the virtual desktop start below zero.
    assert_eq!(at([-1000.0, 50.0]).restorable_position([-1456.0, 0.0, 2912.0, 819.0]), Some([-1000.0, 50.0]));
}

#[test]
fn a_recorded_take_keeps_its_cut_positions() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("cuts.wav");
    make_wav(&p, 400, 3);
    let rec = RecordedTake { cuts: vec![10, 20], ..take(&p, 400, peaks_of(400, 3), EndReason::Stopped) };
    let s = Session::from_recording(rec).unwrap();
    assert_eq!(s.take.cuts, vec![10, 20]);
}

#[test]
fn an_opened_file_has_no_cut_positions() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("opened.wav");
    make_wav(&p, 400, 4);
    assert!(Session::open(&p, false).unwrap().take.cuts.is_empty());
}

/// A mono take of these samples, opened as a saved file.
fn mono(dir: &Path, samples: &[f32]) -> Session {
    let path = dir.join("mono.wav");
    let mut w = WavWriter::create(&path, &WavFormat::float32_extensible(48_000, 1)).unwrap();
    w.write_audio(&f32_bytes(samples)).unwrap();
    w.finish().unwrap();
    Session::open(&path, false).unwrap()
}

fn sine(frames: usize) -> Vec<f32> {
    (0..frames).map(|n| (n as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5).collect()
}

/// `frames` of silence with sound in `[from, to)`.
fn sound_between(frames: usize, from: usize, to: usize) -> Vec<f32> {
    (0..frames).map(|n| if (from..to).contains(&n) { 0.5 } else { 0.0 }).collect()
}

#[test]
fn both_handles_go_to_the_nearest_crossing() {
    let dir = tempfile::tempdir().unwrap();
    let wave = sine(48_000);
    let mut s = mono(dir.path(), &wave);
    s.selection.start = 1_000;
    s.selection.end = 40_000;
    s.snap_to_zero().unwrap();
    for (handle, was) in [(s.selection.start, 1_000u64), (s.selection.end, 40_000)] {
        let (prev, next) = (wave[handle as usize - 1], wave[handle as usize]);
        assert!(prev.signum() != next.signum(), "{handle}: {prev} and {next} do not cross zero");
        // A crossing of 440 Hz every 55 frames at most.
        assert!(handle.abs_diff(was) <= 28, "{handle} is far from {was}");
    }
}

#[test]
fn the_whole_take_stays_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sine(4_800));
    s.snap_to_zero().unwrap();
    assert!(s.selection.is_full());
}

#[test]
fn a_handle_looks_only_10_ms_around_it() {
    let dir = tempfile::tempdir().unwrap();
    // The only crossing is at frame 2_000, 1_000 frames from the handle: 10 ms are 480.
    let wave: Vec<f32> = (0..4_000).map(|n| if n < 2_000 { 0.5 } else { -0.5 }).collect();
    let mut s = mono(dir.path(), &wave);
    s.selection.start = 1_000;
    s.selection.end = 4_000;
    s.snap_to_zero().unwrap();
    assert!(s.selection.start.abs_diff(1_000) <= 480, "{}", s.selection.start);
}

#[test]
fn handles_that_would_meet_stay_where_they_are() {
    let dir = tempfile::tempdir().unwrap();
    let wave: Vec<f32> = (0..1_000).map(|n| if n < 500 { 0.5 } else { -0.5 }).collect();
    let mut s = mono(dir.path(), &wave);
    s.selection.start = 499;
    s.selection.end = 501;
    s.snap_to_zero().unwrap();
    assert_eq!((s.selection.start, s.selection.end), (499, 501));
}

#[test]
fn a_handle_next_to_the_edge_of_the_take_stays_inside() {
    let dir = tempfile::tempdir().unwrap();
    let wave: Vec<f32> = (0..1_000).map(|n| if n < 5 { 0.5 } else { -0.5 }).collect();
    let mut s = mono(dir.path(), &wave);
    s.selection.start = 1;
    s.selection.end = 999;
    s.snap_to_zero().unwrap();
    assert_eq!(s.selection.start, 5);
    assert!(s.selection.end <= 1_000);
}

#[test]
fn silence_at_both_ends_is_trimmed() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sound_between(4_000, 1_000, 3_000));
    assert!(s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (1_000, 3_000));
    assert!(!s.selection.is_full());
}

#[test]
fn silence_at_one_end_is_trimmed_and_the_other_handle_stays() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sound_between(4_000, 1_000, 4_000));
    assert!(s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (1_000, 4_000));
    let mut s = mono(dir.path(), &sound_between(4_000, 0, 3_000));
    assert!(s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (0, 3_000));
}

#[test]
fn trimming_stays_inside_the_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut wave = sound_between(4_000, 2_000, 2_100);
    wave[100] = 0.5;
    wave[3_900] = 0.5;
    let mut s = mono(dir.path(), &wave);
    s.selection.start = 1_000;
    s.selection.end = 3_000;
    assert!(s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (2_000, 2_100));
}

#[test]
fn a_selection_of_only_silence_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sound_between(4_000, 3_000, 3_100));
    s.selection.start = 1_000;
    s.selection.end = 2_000;
    assert!(!s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (1_000, 2_000));
}

#[test]
fn what_is_below_minus_60_dbfs_counts_as_silence() {
    let dir = tempfile::tempdir().unwrap();
    let wave: Vec<f32> = (0..4_000)
        .map(|n| if (1_000..3_000).contains(&n) { 0.002 } else { 0.000_5 })
        .collect();
    let mut s = mono(dir.path(), &wave);
    assert!(s.trim_silence().unwrap());
    assert_eq!((s.selection.start, s.selection.end), (1_000, 3_000));
}

#[test]
fn sound_on_the_edge_of_the_take_leaves_it_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sound_between(4_000, 0, 4_000));
    assert!(s.trim_silence().unwrap());
    assert!(s.selection.is_full());
}

#[test]
fn a_trimmed_take_saves_exactly_the_frames_left() {
    let dir = tempfile::tempdir().unwrap();
    let wave = sound_between(4_000, 1_000, 3_000);
    let mut s = mono(dir.path(), &wave);
    let source = s.take.path.clone();
    s.trim_silence().unwrap();
    let dest = dir.path().join("recortada.wav");
    s.save(&dest).unwrap();
    assert_eq!(data(&dest, 0, 2_000), data(&source, 1_000, 2_000));
}

#[test]
fn a_failed_read_moves_no_handle() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = mono(dir.path(), &sound_between(4_000, 1_000, 3_000));
    s.selection.start = 500;
    s.selection.end = 3_500;
    s.reader.release();
    assert!(s.trim_silence().is_err());
    assert!(s.snap_to_zero().is_err());
    assert_eq!((s.selection.start, s.selection.end), (500, 3_500));
}

#[test]
fn a_file_opened_from_disk_counts_as_saved_and_survives_closing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kick.wav");
    make_wav(&path, 1_000, 3);
    let before = std::fs::read(&path).unwrap();
    let s = Session::open_existing(&path).unwrap();
    assert!(s.take.saved);
    assert!(!s.take.is_temp);
    assert!(s.selection.is_full());
    s.close().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn opening_a_file_that_is_not_a_wav_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nota.wav");
    std::fs::write(&path, b"esto no es audio, aunque se llame .wav y tenga texto").unwrap();
    assert!(matches!(Session::open_existing(&path), Err(SessionError::Wav(WavError::NotWav))));
    assert!(path.exists(), "a file that does not open is not touched");
}

#[test]
fn opening_a_missing_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(Session::open_existing(&dir.path().join("no.wav")), Err(SessionError::Wav(_) | SessionError::Io(_))));
}

#[test]
fn opening_a_wav_without_audio_says_it_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vacio.wav");
    make_wav(&path, 0, 3);
    assert!(matches!(Session::open_existing(&path), Err(SessionError::Empty)));
    assert!(path.exists(), "a file that does not open is not touched");
}

#[test]
fn a_session_knows_its_own_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kick.wav");
    let other = dir.path().join("snare.wav");
    make_wav(&path, 100, 3);
    make_wav(&other, 100, 4);
    let s = Session::open_existing(&path).unwrap();
    assert!(s.holds_file(&path));
    assert!(s.holds_file(&dir.path().join(".").join("kick.wav")));
    assert!(!s.holds_file(&other));
    assert!(!s.holds_file(&dir.path().join("no.wav")));
}
