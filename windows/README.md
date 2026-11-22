# Windows Packaging

Creates a distributable Windows ZIP archive from SidCraft's release executable.
The whole build is a single Bun/TypeScript file: `build.ts`.

## Requirements

- Windows
- [Bun](https://bun.sh/) (check with `bun --version`)
- `target/release/sidcraft.exe` produced by `cargo build --release`
- Windows PowerShell (used by `Compress-Archive` to create the ZIP)

## Usage

Run the script from the repository root:

```bash
bun windows/build.ts
```

Or from inside the `windows` directory:

```bash
cd windows
bun build.ts
```

If the release executable is missing, the script prints a reminder and exits
without creating an archive:

```text
Release binary not found: ...\target\release\sidcraft.exe
Run: cargo build --release
```

## Output

```text
dist/SidCraft-<version>-windows-x86_64.zip
```

The version is read from the `[package] version` field in `Cargo.toml`.
The `dist/` directory is created in the repository root if it does not exist.

The ZIP contains only the executable, stored at the archive root:

```text
sidcraft.exe
```

## Notes

- The filename architecture is fixed to `x86_64`, matching the default Windows
  release build.
- An existing archive with the same name is overwritten.
- The script does not run `cargo build --release` itself; build the Rust binary
  first, then run the packaging step.
