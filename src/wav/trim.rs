use super::{WavError, WavReader, WavWriter};
use crate::variant::Variant;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const COPY_BYTES: u64 = 1024 * 1024;

/// `x.wav` -> `x.wav.part`.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s: OsString = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn copy_range(src: &mut WavReader, start: u64, end: u64, out: &Path) -> Result<(), WavError> {
    if start > end || end > src.frames() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "range outside the audio").into());
    }
    let mut w = WavWriter::create(out, src.format())?;
    let step = (COPY_BYTES / u64::from(src.format().block_align().max(1))).max(1);
    let mut pos = start;
    while pos < end {
        let n = step.min(end - pos);
        w.write_audio(&src.read_frames(pos, n)?)?;
        pos += n;
    }
    w.finish()?;
    Ok(())
}

/// Copies frames `[start, end)` byte for byte into a new WAV at `out`.
/// On failure `out` is removed.
pub fn write_range(src: &mut WavReader, start: u64, end: u64, out: &Path) -> Result<(), WavError> {
    let r = copy_range(src, start, end, out);
    if r.is_err() {
        let _ = fs::remove_file(out);
    }
    r
}

/// Writes the range to `dest.part` and then renames it over `dest`.
pub fn write_trimmed(src: &mut WavReader, start: u64, end: u64, dest: &Path) -> Result<(), WavError> {
    let part = part_path(dest);
    write_range(src, start, end, &part)?;
    if let Err(e) = fs::rename(&part, dest) {
        let _ = fs::remove_file(&part);
        return Err(e.into());
    }
    Ok(())
}

/// Reverses the order of the frames in `bytes`, which holds whole frames of
/// `block_align` bytes; the bytes inside each frame keep their order.
pub fn reverse_frames(bytes: &mut [u8], block_align: usize) {
    bytes.reverse();
    for frame in bytes.chunks_exact_mut(block_align.max(1)) {
        frame.reverse();
    }
}

fn copy_variant(src: &mut WavReader, start: u64, end: u64, variant: Variant, out: &Path) -> Result<(), WavError> {
    if start > end || end > src.frames() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "range outside the audio").into());
    }
    let block_align = src.format().block_align().max(1);
    let mut w = WavWriter::create(out, &variant.format(src.format()))?;
    let step = (COPY_BYTES / u64::from(block_align)).max(1);
    let total = end - start;
    let mut done = 0;
    while done < total {
        let n = step.min(total - done);
        if variant.reversed {
            let mut block = src.read_frames(end - done - n, n)?;
            reverse_frames(&mut block, usize::from(block_align));
            w.write_audio(&block)?;
        } else {
            w.write_audio(&src.read_frames(start + done, n)?)?;
        }
        done += n;
    }
    w.finish()?;
    Ok(())
}

/// Writes frames `[start, end)` as `variant` to `dest.part` and then renames it over
/// `dest`. Only the order of whole frames and the rate in the header change; the
/// bytes of every frame are copied as they are. On failure no `.part` is left.
pub fn write_variant(src: &mut WavReader, start: u64, end: u64, variant: Variant, dest: &Path) -> Result<(), WavError> {
    let part = part_path(dest);
    if let Err(e) = copy_variant(src, start, end, variant, &part) {
        let _ = fs::remove_file(&part);
        return Err(e);
    }
    if let Err(e) = fs::rename(&part, dest) {
        let _ = fs::remove_file(&part);
        return Err(e.into());
    }
    Ok(())
}
