// Typed wrappers over the Rust core's Tauri commands. The webview makes no
// network calls of its own; everything goes through here.
import { invoke } from "@tauri-apps/api/core";
import type { Pack, PackEntry, PackSummary } from "./model";

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
}

export interface Freshness { fetchedAt: number; fromCache: boolean; stale: boolean }

export interface SearchHit {
  source: Source;
  id: string;
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
}

export interface SearchPage { source: Source; total: number; hits: SearchHit[]; freshness?: Freshness | null }

export interface ProjectDetails {
  source: Source; id: string; slug: string; name: string; summary: string; body: string;
  license?: { id: string; name: string; url?: string | null } | null;
  pageUrl: string; links: [string, string][]; gameVersions: string[]; loaders: string[]; categories: string[];
  clientSide?: string | null; serverSide?: string | null; downloads: number; follows: number;
  updated?: string | null; published?: string | null; iconUrl?: string | null; freshness: Freshness;
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
  | { kind: "pinTo"; key: string; versionId: string; versionNumber: string };
export interface Issue { id: string; severity: Severity; kind: string; entryKey?: string | null; otherKey?: string | null; message: string; fix?: Fix | null }
export interface Suggestion { projectId: string; name: string; summary: string; iconUrl?: string | null; suggestedBy: string[] }
export interface Analysis { entries: Record<string, EntryReport>; issues: Issue[]; suggestions: Suggestion[]; complete: boolean }
export interface AddPlan {
  rootKey: string; additions: { key: string; entry: PackEntry }[]; links: { key: string; parent: string }[];
  promote?: string | null; suggestions: Suggestion[]; notes: string[];
}

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
  categories: (force = false) => call<Category[]>("categories", { force }),
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
  planAdd: (pack: Pack, projectId: string, pinVersion?: string | null, forKey?: string | null) =>
    call<AddPlan>("plan_add", { pack, projectId, pinVersion: pinVersion ?? null, forKey: forKey ?? null, addedAt: new Date().toISOString() }),
  planRemoval: (pack: Pack, keys: string[]) => call<string[]>("plan_removal", { pack, keys }),
  entryVersions: (projectId: string, mcVersion: string, loader: Loader) => call<FileInfo[]>("entry_versions", { projectId, mcVersion, loader }),
  cacheStats: () => call<CacheStats>("cache_stats"),
  clearCache: () => call<CacheStats>("clear_cache"),
  openExternal: (url: string) => call<void>("open_external", { url }),
};
