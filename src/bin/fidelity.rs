//! Fidelity test: plays known noise through the default output device in its mix format
//! while Teeth's recorder captures this process, then compares the recording with the
//! signal.
//!
//! Exit codes: 0 bit-exact, 1 not bit-exact (or no usable recording), 2 unsupported
//! format or a setup failure. It plays audible noise: use headphones or low speaker volume.

use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::Duration;
use teeth::capture::CaptureSource;
use teeth::recorder::Recorder;
use teeth::wav::{SampleKind, WavFormat, WavReader};
use teeth::win;
use windows::Win32::Media::Audio::{AUDCLNT_SHAREMODE_SHARED, IAudioClient, IAudioRenderClient};
use windows::Win32::System::Com::{CLSCTX_ALL, CoTaskMemFree};

const SEED: u64 = 0x2545_F491_4F6C_DD1D;
const AMPLITUDE: f32 = 0.1;
const SILENCE_SECS: f64 = 0.5;
const NOISE_SECS: f64 = 5.0;
/// Silence played after the noise. Playback stops once the buffer has drained, before
/// the last ~40 ms are heard; this tail carries the end of the noise into the capture.
const TRAIL_SECS: f64 = 0.3;
const TAIL_SECS: u64 = 1;
/// Samples that must match to accept an alignment.
const PROBE: usize = 64;
/// Samples of the signal used to find the lag when no bit-exact alignment exists.
const LAG_PROBE: usize = 2048;
const BUFFER_HNS: i64 = 2_000_000;

fn main() -> ExitCode {
    win::com_init();
    let code = run();
    win::com_uninit();
    code
}

fn run() -> ExitCode {
    let dir = std::env::temp_dir().join("Teeth-fidelity");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("No se pudo crear la carpeta temporal: {e}");
        return ExitCode::from(2);
    }
    // The noise comes from this process, which `SystemExceptSelf` would leave out.
    let recorder = match Recorder::start(&dir, CaptureSource::Process(std::process::id())) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            let _ = std::fs::remove_dir(&dir);
            return ExitCode::from(2);
        }
    };
    let format = recorder.format().clone();
    if format.sample_kind() != SampleKind::F32 {
        println!("Formato no compatible con la prueba");
        if let Ok(t) = recorder.stop() {
            let _ = std::fs::remove_file(t.path);
        }
        let _ = std::fs::remove_dir(&dir);
        return ExitCode::from(2);
    }
    println!("Dispositivo: {} ({})", recorder.device_name(), format.describe());
    println!("{}", capture_text(recorder.volume_independent()));
    println!("Reproduciendo ruido durante {NOISE_SECS} s...");

    let (lead, noise, trail) = build_signal(&format);
    let played = play(lead, &noise, trail, &format);
    sleep(Duration::from_secs(TAIL_SECS));
    let take = recorder.stop();
    let code = match (played, take) {
        (Err(e), take) => {
            eprintln!("No se pudo reproducir: {e}");
            if let Ok(t) = take {
                let _ = std::fs::remove_file(t.path);
            }
            ExitCode::from(2)
        }
        (Ok(()), Err(e)) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
        (Ok(()), Ok(take)) => {
            let result = analyze(&take.path, &noise);
            if take.discontinuities > 0 {
                println!("Aviso: la toma tiene {} cortes.", take.discontinuities);
            }
            let _ = std::fs::remove_file(&take.path);
            result
        }
    };
    let _ = std::fs::remove_dir(&dir);
    code
}

/// Which capture the recorder got: before the Windows volume or after it.
fn capture_text(volume_independent: bool) -> &'static str {
    if volume_independent {
        "Captura por aplicación: independiente del volumen de Windows."
    } else {
        "Captura del dispositivo: depende del volumen de Windows."
    }
}

/// Frames of leading silence, the interleaved noise samples to compare, and frames of
/// trailing silence.
fn build_signal(format: &WavFormat) -> (u64, Vec<f32>, u64) {
    let rate = f64::from(format.sample_rate());
    let lead = (SILENCE_SECS * rate).round() as u64;
    let frames = (NOISE_SECS * rate).round() as usize;
    let trail = (TRAIL_SECS * rate).round() as u64;
    (lead, noise(frames * usize::from(format.channels())), trail)
}

