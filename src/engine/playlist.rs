// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

use crate::engine::hvsc::{self, DEFAULT_HVSC_URL, mirror_resource_url};
use crate::engine::sid_file::SidFile;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A single entry in a playlist, representing a SID tune source.
#[derive(Debug, Clone)]
pub struct PlaylistEntry {
    /// Original source (file path or URL)
    pub source: String,
    /// Display name (filename without path)
    pub display_name: String,
    /// Optional subsong override (1-indexed)
    pub subsong: Option<u16>,
}

impl PlaylistEntry {
    /// Creates a new entry, extracting display name and optional subsong.
    fn new(source: &str) -> Option<Self> {
        let trimmed = source.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        let (path_part, subsong) = parse_subsong(trimmed);
        let display_name = extract_filename(path_part);

        Some(Self {
            source: path_part.to_string(),
            display_name,
            subsong,
        })
    }

    /// Returns true if this entry is an http(s) or `file://` URL.
    pub fn is_url(&self) -> bool {
        self.source.starts_with("http://")
            || self.source.starts_with("https://")
            || self.source.starts_with("file://")
    }

    /// Loads the SID file from this entry's source, using the shipped mirror
    /// for legacy HVSC-relative paths.
    #[allow(dead_code)] // convenience wrapper for callers without a selected mirror
    pub fn load(&self) -> io::Result<SidFile> {
        self.load_with_mirror(DEFAULT_HVSC_URL)
    }

    /// Loads the SID file from this entry's source.
    ///
    /// Entries saved by older versions (and hand-written m3u files) may hold an
    /// HVSC-relative path such as `/MUSICIANS/E/Emarti/Tune.sid`. Those are not
    /// local files, so a failed local load falls back to the selected mirror.
    pub fn load_with_mirror(&self, mirror_location: &str) -> io::Result<SidFile> {
        SidFile::parse(&fetch_sid_bytes_with_fallback(
            &self.source,
            mirror_location,
        )?)
    }
}

/// Fetches the original SID file bytes for a playlist source.
///
/// Uses the built-in mirror for legacy HVSC-relative paths; callers that know
/// the selected mirror should prefer [`fetch_sid_bytes_with_fallback`].
#[allow(dead_code)] // convenience wrapper for callers without a selected mirror
pub fn fetch_sid_bytes(source: &str) -> io::Result<Vec<u8>> {
    fetch_sid_bytes_with_fallback(source, DEFAULT_HVSC_URL)
}

/// Fetches the original SID file bytes for a playlist source.
///
/// Supports local paths, `file://` URLs, http(s) URLs and the HVSC-relative
/// paths used by older playlists. Legacy paths are resolved against
/// `mirror_location`, which may be a remote mirror or a local folder root.
pub fn fetch_sid_bytes_with_fallback(source: &str, mirror_location: &str) -> io::Result<Vec<u8>> {
    if source.starts_with("http://")
        || source.starts_with("https://")
        || source.starts_with("file://")
    {
        return hvsc::fetch_bytes(source);
    }
    match fs::read(source) {
        Ok(bytes) => Ok(bytes),
        Err(err) => match hvsc_relative_path(source) {
            Some(relative) => {
                let url = mirror_resource_url(mirror_location, &relative);
                hvsc::fetch_bytes(&url).map_err(|fallback| {
                    io::Error::new(
                        fallback.kind(),
                        format!("{err} (HVSC fallback: {fallback})"),
                    )
                })
            }
            None => Err(err),
        },
    }
}

/// HVSC top-level collections that can appear as playlist sources.
const HVSC_ROOTS: &[&str] = &[
    "/MUSICIANS/",
    "/GAMES/",
    "/DEMOS/",
    "/DOCUMENTS/",
    "/Update/",
];

/// Maps an HVSC path recorded without its mirror prefix to the path component
/// of an HVSC URL, e.g. `/MUSICIANS/E/Emarti/Tune.sid` or
/// `C:/MUSICIANS/E/Emarti/Tune.sid`. Returns `None` for anything else.
fn hvsc_relative_path(source: &str) -> Option<String> {
    let normalized = source.replace('\\', "/");
    HVSC_ROOTS
        .iter()
        .find_map(|root| normalized.find(root).map(|pos| normalized[pos..].to_string()))
}

/// Parses optional @N subsong suffix from a path.
fn parse_subsong(s: &str) -> (&str, Option<u16>) {
    if let Some(at_pos) = s.rfind('@') {
        let suffix = &s[at_pos + 1..];
        if let Ok(num) = suffix.parse::<u16>() {
            return (&s[..at_pos], Some(num));
        }
    }
    (s, None)
}

