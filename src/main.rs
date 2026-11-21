// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_icon;
mod engine;
mod startup;
mod theme;
mod ui;
mod update;
mod visuals;
mod window_alpha;

use eframe::egui;
use engine::audio::AudioOutput;
use engine::config::Config;
use engine::export::{ExportProgress, ExportSettings};
use engine::hvsc::{HvscBrowser, HvscCollectionFilter, HvscEntry, DEFAULT_HVSC_URL};
use engine::player::{
    FilterInfo, NoteSample, SamplingMethod, SharedPlayer, VoiceInfo, create_shared_player,
};
use engine::playlist::{self, extract_filename, Playlist};
use engine::sid_file::SidFile;
use residfp::ChipModel;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use update::ReleaseInfo;
use visuals::{Spectrogram, Spectrum, VoiceScopes, VuMeter};

/// The primary views of the shell. The Home view is the equivalent of PdfCraft's
/// Home tab; the rest correspond to a workspace tab and its mode-bar tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Home,
    Player,
    Playlist,
    Hvsc,
    RandomList,
    Export,
    Settings,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Player => "Player",
            Self::Playlist => "Playlist",
            Self::Hvsc => "HVSC",
            Self::RandomList => "Random List",
            Self::Export => "Export",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Player => "music",
            Self::Playlist => "list",
            Self::Hvsc => "globe",
            Self::RandomList => "shuffle",
            Self::Export => "save",
            Self::Settings => "gear",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Home => "Overview and recent tunes",
            Self::Player => "Now playing, visualiser and transport",
            Self::Playlist => "Tracks, subsongs and playlist file",
            Self::Hvsc => "Search and browse the High Voltage SID Collection",
            Self::RandomList => "Build a random playlist from HVSC collections",
            Self::Export => "Render the active SID tune to WAV, FLAC or MP3",
            Self::Settings => "Appearance, audio and emulation",
        }
    }
}

/// How the visualiser draws the current voices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizMode {
    /// Envelope history per voice (the original scope).
    Scope,
    /// Per-voice level bars.
    Vu,
    /// Log-spaced FFT spectrum analyser.
    Spectrum,
    /// Scrolling FFT waterfall.
    Spectrogram,
    /// Triggered waveform of the mixed output.
    Oscilloscope,
    /// Note lanes derived from the SID frequency registers.
    PianoRoll,
    /// Filter response curve plus per-voice register cards.
    Filter,
    /// Envelope curves coloured by ADSR stage.
    Adsr,
}

impl VizMode {
    /// Every visualiser, in the order shown in the pickers.
    pub const ALL: [Self; 8] = [
        Self::Scope,
        Self::Vu,
        Self::Spectrum,
        Self::Spectrogram,
        Self::Oscilloscope,
        Self::PianoRoll,
        Self::Filter,
        Self::Adsr,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Scope => "Envelope",
            Self::Vu => "VU meter",
            Self::Spectrum => "Spectrum",
            Self::Spectrogram => "Spectrogram",
            Self::Oscilloscope => "Oscilloscope",
            Self::PianoRoll => "Piano roll",
            Self::Filter => "Filter",
            Self::Adsr => "ADSR",
        }
    }

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Vu => "vu",
            Self::Spectrum => "spectrum",
            Self::Spectrogram => "spectrogram",
            Self::Oscilloscope => "oscilloscope",
            Self::PianoRoll => "pianoroll",
            Self::Filter => "filter",
            Self::Adsr => "adsr",
        }
    }

    pub fn from_config(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|mode| mode.config_key() == key)
            .unwrap_or(Self::Scope)
    }
}

/// How the player reacts when the end of a track is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    /// Keep playing the next track in the playlist (default).
    Off,
    /// Restart the current track when it ends.
    One,
    /// Restart the playlist from its first track when it ends.
    All,
}

/// Smallest size of the full shell window.
const MIN_WINDOW_SIZE: [f32; 2] = [760.0, 520.0];
/// Effectively unconstrained maximum while the full shell is open.
const FULL_MAX_SIZE: [f32; 2] = [16384.0, 16384.0];
/// Size used the first time the mini player is opened.
const MINI_DEFAULT_SIZE: [f32; 2] = [400.0, 300.0];
/// Limits the user can resize the mini player to: large enough for the
/// visualiser and controls, but small enough never to take over the desktop.
const MINI_MIN_SIZE: [f32; 2] = [320.0, 200.0];
const MINI_MAX_SIZE: [f32; 2] = [640.0, 480.0];

/// Full-shell geometry saved while the mini player is active, so leaving it
/// restores the window the user came from.
#[derive(Clone, Copy)]
struct MiniRestore {
    inner_size: egui::Vec2,
    outer_pos: egui::Pos2,
    maximized: bool,
}

/// Viewport bounds just sent to the OS during a mini/full switch. Until the
/// viewport reports them back, the values still belong to the old mode and
/// must not be written into the new mode's saved geometry.
struct PendingGeometry {
    inner_size: Option<egui::Vec2>,
    outer_pos: Option<egui::Pos2>,
    maximized: Option<bool>,
    at: Instant,
}

/// Destructive playlist operation waiting for a Yes/No confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Delete the active playlist file.
    DeletePlaylist,
    /// Remove the selected track from the active playlist.
    RemoveTrack,
    /// Remove every track from the active playlist.
    ClearTracks,
}

impl ConfirmAction {
    /// Window title for this action.
    pub const fn title(self) -> &'static str {
        match self {
            Self::DeletePlaylist => "Delete playlist",
            Self::RemoveTrack => "Remove track",
            Self::ClearTracks => "Clear playlist",
        }
    }
}

enum HvscAsyncMsg {
    EntriesLoaded {
        path: String,
        entries: Vec<HvscEntry>,
    },
    SearchDone(Vec<String>),
    SongDownloaded {
        sid: SidFile,
        source: String,
        subsong: u16,
    },
    Error(String),
}

/// Where the in-app update flow currently is. The release itself is kept
/// separately in [`SidcraftApp::update_release`] so retries do not lose it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    /// Nothing has been checked in this session.
    Idle,
    /// GitHub is being asked for the newest stable release.
    Checking,
    /// A newer release is waiting for the user to start the download.
    Available,
    /// The asset is being written to the Downloads folder.
    Downloading { received: u64, total: Option<u64> },
    /// Cancelled an active download; the worker is winding down.
    Cancelling,
    /// The verified file is on disk and can be installed.
    Ready(PathBuf),
    /// The check finished and this build is the current stable release.
    UpToDate,
    /// Asking GitHub failed.
    CheckFailed(String),
    /// Downloading or verifying the asset failed.
    DownloadFailed(String),
}

enum UpdateAsyncMsg {
    Checked(Result<Option<ReleaseInfo>, String>),
    Progress { received: u64, total: Option<u64> },
    Downloaded { release: ReleaseInfo, path: PathBuf },
    Cancelled,
    DownloadFailed(String),
}

pub struct SidcraftApp {
    player: SharedPlayer,
    sid_file: SidFile,
    current_song: u16,
    total_songs: u16,
    paused: bool,
    chip_models: Vec<ChipModel>,
    selected_sid: usize,
    sample_rate: u32,
    sampling_method: SamplingMethod,
    ekv_filter: bool,

    // Audio stream handle
    _stream: Option<cpal::Stream>,

    // Visuals
    vu_meter: VuMeter,
    voice_scopes: VoiceScopes,
    /// FFT spectrum analyser for the Spectrum and Spectrogram views.
    spectrum: Spectrum,
    /// Scrolling spectrum columns for the waterfall view.
    spectrogram: Spectrogram,
    /// Newest mixed output samples for the oscilloscope.
    audio_samples: Vec<f32>,
    /// Piano-roll history per voice.
    note_history: Vec<Vec<NoteSample>>,
    /// Current register state per voice.
    voice_infos: Vec<VoiceInfo>,
    /// Current filter configuration per SID chip.
    filter_infos: Vec<FilterInfo>,
    /// ADSR stage history per voice.
    envelope_states: Vec<Vec<u8>>,
    /// Raw envelope curves used by the ADSR view.
    envelope_curves: Vec<Vec<f32>>,
    /// Per-voice mute switches mirrored from the player for the mixer UI.
    pub voice_mutes: Vec<bool>,

    // Song timing
    song_elapsed: Duration,
    song_resumed_at: Instant,
    song_timeout: Duration,
    default_timeout: Duration,

    // Seeking (runs off the UI thread because SID has no random access)
    seek_cancel: Arc<AtomicBool>,
    seek_gen: Arc<AtomicU64>,
    seek_tx: Sender<u64>,
    seek_rx: Receiver<u64>,
    seek_target: Duration,
    pub seeking: bool,
    pub current_source: Option<String>,
    pub volume: Arc<Mutex<f32>>,
    pub muted: Arc<AtomicBool>,

    // Playlist
    playlist: Playlist,
    playlist_path: PathBuf,
    selected_playlist_idx: usize,
    /// Row being dragged in the playlist view, for drag-to-reorder.
    playlist_drag_from: Option<usize>,
    /// Scroll the selected playlist row into view on the next render.
    playlist_scroll_to_selected: bool,
    playlist_files: Vec<PathBuf>,
    pub show_new_playlist: bool,
    pub new_playlist_name: String,
    new_playlist_error: Option<String>,
    pub show_rename_playlist: bool,
    pub rename_playlist_name: String,
    rename_playlist_error: Option<String>,
    pub confirm_action: Option<ConfirmAction>,

    // Playback behaviour
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,

    // HVSC Browser
    hvsc_browser: HvscBrowser,
    hvsc_search_query: String,
    hvsc_search_results: Vec<String>,
    hvsc_search_selected: usize,
    hvsc_loading: bool,
    hvsc_async_tx: Sender<HvscAsyncMsg>,
    hvsc_async_rx: Receiver<HvscAsyncMsg>,

