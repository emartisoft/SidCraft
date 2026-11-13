# SidCraft Linux AppImage (Bun + TypeScript)

This project builds, installs and uninstalls the Linux AppImage for SidCraft
using **Bun + TypeScript**. There are **no external bash scripts**; every step
is executed from TypeScript with `Bun.spawn`, `Bun.Image` and `Bun.write`.

## Requirements

- Bun **1.4+** (for `Bun.Image`)
- Rust/Cargo and the Linux build dependencies (for `cargo build --release`)
- `curl` is not required: `appimagetool` is downloaded with `fetch` on first run
- ImageMagick/Pillow is not required for icons: `Bun.Image` is used

## Usage

All commands are run in the `linux/` directory:

```bash
cd linux

# Build the AppImage (cargo build + icons + AppDir + appimagetool)
bun run appimage build

# Repackage using an already built target/release/sidcraft
bun run appimage build --skip-build

# Install desktop integration (.desktop + icons, no root required)
bun run appimage install

# Install a specific AppImage
bun run appimage install ../dist/SidCraft-0.1.0-x86_64.AppImage

# Uninstall
bun run appimage uninstall
```

Shortcuts:

```bash
bun run build    # = bun run appimage build
bun run setup    # = bun run appimage install
bun run remove   # = bun run appimage uninstall
```

If you want to run it with a single command from the repository root:

```bash
bun linux/src/cli.ts build
bun linux/src/cli.ts install
bun linux/src/cli.ts uninstall
```

## What does it produce?

```
dist/SidCraft-<version>-<arch>.AppImage   # in the repository root
target/appimage/SidCraft.AppDir/          # intermediate AppDir
icon/hicolor/<size>/apps/sidcraft.png
```

The version is read from the `[package] version` field in `../Cargo.toml`, and
the architecture is derived from `process.arch` (`x64` → `x86_64`,
`arm64` → `aarch64`).

## AppDir structure

```
SidCraft.AppDir/
├── AppRun -> usr/bin/sidcraft        # a symlink, not a shell script
├── sidcraft.desktop
├── sidcraft.png
├── .DirIcon -> sidcraft.png
└── usr/
    ├── bin/sidcraft
    └── share/
        ├── applications/sidcraft.desktop
        ├── icons/hicolor/<size>/apps/sidcraft.png
        └── metainfo/com.emarti.sidcraft.metainfo.xml
```

## About the desktop icon

On Wayland the application cannot set its own window icon; the desktop
environment finds the icon from an **installed** `.desktop` file that matches
the window's `app_id` (`sidcraft`). That is why the `install` command:

- writes `~/.local/share/applications/sidcraft.desktop`,
- installs the icons into `~/.local/share/icons/hicolor/...`,
- runs `update-desktop-database` and `gtk-update-icon-cache`.

`uninstall` removes all of these again. The XDG variables (`XDG_DATA_HOME`,
`XDG_CACHE_HOME`) are taken into account.

## "Show Apps" visibility on GNOME

GNOME Shell's application grid (`AppDisplay._loadApps`) **deliberately does not
list applications pinned to the dock**: every application found in the
`org.gnome.shell favorite-apps` key is filtered out of the grid, because it is
already visible in the dock. That is why SidCraft disappears from "Show Apps"
once you pin it to the taskbar. On Ubuntu, Firefox, Nautilus, Snap Store and
Ptyxis are also missing from the grid for the same reason.

`install` therefore removes SidCraft from the `favorite-apps` list, so that the
application is listed in the **Show Apps** grid:

```bash
bun run appimage install
# ==> GNOME: unpinned SidCraft from the dock so it appears in Show Apps.
```

`uninstall` clears the same entry as well. The shell updates immediately after
installation (the `changed::favorite-apps` signal); no session logout is needed.

> **Note:** On GNOME there is no supported way to show the same application in
> both the dock and the grid. If you pin SidCraft to the dock again, it is
> removed from the grid once more. If you want both at the same time, you can
> create a copy of the `.desktop` file under a different name (e.g.
> `sidcraft-dock.desktop`) and pin **that** to the dock; `sidcraft.desktop`
> stays in the grid.

## Files

| File | Purpose |
|---|---|
| `src/cli.ts` | Command line interface (`build`/`install`/`uninstall`) |
| `src/build.ts` | End-to-end AppImage build |
| `src/appdir.ts` | Building the AppDir |
| `src/icons.ts` | hicolor icon set via `Bun.Image` |
| `src/appimagetool.ts` | Downloading and running appimagetool |
| `src/install.ts` | Install/remove desktop integration |
| `src/gnome.ts` | Clearing the GNOME `favorite-apps` entry (Show Apps) |
| `src/exec.ts` | Shell-free process execution (`Bun.spawn`) |
| `src/config.ts` | Paths, version, architecture |
| `assets/` | `.desktop` and AppStream templates |

## Notes

- The AppImage only packages the application. OpenGL/EGL, X11/Wayland, ALSA and
  D-Bus portal libraries come from the host system.
- `appimagetool` is cached under `~/.cache/sidcraft/appimage-tools/`.
- For TypeScript type checking (optional): `bun install && bun run typecheck`.
  `bun install` is not required for normal usage.
