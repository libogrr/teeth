//! Multi-level min/max peak cache for fast, exact waveform drawing.
//!
//! Level `L` groups `BASE_BIN * FANOUT^L` frames per bin. Only bins that
//! contain at least one frame exist. Completed bins are stored; the partial
//! (last) bin of each level is derived on demand from the levels below.

use std::io;

type Peak = (f32, f32);

fn merge(a: Peak, b: Peak) -> Peak {
    (a.0.min(b.0), a.1.max(b.1))
}

#[derive(Clone, Copy)]
struct Partial {
    min: f32,
    max: f32,
    count: u64,
}

impl Partial {
    const EMPTY: Partial = Partial { min: f32::INFINITY, max: f32::NEG_INFINITY, count: 0 };
}

pub struct PeakCache {
    channels: usize,
    frames: u64,
    /// `done[level][channel]`: completed bins of that level.
    done: Vec<Vec<Vec<Peak>>>,
    /// Per-channel partial level-0 bin being filled.
    partial0: Vec<Partial>,
}

impl PeakCache {
    pub const BASE_BIN: u64 = 256;
    pub const FANOUT: u64 = 4;

    pub fn new(channels: usize) -> PeakCache {
        PeakCache {
            channels,
            frames: 0,
            done: Vec::new(),
            partial0: vec![Partial::EMPTY; channels],
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Number of levels that hold at least one bin (complete or partial).
    pub fn levels(&self) -> usize {
        if self.frames == 0 {
            return 0;
        }
        1 + self.done.iter().take_while(|l| !l[0].is_empty()).count()
    }

    pub fn push(&mut self, interleaved: &[f32]) {
        let chans = self.channels;
        if chans == 0 {
            return;
        }
        for frame in interleaved.chunks_exact(chans) {
            for (ch, &v) in frame.iter().enumerate() {
                let p = &mut self.partial0[ch];
                p.min = p.min.min(v);
                p.max = p.max.max(v);
                p.count += 1;
            }
            self.frames += 1;
            if self.partial0[0].count == Self::BASE_BIN {
                self.complete_level0();
            }
        }
    }

    fn complete_level0(&mut self) {
        if self.done.is_empty() {
            self.done.push(vec![Vec::new(); self.channels]);
        }
        for ch in 0..self.channels {
            let p = std::mem::replace(&mut self.partial0[ch], Partial::EMPTY);
            self.done[0][ch].push((p.min, p.max));
        }
        // Cascade: every FANOUT completed bins of level L make one of L+1.
        let mut level = 0;
        while (self.done[level][0].len() as u64).is_multiple_of(Self::FANOUT) {
            if self.done.len() == level + 1 {
                self.done.push(vec![Vec::new(); self.channels]);
            }
            for ch in 0..self.channels {
                let src = &self.done[level][ch];
                let merged = src[src.len() - Self::FANOUT as usize..]
                    .iter()
                    .copied()
                    .reduce(merge)
                    .unwrap();
                self.done[level + 1][ch].push(merged);
            }
            level += 1;
        }
    }

    fn bin_size(level: usize) -> u64 {
        Self::BASE_BIN * Self::FANOUT.pow(level as u32)
    }

    /// Partial (last, incomplete) bin of `level`, built from the levels below.
    fn partial(&self, level: usize, ch: usize) -> Option<Peak> {
        if level == 0 {
            let p = self.partial0[ch];
            return (p.count > 0).then_some((p.min, p.max));
        }
        let below_done = self.done.get(level - 1).map(|l| &l[ch][..]).unwrap_or(&[]);
        let folded = self.done.get(level).map_or(0, |l| l[ch].len()) * Self::FANOUT as usize;
        let rest = below_done.get(folded..).unwrap_or(&[]);
        rest.iter().copied().reduce(merge).into_iter().chain(self.partial(level - 1, ch)).reduce(merge)
    }

    fn bin_count(&self, level: usize) -> u64 {
        self.frames.div_ceil(Self::bin_size(level))
    }

    fn bin(&self, level: usize, ch: usize, i: u64) -> Peak {
        let done = self.done.get(level).map(|l| &l[ch][..]).unwrap_or(&[]);
        match done.get(i as usize) {
            Some(&p) => p,
            None => self.partial(level, ch).expect("bin index within range"),
        }
    }

    pub fn bins(&self, level: usize, channel: usize) -> Vec<(f32, f32)> {
        if channel >= self.channels || level >= self.levels() {
            return Vec::new();
        }
        let mut out = self.done.get(level).map(|l| l[channel].clone()).unwrap_or_default();
        out.extend(self.partial(level, channel));
        out
    }

    pub fn columns(&self, channel: usize, from: u64, to: u64, n: usize) -> Option<Vec<(f32, f32)>> {
        if n == 0 || to <= from || channel >= self.channels || self.frames == 0 {
            return None;
        }
        let span = to - from;
        let per_col = span / n as u64;
        if per_col < Self::BASE_BIN {
            return None;
        }
        let mut level = 0;
        while level + 1 < self.levels() && Self::bin_size(level + 1) <= per_col {
            level += 1;
        }
        let size = Self::bin_size(level);
        let last_bin = self.bin_count(level) - 1;
        let mut out = Vec::with_capacity(n);
        for c in 0..n as u64 {
            let a = from + (c as u128 * span as u128 / n as u128) as u64;
            let b = from + ((c + 1) as u128 * span as u128 / n as u128) as u64;
            let (a, b) = (a.min(self.frames), b.min(self.frames));
            if a >= b {
                out.push((0.0, 0.0));
                continue;
            }
            let first = (a / size).min(last_bin);
            let last = ((b - 1) / size).min(last_bin);
            let peak = (first..=last)
                .map(|i| self.bin(level, channel, i))
                .reduce(merge)
                .unwrap();
            out.push(peak);
        }
        Some(out)
    }

    /// The first frame of `[from, to)` with a sample at `level` or above in any channel. The
    /// cache rules out the bins without one; `raw` reads only the bins that may have it.
    pub fn first_above(
        &self,
        from: u64,
        to: u64,
        level: f32,
        raw: &mut dyn FnMut(u64, u64) -> io::Result<Vec<f32>>,
    ) -> io::Result<Option<u64>> {
        self.find_above(from, to, level, false, raw)
    }

    /// The last frame of `[from, to)` with a sample at `level` or above in any channel, read as
    /// `first_above` does.
    pub fn last_above(
        &self,
        from: u64,
        to: u64,
        level: f32,
        raw: &mut dyn FnMut(u64, u64) -> io::Result<Vec<f32>>,
    ) -> io::Result<Option<u64>> {
        self.find_above(from, to, level, true, raw)
    }

    fn find_above(
        &self,
        from: u64,
        to: u64,
        level: f32,
        backwards: bool,
        raw: &mut dyn FnMut(u64, u64) -> io::Result<Vec<f32>>,
    ) -> io::Result<Option<u64>> {
        let to = to.min(self.frames);
        if from >= to || self.channels == 0 {
            return Ok(None);
        }
        let size = Self::BASE_BIN;
        let (first, last) = (from / size, (to - 1) / size);
        let reaches = |v: f32| v.abs() >= level;
        for n in 0..=last - first {
            let i = if backwards { last - n } else { first + n };
            let lit = (0..self.channels).any(|ch| {
                let (lo, hi) = self.bin(0, ch, i);
                reaches(lo) || reaches(hi)
            });
            if !lit {
                continue;
            }
            let a = (i * size).max(from);
            let samples = raw(a, ((i + 1) * size).min(to))?;
            let mut frames = samples.chunks_exact(self.channels);
            let loud = |frame: &[f32]| frame.iter().any(|&v| reaches(v));
            let found = if backwards { frames.rposition(loud) } else { frames.position(loud) };
            if let Some(at) = found {
                return Ok(Some(a + at as u64));
            }
        }
        Ok(None)
    }

    pub fn max_abs(&self, from: u64, to: u64, raw: &mut dyn FnMut(u64, u64) -> Vec<f32>) -> f32 {
        let to = to.min(self.frames);
        if from >= to {
            return 0.0;
        }
        let abs_max = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let size = Self::BASE_BIN;
        // First fully covered bin starts at `s`; coverage ends at `e` (the final
        // partial bin counts as covered when `to` reaches the end of the data).
        let s = from.div_ceil(size) * size;
        let e = if to == self.frames { to } else { to / size * size };
        if s >= e {
            return abs_max(&raw(from, to));
        }
        let mut m = 0.0f32;
        if from < s {
            m = m.max(abs_max(&raw(from, s)));
        }
        if e < to {
            m = m.max(abs_max(&raw(e, to)));
        }
        let first = s / size;
        let end = e.div_ceil(size);
        for ch in 0..self.channels {
            for i in first..end {
                let (lo, hi) = self.bin(0, ch, i);
                m = m.max(lo.abs()).max(hi.abs());
            }
        }
        m
    }
}
