//! Editing session: one open take, its peaks, selection and view; save, close and recovery.

use crate::edges::{SEARCH_SECS, SILENCE, nearest_zero};
use crate::files::move_or_copy;
use crate::peaks::PeakCache;
use crate::recorder::RecordedTake;
use crate::recorder::writer_loop::EndReason;
use crate::selection::Selection;
use crate::view::View;
use crate::wav::{WavError, WavReader, part_path, repair, write_range, write_trimmed};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const BLOCK_BYTES: u64 = 1024 * 1024;
/// Smallest file that can hold a WAV header (`RIFF`, `fmt ` and `data`). A shorter file
/// cannot contain audio.
const MIN_WAV_LEN: u64 = 44;

pub struct Take {
    pub path: PathBuf,
    pub is_temp: bool,
    pub saved: bool,
    pub discontinuities: u32,
    /// Where each cut starts, in frames; empty for a take opened from a file.
    pub cuts: Vec<u64>,
}

#[derive(Debug)]
pub enum SessionError {
    Empty,
    Wav(WavError),
    Io(io::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::Empty => write!(f, "la toma está vacía"),
            SessionError::Wav(e) => write!(f, "{e}"),
            SessionError::Io(e) => write!(f, "error de lectura/escritura: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<WavError> for SessionError {
    fn from(e: WavError) -> Self {
        SessionError::Wav(e)
    }
}

impl From<io::Error> for SessionError {
    fn from(e: io::Error) -> Self {
        SessionError::Io(e)
    }
}

pub struct Session {
    pub take: Take,
    pub reader: WavReader,
    pub peaks: PeakCache,
    pub selection: Selection,
    pub view: View,
    pub playhead: u64,
}

fn build_peaks(reader: &mut WavReader) -> io::Result<PeakCache> {
    let mut peaks = PeakCache::new(usize::from(reader.format().channels()));
    let total = reader.frames();
    let step = (BLOCK_BYTES / u64::from(reader.format().block_align().max(1))).max(1);
    let mut buf = Vec::new();
    let mut pos = 0;
    while pos < total {
        let n = step.min(total - pos);
        buf.clear();
        reader.read_f32(pos, n, &mut buf)?;
        peaks.push(&buf);
        pos += n;
    }
    Ok(peaks)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

impl Session {
    fn assemble(path: PathBuf, is_temp: bool, discontinuities: u32, cuts: Vec<u64>, reader: WavReader, peaks: PeakCache) -> Session {
        let total = reader.frames();
        Session {
            take: Take { path, is_temp, saved: false, discontinuities, cuts },
            reader,
            peaks,
            selection: Selection::new(total),
            view: View::new(total),
            playhead: 0,
        }
    }

    /// Takes over a finished recording. Deletes the temp file when it holds no audio.
    pub fn from_recording(rec: RecordedTake) -> Result<Session, SessionError> {
        let disk_error = matches!(rec.end, EndReason::DiskError(_));
        if disk_error {
            // The in-memory peaks may be ahead of what reached the disk.
            repair(&rec.path)?;
        }
        let mut reader = WavReader::open(&rec.path)?;
        if reader.frames() == 0 {
            reader.release();
            let _ = std::fs::remove_file(&rec.path);
            return Err(SessionError::Empty);
        }
        let peaks = if disk_error { build_peaks(&mut reader)? } else { rec.peaks };
        Ok(Session::assemble(rec.path, true, rec.discontinuities, rec.cuts, reader, peaks))
    }

    pub fn open(path: &Path, is_temp: bool) -> Result<Session, SessionError> {
        let mut reader = WavReader::open(path)?;
        if reader.frames() == 0 {
            return Err(SessionError::Empty);
        }
        let peaks = build_peaks(&mut reader)?;
        Ok(Session::assemble(path.to_path_buf(), is_temp, 0, Vec::new(), reader, peaks))
    }

    /// Opens a file from the disk as it is. It counts as saved: discarding it loses nothing, and
    /// closing it leaves the file where it was.
    pub fn open_existing(path: &Path) -> Result<Session, SessionError> {
        let mut session = Session::open(path, false)?;
        session.take.saved = true;
        Ok(session)
    }

    /// Whether `path` is the file this take lives in.
    pub fn holds_file(&self, path: &Path) -> bool {
        same_file(path, &self.take.path)
    }

    /// Repairs a stale header and opens the orphan. An orphan without audio (no frames, or
    /// too short to hold a header) is deleted and reported as `Empty`.
    pub fn recover(path: &Path) -> Result<Session, SessionError> {
        if std::fs::metadata(path)?.len() < MIN_WAV_LEN || repair(path)? == 0 {
            let _ = std::fs::remove_file(path);
            return Err(SessionError::Empty);
        }
        Session::open(path, true)
    }

    /// Recovers the first of `orphans` (newest first) that opens. Orphans without audio are
    /// deleted on the way; one that fails is kept, returned with its error, and the next
    /// one is tried.
    pub fn recover_first(orphans: impl IntoIterator<Item = PathBuf>) -> (Option<Session>, Vec<(PathBuf, SessionError)>) {
        let mut failed = Vec::new();
        for path in orphans {
            match Session::recover(&path) {
                Ok(session) => return (Some(session), failed),
                Err(SessionError::Empty) => {}
                Err(e) => failed.push((path, e)),
            }
        }
        (None, failed)
    }

    fn reopen(&mut self) -> Result<(), SessionError> {
        self.reader = WavReader::open(&self.take.path)?;
        Ok(())
    }

    pub fn save(&mut self, dest: &Path) -> Result<(), SessionError> {
        let same = dest.exists() && same_file(dest, &self.take.path);
        if self.selection.is_full() && self.take.is_temp && !same {
            self.reader.release();
            if let Err(e) = move_or_copy(&self.take.path, dest) {
                self.reopen()?;
                return Err(e.into());
            }
            self.take.path = dest.to_path_buf();
            self.take.is_temp = false;
            self.reopen()?;
        } else if same {
            let part = part_path(dest);
            write_range(&mut self.reader, self.selection.start, self.selection.end, &part)?;
            self.reader.release();
            if let Err(e) = std::fs::rename(&part, &self.take.path) {
                let _ = std::fs::remove_file(&part);
                self.reopen()?;
                return Err(e.into());
            }
            self.reopen()?;
            self.peaks = build_peaks(&mut self.reader)?;
            let total = self.reader.frames();
            self.selection = Selection::new(total);
            self.view.set_total(total);
            self.view.fit();
            self.playhead = 0;
        } else {
            write_trimmed(&mut self.reader, self.selection.start, self.selection.end, dest)?;
        }
        self.take.saved = true;
        Ok(())
    }

    /// Ends the session; a temp take is deleted.
    pub fn close(mut self) -> io::Result<()> {
        self.reader.release();
        if self.take.is_temp {
            std::fs::remove_file(&self.take.path)?;
        }
        Ok(())
    }

    pub fn read_raw(&mut self, from: u64, to: u64) -> io::Result<Vec<f32>> {
        let mut out = Vec::new();
        self.reader.read_f32(from, to.saturating_sub(from), &mut out)?;
        Ok(out)
    }

    /// The boundary near `at` where a cut is cleanest. The edges of the take stay where they are.
    fn zero_near(&mut self, at: u64) -> io::Result<u64> {
        let total = self.reader.frames();
        if at == 0 || at >= total {
            return Ok(at);
        }
        let format = self.reader.format();
        let channels = usize::from(format.channels());
        let reach = (SEARCH_SECS * f64::from(format.sample_rate())).round() as u64;
        // One frame more on each side: a boundary needs the frames before and after it.
        let from = at.saturating_sub(reach + 1);
        let samples = self.read_raw(from, (at + reach + 1).min(total))?;
        Ok(from + nearest_zero(&samples, channels, (at - from) as usize) as u64)
    }

    /// Moves both handles to their nearest zero crossing. Handles that would meet stay.
    pub fn snap_to_zero(&mut self) -> io::Result<()> {
        let start = self.zero_near(self.selection.start)?;
        let end = self.zero_near(self.selection.end)?;
        if start < end {
            self.selection.start = start;
            self.selection.end = end;
        }
        Ok(())
    }

    /// Moves the handles inward to the first and last sound of the selection. Returns `false`,
    /// with the handles where they were, when the selection is only silence.
    pub fn trim_silence(&mut self) -> io::Result<bool> {
        let (start, end) = (self.selection.start, self.selection.end);
        let reader = &mut self.reader;
        let mut raw = |a: u64, b: u64| {
            let mut out = Vec::new();
            reader.read_f32(a, b.saturating_sub(a), &mut out)?;
            Ok(out)
        };
        let Some(first) = self.peaks.first_above(start, end, SILENCE, &mut raw)? else { return Ok(false) };
        let last = self.peaks.last_above(start, end, SILENCE, &mut raw)?.unwrap_or(first);
        self.selection.start = first;
        self.selection.end = last + 1;
        Ok(true)
    }

    /// Peak (absolute) of the current selection.
    pub fn selection_peak(&mut self) -> f32 {
        let (start, end) = (self.selection.start, self.selection.end);
        let reader = &mut self.reader;
        self.peaks.max_abs(start, end, &mut |a, b| {
            let mut out = Vec::new();
            let _ = reader.read_f32(a, b.saturating_sub(a), &mut out);
            out
        })
    }
}
