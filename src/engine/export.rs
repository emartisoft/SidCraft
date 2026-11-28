// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Offline renderer: drives the SID emulator headlessly and writes the
// resulting audio to WAV, FLAC or MP3. WAV is written in pure Rust (via hound);
// FLAC and MP3 encoding delegates to a system `ffmpeg` process.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::engine::player::{Player, SamplingMethod};
use crate::engine::sid_file::SidFile;

/// Export format chosen by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Wav,
    Flac,
    Mp3,
}

impl ExportFormat {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Mp3 => "MP3",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
        }
    }
}

/// Per-voice gain override for export (0.0 = mute, 1.0 = full).
#[derive(Clone, Debug)]
pub struct ExportSettings {
    /// Target audio format.
    pub format: ExportFormat,
    /// Duration to render in seconds.
    pub duration_secs: f32,
    /// Fade-out duration at the end (seconds). 0 = no fade.
    pub fade_out_secs: f32,
    /// Output sample rate (usually matches the live rate).
    pub sample_rate: u32,
    /// Master output gain, 0.0–1.0.
    pub gain: f32,
    /// Per-voice mute overrides (true = mute). Indexed chip*3+voice.
    pub voice_mutes: Vec<bool>,
    /// MP3 bit-rate kbps (ignored for WAV/FLAC).
    pub mp3_bitrate_kbps: u32,
    /// Subsong to export.
    pub subsong: u16,
    /// Whether to use the EKV transistor filter.
    pub ekv_filter: bool,
    /// SID chip model override index (0 = SID1, etc.)
    pub chip_model_override: bool,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            format: ExportFormat::Wav,
            duration_secs: 180.0,
            fade_out_secs: 3.0,
            sample_rate: 44100,
            gain: 1.0,
            voice_mutes: Vec::new(),
            mp3_bitrate_kbps: 192,
            subsong: 1,
            ekv_filter: false,
            chip_model_override: false,
        }
    }
}

/// Progress token shared between the render thread and the UI.
#[derive(Clone)]
pub struct ExportProgress {
    /// Rendered samples so far.
    pub rendered: Arc<AtomicU64>,
    /// Total samples to render.
    pub total: u64,
    /// Set to `true` by the UI thread to cancel the export.
    pub cancel: Arc<AtomicBool>,
    /// Set to `true` by the render thread when done.
    pub done: Arc<AtomicBool>,
    /// Error message (empty = no error).
    pub error: Arc<std::sync::Mutex<Option<String>>>,
}

impl ExportProgress {
    pub fn new(total_samples: u64) -> Self {
        Self {
            rendered: Arc::new(AtomicU64::new(0)),
            total: total_samples,
            cancel: Arc::new(AtomicBool::new(false)),
            done: Arc::new(AtomicBool::new(false)),
            error: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            return 1.0;
        }
        (self.rendered.load(Ordering::Relaxed) as f32 / self.total as f32).clamp(0.0, 1.0)
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn error_message(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }
}

/// Renders `sid_file` with `settings` to `output_path`, reporting progress
/// through `progress`. Runs synchronously; call from a worker thread.
pub fn render_to_file(
    sid_file: &SidFile,
    settings: &ExportSettings,
    output_path: &Path,
    progress: &ExportProgress,
) {
    let result = match settings.format {
        ExportFormat::Wav => render_wav(sid_file, settings, output_path, progress),
        ExportFormat::Flac => render_flac(sid_file, settings, output_path, progress),
        ExportFormat::Mp3 => render_mp3(sid_file, settings, output_path, progress),
    };
    if let Err(e) = result {
        if let Ok(mut guard) = progress.error.lock() {
            *guard = Some(e.to_string());
        }
    }
    progress.done.store(true, Ordering::Relaxed);
}

// ── internal helpers ────────────────────────────────────────────────────────

fn build_player(sid_file: &SidFile, settings: &ExportSettings) -> Result<Player, String> {
    Player::new(
        sid_file,
        settings.subsong,
        settings.sample_rate,
        None,
        SamplingMethod::ResampleTwoPass,
    )
    .map_err(|e| e.to_string())
    .map(|mut p| {
        if settings.ekv_filter {
            p.set_ekv_filter(true);
        }
        for (i, &muted) in settings.voice_mutes.iter().enumerate() {
            p.set_voice_muted(i, muted);
        }
        p
    })
}

/// Fills `buf` with one chunk of rendered audio, applying gain and fade-out.
fn fill_chunk(
    player: &mut Player,
    buf: &mut Vec<f32>,
    chunk_size: usize,
    rendered: u64,
    total: u64,
    fade_out_samples: u64,
    gain: f32,
) {
    buf.resize(chunk_size, 0.0);
    player.fill_buffer(buf);
    let fade_start = total.saturating_sub(fade_out_samples);
    for (i, s) in buf.iter_mut().enumerate() {
        let pos = rendered + i as u64;
        let fade = if fade_out_samples > 0 && pos >= fade_start {
            let t = (pos - fade_start) as f32 / fade_out_samples as f32;
            1.0 - t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        *s *= gain * fade;
    }
}

/// ffmpeg'i bulur: önce FFMPEG_PATH, sonra etkin PATH, sonra platforma özgü
/// bilinen konumlar (Windows kayıt defteri / macOS ve Linux'ta yaygın dizinler).
fn find_ffmpeg() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FFMPEG_PATH") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }

    let path_dirs = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(path) = find_ffmpeg_in_dirs(path_dirs) {
        return Some(path);
    }

    #[cfg(windows)]
    if let Some(ffmpeg) = find_ffmpeg_in_dirs(windows_registry_path_dirs()) {
        return Some(ffmpeg);
    }

    #[cfg(not(windows))]
    if let Some(ffmpeg) = find_ffmpeg_in_dirs(unix_fallback_ffmpeg_dirs()) {
        return Some(ffmpeg);
    }

    None
}

/// Builds a `Command` that suppresses the transient console window when a
/// Windows GUI process spawns a console child (`ffmpeg.exe`, `reg.exe`, etc.).
fn command_no_window(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = Command::new(program);
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    }

    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}

