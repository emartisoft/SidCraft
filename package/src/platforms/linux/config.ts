import { homedir } from "node:os";
import { join } from "node:path";
import {
  ASSETS_DIR,
  DIST_DIR,
  REPO_ROOT,
  appImageArch,
  readVersion,
} from "../../config";

export { ASSETS_DIR, DIST_DIR, REPO_ROOT, appImageArch };
export const projectVersion = readVersion;

export const BINARY = join(REPO_ROOT, "target/release/sidcraft");
export const ICON_SOURCE = join(REPO_ROOT, "icon/sidcraft.png");
export const HICOLOR_DIR = join(REPO_ROOT, "icon/hicolor");
export const APPDIR = join(REPO_ROOT, "target/appimage/SidCraft.AppDir");

/** File name of the installed desktop entry, also its GNOME app id. */
export const DESKTOP_ID = "sidcraft.desktop";

export const XDG_DATA_HOME =
  process.env.XDG_DATA_HOME ?? join(homedir(), ".local/share");
export const XDG_CACHE_HOME =
  process.env.XDG_CACHE_HOME ?? join(homedir(), ".cache");

export const ICON_SIZES = [16, 24, 32, 48, 64, 128, 256] as const;