fn analyze(path: &Path, signal: &[f32]) -> ExitCode {
    let mut reader = match WavReader::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("No se pudo abrir la grabación: {e}");
            return ExitCode::from(2);
        }
    };
    let mut rec = Vec::new();
    if let Err(e) = reader.read_f32(0, reader.frames(), &mut rec) {
        eprintln!("No se pudo leer la grabación: {e}");
        return ExitCode::from(2);
    }
    let offset = find_offset(&rec, signal, PROBE);
    if let Some(off) = offset {
        let (same, total) = compare_exact(&rec, signal, off);
        if same == total {
            println!("BIT A BIT: {same} de {total} muestras idénticas");
            return ExitCode::SUCCESS;
        }
        println!("{same} de {total} muestras idénticas");
    }
    let lag = offset.or_else(|| find_lag(&rec, signal, LAG_PROBE));
    match lag {
        Some(lag) => {
            let (gain, max_diff) = estimate_gain(&rec, signal, lag);
            println!(
                "NO ES BIT A BIT: ganancia aparente {:.2} dB, diferencia máxima {:.6}",
                gain_db(gain),
                max_diff
            );
        }
        None => println!("NO ES BIT A BIT: la grabación no contiene la señal reproducida"),
    }
    ExitCode::from(1)
}

/// Fixed-seed xorshift64* noise in `[-AMPLITUDE, AMPLITUDE)`.
fn noise(samples: usize) -> Vec<f32> {
    let mut x = SEED;
    (0..samples)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40; // 24 bits
            ((r as f64 / (1u64 << 23) as f64 - 1.0) as f32) * AMPLITUDE
        })
        .collect()
}

/// First sample index in `rec` where the first `probe` samples of `signal` appear
/// bit for bit.
fn find_offset(rec: &[f32], signal: &[f32], probe: usize) -> Option<usize> {
    let probe = probe.min(signal.len());
    if probe == 0 || rec.len() < probe {
        return None;
    }
    let head = &signal[..probe];
    (0..=rec.len() - probe).find(|&o| {
        rec[o..o + probe]
            .iter()
            .zip(head)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    })
}

/// Counts samples of `signal` that equal `rec` bit for bit from `offset`.
/// Samples missing from the end of `rec` count as different. Returns (identical, total).
fn compare_exact(rec: &[f32], signal: &[f32], offset: usize) -> (usize, usize) {
    let same = rec
        .get(offset..)
        .unwrap_or(&[])
        .iter()
        .zip(signal)
        .filter(|(a, b)| a.to_bits() == b.to_bits())
        .count();
    (same, signal.len())
}

/// Lag of `signal` inside `rec` that maximizes the cross-correlation of the first
/// `probe` samples of the signal.
fn find_lag(rec: &[f32], signal: &[f32], probe: usize) -> Option<usize> {
    let probe = probe.min(signal.len());
    if probe == 0 || rec.len() < probe {
        return None;
    }
    let head = &signal[..probe];
    let mut best: Option<(usize, f64)> = None;
    for lag in 0..=rec.len() - probe {
        let dot: f64 = rec[lag..lag + probe]
            .iter()
            .zip(head)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        if best.is_none_or(|(_, d)| dot.abs() > d) {
            best = Some((lag, dot.abs()));
        }
    }
    best.filter(|&(_, d)| d > 0.0).map(|(lag, _)| lag)
}

/// Least-squares gain of the recording relative to the signal at `lag`, and the largest
/// absolute sample difference over the overlap.
fn estimate_gain(rec: &[f32], signal: &[f32], lag: usize) -> (f64, f64) {
    let overlap = rec.get(lag..).unwrap_or(&[]);
    let (mut cross, mut energy, mut max_diff) = (0.0f64, 0.0f64, 0.0f64);
    for (r, s) in overlap.iter().zip(signal) {
        let (r, s) = (f64::from(*r), f64::from(*s));
        cross += r * s;
        energy += s * s;
        max_diff = max_diff.max((r - s).abs());
    }
    let gain = if energy > 0.0 { cross / energy } else { 0.0 };
    (gain, max_diff)
}

fn gain_db(gain: f64) -> f64 {
    20.0 * gain.abs().log10()
}

