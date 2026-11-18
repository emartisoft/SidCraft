// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

//! GitHub release checks, update downloads and self-installation.
//!
//! The check only asks for the public release list and never downloads a file
//! until the user presses **Download** in the update window. Downloads are
//! written next to the user's other downloads, verified against the SHA-256
//! digest GitHub publishes for the asset, and then installed where the platform
//! allows it. macOS keeps the DMG for the user to open manually: an app bundle
//! cannot reliably replace itself while it is running.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;

/// Public releases, newest first. Stable releases only; pre-releases are for
/// testers and are deliberately ignored by the update check.
const RELEASES_URL: &str =
    "https://api.github.com/repos/emartisoft/SidCraft/releases?per_page=10";

const USER_AGENT: &str = concat!("SidCraft/", env!("CARGO_PKG_VERSION"));

/// Size of one read from the HTTP body.
const DOWNLOAD_CHUNK: u64 = 64 * 1024;

/// Send a progress message at most every this many bytes.
const PROGRESS_STEP: u64 = 128 * 1024;

/// Release notes longer than this are cut; the release page has the rest.
const MAX_NOTES: usize = 4000;

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    /// Version without the `v` prefix, for example `0.1.2`.
    pub version: String,
    /// Original Git tag, for example `v0.1.2`.
    pub tag: String,
    /// GitHub release page.
    pub page_url: String,
    /// Release notes as plain text.
    pub notes: String,
    /// Asset for this OS, when the release has one.
    pub download_url: Option<String>,
    pub download_name: Option<String>,
    pub download_size: Option<u64>,
    /// `sha256:...` digest reported by the GitHub API.
    pub digest: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

/// Fetches the newest stable release. `Ok(None)` means GitHub has no stable
/// release yet; that is not an error.
pub fn fetch_latest() -> Result<Option<ReleaseInfo>, String> {
    let agent = agent(Duration::from_secs(12));
    let response = agent
        .get(RELEASES_URL)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(http_error)?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|error| format!("Could not read GitHub's response: {error}"))?;
    let releases: Vec<GhRelease> = serde_json::from_str(&body)
        .map_err(|error| format!("Could not parse GitHub's response: {error}"))?;

    let mut best: Option<(ReleaseInfo, [u64; 3])> = None;
    for release in releases {
        if release.draft || release.prerelease {
            continue;
        }
        let version = normalize_version(&release.tag_name);
        let Some(parts) = version_parts(&version) else {
            continue;
        };
        let asset = pick_asset(&release.assets);
        let info = ReleaseInfo {
            version,
            tag: release.tag_name,
            page_url: release.html_url,
            notes: truncate_notes(release.body.unwrap_or_default()),
            download_url: asset.map(|asset| asset.browser_download_url.clone()),
            download_name: asset.map(|asset| asset.name.clone()),
            download_size: asset.map(|asset| asset.size),
            digest: asset.and_then(|asset| asset.digest.clone()),
        };
        let replace = best
            .as_ref()
            .map_or(true, |(_, current)| parts > *current);
        if replace {
            best = Some((info, parts));
        }
    }

    Ok(best.map(|(release, _)| release))
}

/// Compares two version strings numerically, ignoring a leading `v` and any
/// pre-release suffix.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (
        version_parts(&normalize_version(latest)),
        version_parts(&normalize_version(current)),
    ) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// Downloads `url` to `dest`, verifies the SHA-256 digest when GitHub provides
