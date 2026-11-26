#!/usr/bin/env bun

/**
 * One CLI for all SidCraft packaging. The command is selected by the current
 * OS by default; Linux desktop integration and macOS bundle targets are also
 * available explicitly.
 */

import { build as buildWindows } from "./platforms/windows/build";
import { build as buildMacos } from "./platforms/macos/build";
import { build as buildLinux } from "./platforms/linux/build";
import {
  install as installLinux,
  uninstall as uninstallLinux,
} from "./platforms/linux/install";

const HELP = `SidCraft packaging (Bun + TypeScript)

Usage:
  bun run build [options]       Package for the current OS
  bun run app [options]         macOS: build only SidCraft.app
  bun run dmg [options]         macOS: build SidCraft.app and the .dmg
  bun run install [path]        Linux: install desktop entry and icons
  bun run uninstall             Linux: remove desktop integration
  bun run help                  Show this help

Build options:
  --skip-build     Linux: reuse target/release/sidcraft instead of rebuilding
  --no-sign        macOS: skip ad-hoc codesign
  --open           macOS: open the produced .app/.dmg

The final package is written to the repository's dist/ directory.
`;

interface ParsedArgs {
  flags: Set<string>;
  positional: string[];
}

function parseArgs(args: string[]): ParsedArgs {
  return {
    flags: new Set(args.filter((arg) => arg.startsWith("--"))),
    positional: args.filter((arg) => !arg.startsWith("--")),
  };
}

function requirePlatform(platform: NodeJS.Platform, command: string): void {
  if (process.platform !== platform) {
    throw new Error(`'${command}' is only available on ${platform}`);
  }
}

function skipCargoBuild(flags: Set<string>): boolean {
  return flags.has("--skip-build") || Bun.env.SIDCRAFT_SKIP_CARGO_BUILD === "1";
}

async function macosTarget(
  target: "all" | "app" | "dmg",
  flags: Set<string>,
): Promise<string> {
  requirePlatform("darwin", target);
  return buildMacos({
    target,
    sign: !flags.has("--no-sign"),
    open: flags.has("--open"),
  });
}

async function main(): Promise<void> {
  const [command = "build", ...rest] = Bun.argv.slice(2);
  const { flags, positional } = parseArgs(rest);

  switch (command) {
    case "build":
    case "--help":
    case "-h":
    case "help": {
      if (command === "help" || command === "--help" || command === "-h") {
        console.log(HELP);
        return;
      }

      switch (process.platform) {
        case "win32":
          await buildWindows();
          return;
        case "linux":
          await buildLinux({ skipBuild: skipCargoBuild(flags) });
          return;
        case "darwin":
          await macosTarget("all", flags);
          return;
        default:
          throw new Error(`Unsupported platform: ${process.platform}`);
      }
    }

    case "app":
      await macosTarget("app", flags);
      return;

    case "dmg":
      await macosTarget("dmg", flags);
      return;

    case "install":
      requirePlatform("linux", command);
      await installLinux(positional[0]);
      return;

    case "uninstall":
      requirePlatform("linux", command);
      await uninstallLinux();
      return;

    default:
      console.error(`error: unknown command: ${command}\n`);
      console.error(HELP);
      process.exitCode = 2;
  }
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`error: ${message}`);
  process.exit(1);
});
