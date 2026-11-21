# ![SidCraft](icon/sidcraft-header.png) SidCraft

SidCraft is a desktop player for Commodore 64 SID music files in PSID and RSID
formats, written in Rust. It emulates SID audio, so an internet connection is
not required to play local files. The built-in HVSC browser connects to an
online HVSC mirror.

## Features

- Open SID files and switch between the tunes (subsongs) included in a file.
- Open SID files with SidCraft from the OS ("Open with", Finder, a file
  manager or the command line); without a file argument the active playlist
  is used. Only one instance runs: opening another file replaces the tune in
  the window that is already playing.
- Emulate and play C64 SID audio, including tunes that use one, two, or three
  SID chips.
- Configure the SID chip model and the EKV transistor filter for the MOS 6581;
  the EKV/standard filter choice is remembered between sessions.
- Play, pause, stop, and seek within a tune.
- Create, manage, and save M3U playlists; shuffle and repeat playback.
- Browse folders and search by title, author, or path in an HVSC mirror.
  Tunes can be downloaded from the browser and played.
- Generate random playlists from the HVSC index: choose 5–100 tracks and one
  or more collections (Demos, Games, Musicians), then save the result as a
  named M3U playlist in the library.
- Choose from eight visualisations: envelope, VU meter, spectrum,
  spectrogram, oscilloscope, piano roll, filter, and ADSR.
- Switch to a frameless mini player with Ctrl+M, the mini button, or a
  double-click on the header. It has transport controls, an inline progress
  bar and a compact visualisation; double-click the visualisation to cycle to
  the next one. Drag the header to move it and the bottom-right corner to
  resize it. Its size is remembered, and Settings offers adjustable
  opacity (64% by default) and a "Start in mini player" option. The full window
  and the mini player each remember their own position. 
- Export a SID tune to WAV, FLAC, or MP3. Configure the export duration,
  fade-out, sample rate, and master gain; mute individual voices.
- Check GitHub for a newer stable release from Help, About or Settings, either
  automatically about 30 seconds after startup or on demand. The update is
  downloaded in the app after confirmation and verified against its SHA-256
  digest; Windows and Linux restart into the new version, macOS opens the DMG.
- Choose system, light, or dark themes. Preferences such as window size are
  saved in the application settings; the full window and the mini player each
  keep their own position on Windows, macOS, and Linux/X11.

![SidCraft-Player](docs/screenshots/player.png)
   
## Documentation

See [docs/DOCUMENTATION.md](docs/DOCUMENTATION.md) for the feature overview,
FFmpeg requirements and build instructions for Windows, Linux and macOS.

## License

SidCraft is licensed under the GNU General Public License v3.0 only. See [LICENSE](LICENSE) for the full text.

## Acknowledgements and third-party code

**crabsid** — The `src/engine/` directory (excluding `export.rs`) contains files derived from [crabsid](https://github.com/mlund/crabsid) by Mikael Lund, which are licensed under the MIT License. These files retain their original copyright notices, and those that were modified are additionally marked as derived from crabsid. The original MIT license text is included in [licenses/MIT-crabsid.txt](licenses/MIT-crabsid.txt).

**PdfCraft** — `src/theme.rs` contains design tokens derived from [PdfCraft](https://github.com/storytold/pdfcraft) (crate `ui-egui`, file `src/theme.rs`), Copyright (c) 2026 ArtCraft Team and the PdfCraft contributors, licensed MIT OR Apache-2.0. The MIT option is elected so the derived code can be combined into this GPL-3.0-only work. `src/ui.rs` and `src/main.rs` only follow PdfCraft's shell layout grammar and contain no copied code. The licence texts are included in [licenses/LICENSE-PDFCRAFT-MIT.txt](licenses/LICENSE-PDFCRAFT-MIT.txt) and [licenses/LICENSE-PDFCRAFT-APACHE-2.0.txt](licenses/LICENSE-PDFCRAFT-APACHE-2.0.txt).

**Full list:** [licenses/ATTRIBUTION.md](licenses/ATTRIBUTION.md)
