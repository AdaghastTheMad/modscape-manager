// Typed wrappers over the Rust core's Tauri commands. The webview makes no
// network calls of its own; everything goes through here.
import { invoke } from "@tauri-apps/api/core";
import type { AppliedTemplate, Pack, PackEntry, PackSummary } from "./model";

export type Source = "modrinth" | "curseforge";
export type Loader = "neoforge" | "fabric" | "forge" | "quilt";
export type Sort = "relevance" | "downloads" | "follows" | "updated" | "newest";

export interface AppError { code: string; message: string; help?: string | null }

export interface KeyStatus {
  keychainAvailable: boolean;
  hasKey: boolean;
  backend: string;
  problem?: { message: string; help: string } | null;
}

export interface SearchQuery {
  text: string;
  gameVersion?: string;
  loader?: Loader;
  categories?: string[];
  side?: "client" | "server";
  sort?: Sort;
  includeIncompatible?: boolean;
  limit?: number;
  offset?: number;
  force?: boolean;
  /** Mods (default), resource packs, shaders or datapacks. */
  kind?: EntryKindT;
}

export interface Freshness { fetchedAt: number; fromCache: boolean; stale: boolean }

export interface SearchHit {
  source: Source;
  id: string;
  /** Set by the UI: which content tab the hit came from (adds it as that kind). */
  kind?: EntryKindT;
  slug: string;
  name: string;
  summary: string;
  authors: string[];
  downloads: number;
  follows?: number | null;
  iconUrl?: string | null;
  updated?: string | null;
  categories: string[];
  license?: string | null;
  clientSide?: string | null;
  serverSide?: string | null;
  compatible?: boolean | null;
  /** CurseForge: the author blocks third-party downloads. */
  distributionBlocked?: boolean;
  /** The same mod on the other site (merged search). */
  also?: AltRef | null;
}

export interface AltRef { source: Source; id: string; slug: string; name: string; downloads: number }
export interface PerSource<T> { modrinth?: T | null; curseforge?: T | null }
export interface MergedPage { hits: SearchHit[]; totals: PerSource<number>; next: PerSource<number>; notes: string[]; stale: boolean }
export type Confidence = "manual" | "verified" | "likely" | "possible";
export interface Candidate { source: Source; id: string; slug: string; name: string; iconUrl?: string | null; downloads: number; confidence: Confidence; reason: string }
export interface EntryMatches { key: string; candidates: Candidate[] }

export interface SearchPage { source: Source; total: number; hits: SearchHit[]; freshness?: Freshness | null }

export interface ProjectDetails {
  source: Source; id: string; slug: string; name: string; summary: string; body: string;
  license?: { id: string; name: string; url?: string | null } | null;
  pageUrl: string; links: [string, string][]; gameVersions: string[]; loaders: string[]; categories: string[];
  clientSide?: string | null; serverSide?: string | null; downloads: number; follows: number;
  updated?: string | null; published?: string | null; iconUrl?: string | null; freshness: Freshness; distributionBlocked?: boolean;
}

export interface Category { name: string; iconSvg: string }
export interface GameVersion { version: string; version_type: "release" | "snapshot" | "alpha" | "beta" | string; date: string; major: boolean }
export interface LoaderVersions { versions: { version: string; stable: boolean }[]; note?: string | null }
export interface AppInfo { version: string; userAgent: string; os: string; allowedHosts: string[] }
export interface Saved { pack: Pack; notes: string[] }
export interface CacheStats { entries: number; bytes: number }

// ---- resolver ----
export interface FileInfo { versionId: string; versionNumber: string; versionType: string; datePublished: string; filename?: string | null; loaders: string[]; pinned: boolean }
export interface DepLink { projectId: string; name: string; inPack?: string | null; versionId?: string | null }
export interface EntryReport { file?: FileInfo | null; required: DepLink[]; optional: DepLink[]; incompatible: DepLink[]; requiredBy: string[]; checked: boolean }
export type Severity = "error" | "warning" | "info";
export type Fix =
  | { kind: "addProject"; projectId: string; name: string; pinVersion?: string | null; forKey?: string | null }
  | { kind: "remove"; key: string }
  | { kind: "unpin"; key: string }
  | { kind: "pinTo"; key: string; versionId: string; versionNumber: string }
  | { kind: "useSource"; key: string; source: Source }
  | { kind: "setKind"; key: string; to: EntryKindT };
