mod common;
use common::f32_bytes;
use std::path::{Path, PathBuf};
use teeth::dragout::{Exports, export, sample_name, unique_path};
use teeth::files::LocalTime;
use teeth::session::Session;
use teeth::variant::Variant;
use teeth::wav::{WavFormat, WavReader, WavWriter};

const NOW: LocalTime = LocalTime { year: 2026, month: 11, day: 1, hour: 14, minute: 32, second: 5 };
const LATER: LocalTime = LocalTime { year: 2026, month: 11, day: 5, hour: 14, minute: 32, second: 5 };

/// A stereo float take of `frames` frames whose samples count up, opened as a saved file.
fn take(dir: &Path, frames: usize) -> (Session, PathBuf) {
    let path = dir.join("toma.wav");
    let samples: Vec<f32> = (0..frames * 2).map(|i| i as f32 / 1e6).collect();
    let mut w = WavWriter::create(&path, &WavFormat::float32_extensible(48_000, 2)).unwrap();
    w.write_audio(&f32_bytes(&samples)).unwrap();
    w.finish().unwrap();
    (Session::open(&path, false).unwrap(), path)
}

fn data(path: &Path) -> Vec<u8> {
    let mut r = WavReader::open(path).unwrap();
    let frames = r.frames();
    r.read_frames(0, frames).unwrap()
}

fn files_in(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

#[test]
fn names_carry_the_source_and_the_time() {
    assert_eq!(sample_name("Google Chrome", Variant::default(), &NOW), "Google Chrome 2026-11-01 14-32-05.wav");
    assert_eq!(sample_name("a/b:c*?", Variant::default(), &NOW), "abc 2026-11-01 14-32-05.wav");
    assert_eq!(sample_name("", Variant::default(), &NOW), "Teeth 2026-11-01 14-32-05.wav");
}

#[test]
fn colliding_names_get_a_counter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.wav"), b"").unwrap();
    std::fs::write(dir.path().join("x-2.wav"), b"").unwrap();
    assert_eq!(unique_path(dir.path(), "x.wav"), dir.path().join("x-3.wav"));
}

#[test]
fn export_copies_the_selection_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, path) = take(dir.path(), 1_000);
    s.selection.start = 100;
    s.selection.end = 350;
    let out_dir = dir.path().join("Samples");
    let sample = export(&mut s, &mut Exports::default(), &out_dir, "Google Chrome", Variant::default(), &NOW).unwrap();
    assert_eq!(sample, out_dir.join("Google Chrome 2026-11-01 14-32-05.wav"));
    assert_eq!(data(&sample), data(&path)[100 * 8..350 * 8]);
}

#[test]
fn the_same_range_is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = take(dir.path(), 1_000);
    s.selection.start = 10;
    s.selection.end = 20;
    let out_dir = dir.path().join("Samples");
    let mut exports = Exports::default();
    let first = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    let later = LocalTime { second: 9, ..NOW };
    let second = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &later).unwrap();
    assert_eq!(first, second);
    assert_eq!(files_in(&out_dir), 1);
}

#[test]
fn a_deleted_sample_is_written_again() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = take(dir.path(), 1_000);
    let out_dir = dir.path().join("Samples");
    let mut exports = Exports::default();
    let first = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    std::fs::remove_file(&first).unwrap();
    let again = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    assert!(again.exists());
    assert_eq!(files_in(&out_dir), 1);
}

#[test]
fn a_rewritten_take_does_not_reuse_old_samples() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, path) = take(dir.path(), 1_000);
    s.selection.start = 0;
    s.selection.end = 50;
    let out_dir = dir.path().join("Samples");
    let mut exports = Exports::default();
    let first = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    // Trim the take over its own file: frames 500..1000 become the whole take.
    s.selection.start = 500;
    s.selection.end = 1_000;
    s.save(&path).unwrap();
    s.selection.start = 0;
    s.selection.end = 50;
    let second = export(&mut s, &mut exports, &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    assert_ne!(first, second);
    assert_ne!(data(&first), data(&second));
}

