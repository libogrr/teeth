//! COM, device and thread helpers shared by capture, playback and the UI.
//!
//! The device helpers need COM on the calling thread, in any apartment. They never
//! initialize it themselves: `main` puts the UI thread in an STA before the window opens
//! (the window library needs that apartment for OLE), and joining the MTA there would
//! break it.

use std::cell::Cell;
use std::ffi::c_void;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Media::Audio::{
    IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{BOOL, PWSTR, w};

/// Size of a `WAVEFORMATEX` without its trailing extension.
const WAVEFORMATEX_LEN: usize = 18;

thread_local! {
    /// Whether `com_init` initialized COM on this thread and owes a `CoUninitialize`.
    static COM_OWNED: Cell<bool> = const { Cell::new(false) };
}

/// Joins the current thread to the multithreaded apartment, at most once per thread.
/// A thread already in another apartment keeps it, and COM stays usable there.
pub fn com_init() {
    COM_OWNED.with(|owned| {
        if !owned.get() {
            // SAFETY: no reserved pointer; failure only means COM is already set up differently.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            owned.set(hr.is_ok());
        }
    });
}

/// Balances a successful `com_init` on this thread. Release every COM object first.
pub fn com_uninit() {
    COM_OWNED.with(|owned| {
        if owned.replace(false) {
            // SAFETY: pairs with the successful CoInitializeEx recorded by `com_init`.
            unsafe { CoUninitialize() };
        }
    });
}

/// Default playback endpoint for the console role (`eRender`, `eConsole`).
pub fn default_render_device() -> windows::core::Result<IMMDevice> {
    // SAFETY: plain COM calls; the enumerator is released when it goes out of scope.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        devices.GetDefaultAudioEndpoint(eRender, eConsole)
    }
}

/// Endpoint ID string, stable while the device exists.
pub fn device_id(d: &IMMDevice) -> Option<String> {
    // SAFETY: GetId returns a CoTaskMemAlloc'd string, read once and freed here.
    unsafe { take_co_string(d.GetId().ok()?) }
}

/// Friendly name shown in Windows sound settings (`PKEY_Device_FriendlyName`).
pub fn device_name(d: &IMMDevice) -> Option<String> {
    // SAFETY: the PROPVARIANT is cleared and the allocated string freed on every path.
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let text = PropVariantToStringAlloc(&value);
        // `PROPVARIANT`'s Drop clears it again when `value` leaves scope. That second
        // clear is harmless: PropVariantClear leaves the value VT_EMPTY, and clearing
        // VT_EMPTY frees nothing. Unlike capture's blob, this value was allocated by COM.
        let _ = PropVariantClear(&mut value);
        take_co_string(text.ok()?).filter(|s| !s.is_empty())
    }
}

/// Friendly name of the current default output device.
pub fn default_output_name() -> Option<String> {
    default_render_device().ok().and_then(|d| device_name(&d))
}

/// Registers the current thread with MMCSS under the "Pro Audio" task.
/// The registration lasts until the thread ends; failure leaves normal priority.
pub fn raise_to_pro_audio() {
    let mut task_index = 0u32;
    // SAFETY: static task name and a valid out pointer.
    let _ = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) };
}

/// Copies a mix format verbatim: the 18-byte `WAVEFORMATEX` plus its `cbSize` extension bytes.
/// Returns an empty vector for a null pointer.
///
/// # Safety
/// `f` must be null or point to a readable `WAVEFORMATEX` followed by `cbSize` readable
/// bytes, as `IAudioClient::GetMixFormat` returns.
pub unsafe fn mix_format_bytes(f: *const WAVEFORMATEX) -> Vec<u8> {
    if f.is_null() {
        return Vec::new();
    }
    let base = f.cast::<u8>();
    // SAFETY: the caller guarantees the 18-byte header is readable.
    let head = unsafe { std::slice::from_raw_parts(base, WAVEFORMATEX_LEN) };
    let extra = usize::from(u16::from_le_bytes([head[16], head[17]]));
    // SAFETY: the caller guarantees `cbSize` bytes follow the header.
    unsafe { std::slice::from_raw_parts(base, WAVEFORMATEX_LEN + extra) }.to_vec()
}

/// Converts a COM-allocated wide string and frees it.
///
/// # Safety
/// `p` must be null or a NUL-terminated string allocated with `CoTaskMemAlloc`.
unsafe fn take_co_string(p: PWSTR) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: valid NUL-terminated string per the contract above; freed exactly once.
    unsafe {
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

/// Asks Windows to draw the title bar of `hwnd` in dark mode. Older builds ignore it.
pub fn dark_title_bar(hwnd: isize) {
    let on = BOOL::from(true);
    // SAFETY: `on` outlives the call and its size is passed; an invalid or foreign window
    // only makes the call fail, which is ignored.
    let _ = unsafe {
        DwmSetWindowAttribute(
            HWND(hwnd as *mut c_void),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const on).cast(),
            size_of::<BOOL>() as u32,
        )
    };
}

/// The virtual desktop, all monitors together, as `[left, top, width, height]` in points.
pub fn virtual_screen() -> [f32; 4] {
    // SAFETY: both calls only read system values.
    let (metrics, dpi) = unsafe {
        let metric = |m| GetSystemMetrics(m) as f32;
        let area = [metric(SM_XVIRTUALSCREEN), metric(SM_YVIRTUALSCREEN), metric(SM_CXVIRTUALSCREEN), metric(SM_CYVIRTUALSCREEN)];
        (area, GetDpiForSystem())
    };
    let scale = dpi.max(1) as f32 / 96.0;
    metrics.map(|v| v / scale)
}

/// Whether Windows shows animations in windows (Accessibility -> Visual effects).
/// Defaults to true when the setting cannot be read.
pub fn client_area_animation() -> bool {
    let mut on = BOOL::from(true);
    // SAFETY: SPI_GETCLIENTAREAANIMATION writes one BOOL to the given pointer.
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut on).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_err() || on.as_bool()
}
