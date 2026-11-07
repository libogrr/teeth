//! Applications that play audio: which ones have a session on the output device, their
//! names and icons, and the process tree that records only one of them.
//!
//! Process loopback records one process tree. A browser plays from a child process, so Teeth
//! records the tree of the app's root process: the one whose parent is not the same app.

use crate::win;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::Graphics::Gdi::{
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, ReleaseDC,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::System::Com::CLSCTX_ALL;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};
use windows::core::{Interface, PCWSTR, PWSTR, w};

/// One running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent: u32,
    /// Executable file name, as Windows lists it (`chrome.exe`).
    pub exe: String,
    /// Creation time (100-ns units since 1601); `u64::MAX` when unknown or not read.
    pub created: u64,
}

/// One audio session on the default output device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    /// Zero when Windows does not say which process it belongs to.
    pub pid: u32,
    /// It has a stream running, even one that plays silence.
    pub active: bool,
    /// Its meter shows sound right now.
    pub audible: bool,
    /// Windows' own system sounds.
    pub system: bool,
}

/// An application that has audio, as the source selector lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppEntry {
    pub exe: String,
    pub name: String,
    pub path: Option<PathBuf>,
    pub sounding: bool,
}

fn same_exe(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The processes of `exe` whose parent is not another listed process of `exe`.
pub fn roots<'a>(procs: &'a [ProcInfo], exe: &str) -> Vec<&'a ProcInfo> {
    let is_exe = |pid: u32| procs.iter().any(|p| p.pid == pid && same_exe(&p.exe, exe));
    procs.iter().filter(|p| same_exe(&p.exe, exe) && !is_exe(p.parent)).collect()
}

/// Whether `pid` is `root` or descends from it.
fn in_tree(procs: &[ProcInfo], root: u32, pid: u32) -> bool {
    let mut current = pid;
    // A bound in case parent IDs, reused by Windows, ever form a loop.
    for _ in 0..64 {
        if current == root {
            return true;
        }
        match procs.iter().find(|p| p.pid == current) {
            Some(p) if p.parent != current => current = p.parent,
            _ => return false,
        }
    }
    false
}

/// The root of `exe` to record: the first whose tree holds a process in `sounding`, else
/// the one created first.
pub fn pick_root(procs: &[ProcInfo], exe: &str, sounding: &[u32]) -> Option<u32> {
    let roots = roots(procs, exe);
    roots
        .iter()
        .find(|r| sounding.iter().any(|&pid| in_tree(procs, r.pid, pid)))
        .or_else(|| roots.iter().min_by_key(|r| r.created))
        .map(|r| r.pid)
}

/// Applications with a session, one per executable: `(exe, sounding)`, sounding ones first,
/// then by name. Leaves out Windows' system sounds and Teeth's own process tree.
pub fn group(sessions: &[SessionInfo], procs: &[ProcInfo], own_pid: u32) -> Vec<(String, bool)> {
    let mut apps: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for s in sessions.iter().filter(|s| s.pid != 0 && !s.system && !in_tree(procs, own_pid, s.pid)) {
        let Some(p) = procs.iter().find(|p| p.pid == s.pid) else { continue };
        let entry = apps.entry(p.exe.to_ascii_lowercase()).or_insert_with(|| (p.exe.clone(), false));
        entry.1 |= s.audible;
    }
    let mut list: Vec<(String, bool)> = apps.into_values().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase())));
    list
}

/// Every running process, without creation times: reading them opens each process.
pub fn snapshot() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    // SAFETY: a process snapshot read with a correctly sized entry and closed at the end.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(snap, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(ProcInfo {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                created: u64::MAX,
            });
            more = Process32NextW(snap, &mut entry).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Opens `pid` for limited queries; closed when dropped.
struct Process(HANDLE);