export interface Issue { id: string; severity: Severity; kind: string; entryKey?: string | null; otherKey?: string | null; message: string; fix?: Fix | null }
export interface Suggestion { projectId: string; name: string; summary: string; iconUrl?: string | null; suggestedBy: string[] }
export interface Analysis { entries: Record<string, EntryReport>; issues: Issue[]; suggestions: Suggestion[]; complete: boolean }
export interface AddPlan {
  rootKey: string; additions: { key: string; entry: PackEntry }[]; links: { key: string; parent: string }[];
  promote?: string | null; idLinks: { key: string; modrinthId?: string | null; curseforgeId?: number | null }[];
  suggestions: Suggestion[]; notes: string[];
}

// ---- export / import ----
export type ResetArea = "packs" | "payloads" | "templates" | "links" | "cache" | "scanCache" | "curseforgeKey";
export interface ScanEstimate { files: number; downloadBytes: number; cached: number; unavailable: string[] }
export interface ScanReport { jars: number; modIds: number; issues: Issue[]; notes: string[]; skipped: string[] }
export type ExportFormat = "mrpack" | "curseforgeZip" | "packwiz" | "markdown" | "csv";
export interface ExportFile { key: string; name: string; source: Source; kind: EntryKindT; path: string; projectId: string; fileId: string; versionNumber: string; url: string; size: number; distributionBlocked: boolean }
export interface Excluded { key: string; name: string; reason: string }
export interface ExportProblem { severity: Severity; message: string; fixes: Fix[]; fixLabel?: string | null }
export interface ExportPlan { format: ExportFormat; files: ExportFile[]; excluded: Excluded[]; problems: ExportProblem[]; loaderLabel: string; canExport: boolean; overrides: number; server: boolean }
export interface ExportOptions { server?: boolean }

// ---------- override templates (M9) ----------
export interface TemplateFile { path: string; size: number; editable: boolean; modHint: string | null }
export interface Template {
  id: string; name: string; description: string; version: number; createdAt: string; updatedAt: string;
  files: TemplateFile[]; folders: string[]; bytes: number; folder: string;
}
export interface FileText { path: string; size: number; text: string | null }
export interface SourceEntry { name: string; isDir: boolean; files: number; bytes: number; suggested: boolean }
export interface SourceScan { kind: "folder" | "instance" | "mrpack" | "curseforge"; path: string; suggestedName: string; entries: SourceEntry[]; notes: string[] }
export interface MergedFile { path: string; from: string[]; size: number }
export interface TemplateChange { id: string; name: string; fromVersion: number; toVersion: number; added: string[]; changed: string[]; removed: string[] }
export interface ModUse { hint: string; entry: string | null; entryName: string | null; files: string[] }
export interface Overview { attached: Template[]; missing: string[]; merged: MergedFile[]; clashes: number; updates: TemplateChange[]; mods: ModUse[] }
export interface ImportReport { pack: Pack; notes: string[]; unknown: string[]; hasOverrides: boolean; overrideSource: SourceScan | null }

// ---- duplicate to another version ----
export interface PortTarget { mcVersion: string; loader: Loader; loaderVersion: string; name?: string; keepMissing: boolean; keep: string[] }
export interface DroppedDep { key: string; name: string; hasBuild: boolean; wasNeededBy: string[]; library: boolean }
export interface PortReport {
  ported: string[]; missing: { key: string; name: string; otherSite?: Source | null; neededBy: string[] }[];
  changedDeps: { key: string; name: string; added: string[]; removed: string[] }[];
  newDeps: string[]; droppedDeps: DroppedDep[]; kept: string[]; pinsDropped: number; notes: string[];
}
export interface PortPlan { pack: Pack; report: PortReport }

