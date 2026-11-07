mod common;
use common::*;
use std::io::Cursor;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};
use teeth::recorder::writer_loop::{run_writer, CaptureEvent, EndReason, Packet, Shared};
use teeth::wav::{WavFormat, WavWriter};

fn audio(samples: &[f32], discontinuity: bool) -> CaptureEvent {
    CaptureEvent::Packet { packet: Packet::Audio(f32_bytes(samples)), discontinuity }
}

#[test]
fn writes_audio_and_silence_in_order() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    let shared = Shared::new(2);
    let (tx, rx) = channel();
    tx.send(audio(&[0.25; 4], false)).unwrap();
    tx.send(CaptureEvent::Packet { packet: Packet::Silence { frames: 3 }, discontinuity: false }).unwrap();
    tx.send(audio(&[-0.5; 2], false)).unwrap();
    tx.send(CaptureEvent::End(EndReason::Stopped)).unwrap();
    let (out, end) = run_writer(rx, w, &shared);
    let bytes = out.unwrap().into_inner();
    let mut expected = f32_bytes(&[0.25; 4]);
    expected.extend(vec![0u8; 24]);
    expected.extend(f32_bytes(&[-0.5; 2]));
    assert_eq!(&bytes[116..], &expected[..]);
    assert_eq!(end, EndReason::Stopped);
    assert_eq!(shared.frames.load(SeqCst), 6);
}

#[test]
fn first_discontinuity_is_ignored() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    let shared = Shared::new(2);
    let (tx, rx) = channel();
    tx.send(audio(&[0.1; 2], true)).unwrap();
    tx.send(audio(&[0.1; 2], true)).unwrap();
    tx.send(audio(&[0.1; 2], false)).unwrap();
    tx.send(CaptureEvent::End(EndReason::Stopped)).unwrap();
    let (out, _) = run_writer(rx, w, &shared);
    out.unwrap();
    assert_eq!(shared.discontinuities.load(SeqCst), 1);
}

#[test]
fn peaks_receive_decoded_samples() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    let shared = Shared::new(2);
    let mut samples = vec![0.1f32; 600];
    samples[280 * 2 + 1] = 1.5;
    let (tx, rx) = channel();
    tx.send(audio(&samples, false)).unwrap();
    tx.send(CaptureEvent::End(EndReason::Stopped)).unwrap();
    let (out, _) = run_writer(rx, w, &shared);
    out.unwrap();
    let peaks = shared.peaks.lock().unwrap();
    assert_eq!(peaks.bins(0, 1)[1], (0.1, 1.5));
    assert_eq!(peaks.bins(0, 0)[0], (0.1, 0.1));
}

#[test]
fn header_refreshes_every_second_of_frames() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let cursor = SharedCursor::new();
    let w = WavWriter::new(cursor.clone(), &f).unwrap();
    let shared = Arc::new(Shared::new(2));
    let (tx, rx) = channel();
    let s2 = Arc::clone(&shared);
    let handle = std::thread::spawn(move || run_writer(rx, w, &s2).1);
    for _ in 0..4_801 {
        tx.send(audio(&[0.1; 20], false)).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while shared.frames.load(SeqCst) != 48_010 {
        assert!(Instant::now() < deadline, "writer did not consume all packets");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(u32le(&cursor.bytes(), 112), 48_000 * 8);
    drop(tx);
    assert_eq!(handle.join().unwrap(), EndReason::Stopped);
}

#[test]
fn queued_frames_decrease_as_written() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    let shared = Shared::new(2);
    shared.queued_frames.store(10, SeqCst);
    let (tx, rx) = channel();
    tx.send(audio(&[0.1; 8], false)).unwrap();
    tx.send(CaptureEvent::End(EndReason::Stopped)).unwrap();
    let (out, _) = run_writer(rx, w, &shared);
    out.unwrap();
    assert_eq!(shared.queued_frames.load(SeqCst), 6);
}

#[test]
fn write_error_stops_capture() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(FailAfter::new(120), &f).unwrap();
    let shared = Shared::new(2);
    let (tx, rx) = channel();
    tx.send(audio(&[0.1; 8], false)).unwrap();
    let (_out, end) = run_writer(rx, w, &shared);
    assert!(matches!(end, EndReason::DiskError(_)), "{end:?}");
    assert!(shared.stop.load(SeqCst));
}

#[test]
fn cuts_record_where_each_counted_cut_starts() {
    let f = WavFormat::float32_extensible(48_000, 2);
    let w = WavWriter::new(Cursor::new(vec![]), &f).unwrap();
    let shared = Shared::new(2);
    let (tx, rx) = channel();
    // The first packet is never a cut; the next two are, at the frames already written.
    tx.send(audio(&[0.1; 4], true)).unwrap();
    tx.send(audio(&[0.1; 6], true)).unwrap();
    tx.send(CaptureEvent::Packet { packet: Packet::Silence { frames: 5 }, discontinuity: true }).unwrap();
    tx.send(CaptureEvent::End(EndReason::Stopped)).unwrap();
    let (out, _) = run_writer(rx, w, &shared);
    out.unwrap();
    assert_eq!(*shared.cuts.lock().unwrap(), vec![2, 5]);
    assert_eq!(shared.discontinuities.load(SeqCst), 2);
}
