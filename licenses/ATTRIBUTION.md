# Third-party code in SidCraft

SidCraft itself is licensed under the GNU General Public License v3.0 only
(see [../LICENSE](../LICENSE)). The files below contain code taken from, or
modelled on, other projects. Their original copyright notices are kept in the
file headers, and the full licence texts are stored in this directory.

## PdfCraft

* Project: PdfCraft — <https://github.com/storytold/pdfcraft>
* Copyright (c) 2026 ArtCraft Team and the PdfCraft contributors
* Licence: MIT OR Apache-2.0
* Licence texts: [LICENSE-PDFCRAFT-MIT.txt](LICENSE-PDFCRAFT-MIT.txt),
  [LICENSE-PDFCRAFT-APACHE-2.0.txt](LICENSE-PDFCRAFT-APACHE-2.0.txt)

| SidCraft file | Relationship |
|---|---|
| `src/theme.rs` | Derived from PdfCraft `crates/ui-egui/src/theme.rs`. The `ThemeKind` / `ThemePreference` types, the `Tokens` struct and its light/dark palette are taken from that file. The MIT option of PdfCraft's dual licence is elected, which is why the derived code can be combined into this GPL-3.0-only work. |
| `src/ui.rs` | Independent SidCraft implementation. Only the shell *layout grammar* (tab strip, mode bar, tool panel, pasteboard, inspector, status bar) is inspired by PdfCraft; no code is copied. |
| `src/main.rs` | Independent SidCraft implementation. The Home view mirrors the idea of PdfCraft's Home tab; no code is copied. |

## crabsid

* Project: crabsid — <https://github.com/mlund/crabsid>
* Copyright (c) 2026 Mikael Lund
* Licence: MIT (the project is dual-licensed; the MIT terms apply to these files)
* Licence text: [MIT-crabsid.txt](MIT-crabsid.txt)

The files in `src/engine/` originate from crabsid. Files that were modified for
SidCraft carry a "Portions derived from crabsid" note in their header; files
that are byte-for-byte identical keep their original MIT header unchanged.

| SidCraft file | Relationship |
|---|---|
| `src/engine/audio.rs` | Identical to `crabsid/src/audio.rs`. |
| `src/engine/cia.rs` | Identical to `crabsid/src/cia.rs`. |
| `src/engine/config.rs` | Modified from `crabsid/src/config.rs`. |
| `src/engine/hvsc.rs` | Modified from `crabsid/src/hvsc.rs`. |
| `src/engine/memory.rs` | Modified from `crabsid/src/memory.rs`. |
| `src/engine/player.rs` | Modified from `crabsid/src/player.rs`. |
| `src/engine/playlist.rs` | Modified from `crabsid/src/playlist.rs`. |
| `src/engine/sid_file.rs` | Modified from `crabsid/src/sid_file.rs`. |

## Rust dependencies

The Rust dependencies pulled in by Cargo are not vendored here; each keeps its
own licence. The most relevant one for the project licence is `residfp`
(<https://github.com/mlund/residfp>), which is GPL-3.0-or-later.