/// one, and reports progress as `(received, total)`. A cancelled download is
/// reported as `Err("cancelled")` and leaves no partial file behind.
pub fn download<F>(
    url: &str,
    digest: Option<&str>,
    dest: &Path,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64, Option<u64>),
{
    let parent = dest
        .parent()
        .ok_or_else(|| "The download has no target folder.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create the download folder: {error}"))?;

    let part = part_path(dest);
    let _ = fs::remove_file(&part);

    let result = (|| {
        let agent = agent(Duration::from_secs(600));
        let response = agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(http_error)?;
        let total = response.body().content_length();
        let mut reader = response.into_body().into_reader();
        let mut file = File::create(&part)
            .map_err(|error| format!("Could not create the download file: {error}"))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        let mut last_report = 0u64;
        let mut buffer = vec![0u8; DOWNLOAD_CHUNK as usize];

        loop {
            if cancel.load(AtomicOrdering::Relaxed) {
                return Err("cancelled".to_string());
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("The download stopped: {error}"))?;
            if read == 0 {
                break;
            }
            file.write_all(&buffer[..read])
                .map_err(|error| format!("Could not save the download: {error}"))?;
            hasher.update(&buffer[..read]);
            received += read as u64;
            if received - last_report >= PROGRESS_STEP || total == Some(received) {
                on_progress(received, total);
                last_report = received;
            }
        }

        file.flush()
            .map_err(|error| format!("Could not save the download: {error}"))?;
        if let Some(total) = total
            && received != total
        {
            return Err(format!(
                "The download ended early ({received} of {total} bytes)."
            ));
        }
        if let Some(expected) = digest.and_then(|value| value.strip_prefix("sha256:")) {
            let actual: String = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if !actual.eq_ignore_ascii_case(expected) {
                return Err("The downloaded file failed its checksum check.".to_string());
            }
        }

        on_progress(received, total);
        fs::rename(&part, dest)
            .map_err(|error| format!("Could not save the download: {error}"))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Where the asset should be saved. Existing files are not overwritten; a
/// numbered name is used instead.
pub fn download_path(release: &ReleaseInfo) -> PathBuf {
    let folder = dirs::download_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Downloads")))
        .unwrap_or_else(std::env::temp_dir);
    let name = release
        .download_name
        .as_deref()
        .map(sanitize_file_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("SidCraft-{}", release.version));
    unique_path(&folder, &name)
}

/// True when the downloaded file can be installed and SidCraft restarted
/// without leaving the app. macOS uses the DMG instead.
pub const fn can_install_in_place() -> bool {
    cfg!(any(windows, target_os = "linux"))
}

/// Installs a downloaded update and returns the executable to start after this
/// process exits. Only supported on Windows and Linux.
pub fn install_downloaded(downloaded: &Path) -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        install_windows(downloaded)
    }
    #[cfg(target_os = "linux")]
    {
        install_linux(downloaded)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = downloaded;
        Err("Automatic installation is not supported on this platform.".to_string())
    }
}

/// Removes the previous executable left behind by a Windows self-update.
pub fn cleanup_previous_install() {
    #[cfg(windows)]
    {
        let Ok(executable) = std::env::current_exe() else {
            return;
        };
        let Some(folder) = executable.parent() else {
            return;
        };
        let Some(name) = executable.file_name().and_then(|name| name.to_str()) else {
            return;
        };
        let prefix = format!("{name}.old-");
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let is_old = entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix));
            if is_old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Starts `executable` after this process has exited. A tiny detached helper
/// waits for the current PID, which avoids racing the single-instance socket.
pub fn relaunch_after_exit(executable: &Path) -> Result<(), String> {
    let pid = std::process::id();

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        // CREATE_NO_WINDOW: the helper must not flash a console window.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let path = executable.display().to_string().replace('\'', "''");
        let script = format!(
            "while (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; Start-Process -FilePath '{path}'"
        );
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::{Command, Stdio};

        let script = "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"";
        Command::new("/bin/sh")
            .args([
                "-c",
                script,
                "sidcraft-update",
                &pid.to_string(),
                &executable.to_string_lossy(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (pid, executable);
        return Err("Automatic restart is not supported on this platform.".to_string());
    }

    Ok(())
}

/// Opens a file or folder with the platform's default handler.
pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Reveals a downloaded file in the platform's file manager.
pub fn reveal_path(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let folder = path.parent().unwrap_or(path);
        std::process::Command::new("explorer")
            .arg(folder)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R"])
            .arg(path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let folder = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(folder)
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Human-readable file size for the download progress line.
pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_per_read(Some(Duration::from_secs(30)))
            .https_only(true)
            .build(),
    )
}

fn http_error(error: ureq::Error) -> String {
    match &error {
        ureq::Error::StatusCode(403 | 429) => {
            "GitHub's request limit was reached. Try again later.".to_string()
        }
        ureq::Error::StatusCode(404) => "No releases were found on GitHub.".to_string(),
        ureq::Error::StatusCode(code) => format!("GitHub returned HTTP {code}."),
        ureq::Error::Timeout(_) => "The request timed out.".to_string(),
        _ => error.to_string(),
    }
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches(['v', 'V']).to_string()
}

fn version_parts(version: &str) -> Option<[u64; 3]> {
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts = [0u64; 3];
    let mut found = false;
    for (index, part) in core.split('.').take(3).enumerate() {
        let digits: String = part
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            return None;
        }
        parts[index] = digits.parse().ok()?;
        found = true;
    }
    found.then_some(parts)
}

fn pick_asset(assets: &[GhAsset]) -> Option<&GhAsset> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    assets
        .iter()
        .filter_map(|asset| asset_rank(&asset.name, os).map(|rank| (asset, rank)))
        .min_by_key(|(asset, rank)| {
            let arch_penalty = if asset.name.to_ascii_lowercase().contains(arch) {
                0
            } else {
                10
            };
            *rank + arch_penalty
        })
        .map(|(asset, _)| asset)
}

