// Small UI preferences kept in the webview's localStorage (nothing leaves the machine).
import { useEffect, useState } from "react";

const KEY = "modpack-planner.prefs";
export interface Prefs {
  /** Keep the list's grouping and the graph's regions the same. Off by default. */
  syncGrouping: boolean;
}
const DEFAULTS: Prefs = { syncGrouping: false };

function read(): Prefs {
  try { return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") }; } catch { return DEFAULTS; }
}

const listeners = new Set<(p: Prefs) => void>();

export function setPref<K extends keyof Prefs>(k: K, v: Prefs[K]) {
  const next = { ...read(), [k]: v };
  localStorage.setItem(KEY, JSON.stringify(next));
  listeners.forEach((l) => l(next));
}

export function usePrefs(): Prefs {
  const [p, setP] = useState(read);
  useEffect(() => { listeners.add(setP); return () => { listeners.delete(setP); }; }, []);
  return p;
}
