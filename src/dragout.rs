//! Samples dragged out of Teeth: where they go, how they are named and when one already
//! written can be handed out again.
//!
//! DAWs link to a dropped file instead of copying it, so samples live in a permanent folder,
//! `Documentos\Teeth\Samples`, that Teeth never cleans. A sample is the selection's bytes
//! copied as they are; with a variant, whole frames are reordered or the header rate changes.

use crate::files::LocalTime;
use crate::session::Session;
use crate::variant::Variant;
use crate::wav::{header_len, write_variant};
use std::collections::HashMap;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK,
};
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_NONE, DoDragDrop, IDropSource,
    IDropSource_Impl,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowThreadProcessId, WindowFromPoint};
use windows::Win32::UI::Shell::{
    BHID_DataObject, FOLDERID_Documents, IShellItem, KNOWN_FOLDER_FLAG, SHCreateItemFromParsingName,
    SHGetKnownFolderPath,
};
use windows::core::{BOOL, HRESULT, PCWSTR};

/// `Documentos\Teeth\Samples`, or `%USERPROFILE%\Documents\Teeth\Samples` if Windows does not
/// say where the documents folder is.
pub fn samples_dir() -> PathBuf {
    documents_dir()
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Documents")))
        .unwrap_or_else(std::env::temp_dir)
        .join("Teeth")
        .join("Samples")
}

fn documents_dir() -> Option<PathBuf> {
    // SAFETY: the returned string is allocated by the shell, read once and freed here.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Documents, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let path = path_from_wide(p.as_wide());
        CoTaskMemFree(Some(p.0 as *const c_void));
        Some(path)
    }
}

/// A path from UTF-16 as Windows gives it, valid Unicode or not.
fn path_from_wide(units: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(units))
}

/// `name` without the characters Windows refuses in file names, nor control characters
/// or trailing dots and spaces; «Teeth» when nothing is left.
pub fn sanitize(name: &str) -> String {
    let kept: String = name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()).collect();
    let kept = kept.trim().trim_end_matches(['.', ' ']).to_string();
    if kept.is_empty() { "Teeth".to_string() } else { kept }
}

/// `{source} AAAA-MM-DD HH-MM-SS{variant}.wav`, with the variant's suffix (` -3st rev`) after
/// the time, nothing for the audio as it was recorded.
pub fn sample_name(source: &str, variant: Variant, t: &LocalTime) -> String {
    format!(
        "{} {:04}-{:02}-{:02} {:02}-{:02}-{:02}{}.wav",
        sanitize(source),
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        variant.name_suffix()
    )
}

/// `dir\name` if free, else `dir\{stem}-2.wav`, `-3`…
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".wav").unwrap_or(name);
    (2u32..)
        .map(|n| dir.join(format!("{stem}-{n}.wav")))
        .find(|p| !p.exists())
        .expect("unbounded counter")
}

/// Which take, as it is now (its file and audio size), which range of it and which variant.
type ExportKey = (PathBuf, u64, u64, u64, Variant);

/// Samples already written in this session, so dragging the same range twice hands out the
/// same file.
#[derive(Default)]
pub struct Exports {
    written: HashMap<ExportKey, PathBuf>,
}

/// Writes the session's selection into `dir` as a new sample named after `source`, `variant`
/// and `now`, or returns the one already written for the same take, range and variant if it
/// is still there and whole.
pub fn export(
    session: &mut Session,
    exports: &mut Exports,
    dir: &Path,
    source: &str,
    variant: Variant,
    now: &LocalTime,
) -> Result<PathBuf, String> {
    let fail = |e: &dyn std::fmt::Display| format!("No se pudo crear el sample: {e}");
    let (start, end) = (session.selection.start, session.selection.end);
    let key = (session.take.path.clone(), session.reader.data_len(), start, end, variant);
    let format = variant.format(session.reader.format());
    let data = (end - start) * u64::from(format.block_align());
    let expected = header_len(&format) + data + data % 2;
    if let Some(path) = exports.written.get(&key)
        && std::fs::metadata(path).is_ok_and(|m| m.len() == expected)
    {
        return Ok(path.clone());
    }
    std::fs::create_dir_all(dir).map_err(|e| fail(&e))?;
    let dest = unique_path(dir, &sample_name(source, variant, now));
    write_variant(&mut session.reader, start, end, variant, &dest).map_err(|e| fail(&e))?;
    exports.written.insert(key, dest.clone());
    Ok(dest)
}

