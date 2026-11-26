import { join, resolve } from "node:path";

/** Directory containing this unified packaging project. */
export const PACKAGE_DIR = resolve(import.meta.dir, "..");
/** Cargo repository root (one level above `package/`). */
export const REPO_ROOT = resolve(PACKAGE_DIR, "..");
/** Final packages are written here. */
export const DIST_DIR = join(REPO_ROOT, "dist");
/** Shared templates for Linux AppImage integration. */
export const ASSETS_DIR = join(PACKAGE_DIR, "assets");

export function binaryName(): string {
  return process.platform === "win32" ? "sidcraft.exe" : "sidcraft";
}

export function releaseBinary(): string {
  return join(REPO_ROOT, "target", "release", binaryName());
}

/** Reads the package version from the repository's Cargo.toml. */
export async function readVersion(): Promise<string> {
  const manifest = await Bun.file(join(REPO_ROOT, "Cargo.toml")).text();
  const version = manifest.match(
    /^\[package\][\s\S]*?^\s*version\s*=\s*"([^"]+)"/m,
  )?.[1];

  if (!version) {
    throw new Error("Could not read the package version from Cargo.toml");
  }

  return version;
}

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
