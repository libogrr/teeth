//! Pruebas que necesitan un dispositivo de salida real. Ejecutar con `cargo test -- --ignored`.

use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};
use teeth::capture::{CaptureSource, start_capture};
use teeth::recorder::writer_loop::{CaptureEvent, EndReason, Packet, Shared};

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn loopback_delivers_real_time_frames_even_in_silence() {
    let shared = Arc::new(Shared::new(8));
    let (tx, rx) = channel();
    let info = start_capture(tx, shared.clone(), CaptureSource::SystemExceptSelf)
        .expect("start_capture");
    let rate = info.format.sample_rate();
    let block = usize::from(info.format.block_align());
    assert!(block > 0);
    assert!(!info.device_name.is_empty());

    // `silent` counts frames that arrived as silence, or as audio bytes that are all zero:
    // with nothing playing, these must still flow in real time.
    let (mut frames, mut silent) = (0u64, 0u64);
    let mut count = |packet: &Packet| match packet {
        Packet::Audio(bytes) => {
            assert_eq!(bytes.len() % block, 0, "audio packet is not whole frames");
            let n = (bytes.len() / block) as u64;
            frames += n;
            if bytes.iter().all(|b| *b == 0) {
                silent += n;
            }
        }
        Packet::Silence { frames: n } => {
            frames += n;
            silent += n;
        }
    };

    let deadline = Instant::now() + Duration::from_secs(2);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(CaptureEvent::Packet { packet, .. }) => count(&packet),
            Ok(CaptureEvent::End(reason)) => panic!("capture ended early: {reason:?}"),
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => panic!("capture thread vanished"),
        }
    }
    shared.stop.store(true, SeqCst);
    let end = loop {
        match rx
            .recv_timeout(Duration::from_secs(5))
            .expect("End after stop")
        {
            CaptureEvent::Packet { packet, .. } => count(&packet),
            CaptureEvent::End(reason) => break reason,
        }
    };
    info.thread.join().expect("capture thread panicked");

    let expected = 2 * u64::from(rate);
    println!(
        "device: {} | format: {} | volume_independent: {} | frames: {frames} (expected {expected}) | silent: {silent}",
        info.device_name,
        info.format.describe(),
        info.volume_independent
    );
    assert_eq!(end, EndReason::Stopped);
    assert_eq!(shared.queued_frames.load(SeqCst), frames);
    assert!(
        frames.abs_diff(expected) <= expected / 10,
        "got {frames} frames, expected {expected} ± 10 %"
    );
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn recorder_writes_a_readable_take() {
    use teeth::recorder::Recorder;
    use teeth::wav::WavReader;

    let dir = tempfile::tempdir().unwrap();
    let rec = Recorder::start(dir.path(), CaptureSource::SystemExceptSelf).expect("Recorder::start");
    let volume_independent = rec.volume_independent();
    let rate = u64::from(rec.format().sample_rate());
    std::thread::sleep(Duration::from_secs(2));
    let take = rec.stop().expect("stop");

    let reader = WavReader::open(&take.path).expect("open take");
    let expected = 2 * rate;
    println!(
        "path: {} | volume_independent: {volume_independent} | take.frames: {} | reader.frames: {} | expected {expected} | disc: {} | end: {:?}",
        take.path.display(),
        take.frames,
        reader.frames(),
        take.discontinuities,
        take.end
    );
    assert_eq!(reader.frames(), take.frames);
    assert!(
        take.frames.abs_diff(expected) <= expected / 10,
        "got {} frames, expected {expected} ± 10 %",
        take.frames
    );
    assert_eq!(take.end, EndReason::Stopped);
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn player_reaches_end() {
    use teeth::player::Player;
    use teeth::variant::Variant;
    use teeth::wav::{WavFormat, WavWriter};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silencio.wav");
    let format = WavFormat::float32_extensible(48_000, 2);
    let mut writer = WavWriter::create(&path, &format).expect("create wav");
    writer.write_silence(24_000).expect("write silence");
    writer.finish().expect("finish wav");

    let player = Player::start(&path, 0, 24_000, 0, false, Variant::default()).expect("Player::start");
    let deadline = Instant::now() + Duration::from_secs(2);
    while !player.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(player.is_finished(), "playback did not finish within 2 s");
    assert_eq!(player.position(), 24_000);
    assert_eq!(player.stop(), 24_000);
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn looping_player_repeats_until_looping_is_turned_off() {
    use teeth::player::Player;
    use teeth::variant::Variant;
    use teeth::wav::{WavFormat, WavWriter};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silencio.wav");
    let format = WavFormat::float32_extensible(48_000, 2);
    let mut writer = WavWriter::create(&path, &format).expect("create wav");
    writer.write_silence(48_000).expect("write silence");
    writer.finish().expect("finish wav");

    // Laps of 50 ms, shorter than the device buffer, after a first lap from 30_000.
    let player = Player::start(&path, 30_000, 12_400, 10_000, true, Variant::default()).expect("Player::start");
    assert!(player.is_finished(), "a start past the end plays nothing");
    let player = Player::start(&path, 11_000, 12_400, 10_000, true, Variant::default()).expect("Player::start");
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        assert!(!player.is_finished(), "a looping playback never finishes");
        let at = player.position();
        assert!((10_000..12_400).contains(&at), "position {at} outside the loop");
        std::thread::sleep(Duration::from_millis(7));
    }
    player.set_looping(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !player.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(player.is_finished(), "playback did not end after looping was turned off");
    assert_eq!(player.stop(), 12_400);
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn pitched_reverse_walks_back_to_the_start_at_its_rate() {
    use teeth::player::Player;
    use teeth::variant::Variant;
    use teeth::wav::{WavFormat, WavWriter};

    // At -3 st the cursor walks back at 40_363 frames/s: the 48_000 frames between 72_000
    // and 24_000 take 1.189 s, against 1.0 s at the unpitched 48_000. Timing between two
    // crossings leaves out the time the device takes to open. The pcm format's fmt has no
    // cbSize, and the device does not mix at 40_363 Hz, so the retry with conversion runs.
    let v = Variant { semitones: -3, reversed: true };
    for format in [WavFormat::float32_extensible(48_000, 2), WavFormat::pcm(48_000, 2, 16)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silencio.wav");
        let mut writer = WavWriter::create(&path, &format).expect("create wav");
        writer.write_silence(96_000).expect("write silence");
        writer.finish().expect("finish wav");

        let deadline = Instant::now() + Duration::from_millis(3_500);
        let player = Player::start(&path, 96_000, 96_000, 0, false, v).expect("Player::start");
        // While it runs, the position never grows and stays within the take.
        let mut last = player.position();
        let (mut crossed_72k, mut crossed_24k) = (None, None);
        loop {
            let at = player.position();
            assert!(at <= last && at <= 96_000, "position {at} after {last}");
            last = at;
            if at <= 72_000 && crossed_72k.is_none() {
                crossed_72k = Some(Instant::now());
            }
            if at <= 24_000 && crossed_24k.is_none() {
                crossed_24k = Some(Instant::now());
            }
            if player.is_finished() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(player.is_finished(), "playback did not finish within 3.5 s");
        let (from, to) = (crossed_72k.expect("never reached 72_000"), crossed_24k.expect("never reached 24_000"));
        let between = to - from;
        assert!(
            (Duration::from_millis(1_100)..=Duration::from_millis(1_300)).contains(&between),
            "48_000 frames took {between:?}, expected about 1.189 s"
        );
        assert_eq!(player.stop(), 0);
    }
}

#[test]
#[ignore = "necesita un dispositivo de salida"]
fn a_reversed_loop_stays_inside_until_looping_is_turned_off() {
    use teeth::player::Player;
    use teeth::variant::Variant;
    use teeth::wav::{WavFormat, WavWriter};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silencio.wav");
    let format = WavFormat::float32_extensible(48_000, 2);
    let mut writer = WavWriter::create(&path, &format).expect("create wav");
    writer.write_silence(48_000).expect("write silence");
    writer.finish().expect("finish wav");

    // Laps of 50 ms, shorter than the device buffer, after a first lap from 11_000 back.
    let rev = Variant { semitones: 0, reversed: true };
    let player = Player::start(&path, 10_000, 12_400, 10_000, true, rev).expect("Player::start");
    assert!(player.is_finished(), "a start on the start handle plays nothing");
    let player = Player::start(&path, 11_000, 12_400, 10_000, true, rev).expect("Player::start");
    let deadline = Instant::now() + Duration::from_millis(600);
    while Instant::now() < deadline {
        assert!(!player.is_finished(), "a looping playback never finishes");
        let at = player.position();
        assert!((10_000..=12_400).contains(&at), "position {at} outside the loop");
        std::thread::sleep(Duration::from_millis(7));
    }
    player.set_looping(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !player.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(player.is_finished(), "playback did not end after looping was turned off");
    assert_eq!(player.stop(), 10_000);
}

#[test]
#[ignore = "necesita aplicaciones con audio"]
fn list_apps_finds_a_playing_app() {
    use teeth::sources::{icon_rgba, list_apps, resolve};
    teeth::win::com_init();
    let start = Instant::now();
    let apps = list_apps();
    println!("list_apps: {:?}", start.elapsed());
    for app in &apps {
        let icon = app.path.as_deref().and_then(icon_rgba).map(|(size, _)| size);
        println!(
            "{} | {} | sounding {} | root {:?} | icon {:?} | {:?}",
            app.exe,
            app.name,
            app.sounding,
            resolve(&app.exe),
            icon,
            app.path
        );
    }
    assert!(!apps.is_empty(), "no app has an audio session");
}
