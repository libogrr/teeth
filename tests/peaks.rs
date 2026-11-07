mod common;

use common::XorShift;
use teeth::peaks::PeakCache;

fn brute(sig: &[f32], ch: usize, chans: usize, a: u64, b: u64) -> (f32, f32) {
    let mut mn = f32::INFINITY;
    let mut mx = f32::NEG_INFINITY;
    for f in a..b {
        let v = sig[f as usize * chans + ch];
        mn = mn.min(v);
        mx = mx.max(v);
    }
    (mn, mx)
}

fn make_sig(len: u64, chans: usize, seed: u64) -> Vec<f32> {
    let mut rng = XorShift::new(seed);
    (0..len as usize * chans).map(|_| rng.next_f32()).collect()
}

#[test]
fn bins_match_brute_force_on_every_level() {
    for &len in &[1u64, 255, 256, 257, 70_001, 300_000] {
        let sig = make_sig(len, 2, 42 + len);
        let mut cache = PeakCache::new(2);
        for chunk in sig.chunks(997 * 2) {
            cache.push(chunk);
        }
        assert_eq!(cache.frames(), len);
        assert_eq!(cache.channels(), 2);
        assert!(cache.levels() >= 1);
        for level in 0..cache.levels() {
            let size = PeakCache::BASE_BIN * PeakCache::FANOUT.pow(level as u32);
            for ch in 0..2 {
                let bins = cache.bins(level, ch);
                let expected_len = len.div_ceil(size) as usize;
                assert_eq!(bins.len(), expected_len, "len {len} level {level}");
                for (i, bin) in bins.iter().enumerate() {
                    let i = i as u64;
                    let want = brute(&sig, ch, 2, i * size, ((i + 1) * size).min(len));
                    assert_eq!(*bin, want, "len {len} level {level} ch {ch} bin {i}");
                }
            }
        }
    }
}

#[test]
fn columns_contain_exact_and_stay_within_touched_bins() {
    let len = 300_000u64;
    let sig = make_sig(len, 1, 7);
    let mut cache = PeakCache::new(1);
    for chunk in sig.chunks(997) {
        cache.push(chunk);
    }
    let (from, to, n) = (1234u64, 290_000u64, 333usize);
    let cols = cache.columns(0, from, to, n).expect("zoom is sufficient");
    assert_eq!(cols.len(), n);
    let span = to - from;
    for (c, r) in cols.iter().enumerate() {
        let c = c as u64;
        let a = from + c * span / n as u64;
        let b = from + (c + 1) * span / n as u64;
        let exact = brute(&sig, 0, 1, a, b);
        assert!(r.0 <= exact.0 && r.1 >= exact.1, "column {c}");
        let lo = a / 256 * 256;
        let hi = (b.div_ceil(256) * 256).min(len);
        let outer = brute(&sig, 0, 1, lo, hi);
        assert!(r.0 >= outer.0 && r.1 <= outer.1, "column {c} outside touched bins");
    }
}

fn merge(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    (a.0.min(b.0), a.1.max(b.1))
}

/// Exact (min, max) of a run of samples of one channel.
fn extremes(samples: &[f32]) -> (f32, f32) {
    samples.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(mn, mx), &v| (mn.min(v), mx.max(v)))
}

/// Brute-force extremes of any frame range of one channel, sped up with exact extremes of
/// fixed 1024-frame blocks computed straight from the samples (independent of the cache).
struct Oracle {
    lane: Vec<f32>,
    blocks: Vec<(f32, f32)>,
}

impl Oracle {
    const BLOCK: usize = 1024;

    fn new(sig: &[f32], ch: usize, chans: usize) -> Oracle {
        let lane: Vec<f32> = sig.iter().skip(ch).step_by(chans).copied().collect();
        let blocks = lane.chunks(Self::BLOCK).map(extremes).collect();
        Oracle { lane, blocks }
    }

    fn range(&self, a: u64, b: u64) -> (f32, f32) {
        let (a, b) = (a as usize, b as usize);
        let (s, e) = (a.div_ceil(Self::BLOCK), b / Self::BLOCK);
        if s >= e {
            return extremes(&self.lane[a..b]);
        }
        let edges = merge(extremes(&self.lane[a..s * Self::BLOCK]), extremes(&self.lane[e * Self::BLOCK..b]));
        self.blocks[s..e].iter().copied().fold(edges, merge)
    }
}

fn bin_size(level: usize) -> u64 {
    PeakCache::BASE_BIN * PeakCache::FANOUT.pow(level as u32)
}

/// The level `columns` reads for `per_col` frames per column: the coarsest whose bins fit.
fn level_for(cache: &PeakCache, per_col: u64) -> usize {
    let mut level = 0;
    while level + 1 < cache.levels() && bin_size(level + 1) <= per_col {
        level += 1;
    }
    level
}

