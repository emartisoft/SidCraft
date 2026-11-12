// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Application icon handling.
//!
//! Two artefacts carry the SidCraft artwork:
//!
//! * `icon/sidcraft.ico` — embedded into the Windows executable by
//!   `build.rs`, so Explorer, the task bar and Alt+Tab show it.
//! * `icon/sidcraft.png` — decoded at runtime and handed to egui, which sets
//!   the native window icon (title bar and task bar button).
//!
//! The PNG is the source of truth for the runtime icon because it is the only
//! format every platform understands; the ICO is Windows-only.

use eframe::egui::{ColorImage, IconData, TextureHandle, TextureOptions};

/// The raw PNG bytes of the application icon (256x256 RGBA).
const ICON_PNG: &[u8] = include_bytes!("../icon/sidcraft.png");

/// Decodes the embedded PNG into icon data for the native window.
///
/// Returns `None` when the PNG cannot be decoded, in which case the window
/// simply keeps the platform default icon.
pub fn icon_data() -> Option<IconData> {
    match eframe::icon_data::from_png_bytes(ICON_PNG) {
        Ok(icon) => Some(icon),
        Err(err) => {
            eprintln!("Could not decode the application icon: {err}");
            None
        }
    }
}

/// Decodes the embedded PNG into a colour image that can be drawn in the UI.
pub fn color_image() -> Option<ColorImage> {
    let icon = icon_data()?;
    // The PNG stores straight (unmultiplied) alpha, which is exactly what
    // `from_rgba_unmultiplied` expects.
    Some(ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    ))
}

/// Loads the application icon as a GPU texture for in-app drawing.
///
/// The texture is cached by egui under `name`, so repeated calls with the
/// same name reuse the same handle instead of uploading again.
pub fn texture(ctx: &eframe::egui::Context) -> Option<TextureHandle> {
    let image = color_image()?;
    Some(ctx.load_texture("sidcraft-icon", image, TextureOptions::LINEAR))
}

/// Gives the process an explicit identity on Windows.
///
/// Without an AppUserModelID, Windows groups the window under the generic
/// `sidcraft.exe` identity and the task bar button may show a placeholder
/// instead of the embedded icon.
#[cfg(windows)]
pub fn set_app_user_model_id() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    // "com.emarti.sidcraft", NUL terminated UTF-16.
    let app_id: Vec<u16> = "com.emarti.sidcraft\0".encode_utf16().collect();
    // SAFETY: `app_id` is a valid, NUL-terminated UTF-16 string that stays
    // alive for the duration of the call.
    let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr()) };
}

/// No-op on platforms that do not use AppUserModelIDs.
#[cfg(not(windows))]
pub fn set_app_user_model_id() {}
