# ![SidCraft](../icon/sidcraft-header.png) SidCraft

## Opening SID files from the OS

SidCraft can be started with a SID file as its argument. The file is loaded
and played immediately; the saved playlist stays untouched. When no file is
given, SidCraft resumes with the playlist that was active last time. If the
argument is an `.m3u` playlist, that playlist becomes the active one.

Only one SidCraft instance runs at a time. Opening a file while the app is
already running hands it to the existing window over a local socket and the
new process exits, so the current tune is replaced instead of a second window
and a second audio stream appearing. Starting SidCraft again without a file
brings the running window to the front. On macOS this is Finder's normal
behaviour; on Windows and Linux the app takes care of it.

### Windows

Right-click a `.sid` file, choose **Open with** → **Choose another app** and
pick `sidcraft.exe`. Dragging a file onto `sidcraft.exe` works as well. From a
terminal:

```powershell
.\target\release\sidcraft.exe "C:\Music\HVSC\...\Tune.sid"
```

### Linux

The desktop entry declares the `audio/prs.sid`, `audio/x-sid` and
`audio/x-psid` MIME types, so file managers list SidCraft under **Open With**
after `bun run appimage install` (or when using the AppImage). From a terminal:

```bash
./target/release/sidcraft ~/Music/HVSC/MUSICIANS/.../Tune.sid
```

### macOS

The `.app` bundle built by `macos/build.ts` declares the SID document type, so
Finder lists SidCraft under **Open With**. From a terminal:

```bash
./target/release/sidcraft ~/Music/HVSC/MUSICIANS/.../Tune.sid
```

## FFmpeg dependency

FFmpeg is not required to build SidCraft or play SID files. WAV export is
supported natively. FLAC export uses FFmpeg; if FFmpeg is unavailable, the
application falls back to high-definition WAV output. MP3 export requires
FFmpeg with the `libmp3lame` encoder. SidCraft does not bundle FFmpeg, so it
must be installed and accessible on your computer.

After installation, verify it in a terminal with `ffmpeg -version`. SidCraft
must be able to find the `ffmpeg` executable through `PATH`. If it cannot be
found automatically, set the `FFMPEG_PATH` environment variable to the full
path to the `ffmpeg` executable, then restart SidCraft.

### Windows

1. Visit the [FFmpeg download page](https://ffmpeg.org/download.html) and
   download a Windows build.
2. Extract the archive and add its `bin` folder to the `PATH` environment
   variable. For example: `C:\ffmpeg\bin`.
3. Restart SidCraft and verify the installation with `ffmpeg -version`.

If needed, set the full path in PowerShell for the current session only:

```powershell
$env:FFMPEG_PATH = 'C:\ffmpeg\bin\ffmpeg.exe'
```

### macOS

Install with Homebrew:

```bash
brew install ffmpeg
ffmpeg -version
```

### Linux

Use your distribution's package manager. Debian/Ubuntu example:

```bash
sudo apt update
sudo apt install ffmpeg
ffmpeg -version
```

On Fedora, you can use `sudo dnf install ffmpeg`; an additional repository
such as RPM Fusion may be required to access the FFmpeg package. On Arch Linux:

```bash
sudo pacman -S ffmpeg
```

## Building from source

SidCraft uses the Rust 2024 edition and requires Rust 1.85 or newer. If Rust
and Cargo are not installed, install them from [rustup.rs](https://rustup.rs/).
Run the following command from the repository root:

```bash
cargo build --release
```

When the build completes, the executable is placed under `target/release/`.
Run it with:

```bash
# Windows PowerShell
.\target\release\sidcraft.exe

# macOS or Linux
./target/release/sidcraft
```

### Windows

- Install Rust with the MSVC toolchain.
- In the Visual Studio Build Tools installer, select the **Desktop
  development with C++** workload and a Windows SDK.
- Open PowerShell or Developer PowerShell and run `cargo build --release`
  from the repository root.

Output: `target\release\sidcraft.exe`.

### macOS

- Install the Xcode Command Line Tools with `xcode-select --install`.
- Run `cargo build --release` from the repository root.

Output: `target/release/sidcraft`. This builds a binary for the Mac's current
architecture. To optionally create a `.app` bundle and `.dmg` package, install
Bun and the Xcode Command Line Tools, then run:

```bash
cd macos
bun run build   # .app + .dmg
bun run app     # .app only
bun run dmg     # .app + .dmg
bun run help    # help
bun run typecheck
```

### Linux

- In addition to Rust and Cargo, you need a C compiler, `pkg-config`, ALSA
  development libraries, and development libraries for the graphical window
  system.
- Common Debian/Ubuntu packages:

```bash
sudo apt update
sudo apt install build-essential pkg-config libasound2-dev \
  libx11-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libegl1-mesa-dev libgl1-mesa-dev
```

- Then run `cargo build --release` from the repository root.

Output: `target/release/sidcraft`. To optionally create an AppImage, install
Bun 1.4 or newer and run:

```bash
cd linux
# Build the AppImage (cargo build + icons + AppDir + appimagetool)
bun run appimage build
```

System library package names may vary between Linux distributions. The
AppImage builder downloads `appimagetool` the first time it runs.

## License

SidCraft is licensed under GNU GPL version 3. See the repository's `LICENSE`
and `licenses/ATTRIBUTION.md` files for third-party code and license notices.
