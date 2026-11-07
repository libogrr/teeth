//! A single Teeth at a time. A second window would recover the take the first one is writing
//! or showing, and could move or delete it, so a second launch only brings the open Teeth to
//! the front and quits.
//!
//! The open Teeth marks its window with a window property, and the second launch looks for
//! that mark. A title would not do: an Explorer window showing `Documentos\Teeth` is also
//! titled «Teeth». Nor would the window class, which winit names generically.

use std::ffi::c_void;
use std::time::Duration;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetLastActivePopup, GetPropW, IsIconic, SW_RESTORE, SetForegroundWindow, SetPropW,
    ShowWindowAsync,
};
use windows::core::{BOOL, PCWSTR, w};

/// The window property that marks Teeth's window.
const MARK: PCWSTR = w!("Teeth.ventana");
/// How long a second launch waits for the open Teeth to create its window.
const WAIT: Duration = Duration::from_secs(3);
/// How often it looks again meanwhile.
const STEP: Duration = Duration::from_millis(100);

/// Claims the single-instance mutex; `true` when another process already holds it. The
/// handle is never closed, so the mutex lives as long as the process.
pub fn already_running() -> bool {
    // SAFETY: static, NUL-terminated name made with `w!`; no security attributes.
    let handle = unsafe { CreateMutexW(None, false, w!("Local\\Teeth-single-instance")) };
    // SAFETY: read right after the call, before anything else can change the thread's error.
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // If the mutex cannot be created at all, running unguarded beats not running.
    handle.is_ok() && existed
}

/// Marks `hwnd` as Teeth's window, so a second launch can find it.
pub fn mark_window(hwnd: isize) {
    // Any non-null value marks the window; it is never read as a pointer.
    let flag = HANDLE(std::ptr::dangling_mut::<c_void>());
    // SAFETY: static property name and a non-null flag. An invalid window only makes the call
    // fail, which is ignored.
    let _ = unsafe { SetPropW(HWND(hwnd as *mut c_void), MARK, Some(flag)) };
}

/// The top-level windows that carry Teeth's mark, in `EnumWindows` order.
pub fn marked_windows() -> Vec<isize> {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        // SAFETY: `found` is the `Vec` that `marked_windows` lends for the duration of
        // `EnumWindows`, which calls this on the same thread.
        let found = unsafe { &mut *(found.0 as *mut Vec<isize>) };
        // SAFETY: `hwnd` comes from `EnumWindows`; a window that is gone returns null.
        if !unsafe { GetPropW(hwnd, MARK) }.is_invalid() {
            found.push(hwnd.0 as isize);
        }
        true.into()
    }
    let mut found: Vec<isize> = Vec::new();
    // SAFETY: `found` outlives the call, and `visit` only uses it while `EnumWindows` runs.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut found as isize)) };
    found
}

/// Brings `hwnd` to the front, restoring it if it is minimized; if it owns an open dialog,
/// such as «Guardar como», the dialog comes to the front instead. None of these calls waits
/// for the window's thread, which may be busy saving a long take.
pub fn bring_to_front(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut c_void);
    // SAFETY: plain calls on a window handle; one that is gone makes them fail, which is
    // ignored.
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(GetLastActivePopup(hwnd));
    }
}

/// Calls `find` until it gives something, sleeping `step` between tries, and gives up when
/// another `step` would pass `limit`. There is no sleep after the last try.
pub fn wait_for<T>(
    mut find: impl FnMut() -> Option<T>,
    mut sleep: impl FnMut(Duration),
    limit: Duration,
    step: Duration,
) -> Option<T> {
    let mut waited = Duration::ZERO;
    loop {
        if let Some(found) = find() {
            return Some(found);
        }
        if waited + step > limit {
            return None;
        }
        sleep(step);
        waited += step;
    }
}

/// What a second launch does: waits up to 3 s for the open Teeth's window and brings it to
/// the front. If it never appears, nothing happens; that Teeth shows up on its own.
pub fn bring_running_to_front() {
    if let Some(hwnd) = wait_for(|| marked_windows().first().copied(), std::thread::sleep, WAIT, STEP) {
        bring_to_front(hwnd);
    }
}