fn asset_rank(name: &str, os: &str) -> Option<u8> {
    let name = name.to_ascii_lowercase();
    match os {
        "windows" => {
            if name.contains("windows") && name.ends_with(".zip") {
                Some(0)
            } else if name.contains("windows") && name.ends_with(".exe") {
                Some(1)
            } else {
                None
            }
        }
        "macos" => {
            let mac = name.contains("macos")
                || name.contains("darwin")
                || name.contains("osx");
            if mac && name.ends_with(".dmg") {
                Some(0)
            } else if mac && name.ends_with(".zip") {
                Some(1)
            } else {
                None
            }
        }
        _ => {
            if name.contains("linux") && name.ends_with(".appimage") {
                Some(0)
            } else if name.contains("linux") && name.ends_with(".tar.gz") {
                Some(1)
            } else if name.contains("linux") && name.ends_with(".zip") {
                Some(2)
            } else {
                None
            }
        }
    }
}

fn truncate_notes(notes: String) -> String {
    let notes = notes.trim();
    if notes.chars().count() <= MAX_NOTES {
        return notes.to_string();
    }
    let mut truncated: String = notes.chars().take(MAX_NOTES).collect();
    truncated.push('…');
    truncated
}

fn sanitize_file_name(name: &str) -> String {
    name.rsplit(|character| character == '/' || character == '\\')
        .next()
        .unwrap_or(name)
        .chars()
        .map(|character| if character.is_control() { '_' } else { character })
        .collect::<String>()
        .trim()
        .to_string()
}

fn unique_path(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (name, String::new()),
    };
    for index in 1..1000 {
        let candidate = folder.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    folder.join(format!("{stem}-{}{extension}", std::process::id()))
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

#[cfg(windows)]
fn install_windows(downloaded: &Path) -> Result<PathBuf, String> {
    let executable =
        std::env::current_exe().map_err(|error| format!("Could not find SidCraft: {error}"))?;
    let is_exe = downloaded
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    let staged = if is_exe {
        downloaded.to_path_buf()
    } else {
        extract_windows_exe(downloaded)?
    };

    let old_name = format!(
        "{}.old-{}",
        executable
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        std::process::id()
    );
    let old = executable.with_file_name(old_name);
    fs::rename(&executable, &old).map_err(|error| {
        format!(
            "Could not replace the running SidCraft executable (is it in a read-only folder?): {error}"
        )
    })?;
    if let Err(error) = fs::copy(&staged, &executable) {
        let _ = fs::rename(&old, &executable);
        return Err(format!("Could not write the new SidCraft executable: {error}"));
    }

    let _ = fs::remove_file(&staged);
    let _ = fs::remove_file(downloaded);
    Ok(executable)
}

#[cfg(windows)]
fn extract_windows_exe(zip_path: &Path) -> Result<PathBuf, String> {
    let file = File::open(zip_path)
        .map_err(|error| format!("Could not open the update archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Could not read the update archive: {error}"))?;
    let index = (0..archive.len())
        .find(|&index| {
            archive.by_index(index).is_ok_and(|entry| {
                !entry.is_dir()
                    && entry
                        .name()
                        .ok()
                        .and_then(|name| {
                            name.rsplit(|character| character == '/' || character == '\\')
                                .next()
                                .map(|name| name.eq_ignore_ascii_case("sidcraft.exe"))
                        })
                        .unwrap_or(false)
            })
        })
        .ok_or_else(|| "The update archive does not contain sidcraft.exe.".to_string())?;

    let mut entry = archive
        .by_index(index)
        .map_err(|error| format!("Could not read sidcraft.exe: {error}"))?;
    let staged = std::env::temp_dir().join(format!(
        "sidcraft-update-{}.exe",
        std::process::id()
    ));
    let mut output = File::create(&staged)
        .map_err(|error| format!("Could not create the new executable: {error}"))?;
    std::io::copy(&mut entry, &mut output)
        .map_err(|error| format!("Could not extract sidcraft.exe: {error}"))?;
    output
        .flush()
        .map_err(|error| format!("Could not extract sidcraft.exe: {error}"))?;
    Ok(staged)
}

#[cfg(target_os = "linux")]
fn install_linux(downloaded: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;

    let is_appimage = downloaded
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("appimage"));
    if !is_appimage {
        return Err(
            "The downloaded file is not an AppImage; install it manually from your Downloads folder."
                .to_string(),
        );
    }

    let appimage = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "SidCraft is not running from an AppImage, so it cannot replace itself.".to_string()
        })?;
    let appimage = fs::canonicalize(&appimage).unwrap_or(appimage);
    let folder = appimage
        .parent()
        .ok_or_else(|| "Could not find the AppImage folder.".to_string())?;
    let name = appimage
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let staged = folder.join(format!(".{name}.new-{}", std::process::id()));

    fs::copy(downloaded, &staged)
        .map_err(|error| format!("Could not write next to the AppImage: {error}"))?;
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("Could not make the new AppImage executable: {error}"))?;
    if let Err(error) = fs::rename(&staged, &appimage) {
        let _ = fs::remove_file(&staged);
        return Err(format!(
            "Could not replace the AppImage (is it in a read-only folder?): {error}"
        ));
    }

    let _ = fs::remove_file(downloaded);
    Ok(appimage)
}