/// Noise whose loudness changes every 50 000 frames and per channel, with spikes on bin
/// edges, so that neighbouring columns have different extremes.
fn varied_sig(len: u64, chans: usize, seed: u64) -> Vec<f32> {
    let mut rng = XorShift::new(seed);
    let mut sig: Vec<f32> = (0..len as usize * chans)
        .map(|i| {
            let (f, ch) = ((i / chans) as u64, (i % chans) as u64);
            let env = 0.05 + 0.95 * ((f / 50_000 + ch) % 7) as f32 / 6.0;
            rng.next_f32() * env
        })
        .collect();
    let spikes = [1_023u64, 1_024, 4_095, 16_384, 65_535, 262_144, 1_048_575, 1_048_576, len - 1];
    for (k, &f) in spikes.iter().enumerate() {
        let ch = k % chans;
        sig[f as usize * chans + ch] = if k % 2 == 0 { 1.9 } else { -1.9 };
    }
    sig
}

#[test]
fn columns_match_brute_force_on_upper_levels() {
    for (chans, len) in [(1usize, 3_000_001u64), (3, 2_000_003)] {
        let sig = varied_sig(len, chans, 99 + len);
        let mut cache = PeakCache::new(chans);
        for chunk in sig.chunks(48_000 * chans) {
            cache.push(chunk);
        }
        let oracles: Vec<Oracle> = (0..chans).map(|ch| Oracle::new(&sig, ch, chans)).collect();
        // (from, to, columns, level that must be read)
        let cases = [
            (0, len, (len / 2_000) as usize, 1),
            (0, len, (len / 9_000) as usize, 2),
            (0, len, (len / 40_000) as usize, 3),
            (0, len, (len / 100_000) as usize, 4),
            (0, len, 7, 5),
            (0, len, 2, if len > 2 * bin_size(6) { 6 } else { 5 }),
            (777_777, 777_777 + 1_234_567, 300, 2),
            (777_777, 777_777 + 1_234_567, 60, 3),
            (len / 3 + 123, len + len / 2, (len / 5_000) as usize, 2), // runs past the end
        ];
        for (from, to, n, want_level) in cases {
            let span = to - from;
            let level = level_for(&cache, span / n as u64);
            assert_eq!(level, want_level, "{chans} ch, {from}..{to} in {n}");
            let size = bin_size(level);
            for (ch, oracle) in oracles.iter().enumerate() {
                let cols = cache.columns(ch, from, to, n).expect("zoomed out enough");
                assert_eq!(cols.len(), n);
                for (c, &r) in cols.iter().enumerate() {
                    let c = c as u64;
                    let a = (from + c * span / n as u64).min(len);
                    let b = (from + (c + 1) * span / n as u64).min(len);
                    let at = format!("{chans} ch, {from}..{to} in {n}, ch {ch}, column {c}");
                    if a >= b {
                        assert_eq!(r, (0.0, 0.0), "{at}: past the end");
                        continue;
                    }
                    let exact = oracle.range(a, b);
                    assert!(r.0 <= exact.0 && r.1 >= exact.1, "{at}: misses the exact extremes");
                    let lo = a / size * size;
                    let hi = (b.div_ceil(size) * size).min(len);
                    let touched = oracle.range(lo, hi);
                    assert!(r.0 >= touched.0 && r.1 <= touched.1, "{at}: outside touched bins");
                }
            }
        }
    }
}

#[test]
fn columns_none_when_too_zoomed() {
    let sig = make_sig(10_000, 1, 3);
    let mut cache = PeakCache::new(1);
    cache.push(&sig);
    assert_eq!(cache.columns(0, 0, 10_000, 100), None);
}

#[test]
fn max_abs_is_exact_at_partial_edges() {
    let mut sig: Vec<f32> = make_sig(10_000, 2, 11).iter().map(|v| v / 3.0).collect();
    sig[300 * 2 + 1] = -1.75;
    sig[9_900 * 2] = 1.9;
    let mut cache = PeakCache::new(2);
    for chunk in sig.chunks(997 * 2) {
        cache.push(chunk);
    }
    let mut raw = |a: u64, b: u64| sig[a as usize * 2..b as usize * 2].to_vec();
    assert!(cache.max_abs(301, 9_000, &mut raw) < 1.75);
    assert_eq!(cache.max_abs(300, 9_000, &mut raw), 1.75);
    assert_eq!(cache.max_abs(0, 10_000, &mut raw), 1.9);
}

#[test]
fn new_with_zero_channels_does_not_panic() {
    let cache = PeakCache::new(0);
    assert_eq!(cache.frames(), 0);
    assert_eq!(cache.levels(), 0);
}

