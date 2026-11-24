// Mirrors src-tauri/src/model.rs. Unknown fields are preserved by the core.
import type { AddPlan, Analysis, Loader, SearchHit, Source } from "./api";

export type SourcePreference = Source | "mixed";
/** Which build an unpinned mod uses: newest release first, or newest of any channel. */
export type ChannelPolicy = "stable" | "newest";
export type EntryKind = "mod" | "resourcepack" | "shader" | "datapack";
export type Reason = "user" | "dependency" | "payload";

export interface ModRef { modrinthId?: string; curseforgeId?: number; preferred: Source }

export interface EntryDisplay {
  name: string; slug: string; summary: string; iconUrl?: string | null; categories: string[];
  clientSide?: string | null; serverSide?: string | null; license?: string | null; updated?: string | null; downloads?: number | null;
}

export interface PackEntry {
  ref: ModRef;
  fileId?: string;
  kind: EntryKind;
  reason: Reason;
  pulledInBy: string[];
  payloadIds: string[];
  tags: string[];
  notes?: string;
  side?: string;
  graphPos?: { x: number; y: number; pinned: boolean };
  display: EntryDisplay;
  /** User override for library dimming; undefined = decide from metadata. */
  libraryOverride?: boolean;
  addedAt: string;
  [extra: string]: unknown;
}

export interface Pack {
  id: string;
  name: string;
  mcVersion: string;
  loader: Loader;
  loaderVersion: string;
  sourcePreference: SourcePreference;
  channelPolicy: ChannelPolicy;
  icon?: string | null;
  entries: Record<string, PackEntry>;
  choices: Record<string, unknown>;
  appliedPayloads: { id: string; version: number }[];
  overrideTemplates: string[];
  appliedTemplates: { id: string; version: number }[];
  snapshots: unknown[];
  dismissedSuggestions: string[];
  createdAt: string;
  updatedAt: string;
  [extra: string]: unknown;
}

export interface PackSummary { id: string; name: string; mcVersion: string; loader: Loader; entryCount: number; updatedAt: string; icon?: string | null }

export const LOADERS: { id: Loader; label: string }[] = [
  { id: "neoforge", label: "NeoForge" }, { id: "fabric", label: "Fabric" }, { id: "forge", label: "Forge" }, { id: "quilt", label: "Quilt" },
];
export const loaderLabel = (l: Loader) => LOADERS.find((x) => x.id === l)?.label ?? l;

export const nowIso = () => new Date().toISOString();

export function newPack(p: Pick<Pack, "name" | "mcVersion" | "loader" | "loaderVersion" | "icon">): Pack {
  const t = nowIso();
  return {
    id: crypto.randomUUID(), sourcePreference: "modrinth", channelPolicy: "stable", entries: {}, choices: {}, appliedPayloads: [], overrideTemplates: [],
    appliedTemplates: [], snapshots: [], dismissedSuggestions: [], createdAt: t, updatedAt: t, ...p,
  };
}

export function entryFromHit(h: SearchHit): PackEntry {
  return {
    ref: h.source === "modrinth" ? { modrinthId: h.id, preferred: "modrinth" } : { curseforgeId: Number(h.id), preferred: "curseforge" },
    kind: "mod", reason: "user", pulledInBy: [], payloadIds: [], tags: [], addedAt: nowIso(),
    display: {
      name: h.name, slug: h.slug, summary: h.summary, iconUrl: h.iconUrl, categories: h.categories, clientSide: h.clientSide,
      serverSide: h.serverSide, license: h.license, updated: h.updated, downloads: h.downloads,
    },
  };
}

/** Finds the entry key holding this project, if any. */
export function findEntry(pack: Pack, source: Source, id: string): string | undefined {
  return Object.keys(pack.entries).find((k) => {
    const r = pack.entries[k].ref;
    return source === "modrinth" ? r.modrinthId === id : String(r.curseforgeId) === id;
  });
}

/**
 * Library detection. Modrinth tags are too coarse to rely on alone (JEI and EMI
 * carry "library" too), so: the user's override wins; otherwise a mod is a
 * library when "library" is its only category, or when it carries the tag and
 * was pulled in as a dependency (milestone 3).
 */
export function isLibrary(e: Pick<PackEntry, "display" | "reason" | "libraryOverride">): boolean {
  if (e.libraryOverride !== undefined) return e.libraryOverride;
  const cats = e.display.categories;
  if (!cats.includes("library")) return false;
  return cats.every((c) => c === "library") || e.reason === "dependency";
}
export const prettyCategory = (c: string) => c.replace(/-/g, " ").replace(/\b\w/g, (m) => m.toUpperCase());

/** Applies a core AddPlan. Defensive: the pack may have changed while the plan was computed. */
export function applyPlan(p: Pack, plan: AddPlan): Pack {
  const entries = { ...p.entries };
  const has = (id?: string) => !!id && Object.values(entries).some((e) => e.ref.modrinthId === id);
  for (const a of plan.additions) {
    if (entries[a.key] || has(a.entry.ref.modrinthId)) continue;
    entries[a.key] = a.entry;
  }
  for (const l of plan.links) {
    const e = entries[l.key];
    if (e && !e.pulledInBy.includes(l.parent)) entries[l.key] = { ...e, pulledInBy: [...e.pulledInBy, l.parent] };
  }
  if (plan.promote && entries[plan.promote]) entries[plan.promote] = { ...entries[plan.promote], reason: "user" };
  return { ...p, entries };
}

/** Keeps "pulled in by" in step with what each mod's chosen file actually requires.
 *  Only from a complete analysis, so an offline check never makes mods look unneeded. */
export function syncPulledInBy(p: Pack, a: Analysis): Pack | null {
  if (!a.complete) return null;
  let changed = false;
  const entries = { ...p.entries };
  for (const [k, e] of Object.entries(entries)) {
    const rep = a.entries[k];
    if (!rep?.checked) continue;
    const want = [...rep.requiredBy].filter((x) => x in entries).sort();
    const have = [...e.pulledInBy].sort();
    if (want.join("|") !== have.join("|")) { entries[k] = { ...e, pulledInBy: want }; changed = true; }
  }
  return changed ? { ...p, entries } : null;
}

export const entryName = (p: Pack, k: string) => p.entries[k]?.display.name ?? "a removed mod";