    // Random List panel
    random_playlist_name: String,
    random_track_count: u32,
    random_collections: HvscCollectionFilter,
    random_list_error: Option<String>,
    /// Summary of a freshly created random playlist, shown in a confirmation
    /// dialog. Pressing "Ok" reveals the Playlist view.
    random_list_result: Option<String>,

    // Update check and in-app download
    pub update_phase: UpdatePhase,
    /// Newest release returned by GitHub, kept for retries and the dialog.
    pub update_release: Option<ReleaseInfo>,
    /// True when the current check was started by the user, so the result is
    /// also reported in the status bar and the About window.
    pub update_manual: bool,
    /// The startup check runs once per session.
    update_checked_once: bool,
    /// When the automatic check should run; set on the first frame.
    update_auto_at: Option<Instant>,
    update_async_tx: Sender<UpdateAsyncMsg>,
    update_async_rx: Receiver<UpdateAsyncMsg>,
    update_cancel: Arc<AtomicBool>,
    pub show_update_dialog: bool,

    // Shell state
    pub view: View,
    pub theme_pref: theme::ThemePreference,
    applied_theme: Option<theme::ThemeKind>,
    /// Last title sent to the OS title bar, so it is only updated on change.
    window_title: String,
    /// Native handle used to apply OS-level mini-player opacity.
    #[cfg(not(target_arch = "wasm32"))]
    window_handle: Option<raw_window_handle::RawWindowHandle>,
    /// Last alpha actually sent to the OS, to avoid redundant Win32 calls.
    #[cfg(not(target_arch = "wasm32"))]
    last_window_alpha: Option<u8>,
    pub viz_mode: VizMode,
    pub left_open: bool,
    pub right_open: bool,
    /// Frameless compact player; the shell is hidden while it is active.
    mini_mode: bool,
    /// Mini-player window opacity, applied to its framebuffer on all platforms.
    pub mini_opacity: f32,
    /// Open in mini mode on the first rendered frame, then clear the request.
    mini_start_pending: bool,
    /// Open directly in the mini player on every launch.
    pub start_in_mini: bool,
    /// Full-shell geometry saved when the mini player was entered.
    mini_restore: Option<MiniRestore>,
    /// Geometry just sent to the OS while switching between mini and full.
    /// Until it lands (or a short grace period passes) the viewport still
    /// reports the old mode's bounds, so geometry recording is suspended.
    pending_geometry: Option<PendingGeometry>,
    pub show_about: bool,
    pub global_search: String,
    config: Config,

    // UI state
    pub status_message: String,
    status_timer: Instant,
    /// Window geometry changed and still needs to be written to the config.
    geometry_dirty: bool,
    /// Last time the geometry was persisted (throttles resize writes).
    geometry_saved_at: Option<Instant>,
    /// When set, playback is waiting for a 1s gap before advancing to the next track.
    track_gap_until: Option<Instant>,

    // Export panel
    pub export_settings: ExportSettings,
    pub export_progress: Option<ExportProgress>,
    pub export_output_dir: Option<std::path::PathBuf>,
    pub export_last_path: Option<std::path::PathBuf>,
}

fn default_playlist_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sidcraft")
        .join("playlist.m3u")
}

/// Resolves the playlist library at startup.
///
/// Older versions kept a single `playlist.m3u` next to the config file. It is
/// copied (never moved) into `playlists/Default.m3u` so the user's tracks are
/// preserved while the app moves to a playlist library. The playlist recorded
/// in the config wins when it still exists.
fn startup_playlist(config: &Config) -> (Playlist, PathBuf) {
    let dir = playlist::playlists_dir();
    let _ = fs::create_dir_all(&dir);

    let legacy = default_playlist_path();
    let default_path = dir.join("Default.m3u");
    if legacy.is_file() && !default_path.exists() {
        match Playlist::load(&legacy) {
            Ok(playlist) => {
                let _ = playlist.save(&default_path);
            }
            Err(_) => {
                let _ = fs::copy(&legacy, &default_path);
            }
        }
    }

    let active = config
        .active_playlist
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .unwrap_or(default_path);

    if !active.is_file() {
        let _ = Playlist::empty().save(&active);
    }

    let playlist = Playlist::load(&active).unwrap_or_else(|_| Playlist::empty());
    (playlist, active)
}

/// Loads the first entry of `playlist`, or a silent stub when the playlist is
/// empty or its first file cannot be read.
fn first_playlist_tune(playlist: &Playlist) -> (SidFile, u16, Option<String>) {
    let Some(entry) = playlist.entries.first() else {
        return (SidFile::silent(), 1, None);
    };
    match entry.load() {
        Ok(sid) => {
            let song = entry.subsong.unwrap_or(sid.start_song);
            (sid, song, Some(entry.source.clone()))
        }
        Err(_) => (SidFile::silent(), 1, None),
    }
}

