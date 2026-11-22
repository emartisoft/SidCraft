// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

//! User configuration persistence.

use crate::engine::hvsc::{DEFAULT_HVSC_QUERY_URL, DEFAULT_HVSC_URL, normalize_mirror_location};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

const fn default_true() -> bool {
    true
}

fn default_theme() -> String {
    "system".to_string()
}

fn default_accent() -> String {
    "blue".to_string()
}

fn default_viz_mode() -> String {
    "scope".to_string()
}

const fn default_mini_opacity() -> f32 {
    0.64
}

/// A user-managed HVSC mirror: an HTTP directory root, a `?dir=` lister or a
/// local `file://` folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HvscMirror {
    /// Stable id so editing a name or URL cannot break the saved selection.
    pub id: u64,
    /// Short label shown in Settings and the HVSC browser.
    pub name: String,
    /// Mirror location, always normalised by the UI before saving.
    pub location: String,
}

fn default_hvsc_mirrors() -> Vec<HvscMirror> {
    vec![
        HvscMirror {
            id: 1,
            name: "brona.dk".to_string(),
            location: DEFAULT_HVSC_URL.to_string(),
        },
        HvscMirror {
            id: 2,
            name: "SaNNiC".to_string(),
            location: DEFAULT_HVSC_QUERY_URL.to_string(),
        },
    ]
}

const fn default_hvsc_mirror_id() -> u64 {
    1
}

/// User configuration stored in config file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Theme preference: `system`, `light` or `dark`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Accent hue: `blue`, `red`, `green`, `yellow`, `orange` or `turquoise`.
    #[serde(default = "default_accent")]
    pub accent_color: String,
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
    /// HVSC mirrors offered in Settings; the first entry is the default.
    #[serde(default = "default_hvsc_mirrors")]
    pub hvsc_mirrors: Vec<HvscMirror>,
    /// Id of the mirror currently used by the browser and saved playlists.
    #[serde(default = "default_hvsc_mirror_id")]
    pub active_hvsc_mirror: u64,
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
            accent_color: default_accent(),
            left_panel: true,
            right_panel: true,
            viz_mode: default_viz_mode(),
            ekv_filter: false,
            check_updates: true,
            show_favourite_top_100: true,
            hvsc_mirrors: default_hvsc_mirrors(),
            active_hvsc_mirror: default_hvsc_mirror_id(),
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
        let mut config: Self = config_path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        for mirror in &mut config.hvsc_mirrors {
            mirror.name = mirror.name.trim().to_string();
            mirror.location = normalize_mirror_location(&mirror.location);
        }
        config
            .hvsc_mirrors
            .retain(|mirror| !mirror.name.is_empty() && !mirror.location.is_empty());
        let mut locations = HashSet::new();
        config
            .hvsc_mirrors
            .retain(|mirror| locations.insert(mirror.location.clone()));
        if config.hvsc_mirrors.is_empty() {
            config.hvsc_mirrors = default_hvsc_mirrors();
        }
        if !config
            .hvsc_mirrors
            .iter()
            .any(|mirror| mirror.id == config.active_hvsc_mirror)
        {
            config.active_hvsc_mirror = config.hvsc_mirrors[0].id;
        }
        config
    }

    /// Returns the currently selected HVSC mirror, falling back to the first.
    pub fn active_hvsc_mirror(&self) -> &HvscMirror {
        self.hvsc_mirrors
            .iter()
            .find(|mirror| mirror.id == self.active_hvsc_mirror)
            .or_else(|| self.hvsc_mirrors.first())
            .expect("config always has at least one HVSC mirror")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_two_mirrors_with_brona_selected() {
        let config = Config::default();
        assert_eq!(config.hvsc_mirrors.len(), 2);
        assert_eq!(config.hvsc_mirrors[0].id, 1);
        assert_eq!(config.hvsc_mirrors[1].location, DEFAULT_HVSC_QUERY_URL);
        assert_eq!(config.active_hvsc_mirror().id, 1);
        assert_eq!(config.active_hvsc_mirror().location, DEFAULT_HVSC_URL);
    }

    #[test]
    fn configs_without_mirror_fields_still_parse() {
        let config: Config = toml::from_str("theme = \"dark\"\n").unwrap();
        assert_eq!(config.active_hvsc_mirror().id, 1);
        assert_eq!(config.hvsc_mirrors.len(), 2);
    }

    #[test]
    fn mirror_settings_round_trip_through_toml() {
        let mut config = Config::default();
        config.hvsc_mirrors.push(HvscMirror {
            id: 3,
            name: "Local HVSC".to_string(),
            location: "file://C:/HVSC/C64Music".to_string(),
        });
        config.active_hvsc_mirror = 3;

        let saved = toml::to_string(&config).unwrap();
        let loaded: Config = toml::from_str(&saved).unwrap();
        assert_eq!(loaded.hvsc_mirrors, config.hvsc_mirrors);
        assert_eq!(loaded.active_hvsc_mirror, 3);
    }
}
