//! Where a trim handle cuts cleanly: the zero crossings around it, and what counts as silence.

/// A sample at this level or above is sound; below it, silence. It is −60 dBFS.
pub const SILENCE: f32 = 0.001;
/// How far a handle looks for a zero crossing on each side, in seconds.
pub const SEARCH_SECS: f64 = 0.01;

/// Whether a cut between samples `prev` and `next` of one channel is on a zero crossing.
fn crosses(prev: f32, next: f32) -> bool {
    prev == 0.0 || next == 0.0 || (prev < 0.0) != (next < 0.0)
}

/// The boundary between two frames of `samples` (interleaved, `channels` per frame) where a cut
/// near boundary `at` is cleanest: the nearest one where every channel crosses zero, or else
/// the one where the channels are closest to zero. Boundary `b` sits before frame `b`. Without
/// two frames there is no boundary, and it returns `at`.
pub fn nearest_zero(samples: &[f32], channels: usize, at: usize) -> usize {
    if channels == 0 {
        return at;
    }
    let frames: Vec<&[f32]> = samples.chunks_exact(channels).collect();
    let pairs = |b: usize| frames[b - 1].iter().zip(frames[b]);
    let all_cross = |b: &usize| pairs(*b).all(|(&prev, &next)| crosses(prev, next));
    let level = |b: usize| pairs(b).map(|(prev, next)| prev.abs() + next.abs()).sum::<f32>().abs();
    (1..frames.len())
        .filter(all_cross)
        .min_by_key(|b| b.abs_diff(at))
        .or_else(|| {
            (1..frames.len()).min_by(|&a, &b| level(a).total_cmp(&level(b)).then(a.abs_diff(at).cmp(&b.abs_diff(at))))
        })
        .unwrap_or(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handle_moves_to_the_nearest_crossing() {
        // Crossings at boundaries 2 (0.4 to -0.3) and 7 (-0.2 to 0.1).
        let s = [0.5, 0.4, -0.3, -0.6, -0.7, -0.5, -0.2, 0.1, 0.4];
        assert_eq!(nearest_zero(&s, 1, 3), 2);
        assert_eq!(nearest_zero(&s, 1, 5), 7);
    }

    #[test]
    fn the_earlier_crossing_wins_a_tie() {
        let s = [0.5, -0.5, -0.5, 0.5];
        assert_eq!(nearest_zero(&s, 1, 2), 1);
    }

    #[test]
    fn a_handle_on_a_crossing_stays() {
        let s = [0.5, 0.4, -0.3, -0.6];
        assert_eq!(nearest_zero(&s, 1, 2), 2);
    }

    #[test]
    fn digital_silence_counts_as_a_crossing() {
        let s = [0.5, 0.0, 0.0, 0.0, 0.5];
        assert_eq!(nearest_zero(&s, 1, 2), 2);
        // Into and out of silence, on either side of zero.
        assert_eq!(nearest_zero(&[-0.5, 0.0, -0.5], 1, 1), 1);
        assert_eq!(nearest_zero(&[-0.5, 0.0, -0.5], 1, 2), 2);
    }

    #[test]
    fn all_channels_crossing_together_beat_a_nearer_crossing_in_one() {
        // Left crosses at boundaries 1 and 3; right only at 3.
        let s = [0.5, 0.5, -0.5, 0.5, -0.5, 0.5, 0.5, -0.5];
        assert_eq!(nearest_zero(&s, 2, 1), 3);
    }

    #[test]
    fn opposite_channels_crossing_at_once_is_a_crossing() {
        let s = [0.5, -0.5, 0.4, -0.4, -0.3, 0.3, -0.6, 0.6];
        assert_eq!(nearest_zero(&s, 2, 1), 2);
    }

    #[test]
    fn without_a_crossing_it_goes_where_the_channels_are_closest_to_zero() {
        // An offset that never reaches zero: the quietest pair of frames is at boundary 3.
        let s = [0.9, 0.5, 0.1, 0.2, 0.8];
        assert_eq!(nearest_zero(&s, 1, 1), 3);
    }

    #[test]
    fn no_pair_of_frames_keeps_the_handle() {
        assert_eq!(nearest_zero(&[0.5], 1, 1), 1);
        assert_eq!(nearest_zero(&[], 1, 0), 0);
        assert_eq!(nearest_zero(&[0.5, 0.5], 0, 1), 1);
    }

    #[test]
    fn a_broken_sample_is_never_the_quietest() {
        let s = [0.9, f32::NAN, 0.1, 0.2];
        assert_eq!(nearest_zero(&s, 1, 1), 3);
    }
}