// ---- payloads ----
export type EntryKindT = "mod" | "resourcepack" | "shader" | "datapack";
export interface PCandidate {
  name: string; modrinthId?: string; curseforgeId?: number; iconUrl?: string; loaders?: Loader[]; mcVersions?: string[]; kind?: EntryKindT;
  /** Exactly this file (Modrinth version id / CurseForge file id), and its version number for display. */
  pinVersion?: string; pinNumber?: string;
  /** Use the pinned file even if it isn't built for the pack (another mod needs exactly it). */
  forcePin?: boolean;
}
export interface AnyVersion { versionId: string; versionNumber: string; versionType: string; gameVersions: string[]; loaders: string[]; date: string }
export interface PSlot { id: string; label: string; candidates: PCandidate[] }
export interface PChoiceOption { id: string; label: string; slots: PSlot[] }
export type PItem = ({ kind: "mod" } & PSlot) | { kind: "choice"; id: string; label: string; options: PChoiceOption[]; common: PSlot[] };
export interface Payload { id: string; name: string; description: string; items: PItem[]; includes: string[]; version: number; starter: boolean; createdAt: string; updatedAt: string; [extra: string]: unknown }
export type SlotStatus =
  | { status: "inPack"; key: string; name: string }
  | { status: "add"; projectId: string; altId?: string | null; name: string; iconUrl?: string | null; fallback: boolean; kind: EntryKindT; pinVersion?: string; pinNumber?: string; forcePin?: boolean }
  | { status: "noBuild"; tried: string[] }

export type PlanRow = { slotId: string; from: string; choice?: string | null; label: string } & SlotStatus;
export interface ChoiceView { id: string; label: string; from: string; options: [string, string][]; chosen: string; otherOptionInPack: string[] }
export interface RemovedRow { key: string; name: string; neededBy: string[]; otherPayloads: string[]; userAdded: boolean; suggestRemove: boolean }
export interface PayloadPlan { payloadId: string; name: string; version: number; applied: [string, number][]; rows: PlanRow[]; choices: ChoiceView[]; removed: RemovedRow[]; notes: string[] }
export interface PayloadUpdate { payloadId: string; name: string; appliedVersion: number; currentVersion: number }
export interface SavedPayload { payload: Payload; notes: string[] }

