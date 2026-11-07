mod common;

use common::{XorShift, f32_bytes, u32le, u64le};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use teeth::variant::Variant;
use teeth::wav::{
    WavFormat, WavReader, WavWriter, build_header, header_len, parse_header, repair,
    repaired_header, reverse_frames, write_trimmed, write_variant,
};

fn make(dir: &Path, name: &str, f: &WavFormat, samples: &[f32]) -> PathBuf {
    let p = dir.join(name);
    let mut w = WavWriter::create(&p, f).unwrap();
    w.write_audio(&f32_bytes(samples)).unwrap();
    w.finish().unwrap();
    p
}

fn random(n: usize, seed: u64) -> Vec<f32> {
    let mut r = XorShift::new(seed);
    (0..n).map(|_| r.next_f32()).collect()
}

fn find_part(dir: &Path, found: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            find_part(&p, found);
        } else if p.to_string_lossy().ends_with(".part") {
            found.push(p);
        }
    }
}

#[test]
fn roundtrip_read_frames() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples = random(2000, 7);
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "a.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    assert_eq!(r.frames(), 1000);
    assert_eq!(r.read_frames(10, 5).unwrap(), orig[10 * 8..15 * 8]);
    assert_eq!(r.read_frames(995, 50).unwrap().len(), 5 * 8);
    let mut out = Vec::new();
    r.read_f32(10, 5, &mut out).unwrap();
    assert_eq!(out, samples[20..30]);
}

#[test]
fn release_closes_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(20, 1));
    let mut r = WavReader::open(&p).unwrap();
    r.release();
    assert!(r.read_frames(0, 1).is_err());
    fs::remove_file(&p).unwrap();
}

#[test]
fn parses_rf64_written_by_build_header() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let n = 5_000_000_000u64;
    let p = parse_header(&build_header(&f, n, true), 116 + n).unwrap();
    assert!(p.rf64);
    assert_eq!((p.data_offset, p.data_len), (116, n));
    assert_eq!(p.format, f);
}

#[test]
fn clamped_riff_uses_file_length() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let n = 5_000_000_000u64;
    let p = parse_header(&build_header(&f, n, false), 116 + n + 3).unwrap();
    assert_eq!(p.data_len, n); // los 3 bytes sueltos no forman un frame
}

#[test]
fn skips_unknown_chunks_with_padding() {
    let f = WavFormat::pcm(44_100, 1, 16);
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"LIST");
    b.extend_from_slice(&3u32.to_le_bytes());
    b.extend_from_slice(&[1, 2, 3, 0]); // 3 bytes + pad
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(f.fmt_chunk());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&8u32.to_le_bytes());
    let off = b.len() as u64;
    b.extend_from_slice(&[0; 8]);
    let p = parse_header(&b, b.len() as u64).unwrap();
    assert_eq!((p.data_offset, p.data_len, p.rf64), (off, 8, false));
    assert_eq!(p.format, f);
}

#[test]
fn rejects_non_wav() {
    assert!(parse_header(b"hello world, definitely not a wav", 33).is_err());
}

#[test]
fn repair_fixes_stale_header() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = dir.path().join("crash.wav");
    {
        let mut w = WavWriter::create(&p, &f).unwrap();
        w.write_audio(&f32_bytes(&random(200, 3))).unwrap();
        w.update_header().unwrap();
        w.write_audio(&f32_bytes(&random(100, 4))).unwrap();
        // se suelta sin finish
    }
    assert_eq!(WavReader::open(&p).unwrap().frames(), 100);
    assert_eq!(repair(&p).unwrap(), 150);
    assert_eq!(WavReader::open(&p).unwrap().frames(), 150);
}

#[test]
fn repair_past_4_gib_writes_rf64() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let n = 5_000_000_000u64;
    // Header refreshed during the recording (clamped RIFF), 3 stray bytes after the last frame.
    let stale = build_header(&f, 4_000_000_000, false);
    let (h, data_len) = repaired_header(&stale, 116 + n + 3).unwrap();
    assert_eq!(data_len, n);
    assert_eq!(h, build_header(&f, n, true));
    assert_eq!(&h[..4], b"RF64");
    assert_eq!(u32le(&h, 4), u32::MAX);
    assert_eq!(&h[12..16], b"ds64");
    assert_eq!(u64le(&h, 20), 116 - 8 + n); // RIFF size
    assert_eq!(u64le(&h, 28), n); // data size
    assert_eq!(u64le(&h, 36), n / 8); // sample count
    let p = parse_header(&h, 116 + n).unwrap();
    assert!(p.rf64);
    assert_eq!((p.data_offset, p.data_len), (116, n));
}

#[test]
fn repair_keeps_rf64_written_by_finish() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let n = 5_000_000_000u64;
    let fin = build_header(&f, n, true);
    assert_eq!(repaired_header(&fin, 116 + n).unwrap(), (fin.clone(), n));
}

