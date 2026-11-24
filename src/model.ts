// Mirrors src-tauri/src/model.rs. Unknown fields are preserved by the core.
import type { AddPlan, Analysis, Loader, SearchHit, Source } from "./api";

export type SourcePreference = Source | "mixed";
/** Which build an unpinned mod uses: newest release first, or newest of any channel. */
export type ChannelPolicy = "stable" | "newest";
export type EntryKind = "mod" | "resourcepack" | "shader" | "datapack";
export type Reason = "user" | "dependency" | "payload";

/** A payload candidate from a search hit (both site ids when the hit is merged). */
export function candidateFromHit(h: SearchHit): import("./api").PCandidate {
  const c: import("./api").PCandidate = { name: h.name, iconUrl: h.iconUrl ?? undefined, ...(h.kind && h.kind !== "mod" ? { kind: h.kind } : {}) };
  for (const [src, id] of [[h.source, h.id], [h.also?.source, h.also?.id]] as const) {
    if (src === "modrinth" && id) c.modrinthId = id;
    if (src === "curseforge" && id) c.curseforgeId = Number(id);
  }
  return c;
}

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
  /** Attached override templates, highest priority first (top wins clashes). */
  overrideTemplates: string[];
  /** Templates: `files` holds the hashes as of the last review, to list what changed. */
  appliedTemplates: AppliedTemplate[];
  snapshots: unknown[];
  dismissedSuggestions: string[];
  /** Also check CurseForge copies' required deps (extra API calls; off by default). */
  crossCheckDeps?: boolean;
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

/** Resolver id for a hit: Modrinth ids as-is, CurseForge ids "cf:"-prefixed. */
export const catalogId = (source: Source, id: string) => (source === "curseforge" ? `cf:${id}` : id);

/** Resolver id of an entry on its preferred site. */
export function entryCatalogId(e: PackEntry): string | null {
  if (e.ref.preferred === "curseforge") return e.ref.curseforgeId != null ? `cf:${e.ref.curseforgeId}` : null;
  return e.ref.modrinthId ?? null;
}

export function entryFromHit(h: SearchHit): PackEntry {
  const ref: ModRef = h.source === "modrinth" ? { modrinthId: h.id, preferred: "modrinth" } : { curseforgeId: Number(h.id), preferred: "curseforge" };
  if (h.also?.source === "modrinth") ref.modrinthId = h.also.id;
  if (h.also?.source === "curseforge") ref.curseforgeId = Number(h.also.id);
  return {
    ref,
    kind: h.kind ?? "mod", reason: "user", pulledInBy: [], payloadIds: [], tags: [], addedAt: nowIso(),
    display: {
      name: h.name, slug: h.slug, summary: h.summary, iconUrl: h.iconUrl, categories: h.categories, clientSide: h.clientSide,
      serverSide: h.serverSide, license: h.license, updated: h.updated, downloads: h.downloads,
    },
  };
}

/** Finds the entry key holding this project (on either site), if any. */
export function findEntry(pack: Pack, source: Source, id: string): string | undefined {
  return Object.keys(pack.entries).find((k) => {
    const r = pack.entries[k].ref;
    return source === "modrinth" ? r.modrinthId === id : r.curseforgeId != null && String(r.curseforgeId) === id;
  });
}

/** Entry for a resolver id ("cf:<n>" for CurseForge, otherwise Modrinth). */
export const findByCatalogId = (pack: Pack, id: string) =>
  id.startsWith("cf:") ? findEntry(pack, "curseforge", id.slice(3)) : findEntry(pack, "modrinth", id);

/** Entry for a search hit, checking its other-site twin too. */
export const findHit = (pack: Pack, h: SearchHit) => findEntry(pack, h.source, h.id) ?? (h.also ? findEntry(pack, h.also.source, h.also.id) : undefined);

/** Switches which site an entry uses. Its pin belongs to the old site, so it's cleared. */
export function withSource(e: PackEntry, source: Source): PackEntry {
  const has = source === "modrinth" ? !!e.ref.modrinthId : e.ref.curseforgeId != null;
  if (!has || e.ref.preferred === source) return e;
  return { ...e, ref: { ...e.ref, preferred: source }, fileId: undefined };
}

/** Applies a pack-wide source preference to every entry linked on both sites. */
export function applyPreference(p: Pack, pref: SourcePreference): Pack {
  if (pref === "mixed") return { ...p, sourcePreference: pref };
  const entries = Object.fromEntries(Object.entries(p.entries).map(([k, e]) => [k, withSource(e, pref)]));
  return { ...p, sourcePreference: pref, entries };
}

export const preferredSource = (p: Pack): Source => (p.sourcePreference === "curseforge" ? "curseforge" : "modrinth");

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
  for (const l of plan.idLinks ?? []) {
    const e = entries[l.key];
    if (!e) continue;
    entries[l.key] = { ...e, ref: { ...e.ref, ...(l.modrinthId ? { modrinthId: l.modrinthId } : {}), ...(l.curseforgeId != null ? { curseforgeId: l.curseforgeId } : {}) } };
  }
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

/** A pack's record of an override template: the version (and file hashes) it last reviewed. */
export interface AppliedTemplate { id: string; version: number; files?: Record<string, string> }

/** Content kinds, for labels and the Browse picker. */
export const KINDS: { id: import("./api").EntryKindT; label: string; one: string; search: string }[] = [
  { id: "mod", label: "Mods", one: "mod", search: "Search mods…" },
  { id: "resourcepack", label: "Resource packs", one: "resource pack", search: "Search resource packs…" },
  { id: "shader", label: "Shaders", one: "shader", search: "Search shaders…" },
  { id: "datapack", label: "Datapacks", one: "datapack", search: "Search datapacks…" },
];
export const kindLabel = (k: import("./api").EntryKindT | undefined) => KINDS.find((x) => x.id === (k ?? "mod"))!.one;
