import { homedir } from "node:os";
import { join, resolve } from "node:path";

/** Directory of this file: `linux/src`. */
export const SRC_DIR = import.meta.dir;
/** The Bun project directory: `linux`. */
export const PROJECT_DIR = resolve(SRC_DIR, "..");
/** The Cargo repository root (one level above `linux`). */
export const REPO_ROOT = resolve(PROJECT_DIR, "..");

export const BINARY = join(REPO_ROOT, "target/release/sidcraft");
export const ICON_SOURCE = join(REPO_ROOT, "icon/sidcraft.png");
export const HICOLOR_DIR = join(REPO_ROOT, "icon/hicolor");
export const APPDIR = join(REPO_ROOT, "target/appimage/SidCraft.AppDir");
/** Where the final `.AppImage` files are written. */
export const DIST_DIR = join(REPO_ROOT, "dist");
export const ASSETS_DIR = join(PROJECT_DIR, "assets");

export const ICON_SIZES = [16, 24, 32, 48, 64, 128, 256] as const;

/** File name of the installed desktop entry, also its GNOME app id. */
export const DESKTOP_ID = "sidcraft.desktop";

export const XDG_DATA_HOME =
  process.env.XDG_DATA_HOME ?? join(homedir(), ".local/share");
export const XDG_CACHE_HOME =
  process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache");

/** AppImage architecture name, derived from the Bun/Node architecture. */
export function appImageArch(): string {
  switch (process.arch) {
    case "x64":
      return "x86_64";
    case "arm64":
      return "aarch64";
    default:
      throw new Error(`unsupported architecture: ${process.arch}`);
  }
}

/** Reads the package version from the Cargo manifest. */
export async function projectVersion(): Promise<string> {
  const manifest = await Bun.file(join(REPO_ROOT, "Cargo.toml")).text();
  const match = manifest.match(/^\s*version\s*=\s*"([^"]+)"/m);
  return match?.[1] ?? "0.0.0";
}
