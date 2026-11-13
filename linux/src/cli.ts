import { build } from "./build";
import { install, uninstall } from "./install";

const HELP = `SidCraft Linux AppImage (Bun + TypeScript)

Kullanim:
  bun run appimage build [--skip-build]   AppImage olustur
  bun run appimage install [yol]          .desktop + ikonlari kur
  bun run appimage uninstall              kurulumu kaldir

Kisayollar (linux/ dizininde):
  bun run build
  bun run setup
  bun run remove

Ornekler:
  bun run appimage build
  bun run appimage build --skip-build
  bun run appimage install
  bun run appimage install ../dist/SidCraft-0.1.0-x86_64.AppImage
  bun run appimage uninstall

Notlar:
  - Harici bash script yoktur; tum adimlar TypeScript ile calisir.
  - Ikonlar Bun.Image ile uretilir (harici arac gerekmez).
  - install/uninstall root gerektirmez (~/.local/share kullanilir).
  - GNOME, dock'a sabitlenmis uygulamalari "Show Apps" izgarasinda
    gostermez. install, SidCraft'i favorilerden cikararak izgaraya ekler.
`;

async function main(): Promise<void> {
  const [command, ...args] = process.argv.slice(2);

  switch (command) {
    case "build": {
      const skipBuild = args.includes("--skip-build");
      await build({ skipBuild });
      return;
    }

    case "install": {
      const explicit = args.find((arg) => !arg.startsWith("-"));
      await install(explicit);
      return;
    }

    case "uninstall": {
      await uninstall();
      return;
    }

    case "help":
    case "--help":
    case "-h":
    case undefined: {
      console.log(HELP);
      return;
    }

    default: {
      console.error(`error: unknown command: ${command}\n`);
      console.log(HELP);
      process.exitCode = 2;
    }
  }
}

main().catch((error: unknown) => {
  console.error(
    `error: ${error instanceof Error ? error.message : String(error)}`,
  );
  process.exitCode = 1;
});
