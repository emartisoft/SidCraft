import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import {
  DIST_DIR,
  REPO_ROOT,
  readVersion,
  releaseBinary,
} from "../../config";
import { run } from "../../exec";

function powerShellLiteral(value: string): string {
  return `'${value.replaceAll("'", "''")}'`;
}

/** Creates the Windows ZIP archive from the Cargo release executable. */
export async function build(): Promise<string> {
  const binary = releaseBinary();
  if (!existsSync(binary)) {
    throw new Error(
      `Release binary not found: ${binary}\nRun: cargo build --release`,
    );
  }

  const version = await readVersion();
  const archive = join(DIST_DIR, `SidCraft-${version}-windows-x86_64.zip`);

  mkdirSync(DIST_DIR, { recursive: true });

  const command =
    "Compress-Archive " +
    `-LiteralPath ${powerShellLiteral(binary)} ` +
    `-DestinationPath ${powerShellLiteral(archive)} -Force`;
  await run("powershell", ["-NoProfile", "-Command", command], {
    cwd: REPO_ROOT,
  });

  console.log(`Created ${archive}`);
  return archive;
}
