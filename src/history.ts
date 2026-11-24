// Undo/redo and snapshots for a pack.
import { entryName, loaderLabel, type Pack, type PackEntry } from "./model";

/** A pack with its snapshots left out (what a snapshot and an undo step hold). */
export type PackState = Pack;

export interface Snapshot {
  id: string;
  name: string;
  /** ISO time it was taken. */
  at: string;
  state: PackState;
}

export function stateOf(p: Pack): PackState {
  return { ...p, snapshots: [] };
}

/** Snapshots stored on the pack (older or foreign items are skipped). */
export function snapshotsOf(p: Pack): Snapshot[] {
  return (p.snapshots as unknown[]).filter((s): s is Snapshot => {
    const x = s as Partial<Snapshot> | null;
    return !!x && typeof x.id === "string" && typeof x.name === "string" && !!x.state && typeof x.state === "object" && "entries" in x.state;
  });
}

/**
 * Moves the pack to an earlier/later state for undo, redo or rollback.
 * Snapshots are never undone, and graph positions stay where they are now
 * (moving nodes isn't part of the history; the graph would jump around).
 */
export function restore(target: PackState, current: Pack): Pack {
  const entries: Record<string, PackEntry> = {};
  for (const [k, e] of Object.entries(target.entries)) {
    const now = current.entries[k];
    entries[k] = now ? { ...e, graphPos: now.graphPos } : e;
  }
  return { ...target, id: current.id, entries, snapshots: current.snapshots, updatedAt: current.updatedAt };
}

// ---------------------------------------------------------------- compare
export interface EntryChange { key: string; name: string; changes: string[] }
export interface PackDiff {
  settings: string[];
  added: { key: string; name: string; dependency: boolean }[];
  removed: { key: string; name: string; dependency: boolean }[];
  changed: EntryChange[];
  same: boolean;
}

const list = (xs: string[]) => (xs.length ? xs.join(", ") : "none");

/** What changed going from `from` to `to`. */
export function diffPacks(from: PackState, to: PackState): PackDiff {
  const settings: string[] = [];
  const s = (label: string, a: unknown, b: unknown, fmt: (v: unknown) => string = (v) => String(v ?? "none")) => {
    if (JSON.stringify(a ?? null) !== JSON.stringify(b ?? null)) settings.push(`${label}: ${fmt(a)} → ${fmt(b)}`);
  };
  s("Name", from.name, to.name);
  s("Minecraft", from.mcVersion, to.mcVersion);
  s("Loader", from.loader, to.loader, (v) => loaderLabel(v as Pack["loader"]));
  s("Loader version", from.loaderVersion, to.loaderVersion);
  s("Preferred site", from.sourcePreference, to.sourcePreference);
  s("Builds", from.channelPolicy ?? "stable", to.channelPolicy ?? "stable", (v) => (v === "newest" ? "newest of any kind" : "releases first"));
  s("Override templates", from.overrideTemplates.length, to.overrideTemplates.length, (v) => `${v} attached`);
  if (JSON.stringify(from.overrideTemplates) !== JSON.stringify(to.overrideTemplates) && from.overrideTemplates.length === to.overrideTemplates.length) settings.push("Override templates: order or selection changed");
  s("Payload choices", Object.keys(from.choices).length === 0 ? {} : from.choices, Object.keys(to.choices).length === 0 ? {} : to.choices, (v) => `${Object.keys(v as object).length} made`);
  s("Dependency double-check", !!from.crossCheckDeps, !!to.crossCheckDeps, (v) => (v ? "on" : "off"));

  const added: PackDiff["added"] = [], removed: PackDiff["removed"] = [], changed: EntryChange[] = [];
  const asPack = (st: PackState) => st;
  for (const [k, e] of Object.entries(to.entries)) {
    const old = from.entries[k];
    if (!old) { added.push({ key: k, name: entryName(asPack(to), k), dependency: e.reason === "dependency" }); continue; }
    const c: string[] = [];
    if ((old.fileId ?? null) !== (e.fileId ?? null)) c.push(e.fileId ? (old.fileId ? "pinned to another version" : "version pinned") : "version unpinned (floats to newest)");
    if (old.ref.preferred !== e.ref.preferred) c.push(`now from ${e.ref.preferred === "curseforge" ? "CurseForge" : "Modrinth"}`);
    if (old.ref.modrinthId !== e.ref.modrinthId || old.ref.curseforgeId !== e.ref.curseforgeId) c.push("site links changed");
    if (old.reason !== e.reason) c.push(`${old.reason} → ${e.reason}`);
    if (JSON.stringify(old.tags) !== JSON.stringify(e.tags)) c.push(`tags: ${list(old.tags)} → ${list(e.tags)}`);
    if ((old.notes ?? "") !== (e.notes ?? "")) c.push("notes edited");
    if (old.libraryOverride !== e.libraryOverride) c.push("library setting changed");
    if (JSON.stringify([...old.payloadIds].sort()) !== JSON.stringify([...e.payloadIds].sort())) c.push("payloads changed");
    if (c.length) changed.push({ key: k, name: entryName(asPack(to), k), changes: c });
  }
  for (const [k, e] of Object.entries(from.entries)) {
    if (!to.entries[k]) removed.push({ key: k, name: entryName(asPack(from), k), dependency: e.reason === "dependency" });
  }
  const byName = (a: { name: string }, b: { name: string }) => a.name.localeCompare(b.name);
  added.sort(byName); removed.sort(byName); changed.sort(byName);
  return { settings, added, removed, changed, same: !settings.length && !added.length && !removed.length && !changed.length };
}