#[test]
fn repair_pads_odd_data_and_drops_partial_frame() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::pcm(44_100, 1, 24);
    let p = dir.path().join("odd.wav");
    let audio: Vec<u8> = (1..=15).collect(); // 5 frames, odd length
    {
        let mut w = WavWriter::create(&p, &f).unwrap();
        w.write_audio(&audio).unwrap();
        w.write_audio(&[0xAA, 0xBB]).unwrap(); // a frame cut short by the crash
    }
    assert_eq!(repair(&p).unwrap(), 5);
    let bytes = fs::read(&p).unwrap();
    let hl = header_len(&f) as usize;
    assert_eq!(bytes.len(), hl + 15 + 1);
    assert_eq!(&bytes[..hl], &build_header(&f, 15, true)[..]);
    assert_eq!(&bytes[hl..hl + 15], &audio[..]);
    assert_eq!(bytes[hl + 15], 0, "pad byte");
    assert_eq!(WavReader::open(&p).unwrap().read_frames(0, 10).unwrap(), audio);
}

#[test]
fn repair_leaves_a_finished_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::pcm(44_100, 1, 24);
    let p = dir.path().join("done.wav");
    let mut w = WavWriter::create(&p, &f).unwrap();
    w.write_audio(&(1..=21).collect::<Vec<u8>>()).unwrap();
    w.finish().unwrap();
    let before = fs::read(&p).unwrap();
    assert_eq!(repair(&p).unwrap(), 7);
    assert_eq!(fs::read(&p).unwrap(), before);
}

#[test]
fn trim_copies_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples = random(2000, 11);
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "a.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    for (s, e) in [(10u64, 990u64), (0, 1000)] {
        let dest = dir.path().join(format!("t_{s}_{e}.wav"));
        write_trimmed(&mut r, s, e, &dest).unwrap();
        let bytes = fs::read(&dest).unwrap();
        let hl = header_len(&f) as usize;
        let want = &orig[(s * 8) as usize..(e * 8) as usize];
        assert_eq!(&bytes[hl..], want);
        assert_eq!(bytes.len(), hl + want.len());
        assert_eq!(&bytes[..hl], &build_header(&f, want.len() as u64, true)[..]);
    }
}

#[test]
fn trim_replaces_existing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(2000, 5));
    let dest = dir.path().join("dest.wav");
    fs::write(&dest, b"old contents").unwrap();
    let mut r = WavReader::open(&p).unwrap();
    write_trimmed(&mut r, 0, 10, &dest).unwrap();
    assert_eq!(WavReader::open(&dest).unwrap().frames(), 10);
}

#[test]
fn failed_trim_leaves_no_part_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(2000, 5));
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("no_existe").join("x.wav");
    assert!(write_trimmed(&mut r, 0, 10, &dest).is_err());
    // fallo a mitad de copia: el origen se libera
    let dest2 = dir.path().join("y.wav");
    r.release();
    assert!(write_trimmed(&mut r, 0, 10, &dest2).is_err());
    assert!(!dest2.exists());
    let mut found = Vec::new();
    find_part(dir.path(), &mut found);
    assert!(found.is_empty(), "{found:?}");
    let _ = File::open(&p);
}

#[test]
fn reverse_frames_keeps_each_frame_whole() {
    let mut three = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
    reverse_frames(&mut three, 3);
    assert_eq!(three, [7, 8, 9, 4, 5, 6, 1, 2, 3]);
    let mut one = vec![1, 2];
    reverse_frames(&mut one, 2);
    assert_eq!(one, [1, 2]);
    reverse_frames(&mut [], 2);
}

#[test]
fn a_faithful_variant_writes_what_write_trimmed_writes() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(2000, 11));
    let mut r = WavReader::open(&p).unwrap();
    let trimmed = dir.path().join("trimmed.wav");
    let variant = dir.path().join("variant.wav");
    write_trimmed(&mut r, 10, 990, &trimmed).unwrap();
    write_variant(&mut r, 10, 990, Variant::default(), &variant).unwrap();
    assert_eq!(fs::read(&variant).unwrap(), fs::read(&trimmed).unwrap());
}

#[test]
fn write_variant_with_pitch_keeps_the_audio_and_changes_only_the_rate() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples = random(2000, 11);
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "a.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("pitch.wav");
    write_variant(&mut r, 10, 990, Variant { semitones: -3, reversed: false }, &dest).unwrap();
    let got = WavReader::open(&dest).unwrap();
    assert_eq!(got.format().sample_rate(), 40_363);
    let bytes = fs::read(&dest).unwrap();
    let hl = header_len(got.format()) as usize;
    assert_eq!(&bytes[hl..], &orig[80..7_920]);
    // `fact` and the sizes are those of any file of these frames.
    assert_eq!(&bytes[..hl], &build_header(got.format(), 7_840, true)[..]);
}

