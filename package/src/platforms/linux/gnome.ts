import { DESKTOP_ID } from "./config";
import { capture, which } from "../../exec";

const SCHEMA = "org.gnome.shell";
const KEY = "favorite-apps";

export type FavoriteState = "unpinned" | "not-favorite" | "unavailable";

/** Parses the GVariant text form of a string array: `['a.desktop', 'b']`. */
function parseFavorites(value: string): string[] {
  const ids: string[] = [];
  for (const match of value.matchAll(/'((?:\\.|[^'\\])*)'/g)) {
    ids.push(match[1].replace(/\\(['\\])/g, "$1"));
  }
  return ids;
}

/** Serialises app ids back into the GVariant text form `gsettings set` wants. */
function formatFavorites(ids: string[]): string {
  const quoted = ids.map(
    (id) => `'${id.replace(/\\/g, "\\\\").replace(/'/g, "\\'")}'`,
  );
  return `[${quoted.join(", ")}]`;
}

/**
 * Removes SidCraft from the GNOME dock favorites.
 *
 * GNOME Shell's app grid (`AppDisplay._loadApps`) filters out every app that
 * is listed in `org.gnome.shell favorite-apps`, because those apps already
 * appear in the dash. Pinning SidCraft to the dock therefore removes it from
 * "Show Apps"; unpinning it makes the grid list it again.
 *
 * Returns `unavailable` when there is no usable `gsettings`/schema, e.g. on
 * a non-GNOME desktop or a headless machine.
 */
export async function unpinFromFavorites(): Promise<FavoriteState> {
  const gsettings = which("gsettings");
  if (!gsettings) return "unavailable";

  const current = await capture(gsettings, ["get", SCHEMA, KEY]);
  if (current.code !== 0) return "unavailable";

  const favorites = parseFavorites(current.stdout);
  if (!favorites.includes(DESKTOP_ID)) return "not-favorite";

  const remaining = favorites.filter((id) => id !== DESKTOP_ID);
  const written = await capture(gsettings, [
    "set",
    SCHEMA,
    KEY,
    formatFavorites(remaining),
  ]);
  if (written.code !== 0) return "unavailable";

  return "unpinned";
}