fn find_ffmpeg_in_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    #[cfg(windows)]
    const EXECUTABLE: &str = "ffmpeg.exe";
    #[cfg(not(windows))]
    const EXECUTABLE: &str = "ffmpeg";

    dirs.into_iter().find_map(|dir| {
        let candidate = dir.join(EXECUTABLE);
        if candidate.is_file()
            && command_no_window(&candidate)
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        {
            Some(candidate)
        } else {
            None
        }
    })
}

#[cfg(windows)]
fn windows_registry_path_dirs() -> Vec<PathBuf> {
    const PATH_KEYS: [&str; 2] = [
        r"HKCU\Environment",
        r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
    ];

    PATH_KEYS
        .iter()
        .filter_map(|key| {
            let output = command_no_window("reg.exe")
                .args(["query", key, "/v", "Path"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }

            let text = String::from_utf8_lossy(&output.stdout);
            let value = text.lines().find_map(parse_registry_path_line)?;
            let expanded = expand_environment_variables(value);
            Some(std::env::split_paths(std::ffi::OsStr::new(&expanded)).collect::<Vec<_>>())
        })
        .flatten()
        .collect()
}

#[cfg(windows)]
fn parse_registry_path_line(line: &str) -> Option<&str> {
    let (name, rest) = line.trim().split_once(char::is_whitespace)?;
    if !name.eq_ignore_ascii_case("Path") {
        return None;
    }
    let (_, value) = rest.trim_start().split_once(char::is_whitespace)?;
    Some(value.trim())
}

#[cfg(windows)]
fn expand_environment_variables(value: &str) -> String {
    let mut expanded = String::new();
    let mut remaining = value;
    while let Some(start) = remaining.find('%') {
        expanded.push_str(&remaining[..start]);
        let after_start = &remaining[start + 1..];
        let Some(end) = after_start.find('%') else {
            expanded.push_str(&remaining[start..]);
            return expanded;
        };
        let variable = &after_start[..end];
        if let Some(value) = std::env::var_os(variable) {
            expanded.push_str(&value.to_string_lossy());
        } else {
            expanded.push('%');
            expanded.push_str(variable);
            expanded.push('%');
        }
        remaining = &after_start[end + 1..];
    }
    expanded.push_str(remaining);
    expanded
}

/// macOS/Linux'ta PATH'te görünmeyen yaygın kurulum dizinleri.
/// GUI uygulamaları Finder/Dock üzerinden başlatıldığında sınırlı bir PATH
/// miras aldığı için Homebrew vb. konumları ayrıca yokluyoruz.
#[cfg(not(windows))]
fn unix_fallback_ffmpeg_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"), // Homebrew (Apple Silicon)
        PathBuf::from("/usr/local/bin"),    // Homebrew (Intel) / elle kurulum
        PathBuf::from("/opt/local/bin"),    // MacPorts
        PathBuf::from("/snap/bin"),         // Snap (Linux)
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
    }
    dirs
}

/// Ara WAV'ı verilen codec argümanlarıyla hedef dosyaya çevirir.
/// Hata olursa ffmpeg'in stderr çıktısını hata mesajına koyar.
fn ffmpeg_convert(input: &Path, output: &Path, codec_args: &[&str]) -> io::Result<()> {
    let ffmpeg = find_ffmpeg().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "ffmpeg bulunamadı. FFMPEG_PATH'i ffmpeg.exe'nin tam yoluna ayarlayın veya PATH'e ekleyip SidCraft'ı yeniden başlatın.",
        )
    })?;

    let out = command_no_window(&ffmpeg)
        .args([
            "-y",
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
        ])
        .arg(input)
        .args(codec_args)
        .arg(output)
        .output()?;

    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "ffmpeg dönüşüm hatası ({}): {}",
                ffmpeg.display(),
                if stderr.trim().is_empty() {
                    format!("çıkış kodu {:?}", out.status.code())
                } else {
                    stderr.trim().to_string()
                }
            ),
        ));
    }

    // ffmpeg bazı durumlarda exit code 0 döndürse bile çıktı üretmeyebilir.
    // Bu durumda sessiz "tamamlandı" göstermek yerine hata ver.
    match std::fs::metadata(output) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "ffmpeg çıktı dosyası boş: {} ({} ){}",
                output.display(),
                ffmpeg.display(),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" ({})", stderr.trim())
                }
            ),
        )),
        Err(e) => Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "ffmpeg çıktı dosyası oluşturulamadı: {} ({}, ffmpeg: {}){}",
                output.display(),
                e,
                ffmpeg.display(),
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(" | ffmpeg: {}", stderr.trim())
                }
            ),
        )),
    }
}

