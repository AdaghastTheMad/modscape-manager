import cytoscape, { type Core, type ElementDefinition, type NodeSingular } from "cytoscape";
import fcose from "cytoscape-fcose";
import { useEffect, useMemo, useRef, useState } from "react";
import type { Analysis, SearchHit } from "../../api";
import { Button, Select } from "../../components/kit";
import { colourKey, GROUPINGS, groupKey, groupRank, groupTitle, type Grouping } from "../../grouping";
import { loadIconUrl } from "../../hooks";
import { isLibrary, type Pack } from "../../model";

cytoscape.use(fcose);

export interface GraphPos { x: number; y: number; pinned: boolean }

// Category colours (node borders + regions). Unknown groups get a stable colour from the hash.
const KNOWN: Record<string, string> = {
  technology: "#4fe3f0", magic: "#b47cff", adventure: "#3fd66b", worldgen: "#7fb850", mobs: "#e0843c",
  decoration: "#e6b85c", utility: "#9aa7ff", optimization: "#ffc23d", storage: "#c9a06b", food: "#ff8a8a",
  equipment: "#d0d0d8", transportation: "#5cc2ff", "game-mechanics": "#ff6fb0", library: "#7d7d88",
  management: "#8fd6c0", social: "#ffa0e0", economy: "#e6d15c", minigame: "#ff9f5c", cursed: "#a35cff",
  user: "#3fd66b", unneeded: "#7d7d88", untagged: "#7d7d88",
};
const PALETTE = ["#4fe3f0", "#b47cff", "#3fd66b", "#ffc23d", "#ff6fb0", "#5cc2ff", "#e0843c", "#9aa7ff", "#8fd6c0", "#e6b85c"];
function colorOf(group: string) {
  const k = colourKey(group);
  if (KNOWN[k]) return KNOWN[k];
  let h = 0;
  for (const c of k) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return PALETTE[h % PALETTE.length];
}

/** Trackpads send many small, fractional wheel deltas; mice send few large, whole ones. */
function looksLikeMouseWheel(e: WheelEvent) {
  return e.deltaMode !== 0 || (e.deltaX === 0 && Math.abs(e.deltaY) >= 50 && Number.isInteger(e.deltaY));
}

interface Menu { key: string; x: number; y: number }

export type BulkVersionAction = "pin" | "float";

