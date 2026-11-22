// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti
//
// Portions derived from crabsid by Mikael Lund,
// Copyright (c) 2026 Mikael Lund, licensed under the MIT License.
// See licenses/MIT-crabsid.txt for the original license text.

//! HVSC (High Voltage SID Collection) browser with STIL metadata support.

use crate::engine::sid_file::SidFile;
use rand::seq::SliceRandom;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Fetches bytes from a URL (http/https), a `file://` URL or a plain local path.
///
/// Shared with the download buttons so a playlist entry can be saved as the
/// original SID file regardless of whether it came from disk or HVSC.
pub fn fetch_bytes(source: &str) -> io::Result<Vec<u8>> {
    if let Some(path) = source.strip_prefix("file://") {
        std::fs::read(Path::new(path))
    } else if source.starts_with("http://") || source.starts_with("https://") {
        let response = ureq::get(source)
            .call()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let mut bytes = Vec::new();
        response.into_body().into_reader().read_to_end(&mut bytes)?;
        Ok(bytes)
    } else {
        std::fs::read(source)
    }
}

/// Latin-1 maps directly to Unicode code points (each byte == codepoint).
fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Fetches text from a URL or local path, treating bytes as Latin-1.
fn fetch_latin1_text(url: &str) -> io::Result<String> {
    Ok(decode_latin1(&fetch_bytes(url)?))
}

/// Fetches text from a URL or local path as UTF-8.
fn fetch_text(url: &str) -> io::Result<String> {
    let bytes = fetch_bytes(url)?;
    String::from_utf8(bytes).map_err(|e| io::Error::other(e.to_string()))
}

/// Default HVSC mirror URL.
pub const DEFAULT_HVSC_URL: &str = "https://hvsc.brona.dk/HVSC/C64Music";

/// Fetches the HVSC release number from `DOCUMENTS/hv_sids.txt`.
///
/// That file begins with a stub comment line and then a line such as
/// `Release 85`, which is the collection version shown in the browser title.
pub fn fetch_release(base_url: &str) -> io::Result<Option<u32>> {
    let content = fetch_text(&format!("{base_url}/DOCUMENTS/hv_sids.txt"))?;
    Ok(parse_release(&content))
}

/// Parses the release line, preferring the documented second line but falling
/// back to any `Release N` line if the file gains a header.
fn parse_release(content: &str) -> Option<u32> {
    let from_second_line = content
        .lines()
        .nth(1)
        .and_then(|line| line.trim().strip_prefix("Release"));
    let from_any_line = content
        .lines()
        .find_map(|line| line.trim().strip_prefix("Release"));
    from_second_line
        .or(from_any_line)
        .and_then(|rest| rest.trim().parse::<u32>().ok())
}