impl SidcraftApp {
    /// macOS renders the native title bar through the window's alpha channel,
    /// so a transparent viewport makes it see-through. The alpha buffer is still
    /// required for the mini player's opacity, hence toggle the OS window
    /// transparency around mode switches instead of removing it at creation.
    #[cfg(target_os = "macos")]
    fn set_macos_window_transparency(ctx: &egui::Context, transparent: bool) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Transparent(transparent));
    }

    pub fn new(cc: &eframe::CreationContext<'_>, startup_path: Option<PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        // Keep the full shell's title bar opaque on macOS. The viewport itself
        // stays transparent so the frameless mini player can still fade.
        #[cfg(target_os = "macos")]
        Self::set_macos_window_transparency(&ctx, false);
        startup::set_wakeup(move || ctx.request_repaint());
        let config = Config::load();
        // Remembered filter model: 6581 chips use the EKV transistor model
        // when the user turned it on in Settings or the player panel.
        let ekv_filter = config.ekv_filter;

        // A file named by the OS ("Open with", Finder, a file manager or the
        // shell) wins over the saved playlist. An M3U becomes the active
        // playlist; a SID is played directly and leaves the playlist alone.
        let mut startup_error = None;
        let (playlist, playlist_path) = match startup_path
            .as_deref()
            .filter(|path| startup::is_playlist(path))
        {
            Some(path) => match Playlist::load(path) {
                Ok(playlist) => (playlist, path.to_path_buf()),
                Err(e) => {
                    startup_error =
                        Some(format!("Could not open playlist {}: {e}", path.display()));
                    startup_playlist(&config)
                }
            },
            None => startup_playlist(&config),
        };

        let mut playlist_files = playlist::list_playlists();
        if !playlist_files.contains(&playlist_path) && playlist_path.is_file() {
            playlist_files.push(playlist_path.clone());
        }

        let (sid_file, initial_song, initial_source) = match startup_path
            .as_deref()
            .filter(|path| startup::is_sid(path))
        {
            Some(path) => match SidFile::load(path) {
                Ok(sid) => {
                    let song = sid.start_song;
                    let source = path.to_string_lossy().to_string();
                    (sid, song, Some(source))
                }
                Err(e) => {
                    startup_error = Some(format!(
                        "Could not open {}: {e}; continuing with the playlist",
                        path.display()
                    ));
                    first_playlist_tune(&playlist)
                }
            },
            None => first_playlist_tune(&playlist),
        };

        let audio = match AudioOutput::probe() {
            Ok(a) => a,
            Err(e) => {
                eprintln!("Audio probe error: {e}");
                panic!("Failed to probe audio device: {e}");
            }
        };
        let sample_rate = audio.sample_rate;
        let sampling_method = SamplingMethod::ResampleTwoPass;

        // The initial playlist entry may be unplayable (RSID needing hardware we
        // don't emulate, broken file, ...). Never abort startup for that: fall
        // back to the silent stub and let `load_sid_direct` below report the
        // failure in the status bar while the rest of the app stays usable.
        let player = match create_shared_player(
            &sid_file,
            initial_song,
            sample_rate,
            None,
            sampling_method,
        ) {
            Ok(player) => player,
            Err(e) => {
                eprintln!("Initial player creation failed: {e}");
                create_shared_player(
                    &SidFile::silent(),
                    1,
                    sample_rate,
                    None,
                    sampling_method,
                )
                .expect("silent player creation failed")
            }
        };

        let chip_models = player
            .lock()
            .map(|p| p.chip_models().to_vec())
            .unwrap_or_else(|_| vec![ChipModel::Mos6581]);
        let sid_count = chip_models.len();

        let player_audio = Arc::clone(&player);
        let volume_state = Arc::new(Mutex::new(1.0f32));
        let muted_state = Arc::new(AtomicBool::new(false));
        let vol_handle = Arc::clone(&volume_state);
        let muted_handle = Arc::clone(&muted_state);
        let stream = match audio.start(move |data| {
            // Fill the buffer first: `fill_buffer` overwrites every sample, so
            // the gain/mute stage has to run afterwards or it would be lost.
            if let Ok(mut p) = player_audio.lock() {
                p.fill_buffer(data);
            }
            let vol = if muted_handle.load(Ordering::SeqCst) {
                0.0
            } else {
                *vol_handle.lock().unwrap_or_else(|e| e.into_inner())
            };
            if vol != 1.0 {
                for s in data.iter_mut() {
                    *s *= vol;
                }
            }
        }) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("Audio start error: {e}");
                None
            }
        };

        let mut hvsc_browser = HvscBrowser::new(engine::hvsc::DEFAULT_HVSC_URL);
        hvsc_browser.load_stil();

        let (hvsc_async_tx, hvsc_async_rx) = channel();
        let (seek_tx, seek_rx) = channel();
        let (update_async_tx, update_async_rx) = channel();

        // Initial fetch of HVSC root directory
        let tx = hvsc_async_tx.clone();
        let browser_clone = hvsc_browser.clone();
        let root_path = browser_clone.current_path.clone();
        std::thread::spawn(move || match browser_clone.list() {
            Ok(entries) => {
                let _ = tx.send(HvscAsyncMsg::EntriesLoaded {
                    path: root_path,
                    entries,
                });
            }
            Err(e) => {
                let _ = tx.send(HvscAsyncMsg::Error(e.to_string()));
            }
        });

        let mut app = Self {
            player,
            sid_file: SidFile::silent(),
            current_song: initial_song,
            total_songs: 1,
            paused: false,
            chip_models,
            selected_sid: 0,
            sample_rate,
            sampling_method,
            ekv_filter,
            _stream: stream,
            vu_meter: VuMeter::with_voice_count(sid_count * 3),
            voice_scopes: VoiceScopes::with_voice_count(sid_count * 3),
            spectrum: Spectrum::new(),
            spectrogram: Spectrogram::new(),
            audio_samples: Vec::new(),
            note_history: Vec::new(),
            voice_infos: Vec::new(),
            filter_infos: Vec::new(),
            envelope_states: Vec::new(),
            envelope_curves: Vec::new(),
            voice_mutes: vec![false; sid_count * 3],
            song_elapsed: Duration::ZERO,
            song_resumed_at: Instant::now(),
            song_timeout: Duration::from_secs(180),
            default_timeout: Duration::from_secs(180),
            seek_cancel: Arc::new(AtomicBool::new(false)),
            seek_gen: Arc::new(AtomicU64::new(0)),
            seek_tx,
            seek_rx,
            seek_target: Duration::ZERO,
            seeking: false,
            current_source: None,
            volume: Arc::clone(&volume_state),
            muted: Arc::clone(&muted_state),
            playlist,
            playlist_path,
            selected_playlist_idx: 0,
            playlist_drag_from: None,
            playlist_scroll_to_selected: false,
            playlist_files,
            show_new_playlist: false,
            new_playlist_name: String::new(),
            new_playlist_error: None,
            show_rename_playlist: false,
            rename_playlist_name: String::new(),
            rename_playlist_error: None,
            confirm_action: None,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            hvsc_browser,
            hvsc_search_query: String::new(),
            hvsc_search_results: Vec::new(),
            hvsc_search_selected: 0,
            hvsc_loading: true,
            hvsc_async_tx,
            hvsc_async_rx,
            random_playlist_name: String::new(),
            random_track_count: 25,
            random_collections: HvscCollectionFilter::default(),
            random_list_error: None,
            random_list_result: None,
            update_phase: UpdatePhase::Idle,
            update_release: None,
            update_manual: false,
            update_checked_once: false,
            update_auto_at: None,
            update_async_tx,
            update_async_rx,
            update_cancel: Arc::new(AtomicBool::new(false)),
            show_update_dialog: false,
            view: View::Home,
            theme_pref: match config.theme.as_str() {
                "light" => theme::ThemePreference::Light,
                "dark" => theme::ThemePreference::Dark,
                _ => theme::ThemePreference::System,
            },
            applied_theme: None,
            window_title: "SidCraft".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            window_handle: None,
            #[cfg(not(target_arch = "wasm32"))]
            last_window_alpha: None,
            viz_mode: VizMode::from_config(&config.viz_mode),
            left_open: config.left_panel,
            right_open: config.right_panel,
            mini_mode: false,
            mini_opacity: config.mini_opacity.clamp(0.25, 1.0),
            mini_start_pending: config.start_in_mini,
            start_in_mini: config.start_in_mini,
            mini_restore: None,
            pending_geometry: None,
            show_about: false,
            global_search: String::new(),
            config,
            status_message: "Welcome to SidCraft".into(),
            status_timer: Instant::now(),
            geometry_dirty: false,
            geometry_saved_at: None,
            track_gap_until: None,
            export_settings: ExportSettings {
                ekv_filter,
                ..ExportSettings::default()
            },
            export_progress: None,
            export_output_dir: None,
            export_last_path: None,
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            use raw_window_handle::HasWindowHandle;
            app.window_handle = cc.window_handle().ok().map(|h| h.as_raw().clone());
        }

        if !sid_file.name.is_empty() {
            app.load_sid_direct(sid_file, Some(initial_song), initial_source);
            app.view = View::Player;
        } else {
            app.paused = true;
            app.song_elapsed = Duration::ZERO;
            app.song_resumed_at = Instant::now();
            if let Ok(mut p) = app.player.lock() {
                if !p.is_paused() {
                    p.toggle_pause();
                }
            }
        }
        if let Some(error) = startup_error {
            app.set_status(error);
        }

        app
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = msg.into();
        self.status_timer = Instant::now();
    }

    /// Persist shell preferences whenever the user changes one.
    pub fn sync_config(&mut self) {
        let theme = match self.theme_pref {
            theme::ThemePreference::System => "system",
            theme::ThemePreference::Light => "light",
            theme::ThemePreference::Dark => "dark",
        };
        let viz = self.viz_mode.config_key();
        let active = self.playlist_path.to_string_lossy().to_string();
        if self.config.theme != theme
            || self.config.left_panel != self.left_open
            || self.config.right_panel != self.right_open
            || self.config.viz_mode != viz
            || self.config.ekv_filter != self.ekv_filter
            || self.config.active_playlist.as_deref() != Some(active.as_str())
            || (self.config.mini_opacity - self.mini_opacity).abs() > 0.001
            || self.config.start_in_mini != self.start_in_mini
        {
            self.config.theme = theme.to_string();
            self.config.left_panel = self.left_open;
            self.config.right_panel = self.right_open;
            self.config.viz_mode = viz.to_string();
            self.config.ekv_filter = self.ekv_filter;
            self.config.active_playlist = Some(active);
            self.config.mini_opacity = self.mini_opacity;
            self.config.start_in_mini = self.start_in_mini;
            self.config.save();
        }
    }

    pub fn load_sid_direct(&mut self, sid: SidFile, song: Option<u16>, source: Option<String>) {
        self.current_source = source;
        self.cancel_seek();
        let subsong = song.unwrap_or(sid.start_song);
        let chip_override = self.chip_models.first().copied();

        let load_res = if let Ok(mut p) = self.player.lock() {
            if chip_override.is_some() {
                p.switch_chip_model(0); // sync if needed
            }
            p.load_sid_file(&sid, subsong).map_err(|e| e.to_string())
        } else {
            Err("Lock poisoned".to_string())
        };

        match load_res {
            Ok(()) => {
                if self.ekv_filter {
                    if let Ok(mut p) = self.player.lock() {
                        p.set_ekv_filter(true);
                    }
                }
                self.current_song = subsong;
                self.total_songs = sid.songs;
                self.song_elapsed = Duration::ZERO;
                self.song_resumed_at = Instant::now();
                self.paused = false;

                // HVSC Songlengths are keyed by the SID file MD5 (the same lookup
                // crabsid uses); fall back to the fixed playtime for unknown tunes.
                let duration = self.hvsc_browser.song_duration(&sid.md5, subsong);
                self.song_timeout = duration.unwrap_or(self.default_timeout);

                if let Ok(p) = self.player.lock() {
                    self.chip_models = p.chip_models().to_vec();
                }
                let sid_count = self.chip_models.len();
                self.vu_meter = VuMeter::with_voice_count(sid_count * 3);
                self.voice_scopes = VoiceScopes::with_voice_count(sid_count * 3);
                self.voice_mutes = vec![false; sid_count * 3];
                self.spectrum = Spectrum::new();
                self.spectrogram.clear();
                self.audio_samples.clear();
                self.note_history.clear();
                self.voice_infos.clear();
                self.filter_infos.clear();
                self.envelope_states.clear();
                self.envelope_curves.clear();
                self.set_status(format!("Playing: {}", sid.name));
                self.sid_file = sid;
                self.export_settings.subsong = subsong;
                self.export_settings.duration_secs = self.song_timeout.as_secs_f32();
                self.export_settings.ekv_filter = self.ekv_filter;
                self.export_settings.voice_mutes = vec![false; sid_count * 3];
            }
            Err(e) => {
                self.set_status(format!("Load failed: {e}"));
            }
        }
    }

    pub fn toggle_pause(&mut self) {
        self.cancel_seek();
        let (now_paused, was_paused) = {
            let Ok(mut p) = self.player.lock() else {
                return;
            };
            p.toggle_pause();
            let was = self.paused;
            (p.is_paused(), was)
        };

        self.paused = now_paused;
        if self.paused && !was_paused {
            self.song_elapsed += self.song_resumed_at.elapsed();
            self.set_status("Playback paused");
        } else if !self.paused && was_paused {
            self.song_resumed_at = Instant::now();
            self.set_status("Playback resumed");
        }
    }

    pub fn stop_playback(&mut self) {
        self.cancel_seek();
        {
            let Ok(mut p) = self.player.lock() else {
                return;
            };
            if !p.is_paused() {
                p.toggle_pause();
            }
        }
        self.paused = true;
        self.song_elapsed = Duration::ZERO;
        self.set_status("Playback stopped");
    }

    pub fn change_subsong(&mut self, song: u16) {
        if song < 1 || song > self.total_songs {
            return;
        }
        self.cancel_seek();
        let success = {
            let Ok(mut p) = self.player.lock() else {
                return;
            };
            p.load_song(song).is_ok()
        };

        if success {
            self.current_song = song;
            self.song_elapsed = Duration::ZERO;
            self.song_resumed_at = Instant::now();
            let md5 = &self.sid_file.md5;
            self.song_timeout = self
                .hvsc_browser
                .song_duration(md5, song)
                .unwrap_or(self.default_timeout);
            self.export_settings.subsong = song;
            self.export_settings.duration_secs = self.song_timeout.as_secs_f32();
            self.set_status(format!("Subsong: {}/{}", song, self.total_songs));
        }
    }

    pub fn cycle_chip_model(&mut self) {
        let (model, count) = {
            let Ok(mut p) = self.player.lock() else {
                return;
            };
            let sid_count = p.sid_count();
            if self.selected_sid >= sid_count {
                self.selected_sid = 0;
            }
            p.switch_chip_model(self.selected_sid);
            self.chip_models = p.chip_models().to_vec();
            (self.chip_models[self.selected_sid], sid_count)
        };

        self.set_status(format!("SID #{} -> {:?}", self.selected_sid + 1, model));
        if count > 1 {
            self.selected_sid = (self.selected_sid + 1) % count;
        }
    }

    pub fn set_ekv_filter(&mut self, enabled: bool) {
        if let Ok(mut p) = self.player.lock() {
            p.set_ekv_filter(enabled);
        }
        self.ekv_filter = enabled;
        self.sync_config();
        self.set_status(if enabled {
            "EKV transistor filter enabled"
        } else {
            "EKV transistor filter disabled"
        });
    }

    pub fn play_playlist_index(&mut self, idx: usize) {
        self.track_gap_until = None;
        if idx >= self.playlist.len() {
            return;
        }
        self.selected_playlist_idx = idx;
        let entry = self.playlist.entries[idx].clone();
        match entry.load() {
            Ok(sid) => {
                let song = entry.subsong.unwrap_or(sid.start_song);
                self.load_sid_direct(sid, Some(song), Some(entry.source));
            }
            Err(e) => {
                self.set_status(format!("Playlist load err: {e}"));
            }
        }
    }

    pub fn next_track(&mut self) {
        if !self.playlist.is_empty() {
            let next = if self.repeat_mode == RepeatMode::One {
                // Repeat one: replay the current track.
                self.selected_playlist_idx
            } else if self.shuffle && self.playlist.len() > 1 {
                let len = self.playlist.len();
                let mut idx = self.random_playlist_index(len, self.selected_playlist_idx);
                if idx == self.selected_playlist_idx {
                    idx = (idx + 1) % len;
                }
                idx
            } else {
                (self.selected_playlist_idx + 1) % self.playlist.len()
            };
            self.play_playlist_index(next);
        } else if self.current_song < self.total_songs {
            self.change_subsong(self.current_song + 1);
        }
    }

    pub fn prev_track(&mut self) {
        if self.repeat_mode == RepeatMode::One {
            // Repeat one: restart the current track.
            if !self.playlist.is_empty() {
                self.play_playlist_index(self.selected_playlist_idx);
            } else if self.current_song > 1 {
                self.change_subsong(self.current_song - 1);
            }
            return;
        }
        if !self.playlist.is_empty() {
            let prev = if self.selected_playlist_idx == 0 {
                self.playlist.len().saturating_sub(1)
            } else {
                self.selected_playlist_idx - 1
            };
            self.play_playlist_index(prev);
        } else if self.current_song > 1 {
            self.change_subsong(self.current_song - 1);
        }
    }

    /// Picks a pseudo-random playlist index different from `exclude`.
    fn random_playlist_index(&self, len: usize, exclude: usize) -> usize {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as usize)
            .unwrap_or(0);
        let mut state = nanos
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut idx = ((state >> 33) as usize) % len;
        let mut guard = 0;
        while idx == exclude && guard < 100 {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            idx = ((state >> 33) as usize) % len;
            guard += 1;
        }
        idx
    }

    /// Cycles the repeat mode: off -> one -> all -> off.
    pub fn cycle_repeat(&mut self) {
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::One,
            RepeatMode::One => RepeatMode::All,
            RepeatMode::All => RepeatMode::Off,
        };
    }

    pub fn open_file_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("SID Files", &["sid", "mus"])
            .add_filter("Playlists", &["m3u"])
            .pick_file()
        {
            // Loading an .m3u makes it the active playlist and starts its
            // first track; the previous list stays saved on disk.
            if startup::is_playlist(&path) {
                self.switch_playlist(path);
                return;
            }
            let path_str = path.to_string_lossy().to_string();
            match SidFile::load(&path) {
                Ok(sid) => {
                    if self.playlist.add(&path_str, None) {
                        self.save_playlist();
                    }
                    self.selected_playlist_idx = self.playlist.len().saturating_sub(1);
                    self.load_sid_direct(sid, None, Some(path_str));
                    self.view = View::Player;
                }
                Err(e) => self.set_status(format!("Error: {e}")),
            }
        }
    }

    /// Opens a tune or playlist handed over by the OS or the command line.
    ///
    /// Unlike the file dialog this does not add the tune to the playlist: an
    /// "Open with" request is a one-off request to play that file.
    pub fn open_path(&mut self, path: PathBuf) {
        if startup::is_playlist(&path) {
            self.switch_playlist(path);
            return;
        }
        let path_str = path.to_string_lossy().to_string();
        match SidFile::load(&path) {
            Ok(sid) => {
                self.load_sid_direct(sid, None, Some(path_str));
                self.view = View::Player;
            }
            Err(e) => self.set_status(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// Opens files the OS asked for while the app was already running.
    fn handle_open_requests(&mut self, ctx: &egui::Context) {
        let requests = startup::take_requests();
        if requests.is_empty() {
            return;
        }
        // A forwarded launch is a user action: bring the window forward.
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        for request in requests {
            match request {
                startup::Request::Open(path) => self.open_path(path),
                startup::Request::Focus => {}
            }
        }
        ctx.request_repaint();
    }

    /// Makes `path` the active playlist, loading its entries and starting the
    /// first track. An empty playlist leaves whatever is playing untouched.
    pub fn switch_playlist(&mut self, path: PathBuf) {
        if path == self.playlist_path {
            return;
        }
        match Playlist::load(&path) {
            Ok(playlist) => {
                self.playlist = playlist;
                self.playlist_path = path;
                self.selected_playlist_idx = 0;
                self.playlist_drag_from = None;
                self.playlist_scroll_to_selected = false;
                self.refresh_playlist_files();
                if !self.playlist.is_empty() {
                    self.play_playlist_index(0);
                }
                self.sync_config();
                self.set_status(format!(
                    "Playlist “{}” — {} tracks",
                    playlist::playlist_display_name(&self.playlist_path),
                    self.playlist.len()
                ));
            }
            Err(e) => self.set_status(format!("Could not load playlist: {e}")),
        }
    }

    /// Re-reads the playlist library directory.
    pub fn refresh_playlist_files(&mut self) {
        self.playlist_files = playlist::list_playlists();
        if !self.playlist_files.contains(&self.playlist_path) && self.playlist_path.is_file() {
            self.playlist_files.push(self.playlist_path.clone());
        }
    }

    /// Creates an empty playlist in the library and makes it active.
    /// Returns `false` when the name is already taken or the file cannot be
    /// written.
    pub fn create_playlist(&mut self, name: &str) -> bool {
        self.new_playlist_error = None;
        let path = playlist::new_playlist_path(name);
        if path.exists() {
            let message = format!(
                "A playlist named “{}” already exists",
                playlist::playlist_display_name(&path)
            );
            self.new_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }
        if let Err(e) = fs::create_dir_all(playlist::playlists_dir()) {
            let message = format!("Could not create playlist folder: {e}");
            self.new_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }
        if let Err(e) = Playlist::default().save(&path) {
            let message = format!("Could not create playlist: {e}");
            self.new_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }
        self.switch_playlist(path);
        self.set_status(format!(
            "Created playlist “{}”",
            playlist::playlist_display_name(&self.playlist_path)
        ));
        true
    }

    /// Builds a playlist from randomly sampled HVSC paths and activates it
    /// without interrupting the current playback.
    pub fn create_random_playlist(&mut self) {
        self.random_list_error = None;

        let name = self.random_playlist_name.trim().to_string();
        if name.is_empty() {
            self.fail_random_list("Enter a name for the random playlist.");
            return;
        }
        if !self.random_collections.any() {
            self.fail_random_list("Select at least one collection: Demos, Games or Musicians.");
            return;
        }

        let count = self.random_track_count.clamp(5, 100) as usize;
        let paths = self
            .hvsc_browser
            .random_paths(self.random_collections, count);
        if paths.is_empty() {
            self.fail_random_list("No HVSC tunes are available for the selected collections.");
            return;
        }

        let path = playlist::new_playlist_path(&name);
        if path.exists() {
            let message = format!(
                "A playlist named “{}” already exists",
                playlist::playlist_display_name(&path)
            );
            self.fail_random_list(&message);
            return;
        }
        if let Err(e) = fs::create_dir_all(playlist::playlists_dir()) {
            self.fail_random_list(&format!("Could not create playlist folder: {e}"));
            return;
        }

        let mut list = Playlist::empty();
        for hvsc_path in paths {
            list.add(&format!("{DEFAULT_HVSC_URL}{hvsc_path}"), None);
        }
        if let Err(e) = list.save(&path) {
            self.fail_random_list(&format!("Could not save random playlist: {e}"));
            return;
        }

        let created = list.len();
        self.playlist = list;
        self.playlist_path = path;
        self.selected_playlist_idx = 0;
        self.playlist_drag_from = None;
        self.playlist_scroll_to_selected = false;
        self.refresh_playlist_files();
        self.sync_config();
        self.random_playlist_name.clear();

        let mut message = format!(
            "Created random playlist “{}” — {} tracks",
            playlist::playlist_display_name(&self.playlist_path),
            created
        );
        if created < count {
            message.push_str(&format!(" (only {created} available)"));
        }
        self.set_status(message.clone());
        // Report the result in a dialog; the Playlist view is revealed only
        // after the user acknowledges it.
        self.random_list_result = Some(message);
    }

    fn fail_random_list(&mut self, message: &str) {
        self.random_list_error = Some(message.to_string());
        self.set_status(message);
    }

    pub fn save_playlist(&mut self) {
        if let Err(e) = self.playlist.save(&self.playlist_path) {
            self.set_status(format!("Could not save playlist: {e}"));
        }
    }

    /// Adds the currently playing tune to the active playlist, unless it is
    /// already part of it. Playback keeps running untouched.
    pub fn add_current_to_playlist(&mut self) {
        if self.sid_file.name.is_empty() {
            self.set_status("No tune loaded");
            return;
        }
        let Some(source) = self.current_source.clone() else {
            self.set_status("No tune source to add");
            return;
        };
        if self.playlist.entries.iter().any(|e| e.source == source) {
            self.set_status("Already in the playlist");
            return;
        }
        if self.playlist.add(&source, Some(self.current_song)) {
            self.save_playlist();
            self.set_status(format!("Added “{}” to the playlist", self.sid_file.name));
        }
    }

    /// Starts playback of the whole active playlist from its first track.
    pub fn play_all(&mut self) {
        if !self.playlist.is_empty() {
            self.play_playlist_index(0);
        }
    }

    /// Removes the selected track from the active playlist and persists it.
    pub fn remove_playlist_track(&mut self) {
        if self.playlist.is_empty() {
            return;
        }
        self.playlist.remove(self.selected_playlist_idx);
        if self.selected_playlist_idx >= self.playlist.len() && !self.playlist.is_empty() {
            self.selected_playlist_idx = self.playlist.len() - 1;
        }
        self.save_playlist();
        self.set_status(format!(
            "Removed “{}” from the playlist",
            playlist::playlist_display_name(&self.playlist_path)
        ));
    }

    /// Moves the selected playlist entry to the final index `to`, keeping the
    /// selection on the moved track and persisting the new order.
    pub fn move_selected_playlist_entry(&mut self, to: usize) {
        let from = self.selected_playlist_idx;
        if from >= self.playlist.len() {
            return;
        }
        let name = self.playlist.entries[from].display_name.clone();
        let Some(new_index) = self.playlist.move_entry(from, to) else {
            return;
        };
        self.selected_playlist_idx = new_index;
        self.playlist_scroll_to_selected = true;
        self.save_playlist();
        self.set_status(format!(
            "Moved “{name}” to position {} of {}",
            new_index + 1,
            self.playlist.len()
        ));
    }

    /// Moves the selected track one position up.
    pub fn move_selected_track_up(&mut self) {
        if self.selected_playlist_idx > 0 {
            self.move_selected_playlist_entry(self.selected_playlist_idx - 1);
        }
    }

    /// Moves the selected track one position down.
    pub fn move_selected_track_down(&mut self) {
        let next = self.selected_playlist_idx + 1;
        if next < self.playlist.len() {
            self.move_selected_playlist_entry(next);
        }
    }

    /// Moves the selected track to the top of the playlist.
    pub fn move_selected_track_to_top(&mut self) {
        self.move_selected_playlist_entry(0);
    }

    /// Moves the selected track to the bottom of the playlist.
    pub fn move_selected_track_to_bottom(&mut self) {
        let last = self.playlist.len().saturating_sub(1);
        self.move_selected_playlist_entry(last);
    }

    /// Clears every track of the active playlist and persists it.
    pub fn clear_playlist_tracks(&mut self) {
        self.playlist.entries.clear();
        self.selected_playlist_idx = 0;
        self.playlist_drag_from = None;
        self.save_playlist();
        self.set_status("Playlist cleared");
    }

    /// Deletes the active playlist file and activates the next playlist in the
    /// library, or a fresh empty `Default.m3u` when none is left. Playback keeps
    /// running untouched.
    pub fn delete_active_playlist(&mut self) {
        let path = self.playlist_path.clone();
        let name = playlist::playlist_display_name(&path);
        if let Err(e) = fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            self.set_status(format!("Could not delete playlist: {e}"));
            return;
        }

        self.playlist_files.retain(|p| *p != path);
        self.selected_playlist_idx = 0;
        self.playlist_drag_from = None;
        self.playlist_scroll_to_selected = false;

        if let Some(next) = self.playlist_files.first().cloned() {
            self.playlist = Playlist::load(&next).unwrap_or_default();
            self.playlist_path = next;
        } else {
            let dir = playlist::playlists_dir();
            let _ = fs::create_dir_all(&dir);
            let fallback = dir.join("Default.m3u");
            let _ = Playlist::default().save(&fallback);
            self.playlist = Playlist::default();
            self.playlist_path = fallback;
        }

        self.refresh_playlist_files();
        self.sync_config();
        self.set_status(format!("Deleted playlist “{name}”"));
    }

    /// Renames the active playlist file, keeping it in its own directory.
    /// Returns `false` when the name is empty, already taken or the file cannot
    /// be moved.
    pub fn rename_playlist(&mut self, name: &str) -> bool {
        self.rename_playlist_error = None;
        if name.trim().is_empty() {
            let message = "Playlist name cannot be empty".to_string();
            self.rename_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }

        let parent = self
            .playlist_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(playlist::playlists_dir);
        let new_path =
            parent.join(format!("{}.m3u", playlist::sanitize_playlist_name(name)));
        if new_path == self.playlist_path {
            return true;
        }
        if new_path.exists() {
            let message = format!(
                "A playlist named “{}” already exists",
                playlist::playlist_display_name(&new_path)
            );
            self.rename_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }
        if let Err(e) = fs::rename(&self.playlist_path, &new_path) {
            let message = format!("Could not rename playlist: {e}");
            self.rename_playlist_error = Some(message.clone());
            self.set_status(message);
            return false;
        }

        self.playlist_path = new_path;
        self.refresh_playlist_files();
        self.sync_config();
        self.set_status(format!(
            "Renamed playlist to “{}”",
            playlist::playlist_display_name(&self.playlist_path)
        ));
        true
    }

    /// Opens the OS file manager on the playlist library folder.
    pub fn open_playlist_folder(&mut self) {
        let dir = playlist::playlists_dir();
        let _ = fs::create_dir_all(&dir);
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        }
        #[cfg(not(windows))]
        {
            let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
        }
        self.set_status(format!("Opening playlist folder {}", dir.display()));
    }

    /// Saves the active playlist to a user-chosen location ("Save As"). A copy
    /// is also placed in the playlist library so the list shows up in the
    /// Library dropdown; that library copy becomes the active playlist.
    pub fn save_playlist_as(&mut self) -> bool {
        let dialog = rfd::FileDialog::new();
        let Some(path) = dialog.add_filter("Playlists", &["m3u"]).save_file() else {
            return false;
        };

        let m3u: String = self
            .playlist
            .entries
            .iter()
            .map(|e| {
                if let Some(sub) = e.subsong {
                    format!("{}@{}\n", e.source, sub)
                } else {
                    format!("{}\n", e.source)
                }
            })
            .collect();
        if let Err(e) = fs::write(&path, m3u) {
            self.set_status(format!("Could not save playlist: {e}"));
            return false;
        }

        let library = playlist::playlists_dir();
        let mut active = path.clone();
        // Saving directly into the library needs no second copy.
        if path.parent() != Some(library.as_path()) {
            if let Err(e) = fs::create_dir_all(&library) {
                self.set_status(format!("Could not create playlist folder: {e}"));
                return false;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Playlist".to_string());
            let stem = playlist::sanitize_playlist_name(&stem);
            let mut copy = library.join(format!("{stem}.m3u"));
            let mut suffix = 2;
            while copy.exists() {
                copy = library.join(format!("{stem} ({suffix}).m3u"));
                suffix += 1;
            }
            if let Err(e) = fs::copy(&path, &copy) {
                self.set_status(format!("Could not copy playlist to library: {e}"));
                return false;
            }
            active = copy;
        }

        self.playlist_path = active.clone();
        self.refresh_playlist_files();
        self.sync_config();
        if active == path {
            self.set_status(format!("Saved playlist to {}", path.display()));
        } else {
            self.set_status(format!(
                "Saved playlist to {} (library copy: {})",
                path.display(),
                active.display()
            ));
        }
        true
    }

    /// Makes `path` the active playlist without starting playback.
    pub fn select_playlist(&mut self, path: PathBuf) {
        if path == self.playlist_path {
            return;
        }
        match Playlist::load(&path) {
            Ok(loaded) => {
                self.playlist = loaded;
                self.playlist_path = path.clone();
                self.selected_playlist_idx = 0;
                self.refresh_playlist_files();
                self.sync_config();
                self.set_status(format!(
                    "Playlist “{}” — {} tracks",
                    playlist::playlist_display_name(&self.playlist_path),
                    self.playlist.len()
                ));
            }
            Err(e) => {
                self.set_status(format!("Could not load playlist: {e}"));
            }
        }
    }

    /// Batch picker for adding several SID files to the playlist at once. Each
    /// newly added track becomes the selected entry; the first added track is
    /// played.
    pub fn add_files_dialog(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("SID Files", &["sid", "mus"])
            .pick_files()
        {
            let mut added_count = 0usize;
            let mut first: Option<(usize, SidFile, String)> = None;
            for path in paths {
                let path_str = path.to_string_lossy().to_string();
                if let Ok(sid) = SidFile::load(&path) {
                    if self.playlist.add(&path_str, None) {
                        let idx = self.playlist.len() - 1;
                        if first.is_none() {
                            first = Some((idx, sid, path_str));
                        }
                        added_count += 1;
                    }
                }
            }
            if let Some((idx, _sid, _path_str)) = first {
                self.save_playlist();
                self.selected_playlist_idx = idx;
                self.set_status(format!("Added {} files to the playlist", added_count));
            }
        }
    }

    /// Start an HVSC search in the background. The result lands in
    /// [`SidcraftApp::handle_async_hvsc`].
    pub fn start_hvsc_search(&mut self) {
        let query = self.hvsc_search_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.hvsc_loading = true;
        self.hvsc_search_results.clear();
        let tx = self.hvsc_async_tx.clone();
        let browser = self.hvsc_browser.clone();
        self.set_status(format!("Searching HVSC for \"{query}\"…"));
        std::thread::spawn(move || match browser.search(&query) {
            Ok(res) => {
                let _ = tx.send(HvscAsyncMsg::SearchDone(res));
            }
            Err(e) => {
                let _ = tx.send(HvscAsyncMsg::Error(e.to_string()));
            }
        });
    }

    pub fn hvsc_go_up(&mut self) {
        let current = self.hvsc_browser.current_path.trim_end_matches('/');
        if current.is_empty() {
            return;
        }
        let parent = match current.rfind('/') {
            Some(0) | None => "/".to_string(),
            Some(pos) => format!("{}/", &current[..pos]),
        };
        self.hvsc_open_dir(&parent);
    }

    pub fn hvsc_open_dir(&mut self, path: &str) {
        let mut target = if path.is_empty() {
            "/".to_string()
        } else if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        if target.len() > 1 && !target.ends_with('/') {
            target.push('/');
        }

        self.hvsc_loading = true;
        self.hvsc_search_results.clear();
        self.set_status(format!("Browsing {target}…"));

        let tx = self.hvsc_async_tx.clone();
        let mut browser = self.hvsc_browser.clone();
        std::thread::spawn(move || {
            browser.current_path = target.clone();
            match browser.list() {
                Ok(entries) => {
                    let _ = tx.send(HvscAsyncMsg::EntriesLoaded {
                        path: target,
                        entries,
                    });
                }
                Err(e) => {
                    let _ = tx.send(HvscAsyncMsg::Error(e.to_string()));
                }
            }
        });
    }

    pub fn hvsc_fetch(&mut self, path: &str) {
        // Do not set `hvsc_loading` here: that flag swaps the browser list for
        // a spinner, and fetching a single tune should leave the list in place.
        self.set_status(format!("Loading {path}…"));
        let tx = self.hvsc_async_tx.clone();
        let browser = self.hvsc_browser.clone();
        let path = path.to_string();
        // Store the full URL in the playlist so entries stay loadable even when
        // they are played outside the HVSC browser.
        let source = format!("{DEFAULT_HVSC_URL}{path}");
        std::thread::spawn(move || match browser.fetch_entry(&path) {
            Ok(sid) => {
                let subsong = sid.start_song;
                let _ = tx.send(HvscAsyncMsg::SongDownloaded {
                    sid,
                    source,
                    subsong,
                });
            }
            Err(e) => {
                let _ = tx.send(HvscAsyncMsg::Error(e.to_string()));
            }
        });
    }

    fn handle_async_hvsc(&mut self) {
        while let Ok(msg) = self.hvsc_async_rx.try_recv() {
            self.hvsc_loading = false;
            match msg {
                HvscAsyncMsg::EntriesLoaded { path, entries } => {
                    self.hvsc_browser.current_path = path;
                    self.hvsc_browser.entries = entries;
                    self.hvsc_browser.selected = 0;
                }
                HvscAsyncMsg::SearchDone(results) => {
                    self.hvsc_search_results = results;
                    self.hvsc_search_selected = 0;
                    self.set_status(format!("Found {} tunes", self.hvsc_search_results.len()));
                }
                HvscAsyncMsg::SongDownloaded {
                    sid,
                    source,
                    subsong,
                } => {
                    // Playing a tune from the HVSC browser must not touch the
                    // playlist: only explicit user actions should add tracks.
                    // Keep the browser open as well; the Player tab is one click
                    // away when the user wants to watch it.
                    self.load_sid_direct(sid, Some(subsong), Some(source));
                }
                HvscAsyncMsg::Error(err) => {
                    self.set_status(format!("HVSC error: {err}"));
                }
            }
        }
    }

    /// Runs the startup check 30 seconds after launch, once per session.
    fn maybe_auto_check_updates(&mut self) {
        if self.update_checked_once || !self.config.check_updates {
            return;
        }
        let now = Instant::now();
        match self.update_auto_at {
            Some(at) if now >= at => {
                self.update_checked_once = true;
                self.start_update_check(false);
            }
            None => self.update_auto_at = Some(now + Duration::from_secs(30)),
            _ => {}
        }
    }

    /// Asks GitHub for the newest stable release on a worker thread. The UI
    /// stays responsive while the request runs.
    pub fn start_update_check(&mut self, manual: bool) {
        if self.update_busy() {
            return;
        }
        self.update_manual = manual;
        self.update_phase = UpdatePhase::Checking;
        self.update_release = None;
        // A check started by hand also satisfies the once-per-session startup
        // check, so the app does not ask GitHub twice on launch.
        self.update_checked_once = true;
        if manual {
            self.set_status("Checking for updates…");
        }

        let tx = self.update_async_tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(UpdateAsyncMsg::Checked(update::fetch_latest()));
        });
    }

    /// Downloads the asset of the release found by the check. The file is
    /// cached until the user presses Download, so the check alone costs only
    /// one small JSON request.
    pub fn start_update_download(&mut self) {
        let Some(release) = self.update_release.clone() else {
            return;
        };
        let Some(url) = release.download_url.clone() else {
            self.update_phase = UpdatePhase::DownloadFailed(
                "This release has no download for this platform.".to_string(),
            );
            self.show_update_dialog = true;
            return;
        };

        let dest = update::download_path(&release);
        let digest = release.digest.clone();
        self.update_cancel.store(false, Ordering::Relaxed);
        self.show_update_dialog = true;
        self.update_phase = UpdatePhase::Downloading {
            received: 0,
            total: release.download_size,
        };

        let cancel = Arc::clone(&self.update_cancel);
        let tx = self.update_async_tx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let result = update::download(&url, digest.as_deref(), &dest, &cancel, |received, total| {
                let _ = progress_tx.send(UpdateAsyncMsg::Progress { received, total });
            });
            match result {
                Ok(()) => {
                    let _ = tx.send(UpdateAsyncMsg::Downloaded {
                        release,
                        path: dest,
                    });
                }
                Err(error) if error == "cancelled" => {
                    let _ = tx.send(UpdateAsyncMsg::Cancelled);
                }
                Err(error) => {
                    let _ = tx.send(UpdateAsyncMsg::DownloadFailed(error));
                }
            }
        });
    }

    /// Asks the worker to stop. The phase stays `Cancelling` until the worker
    /// confirms, so a fresh download cannot race the old one.
    pub fn cancel_update_download(&mut self) {
        if matches!(self.update_phase, UpdatePhase::Downloading { .. }) {
            self.update_cancel.store(true, Ordering::Relaxed);
            self.update_phase = UpdatePhase::Cancelling;
        }
    }

    /// Applies messages from the update worker.
    fn handle_update_async(&mut self) {
        while let Ok(msg) = self.update_async_rx.try_recv() {
            match msg {
                UpdateAsyncMsg::Checked(Ok(Some(release))) => {
                    let newer =
                        update::is_newer(&release.version, env!("CARGO_PKG_VERSION"));
                    self.update_release = Some(release.clone());
                    if newer {
                        self.update_phase = UpdatePhase::Available;
                        self.show_update_dialog = true;
                        self.set_status(format!(
                            "SidCraft {} is available",
                            release.version
                        ));
                    } else {
                        self.update_phase = UpdatePhase::UpToDate;
                        if self.update_manual {
                            self.set_status(format!(
                                "SidCraft {} is up to date",
                                env!("CARGO_PKG_VERSION")
                            ));
                        }
                    }
                }
                UpdateAsyncMsg::Checked(Ok(None)) => {
                    self.update_release = None;
                    self.update_phase = UpdatePhase::UpToDate;
                    if self.update_manual {
                        self.set_status("No releases were found on GitHub");
                    }
                }
                UpdateAsyncMsg::Checked(Err(error)) => {
                    self.update_phase = UpdatePhase::CheckFailed(error.clone());
                    if self.update_manual {
                        self.set_status(format!("Update check failed: {error}"));
                    }
                }
                UpdateAsyncMsg::Progress { received, total } => {
                    if matches!(self.update_phase, UpdatePhase::Downloading { .. }) {
                        self.update_phase =
                            UpdatePhase::Downloading { received, total };
                    }
                }
                UpdateAsyncMsg::Downloaded { release, path } => {
                    if matches!(self.update_phase, UpdatePhase::Cancelling) {
                        // The user cancelled while the last chunk was arriving.
                        let _ = fs::remove_file(&path);
                    } else if matches!(self.update_phase, UpdatePhase::Downloading { .. }) {
                        self.update_release = Some(release);
                        self.update_phase = UpdatePhase::Ready(path);
                        self.show_update_dialog = true;
                        self.set_status("Update downloaded");
                    }
                }
                UpdateAsyncMsg::Cancelled => {
                    if matches!(
                        self.update_phase,
                        UpdatePhase::Downloading { .. } | UpdatePhase::Cancelling
                    ) {
                        self.update_phase = UpdatePhase::Available;
                        self.set_status("Download cancelled");
                    }
                }
                UpdateAsyncMsg::DownloadFailed(error) => {
                    if matches!(
                        self.update_phase,
                        UpdatePhase::Downloading { .. } | UpdatePhase::Cancelling
                    ) {
                        self.update_phase = UpdatePhase::DownloadFailed(error);
                        self.show_update_dialog = true;
                    }
                }
            }
        }
    }

    /// True while a check or download keeps the update flow busy.
    pub fn update_busy(&self) -> bool {
        matches!(
            self.update_phase,
            UpdatePhase::Checking
                | UpdatePhase::Downloading { .. }
                | UpdatePhase::Cancelling
        )
    }

    /// Status line for the update row in About and Settings.
    pub fn update_status_line(&self) -> Option<String> {
        match &self.update_phase {
            UpdatePhase::Idle => None,
            UpdatePhase::Checking => Some("Checking for updates…".to_string()),
            UpdatePhase::Available => self
                .update_release
                .as_ref()
                .map(|release| format!("SidCraft {} is available.", release.version)),
            UpdatePhase::Downloading { received, total } => {
                let mut line =
                    format!("Downloading update… {}", update::format_size(*received));
                if let Some(total) = total {
                    line.push_str(&format!(" of {}", update::format_size(*total)));
                }
                Some(line)
            }
            UpdatePhase::Cancelling => Some("Cancelling the download…".to_string()),
            UpdatePhase::Ready(path) => Some(format!(
                "Update downloaded to {}.",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
            UpdatePhase::UpToDate => Some(format!(
                "SidCraft {} is up to date.",
                env!("CARGO_PKG_VERSION")
            )),
            UpdatePhase::CheckFailed(error) => {
                Some(format!("Could not check for updates: {error}"))
            }
            UpdatePhase::DownloadFailed(error) => {
                Some(format!("Could not download the update: {error}"))
            }
        }
    }

    /// The About window only shows results that belong to a manual check;
    /// an automatic check stays quiet unless it found a new release.
    pub fn update_about_line(&self) -> Option<String> {
        if self.update_manual
            || matches!(
                self.update_phase,
                UpdatePhase::Checking
                    | UpdatePhase::Available
                    | UpdatePhase::Downloading { .. }
                    | UpdatePhase::Cancelling
                    | UpdatePhase::Ready(_)
            )
        {
            self.update_status_line()
        } else {
            None
        }
    }

    /// Replaces the running executable with the downloaded one and asks the
    /// helper to start the new version once this process exits. Windows and
    /// Linux only; macOS opens the DMG instead.
    pub fn install_update(&mut self, ctx: &egui::Context) {
        let UpdatePhase::Ready(path) = &self.update_phase else {
            return;
        };
        let path = path.clone();
        match update::install_downloaded(&path) {
            Ok(executable) => match update::relaunch_after_exit(&executable) {
                Ok(()) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(error) => {
                    self.update_phase = UpdatePhase::DownloadFailed(format!(
                        "The update was installed, but SidCraft could not restart: {error}"
                    ));
                    self.show_update_dialog = true;
                }
            },
            Err(error) => {
                self.update_phase = UpdatePhase::DownloadFailed(error);
                self.show_update_dialog = true;
            }
        }
    }

    pub fn current_elapsed(&self) -> Duration {
        if self.paused || self.seeking {
            self.song_elapsed
        } else {
            self.song_elapsed + self.song_resumed_at.elapsed()
        }
    }

    /// Seek to a fraction of the current song's length.
    ///
    /// The SID has no random access, so seeking restarts the current subsong and
    /// fast-forwards the emulator to the requested position. The jump runs on a
    /// worker thread: the emulator work can take a moment and blocking the UI
    /// thread there makes the whole window freeze.
    pub fn seek_to_fraction(&mut self, fraction: f32) {
        let total = self.song_timeout.as_secs_f32().max(1.0);
        let target = total * fraction.clamp(0.0, 1.0);
        let target_duration = Duration::from_secs_f32(target);
        let song = self.current_song;
        let was_paused = self.paused;

        // Abandon any seek still running and start a fresh generation.
        self.seek_cancel.store(true, Ordering::SeqCst);
        let cancel = Arc::new(AtomicBool::new(false));
        self.seek_cancel = Arc::clone(&cancel);
        let generation = self.seek_gen.fetch_add(1, Ordering::SeqCst) + 1;

        let player = Arc::clone(&self.player);
        let tx = self.seek_tx.clone();
        std::thread::spawn(move || {
            if !cancel.load(Ordering::SeqCst) {
                if let Ok(mut p) = player.lock() {
                    if !cancel.load(Ordering::SeqCst) && p.load_song(song).is_ok() {
                        p.fast_forward(target, &cancel);
                        // `load_song` clears the pause flag; carry the UI state
                        // across so seeking while paused stays silent.
                        if p.is_paused() != was_paused {
                            p.toggle_pause();
                        }
                    }
                }
            }
            let _ = tx.send(generation);
        });

        self.seeking = true;
        self.seek_target = target_duration;
        self.song_elapsed = target_duration;
        self.song_resumed_at = Instant::now();
        let secs = target as u64;
        self.set_status(format!("Seeking to {:02}:{:02}…", secs / 60, secs % 60));
    }

    /// Cancels a pending seek without waiting for it.
    fn cancel_seek(&mut self) {
        self.seek_cancel.store(true, Ordering::SeqCst);
        self.seeking = false;
    }

    fn handle_seek(&mut self) {
        while let Ok(generation) = self.seek_rx.try_recv() {
            if generation == self.seek_gen.load(Ordering::SeqCst) {
                // The emulator is now at the requested position: restart the
                // clock from there so the bar and the audio stay together.
                let target = self.seek_target;
                self.seek_target = Duration::ZERO;
                self.song_elapsed = target;
                self.song_resumed_at = Instant::now();
                self.seeking = false;
                let secs = target.as_secs();
                let stamp = format!("{:02}:{:02}", secs / 60, secs % 60);
                if self.paused {
                    self.set_status(format!("Paused at {stamp}"));
                } else {
                    self.set_status(format!("Playing from {stamp}"));
                }
            }
        }
    }

    pub fn playlist_display_name(&self, idx: usize) -> String {
        self.playlist
            .entries
            .get(idx)
            .map(|e| extract_filename(&e.source))
            .unwrap_or_default()
    }

    /// Reads the current window geometry in a way that works on every backend.
    ///
    /// winit cannot report window positions on Wayland (`inner_position()` and
    /// `outer_position()` both return `NotSupported`), so egui leaves
    /// `ViewportInfo::inner_rect` and `outer_rect` as `None` there. The size is
    /// still available through `InputState::screen_rect`, which eframe fills
    /// from `Window::inner_size()`. Using that as a fallback keeps size
    /// persistence working on Linux; the position stays unknown and is not saved.
    fn viewport_geometry(
        ctx: &egui::Context,
    ) -> (
        Option<egui::Vec2>,
        Option<egui::Pos2>,
        Option<bool>,
        Option<bool>,
    ) {
        ctx.input(|i| {
            let vp = i.viewport();
            let size = vp.inner_rect.map(|rect| rect.size()).or_else(|| {
                let screen = i.screen_rect;
                (screen.is_finite() && screen.width() > 1.0 && screen.height() > 1.0)
                    .then(|| screen.size())
            });
            (
                size,
                vp.outer_rect.map(|rect| rect.min),
                vp.maximized,
                vp.minimized,
            )
        })
    }

    /// Remembers the OS window geometry so the next launch reopens at the same
    /// size and position. Writes are throttled to once a second while the user
    /// is resizing and flushed once the geometry settles.
    pub fn remember_window(&mut self, ctx: &egui::Context) {
        let (size, pos, maximized, minimized) = Self::viewport_geometry(ctx);
        // While minimized the OS reports bogus bounds; keep the last good ones.
        if minimized == Some(true) {
            return;
        }

        // A mode switch changes decorations, size and position asynchronously.
        // Until the viewport matches what we requested, its bounds still
        // describe the previous mode; recording them would overwrite the saved
        // geometry with stale values (the window then "grows" next switch).
        let mut clear_pending = false;
        let ready = match self.pending_geometry.as_ref() {
            Some(pending) => {
                let size_ok = pending.inner_size.map_or(true, |want| {
                    size.is_some_and(|actual| (actual - want).abs().max_elem() < 2.0)
                });
                let pos_ok = pending.outer_pos.map_or(true, |want| {
                    pos.is_some_and(|actual| (actual - want).abs().max_elem() < 2.0)
                });
                let max_ok = pending
                    .maximized
                    .map_or(true, |want| maximized.is_some_and(|v| v == want));
                if (size_ok && pos_ok && max_ok)
                    || pending.at.elapsed() >= Duration::from_millis(1000)
                {
                    clear_pending = true;
                    true
                } else {
                    false
                }
            }
            None => true,
        };
        if clear_pending {
            self.pending_geometry = None;
        }
        if !ready {
            return;
        }

        let mut changed = false;

        if self.mini_mode {
            if let Some(size) = size {
                let (w, h) = (size.x, size.y);
                if self.config.mini_width.map_or(true, |v| (v - w).abs() > 0.5) {
                    self.config.mini_width = Some(w);
                    changed = true;
                }
                if self.config.mini_height.map_or(true, |v| (v - h).abs() > 0.5) {
                    self.config.mini_height = Some(h);
                    changed = true;
                }
            }
            if let Some(pos) = pos {
                let (x, y) = (pos.x, pos.y);
                if self.config.mini_x.map_or(true, |v| (v - x).abs() > 0.5) {
                    self.config.mini_x = Some(x);
                    changed = true;
                }
                if self.config.mini_y.map_or(true, |v| (v - y).abs() > 0.5) {
                    self.config.mini_y = Some(y);
                    changed = true;
                }
            }
        } else {
            if let Some(size) = size {
                let (w, h) = (size.x, size.y);
                if self
                    .config
                    .window_width
                    .map_or(true, |v| (v - w).abs() > 0.5)
                {
                    self.config.window_width = Some(w);
                    changed = true;
                }
                if self
                    .config
                    .window_height
                    .map_or(true, |v| (v - h).abs() > 0.5)
                {
                    self.config.window_height = Some(h);
                    changed = true;
                }
            }
            if let Some(pos) = pos {
                let (x, y) = (pos.x, pos.y);
                if self.config.window_x.map_or(true, |v| (v - x).abs() > 0.5) {
                    self.config.window_x = Some(x);
                    changed = true;
                }
                if self.config.window_y.map_or(true, |v| (v - y).abs() > 0.5) {
                    self.config.window_y = Some(y);
                    changed = true;
                }
            }
            if let Some(maximized) = maximized
                && maximized != self.config.window_maximized
            {
                self.config.window_maximized = maximized;
                changed = true;
            }
        }

        if changed {
            self.geometry_dirty = true;
        }
        if self.geometry_dirty
            && self
                .geometry_saved_at
                .map_or(true, |t| t.elapsed() >= Duration::from_secs(1))
        {
            self.config.save();
            self.geometry_dirty = false;
            self.geometry_saved_at = Some(Instant::now());
        }
    }

    /// Apply OS-level opacity where available. `255` means fully opaque.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn apply_window_alpha(&self, alpha: u8, previous: Option<u8>) {
        if let Some(handle) = &self.window_handle {
            window_alpha::set_alpha(handle, alpha, previous);
        }
    }

    /// Switches to the compact, frameless player. The current window size and
    /// position are saved so [`Self::exit_mini_mode`] can restore them.
    pub fn enter_mini_mode(&mut self, ctx: &egui::Context) {
        if self.mini_mode {
            return;
        }
        let (size, pos, maximized, _) = Self::viewport_geometry(ctx);
        // On the first frame an X11 window can still report (0, 0) before the
        // window manager places it. Prefer the last saved full-shell position
        // in that case so leaving the mini player does not jump to 0, 0.
        let full_pos = pos
            .filter(|p| p.x > 0.0 || p.y > 0.0)
            .or_else(|| match (self.config.window_x, self.config.window_y) {
                (Some(x), Some(y)) => Some(egui::pos2(x, y)),
                _ => None,
            })
            .unwrap_or(egui::pos2(80.0, 80.0));
        self.mini_restore = Some(MiniRestore {
            inner_size: size.unwrap_or(egui::vec2(MIN_WINDOW_SIZE[0], MIN_WINDOW_SIZE[1])),
            outer_pos: full_pos,
            maximized: maximized.unwrap_or(false),
        });
        self.mini_mode = true;

        let min_size = egui::vec2(MINI_MIN_SIZE[0], MINI_MIN_SIZE[1]);
        let max_size = egui::vec2(MINI_MAX_SIZE[0], MINI_MAX_SIZE[1]);
        let size = match (self.config.mini_width, self.config.mini_height) {
            (Some(w), Some(h)) => egui::vec2(
                w.clamp(MINI_MIN_SIZE[0], MINI_MAX_SIZE[0]),
                h.clamp(MINI_MIN_SIZE[1], MINI_MAX_SIZE[1]),
            ),
            _ => egui::vec2(MINI_DEFAULT_SIZE[0], MINI_DEFAULT_SIZE[1]),
        };
        if self.mini_restore.map_or(false, |restore| restore.maximized) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        // Remove the frame *before* setting InnerSize. On Windows a style
        // change preserves the outer rect, so resizing first would let the
        // freed title bar/border enlarge the frameless client area.
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        // The frameless player has no native title bar; restore transparency so
        // its opacity setting can still show the desktop through the window.
        #[cfg(target_os = "macos")]
        Self::set_macos_window_transparency(ctx, true);
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        // The full shell has a 760x520 minimum; lower it before resizing or
        // the OS clamps the mini window back to the large size. The upper
        // bound keeps the compact player from covering the desktop.
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_size));
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(max_size));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));

        // Reopen where the mini player was last left. `remember_window` waits
        // for both this position and the size above before saving again.
        // Wayland cannot report or set a global window position, so skip the
        // position command (and the wait for it) when the backend has none.
        let saved_pos = match (self.config.mini_x, self.config.mini_y) {
            (Some(x), Some(y)) if pos.is_some() => Some(egui::pos2(x, y)),
            _ => None,
        };
        if let Some(pos) = saved_pos {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        }
        self.pending_geometry = Some(PendingGeometry {
            inner_size: Some(size),
            outer_pos: saved_pos,
            maximized: Some(false),
            at: Instant::now(),
        });
    }

    /// Leaves the mini player and restores the full shell window.
    pub fn exit_mini_mode(&mut self, ctx: &egui::Context) {
        if !self.mini_mode {
            return;
        }
        self.mini_mode = false;
        let restore = self.mini_restore.take();
        // Keep geometry recording suspended until the full shell's bounds are
        // actually reported; otherwise the mini size gets saved as the full
        // window size (and the shell comes back at the wrong size). On Wayland
        // there is no outer position to wait for, so do not block on it.
        let pos_supported = Self::viewport_geometry(ctx).1.is_some();
        self.pending_geometry = Some(PendingGeometry {
            inner_size: restore.filter(|r| !r.maximized).map(|r| r.inner_size),
            outer_pos: restore
                .filter(|r| !r.maximized && pos_supported)
                .map(|r| r.outer_pos),
            maximized: restore.map(|r| r.maximized),
            at: Instant::now(),
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
        // The native title bar is back: make it opaque again (macOS only).
        #[cfg(target_os = "macos")]
        Self::set_macos_window_transparency(ctx, false);
        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
            MIN_WINDOW_SIZE[0],
            MIN_WINDOW_SIZE[1],
        )));
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(
            FULL_MAX_SIZE[0],
            FULL_MAX_SIZE[1],
        )));
        if let Some(restore) = restore {
            if restore.maximized {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(restore.inner_size));
                if pos_supported {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(restore.outer_pos));
                }
            }
        }
    }

    /// Total number of emulated voices (3 per SID chip).
    pub fn total_voices(&self) -> usize {
        self.chip_models.len() * 3
    }

    /// Kicks off a background export render.
    pub fn start_export(&mut self) {
        use engine::export::render_to_file;

        // Pick an output path
        let ext = self.export_settings.format.extension();
        let suggested = engine::export::suggested_filename(
            &self.sid_file,
            self.export_settings.subsong,
            self.export_settings.format,
        );
        let dialog = rfd::FileDialog::new()
            .add_filter(
                self.export_settings.format.label(),
                &[ext],
            )
            .set_file_name(&suggested);

        let dialog = if let Some(ref dir) = self.export_output_dir {
            dialog.set_directory(dir)
        } else {
            dialog
        };

        let Some(mut path) = dialog.save_file() else {
            return;
        };

        // Ensure the file has the extension matching the selected format.
        // Some save dialogs (or manual edits) may omit or keep a previous extension,
        // which would cause ffmpeg to pick the wrong container/format.
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            path.set_extension(ext);
        }

        // Remember the chosen directory for next time
        if let Some(parent) = path.parent() {
            self.export_output_dir = Some(parent.to_path_buf());
        }

        let total_samples =
            (self.export_settings.duration_secs * self.export_settings.sample_rate as f32) as u64;
        let progress = ExportProgress::new(total_samples);
        let progress_clone = progress.clone();

        self.export_last_path = Some(path.clone());
        self.export_progress = Some(progress);

        // Ensure voice_mutes has the right length
        let voice_count = self.total_voices();
        if self.export_settings.voice_mutes.len() != voice_count {
            self.export_settings.voice_mutes.resize(voice_count, false);
        }

        let sid_file = self.sid_file.clone();
        let settings = self.export_settings.clone();

        std::thread::spawn(move || {
            render_to_file(&sid_file, &settings, &path, &progress_clone);
        });

        self.set_status(format!(
            "Exporting {} to {}…",
            self.export_settings.format.label(),
            self.export_last_path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
        ));
    }
}