export function Graph({ pack, analysis, selectedKey, grouping, onGrouping, onSelect, onOpen, onRemove, onTogglePin, onTag, onPositions, onDropHit, onBulkVersions, onBulkTag }: {
  pack: Pack; analysis: Analysis | null; selectedKey?: string;
  grouping: Grouping; onGrouping: (g: Grouping) => void;
  onSelect: (k: string | null) => void; onOpen: (k: string) => void; onRemove: (k: string) => void;
  /** Pin/float the mod's file version. */
  onTogglePin: (k: string) => void;
  onTag: (k: string) => void;
  onPositions: (p: Record<string, GraphPos>) => void;
  onDropHit: (h: SearchHit, pos: { x: number; y: number }) => void;
  onBulkVersions: (keys: string[], action: BulkVersionAction) => void;
  onBulkTag: (keys: string[], tag: string) => void;
}) {
  const box = useRef<HTMLDivElement | null>(null);
  const cyRef = useRef<Core | null>(null);
  const [hideLibs, setHideLibs] = useState(false);
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [only, setOnly] = useState("");
  const [menu, setMenu] = useState<Menu | null>(null);
  const [multi, setMulti] = useState<string[]>([]);
  const [bulkTag, setBulkTag] = useState("");
  const cb = useRef({ onSelect, onOpen, onPositions });
  cb.current = { onSelect, onOpen, onPositions };

  // ---- data → elements
  const problems = useMemo(() => {
    const m = new Map<string, "error" | "warning">();
    for (const i of analysis?.issues ?? []) {
      for (const k of [i.entryKey, i.kind === "incompatible" ? i.otherKey : null]) {
        if (!k || i.severity === "info") continue;
        if (i.severity === "error" || !m.has(k)) m.set(k, i.severity === "error" ? "error" : "warning");
      }
    }
    return m;
  }, [analysis]);

  const elements = useMemo<ElementDefinition[]>(() => {
    const els: ElementDefinition[] = [];
    const groups = new Set<string>();
    for (const [k, e] of Object.entries(pack.entries)) {
      const g = groupKey(e, grouping, pack);
      groups.add(g);
      const dependents = analysis?.entries[k]?.requiredBy.length ?? e.pulledInBy.length;
      els.push({
        group: "nodes",
        data: {
          id: k, parent: `grp:${g}`, label: e.display.name, size: Math.round(36 + 10 * Math.sqrt(dependents)),
          color: colorOf(g), iconUrl: e.display.iconUrl ?? undefined, group: g, problem: problems.get(k),
        },
        classes: ["mod", isLibrary(e) ? "lib" : "", problems.get(k) ?? "", e.graphPos?.pinned ? "pinned" : ""].filter(Boolean).join(" "),
      });
    }
    for (const g of groups) {
      els.push({ group: "nodes", data: { id: `grp:${g}`, label: groupTitle(g, grouping), color: colorOf(g), group: g }, classes: "region" });
    }
    const seen = new Set<string>();
    for (const [k, rep] of Object.entries(analysis?.entries ?? {})) {
      if (!pack.entries[k]) continue;
      for (const [list, cls] of [[rep.required, "required"], [rep.optional, "optional"], [rep.incompatible, "incompatible"]] as const) {
        for (const d of list) {
          if (!d.inPack || d.inPack === k || !pack.entries[d.inPack]) continue;
          const id = cls === "incompatible" ? `x:${[k, d.inPack].sort().join(":")}` : `${cls}:${k}:${d.inPack}`;
          if (seen.has(id)) continue;
          seen.add(id);
          els.push({ group: "edges", data: { id, source: k, target: d.inPack }, classes: cls });
        }
      }
    }
    return els;
  }, [pack, analysis, grouping, problems]);

  // ---- create once
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const cy = cytoscape({
      container: el,
      minZoom: 0.12,
      maxZoom: 3,
      // Zoom is handled below (trackpad-aware); drag on empty space pans;
      // Shift+drag draws a selection box.
      userZoomingEnabled: false,
      boxSelectionEnabled: true,
      selectionType: "single",
      style: [
        { selector: "node.mod", style: {
          shape: "rectangle", width: "data(size)", height: "data(size)", "background-color": "#2e2e37",
          "border-width": 3, "border-color": "data(color)", label: "data(label)", "text-valign": "bottom", "text-margin-y": 6,
          color: "#ececf1", "font-family": "Pixelify Sans, monospace", "font-size": 11, "text-outline-color": "#000", "text-outline-width": 2,
          "text-max-width": "110px", "text-wrap": "ellipsis", "min-zoomed-font-size": 7,
        } },
        { selector: "node.mod[icon]", style: {
          "background-image": "data(icon)", "background-fit": "contain", "background-width": "78%", "background-height": "78%",
          "background-image-smoothing": "no",
        } },
        { selector: "node.lib", style: { opacity: 0.65 } },
        { selector: "node.warning", style: { "border-color": "#ffc23d", "border-style": "double", "border-width": 5 } },
        { selector: "node.error", style: { "border-color": "#ff5a52", "border-width": 5 } },
        { selector: "node.pinned", style: { "border-style": "dashed" } },
        { selector: "node.region", style: {
          shape: "round-rectangle", "background-color": "data(color)", "background-opacity": 0.07, "border-width": 1,
          "border-color": "data(color)", "border-opacity": 0.45, "border-style": "dashed", label: "data(label)",
          "text-valign": "top", "text-halign": "center", "text-margin-y": -4, color: "data(color)", "font-family": "Pixelify Sans, monospace",
          "font-size": 15, "text-outline-color": "#000", "text-outline-width": 2, padding: "22px",
          // Regions are scenery: clicks and drags go through them to the canvas,
          // so panning and Shift-drag selection work from anywhere.
          events: "no",
        } },
        { selector: "node.region:selected", style: { "border-width": 1, "background-opacity": 0.07 } },
        { selector: "edge", style: {
          "curve-style": "bezier", width: 1.5, "line-color": "#8e8e96", "target-arrow-color": "#8e8e96", "target-arrow-shape": "triangle",
          "arrow-scale": 0.9, opacity: 0.75,
        } },
        { selector: "edge.optional", style: { "line-style": "dashed", opacity: 0.55 } },
        { selector: "edge.incompatible", style: { "line-color": "#ff5a52", "target-arrow-color": "#ff5a52", "target-arrow-shape": "tee", "source-arrow-shape": "tee", "source-arrow-color": "#ff5a52", width: 3, opacity: 1 } },
        { selector: "node.mod:selected", style: { "border-color": "#5000ff", "border-width": 6 } },
        { selector: ".faded", style: { opacity: 0.1 } },
        { selector: ".chain", style: { opacity: 1 } },
        { selector: ".hidden", style: { display: "none" } },
      ],
    });
    cyRef.current = cy;
    if (import.meta.env.DEV) (window as unknown as { __cy?: Core }).__cy = cy; // dev-only handle for testing
    cy.nodes().unselectify();

    // Selection: Cytoscape handles it natively (click = one, Cmd/Ctrl/Shift-click
    // adds or removes, Shift-drag draws a box). We read the result afterwards.
    const selected = () => cy.nodes("node.mod:selected").toArray().map((n) => n.id());
    const report = () => setTimeout(() => {
      const ids = selected();
      setMulti(ids);
      cb.current.onSelect(ids.length === 1 ? ids[0] : null);
    }, 0);
    cy.on("tap", "node.mod", report);
    cy.on("boxend", report);
    cy.on("tap", (ev) => { if (ev.target === cy) report(); setMenu(null); });
    cy.on("dbltap", "node.mod", (ev) => cb.current.onOpen(ev.target.id()));
    cy.on("cxttap", "node.mod", (ev) => {
      const p = ev.renderedPosition ?? ev.target.renderedPosition();
      setMenu({ key: ev.target.id(), x: p.x, y: p.y });
    });
    // Dragging mods pins them where you leave them (all selected ones move together).
    cy.on("dragfree", "node.mod", (ev) => {
      const moved = (ev.target as NodeSingular).selected() ? cy.nodes("node.mod:selected") : (ev.target as NodeSingular);
      const out: Record<string, GraphPos> = {};
      moved.forEach((n) => {
        n.addClass("pinned");
        n.data("placed", true);
        const p = n.position();
        out[n.id()] = { x: Math.round(p.x), y: Math.round(p.y), pinned: true };
      });
      cb.current.onPositions(out);
    });

    // Navigation that suits trackpads: two-finger scroll pans, pinch zooms;
    // a real mouse wheel zooms; Cmd/Ctrl+scroll always zooms.
    const zoomAt = (factor: number, x: number, y: number) => {
      const level = Math.min(cy.maxZoom(), Math.max(cy.minZoom(), cy.zoom() * factor));
      cy.zoom({ level, renderedPosition: { x, y } });
    };
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const r = el.getBoundingClientRect();
      const x = e.clientX - r.left, y = e.clientY - r.top;
      const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaMode === 2 ? e.deltaY * r.height : e.deltaY;
      if (e.ctrlKey) zoomAt(Math.exp(-dy * 0.01), x, y); // trackpad pinch (Chromium/WebKitGTK) or Ctrl+wheel
      else if (e.metaKey || looksLikeMouseWheel(e)) zoomAt(Math.exp(-dy * 0.0015), x, y);
      else cy.panBy({ x: -e.deltaX, y: -e.deltaY });
    };
    // Safari/WKWebView (the macOS app) reports pinch as gesture events instead.
    let gestureStart = 1;
    const onGestureStart = (e: Event) => { e.preventDefault(); gestureStart = cy.zoom(); };
    const onGestureChange = (e: Event) => {
      e.preventDefault();
      const g = e as Event & { scale: number; clientX: number; clientY: number };
      const r = el.getBoundingClientRect();
      const level = Math.min(cy.maxZoom(), Math.max(cy.minZoom(), gestureStart * g.scale));
      cy.zoom({ level, renderedPosition: { x: (g.clientX ?? r.width / 2 + r.left) - r.left, y: (g.clientY ?? r.height / 2 + r.top) - r.top } });
    };
    el.addEventListener("wheel", onWheel, { passive: false, capture: true });
    el.addEventListener("gesturestart", onGestureStart, { passive: false } as AddEventListenerOptions);
    el.addEventListener("gesturechange", onGestureChange, { passive: false } as AddEventListenerOptions);
    return () => {
      el.removeEventListener("wheel", onWheel, { capture: true } as EventListenerOptions);
      el.removeEventListener("gesturestart", onGestureStart);
      el.removeEventListener("gesturechange", onGestureChange);
      cy.destroy();
      cyRef.current = null;
    };
  }, []);

  // ---- layout: saved nodes stay put; only new ones are placed (or everything unpinned, on request)
  const layoutRun = useRef(0);
  const runLayout = (cy: Core, all: boolean) => {
    const fixed = cy.nodes("node.mod").filter((n) => (all ? n.hasClass("pinned") : !!n.data("placed")));
    const run = ++layoutRun.current;
    const layout = cy.layout({
      // Tuned on the 150-mod fixture: strong compound gravity keeps regions
      // together despite the many edges into the Libraries region.
      name: "fcose", animate: false, randomize: fixed.length === 0, quality: "proof", numIter: 3000,
      nodeSeparation: 70, idealEdgeLength: (e: cytoscape.EdgeSingular) => (e.source().parent().same(e.target().parent()) ? 70 : 220),
      edgeElasticity: (e: cytoscape.EdgeSingular) => (e.source().parent().same(e.target().parent()) ? 0.45 : 0.08),
      nodeRepulsion: () => 6500, nestingFactor: 0.6, gravity: 0.15, gravityCompound: 2.2, gravityRange: 3.8, gravityRangeCompound: 1.2,
      packComponents: true, tile: true, tilingPaddingVertical: 20, tilingPaddingHorizontal: 20,
      fixedNodeConstraint: fixed.map((n) => ({ nodeId: n.id(), position: { ...(n as NodeSingular).position() } })),
    } as cytoscape.LayoutOptions);
    layout.one("layoutstop", () => {
      if (run !== layoutRun.current) return;
      const out: Record<string, GraphPos> = {};
      cy.nodes("node.mod").forEach((n) => {
        n.data("placed", true);
        const p = n.position();
        out[n.id()] = { x: Math.round(p.x), y: Math.round(p.y), pinned: n.hasClass("pinned") };
      });
      cb.current.onPositions(out);
      cy.fit(undefined, 40);
    });
    layout.run();
  };

  // ---- sync elements
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    // Packs saved by an early dev build stacked every mod at 0,0: treat those
    // positions as missing so the normal layout runs (and saves real ones).
    const saved = Object.values(pack.entries).filter((e) => e.graphPos);
    const bogus = saved.length > 1 && saved.every((e) => e.graphPos!.x === 0 && e.graphPos!.y === 0);
    const posOf = (k: string) => (bogus ? undefined : pack.entries[k]?.graphPos);
    if (bogus) cy.nodes("node.mod").forEach((n) => { n.data("placed", false); });
    let needsLayout = false;
    cy.batch(() => {
      const want = new Set(elements.map((e) => e.data.id as string));
      cy.elements().forEach((el) => { if (!want.has(el.id())) el.remove(); });
      const sorted = [...elements].sort((a, b) => rank(a) - rank(b)); // parents, then mods, then edges
      for (const def of sorted) {
        const ex = cy.getElementById(def.data.id as string);
        if (ex.nonempty()) {
          if (ex.isNode() && def.data.parent && ex.data("parent") !== def.data.parent) ex.move({ parent: def.data.parent as string });
          ex.data({ ...def.data, icon: ex.data("icon"), placed: ex.data("placed") });
          const sel = ex.selected();
          ex.classes(def.classes as string);
          if (sel) ex.select();
          continue;
        }
        const pos = def.group === "nodes" ? posOf(def.data.id as string) : undefined;
        // Cytoscape keeps the data object it's given, so hand it a copy: our
        // element definitions are shared (and React mounts twice in dev).
        const added = cy.add({ ...def, data: { ...def.data, placed: !!pos }, position: pos ? { x: pos.x, y: pos.y } : undefined });
        if (def.classes === "region") added.unselectify();
        if (!pos && def.group === "nodes" && def.classes?.includes("mod")) needsLayout = true;
      }
    });
    cy.nodes("node.mod").forEach((n) => {
      const url = n.data("iconUrl");
      if (url && !n.data("icon")) loadIconUrl(url).then((src) => { if (src && !n.removed()) n.data("icon", src); });
    });
    if (needsLayout || bogus) runLayout(cy, false);
  }, [elements]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- filters
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.batch(() => {
      cy.nodes("node.mod").forEach((n) => {
        const hide = (hideLibs && n.hasClass("lib")) || (onlyProblems && !n.data("problem")) || (!!only && n.data("group") !== only);
        n.toggleClass("hidden", hide);
      });
      cy.nodes("node.region").forEach((r) => { r.toggleClass("hidden", r.children().filter((c) => !c.hasClass("hidden")).empty()); });
    });
  }, [hideLibs, onlyProblems, only, elements]);

  // ---- selection from outside (list, details) + chain highlight for a single mod
  useEffect(() => {
    const cy = cyRef.current;
    if (!cy) return;
    cy.batch(() => {
      cy.elements().removeClass("faded chain");
      const sel = cy.nodes("node.mod:selected");
      if (selectedKey && !(sel.length > 1 && sel.toArray().some((n) => n.id() === selectedKey))) {
        cy.nodes().unselect();
        cy.getElementById(selectedKey).select();
      } else if (!selectedKey && sel.length <= 1) {
        cy.nodes().unselect();
      }
      if (!selectedKey || cy.nodes("node.mod:selected").length !== 1) return;
      const start = cy.getElementById(selectedKey);
      if (start.empty()) return;
      const chain = cy.collection().union(start);
      for (const dir of ["out", "in"] as const) {
        let frontier = cy.collection().union(start);
        const seen = new Set([start.id()]);
        while (frontier.nonempty()) {
          let next = cy.collection();
          frontier.forEach((n) => {
            const edges = (dir === "out" ? n.outgoers("edge.required, edge.optional") : n.incomers("edge.required, edge.optional")) as cytoscape.EdgeCollection;
            edges.forEach((e) => {
              const other = dir === "out" ? e.target() : e.source();
              chain.merge(e);
              if (!seen.has(other.id())) { seen.add(other.id()); chain.merge(other); next = next.union(other); }
            });
          });
          frontier = next;
        }
      }
      chain.merge(start.connectedEdges("edge.incompatible"));
      cy.elements("node.mod, edge").not(chain).addClass("faded");
      chain.addClass("chain");
    });
    if (selectedKey) setMulti((m) => (m.length > 1 && m.includes(selectedKey) ? m : [selectedKey]));
  }, [selectedKey, elements]);

  // ---- bulk position actions
  const setPinned = (keys: string[], pinned: boolean) => {
    const cy = cyRef.current;
    if (!cy) return;
    const out: Record<string, GraphPos> = {};
    keys.forEach((k) => {
      const n = cy.getElementById(k);
      if (n.empty()) return;
      n.toggleClass("pinned", pinned);
      const p = n.position();
      out[k] = { x: Math.round(p.x), y: Math.round(p.y), pinned };
    });
    onPositions(out);
  };
  const unpinAll = () => {
    const cy = cyRef.current;
    if (!cy) return;
    setPinned(cy.nodes("node.mod").map((n) => n.id()), false);
    cy.nodes().unselect();
    setMulti([]);
    onSelect(null);
    runLayout(cy, true);
  };

  const groups = useMemo(() => [...new Set(elements.filter((e) => e.classes === "region").map((e) => e.data.group as string))]
    .sort((a, b) => groupRank(a) - groupRank(b) || a.localeCompare(b)), [elements]);
  const pinnedCount = Object.values(pack.entries).filter((e) => e.graphPos?.pinned).length;
  const menuEntry = menu ? pack.entries[menu.key] : undefined;
  const selection = multi.filter((k) => pack.entries[k]);

  return (
    <div className="graph-wrap">
      <div className="graph-tools row">
        <Select aria-label="Group graph by" value={grouping} onChange={(e) => onGrouping(e.target.value as Grouping)}>
          {GROUPINGS.map((g) => <option key={g.id} value={g.id}>Regions: {g.label}</option>)}
        </Select>
        <Select aria-label="Show one group" value={only} onChange={(e) => setOnly(e.target.value)}>
          <option value="">All groups</option>
          {groups.map((g) => <option key={g} value={g}>{groupTitle(g, grouping)}</option>)}
        </Select>
        <label className="mc-check"><input type="checkbox" checked={hideLibs} onChange={(e) => setHideLibs(e.target.checked)} /> Hide libraries</label>
        <label className="mc-check"><input type="checkbox" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} /> Only problems</label>
        <span className="grow" />
        <Button size="small" onClick={() => cyRef.current?.fit(undefined, 40)}>Fit</Button>
        <Button size="small" onClick={() => cyRef.current && runLayout(cyRef.current, true)} title="Re-arrange everything except pinned (dragged) mods">Re-layout</Button>
        <Button size="small" disabled={!pinnedCount} onClick={unpinAll} title="Release every pinned position and lay the graph out fresh">Unpin all{pinnedCount ? ` (${pinnedCount})` : ""}</Button>
      </div>
      {selection.length > 1 && (
        <div className="graph-bulk row" role="toolbar" aria-label="Selected mods">
          <strong className="pixel">{selection.length} selected</strong>
          <Button size="small" onClick={() => setPinned(selection, true)}>Pin positions</Button>
          <Button size="small" onClick={() => setPinned(selection, false)}>Unpin positions</Button>
          <Button size="small" onClick={() => onBulkVersions(selection, "pin")}>Pin versions</Button>
          <Button size="small" onClick={() => onBulkVersions(selection, "float")}>Float versions</Button>
          <form className="row" style={{ gap: 4 }} onSubmit={(e) => { e.preventDefault(); if (bulkTag.trim()) { onBulkTag(selection, bulkTag.trim()); setBulkTag(""); } }}>
            <input className="mc-input tag-input" placeholder="tag all…" value={bulkTag} onChange={(e) => setBulkTag(e.target.value)} aria-label="Tag all selected mods" />
          </form>
          <span className="grow" />
          <Button size="small" onClick={() => { cyRef.current?.nodes().unselect(); setMulti([]); onSelect(null); }}>Clear</Button>
        </div>
      )}
      <div className="graph-canvas" ref={box} role="application"
        aria-label="Dependency graph. Click a mod to highlight its chain; Cmd/Ctrl-click or Shift-drag to select several; double-click for details; right-click for actions; drag to pin. Two-finger scroll pans, pinch zooms."
        onDragOver={(e) => { if (e.dataTransfer.types.includes("application/x-mpp-hit")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; } }}
        onDrop={(e) => {
          const raw = e.dataTransfer.getData("application/x-mpp-hit");
          const cy = cyRef.current;
          if (!raw || !cy || !box.current) return;
          e.preventDefault();
          const r = box.current.getBoundingClientRect();
          const pan = cy.pan(), zoom = cy.zoom();
          onDropHit(JSON.parse(raw) as SearchHit, { x: (e.clientX - r.left - pan.x) / zoom, y: (e.clientY - r.top - pan.y) / zoom });
        }} />
      {Object.keys(pack.entries).length === 0 && (
        <div className="graph-empty empty-state"><div><div className="pixel">Nothing to graph yet</div><p>Drag a mod from the search results onto this canvas, or press + to add it.</p></div></div>
      )}
      <div className="graph-legend muted">
        <span><i className="lg lg--req" /> requires</span><span><i className="lg lg--opt" /> optional</span><span><i className="lg lg--inc" /> incompatible</span>
        <span>dashed border = pinned · bigger = more dependents · Shift-drag or ⌘/Ctrl-click to select several · pinch to zoom</span>
      </div>
      {menu && menuEntry && (
        <div className="graph-menu mc-panel" style={{ left: menu.x + 8, top: menu.y + 40 }} role="menu" onMouseLeave={() => setMenu(null)}>
          <div className="pixel" style={{ marginBottom: 4 }}>{menuEntry.display.name}</div>
          <button role="menuitem" onClick={() => { onOpen(menu.key); setMenu(null); }}>Details</button>
          <button role="menuitem" onClick={() => { onTogglePin(menu.key); setMenu(null); }}>{menuEntry.fileId ? "Float to newest version" : "Pin current version"}</button>
          <button role="menuitem" onClick={() => { onTag(menu.key); setMenu(null); }}>Add tag…</button>
          {menuEntry.graphPos?.pinned && (
            <button role="menuitem" onClick={() => { setPinned([menu.key], false); setMenu(null); }}>Unpin position</button>
          )}
          <button role="menuitem" className="danger" onClick={() => { onRemove(menu.key); setMenu(null); }}>Remove</button>
        </div>
      )}
    </div>
  );
}

function rank(e: ElementDefinition) {
  if (e.group === "edges") return 2;
  return e.classes === "region" ? 0 : 1;
}
