// Small UI preferences kept in the webview's localStorage (nothing leaves the machine).
import { useEffect, useState } from "react";

const KEY = "modscape-manager.prefs";
export interface Prefs {
  /** Keep the list's grouping and the graph's regions the same. Off by default. */
  syncGrouping: boolean;
  /** Click sound on buttons (and the shortcuts that do the same). Off by default. */
  clickSound: boolean;
  /** Also click when picking an item in a list (mods, payloads, templates…). Off by default. */
  clickSoundLists: boolean;
  /** Dark tiled backdrop behind the panels. */
  backdrop: boolean;
  /** Developer: show the Component kit tab. Off by default. */
  showKit: boolean;
}
const DEFAULTS: Prefs = { syncGrouping: false, clickSound: false, clickSoundLists: false, backdrop: true, showKit: false };

function read(): Prefs {
  try { return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") }; } catch { return DEFAULTS; }
}

const listeners = new Set<(p: Prefs) => void>();

/** Current prefs outside React (e.g. in event handlers). */
export const getPrefs = read;

/** Danger zone: back to the defaults. */
export function resetPrefs() {
  localStorage.removeItem(KEY);
  listeners.forEach((l) => l(DEFAULTS));
}

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
