import { useEffect, useState } from "react";
import { api, inTauri } from "./api";

// Icons are fetched by the core (cached 30 days) and handed over as data URLs.
const iconCache = new Map<string, Promise<string | null>>();
let active = 0;
const queue: (() => void)[] = [];
const MAX_PARALLEL = 6;

function loadIcon(url: string): Promise<string | null> {
  let p = iconCache.get(url);
  if (!p) {
    p = new Promise<string | null>((resolve) => {
      const run = () => {
        active++;
        api.icon(url).then(resolve, () => resolve(null)).finally(() => { active--; queue.shift()?.(); });
      };
      if (active < MAX_PARALLEL) run(); else queue.push(run);
    });
    iconCache.set(url, p);
  }
  return p;
}

export function useIcon(url?: string | null): string | null {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    setSrc(null);
    if (!url || !inTauri()) return;
    let live = true;
    loadIcon(url).then((s) => { if (live) setSrc(s); });
    return () => { live = false; };
  }, [url]);
  return src;
}

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => { const t = setTimeout(() => setV(value), ms); return () => clearTimeout(t); }, [value, ms]);
  return v;
}

export const fmtCompact = new Intl.NumberFormat(undefined, { notation: "compact" });
export const fmtDate = (iso?: string | null) => (iso ? new Date(iso).toLocaleDateString() : "unknown");

/** True when the keyboard event target is a text field (so global shortcuts should stand down). */
export const typingIn = (e: KeyboardEvent) => {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
};
