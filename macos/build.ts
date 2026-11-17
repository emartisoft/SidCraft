#!/usr/bin/env bun
import { $ } from "bun";
import { chmodSync, cpSync, existsSync, mkdirSync, rmSync, symlinkSync } from "node:fs";
import { basename, join, resolve } from "node:path";

const ROOT = resolve(import.meta.dir, "..");
const DIST = join(ROOT, "dist");
const APP_NAME = "SidCraft";
const BINARY_NAME = "sidcraft";
const BUNDLE_ID = "com.emarti.sidcraft";
const MIN_MACOS = "11.0";

type Command = "all" | "app" | "dmg" | "help";

const HELP = `Usage: bun macos/build.ts <command> [options]

Commands:
  all    Build ${APP_NAME}.app and ${APP_NAME}-<version>.dmg (default)
  app    Build only ${APP_NAME}.app
  dmg    Build ${APP_NAME}.app and ${APP_NAME}-<version>.dmg
  help   Show this help

Options:
  --no-sign   Skip ad-hoc code signing
  --open      Open the produced artifact`;

function parseArgs() {
  const args = Bun.argv.slice(2);
  const flags = new Set(args.filter((arg) => arg.startsWith("--")));
  const knownFlags = new Set(["--no-sign", "--open", "--help"]);
  const unknownFlags = [...flags].filter((flag) => !knownFlags.has(flag));

  if (unknownFlags.length > 0) {
    throw new Error(`Unknown option(s): ${unknownFlags.join(", ")}`);
  }

  const command = (args.find((arg) => !arg.startsWith("--")) ?? "all") as Command;
  const validCommands = new Set<Command>(["all", "app", "dmg", "help"]);

  if (!validCommands.has(command)) {
    throw new Error(`Unknown command: ${command}`);
  }

  return {
    command,
    sign: !flags.has("--no-sign"),
    open: flags.has("--open"),
    help: flags.has("--help") || command === "help",
  };
}