/// WAV'ı geçici olarak yazar, ffmpeg ile dönüştürür, geçici dosyayı siler.
fn render_via_ffmpeg(
    sid_file: &SidFile,
    settings: &ExportSettings,
    path: &Path,
    progress: &ExportProgress,
    codec_args: &[&str],
) -> io::Result<()> {
    let tmp = path.with_extension("tmp_export.wav");
    render_wav_raw(sid_file, settings, &tmp, progress)?;

    if progress.is_cancelled() {
        let _ = std::fs::remove_file(&tmp);
        return Ok(());
    }

    let result = ffmpeg_convert(&tmp, path, codec_args);
    let _ = std::fs::remove_file(&tmp);
    if result.is_err() {
        // Hatalı/yarım kalmış çıktı dosyasını bırakma.
        let _ = std::fs::remove_file(path);
    }
    result
}

fn render_flac(
    sid_file: &SidFile,
    settings: &ExportSettings,
    path: &Path,
    progress: &ExportProgress,
) -> io::Result<()> {
    render_via_ffmpeg(sid_file, settings, path, progress, &["-c:a", "flac"])
}

fn render_mp3(
    sid_file: &SidFile,
    settings: &ExportSettings,
    path: &Path,
    progress: &ExportProgress,
) -> io::Result<()> {
    let bitrate = format!("{}k", settings.mp3_bitrate_kbps);
    render_via_ffmpeg(
        sid_file,
        settings,
        path,
        progress,
        &["-c:a", "libmp3lame", "-b:a", &bitrate],
    )
}

fn render_wav(
    sid_file: &SidFile,
    settings: &ExportSettings,
    path: &Path,
    progress: &ExportProgress,
) -> io::Result<()> {
    render_wav_raw(sid_file, settings, path, progress)
}

/// WAV'ı doğrudan hedef dosyaya yazar. Hem WAV çıktısı hem de ffmpeg girdisi için kullanılır.
fn render_wav_raw(
    sid_file: &SidFile,
    settings: &ExportSettings,
    path: &Path,
    progress: &ExportProgress,
) -> io::Result<()> {
    let mut player = build_player(sid_file, settings)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: settings.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let total = (settings.duration_secs * settings.sample_rate as f32) as u64;
    let fade_out = (settings.fade_out_secs * settings.sample_rate as f32) as u64;
    const CHUNK: usize = 4096;
    let mut buf = Vec::with_capacity(CHUNK);
    let mut rendered = 0u64;

    while rendered < total {
        if progress.is_cancelled() {
            drop(writer);
            let _ = std::fs::remove_file(path);
            return Ok(());
        }
        let chunk = CHUNK.min((total - rendered) as usize);
        fill_chunk(
            &mut player,
            &mut buf,
            chunk,
            rendered,
            total,
            fade_out,
            settings.gain,
        );
        for &s in &buf[..chunk] {
            #[allow(clippy::cast_possible_truncation)]
            let sample = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
            writer
                .write_sample(sample)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        }
        rendered += chunk as u64;
        progress.rendered.store(rendered, Ordering::Relaxed);
    }

    writer
        .finalize()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
}

/// Returns a suggested file name stem for the export, derived from the SID metadata.
pub fn suggested_filename(sid_file: &SidFile, subsong: u16, format: ExportFormat) -> String {
    let name = if sid_file.name.is_empty() {
        "export".to_string()
    } else {
        // Sanitize: replace characters that are problematic in file names.
        sid_file
            .name
            .chars()
            .map(|c| match c {
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect()
    };
    if sid_file.songs > 1 {
        format!("{name} - {subsong:02}.{}", format.extension())
    } else {
        format!("{name}.{}", format.extension())
    }
}

/// Converts a path with a user-chosen directory and an auto-suggested filename.
#[allow(dead_code)]
pub fn default_export_path(
    directory: Option<&Path>,
    sid_file: &SidFile,
    subsong: u16,
    format: ExportFormat,
) -> PathBuf {
    let filename = suggested_filename(sid_file, subsong, format);
    match directory {
        Some(dir) => dir.join(filename),
        None => PathBuf::from(filename),
    }
}
