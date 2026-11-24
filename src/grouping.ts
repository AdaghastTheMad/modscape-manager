// One grouping vocabulary for the list and the graph, so they can be synced.
import { entryName, isLibrary, prettyCategory, type Pack, type PackEntry } from "./model";

export type Grouping = "category" | "tag" | "pulledInBy";

export const GROUPINGS: { id: Grouping; label: string }[] = [
  { id: "category", label: "category" },
  { id: "tag", label: "first tag" },
  { id: "pulledInBy", label: '"pulled in by"' },
];

// Special group keys (start with "@", which no category/tag/mod name uses here).
const LIB = "@library", USER = "@user", UNNEEDED = "@unneeded", UNTAGGED = "@untagged";

export function groupKey(e: PackEntry, by: Grouping, pack: Pack): string {
  if (by === "pulledInBy") {
    if (e.reason !== "dependency") return USER;
    const parents = e.pulledInBy.filter((k) => k in pack.entries);
    return parents.length ? parents.map((k) => entryName(pack, k)).sort().join(", ") : UNNEEDED;
  }
  if (isLibrary(e)) return LIB;
  if (by === "tag") return e.tags[0] ? `#${e.tags[0]}` : UNTAGGED;
  return e.display.categories.find((c) => c !== "library") ?? "uncategorized";
}

export function groupTitle(g: string, by: Grouping): string {
  if (g === LIB) return "Libraries";
  if (g === USER) return "Added by you";
  if (g === UNNEEDED) return "Dependencies nothing needs";
  if (g === UNTAGGED) return "No tag";
  if (by === "pulledInBy") {
    // Widely shared dependencies (e.g. Fabric API) can have dozens of parents.
    const names = g.split(", ");
    return `Pulled in by ${names.length > 3 ? `${names.slice(0, 3).join(", ")} +${names.length - 3} more` : g}`;
  }
  if (by === "tag") return g;
  return prettyCategory(g);
}

/** Sort order: "added by you" first; libraries / unneeded / untagged last. */
export function groupRank(g: string) {
  if (g === USER) return -1;
  return g === LIB || g === UNNEEDED || g === UNTAGGED ? 1 : 0;
}

export const isLibraryGroup = (g: string) => g === LIB;
/** Category slug for colouring a group (special groups map to their own colour keys). */
export const colourKey = (g: string) => (g === LIB ? "library" : g.replace(/^[@#]/, ""));