/// What the drag does next: Escape cancels, releasing the left button drops, holding it
/// continues.
fn continue_drag(escape: bool, left_down: bool) -> HRESULT {
    if escape {
        DRAGDROP_S_CANCEL
    } else if !left_down {
        DRAGDROP_S_DROP
    } else {
        S_OK
    }
}

/// The source side of a Windows drag: when it ends, and the system's own cursors.
#[windows_core::implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        continue_drag(escape_pressed.as_bool(), key_state.contains(MK_LBUTTON))
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Whether a finished drop landed somewhere: with an effect, on a window of another process.
/// Windows reports a copy on Teeth's own window too, because the window library always marks
/// it as accepting files, though nothing there takes the file.
fn drop_accepted(effect: DROPEFFECT, window_process: u32, own_process: u32) -> bool {
    effect != DROPEFFECT_NONE && window_process != own_process
}

/// The process of the window under the mouse; 0 if it cannot be told.
fn process_under_cursor() -> u32 {
    let mut at = POINT::default();
    let mut process = 0;
    // SAFETY: both calls only read into the given out values, and a window that is already
    // gone only leaves `process` at 0.
    unsafe {
        if GetCursorPos(&mut at).is_ok() {
            GetWindowThreadProcessId(WindowFromPoint(at), Some(&mut process));
        }
    }
    process
}

/// Drags the file at `path` with Windows drag and drop, the way Explorer does, so a DAW,
/// a folder or the desktop takes it as a file. Blocks until the drop or the cancel; `true`
/// when something accepted it. Must run on the window's thread, where OLE is initialized.
pub fn drag_file(path: &Path) -> Result<bool, String> {
    let fail = |e: windows::core::Error| format!("No se pudo arrastrar el sample: {e}");
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call.
    let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.map_err(fail)?;
    // SAFETY: a live shell item; the data object is the shell's own for that file.
    let data: IDataObject = unsafe { item.BindToHandler(None, &BHID_DataObject) }.map_err(fail)?;
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT_NONE;
    // SAFETY: both objects are live COM objects and `effect` is a valid out pointer; this
    // runs on the UI thread, which the window library initialized for OLE.
    let result = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect) };
    match result {
        r if r == DRAGDROP_S_DROP => Ok(drop_accepted(effect, process_under_cursor(), std::process::id())),
        r if r == DRAGDROP_S_CANCEL => Ok(false),
        r => r.ok().map(|()| false).map_err(fail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_counts_only_when_another_process_takes_it() {
        assert!(drop_accepted(DROPEFFECT_COPY, 10, 20));
        assert!(drop_accepted(DROPEFFECT_LINK, 10, 20));
        assert!(!drop_accepted(DROPEFFECT_NONE, 10, 20));
        // Windows reports a copy on Teeth's own window, which the window library marks as
        // accepting files, though nothing there takes the file.
        assert!(!drop_accepted(DROPEFFECT_COPY, 20, 20));
    }

    #[test]
    fn a_wide_path_keeps_units_that_are_not_valid_unicode() {
        let units = [0x43, 0x3A, 0x5C, 0xD800, 0x78];
        let path = path_from_wide(&units);
        assert_eq!(path.as_os_str().encode_wide().collect::<Vec<u16>>(), units);
    }

    #[test]
    fn escape_cancels_release_drops_and_holding_continues() {
        assert_eq!(continue_drag(true, true), DRAGDROP_S_CANCEL);
        assert_eq!(continue_drag(false, false), DRAGDROP_S_DROP);
        assert_eq!(continue_drag(false, true), S_OK);
    }
}