/// Returns the cache directory for crabsid, creating it if needed.
fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("crabsid");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Clears the HVSC cache files.
#[allow(dead_code)] // kept for the remote-driven browser API
pub fn clear_cache() {
    let Some(dir) = cache_dir() else { return };
    for name in [StilDatabase::CACHE_NAME, SonglengthsDatabase::CACHE_NAME] {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// Reads a file as Latin-1 or UTF-8.
fn read_file(path: &Path, latin1: bool) -> io::Result<String> {
    if latin1 {
        Ok(decode_latin1(&fs::read(path)?))
    } else {
        fs::read_to_string(path)
    }
}

/// Reads cached file if present, otherwise fetches from URL and caches result.
fn fetch_with_cache(url: &str, cache_name: &str, latin1: bool) -> io::Result<String> {
    let cache_path = cache_dir().map(|d| d.join(cache_name));

    if let Some(ref path) = cache_path {
        match read_file(path, latin1) {
            Ok(content) => return Ok(content),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }

    let content = if latin1 {
        fetch_latin1_text(url)?
    } else {
        fetch_text(url)?
    };

    if let Some(path) = cache_path {
        let _ = fs::write(&path, &content);
    }

    Ok(content)
}

/// Metadata for a SID file from STIL.
#[derive(Debug, Clone, Default)]
pub struct StilEntry {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub comment: Option<String>,
}

/// Parsed STIL database mapping paths to metadata.
#[derive(Debug, Default, Clone)]
pub struct StilDatabase {
    entries: HashMap<String, StilEntry>,
}

impl StilDatabase {
    pub const CACHE_NAME: &'static str = "STIL.txt";

    /// Fetches and parses the STIL file from HVSC, using cache if available.
    pub fn fetch(base_url: &str) -> io::Result<Self> {
        let url = format!("{base_url}/DOCUMENTS/{}", Self::CACHE_NAME);
        let content = fetch_with_cache(&url, Self::CACHE_NAME, true)?;
        Ok(Self::parse(&content))
    }

    fn parse(content: &str) -> Self {
        let mut entries = HashMap::new();
        let mut current_path: Option<String> = None;
        let mut current_entry = StilEntry::default();

        for line in content.lines() {
            if line.starts_with('/') && line.ends_with(".sid") {
                if let Some(path) = current_path.take() {
                    entries.insert(path, current_entry);
                }
                current_path = Some(line.to_string());
                current_entry = StilEntry::default();
                continue;
            }

            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("TITLE:") {
                current_entry.title = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("ARTIST:") {
                current_entry.artist = Some(rest.trim().to_string());
            } else if let Some(rest) = trimmed.strip_prefix("COMMENT:") {
                current_entry.comment = Some(rest.trim().to_string());
            }
        }

        if let Some(path) = current_path {
            entries.insert(path, current_entry);
        }

        Self { entries }
    }

    /// Returns the number of entries in the database.
    #[allow(dead_code)] // kept for the STIL index API
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the database is empty.
    #[allow(dead_code)] // Required by clippy for len() method
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks up STIL info for a given HVSC path.
    pub fn get(&self, path: &str) -> Option<&StilEntry> {
        self.entries.get(path)
    }
}

/// Song lengths database mapping MD5 hashes to per-subsong durations.
#[derive(Debug, Default, Clone)]
pub struct SonglengthsDatabase {
    entries: HashMap<String, Vec<std::time::Duration>>,
    /// Every SID path listed in the file, in file order. The Songlengths file
    /// names all of HVSC, so this doubles as the list of tunes that exist.
    paths: Vec<String>,
}

impl SonglengthsDatabase {
    pub const CACHE_NAME: &'static str = "Songlengths.md5";

    /// Fetches and parses the Songlengths.md5 file from HVSC, using cache if available.
    pub fn fetch(base_url: &str) -> io::Result<Self> {
        let url = format!("{base_url}/DOCUMENTS/{}", Self::CACHE_NAME);
        let content = fetch_with_cache(&url, Self::CACHE_NAME, false)?;
        Ok(Self::parse(&content))
    }

    fn parse(content: &str) -> Self {
        let mut entries = HashMap::new();
        let mut paths = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }
            // `; /MUSICIANS/S/Shogoon/No_Way_Out.sid` names the file the next
            // MD5 line belongs to; collect these to index the whole collection.
            if let Some(path) = line.strip_prefix(';') {
                let path = path.trim();
                if path.starts_with('/') && path.ends_with(".sid") {
                    paths.push(path.to_string());
                }
                continue;
            }
            // Format: <md5>=<time1> <time2> ...
            if let Some((hash, times)) = line.split_once('=') {
                let durations: Vec<std::time::Duration> = times
                    .split_whitespace()
                    .filter_map(parse_duration)
                    .collect();
                if !durations.is_empty() {
                    entries.insert(hash.to_lowercase(), durations);
                }
            }
        }
        Self { entries, paths }
    }

    /// Every SID path listed in the file.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }

    /// Looks up song durations by MD5 hash. Expects lowercase hex (matches `SidFile::md5`).
    pub fn get(&self, md5: &str) -> Option<&[std::time::Duration]> {
        self.entries.get(md5).map(|v| v.as_slice())
    }

    /// Returns the number of entries in the database.
    #[allow(dead_code)] // May be useful for status display
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Parses duration string "mm:ss" or "mm:ss.mmm" into Duration.
fn parse_duration(s: &str) -> Option<std::time::Duration> {
    // Strip trailing attributes such as "(G)" or "(M)" that HVSC appends to durations.
    let s = s.split('(').next()?.trim();
    let (mins, rest) = s.split_once(':')?;
    let mins: u64 = mins.parse().ok()?;

    let (secs, millis) = if let Some((s, ms)) = rest.split_once('.') {
        let secs: u64 = s.parse().ok()?;
        let millis: u64 = ms.parse().ok()?;
        (secs, millis)
    } else {
        (rest.parse().ok()?, 0)
    };

    Some(std::time::Duration::from_millis(
        mins * 60_000 + secs * 1000 + millis,
    ))
}

/// An entry in the HVSC browser (directory or file).
#[derive(Debug, Clone)]
pub struct HvscEntry {
    /// Display name
    pub name: String,
    /// Full HVSC path (e.g., "/MUSICIANS/H/Hubbard_Rob/")
    pub path: String,
    /// True if this is a directory
    pub is_dir: bool,
}

#[allow(dead_code)] // kept as the entry-level HVSC API
impl HvscEntry {
    /// Returns the full URL for this entry.
    pub fn url(&self, base_url: &str) -> String {
        format!("{base_url}{}", self.path)
    }

    /// Loads this entry as a SID file (only valid for files).
    pub fn load(&self, base_url: &str) -> io::Result<SidFile> {
        if self.is_dir {
            return Err(io::Error::other("Cannot load directory as SID file"));
        }
        let bytes = fetch_bytes(&self.url(base_url))?;
        SidFile::parse(&bytes)
    }
}

/// Top-level HVSC collections that can be sampled for a random playlist.
#[derive(Clone, Copy, Debug, Default)]
pub struct HvscCollectionFilter {
    pub demos: bool,
    pub games: bool,
    pub musicians: bool,
}

impl HvscCollectionFilter {
    /// True when at least one collection is selected.
    pub const fn any(self) -> bool {
        self.demos || self.games || self.musicians
    }

    /// True when `path` belongs to one of the selected collections.
    pub fn matches(self, path: &str) -> bool {
        let normalized = format!(
            "/{}",
            path.trim_start_matches('/')
                .replace('\\', "/")
                .to_ascii_uppercase()
        );
        (self.demos && normalized.contains("/DEMOS/"))
            || (self.games && normalized.contains("/GAMES/"))
            || (self.musicians && normalized.contains("/MUSICIANS/"))
    }
}

/// HVSC directory browser state.
#[derive(Clone)]
pub struct HvscBrowser {
    /// Base URL for HVSC mirror
    pub base_url: String,
    /// Current directory path
    pub current_path: String,
    /// Entries in current directory
    pub entries: Vec<HvscEntry>,
    /// Selected index
    pub selected: usize,
    /// STIL database for metadata
    pub stil: Option<Arc<StilDatabase>>,
    /// STIL loading error (persists across navigation)
    pub stil_error: Option<String>,
    /// Songlengths database for durations
    pub songlengths: Option<Arc<SonglengthsDatabase>>,
    /// HVSC release number from `DOCUMENTS/hv_sids.txt`, when available.
    pub release: Option<u32>,
    /// Lowercase "path\ntitle\nartist" haystack per SID, shared across clones.
    search_index: Arc<Vec<(String, String)>>,
    /// Loading state
    pub loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

/// Normalizes search text: case-insensitive, and `_` in file names counts as a
/// space so "No Way Out" matches `No_Way_Out.sid`.
fn normalize_search_text(text: &str) -> String {
    text.to_lowercase().replace('_', " ")
}

/// Builds one lowercase haystack per SID: the path plus any STIL title and
/// artist. Songlengths lists every tune in the collection, while STIL only has
/// metadata for part of it, so the paths decide what is searchable and STIL
/// entries missing from Songlengths are added on top.
fn build_hvsc_search_index(
    songlengths: Option<&SonglengthsDatabase>,
    stil: Option<&StilDatabase>,
) -> Vec<(String, String)> {
    let mut index = Vec::new();
    // Paths already indexed, so STIL entries for them are not added twice.
    let mut covered: HashSet<&str> = HashSet::new();

    if let Some(songlengths) = songlengths {
        for path in songlengths.paths() {
            covered.insert(path.as_str());
            let entry = stil.and_then(|db| db.get(path));
            index.push((path.clone(), search_haystack(path, entry)));
        }
    }

    if let Some(stil) = stil {
        for (path, entry) in &stil.entries {
            if !covered.contains(path.as_str()) {
                index.push((path.clone(), search_haystack(path, Some(entry))));
            }
        }
    }

    index
}

/// Lowercase search text for one SID: path, then title and artist if known.
fn search_haystack(path: &str, entry: Option<&StilEntry>) -> String {
    let mut haystack = normalize_search_text(path);
    if let Some(entry) = entry {
        for value in [&entry.title, &entry.artist].into_iter().flatten() {
            haystack.push('\n');
            haystack.push_str(&normalize_search_text(value));
        }
    }
    haystack
}

/// True if `needle` starts a word (not just a substring of a longer one).
fn contains_at_word_start(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(needle) {
        let abs = from + pos;
        let starts_word = haystack[..abs]
            .chars()
            .next_back()
            .map_or(true, |c| !c.is_alphanumeric());
        if starts_word {
            return true;
        }
        from = abs + needle.len();
        if from >= haystack.len() {
            return false;
        }
    }
    false
}

/// Ranks a match: exact file names first, then names starting with the query,
/// then path matches, then title/artist mentions.
fn search_score(path: &str, haystack: &str, phrase: &str, tokens: &[String]) -> u32 {
    let mut score = 0;
    if let Some(name) = path.rsplit('/').next() {
        let stem = normalize_search_text(name);
        let stem = stem.strip_suffix(".sid").unwrap_or(&stem);
        if stem == phrase {
            score += 1000;
        } else if stem.starts_with(phrase) {
            score += 500;
        }
    }
    if normalize_search_text(path).contains(phrase) {
        score += 100;
    }
    if haystack.contains(phrase) {
        score += 50;
    }
    for token in tokens {
        if contains_at_word_start(haystack, token) {
            score += 10;
        }
    }
    score
}

/// Top-level HVSC categories shown when the browser is at "/".
fn root_entries() -> Vec<HvscEntry> {
    ["MUSICIANS", "GAMES", "DEMOS"]
        .into_iter()
        .map(|name| HvscEntry {
            name: name.to_string(),
            path: format!("/{name}/"),
            is_dir: true,
        })
        .collect()
}

impl HvscBrowser {
    /// Creates a new browser at the root level.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            current_path: "/".to_string(),
            entries: root_entries(),
            selected: 0,
            stil: None,
            stil_error: None,
            songlengths: None,
            release: None,
            search_index: Arc::new(Vec::new()),
            loading: false,
            error: None,
        }
    }

    /// Fetches the STIL and Songlengths databases and the release number.
    pub fn load_stil(&mut self) {
        // First-run fetch downloads ~10MB across two URLs; run them in parallel
        // with the tiny release file so startup does not wait three times.
        let base_url = self.base_url.as_str();
        let (stil_result, songlengths_result, release_result) = std::thread::scope(|s| {
            let stil = s.spawn(|| StilDatabase::fetch(base_url));
            let songlengths = s.spawn(|| SonglengthsDatabase::fetch(base_url));
            let release = s.spawn(|| fetch_release(base_url));
            (
                stil.join().expect("STIL thread"),
                songlengths.join().expect("Songlengths thread"),
                release.join().expect("HVSC release thread"),
            )
        });

        match stil_result {
            Ok(db) => self.stil = Some(Arc::new(db)),
            Err(e) => self.stil_error = Some(e.to_string()),
        }
        // Songlengths errors are silently ignored - we just fall back to playtime
        if let Ok(db) = songlengths_result {
            self.songlengths = Some(Arc::new(db));
        }
        // A missing release file simply leaves the title without a version.
        if let Ok(Some(version)) = release_result {
            self.release = Some(version);
        }
        self.rebuild_search_index();
    }

    /// Rebuilds the search index from the databases currently loaded.
    fn rebuild_search_index(&mut self) {
        self.search_index = Arc::new(build_hvsc_search_index(
            self.songlengths.as_deref(),
            self.stil.as_deref(),
        ));
    }

    /// Clears the HVSC cache and reloads STIL and Songlengths databases.
    #[allow(dead_code)] // kept for a future "refresh cache" command
    pub fn refresh_cache(&mut self) {
        clear_cache();
        self.stil = None;
        self.stil_error = None;
        self.songlengths = None;
        self.release = None;
        self.search_index = Arc::new(Vec::new());
        self.load_stil();
    }

    /// Returns STIL info for the selected entry if available.
    #[allow(dead_code)]
    pub fn selected_stil_info(&self) -> Option<&StilEntry> {
        let entry = self.entries.get(self.selected)?;
        if entry.is_dir {
            return None;
        }
        self.stil.as_ref()?.get(&entry.path)
    }

    /// Returns song duration for given MD5 and subsong (1-indexed), if available.
    pub fn song_duration(&self, md5: &str, subsong: u16) -> Option<std::time::Duration> {
        let durations = self.songlengths.as_ref()?.get(md5)?;
        // Subsongs are 1-indexed, array is 0-indexed
        durations.get(subsong.saturating_sub(1) as usize).copied()
    }

    /// Returns true if we can navigate up.
    pub fn can_go_up(&self) -> bool {
        self.current_path != "/"
    }

    /// Searches every SID in the collection by path, title, and artist.
    /// Underscores in file names count as spaces, and all query words must
    /// appear, so both "No Way Out" and "shogoon no way" find
    /// `/MUSICIANS/S/Shogoon/No_Way_Out.sid`. Exact name matches come first.
    pub fn search(&self, query: &str) -> io::Result<Vec<String>> {
        let tokens: Vec<String> = query
            .split_whitespace()
            .map(normalize_search_text)
            .filter(|token| !token.is_empty())
            .collect();
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        let phrase = tokens.join(" ");

        let mut matches: Vec<(u32, &str)> = self
            .search_index
            .iter()
            .filter(|(_, haystack)| tokens.iter().all(|token| haystack.contains(token.as_str())))
            .map(|(path, haystack)| (search_score(path, haystack, &phrase, &tokens), path.as_str()))
            .collect();
        matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));

        Ok(matches
            .into_iter()
            .take(100)
            .map(|(_, path)| path.to_string())
            .collect())
    }

    /// Number of indexed SID paths in the selected collections.
    pub fn collection_count(&self, collections: HvscCollectionFilter) -> usize {
        self.search_index
            .iter()
            .filter(|(path, _)| collections.matches(path))
            .count()
    }

    /// Picks up to `count` distinct SID paths from the selected collections.
    /// The index is already in memory, so this needs no network access.
    pub fn random_paths(&self, collections: HvscCollectionFilter, count: usize) -> Vec<String> {
        if !collections.any() || count == 0 {
            return Vec::new();
        }

        let mut pool: Vec<&str> = self
            .search_index
            .iter()
            .map(|(path, _)| path.as_str())
            .filter(|path| collections.matches(path))
            .collect();
        let take = count.min(pool.len());
        let mut rng = rand::rng();
        pool.shuffle(&mut rng);
        pool.into_iter().take(take).map(str::to_string).collect()
    }

    /// Fetches a specific entry as a SID file from HVSC.
    pub fn fetch_entry(&self, path: &str) -> io::Result<SidFile> {
        let url = format!("{}{}", self.base_url, path);
        let bytes = fetch_bytes(&url)?;
        SidFile::parse(&bytes)
    }

    /// Lists entries in the current path.
    pub fn list(&self) -> io::Result<Vec<HvscEntry>> {
        fetch_directory(&self.base_url, &self.current_path)
    }

    #[allow(dead_code)] // kept for the keyboard-driven browser API
    pub fn enter(&mut self) -> Option<HvscEntry> {
        let entry = self.entries.get(self.selected)?.clone();

        if entry.is_dir {
            self.navigate_to(&entry.path);
            None
        } else {
            Some(entry)
        }
    }

    /// Go up one directory level.
    #[allow(dead_code)] // the shell computes the parent path itself
    pub fn go_up(&mut self) {
        if self.current_path == "/" {
            return;
        }

        // Remove trailing slash, find parent
        let path = self.current_path.trim_end_matches('/');
        if let Some(pos) = path.rfind('/') {
            let parent = if pos == 0 {
                "/".to_string()
            } else {
                format!("{}/", &path[..pos])
            };
            self.navigate_to(&parent);
        }
    }

    /// Navigate to a specific path.
    pub fn navigate_to(&mut self, path: &str) {
        if path == "/" {
            // Reset entries to root listing without losing the loaded databases.
            self.entries = root_entries();
            self.current_path = "/".to_string();
            self.selected = 0;
            self.error = None;
            return;
        }

        self.loading = true;
        self.error = None;

        match fetch_directory(&self.base_url, path) {
            Ok(entries) => {
                self.current_path = path.to_string();
                self.entries = entries;
                self.selected = 0;
            }
            Err(e) => {
                self.error = Some(e.to_string());
            }
        }

        self.loading = false;
    }

    #[allow(dead_code)] // kept for the keyboard-driven browser API
    pub fn select_next(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1).min(self.entries.len() - 1);
        }
    }

    #[allow(dead_code)] // kept for the keyboard-driven browser API
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// Returns the currently selected entry.
    #[allow(dead_code)]
    pub fn selected_entry(&self) -> Option<&HvscEntry> {
        self.entries.get(self.selected)
    }
}

