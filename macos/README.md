# macOS Packaging

Builds a macOS `.app` bundle and a distributable `.dmg` from SidCraft's release
binary.

## Requirements

- macOS
- [Bun](https://bun.sh/) (check with `bun --version`)
- `target/release/sidcraft` produced by `cargo build --release`
- Xcode Command Line Tools (for `sips`, `iconutil`, `codesign`, `hdiutil`)

## Usage

Run the commands from the project root:

```bash
# .app and .dmg together
./macos/build.ts

# .app only
./macos/build.ts app

# .app and .dmg
./macos/build.ts dmg
```

Alternatively, use the `package.json` scripts from inside the `macos`
directory:

```bash
cd macos
bun run build   # .app + .dmg
bun run app     # .app only
bun run dmg     # .app + .dmg
bun run help    # help
bun run typecheck
```

## Options

| Option | Description |
| --- | --- |
| `--no-sign` | Skips ad-hoc code signing |
| `--open` | Opens the produced `.app` or `.dmg` file |
| `--help` | Shows the help text |

Examples:

```bash
./macos/build.ts app --no-sign
./macos/build.ts dmg --open
```

## Outputs

| File | Description |
| --- | --- |
| `dist/SidCraft.app` | macOS application bundle |
| `dist/SidCraft-<version>-macos.dmg` | The version number is read from `Cargo.toml` |

The `.app` bundle contains:

```text
SidCraft.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/
    │   └── sidcraft
    └── Resources/
        └── SidCraft.icns
```

## Notes

- If `icon/sidcraft.png` is 256×256, larger icon sizes are upscaled. Use a
  1024×1024 PNG for a sharper icon.
- `codesign --sign -` is only an ad-hoc signature and is intended for local use.
  Distribution to others requires signing with an Apple Developer ID and
  notarization with `xcrun notarytool`.
- The binary is compiled only for the current architecture. For a universal
  binary, also build for the `x86_64-apple-darwin` target and combine them with
  `lipo`.
- The script reads the version number from the `[package]` section in
  `Cargo.toml`.