/// Plays `lead` frames of silence, `noise` and `trail` frames of silence through the
/// default output device in its mix format, and returns once the buffer has drained.
fn play(lead: u64, noise: &[f32], trail: u64, format: &WavFormat) -> Result<(), String> {
    let channels = usize::from(format.channels());
    let mut signal = vec![0.0f32; lead as usize * channels];
    signal.extend_from_slice(noise);
    signal.resize(signal.len() + trail as usize * channels, 0.0);
    let total_frames = signal.len() / channels;
    let err = |op: &'static str| move |e: windows::core::Error| format!("{op}: {e}");
    let device = win::default_render_device().map_err(err("GetDefaultAudioEndpoint"))?;
    // SAFETY: COM is initialized on this thread. The mix format pointer comes from
    // GetMixFormat, outlives Initialize and is freed once. Buffers are written within the
    // `n` frames requested from GetBuffer and released right away.
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).map_err(err("Activate"))?;
        let mix = client.GetMixFormat().map_err(err("GetMixFormat"))?;
        let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER_HNS, 0, mix, None);
        CoTaskMemFree(Some(mix.cast_const().cast()));
        init.map_err(err("Initialize"))?;
        let render: IAudioRenderClient = client.GetService().map_err(err("GetService"))?;
        let buffer = client.GetBufferSize().map_err(err("GetBufferSize"))?;
        let mut next = 0usize;
        let mut started = false;
        loop {
            let padding = client.GetCurrentPadding().map_err(err("GetCurrentPadding"))?;
            let n = ((buffer - padding) as usize).min(total_frames - next);
            if n > 0 {
                let dst = render.GetBuffer(n as u32).map_err(err("GetBuffer"))?;
                let src = signal[next * channels..].as_ptr().cast::<u8>();
                std::ptr::copy_nonoverlapping(src, dst, n * channels * 4);
                render.ReleaseBuffer(n as u32, 0).map_err(err("ReleaseBuffer"))?;
                next += n;
            }
            if !started {
                client.Start().map_err(err("Start"))?;
                started = true;
            }
            if next >= total_frames && padding as usize + n == 0 {
                break;
            }
            sleep(Duration::from_millis(10));
        }
        let _ = client.Stop();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_has_silence_around_the_compared_noise() {
        let format = WavFormat::float32_extensible(48_000, 2);
        let (lead, noise_samples, trail) = build_signal(&format);
        assert_eq!(lead, 24_000);
        assert_eq!(noise_samples, noise(240_000 * 2));
        assert_eq!(trail, 14_400);
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = noise(10_000);
        assert_eq!(a, noise(10_000));
        assert!(a.iter().all(|s| s.abs() <= AMPLITUDE));
        assert!(a.iter().any(|s| *s > 0.09) && a.iter().any(|s| *s < -0.09));
    }

    fn with_prefix(prefix: usize, signal: &[f32], gain: f32) -> Vec<f32> {
        let mut rec = vec![0.0; prefix];
        rec.extend(signal.iter().map(|s| s * gain));
        rec.extend([0.0; 100]);
        rec
    }

    #[test]
    fn finds_exact_offset_and_counts_all_identical() {
        let sig = noise(5_000);
        let rec = with_prefix(777, &sig, 1.0);
        let off = find_offset(&rec, &sig, PROBE).unwrap();
        assert_eq!(off, 777);
        assert_eq!(compare_exact(&rec, &sig, off), (5_000, 5_000));
    }

    #[test]
    fn no_offset_when_probe_differs_or_scaled() {
        let sig = noise(5_000);
        assert_eq!(find_offset(&with_prefix(50, &sig, 0.5), &sig, PROBE), None);
        let mut rec = with_prefix(50, &sig, 1.0);
        rec[50 + 10] += 1e-7;
        assert_eq!(find_offset(&rec, &sig, PROBE), None);
        assert_eq!(find_offset(&[], &sig, PROBE), None);
    }

    #[test]
    fn late_difference_is_counted() {
        let sig = noise(5_000);
        let mut rec = with_prefix(10, &sig, 1.0);
        rec[10 + 4_000] = f32::from_bits(rec[10 + 4_000].to_bits() ^ 1);
        let off = find_offset(&rec, &sig, PROBE).unwrap();
        assert_eq!(compare_exact(&rec, &sig, off), (4_999, 5_000));
    }

    #[test]
    fn truncated_recording_counts_missing_as_different() {
        let sig = noise(1_000);
        let rec = with_prefix(5, &sig, 1.0)[..505].to_vec();
        let off = find_offset(&rec, &sig, PROBE).unwrap();
        assert_eq!(compare_exact(&rec, &sig, off), (500, 1_000));
    }

    #[test]
    fn gain_estimate_of_a_scaled_delayed_copy() {
        let sig = noise(6_000);
        let rec = with_prefix(321, &sig, 0.5);
        let lag = find_lag(&rec, &sig, 1_024).unwrap();
        assert_eq!(lag, 321);
        let (gain, max_diff) = estimate_gain(&rec, &sig, lag);
        assert!((gain - 0.5).abs() < 1e-6);
        assert!((gain_db(gain) + 6.0206).abs() < 1e-3);
        assert!(max_diff > 0.04 && max_diff <= f64::from(AMPLITUDE) * 0.5 + 1e-6);
    }

    #[test]
    fn lag_is_none_for_silence() {
        let sig = noise(3_000);
        assert_eq!(find_lag(&vec![0.0; 5_000], &sig, 512), None);
    }
}