/// Fetches and parses a directory listing from HVSC.
fn fetch_directory(base_url: &str, path: &str) -> io::Result<Vec<HvscEntry>> {
    if let Some(base_path) = base_url.strip_prefix("file://") {
        read_local_directory(base_path, path)
    } else {
        fetch_http_directory(base_url, path)
    }
}

/// Reads a local directory and returns HVSC entries.
fn read_local_directory(base_path: &str, path: &str) -> io::Result<Vec<HvscEntry>> {
    let full_path = Path::new(base_path).join(path.trim_start_matches('/'));
    let mut entries: Vec<HvscEntry> = std::fs::read_dir(&full_path)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().ok()?.is_dir();

            if !is_browsable(is_dir, &name) {
                return None;
            }

            let entry_path = if is_dir {
                format!("{path}{name}/")
            } else {
                format!("{path}{name}")
            };

            Some(HvscEntry {
                name,
                path: entry_path,
                is_dir,
            })
        })
        .collect();

    sort_entries(&mut entries);
    Ok(entries)
}

/// Sorts entries with directories first, then case-insensitive alphabetical for browsing.
fn sort_entries(entries: &mut [HvscEntry]) {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Browser shows directories and `.sid` files only. HVSC's `DOCUMENTS/` and
/// `update/` directories are housekeeping, not browsable music.
fn is_browsable(is_dir: bool, name: &str) -> bool {
    if is_dir {
        !name.eq_ignore_ascii_case("DOCUMENTS") && !name.eq_ignore_ascii_case("update")
    } else {
        name.to_ascii_lowercase().ends_with(".sid")
    }
}

/// Fetches and parses an HTTP directory listing.
fn fetch_http_directory(base_url: &str, path: &str) -> io::Result<Vec<HvscEntry>> {
    let html = fetch_text(&format!("{base_url}{path}"))?;
    Ok(parse_directory_listing(&html, path))
}

/// Extracts href value from an HTML line, filtering navigation/special links.
fn extract_href(line: &str) -> Option<&str> {
    if line.contains("Parent Directory") {
        return None;
    }

    let (_, rest) = line.split_once("href=\"")?;
    let (href, _) = rest.split_once('"')?;

    // Apache listings include sort links and parent refs we don't want.
    let is_navigation =
        href.starts_with('?') || href.starts_with('/') || href.starts_with("http") || href == "../";

    if is_navigation { None } else { Some(href) }
}

/// Parses an Apache-style directory listing HTML.
fn parse_directory_listing(html: &str, base_path: &str) -> Vec<HvscEntry> {
    let mut entries: Vec<HvscEntry> = html
        .lines()
        .filter_map(|line| {
            let href = extract_href(line)?;
            let is_dir = href.ends_with('/');
            let name = href.trim_end_matches('/').to_string();

            if !is_browsable(is_dir, &name) {
                return None;
            }

            Some(HvscEntry {
                name,
                path: format!("{base_path}{href}"),
                is_dir,
            })
        })
        .collect();

    sort_entries(&mut entries);
    entries
}

