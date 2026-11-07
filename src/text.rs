//! Textos de la interfaz en español.

use crate::wav::WavFormat;

/// `mm:ss.mmm` por debajo de una hora, `h:mm:ss.mmm` desde una hora.
pub fn fmt_time(frames: u64, rate: u32) -> String {
    let ms = frames as u128 * 1000 / rate.max(1) as u128;
    let (h, m, s, milli) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{milli:03}")
    } else {
        format!("{m:02}:{s:02}.{milli:03}")
    }
}

/// Pico en dBFS con un decimal, coma decimal y U+2212 para negativos.
pub fn fmt_peak(max_abs: f32) -> String {
    // NaN, zero and negative values all have no level.
    if max_abs.is_nan() || max_abs <= 0.0 {
        return "pico −∞ dBFS".to_string();
    }
    let tenths = (20.0 * (max_abs as f64).log10() * 10.0).round() as i64;
    let sign = match tenths {
        0 => "",
        t if t < 0 => "−",
        _ => "+",
    };
    let a = tenths.abs();
    format!("pico {sign}{},{} dBFS", a / 10, a % 10)
}

pub fn info_line(format: &WavFormat, max_abs: f32, frames: u64) -> String {
    format!(
        "{} · {} · {}",
        format.describe(),
        fmt_peak(max_abs),
        fmt_time(frames, format.sample_rate())
    )
}


pub fn cuts_text(n: u32) -> String {
    if n == 1 {
        "Toma con 1 corte".to_string()
    } else {
        format!("Toma con {n} cortes")
    }
}
