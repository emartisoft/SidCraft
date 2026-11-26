#!/usr/bin/env bun

// Single-file, package.json-free release packager.
// Usage: bun run createPackage.ts
// Builds the Cargo release binary, then runs the packaging project for the
// current OS. Bun's own APIs are used instead of Node child_process/fs imports.

const USAGE = `SidCraft release packager

Usage:
  bun run createPackage.ts

Builds target/release first with Cargo, then runs:
  Windows: bun windows/build.ts
  Linux:   cd linux && bun run appimage build
  macOS:   cd macos && bun run build

The resulting package is written to the repository's dist/ directory.
No package.json is required for this script.
`;

interface PackageStep {
  label: string;
  command: string[];
  cwd: string;
  artifact: string;
}

function separator(): string {
  return process.platform === "win32" ? "\\" : "/";
}

function join(...parts: string[]): string {
  const sep = separator();
  return parts
    .map((part, index) =>
      index === 0 ? part.replace(/[\\/]+$/, "") : part.replace(/^[\\/]+/, ""),
    )
    .filter(Boolean)
    .join(sep);
}

const ROOT = import.meta.dir;
const DIST = join(ROOT, "dist");

async function readVersion(): Promise<string> {
  const manifest = await Bun.file(join(ROOT, "Cargo.toml")).text();
  const version = manifest.match(
    /^\[package\][\s\S]*?^\s*version\s*=\s*"([^"]+)"/m,
  )?.[1];

  if (!version) {
    throw new Error("Could not read the package version from Cargo.toml");
  }

  return version;
}

function appImageArch(): string {
  switch (process.arch) {
    case "x64":
      return "x86_64";
    case "arm64":
      return "aarch64";
    default:
      throw new Error(`Unsupported architecture for AppImage: ${process.arch}`);
  }
}

function packageStep(version: string): PackageStep {
  switch (process.platform) {
    case "win32":
      return {
        label: "bun windows/build.ts",
        command: [process.execPath, join(ROOT, "windows", "build.ts")],
        cwd: ROOT,
        artifact: `SidCraft-${version}-windows-x86_64.zip`,
      };
    case "linux":
      return {
        label: "cd linux && bun run appimage build",
        command: [process.execPath, "run", "appimage", "build"],
        cwd: join(ROOT, "linux"),
        artifact: `SidCraft-${version}-linux-${appImageArch()}.AppImage`,
      };
    case "darwin":
      return {
        label: "cd macos && bun run build",
        command: [process.execPath, "run", "build"],
        cwd: join(ROOT, "macos"),
        artifact: `SidCraft-${version}-macos.dmg`,
      };
    default:
      throw new Error(`Unsupported platform: ${process.platform}`);
  }
}

async function run(label: string, command: string[], cwd: string): Promise<void> {
  console.log(`==> ${label}`);

  const child = Bun.spawn(command, {
    cwd,
    stdout: "inherit",
    stderr: "inherit",
  });
  const exitCode = await child.exited;

  if (exitCode !== 0) {
    throw new Error(`${label} failed with exit code ${exitCode}`);
  }
}

async function main(): Promise<void> {
  if (Bun.argv.includes("--help") || Bun.argv.includes("-h")) {
    console.log(USAGE);
    return;
  }

  const version = await readVersion();
  const step = packageStep(version);

  if (!Bun.which("cargo")) {
    throw new Error("cargo was not found in PATH");
  }

  await run("cargo build --release", ["cargo", "build", "--release"], ROOT);
  await run(step.label, step.command, step.cwd);

  const artifact = join(DIST, step.artifact);
  if (!(await Bun.file(artifact).exists())) {
    throw new Error(`Expected package was not created: ${artifact}`);
  }

  console.log(`\nCreated ${artifact}`);
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`error: ${message}`);
  process.exit(1);
});
