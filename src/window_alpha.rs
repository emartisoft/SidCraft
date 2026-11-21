// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//! OS-level window alpha for the frameless mini player.
//!
//! The renderer-level alpha in `App::clear_color` is portable but some GL
//! stacks ignore it. Where the platform exposes a reliable window alpha we
//! also set it directly. On unsupported platforms this is a no-op and the
//! renderer-level alpha remains the fallback.

#[cfg(not(target_arch = "wasm32"))]
use raw_window_handle::RawWindowHandle;

/// Set the whole window's opacity. `255` restores a normal opaque window.
#[cfg(target_os = "windows")]
pub fn set_alpha(handle: &RawWindowHandle, alpha: u8, previous: Option<u8>) {
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
        WS_EX_LAYERED,
    };

    let RawWindowHandle::Win32(win) = handle else {
        return;
    };
    // windows-sys aliases the *Ptr variants to the 32-bit functions on
    // i686, so the window-long type follows the target pointer width.
    #[cfg(target_pointer_width = "64")]
    type WindowLong = isize;
    #[cfg(target_pointer_width = "32")]
    type WindowLong = i32;

    let hwnd = win.hwnd.get() as HWND;
    unsafe {
        let style: WindowLong = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let layered = WS_EX_LAYERED as WindowLong;
        if alpha >= 255 {
            if style & layered != 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style & !layered);
            }
        } else {
            let style_changed = style & layered == 0;
            if style_changed {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | layered);
            }
            // winit rewrites the extended style when it changes decorations,
            // which drops WS_EX_LAYERED. Re-add it and re-apply the alpha in
            // that case even when the requested value has not changed.
            if style_changed || previous != Some(alpha) {
                // `crkey = 0` with LWA_ALPHA keeps every pixel's own colour
                // and applies `alpha` uniformly to the whole window.
                SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
            }
        }
    }
}

/// Other native platforms rely on the renderer-level alpha in `clear_color`.
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
pub fn set_alpha(_handle: &RawWindowHandle, _alpha: u8, _previous: Option<u8>) {}