#[test]
fn write_variant_with_reverse_reverses_across_copy_blocks() {
    // 150_000 frames of 8 bytes, more than one 1 MiB block; each frame distinct.
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples: Vec<f32> = (0..300_000).map(|i| i as f32).collect();
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "long.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("rev.wav");
    write_variant(&mut r, 0, 150_000, Variant { semitones: 0, reversed: true }, &dest).unwrap();
    let block = usize::from(f.block_align());
    let want: Vec<u8> = orig.chunks_exact(block).rev().flatten().copied().collect();
    let got = WavReader::open(&dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    let hl = header_len(got.format()) as usize;
    assert_eq!(&bytes[hl..], &want[..]);
    assert_eq!(got.format().fmt_chunk(), f.fmt_chunk());
}

#[test]
fn write_variant_with_reverse_reverses_a_range_that_does_not_start_at_zero() {
    // 160_000 frames of 8 bytes, each distinct; the range 10_000..150_000 is 1_120_000
    // bytes, more than one 1 MiB block, and does not begin at the start of the data.
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples: Vec<f32> = (0..320_000).map(|i| i as f32).collect();
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "long.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("rev.wav");
    write_variant(&mut r, 10_000, 150_000, Variant { semitones: 0, reversed: true }, &dest).unwrap();
    let block = usize::from(f.block_align());
    let want: Vec<u8> = orig[10_000 * block..150_000 * block].chunks_exact(block).rev().flatten().copied().collect();
    let got = WavReader::open(&dest).unwrap();
    assert_eq!(got.frames(), 140_000);
    let bytes = fs::read(&dest).unwrap();
    let hl = header_len(got.format()) as usize;
    assert_eq!(&bytes[hl..], &want[..]);
    assert_eq!(got.format().fmt_chunk(), f.fmt_chunk());
}

#[test]
fn write_variant_with_both_reverses_and_changes_the_rate() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let samples = random(2000, 11);
    let orig = f32_bytes(&samples);
    let p = make(dir.path(), "a.wav", &f, &samples);
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("both.wav");
    write_variant(&mut r, 100, 200, Variant { semitones: 7, reversed: true }, &dest).unwrap();
    let got = WavReader::open(&dest).unwrap();
    assert_eq!(got.format().sample_rate(), 71_919);
    let bytes = fs::read(&dest).unwrap();
    let hl = header_len(got.format()) as usize;
    let block = usize::from(f.block_align());
    let want: Vec<u8> = orig[100 * 8..200 * 8].chunks_exact(block).rev().flatten().copied().collect();
    assert_eq!(&bytes[hl..], &want[..]);
    assert_eq!(&bytes[..hl], &build_header(got.format(), 800, true)[..]);
}

#[test]
fn write_variant_keeps_odd_frames_and_the_pad_byte() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::pcm(48_000, 1, 24);
    // Five frames of 3 bytes written with WavWriter: 15 bytes of data, one pad byte.
    let p = dir.path().join("odd.wav");
    let mut w = WavWriter::create(&p, &f).unwrap();
    w.write_audio(&(1..=15).collect::<Vec<u8>>()).unwrap();
    w.finish().unwrap();
    let mut r = WavReader::open(&p).unwrap();
    let dest = dir.path().join("odd_rev.wav");
    write_variant(&mut r, 0, 5, Variant { semitones: 0, reversed: true }, &dest).unwrap();
    let bytes = fs::read(&dest).unwrap();
    let hl = header_len(&f) as usize;
    assert_eq!(bytes.len(), hl + 16);
    assert_eq!(&bytes[hl..hl + 15], &[13, 14, 15, 10, 11, 12, 7, 8, 9, 4, 5, 6, 1, 2, 3]);
    assert_eq!(WavReader::open(&dest).unwrap().frames(), 5);
}

#[test]
fn failed_variant_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(2000, 5));
    let mut r = WavReader::open(&p).unwrap();
    let variant = Variant { semitones: -3, reversed: true };
    let dest = dir.path().join("no_existe").join("x.wav");
    assert!(write_variant(&mut r, 0, 10, variant, &dest).is_err());
    // A range outside the audio fails before anything is left behind.
    let dest1 = dir.path().join("z.wav");
    assert!(write_variant(&mut r, 0, 1_001, variant, &dest1).is_err());
    assert!(write_variant(&mut r, 20, 10, variant, &dest1).is_err());
    assert!(!dest1.exists());
    // Failure halfway through the copy: the source is released.
    let dest2 = dir.path().join("y.wav");
    r.release();
    assert!(write_variant(&mut r, 0, 10, variant, &dest2).is_err());
    assert!(!dest2.exists());
    let mut found = Vec::new();
    find_part(dir.path(), &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_variant_replaces_an_existing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let f = WavFormat::float32_extensible(48_000, 2);
    let p = make(dir.path(), "a.wav", &f, &random(2000, 5));
    let dest = dir.path().join("dest.wav");
    fs::write(&dest, b"old contents").unwrap();
    let mut r = WavReader::open(&p).unwrap();
    write_variant(&mut r, 0, 10, Variant { semitones: 2, reversed: true }, &dest).unwrap();
    assert_eq!(WavReader::open(&dest).unwrap().frames(), 10);
}