/// Interleaved silence of `frames` frames of `chans` channels, with `sound` as (frame, channel,
/// value), and its cache.
fn with_sound(frames: u64, chans: usize, sound: &[(u64, usize, f32)]) -> (Vec<f32>, PeakCache) {
    let mut sig = vec![0.0; frames as usize * chans];
    for &(f, ch, v) in sound {
        sig[f as usize * chans + ch] = v;
    }
    let mut cache = PeakCache::new(chans);
    cache.push(&sig);
    (sig, cache)
}

fn reader(sig: &[f32], chans: usize) -> impl FnMut(u64, u64) -> std::io::Result<Vec<f32>> + '_ {
    move |a, b| Ok(sig[a as usize * chans..b as usize * chans].to_vec())
}

/// First and last frame at `level` or above in `[from, to)`.
fn bounds(sig: &[f32], cache: &PeakCache, from: u64, to: u64, level: f32) -> (Option<u64>, Option<u64>) {
    let chans = cache.channels();
    (
        cache.first_above(from, to, level, &mut reader(sig, chans)).unwrap(),
        cache.last_above(from, to, level, &mut reader(sig, chans)).unwrap(),
    )
}

#[test]
fn the_first_and_last_sound_are_found_in_any_channel() {
    let (sig, cache) = with_sound(5_000, 2, &[(1_000, 1, 0.5), (3_999, 0, -0.5)]);
    assert_eq!(bounds(&sig, &cache, 0, 5_000, 0.001), (Some(1_000), Some(3_999)));
}

#[test]
fn a_sample_at_the_level_is_sound_and_just_below_it_is_silence() {
    let (sig, cache) = with_sound(1_000, 1, &[(300, 0, 0.001), (600, 0, -0.001)]);
    assert_eq!(bounds(&sig, &cache, 0, 1_000, 0.001), (Some(300), Some(600)));
    let (sig, cache) = with_sound(1_000, 1, &[(300, 0, 0.000_99), (600, 0, -0.000_99)]);
    assert_eq!(bounds(&sig, &cache, 0, 1_000, 0.001), (None, None));
}

#[test]
fn sound_outside_the_range_does_not_count() {
    let (sig, cache) = with_sound(5_000, 1, &[(1_000, 0, 0.5), (3_000, 0, 0.5)]);
    assert_eq!(bounds(&sig, &cache, 1_001, 3_000, 0.001), (None, None));
    assert_eq!(bounds(&sig, &cache, 1_000, 3_001, 0.001), (Some(1_000), Some(3_000)));
    assert_eq!(bounds(&sig, &cache, 1_001, 3_001, 0.001), (Some(3_000), Some(3_000)));
}

#[test]
fn sound_in_the_same_bin_as_the_edge_of_the_range_does_not_count() {
    // Frame 100 lights up the bin, but the range starts after it and ends before frame 200.
    let (sig, cache) = with_sound(1_000, 1, &[(100, 0, 0.5), (200, 0, 0.5)]);
    assert_eq!(bounds(&sig, &cache, 101, 200, 0.001), (None, None));
}

#[test]
fn the_unfinished_last_bin_counts() {
    let (sig, cache) = with_sound(300, 1, &[(299, 0, 0.5)]);
    assert_eq!(bounds(&sig, &cache, 0, 300, 0.001), (Some(299), Some(299)));
    assert_eq!(bounds(&sig, &cache, 0, 9_000, 0.001), (Some(299), Some(299)));
}

#[test]
fn an_empty_range_has_no_sound() {
    let (sig, cache) = with_sound(300, 1, &[(100, 0, 0.5)]);
    assert_eq!(bounds(&sig, &cache, 100, 100, 0.001), (None, None));
    assert_eq!(bounds(&sig, &cache, 400, 500, 0.001), (None, None));
}

#[test]
fn looking_for_sound_reads_only_the_bins_that_have_it() {
    let (sig, cache) = with_sound(1_000_000, 2, &[(500_000, 0, 0.5)]);
    let mut read = 0;
    let mut raw = |a: u64, b: u64| {
        read += b - a;
        Ok(sig[a as usize * 2..b as usize * 2].to_vec())
    };
    assert_eq!(cache.first_above(0, 1_000_000, 0.001, &mut raw).unwrap(), Some(500_000));
    assert_eq!(cache.last_above(0, 1_000_000, 0.001, &mut raw).unwrap(), Some(500_000));
    assert_eq!(read, 2 * PeakCache::BASE_BIN);
}

#[test]
fn a_failed_read_is_not_silence() {
    let (_, cache) = with_sound(1_000, 1, &[(100, 0, 0.5)]);
    let mut raw = |_: u64, _: u64| Err(std::io::Error::other("disk"));
    assert!(cache.first_above(0, 1_000, 0.001, &mut raw).is_err());
    assert!(cache.last_above(0, 1_000, 0.001, &mut raw).is_err());
}
