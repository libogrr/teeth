//! Tape variants: reverse, and pitch by changing the rate in the header.

use crate::wav::WavFormat;

/// Largest pitch shift in either direction: one octave.
pub const MAX_SEMITONES: i8 = 12;

/// How a take is heard or exported. It only reorders whole frames or changes
/// the rate in the header; it never recalculates samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Variant {
    pub semitones: i8,
    pub reversed: bool,
}

impl Variant {
    /// No pitch and no reverse: the audio as it was recorded.
    pub fn is_faithful(self) -> bool {
        self == Variant::default()
    }

    /// The header rate: `round(rate * 2^(semitones / 12))`, exactly `rate` at 0 semitones.
    pub fn rate(self, rate: u32) -> u32 {
        if self.semitones == 0 {
            return rate;
        }
        (f64::from(rate) * (f64::from(self.semitones) / 12.0).exp2()).round() as u32
    }

    /// Frames per second the take is walked at; negative in reverse.
    pub fn velocity(self, rate: u32) -> f64 {
        let v = f64::from(self.rate(rate));
        if self.reversed { -v } else { v }
    }

    /// The format with the variant's rate. With 0 semitones it is an untouched clone.
    pub fn format(self, format: &WavFormat) -> WavFormat {
        if self.semitones == 0 {
            return format.clone();
        }
        format.with_rate(self.rate(format.sample_rate()))
    }

    /// File name suffix such as ` -3st rev`; empty when faithful.
    pub fn name_suffix(self) -> String {
        let mut s = String::new();
        if self.semitones != 0 {
            s.push_str(&format!(" {:+}st", self.semitones));
        }
        if self.reversed {
            s.push_str(" rev");
        }
        s
    }

    /// The pitch label, with a real minus sign (U+2212).
    pub fn label(self) -> String {
        match self.semitones {
            0 => "0 st".to_string(),
            n if n < 0 => format!("\u{2212}{} st", -i16::from(n)),
            n => format!("+{n} st"),
        }
    }

    /// Moves the pitch by `delta` semitones, stopping at an octave, and keeps the reverse.
    pub fn stepped(self, delta: i8) -> Variant {
        let semitones = self.semitones.saturating_add(delta).clamp(-MAX_SEMITONES, MAX_SEMITONES);
        Variant { semitones, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(semitones: i8) -> Variant {
        Variant { semitones, reversed: false }
    }

    #[test]
    fn the_rate_follows_the_semitones() {
        assert_eq!(st(-3).rate(48_000), 40_363);
        assert_eq!(st(7).rate(48_000), 71_919);
        assert_eq!(st(-12).rate(48_000), 24_000);
        assert_eq!(st(12).rate(48_000), 96_000);
        assert_eq!(st(-12).rate(44_100), 22_050);
        assert_eq!(st(12).rate(44_100), 88_200);
        assert_eq!(st(0).rate(44_100), 44_100);
        assert_eq!(st(0).velocity(48_000), 48_000.0);
        assert_eq!(Variant { semitones: -3, reversed: true }.velocity(48_000), -40_363.0);
    }

    #[test]
    fn names_and_labels_say_the_variant() {
        let rev = |semitones| Variant { semitones, reversed: true };
        assert_eq!((rev(-3).name_suffix(), rev(-3).label()), (" -3st rev".to_string(), "−3 st".to_string()));
        assert_eq!((st(7).name_suffix(), st(7).label()), (" +7st".to_string(), "+7 st".to_string()));
        assert_eq!((rev(0).name_suffix(), rev(0).label()), (" rev".to_string(), "0 st".to_string()));
        assert_eq!((st(0).name_suffix(), st(0).label()), (String::new(), "0 st".to_string()));
    }

    #[test]
    fn steps_stop_at_an_octave_and_keep_the_reverse() {
        assert_eq!(st(-12).stepped(-1), st(-12));
        assert_eq!(st(12).stepped(1), st(12));
        assert_eq!(st(0).stepped(-1), st(-1));
        let rev = Variant { semitones: 2, reversed: true };
        assert_eq!(rev.stepped(1), Variant { semitones: 3, reversed: true });
    }

    #[test]
    fn only_no_pitch_and_no_reverse_is_faithful() {
        assert!(Variant::default().is_faithful());
        assert!(!st(1).is_faithful());
        assert!(!Variant { semitones: 0, reversed: true }.is_faithful());
    }

    #[test]
    fn no_pitch_keeps_the_fmt_bytes_even_when_they_are_inconsistent() {
        let mut bytes = WavFormat::pcm(48_000, 2, 16).fmt_chunk().to_vec();
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        let f = WavFormat::from_waveformatex_bytes(&bytes).unwrap();
        assert_eq!(Variant { semitones: 0, reversed: true }.format(&f).fmt_chunk(), &bytes[..]);
        assert_eq!(st(-3).format(&f).sample_rate(), 40_363);
    }
}
