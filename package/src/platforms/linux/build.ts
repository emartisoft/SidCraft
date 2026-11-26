import { access, chmod, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { assembleAppDir } from "./appdir";
import { buildAppImage } from "./appimagetool";
import {
  APPDIR,
  BINARY,
  DIST_DIR,
  REPO_ROOT,
  appImageArch,
  projectVersion,
} from "./config";
import { run } from "../../exec";
import { generateIcons } from "./icons";

export interface BuildOptions {
  skipBuild?: boolean;
}

/**
 * Full AppImage pipeline:
 *   1. cargo build --release (unless --skip-build)
 *   2. generate the hicolor icon set with Bun.Image
 *   3. assemble target/appimage/SidCraft.AppDir
 *   4. fetch appimagetool if missing
 *   5. write SidCraft-<version>-linux-<arch>.AppImage to dist/
 */
export async function build(options: BuildOptions = {}): Promise<string> {
  if (!options.skipBuild) {
    console.log("==> cargo build --release");
    await run("cargo", ["build", "--release"], { cwd: REPO_ROOT });
  }

  try {
    await access(BINARY);
  } catch {
    throw new Error(
      `binary not found: ${BINARY} (run without --skip-build)`,
    );
  }

  console.log("==> Generating hicolor icons");
  await generateIcons();

  console.log("==> Assembling AppDir");
  await assembleAppDir();

  const version = await projectVersion();
  const arch = appImageArch();
  const output = join(DIST_DIR, `SidCraft-${version}-linux-${arch}.AppImage`);

  await mkdir(DIST_DIR, { recursive: true });

  console.log(`==> Building ${output}`);
  await buildAppImage(APPDIR, output);
  await chmod(output, 0o755).catch(() => {});

  console.log(`==> Done: ${output}`);
  return output;
}
