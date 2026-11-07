//! File naming, temp folder handling and file moves.

use crate::wav::part_path;
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use windows::Win32::System::SystemInformation::GetLocalTime;

/// Win32 error of a rename between two volumes (`ERROR_NOT_SAME_DEVICE`).
pub const ERROR_NOT_SAME_DEVICE: i32 = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub fn now_local() -> LocalTime {
    // SAFETY: GetLocalTime has no preconditions and returns the value directly.
    let t = unsafe { GetLocalTime() };
    LocalTime {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
    }
}

/// `%TEMP%\Teeth`, created if missing.
pub fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("Teeth");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn temp_file_name(t: &LocalTime) -> String {
    format!(
        "toma-{:04}{:02}{:02}-{:02}{:02}{:02}.wav",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// A path in `dir` that does not exist yet; appends `-2`, `-3`... on collision.
pub fn new_temp_path(dir: &Path, t: &LocalTime) -> PathBuf {
    let first = dir.join(temp_file_name(t));
    if !first.exists() {
        return first;
    }
    let stem = temp_file_name(t).trim_end_matches(".wav").to_string();
    (2u32..)
        .map(|n| dir.join(format!("{stem}-{n}.wav")))
        .find(|p| !p.exists())
        .expect("unbounded counter")
}

pub fn suggested_name(t: &LocalTime) -> String {
    format!(
        "toma {:04}-{:02}-{:02} {:02}-{:02}-{:02}.wav",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// The `.wav` files directly inside `dir`, most recently modified first (ignores `.part`
/// files and subfolders). Empty when `dir` does not exist or cannot be read.
pub fn orphans(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_wav = path
                .extension()
                .and_then(OsStr::to_str)
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
            let meta = entry.metadata().ok()?;
            (is_wav && meta.is_file()).then_some((meta.modified().ok()?, path))
        })
        .collect();
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, p)| p).collect()
}

/// Moves `src` over `dest`, replacing it. On the same volume this is one rename. Across
/// volumes `src` is copied to `dest.part`, which is then renamed over `dest`, and `src` is
/// deleted only after that. On failure `src` is untouched, `dest` keeps its old contents
/// and no `.part` remains.
pub fn move_or_copy(src: &Path, dest: &Path) -> io::Result<()> {
    move_or_copy_with(src, dest, |a, b| fs::rename(a, b), copy_durably)
}

/// `move_or_copy` with its rename and copy steps supplied by the caller.
pub fn move_or_copy_with(
    src: &Path,
    dest: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut copy: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    match rename(src, dest) {
        Err(e) if e.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE) => {}
        done => return done,
    }
    let part = part_path(dest);
    if let Err(e) = copy(src, &part).and_then(|()| rename(&part, dest)) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    // The audio is safe at `dest` now. A temp that cannot be deleted only means the next
    // start offers it again for recovery.
    let _ = fs::remove_file(src);
    Ok(())
}

/// Copies `src` to `dest` and flushes the copy to disk before it replaces anything.
fn copy_durably(src: &Path, dest: &Path) -> io::Result<()> {
    fs::copy(src, dest)?;
    OpenOptions::new().write(true).open(dest)?.sync_all()
}
