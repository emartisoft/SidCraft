import { $ } from "bun";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  rmSync,
  symlinkSync,
} from "node:fs";
import { basename, join } from "node:path";
import {
  DIST_DIR,
  REPO_ROOT,
  readVersion,
  releaseBinary,
} from "../../config";

const APP_NAME = "SidCraft";
const BINARY_NAME = "sidcraft";
const BUNDLE_ID = "com.emarti.sidcraft";
const MIN_MACOS = "11.0";

export interface MacosBuildOptions {
  target?: "all" | "app" | "dmg";
  sign?: boolean;
  open?: boolean;
}

function requireTool(name: string): void {
  if (!Bun.which(name)) {
    throw new Error(`Required macOS tool not found: ${name}`);
  }
}

function ensurePrerequisites(): void {
  if (process.platform !== "darwin") {
    throw new Error("The macOS packager only runs on macOS");
  }

  for (const tool of ["sips", "iconutil", "codesign", "hdiutil"]) {
    requireTool(tool);
  }

  const binary = releaseBinary();
  if (!existsSync(binary)) {
    throw new Error(`Release binary not found: ${binary}\nRun: cargo build --release`);
  }

  const icon = join(REPO_ROOT, "icon", "sidcraft.png");
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
  const iconset = join(DIST_DIR, `${APP_NAME}.iconset`);
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
      await $`sips -z ${size} ${size} ${source} --out ${join(iconset, name)}`
        .cwd(REPO_ROOT)
        .quiet();
    }
    await $`iconutil -c icns ${iconset} -o ${output}`.cwd(REPO_ROOT).quiet();
  } finally {
    rmSync(iconset, { recursive: true, force: true });
  }
}

async function buildApp(version: string, sign: boolean): Promise<string> {
  const appDir = join(DIST_DIR, `${APP_NAME}.app`);
  const contentsDir = join(appDir, "Contents");
  const macosDir = join(contentsDir, "MacOS");
  const resourcesDir = join(contentsDir, "Resources");
  const binary = releaseBinary();
  const icon = join(REPO_ROOT, "icon", "sidcraft.png");
  const icns = join(resourcesDir, `${APP_NAME}.icns`);

  rmSync(appDir, { recursive: true, force: true });
  mkdirSync(macosDir, { recursive: true });
  mkdirSync(resourcesDir, { recursive: true });

  cpSync(binary, join(macosDir, BINARY_NAME));
  chmodSync(join(macosDir, BINARY_NAME), 0o755);

  await generateIcns(icon, icns);
  await Bun.write(join(contentsDir, "Info.plist"), infoPlist(version));

  if (sign) {
    await $`codesign --force --sign - ${appDir}`.cwd(REPO_ROOT).quiet();
    await $`codesign --verify --deep --strict ${appDir}`
      .cwd(REPO_ROOT)
      .quiet();
  }

  console.log(`Created ${appDir}`);
  return appDir;
}

async function buildDmg(appDir: string, version: string): Promise<string> {
  const stage = join(DIST_DIR, ".dmg-stage");
  const dmg = join(DIST_DIR, `${APP_NAME}-${version}-macos.dmg`);

  rmSync(stage, { recursive: true, force: true });
  rmSync(dmg, { force: true });
  mkdirSync(stage, { recursive: true });

  try {
    cpSync(appDir, join(stage, basename(appDir)), { recursive: true });
    symlinkSync("/Applications", join(stage, "Applications"));

    await $`hdiutil create -volname ${APP_NAME} -srcfolder ${stage} -ov -format UDZO ${dmg}`
      .cwd(REPO_ROOT)
      .quiet();
    await $`hdiutil verify ${dmg}`.cwd(REPO_ROOT).quiet();
  } finally {
    rmSync(stage, { recursive: true, force: true });
  }

  console.log(`Created ${dmg}`);
  return dmg;
}

/** Builds the macOS app bundle and, unless `target` is `app`, the DMG. */
export async function build(options: MacosBuildOptions = {}): Promise<string> {
  const { target = "all", sign = true, open = false } = options;

  ensurePrerequisites();
  mkdirSync(DIST_DIR, { recursive: true });

  const version = await readVersion();
  const appDir = await buildApp(version, sign);

  if (target === "app") {
    if (open) await $`open ${appDir}`.cwd(REPO_ROOT).quiet();
    return appDir;
  }

  const dmg = await buildDmg(appDir, version);
  if (open) await $`open ${dmg}`.cwd(REPO_ROOT).quiet();
  return dmg;
}
