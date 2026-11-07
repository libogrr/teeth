#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use teeth::settings::Settings;
use teeth::ui::{MIN_WINDOW, TeethApp, theme};
use teeth::{instance, win};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};

fn main() -> eframe::Result {
    // Opening Teeth again, like Spotify or Slack, brings the open one to the front.
    if instance::already_running() {
        instance::bring_running_to_front();
        return Ok(());
    }
    // The output device lookups in `win` need COM on the UI thread. The window library
    // later calls OleInitialize, which accepts this apartment as it is.
    // SAFETY: no reserved pointer; called once, before any COM use on this thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let settings = Settings::load();
    // A hand-edited or damaged settings file must not produce an unusable window.
    let size = settings
        .window_size
        .filter(|s| s.iter().all(|v| v.is_finite() && *v > 0.0))
        .unwrap_or([860.0, 380.0]);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Teeth")
        .with_inner_size(size)
        .with_min_inner_size(MIN_WINDOW);
    // The clean silhouette: Windows shows the window and taskbar icons under 48 pixels.
    if let Some(icon) = theme::window_icon(include_bytes!("../assets/icon/teeth-window.png")) {
        viewport = viewport.with_icon(icon);
    }
    if let Some(position) = settings.restorable_position(win::virtual_screen()) {
        viewport = viewport.with_position(position);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Teeth",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            theme::install(&cc.egui_ctx);
            if let Ok(handle) = cc.window_handle()
                && let RawWindowHandle::Win32(h) = handle.as_raw()
            {
                win::dark_title_bar(h.hwnd.get());
                instance::mark_window(h.hwnd.get());
            }
            Ok(Box::new(TeethApp::new(cc, settings)))
        }),
    )
}