async function readVersion(): Promise<string> {
  const cargoToml = await Bun.file(join(ROOT, "Cargo.toml")).text();
  const packageSection = cargoToml.match(/^\[package\]([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1] ?? "";
  const version = packageSection.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];

  if (!version) {
    throw new Error("Could not read the package version from Cargo.toml");
  }

  return version;
}

function requireTool(name: string) {
  if (!Bun.which(name)) {
    throw new Error(`Required macOS tool not found: ${name}`);
  }
}

function ensurePrerequisites() {
  if (process.platform !== "darwin") {
    throw new Error("This script only runs on macOS");
  }

  for (const tool of ["sips", "iconutil", "codesign", "hdiutil"]) {
    requireTool(tool);
  }

  const binary = join(ROOT, "target", "release", BINARY_NAME);
  if (!existsSync(binary)) {
    throw new Error(`Release binary not found: ${binary}\nRun: cargo build --release`);
  }

  const icon = join(ROOT, "icon", "sidcraft.png");
  if (!existsSync(icon)) {
    throw new Error(`Application icon not found: ${icon}`);
  }
}

function xmlEscape(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function infoPlist(version: string): string {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>${xmlEscape(APP_NAME)}</string>
  <key>CFBundleDisplayName</key><string>${xmlEscape(APP_NAME)}</string>
  <key>CFBundleExecutable</key><string>${BINARY_NAME}</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleIconFile</key><string>${APP_NAME}.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${xmlEscape(version)}</string>
  <key>CFBundleVersion</key><string>${xmlEscape(version)}</string>
  <key>LSMinimumSystemVersion</key><string>${MIN_MACOS}</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Emarti</string>
  <!-- "Open With" support: Finder sends the chosen file as an AppleEvent,
       which src/startup.rs receives. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Commodore 64 SID music</string>
      <key>CFBundleTypeRole</key><string>Viewer</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>com.emarti.sidcraft.sid</string>
      </array>
      <key>CFBundleTypeExtensions</key>
      <array>
        <string>sid</string>
        <string>mus</string>
      </array>
    </dict>
  </array>
  <key>UTImportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>com.emarti.sidcraft.sid</string>
      <key>UTTypeDescription</key><string>Commodore 64 SID music</string>
      <key>UTTypeConformsTo</key>
      <array>
        <string>public.audio</string>
        <string>public.data</string>
      </array>
      <key>UTTypeTagSpecification</key>
      <dict>
        <key>public.filename-extension</key>
        <array>
          <string>sid</string>
          <string>mus</string>
        </array>
        <key>public.mime-type</key>
        <array>
          <string>audio/prs.sid</string>
          <string>audio/x-sid</string>
        </array>
      </dict>
    </dict>
  </array>
</dict>
</plist>
`;
}

async function generateIcns(source: string, output: string): Promise<void> {
  const iconset = join(DIST, `${APP_NAME}.iconset`);
  const sizes: Array<[number, string]> = [
    [16, "icon_16x16.png"],
    [32, "icon_16x16@2x.png"],
    [32, "icon_32x32.png"],
    [64, "icon_32x32@2x.png"],
    [128, "icon_128x128.png"],
    [256, "icon_128x128@2x.png"],
    [256, "icon_256x256.png"],
    [512, "icon_256x256@2x.png"],
    [512, "icon_512x512.png"],
    [1024, "icon_512x512@2x.png"],
  ];

  rmSync(iconset, { recursive: true, force: true });
  mkdirSync(iconset, { recursive: true });

  try {
    for (const [size, name] of sizes) {
      await $`sips -z ${size} ${size} ${source} --out ${join(iconset, name)}`.cwd(ROOT).quiet();
    }
    await $`iconutil -c icns ${iconset} -o ${output}`.cwd(ROOT).quiet();
  } finally {
    rmSync(iconset, { recursive: true, force: true });
  }
}

async function buildApp(version: string, sign: boolean): Promise<string> {
  const appDir = join(DIST, `${APP_NAME}.app`);
  const contentsDir = join(appDir, "Contents");
  const macosDir = join(contentsDir, "MacOS");
  const resourcesDir = join(contentsDir, "Resources");
  const binary = join(ROOT, "target", "release", BINARY_NAME);
  const icon = join(ROOT, "icon", "sidcraft.png");
  const icns = join(resourcesDir, `${APP_NAME}.icns`);

  rmSync(appDir, { recursive: true, force: true });
  mkdirSync(macosDir, { recursive: true });
  mkdirSync(resourcesDir, { recursive: true });

  cpSync(binary, join(macosDir, BINARY_NAME));
  chmodSync(join(macosDir, BINARY_NAME), 0o755);

  await generateIcns(icon, icns);
  await Bun.write(join(contentsDir, "Info.plist"), infoPlist(version));

  if (sign) {
    await $`codesign --force --sign - ${appDir}`.cwd(ROOT).quiet();
    await $`codesign --verify --deep --strict ${appDir}`.cwd(ROOT).quiet();
  }

  console.log(`Created ${appDir}`);
  return appDir;
}

async function buildDmg(appDir: string, version: string): Promise<string> {
  const stage = join(DIST, ".dmg-stage");
  const dmg = join(DIST, `${APP_NAME}-${version}.dmg`);

  rmSync(stage, { recursive: true, force: true });
  rmSync(dmg, { force: true });
  mkdirSync(stage, { recursive: true });

  try {
    cpSync(appDir, join(stage, basename(appDir)), { recursive: true });
    symlinkSync("/Applications", join(stage, "Applications"));

    await $`hdiutil create -volname ${APP_NAME} -srcfolder ${stage} -ov -format UDZO ${dmg}`.cwd(ROOT).quiet();
    await $`hdiutil verify ${dmg}`.cwd(ROOT).quiet();
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }

  console.log(`Created ${dmg}`);
  return dmg;
}

async function main() {
  const { command, sign, open, help } = parseArgs();

  if (help) {
    console.log(HELP);
    return;
  }

  ensurePrerequisites();
  mkdirSync(DIST, { recursive: true });

  const version = await readVersion();
  const appDir = await buildApp(version, sign);

  if (command === "app") {
    if (open) {
      await $`open ${appDir}`.cwd(ROOT);
    }
    return;
  }

  const dmg = await buildDmg(appDir, version);

  if (open) {
    await $`open ${dmg}`.cwd(ROOT);
  }
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`error: ${message}`);
  process.exit(1);
});