fn main() -> eframe::Result<()> {
    // Must run before the window is created so the task bar button picks up
    // the right identity and icon.
    app_icon::set_app_user_model_id();

    // On macOS the file from Finder arrives as an AppleEvent after launch;
    // the other platforms pass it on the command line.
    startup::install_open_handler();
    let startup_path = startup::from_args();

    // Opening another file while SidCraft runs hands it to the existing
    // window instead of starting a second instance and a second audio stream.
    if startup::hand_over_to_running_instance(startup_path.as_deref()) {
        return Ok(());
    }

    // A Windows self-update renames the old executable and leaves it behind;
    // remove it now that the new build is running.
    update::cleanup_previous_install();

    let config = Config::load();
    // winit selects Wayland whenever WAYLAND_DISPLAY is present, even when an
    // X server is available. Wayland gives applications no way to read or set
    // a window's global position, so the full window and the mini player
    // cannot have their positions remembered there. Let Linux users opt into
    // X11 (XWayland works too) to get separate position persistence.
    #[cfg(target_os = "linux")]
    if config.prefer_x11 {
        // SAFETY: this runs before the event loop, any window, or any spawned
        // thread can read the environment.
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("SidCraft")
		// On Wayland the app id is what ties the window to `sidcraft.desktop`,
        // which is how the desktop environment finds the icon. On X11 it
        // becomes the WM_CLASS (`StartupWMClass` in the desktop file).
        .with_app_id("sidcraft")
        .with_inner_size([
            config.window_width.unwrap_or(1180.0),
            config.window_height.unwrap_or(760.0),
        ])
        .with_min_inner_size(MIN_WINDOW_SIZE)
        // Required for the mini player's opacity setting: the OpenGL surface
        // needs an alpha channel. The full shell still clears to opaque, and on
        // macOS its native title bar is made opaque again after startup.
        .with_transparent(true);
    if let (Some(x), Some(y)) = (config.window_x, config.window_y) {
        viewport = viewport.with_position([x, y]);
    }
    if config.window_maximized {
        viewport = viewport.with_maximized(true);
    }
    // Window icon: title bar and the task bar button. On Windows the same
    // artwork is also compiled into the .exe by `build.rs`, which is what
    // Explorer and Alt+Tab show.
    if let Some(icon) = app_icon::icon_data() {
        viewport = viewport.with_icon(icon);
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "SidCraft",
        native_options,
        Box::new(move |cc| Ok(Box::new(SidcraftApp::new(cc, startup_path)))),
    )
}
