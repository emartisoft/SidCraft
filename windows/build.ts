#!/usr/bin/env bun
import { $ } from "bun";
import { existsSync, mkdirSync } from "node:fs";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const DIST = join(ROOT, "dist");
const BINARY_NAME = "sidcraft.exe";
const BINARY = join(ROOT, "target", "release", BINARY_NAME);

async function readVersion(): Promise<string> {
  const manifest = await Bun.file(join(ROOT, "Cargo.toml")).text();
  const packageSection =
    manifest.match(/^\[package\]([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1] ?? "";
  const version = packageSection.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];

  if (!version) {
    throw new Error("Could not read the package version from Cargo.toml");
  }

  return version;
}

function powerShellLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

async function main(): Promise<void> {
  if (!existsSync(BINARY)) {
    console.log(`Release binary not found: ${BINARY}`);
    console.log("Run: cargo build --release");
    return;
  }

  const version = await readVersion();
  const archiveName = `SidCraft-${version}-windows-x86_64.zip`;
  const archive = join(DIST, archiveName);

  mkdirSync(DIST, { recursive: true });

  const command =
    "Compress-Archive " +
    `-LiteralPath ${powerShellLiteral(BINARY)} ` +
    `-DestinationPath ${powerShellLiteral(archive)} -Force`;
  await $`powershell -NoProfile -Command ${command}`.quiet();

  console.log(`Created ${archive}`);
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`error: ${message}`);
  process.exit(1);
});
