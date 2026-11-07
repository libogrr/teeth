//! Persisted user settings: `%APPDATA%\Teeth\settings.json`.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub last_folder: Option<PathBuf>,
    pub window_size: Option<[f32; 2]>,
    /// Top-left corner of the window, in points.
    pub window_pos: Option<[f32; 2]>,
    /// Executable of the app to record (`chrome.exe`); `None` records the whole system.
    pub source: Option<String>,
}

impl Settings {
    pub fn path() -> PathBuf {
        app_data().join("Teeth").join("settings.json")
    }

    /// The saved settings, or the defaults when there are none or they cannot be read.
    pub fn load() -> Settings {
        Settings::load_from(&Settings::path())
    }

    /// The saved window position, unless it would leave the title bar out of reach on the
    /// virtual desktop `screen` (`[left, top, width, height]`): a monitor may be gone.
    pub fn restorable_position(&self, screen: [f32; 4]) -> Option<[f32; 2]> {
        /// How much of the title bar must stay on screen to grab it.
        const REACH: f32 = 100.0;
        /// Windows may report the frame a little above the top of the screen.
        const FRAME: f32 = 8.0;
        let [x, y] = self.window_pos?;
        let width = self.window_size.map_or(0.0, |s| s[0]);
        let [left, top, w, h] = screen;
        let reachable = x + width >= left + REACH
            && x <= left + w - REACH
            && y >= top - FRAME
            && y <= top + h - REACH;
        reachable.then_some([x, y])
    }

    /// Defaults on any error (missing file, bad JSON).
    pub fn load_from(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(path, json)
    }
}

fn app_data() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}
