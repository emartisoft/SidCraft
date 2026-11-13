import { chmod, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { appImageArch, XDG_CACHE_HOME } from "./config";
import { run } from "./exec";

const DOWNLOAD_URLS = [
  (arch: string) =>
    `https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage`,
  (arch: string) =>
    `https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-${arch}.AppImage`,
];

interface AppImageTool {
  path: string;
  arch: string;
}

/** Downloads appimagetool into the user cache on first use (no root needed). */
async function ensureAppImageTool(): Promise<AppImageTool> {
  const arch = appImageArch();
  const directory = join(XDG_CACHE_HOME, "sidcraft/appimage-tools");
  const path = join(directory, `appimagetool-${arch}.AppImage`);

  if (await Bun.file(path).exists()) {
    await chmod(path, 0o755).catch(() => {});
    return { path, arch };
  }

  await mkdir(directory, { recursive: true });

  let lastError: unknown;
  for (const url of DOWNLOAD_URLS) {
    try {
      console.log(`==> Downloading appimagetool (${arch})`);
      const response = await fetch(url(arch), { redirect: "follow" });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status} ${response.statusText}`);
      }
      await Bun.write(path, response);
      await chmod(path, 0o755);
      return { path, arch };
    } catch (error) {
      lastError = error;
    }
  }

  throw new Error(
    `could not download appimagetool: ${
      lastError instanceof Error ? lastError.message : String(lastError)
    }`,
  );
}

/** Runs appimagetool on an AppDir, producing the final .AppImage file. */
export async function buildAppImage(
  appDir: string,
  output: string,
): Promise<void> {
  const tool = await ensureAppImageTool();
  await run(tool.path, [appDir, output], {
    // appimagetool is itself an AppImage; extracting avoids a hard FUSE
    // dependency on machines without libfuse2.
    env: { APPIMAGE_EXTRACT_AND_RUN: "1", ARCH: tool.arch },
  });
}
