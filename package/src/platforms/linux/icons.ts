import { mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { HICOLOR_DIR, ICON_SIZES, ICON_SOURCE } from "./config";

interface ImageHandle {
  resize(width: number, height: number): ImageHandle;
  write(path: string): Promise<void>;
}

type ImageConstructor = new (source: string) => ImageHandle;

/**
 * Bun 1.4+ ships `Bun.Image`. It is used instead of ImageMagick/Pillow so the
 * whole packaging pipeline has no external image-tool dependency.
 */
function imageConstructor(): ImageConstructor {
  const ctor = (Bun as unknown as { Image?: ImageConstructor }).Image;
  if (!ctor) {
    throw new Error(
      "this Bun build has no Bun.Image; Bun 1.4 or newer is required",
    );
  }
  return ctor;
}

/**
 * Downscales icon/sidcraft.png into the standard Linux hicolor layout:
 * icon/hicolor/<size>x<size>/apps/sidcraft.png
 */
export async function generateIcons(): Promise<void> {
  if (!(await Bun.file(ICON_SOURCE).exists())) {
    throw new Error(`icon source not found: ${ICON_SOURCE}`);
  }

  const Image = imageConstructor();

  for (const size of ICON_SIZES) {
    const destination = join(
      HICOLOR_DIR,
      `${size}x${size}`,
      "apps",
      "sidcraft.png",
    );
    await mkdir(dirname(destination), { recursive: true });
    await new Image(ICON_SOURCE).resize(size, size).write(destination);
  }
}