/// Extracts filename from path or URL. Handles both unix and windows separators.
pub(crate) fn extract_filename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// A playlist of SID tunes loaded from an m3u file.
#[derive(Debug, Clone, Default)]
pub struct Playlist {
    pub entries: Vec<PlaylistEntry>,
}

impl Playlist {
    /// Creates an empty playlist.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Loads a playlist from an m3u file.
    pub fn load<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let content = fs::read_to_string(&path)?;
        let base_dir = path.as_ref().parent();

        let entries: Vec<PlaylistEntry> = content
            .lines()
            .filter_map(|line| {
                let mut entry = PlaylistEntry::new(line)?;
                // Resolve relative paths against the playlist directory. URLs,
                // local absolute paths and legacy HVSC-relative paths are left
                // untouched so they keep working after a mirror change.
                if !entry.is_url()
                    && hvsc_relative_path(&entry.source).is_none()
                    && !Path::new(&entry.source).is_absolute()
                    && let Some(base) = base_dir
                {
                    entry.source = base.join(&entry.source).to_string_lossy().to_string();
                }
                Some(entry)
            })
            .collect();

        Ok(Self { entries })
    }

    /// Saves the playlist to an m3u file.
    pub fn save<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let content: String = self
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
        fs::write(path, content)
    }

    /// Returns true if playlist contains an entry with the given source and subsong.
    pub fn contains(&self, source: &str, subsong: Option<u16>) -> bool {
        self.entries
            .iter()
            .any(|e| e.source == source && e.subsong == subsong)
    }

    /// Adds an entry to the playlist if not already present. Returns true if added.
    pub fn add(&mut self, source: &str, subsong: Option<u16>) -> bool {
        if self.contains(source, subsong) {
            return false;
        }
        if let Some(mut entry) = PlaylistEntry::new(source) {
            entry.subsong = subsong;
            self.entries.push(entry);
            true
        } else {
            false
        }
    }

    /// Removes an entry at the given index.
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    /// Moves the entry at `from` to the final index `to`, clamped to the last
    /// entry. Returns the entry's new index, or `None` when the move is a
    /// no-op or an index is out of bounds.
    pub fn move_entry(&mut self, from: usize, to: usize) -> Option<usize> {
        let len = self.entries.len();
        if from >= len {
            return None;
        }
        let to = to.min(len - 1);
        if from == to {
            return None;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        Some(to)
    }

    /// Returns true if the playlist has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Directory that holds the user's playlist library.
pub fn playlists_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sidcraft")
        .join("playlists")
}

/// Lists the `.m3u` playlists in the library, sorted by name.
pub fn list_playlists() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(playlists_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("m3u"))
        })
        .collect();
    files.sort_by(|a, b| {
        a.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
            .cmp(&b.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
    });
    files
}

/// Turns a user-supplied name into a safe m3u file stem. Empty or unusable
/// names fall back to "Playlist"; invalid path characters become underscores.
pub fn sanitize_playlist_name(name: &str) -> String {
    let mut cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    while cleaned.ends_with('.') || cleaned.ends_with(' ') {
        cleaned.pop();
    }

    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "Playlist".to_string()
    } else {
        cleaned
    }
}

/// Path a new playlist with the given display name would use.
pub fn new_playlist_path(name: &str) -> PathBuf {
    playlists_dir().join(format!("{}.m3u", sanitize_playlist_name(name)))
}

/// Display name for a playlist file (file stem).
pub fn playlist_display_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sidcraft-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn loading_m3u_keeps_legacy_and_url_sources_unchanged() {
        let dir = temp_dir("playlist-load");
        let playlist_path = dir.join("list.m3u");
        fs::write(
            &playlist_path,
            "/MUSICIANS/A/Tune.sid\nfile://C:/HVSC/C64Music/MUSICIANS/B/Tune.sid\nlocal.sid\n",
        )
        .unwrap();

        let playlist = Playlist::load(&playlist_path).unwrap();
        assert_eq!(playlist.entries.len(), 3);
        assert_eq!(playlist.entries[0].source, "/MUSICIANS/A/Tune.sid");
        assert!(playlist.entries[1].is_url());
        assert_eq!(
            playlist.entries[2].source,
            dir.join("local.sid").to_string_lossy().to_string()
        );

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_sources_fall_back_to_a_local_mirror_root() {
        let root = temp_dir("playlist-local-mirror");
        let album = root.join("MUSICIANS/A");
        fs::create_dir_all(&album).unwrap();
        let bytes = b"PSID local mirror bytes";
        fs::write(album.join("Tune.sid"), bytes).unwrap();

        let root_string = root.to_string_lossy().replace('\\', "/");
        let location = format!("file://{root_string}");
        assert_eq!(
            fetch_sid_bytes_with_fallback("/MUSICIANS/A/Tune.sid", &location).unwrap(),
            bytes
        );

        fs::remove_dir_all(&root).unwrap();
    }
}
