use std::cell::{Cell, RefCell};
use std::time::Duration;
use teeth::instance::{mark_window, marked_windows, wait_for};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, RemovePropW, WINDOW_EX_STYLE, WINDOW_STYLE};
use windows::core::w;

const LIMIT: Duration = Duration::from_secs(3);
const STEP: Duration = Duration::from_millis(100);

#[test]
fn a_window_found_at_once_needs_no_wait() {
    let slept = Cell::new(Duration::ZERO);
    assert_eq!(wait_for(|| Some(7), |d| slept.set(slept.get() + d), LIMIT, STEP), Some(7));
    assert_eq!(slept.get(), Duration::ZERO);
}

#[test]
fn the_window_is_searched_again_every_step_until_it_appears() {
    let tries = Cell::new(0);
    let slept = RefCell::new(Vec::new());
    let find = || {
        tries.set(tries.get() + 1);
        (tries.get() == 4).then_some(7)
    };
    assert_eq!(wait_for(find, |d| slept.borrow_mut().push(d), LIMIT, STEP), Some(7));
    assert_eq!(tries.get(), 4);
    assert_eq!(*slept.borrow(), vec![STEP; 3]);
}

#[test]
fn the_wait_gives_up_at_the_limit_without_sleeping_after_the_last_try() {
    let tries = Cell::new(0);
    let slept = Cell::new(Duration::ZERO);
    let find = || -> Option<u8> {
        tries.set(tries.get() + 1);
        None
    };
    assert_eq!(wait_for(find, |d| slept.set(slept.get() + d), LIMIT, STEP), None);
    assert_eq!(tries.get(), 31);
    assert_eq!(slept.get(), LIMIT);
}

/// A top-level window of the predefined `STATIC` class, titled like Teeth's and never shown.
fn hidden_window() -> isize {
    // SAFETY: static, NUL-terminated class and title; no parent, menu, instance or creation
    // data. The window is destroyed by `unmark_and_destroy` on the same thread.
    let hwnd = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!("Teeth"), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None)
    }
    .unwrap();
    hwnd.0 as isize
}

fn unmark_and_destroy(hwnd: isize) {
    let hwnd = HWND(hwnd as *mut _);
    // SAFETY: `hwnd` was created by `hidden_window` on this thread and is still alive; removing
    // a property it does not have only returns null.
    unsafe {
        let _ = RemovePropW(hwnd, w!("Teeth.ventana"));
        DestroyWindow(hwnd).unwrap();
    }
}

#[test]
fn a_marked_window_is_found_and_an_unmarked_one_titled_teeth_is_not() {
    let marked = hidden_window();
    let unmarked = hidden_window();
    mark_window(marked);
    let found = marked_windows();
    unmark_and_destroy(marked);
    unmark_and_destroy(unmarked);
    assert!(found.contains(&marked));
    assert!(!found.contains(&unmarked));
}
