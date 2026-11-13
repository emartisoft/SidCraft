import { chmod, cp, mkdir, rm, symlink } from "node:fs/promises";
import { join } from "node:path";
import {
  APPDIR,
  ASSETS_DIR,
  BINARY,
  HICOLOR_DIR,
} from "./config";

async function copy(
  source: string,
  destination: string,
  mode?: number,
): Promise<void> {
  await cp(source, destination, { recursive: true });
  if (mode !== undefined) await chmod(destination, mode);
}

/**
 * Builds the AppDir that appimagetool turns into an AppImage.
 *
 * `AppRun` is a symlink to the real binary, so the image contains no shell
 * script at all.
 */
export async function assembleAppDir(): Promise<void> {
  await rm(APPDIR, { recursive: true, force: true });

  await mkdir(join(APPDIR, "usr/bin"), { recursive: true });
  await mkdir(join(APPDIR, "usr/share/applications"), { recursive: true });
  await mkdir(join(APPDIR, "usr/share/icons"), { recursive: true });
  await mkdir(join(APPDIR, "usr/share/metainfo"), { recursive: true });

  await copy(BINARY, join(APPDIR, "usr/bin/sidcraft"), 0o755);

  await copy(
    join(ASSETS_DIR, "sidcraft.desktop"),
    join(APPDIR, "sidcraft.desktop"),
    0o644,
  );
  await copy(
    join(ASSETS_DIR, "sidcraft.desktop"),
    join(APPDIR, "usr/share/applications/sidcraft.desktop"),
    0o644,
  );
  await copy(
    join(ASSETS_DIR, "com.emarti.sidcraft.metainfo.xml"),
    join(APPDIR, "usr/share/metainfo/com.emarti.sidcraft.metainfo.xml"),
    0o644,
  );

  await copy(HICOLOR_DIR, join(APPDIR, "usr/share/icons/hicolor"));

  // Root icon, named after `Icon=` in the desktop entry.
  await copy(
    join(HICOLOR_DIR, "256x256/apps/sidcraft.png"),
    join(APPDIR, "sidcraft.png"),
    0o644,
  );
  await symlink("sidcraft.png", join(APPDIR, ".DirIcon"));

  // No shell entry point: AppRun points straight at the binary.
  await symlink("usr/bin/sidcraft", join(APPDIR, "AppRun"));
}
