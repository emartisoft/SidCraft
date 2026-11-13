import {
  chmod,
  copyFile,
  mkdir,
  readdir,
  realpath,
  rm,
  stat,
} from "node:fs/promises";
import { join } from "node:path";
import {
  ASSETS_DIR,
  BINARY,
  DESKTOP_ID,
  DIST_DIR,
  HICOLOR_DIR,
  ICON_SIZES,
  REPO_ROOT,
  XDG_DATA_HOME,
} from "./config";
import { runQuietly, which } from "./exec";
import { unpinFromFavorites } from "./gnome";
import { generateIcons } from "./icons";

const APPLICATIONS_DIR = join(XDG_DATA_HOME, "applications");
const ICONS_DIR = join(XDG_DATA_HOME, "icons/hicolor");
const DESKTOP_FILE = join(APPLICATIONS_DIR, DESKTOP_ID);

async function refreshCaches(): Promise<void> {
  const updateDesktopDatabase = which("update-desktop-database");
  if (updateDesktopDatabase) {
    await runQuietly(updateDesktopDatabase, [APPLICATIONS_DIR]);
  }

  const updateIconCache = which("gtk-update-icon-cache");
  if (updateIconCache) {
    await runQuietly(updateIconCache, ["-f", "-t", ICONS_DIR]);
  }
}

/** Returns the newest SidCraft-*.AppImage in dist/ (or the repository root). */
async function newestAppImage(): Promise<string | undefined> {
  for (const directory of [DIST_DIR, REPO_ROOT]) {
    const entries = await readdir(directory, { withFileTypes: true }).catch(
      () => [],
    );

    const candidates = await Promise.all(
      entries
        .filter(
          (entry) =>
            entry.isFile() &&
            entry.name.startsWith("SidCraft-") &&
            entry.name.endsWith(".AppImage"),
        )
        .map(async (entry) => {
          const path = join(directory, entry.name);
          return { path, mtime: (await stat(path)).mtimeMs };
        }),
    );

    if (candidates.length > 0) {
      candidates.sort((a, b) => b.mtime - a.mtime);
      return candidates[0]?.path;
    }
  }

  return undefined;
}

async function resolveExecutable(explicit?: string): Promise<string> {
  const candidate = explicit ?? (await newestAppImage()) ?? BINARY;
  const path = await realpath(candidate).catch(() => candidate);

  try {
    await stat(path);
  } catch {
    throw new Error(`executable not found: ${path}`);
  }

  await chmod(path, 0o755).catch(() => {});
  return path;
}

/**
 * Installs the desktop entry and hicolor icons into the user's XDG data
 * directories. No root is required and no shell script is involved.
 */
export async function install(explicit?: string): Promise<void> {
  const execPath = await resolveExecutable(explicit);

  await generateIcons();

  for (const size of ICON_SIZES) {
    const directory = join(ICONS_DIR, `${size}x${size}`, "apps");
    await mkdir(directory, { recursive: true });
    await copyFile(
      join(HICOLOR_DIR, `${size}x${size}`, "apps", "sidcraft.png"),
      join(directory, "sidcraft.png"),
    );
  }

  const template = await Bun.file(
    join(ASSETS_DIR, "sidcraft.desktop"),
  ).text();

  // Use replacement functions so `$` in the path is not interpreted.
  let desktop = template
    .replace(/^Exec=.*$/m, () => `Exec="${execPath}" %F`)
    .replace(/^TryExec=.*$/m, () => `TryExec=${execPath}`);
  if (!/^TryExec=/m.test(desktop)) {
    desktop += `TryExec=${execPath}\n`;
  }

  await mkdir(APPLICATIONS_DIR, { recursive: true });
  await Bun.write(DESKTOP_FILE, desktop);
  await chmod(DESKTOP_FILE, 0o644).catch(() => {});

  await refreshCaches();

  // GNOME hides apps that are pinned to the dock from the "Show Apps" grid,
  // so a previously pinned SidCraft would never be listed there.
  const favoriteState = await unpinFromFavorites();

  console.log("==> Installed SidCraft desktop entry and icons");
  console.log(`    Exec: ${execPath}`);
  if (favoriteState === "unpinned") {
    console.log(
      "    GNOME: unpinned SidCraft from the dock so it appears in Show Apps.",
    );
    console.log(
      "    (GNOME does not list pinned apps in the app grid by design.)",
    );
  }
  console.log(
    "    Restart SidCraft. If the icon does not update, log out and back in.",
  );
}

/** Removes the desktop entry and icons installed by `install`. */
export async function uninstall(): Promise<void> {
  await unpinFromFavorites();
  await rm(DESKTOP_FILE, { force: true });

  for (const size of ICON_SIZES) {
    await rm(join(ICONS_DIR, `${size}x${size}`, "apps", "sidcraft.png"), {
      force: true,
    });
  }

  await refreshCaches();
  console.log(
    `==> Removed SidCraft desktop entry and icons from ${XDG_DATA_HOME}`,
  );
}