export const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e) return e as AppError;
  return { code: "unknown", message: String(e) };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri()) throw { code: "no_core", message: "Not running inside the desktop app (the Rust core is unavailable)." } as AppError;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  keyStatus: () => call<KeyStatus>("key_status"),
  saveCurseforgeKey: (key: string) => call<KeyStatus>("save_curseforge_key", { key }),
  clearCurseforgeKey: () => call<KeyStatus>("clear_curseforge_key"),
  search: (source: Source, query: SearchQuery) => call<SearchPage>("search", { source, query }),
  projectDetails: (source: Source, id: string, force = false) => call<ProjectDetails>("project_details", { source, id, force }),
  categories: (force = false, kind: EntryKindT = "mod") => call<Category[]>("categories", { force, kind }),
  gameVersions: (force = false) => call<GameVersion[]>("game_versions", { force }),
  loaderVersions: (loader: Loader, mcVersion: string, force = false) => call<LoaderVersions>("loader_versions", { loader, mcVersion, force }),
  icon: (url: string) => call<string>("icon", { url }),
  listPacks: () => call<PackSummary[]>("list_packs"),
  getPack: (id: string) => call<Pack>("get_pack", { id }),
  savePack: (pack: Pack) => call<Saved>("save_pack", { pack }),
  deletePack: (id: string) => call<void>("delete_pack", { id }),
  exportProject: (pack: Pack) => call<string | null>("export_project", { pack, exportedAt: new Date().toISOString() }),
  importProject: () => call<Saved | null>("import_project"),
  pickIcon: () => call<string | null>("pick_icon"),
  analyzePack: (pack: Pack, force = false) => call<Analysis>("analyze_pack", { pack, force }),
  /** forKey: add it as a dependency of that entry instead of as the user's own pick. */
  /** projectId: Modrinth id, or "cf:<id>" for CurseForge. altId: the same mod on the other site. */
  planAdd: (pack: Pack, projectId: string, pinVersion?: string | null, forKey?: string | null, altId?: string | null, kind?: EntryKindT, forcePin = false) =>
    call<AddPlan>("plan_add", { pack, projectId, pinVersion: pinVersion ?? null, forKey: forKey ?? null, altId: altId ?? null, addedAt: new Date().toISOString(), kind: kind ?? "mod", forcePin }),
  allVersions: (projectId: string) => call<AnyVersion[]>("all_versions", { projectId }),
  searchAll: (query: SearchQuery, sources: Source[], prefer: Source, mrOffset = 0, cfOffset = 0) =>
    call<MergedPage>("search_all", { query, sources, prefer, mrOffset, cfOffset }),
  findMatches: (pack: Pack, key: string) => call<Candidate[]>("find_matches", { pack, key }),
  /** Remember that two projects are the same, so matching offers the link first from now on. */
  rememberLink: (modrinthId: string, curseforgeId: number) => call<void>("remember_link", { modrinthId, curseforgeId }),
  forgetLink: (modrinthId: string, curseforgeId: number) => call<void>("forget_link", { modrinthId, curseforgeId }),
  matchAll: (pack: Pack) => call<EntryMatches[]>("match_all", { pack }),
  planRemoval: (pack: Pack, keys: string[]) => call<string[]>("plan_removal", { pack, keys }),
  entryVersions: (projectId: string, mcVersion: string, loader: Loader, kind: EntryKindT = "mod") => call<FileInfo[]>("entry_versions", { projectId, mcVersion, loader, kind }),
  planPort: (pack: Pack, target: PortTarget) => call<PortPlan>("plan_port", { pack, target }),
  listPayloads: () => call<Payload[]>("list_payloads"),
  savePayload: (payload: Payload) => call<SavedPayload>("save_payload", { payload }),
  deletePayload: (id: string) => call<void>("delete_payload", { id }),
  restoreStarterPayloads: () => call<number>("restore_starter_payloads"),
  planPayload: (pack: Pack, payloadId: string, choices?: Record<string, string>) => call<PayloadPlan>("plan_payload", { pack, payloadId, choices: choices ?? null }),
  payloadFromEntries: (pack: Pack, keys: string[], name: string) => call<SavedPayload>("payload_from_entries", { pack, keys, name }),
  payloadUpdates: (pack: Pack) => call<PayloadUpdate[]>("payload_updates", { pack }),
  exportPayload: (id: string) => call<string | null>("export_payload", { id }),
  importPayload: () => call<SavedPayload | null>("import_payload"),
  exportPlan: (pack: Pack, format: ExportFormat, options: ExportOptions = {}) => call<ExportPlan>("export_plan", { pack, format, options }),
  exportPack: (pack: Pack, format: ExportFormat, options: ExportOptions = {}) => call<string | null>("export_pack", { pack, format, options }),
  importPackFile: () => call<ImportReport | null>("import_pack_file"),
  importPrismInstance: () => call<ImportReport | null>("import_prism_instance"),
  cacheStats: () => call<CacheStats>("cache_stats"),
  clearCache: () => call<CacheStats>("clear_cache"),
  openExternal: (url: string) => call<void>("open_external", { url }),
  /** Danger zone: put areas back to a fresh install's state. */
  resetData: (areas: ResetArea[]) => call<string[]>("reset_data", { areas }),
  scanEstimate: (pack: Pack) => call<ScanEstimate>("scan_estimate", { pack }),
  scanPack: (pack: Pack) => call<ScanReport>("scan_pack", { pack }),
  scanInstance: (pack: Pack) => call<ScanReport | null>("scan_instance", { pack }),
  scanCacheStats: () => call<[number, number]>("scan_cache_stats"),
  clearScanCache: () => call<void>("clear_scan_cache"),
  listTemplates: () => call<Template[]>("list_templates"),
  createTemplate: (name: string) => call<Template>("create_template", { name }),
  setTemplateMeta: (id: string, name: string, description: string) => call<Template>("set_template_meta", { id, name, description }),
  duplicateTemplate: (id: string) => call<Template>("duplicate_template", { id }),
  deleteTemplate: (id: string) => call<void>("delete_template", { id }),
  readTemplateFile: (id: string, path: string) => call<FileText>("read_template_file", { id, path }),
  writeTemplateFile: (id: string, path: string, text: string) => call<Template>("write_template_file", { id, path, text }),
  deleteTemplateFile: (id: string, path: string) => call<Template>("delete_template_file", { id, path }),
  renameTemplateFile: (id: string, from: string, to: string) => call<Template>("rename_template_file", { id, from, to }),
  addTemplateFiles: (id: string, dir: string) => call<Template | null>("add_template_files", { id, dir }),
  openTemplatePath: (id: string, path?: string) => call<void>("open_template_path", { id, path: path ?? null }),
  pickTemplateSource: (folder: boolean) => call<SourceScan | null>("pick_template_source", { folder }),
  importTemplate: (path: string, name: string, keep: string[]) => call<Template>("import_template", { path, name, keep }),
  templateOverview: (pack: Pack) => call<Overview>("template_overview", { pack }),
  templateAppliedRef: (id: string) => call<AppliedTemplate>("template_applied_ref", { id }),
  templateUses: () => call<Record<string, string[]>>("template_uses"),
};
