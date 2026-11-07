//! Playback ranges that need no output device: they finish before touching WASAPI.

use teeth::player::Player;
use teeth::variant::Variant;
use teeth::wav::{WavFormat, WavWriter};

fn silent_wav(dir: &std::path::Path, frames: u64) -> std::path::PathBuf {
    let path = dir.join("silencio.wav");
    let mut w = WavWriter::create(&path, &WavFormat::float32_extensible(48_000, 2)).unwrap();
    w.write_silence(frames).unwrap();
    w.finish().unwrap();
    path
}

#[test]
fn empty_range_finishes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let path = silent_wav(dir.path(), 1_000);
    let p = Player::start(&path, 500, 500, 500, false, Variant::default()).expect("start");
    assert!(p.is_finished());
    assert_eq!(p.position(), 500);
    let p = Player::start(&path, 600, 500, 600, false, Variant::default()).expect("start");
    assert!(p.is_finished());
    assert_eq!(p.stop(), 600);
}

#[test]
fn start_past_end_of_file_finishes_without_panic() {
    let dir = tempfile::tempdir().unwrap();
    let path = silent_wav(dir.path(), 1_000);
    let p = Player::start(&path, 5_000, 9_000, 5_000, false, Variant::default()).expect("start");
    assert!(p.is_finished());
    assert_eq!(p.position(), 5_000);
    p.stop();
}

#[test]
fn a_reverse_with_nothing_before_the_cursor_finishes_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let path = silent_wav(dir.path(), 1_000);
    let rev = Variant { semitones: 0, reversed: true };
    let p = Player::start(&path, 500, 900, 500, false, rev).expect("start");
    assert!(p.is_finished());
    assert_eq!(p.position(), 500);
    let p = Player::start(&path, 900, 500, 900, false, rev).expect("start");
    assert!(p.is_finished());
}