#[test]
fn non_ascii_folders_work() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, path) = take(dir.path(), 200);
    let out_dir = dir.path().join("sámples ñ");
    let sample = export(&mut s, &mut Exports::default(), &out_dir, "Teeth", Variant::default(), &NOW).unwrap();
    assert_eq!(data(&sample), data(&path));
}

#[test]
fn export_errors_name_the_sample() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = take(dir.path(), 200);
    let not_a_dir = dir.path().join("archivo");
    std::fs::write(&not_a_dir, b"x").unwrap();
    let err = export(&mut s, &mut Exports::default(), &not_a_dir, "Teeth", Variant::default(), &NOW).unwrap_err();
    assert!(err.starts_with("No se pudo crear el sample: "), "{err}");
}

#[test]
fn samples_live_in_teeth_samples_under_documents() {
    let dir = teeth::dragout::samples_dir();
    assert!(dir.ends_with(Path::new("Teeth").join("Samples")), "{}", dir.display());
}

#[test]
fn names_carry_the_variant_after_the_time() {
    let v = |semitones, reversed| Variant { semitones, reversed };
    assert_eq!(sample_name("Zen", v(-3, true), &LATER), "Zen 2026-11-05 14-32-05 -3st rev.wav");
    assert_eq!(sample_name("Zen", v(7, false), &LATER), "Zen 2026-11-05 14-32-05 +7st.wav");
    assert_eq!(sample_name("Zen", v(0, true), &LATER), "Zen 2026-11-05 14-32-05 rev.wav");
    assert_eq!(sample_name("Zen", v(0, false), &LATER), "Zen 2026-11-05 14-32-05.wav");
}

#[test]
fn each_variant_of_a_range_has_its_own_sample() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, _) = take(dir.path(), 1_000);
    s.selection.start = 100;
    s.selection.end = 350;
    let out_dir = dir.path().join("Samples");
    let mut exports = Exports::default();
    let low_pitch = Variant { semitones: -3, reversed: false };
    let plain = export(&mut s, &mut exports, &out_dir, "Zen", Variant::default(), &NOW).unwrap();
    let low = export(&mut s, &mut exports, &out_dir, "Zen", low_pitch, &NOW).unwrap();
    let later = LocalTime { second: 9, ..NOW };
    let plain_again = export(&mut s, &mut exports, &out_dir, "Zen", Variant::default(), &later).unwrap();
    let low_again = export(&mut s, &mut exports, &out_dir, "Zen", low_pitch, &later).unwrap();
    assert_ne!(plain, low);
    assert_eq!(plain_again, plain);
    assert_eq!(low_again, low);
    assert_eq!(files_in(&out_dir), 2);
}

#[test]
fn colliding_variant_names_put_the_counter_last() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x -3st rev.wav"), b"").unwrap();
    assert_eq!(unique_path(dir.path(), "x -3st rev.wav"), dir.path().join("x -3st rev-2.wav"));
}

#[test]
fn a_reversed_export_reverses_the_range() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, path) = take(dir.path(), 1_000);
    s.selection.start = 100;
    s.selection.end = 350;
    let out_dir = dir.path().join("Samples");
    let variant = Variant { semitones: 0, reversed: true };
    let sample = export(&mut s, &mut Exports::default(), &out_dir, "Zen", variant, &NOW).unwrap();
    let original = data(&path);
    let expected: Vec<u8> = (100..350).rev().flat_map(|f| original[f * 8..(f + 1) * 8].to_vec()).collect();
    assert_eq!(data(&sample), expected);
    assert_eq!(WavReader::open(&sample).unwrap().format().sample_rate(), 48_000);
}

#[test]
fn a_pitched_export_keeps_the_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let (mut s, path) = take(dir.path(), 1_000);
    s.selection.start = 100;
    s.selection.end = 350;
    let out_dir = dir.path().join("Samples");
    let variant = Variant { semitones: -3, reversed: false };
    let sample = export(&mut s, &mut Exports::default(), &out_dir, "Zen", variant, &NOW).unwrap();
    assert_eq!(data(&sample), data(&path)[100 * 8..350 * 8]);
    assert_eq!(WavReader::open(&sample).unwrap().format().sample_rate(), 40_363);
}
