// Typed wrappers over the Rust core's Tauri commands. The webview makes no
// network calls of its own; everything goes through here.
import { invoke } from "@tauri-apps/api/core";

export type Source = "modrinth" | "curseforge";
export type Loader = "neoforge" | "fabric" | "forge" | "quilt";

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
  limit?: number;
  offset?: number;
}

export interface SearchHit {
  source: Source;
  id: string;
  slug: string;
  name: string;
  summary: string;
  authors: string[];
  downloads: number;
  iconUrl?: string | null;
  updated?: string | null;
  categories: string[];
}

export interface SearchPage { source: Source; total: number; hits: SearchHit[] }
export interface AppInfo { version: string; userAgent: string; os: string }

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
};
