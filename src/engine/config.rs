// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

//! User configuration persistence.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_viz_mode() -> String {
    "scope".to_string()
}

const fn default_mini_opacity() -> f32 {
    0.64
}

/// User configuration stored in config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Theme preference: `system`, `light` or `dark`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Whether the left tool panel is open.
    #[serde(default = "default_true")]
    pub left_panel: bool,
    /// Whether the right inspector is open.
    #[serde(default = "default_true")]
    pub right_panel: bool,
    /// Visualiser: `scope` or `vu`.
    #[serde(default = "default_viz_mode")]
    pub viz_mode: String,
    /// Whether the EKV transistor-model filter is enabled (6581 chips only).
    #[serde(default)]
    pub ekv_filter: bool,
    /// Whether SidCraft asks GitHub for a newer release shortly after startup.
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// Whether the "Favourite Top 100 SIDs" M3U is kept in the playlist library.
    #[serde(default = "default_true")]
    pub show_favourite_top_100: bool,
    /// Path of the playlist that was active when the app last closed.
    #[serde(default)]
    pub active_playlist: Option<String>,
    /// Last window width in points.
    #[serde(default)]
    pub window_width: Option<f32>,
    /// Last window height in points.
    #[serde(default)]
    pub window_height: Option<f32>,
    /// Last window x position in points.
    #[serde(default)]
    pub window_x: Option<f32>,
    /// Last window y position in points.
    #[serde(default)]
    pub window_y: Option<f32>,
    /// Whether the window was maximized when the app last closed.
    #[serde(default)]
    pub window_maximized: bool,
    /// Last mini-player x position in points.
    #[serde(default)]
    pub mini_x: Option<f32>,
    /// Last mini-player y position in points.
    #[serde(default)]
    pub mini_y: Option<f32>,
    /// Last mini-player width in points.
    #[serde(default)]
    pub mini_width: Option<f32>,
    /// Last mini-player height in points.
    #[serde(default)]
    pub mini_height: Option<f32>,
    /// Mini-player window opacity, from 0.25 (translucent) to 1.0 (opaque).
    /// Fresh installs start at 0.64 (64%); existing saved values are kept.
    #[serde(default = "default_mini_opacity")]
    pub mini_opacity: f32,
    /// Open directly in the frameless mini player on launch.
    #[serde(default)]
    pub start_in_mini: bool,
    /// Use the native Wayland backend on Linux instead of X11/XWayland.
    /// Wayland does not expose window positions to applications, so this
    /// disables remembering where the full window and mini player were left.
    #[serde(default)]
    pub prefer_wayland: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            left_panel: true,
            right_panel: true,
            viz_mode: default_viz_mode(),
            ekv_filter: false,
            check_updates: true,
            show_favourite_top_100: true,
            active_playlist: None,
            window_width: None,
            window_height: None,
            window_x: None,
            window_y: None,
            window_maximized: false,
            mini_x: None,
            mini_y: None,
            mini_width: None,
            mini_height: None,
            mini_opacity: default_mini_opacity(),
            start_in_mini: false,
            prefer_wayland: false,
        }
    }
}

impl Config {
    /// Loads config from file, returning defaults if not found or invalid.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Saves config to file (best-effort, errors ignored).
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        let Some(parent) = path.parent() else { return };
        let _ = fs::create_dir_all(parent);
        if let Ok(content) = toml::to_string_pretty(self) {
            let _ = fs::write(&path, content);
        }
    }
}

/// Returns the config file path (~/.config/sidcraft/config.toml).
fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sidcraft").join("config.toml"))
}
