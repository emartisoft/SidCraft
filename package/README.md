# SidCraft packaging

One Bun + TypeScript project for all three platforms. The old `windows/`,
`macos/` and `linux/` packaging projects are now the platform modules under
`src/platforms/`.

## Recommended entry point

From the repository root:

```bash
bun run createPackage.ts
```

That builds `target/release` with Cargo, then runs this project's `build`
command. The finished archive is written to the repository's `dist/` folder.

## Direct usage

Run these from the `package/` directory:

```bash
bun run build       # package for the current OS
bun run app         # macOS: SidCraft.app only
bun run dmg         # macOS: SidCraft.app + .dmg
bun run install-desktop    # Linux: install desktop entry and icons
bun run uninstall-desktop  # Linux: remove desktop integration
bun run help
bun run typecheck   # optional; run bun install first
```

`bun run build` on Linux also runs `cargo build --release` unless
`--skip-build` is passed or `SIDCRAFT_SKIP_CARGO_BUILD=1` is set. The root
`createPackage.ts` sets that environment variable because it has already built
the release binary.

## Outputs

| OS | Output in `dist/` |
| --- | --- |
| Windows | `SidCraft-<version>-windows-x86_64.zip` containing `sidcraft.exe` at the archive root |
| Linux | `SidCraft-<version>-linux-<arch>.AppImage` (`x86_64` or `aarch64`) |
| macOS | `SidCraft.app` and `SidCraft-<version>-macos.dmg` |

The version is read from the `[package]` section of the root `Cargo.toml`.

## Platform requirements

### Windows

- Windows with PowerShell (`Compress-Archive`)
- `target/release/sidcraft.exe`

The ZIP is created with PowerShell; no external archive tool is needed.

### Linux

- Bun 1.4+ (`Bun.Image` generates the hicolor icon set)
- `target/release/sidcraft`
- Network access on the first AppImage build so `appimagetool` can be cached
  under `~/.cache/sidcraft/appimage-tools/`

No external bash scripts or image tools are used. The AppDir is assembled in
`target/appimage/SidCraft.AppDir/`, with `AppRun` as a direct symlink to the
binary.

`install` writes `~/.local/share/applications/sidcraft.desktop` and the hicolor
icons, refreshes the desktop caches, and unpins SidCraft from GNOME favorites so
it appears in the Show Apps grid. `uninstall` reverses this. `XDG_DATA_HOME`
and `XDG_CACHE_HOME` are respected.

### macOS

- macOS with Xcode Command Line Tools (`sips`, `iconutil`, `codesign`, `hdiutil`)
- `target/release/sidcraft`
- `icon/sidcraft.png` (a 1024×1024 source gives the sharpest icon)

Options:

| Option | Description |
| --- | --- |
| `--no-sign` | Skip ad-hoc code signing |
| `--open` | Open the produced `.app` or `.dmg` |

The DMG is staged with an `/Applications` symlink. Ad-hoc signing is for local
use; distribution requires an Apple Developer ID and notarization.
