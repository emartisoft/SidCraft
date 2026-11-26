#!/usr/bin/env bun

// Single-file, package.json-free entry point.
// Usage: bun run createPackage.ts
// Builds the Cargo release binary, then runs the unified package/ project.
// Bun's own APIs are used here instead of Node child_process/fs imports.

const USAGE = `SidCraft release packager

Usage:
  bun run createPackage.ts

1. cargo build --release
2. cd package && bun run build

The packaging project selects the Windows, Linux or macOS builder from the
current OS and writes the result to the repository's dist/ directory.
No root-level package.json is required for this script.
`;

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

function artifactName(version: string): string {
  switch (process.platform) {
    case "win32":
      return `SidCraft-${version}-windows-x86_64.zip`;
    case "linux":
      return `SidCraft-${version}-linux-${appImageArch()}.AppImage`;
    case "darwin":
      return `SidCraft-${version}-macos.dmg`;
    default:
      throw new Error(`Unsupported platform: ${process.platform}`);
  }
}

async function run(
  label: string,
  command: string[],
  cwd: string,
  env: Record<string, string | undefined> = {},
): Promise<void> {
  console.log(`==> ${label}`);

  const child = Bun.spawn(command, {
    cwd,
    env: { ...Bun.env, ...env },
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

  const expectedArtifact = artifactName(await readVersion());

  if (!Bun.which("cargo")) {
    throw new Error("cargo was not found in PATH");
  }

  await run("cargo build --release", ["cargo", "build", "--release"], ROOT);
  await run(
    "cd package && bun run build",
    [process.execPath, "run", "build"],
    join(ROOT, "package"),
    { SIDCRAFT_SKIP_CARGO_BUILD: "1" },
  );

  const artifact = join(DIST, expectedArtifact);
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