impl Process {
    fn open(pid: u32) -> Option<Process> {
        // SAFETY: a plain open with the least access; the handle is owned by the result.
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok().map(Process)
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        // SAFETY: a handle from OpenProcess, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn created(pid: u32) -> u64 {
    let Some(p) = Process::open(pid) else { return u64::MAX };
    let (mut c, mut e, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: valid process handle and out pointers.
    match unsafe { GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u) } {
        Ok(()) => (u64::from(c.dwHighDateTime) << 32) | u64::from(c.dwLowDateTime),
        Err(_) => u64::MAX,
    }
}

fn image_path(pid: u32) -> Option<PathBuf> {
    let p = Process::open(pid)?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` UTF-16 units; Windows writes the length back.
    unsafe { QueryFullProcessImageNameW(p.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
}

/// Audio sessions on the default output device; empty if Windows does not list them.
pub fn sessions() -> Vec<SessionInfo> {
    let mut out = Vec::new();
    let Ok(device) = win::default_render_device() else { return out };
    // SAFETY: COM calls on live interfaces; the UI thread has COM initialized.
    unsafe {
        let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(list) = manager.GetSessionEnumerator() else { return out };
        let count = list.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(control) = list.GetSession(i) else { continue };
            let Ok(control) = control.cast::<IAudioSessionControl2>() else { continue };
            let pid = control.GetProcessId().unwrap_or(0);
            let active = control.GetState().is_ok_and(|s| s == AudioSessionStateActive);
            let audible = active
                && control.cast::<IAudioMeterInformation>().and_then(|m| m.GetPeakValue()).is_ok_and(|peak| peak > 0.0);
            let system = control.IsSystemSoundsSession() == windows::Win32::Foundation::S_OK;
            out.push(SessionInfo { pid, active, audible, system });
        }
    }
    out
}

/// The name Windows shows for an executable (its file description), else its file name
/// without `.exe`.
fn display_name(path: Option<&Path>, exe: &str) -> String {
    path.and_then(file_description)
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| exe.strip_suffix(".exe").or(exe.strip_suffix(".EXE")).unwrap_or(exe).to_string())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn file_description(path: &Path) -> Option<String> {
    let name = wide(path);
    // SAFETY: the version block is read into a buffer of the size Windows asked for, and
    // every pointer VerQueryValueW returns points inside that buffer, which outlives them.
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(name.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut block = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(name.as_ptr()), None, size, block.as_mut_ptr().cast()).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(block.as_ptr().cast(), w!("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let pair = std::slice::from_raw_parts(ptr.cast::<u16>(), 2);
        let key: Vec<u16> = format!("\\StringFileInfo\\{:04x}{:04x}\\FileDescription", pair[0], pair[1])
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        if !VerQueryValueW(block.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let text = std::slice::from_raw_parts(ptr.cast::<u16>(), len as usize);
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        Some(String::from_utf16_lossy(&text[..end]))
    }
}

/// The applications the source selector lists.
pub fn list_apps() -> Vec<AppEntry> {
    let procs = snapshot();
    group(&sessions(), &procs, std::process::id())
        .into_iter()
        .map(|(exe, sounding)| {
            let path = procs.iter().filter(|p| same_exe(&p.exe, &exe)).find_map(|p| image_path(p.pid));
            AppEntry { name: display_name(path.as_deref(), &exe), exe, path, sounding }
        })
        .collect()
}

/// The name Windows shows for a running `exe`, or its file name when it is not running.
pub fn app_name(exe: &str) -> String {
    let path = snapshot().into_iter().filter(|p| same_exe(&p.exe, exe)).find_map(|p| image_path(p.pid));
    display_name(path.as_deref(), exe)
}

/// The executable path of a running `exe`.
pub fn app_path(exe: &str) -> Option<PathBuf> {
    snapshot().into_iter().filter(|p| same_exe(&p.exe, exe)).find_map(|p| image_path(p.pid))
}

/// The PID whose process tree records `exe`, or `None` when it is not running.
pub fn resolve(exe: &str) -> Option<u32> {
    let sounding: Vec<u32> = sessions().into_iter().filter(|s| s.active).map(|s| s.pid).collect();
    let mut procs = snapshot();
    for p in procs.iter_mut().filter(|p| same_exe(&p.exe, exe)) {
        p.created = created(p.pid);
    }
    pick_root(&procs, exe, &sounding)
}

/// The executable's large icon as RGBA pixels, `[width, height]`.
pub fn icon_rgba(path: &Path) -> Option<([usize; 2], Vec<u8>)> {
    let name = wide(path);
    let mut info = SHFILEINFOW::default();
    // SAFETY: `info` is a correctly sized out struct; the icon it returns is destroyed below,
    // and the bitmaps GetIconInfo creates are deleted.
    unsafe {
        let ok = SHGetFileInfoW(
            PCWSTR(name.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let mut icon = ICONINFO::default();
        let read = GetIconInfo(info.hIcon, &mut icon).ok().and_then(|()| bitmap_rgba(icon.hbmColor));
        let _ = DeleteObject(icon.hbmColor.into());
        let _ = DeleteObject(icon.hbmMask.into());
        let _ = DestroyIcon(info.hIcon);
        read
    }
}

/// Reads a colour bitmap as top-down RGBA. Icons without an alpha channel are made opaque.
///
/// # Safety
/// `bitmap` must be a valid bitmap handle.
unsafe fn bitmap_rgba(bitmap: HBITMAP) -> Option<([usize; 2], Vec<u8>)> {
    // SAFETY: per the contract; the screen DC is released before returning.
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(bitmap.into(), size_of::<BITMAP>() as i32, Some((&raw mut bm).cast())) == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth.max(0) as usize, bm.bmHeight.max(0) as usize);
        if w == 0 || h == 0 {
            return None;
        }
        let mut header = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u8; w * h * 4];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut header, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        if lines == 0 {
            return None;
        }
        let has_alpha = pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
        for p in pixels.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
            if !has_alpha {
                p[3] = 255;
            }
        }
        Some(([w, h], pixels))
    }
}
