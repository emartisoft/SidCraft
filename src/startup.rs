// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Files handed to SidCraft by the operating system.
//!
//! When a SID file is opened with SidCraft (Explorer, a Linux file manager,
//! Finder or a shell), the path arrives either on the command line or, on
//! macOS, as a later AppleEvent. This module collects both forms so the app can
//! start playing the requested tune instead of the saved playlist.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// File extensions the app can open directly.
const OPEN_EXTENSIONS: [&str; 3] = ["sid", "mus", "m3u"];

/// Extensions of playlist files, which become the active playlist.
const PLAYLIST_EXTENSIONS: [&str; 1] = ["m3u"];

/// Returns true when `path` is a SID tune or an M3U playlist.
pub fn is_supported(path: &Path) -> bool {
    has_extension(path, &OPEN_EXTENSIONS)
}

/// Returns true when `path` is an M3U playlist.
pub fn is_playlist(path: &Path) -> bool {
    has_extension(path, &PLAYLIST_EXTENSIONS)
}

/// Returns true when `path` is a SID tune (PSID/RSID), not a playlist.
pub fn is_sid(path: &Path) -> bool {
    is_supported(path) && !is_playlist(path)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Returns the SID or playlist file named on the command line, if any.
///
/// Options (arguments starting with `-`) are ignored, so future flags cannot
/// be mistaken for a file. The first supported argument wins when the OS
/// passes several files, for example when they were selected together.
pub fn from_args() -> Option<PathBuf> {
    std::env::args_os().skip(1).find_map(|arg| {
        let argument = arg.to_string_lossy();
        if argument.starts_with('-') {
            return None;
        }
        let path = PathBuf::from(arg);
        is_supported(&path).then_some(path)
    })
}

/// Files opened while the app is already running.
fn pending() -> &'static Mutex<Vec<PathBuf>> {
    static PENDING: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

/// Queues a path for the UI thread to open on the next frame.
#[cfg(target_os = "macos")]
fn queue(path: PathBuf) {
    if is_supported(&path)
        && let Ok(mut pending) = pending().lock()
    {
        pending.push(path);
    }
}

/// Takes the paths queued since the previous call.
pub fn take_pending() -> Vec<PathBuf> {
    pending()
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// Installs the macOS "open documents" handler.
///
/// Finder sends opened files as an AppleEvent rather than as command line
/// arguments, so an app bundle must listen for `odoc` itself. This is a no-op
/// on the other platforms, where the OS already passes the file in `argv`.
pub fn install_open_handler() {
    #[cfg(target_os = "macos")]
    macos::install();
}

#[cfg(target_os = "macos")]
mod macos {
    //! Receives the `odoc` (open documents) AppleEvent from Finder.
    //!
    //! winit/eframe do not forward this event, so SidCraft registers its own
    //! handler with `NSAppleEventManager`. The handler merely copies the paths
    //! into the global queue; `SidcraftApp::update` loads them on the UI thread.

    use super::queue;
    use objc2::rc::Retained;
    use objc2::{AllocAnyThread, define_class, msg_send, sel};
    use objc2_foundation::{
        NSAppleEventDescriptor, NSAppleEventManager, NSObject, NSObjectProtocol,
    };
    use std::path::PathBuf;

    // FourCC codes from AppleEvents.h and AEDataModel.h.
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS_EVENT_ID: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT_KEYWORD: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and the handler
        // does not implement Drop.
        #[unsafe(super(NSObject))]
        #[name = "SidCraftOpenFileHandler"]
        struct OpenFileHandler;

        impl OpenFileHandler {
            /// `application:openFiles:` equivalent for AppleEvents.
            #[unsafe(method(handleOpenEvent:withReplyEvent:))]
            fn handle_open_event(
                &self,
                event: &NSAppleEventDescriptor,
                _reply: &NSAppleEventDescriptor,
            ) {
                // The direct object is a list of file descriptors, not a
                // single path.
                let list: Option<Retained<NSAppleEventDescriptor>> = unsafe {
                    msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT_KEYWORD]
                };
                let Some(list) = list else { return };

                for index in 1..=list.numberOfItems() {
                    let Some(item) = list.descriptorAtIndex(index) else {
                        continue;
                    };
                    if let Some(url) = item.fileURLValue() {
                        if let Some(path) = url.path() {
                            queue(PathBuf::from(path.to_string()));
                        }
                    } else if let Some(path) = item.stringValue() {
                        queue(PathBuf::from(path.to_string()));
                    }
                }
            }
        }

        unsafe impl NSObjectProtocol for OpenFileHandler {}
    );

    impl OpenFileHandler {
        fn new() -> Retained<Self> {
            let this = Self::alloc().set_ivars(());
            // SAFETY: `init` is the designated initializer of NSObject.
            unsafe { msg_send![super(this), init] }
        }
    }

    pub fn install() {
        let handler = OpenFileHandler::new();
        let manager = NSAppleEventManager::sharedAppleEventManager();
        // SAFETY: the method signature matches NSAppleEventManager's
        // `setEventHandler:andSelector:forEventClass:andEventID:`; the
        // selector names the method implemented above and the handler stays
        // alive for the lifetime of the process.
        let _: () = unsafe {
            msg_send![
                &manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleOpenEvent:withReplyEvent:),
                forEventClass: CORE_EVENT_CLASS,
                andEventID: OPEN_DOCUMENTS_EVENT_ID
            ]
        };
        // NSAppleEventManager does not retain its handler; leak one reference
        // so it remains valid while the app runs.
        let _ = Retained::into_raw(handler);
    }
}
