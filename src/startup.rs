// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! Files handed to SidCraft by the operating system.
//!
//! When a SID file is opened with SidCraft (Explorer, a Linux file manager,
//! Finder or a shell), the path arrives either on the command line or, on
//! macOS, as a later AppleEvent. This module collects both forms so the app can
//! start playing the requested tune instead of the saved playlist.
//!
//! It also keeps SidCraft single-instance: a second launch forwards its file
//! to the instance that is already running (over a local socket) and exits, so
//! double-clicking another tune replaces the current one instead of starting a
//! second window and a second audio stream.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions};

/// File extensions the app can open directly.
const OPEN_EXTENSIONS: [&str; 3] = ["sid", "mus", "m3u"];

/// Extensions of playlist files, which become the active playlist.
const PLAYLIST_EXTENSIONS: [&str; 1] = ["m3u"];

/// Stable prefix of the single-instance socket name.
const INSTANCE_PREFIX: &str = "sidcraft-instance";

/// First line of every forwarded request, so a foreign process that happens
/// to own the socket name is ignored.
const PROTOCOL_MARKER: &str = "SIDCRAFT1";

/// Most a listener reads from one request; a path is never this long.
const MAX_REQUEST_BYTES: u64 = 64 * 1024;

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

/// A request addressed to the running SidCraft instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Load and play the tune or playlist at this path.
    Open(PathBuf),
    /// Bring the already running window to the front.
    Focus,
}

/// Files opened while the app is already running.
fn pending() -> &'static Mutex<Vec<Request>> {
    static PENDING: OnceLock<Mutex<Vec<Request>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

/// Queues a request for the UI thread to handle on the next frame.
fn queue(request: Request) {
    if let Ok(mut pending) = pending().lock() {
        pending.push(request);
    }
    // The listener runs off the UI thread and the window may be idle or
    // minimised, so ask for a frame instead of waiting for the next input.
    if let Some(wakeup) = wakeup().get() {
        wakeup();
    }
}

/// Callback that wakes the UI thread; set once the window exists.
fn wakeup() -> &'static OnceLock<Box<dyn Fn() + Send + Sync>> {
    static WAKEUP: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    &WAKEUP
}

/// Lets the listener wake the UI thread when a request arrives.
pub fn set_wakeup(wake: impl Fn() + Send + Sync + 'static) {
    let _ = wakeup().set(Box::new(wake));
}

/// Takes the requests received since the previous call.
pub fn take_requests() -> Vec<Request> {
    pending()
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default()
}

/// Hands a startup request to an already running SidCraft.
///
/// Returns true when the request was forwarded, in which case the caller
/// should exit without opening a window. A false result means this process
/// should carry on as the running instance.
pub fn hand_over_to_running_instance(path: Option<&Path>) -> bool {
    if start_listener() {
        return false;
    }
    // Finder never starts a second copy of a running app: it reuses the
    // existing one and delivers the file as an AppleEvent. A second process
    // without a file argument may be exactly the one waiting for that event,
    // so let it run rather than closing the window the user just opened.
    #[cfg(target_os = "macos")]
    if path.is_none() {
        return false;
    }
    if send_request(path).is_ok() {
        return true;
    }
    // The other instance may have exited between the two attempts; take over
    // its role if the name is free now instead of running without a listener.
    let _ = start_listener();
    false
}

/// Starts the listener that receives requests from later launches.
///
/// Returns false when the name is already owned by another instance (or when
/// the socket could not be created at all).
fn start_listener() -> bool {
    let name = instance_name();
    let Ok(name) = name.to_ns_name::<GenericNamespaced>() else {
        return false;
    };
    let Ok(listener) = ListenerOptions::new().name(name).create_sync() else {
        return false;
    };
    std::thread::Builder::new()
        .name("sidcraft-instance".to_owned())
        .spawn(move || serve(listener))
        .is_ok()
}

/// Accepts connections from later launches until the process exits.
fn serve(listener: interprocess::local_socket::Listener) {
    for connection in listener {
        let Ok(connection) = connection else {
            continue;
        };
        let mut data = Vec::new();
        if connection
            .take(MAX_REQUEST_BYTES)
            .read_to_end(&mut data)
            .is_err()
        {
            continue;
        }
        if let Some(request) = parse_request(&data) {
            queue(request);
        }
    }
}

/// Sends `path` (or a focus request) to the running instance.
fn send_request(path: Option<&Path>) -> io::Result<()> {
    let name = instance_name();
    let name = name.to_ns_name::<GenericNamespaced>()?;
    let mut stream = interprocess::local_socket::Stream::connect(name)?;
    // Do not hang a launch for long if the other instance is wedged.
    let _ = stream.set_send_timeout(Some(Duration::from_secs(2)));

    let mut message = format!("{PROTOCOL_MARKER}\n");
    match path {
        Some(path) => {
            // The other process has its own working directory, so a relative
            // path from this launch would point somewhere else there.
            let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            message.push_str("open\n");
            message.push_str(&path.to_string_lossy());
        }
        None => message.push_str("focus\n"),
    }
    stream.write_all(message.as_bytes())?;
    stream.flush()
}

/// Parses a message received from another instance.
fn parse_request(data: &[u8]) -> Option<Request> {
    let text = std::str::from_utf8(data).ok()?;
    let (marker, rest) = text.split_once('\n')?;
    if marker != PROTOCOL_MARKER {
        return None;
    }
    let (command, payload) = rest.split_once('\n')?;
    match command {
        "open" => {
            let path = PathBuf::from(payload);
            is_supported(&path).then_some(Request::Open(path))
        }
        "focus" => Some(Request::Focus),
        _ => None,
    }
}

/// The socket name, which has to be the same in every launch.
///
/// The Linux abstract namespace and macOS `/tmp` are shared by all users, and
/// Windows named pipes are machine wide, so the home directory is mixed into
/// the name to keep separate users' instances apart.
fn instance_name() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    home.hash(&mut hasher);
    format!("{INSTANCE_PREFIX}-{:016x}", hasher.finish())
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

    use super::{Request, is_supported, queue};
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
                    let path = item
                        .fileURLValue()
                        .and_then(|url| url.path().map(|path| path.to_string()))
                        .or_else(|| item.stringValue().map(|path| path.to_string()));
                    if let Some(path) = path {
                        let path = PathBuf::from(path);
                        if is_supported(&path) {
                            queue(Request::Open(path));
                        }
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
