import type { SearchHit } from "./api";

/**
 * In-app drag and drop. The dragged thing is kept here as well as in
 * dataTransfer, because WKWebView (the macOS app) doesn't reliably hand custom
 * MIME data to drop targets, notably when the drag starts on an image.
 */
export type Dragged = { kind: "hit"; hit: SearchHit } | { kind: "payload"; id: string };
let current: Dragged | null = null;

export function startDrag(e: React.DragEvent, d: Dragged) {
  current = d;
  e.dataTransfer.effectAllowed = "copy";
  e.dataTransfer.setData("text/plain", d.kind === "hit" ? d.hit.name : d.id);
  e.dataTransfer.setData(d.kind === "hit" ? "application/x-mpp-hit" : "application/x-mpp-payload", d.kind === "hit" ? JSON.stringify(d.hit) : d.id);
}
export function endDrag() { current = null; }

/** What's being dragged, if it's ours (call from dragover/drop). */
export function dragged(e: React.DragEvent): Dragged | null {
  if (current) return current;
  const id = e.dataTransfer.getData("application/x-mpp-payload");
  if (id) return { kind: "payload", id };
  const raw = e.dataTransfer.getData("application/x-mpp-hit");
  if (raw) { try { return { kind: "hit", hit: JSON.parse(raw) as SearchHit }; } catch { return null; } }
  return null;
}
/** dragover: accept anything of ours. */
export function acceptDrag(e: React.DragEvent) {
  const t = e.dataTransfer.types;
  if (current || t.includes("application/x-mpp-hit") || t.includes("application/x-mpp-payload")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; }
}
