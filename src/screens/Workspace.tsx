import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api, type AddPlan, type Analysis, type AppError, type Candidate, type EntryMatches, type Fix, type SearchHit, type Source, type Suggestion } from "../api";
import { Button, ErrorNotice, Input, Modal, Notice, Select, Tabs } from "../components/kit";
import { typingIn } from "../hooks";
import { applyPlan, applyPreference, catalogId, entryFromHit, entryName, findByCatalogId, findHit, loaderLabel, nowIso, syncPulledInBy, withSource, type ChannelPolicy, type Pack, type PackEntry, type SourcePreference } from "../model";
import { ModIcon, SourceBadge } from "../components/kit";
import { Browse } from "./workspace/Browse";
import { Details, type Selection } from "./workspace/Details";
import { PackList } from "./workspace/PackList";
import { ExportDialog } from "./workspace/ExportDialog";
import { DuplicateDialog } from "./workspace/DuplicateDialog";
import { PayloadDialog, type PayloadApply } from "./workspace/PayloadDialog";
import { PayloadsPane } from "./workspace/PayloadsPane";
import { Overrides } from "./workspace/Overrides";
import type { Overview } from "../api";
import type { PayloadUpdate } from "../api";
import type { BulkVersionAction, GraphPos } from "./workspace/Graph";
import type { Grouping } from "../grouping";
import { usePrefs } from "../prefs";
// Cytoscape is large; load the graph only when it is first opened.
const Graph = lazy(() => import("./workspace/Graph").then((m) => ({ default: m.Graph })));
import type { PackView } from "./workspace/PackList";
import { SuggestionPicker } from "./workspace/Suggestions";

type AddNotice = { title: string; lines: string[]; suggestions: Suggestion[]; tone: "ok" | "warn" | "error" };

export function Workspace({ packId, onClose, onOpen }: { packId: string; onClose: () => void; onOpen: (id: string) => void }) {
  const [pack, setPack] = useState<Pack | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [saveState, setSaveState] = useState<"saved" | "pending" | "error">("saved");
  const [selection, setSelection] = useState<Selection>(null);
  const [undo, setUndo] = useState<{ label: string; run: () => void } | null>(null);
  const [notice, setNotice] = useState<AddNotice | null>(null);
  const [analysis, setAnalysis] = useState<Analysis | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [orphanPrompt, setOrphanPrompt] = useState<{ keys: string[]; orphans: string[]; picked: Set<string> } | null>(null);
  const [matchReview, setMatchReview] = useState<{ results: EntryMatches[]; picked: Map<string, Candidate> } | null>(null);
  const [matching, setMatching] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [duplicating, setDuplicating] = useState(false);
  const [view, setView] = useState<PackView>("list");
  // List grouping and graph regions: independent unless synced in Settings.
  const prefs = usePrefs();
  const [listGrouping, setListGrouping] = useState<Grouping>("category");
  const [graphGrouping, setGraphGrouping] = useState<Grouping>("category");
  const setGrouping = (which: "list" | "graph") => (g: Grouping) => {
    if (prefs.syncGrouping || which === "list") setListGrouping(g);
    if (prefs.syncGrouping || which === "graph") setGraphGrouping(g);
  };
  useEffect(() => { if (prefs.syncGrouping) setGraphGrouping(listGrouping); }, [prefs.syncGrouping]); // eslint-disable-line react-hooks/exhaustive-deps
  const [tagFocus, setTagFocus] = useState(0);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const dirty = useRef(false);
  const latest = useRef<Pack | null>(null);
  const analysisRun = useRef(0);

  useEffect(() => { api.getPack(packId).then(setPack, setError); }, [packId]);

  // State updaters must stay pure (React may run them twice), so anything
  // random or time-based is computed before calling update().
  useEffect(() => { latest.current = pack; }, [pack]);
  const update = useCallback((fn: (p: Pack) => Pack) => {
    const at = nowIso();
    setPack((p) => (p ? { ...fn(p), updatedAt: at } : p));
    dirty.current = true;
    setSaveState("pending");
  }, []);

  // ---- autosave
  const flush = useCallback(async () => {
    if (!dirty.current || !latest.current) return;
    dirty.current = false;
    try { await api.savePack(latest.current); setSaveState("saved"); }
    catch (e) { setSaveState("error"); setError(e as AppError); dirty.current = true; }
  }, []);
  useEffect(() => {
    if (!dirty.current) return;
    const t = setTimeout(flush, 400);
    return () => clearTimeout(t);
  }, [pack, flush]);
  useEffect(() => () => { flush(); }, [flush]);

  // ---- continuous checks: re-run shortly after the mod list or target changes
  const runAnalysis = useCallback(async (force = false) => {
    const p = latest.current;
    if (!p) return;
    const run = ++analysisRun.current;
    setAnalyzing(true);
    try {
      const a = await api.analyzePack(p, force);
      if (run !== analysisRun.current) return; // a newer check superseded this one
      setAnalysis(a);
      // Keep "pulled in by" true to what files actually require.
      const synced = syncPulledInBy(p, a);
      if (synced && latest.current === p) {
        setPack((cur) => (cur === p ? synced : cur));
        dirty.current = true;
      }
    } catch (e) {
      if (run === analysisRun.current) setError(e as AppError);
    } finally {
      if (run === analysisRun.current) setAnalyzing(false);
    }
  }, []);
  const analysisInputs = pack && JSON.stringify([
    pack.mcVersion, pack.loader, pack.channelPolicy, pack.dismissedSuggestions, pack.crossCheckDeps,
    Object.entries(pack.entries).map(([k, e]) => [k, e.ref, e.fileId, e.reason]),
  ]);
  useEffect(() => {
    if (!analysisInputs) return;
    const t = setTimeout(() => runAnalysis(false), 600);
    return () => clearTimeout(t);
  }, [analysisInputs, runAnalysis]);

  // ---- adding (with dependencies)
  const describePlan = (p: Pack, plan: AddPlan): AddNotice => {
    const root = plan.additions.find((a) => a.key === plan.rootKey)?.entry.display.name ?? entryName(p, plan.rootKey);
    const deps = plan.additions.filter((a) => a.key !== plan.rootKey).map((a) => a.entry.display.name);
    const lines = [
      ...(deps.length ? [`Also added ${deps.length} required ${deps.length === 1 ? "dependency" : "dependencies"}: ${deps.join(", ")}.`] : []),
      ...(plan.links.length ? [`Uses ${plan.links.length} mod${plan.links.length === 1 ? "" : "s"} already in the pack.`] : []),
      ...plan.notes,
    ];
    return { title: plan.promote ? `${root} is now marked as added by you` : `Added ${root}`, lines, suggestions: plan.suggestions, tone: plan.notes.length ? "warn" : "ok" };
  };

  const addProject = useCallback(async (projectId: string, pin?: string | null, hit?: SearchHit, forKey?: string | null, altId?: string | null, at?: { x: number; y: number }) => {
    const p = latest.current;
    if (!p || pending.has(projectId)) return;
    setPending((s) => new Set(s).add(projectId));
    try {
      const plan = await api.planAdd(p, projectId, pin, forKey, altId);
      update((cur) => {
        const next = applyPlan(cur, plan);
        // Dropped on the graph: put (and pin) it where it was dropped.
        const root = next.entries[plan.rootKey];
        if (at && root) next.entries = { ...next.entries, [plan.rootKey]: { ...root, graphPos: { x: Math.round(at.x), y: Math.round(at.y), pinned: true } } };
        return next;
      });
      setNotice(describePlan(p, plan));
      if (!forKey) setSelection({ kind: "entry", key: plan.rootKey });
    } catch (e) {
      const err = e as AppError;
      if (hit) {
        // Offline or API trouble: still add the mod; the checks will list what's missing.
        const key = crypto.randomUUID();
        const entry = entryFromHit(hit);
        update((cur) => (findHit(cur, hit) ? cur : { ...cur, entries: { ...cur.entries, [key]: entry } }));
        setNotice({ title: `Added ${hit.name} without its dependencies`, lines: [err.message, "Checks will list anything missing once the connection is back."], suggestions: [], tone: "warn" });
      } else {
        setNotice({ title: "Couldn't add that mod", lines: [err.message, ...(err.help ? [err.help] : [])], suggestions: [], tone: "error" });
      }
    } finally {
      setPending((s) => { const n = new Set(s); n.delete(projectId); return n; });
    }
  }, [pending, update]);

  // Several picks at once (suggestions): plan each against the pack as it will be
  // after the previous ones, then apply them all in one update.
  const [batchAdding, setBatchAdding] = useState(false);
  const [browseMode, setBrowseMode] = useState<"mods" | "payloads">("mods");
  const [payloadDialog, setPayloadDialog] = useState<{ id: string; title?: string } | null>(null);
  const [savePayloadKeys, setSavePayloadKeys] = useState<string[] | null>(null);
  const [savePayloadName, setSavePayloadName] = useState("");
  const [updates, setUpdates] = useState<PayloadUpdate[]>([]);

  // Payloads this pack applied that have changed since.
  const appliedSig = pack ? JSON.stringify(pack.appliedPayloads) : "";
  useEffect(() => {
    if (!latest.current) return;
    api.payloadUpdates(latest.current).then(setUpdates, () => {});
  }, [appliedSig, payloadDialog]); // eslint-disable-line react-hooks/exhaustive-deps

  // Override templates: what goes in overrides/, clashes, updates (refetched when attachments or mods change).
  const [overview, setOverview] = useState<Overview | null>(null);
  const [overviewError, setOverviewError] = useState<AppError | null>(null);
  const tplSig = pack ? JSON.stringify([pack.overrideTemplates, pack.appliedTemplates.map((a) => [a.id, a.version]), Object.keys(pack.entries).length]) : "";
  useEffect(() => {
    if (!latest.current) return;
    if (latest.current.overrideTemplates.length === 0) { setOverview(null); return; }
    api.templateOverview(latest.current).then((o) => { setOverview(o); setOverviewError(null); }, setOverviewError);
  }, [tplSig]); // eslint-disable-line react-hooks/exhaustive-deps

  // Applies a payload preview: each pick is planned against the pack as it
  // will be after the previous ones (dependencies included), then everything
  // lands in one update with the payload recorded on its mods.
  const applyPayload = useCallback(async (a: PayloadApply) => {
    const start = latest.current;
    if (!start) return;
    let p = start;
    const plans: AddPlan[] = [];
    const failed: string[] = [];
    for (const r of a.add) {
      try {
        const plan = await api.planAdd(p, r.projectId, null, null, r.altId);
        plans.push(plan);
        p = applyPlan(p, plan);
      } catch (e) { failed.push(`${r.name}: ${(e as AppError).message}`); }
    }
    const roots = new Set(plans.map((pl) => pl.rootKey));
    const pid = a.plan.payloadId;
    const ids = new Set(a.plan.applied.map(([id]) => id));
    // Removing mods dropped from the payload also removes dependencies only they needed.
    let removeKeys = [...a.remove];
    if (removeKeys.length) {
      try { removeKeys = [...removeKeys, ...(await api.planRemoval(p, removeKeys))]; } catch { /* just the picked ones */ }
    }
    const removedEntries = removeKeys.filter((k) => p.entries[k]).map((k) => [k, p.entries[k]] as const);
    update((cur) => {
      let next = plans.reduce(applyPlan, cur);
      const entries = { ...next.entries };
      for (const k of removeKeys) delete entries[k];
      for (const [k, e] of Object.entries(entries)) {
        if (e.pulledInBy.some((x) => removeKeys.includes(x))) entries[k] = { ...e, pulledInBy: e.pulledInBy.filter((x) => !removeKeys.includes(x)) };
      }
      // Kept ones aren't from this payload any more; with no payload left they're your own mods.
      for (const k of a.untag) {
        const e = entries[k];
        if (!e) continue;
        const payloadIds = e.payloadIds.filter((x) => !ids.has(x));
        entries[k] = { ...e, payloadIds, reason: e.reason === "payload" && payloadIds.length === 0 ? "user" : e.reason };
      }
      for (const k of [...roots, ...a.covers]) {
        const e = entries[k];
        if (!e) continue;
        entries[k] = {
          ...e,
          // New to the pack via this payload → "payload"; mods you'd added before keep "user".
          reason: roots.has(k) && !start.entries[k] ? "payload" : e.reason,
          payloadIds: e.payloadIds.includes(pid) ? e.payloadIds : [...e.payloadIds, pid],
        };
      }
      const applied = new Map(next.appliedPayloads.map((x) => [x.id, x.version]));
      for (const [id, v] of a.plan.applied) applied.set(id, v);
      next = { ...next, entries, choices: { ...next.choices, ...a.choices }, appliedPayloads: [...applied].map(([id, version]) => ({ id, version })) };
      return next;
    });
    const deps = plans.flatMap((pl) => pl.additions.filter((x) => x.key !== pl.rootKey).map((x) => x.entry.display.name));
    if (removedEntries.length) {
      setUndo({
        label: `Removed ${removedEntries.length} mod${removedEntries.length === 1 ? "" : "s"} dropped from ${a.plan.name}`,
        run: () => { update((cur) => ({ ...cur, entries: { ...cur.entries, ...Object.fromEntries(removedEntries.filter(([k]) => !cur.entries[k])) } })); setUndo(null); },
      });
    }
    const added = a.add.length - failed.length;
    const did = [added && `Added ${added} mod${added === 1 ? "" : "s"}`, removedEntries.length && `removed ${removedEntries.length}`].filter(Boolean).join(", ");
    setNotice({
      title: did ? `${did[0].toUpperCase()}${did.slice(1)} (${a.plan.name})` : `Applied ${a.plan.name}`,
      lines: [
        ...(removedEntries.length > a.remove.length ? [`Removed ${removedEntries.length - a.remove.length} dependenc${removedEntries.length - a.remove.length === 1 ? "y" : "ies"} nothing else needed.`] : []),
        ...(a.untag.length ? [`Kept ${a.untag.length} mod${a.untag.length === 1 ? "" : "s"} the payload no longer has.`] : []),
        ...(deps.length ? [`Also added ${deps.length} required ${deps.length === 1 ? "dependency" : "dependencies"}.`] : []),
        ...plans.flatMap((pl) => pl.notes),
        ...failed.map((f) => `Couldn't add ${f}`),
      ],
      suggestions: [],
      tone: failed.length ? "error" : "ok",
    });
  }, [update]);
  const addMany = useCallback(async (ids: string[]) => {
    const start = latest.current;
    if (!start || !ids.length || batchAdding) return;
    setBatchAdding(true);
    setPending((s) => new Set([...s, ...ids]));
    let p = start;
    const plans: AddPlan[] = [];
    const failed: string[] = [];
    try {
      for (const id of ids) {
        if (findByCatalogId(p, id)) continue;
        try {
          const plan = await api.planAdd(p, id);
          plans.push(plan);
          p = applyPlan(p, plan);
        } catch (e) { failed.push(`${id}: ${(e as AppError).message}`); }
      }
      if (plans.length) update((cur) => plans.reduce(applyPlan, cur));
      const roots = plans.map((pl) => pl.additions.find((a) => a.key === pl.rootKey)?.entry.display.name).filter(Boolean) as string[];
      const deps = plans.flatMap((pl) => pl.additions.filter((a) => a.key !== pl.rootKey).map((a) => a.entry.display.name));
      const more = new Map<string, Suggestion>();
      for (const pl of plans) for (const s of pl.suggestions) if (!findByCatalogId(p, s.projectId)) more.set(s.projectId, s);
      setNotice({
        title: roots.length ? `Added ${roots.join(", ")}` : "Nothing new to add",
        lines: [
          ...(deps.length ? [`Also added ${deps.length} required ${deps.length === 1 ? "dependency" : "dependencies"}: ${[...new Set(deps)].join(", ")}.`] : []),
          ...plans.flatMap((pl) => pl.notes),
          ...failed.map((f) => `Couldn't add ${f}`),
        ],
        suggestions: [...more.values()],
        tone: failed.length ? "error" : plans.some((pl) => pl.notes.length) ? "warn" : "ok",
      });
    } finally {
      setBatchAdding(false);
      setPending((s) => { const n = new Set(s); ids.forEach((i) => n.delete(i)); return n; });
    }
  }, [batchAdding, update]);

  const addHit = useCallback((h: SearchHit, at?: { x: number; y: number }) => {
    addProject(catalogId(h.source, h.id), null, h, null, h.also ? catalogId(h.also.source, h.also.id) : null, at);
  }, [addProject]);

  // Graph positions (layout results and drags) are saved with the pack.
  const setPositions = useCallback((ps: Record<string, GraphPos>) => {
    update((p) => {
      let changed = false;
      const entries = { ...p.entries };
      for (const [k, pos] of Object.entries(ps)) {
        const e = entries[k];
        if (!e) continue;
        const g = e.graphPos;
        if (g && g.x === pos.x && g.y === pos.y && g.pinned === pos.pinned) continue;
        entries[k] = { ...e, graphPos: pos };
        changed = true;
      }
      return changed ? { ...p, entries } : p;
    });
  }, [update]);
  const bulkVersions = useCallback((keys: string[], action: BulkVersionAction) => {
    update((p) => {
      const entries = { ...p.entries };
      for (const k of keys) {
        const e = entries[k];
        if (!e) continue;
        const v = action === "pin" ? analysis?.entries[k]?.file?.versionId : undefined;
        if (action === "pin" && !v) continue; // no file chosen yet: nothing to pin
        entries[k] = { ...e, fileId: v };
      }
      return { ...p, entries };
    });
  }, [analysis, update]);
  const bulkTag = useCallback((keys: string[], tag: string) => {
    const t = tag.replace(/^#/, "").trim();
    if (!t) return;
    update((p) => {
      const entries = { ...p.entries };
      for (const k of keys) if (entries[k] && !entries[k].tags.includes(t)) entries[k] = { ...entries[k], tags: [...entries[k].tags, t] };
      return { ...p, entries };
    });
  }, [update]);
  const togglePin = useCallback((k: string) => {
    const e = latest.current?.entries[k];
    if (!e) return;
    if (e.fileId) return pin(k, null);
    const v = analysis?.entries[k]?.file?.versionId;
    if (v) pin(k, v);
  }, [analysis]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- removing (with orphan cleanup)
  const removeKeys = useCallback((keys: string[]) => {
    const p = latest.current;
    if (!p) return;
    const removed: [string, PackEntry][] = keys.filter((k) => p.entries[k]).map((k) => [k, p.entries[k]]);
    if (!removed.length) return;
    update((cur) => {
      const entries = { ...cur.entries };
      for (const [k] of removed) delete entries[k];
      for (const [k, e] of Object.entries(entries)) {
        if (e.pulledInBy.some((x) => keys.includes(x))) entries[k] = { ...e, pulledInBy: e.pulledInBy.filter((x) => !keys.includes(x)) };
      }
      return { ...cur, entries };
    });
    setSelection((s) => (s?.kind === "entry" && keys.includes(s.key) ? null : s));
    const label = removed.length === 1 ? `Removed ${removed[0][1].display.name}` : `Removed ${removed[0][1].display.name} and ${removed.length - 1} more`;
    setUndo({
      label,
      run: () => {
        update((cur) => {
          const entries = { ...cur.entries };
          for (const [k, e] of removed) if (!entries[k]) entries[k] = e;
          return { ...cur, entries };
        });
        setUndo(null);
      },
    });
  }, [update]);

  const remove = useCallback(async (key: string) => {
    const p = latest.current;
    if (!p?.entries[key]) return;
    let orphans: string[] = [];
    try { orphans = await api.planRemoval(p, [key]); } catch { /* no cleanup offer, just remove */ }
    if (orphans.length) setOrphanPrompt({ keys: [key], orphans, picked: new Set(orphans) });
    else removeKeys([key]);
  }, [removeKeys]);
  useEffect(() => { if (!undo) return; const t = setTimeout(() => setUndo(null), 10000); return () => clearTimeout(t); }, [undo]);

  // ---- sites: per-mod source, pack preference, linking copies
  const useSource = useCallback((key: string, source: Source) => {
    update((p) => (p.entries[key] ? { ...p, entries: { ...p.entries, [key]: withSource(p.entries[key], source) } } : p));
  }, [update]);
  const link = useCallback((key: string, c: Candidate) => {
    update((p) => {
      const e = p.entries[key];
      if (!e) return p;
      const ref = c.source === "modrinth" ? { ...e.ref, modrinthId: c.id } : { ...e.ref, curseforgeId: Number(c.id) };
      let next: PackEntry = { ...e, ref };
      if (p.sourcePreference !== "mixed") next = withSource(next, p.sourcePreference);
      return { ...p, entries: { ...p.entries, [key]: next } };
    });
  }, [update]);
  const unlink = useCallback((key: string, source: Source) => {
    update((p) => {
      const e = p.entries[key];
      if (!e) return p;
      const other: Source = source === "modrinth" ? "curseforge" : "modrinth";
      const ref = { ...e.ref };
      if (source === "modrinth") delete ref.modrinthId; else delete ref.curseforgeId;
      const keep = other === "modrinth" ? !!ref.modrinthId : ref.curseforgeId != null;
      if (!keep) return p; // never unlink an entry's only site
      const moved = e.ref.preferred === source;
      return { ...p, entries: { ...p.entries, [key]: { ...e, ref: { ...ref, preferred: moved ? other : e.ref.preferred }, fileId: moved ? undefined : e.fileId } } };
    });
  }, [update]);
  const setPreference = useCallback((pref: SourcePreference) => {
    const before = latest.current;
    if (!before) return;
    const after = applyPreference(before, pref);
    const changed = Object.keys(after.entries).filter((k) => after.entries[k] !== before.entries[k]);
    update((p) => applyPreference(p, pref));
    if (changed.length) {
      const old = Object.fromEntries(changed.map((k) => [k, before.entries[k]]));
      setUndo({
        label: `Switched ${changed.length} mod${changed.length === 1 ? "" : "s"} to ${pref === "modrinth" ? "Modrinth" : "CurseForge"}`,
        run: () => { update((p) => ({ ...p, sourcePreference: before.sourcePreference, entries: { ...p.entries, ...Object.fromEntries(Object.entries(old).filter(([k]) => k in p.entries)) } })); setUndo(null); },
      });
    }
  }, [update]);
  const matchAll = useCallback(async () => {
    const p = latest.current;
    if (!p) return;
    setMatching(true);
    try {
      const results = await api.matchAll(p);
      // Only byte-identical files are pre-ticked; everything else waits for you.
      const picked = new Map<string, Candidate>();
      for (const r of results) if (r.candidates[0]?.confidence === "verified") picked.set(r.key, r.candidates[0]);
      setMatchReview({ results, picked });
    } catch (e) { setError(e as AppError); }
    finally { setMatching(false); }
  }, []);

  // ---- fixes and small edits
  const pin = useCallback((key: string, versionId: string | null) => {
    update((p) => (p.entries[key] ? { ...p, entries: { ...p.entries, [key]: { ...p.entries[key], fileId: versionId ?? undefined } } } : p));
  }, [update]);
  const applyFix = useCallback((f: Fix) => {
    switch (f.kind) {
      case "addProject": return void addProject(f.projectId, f.pinVersion, undefined, f.forKey);
      case "remove": return void remove(f.key);
      case "unpin": return pin(f.key, null);
      case "pinTo": return pin(f.key, f.versionId);
      case "useSource": return useSource(f.key, f.source);
    }
  }, [addProject, remove, pin, useSource]);
  const dismiss = useCallback((s: Suggestion) => {
    update((p) => (p.dismissedSuggestions.includes(s.projectId) ? p : { ...p, dismissedSuggestions: [...p.dismissedSuggestions, s.projectId] }));
  }, [update]);

  // ---- keyboard: "/" focuses search; Delete/Backspace removes the selected entry
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typingIn(e) || orphanPrompt) return;
      if (e.key === "/") { e.preventDefault(); searchRef.current?.focus(); }
      if ((e.key === "g" || e.key === "G") && !e.metaKey && !e.ctrlKey && !e.altKey) { e.preventDefault(); setView((v) => (v === "graph" ? "list" : "graph")); }
      if ((e.key === "Delete" || e.key === "Backspace") && selection?.kind === "entry") { e.preventDefault(); remove(selection.key); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection, remove, orphanPrompt]);

  if (error && !pack) return <ErrorNotice error={error} />;
  if (!pack) return <p className="muted">Opening pack…</p>;

  const noticeEl = notice && (
    <Notice tone={notice.tone === "error" ? "error" : notice.tone === "ok" ? "ok" : "warn"} title={notice.title}>
      {notice.lines.map((l, i) => <div key={i}>{l}</div>)}
      {notice.suggestions.some((s) => !findByCatalogId(pack, s.projectId)) && (
        <div className="stack" style={{ gap: 4, marginTop: 6 }}>
          <span className="muted">Optional extras (tick any, then add):</span>
          <SuggestionPicker suggestions={notice.suggestions.filter((s) => !findByCatalogId(pack, s.projectId))} onAdd={addMany} busy={batchAdding} />
        </div>
      )}
      <div style={{ marginTop: 6 }}><Button size="small" onClick={() => setNotice(null)}>OK</Button></div>
    </Notice>
  );

  return (
    <div className="workspace-wrap">
      <div className="workspace-bar">
        <Button size="small" onClick={async () => { await flush(); onClose(); }}>← Packs</Button>
        <h2 style={{ margin: 0 }}>{pack.name}</h2>
        <span className="muted">Minecraft {pack.mcVersion} · {loaderLabel(pack.loader)} {pack.loaderVersion}</span>
        <span className="grow" />
        <Select aria-label="Preferred site" title="Which site's copy is used for mods available on both"
          value={pack.sourcePreference} onChange={(e) => setPreference(e.target.value as SourcePreference)}>
          <option value="modrinth">Prefer Modrinth</option>
          <option value="curseforge">Prefer CurseForge</option>
          <option value="mixed">Mixed (choose per mod)</option>
        </Select>
        <Select aria-label="Which builds unpinned mods use" title="Applies to every mod that isn't pinned to a version"
          value={pack.channelPolicy ?? "stable"} onChange={(e) => { const v = e.target.value as ChannelPolicy; update((p) => ({ ...p, channelPolicy: v })); }}>
          <option value="stable">Builds: releases first</option>
          <option value="newest">Builds: newest (incl. beta/alpha)</option>
        </Select>
        <span className={saveState === "error" ? "err" : "muted"} style={{ fontSize: 12 }} aria-live="polite">
          {saveState === "saved" ? "All changes saved" : saveState === "pending" ? "Saving…" : "Save failed"}
        </span>
        <Button size="small" onClick={async () => { await flush(); setDuplicating(true); }} title="Copy this pack to another Minecraft version or loader">Duplicate to version…</Button>
        <Button size="small" variant="primary" onClick={async () => { await flush(); setExporting(true); }}>Export…</Button>
      </div>
      {error && <div className="row"><div className="grow"><ErrorNotice error={error} /></div><Button size="small" onClick={() => setError(null)}>Dismiss</Button></div>}
      <div className={`workspace${view === "graph" ? " workspace--graph" : ""}`}>
        {browseMode === "payloads" ? (
          <PayloadsPane pack={pack} onOpen={(id) => setPayloadDialog({ id })} modeSwitch={
            <Tabs label="Browse" value={browseMode} onChange={setBrowseMode} tabs={[{ id: "mods", label: "Mods" }, { id: "payloads", label: "Payloads" }]} />} />
        ) : <Browse pack={pack} onAdd={addHit} modeSwitch={
          <Tabs label="Browse" value={browseMode} onChange={setBrowseMode} tabs={[{ id: "mods", label: "Mods" }, { id: "payloads", label: "Payloads" }]} />} searchRef={searchRef} pending={pending} selectedId={selection?.kind === "hit" ? selection.hit.id : undefined}
          onSelect={(h) => { const k = findHit(pack, h); setSelection(k ? { kind: "entry", key: k } : { kind: "hit", hit: h }); }} />}
        <PackList pack={pack} analysis={analysis} analyzing={analyzing} undo={undo} notice={noticeEl}
          onDropPayload={(id) => setPayloadDialog({ id })} onDropHit={(h) => addHit(h)}
          onSaveAsPayload={() => { setSavePayloadName(`${pack.name} mods`); setSavePayloadKeys(Object.keys(pack.entries)); }}
          updates={updates.length > 0 && (
            <Notice title={updates.length === 1 ? `Payload "${updates[0].name}" was updated` : `${updates.length} payloads were updated`}>
              {updates.map((u) => (
                <div key={u.payloadId} className="row" style={{ justifyContent: "space-between" }}>
                  <span>{u.name} <span className="muted">v{u.appliedVersion} → v{u.currentVersion}</span></span>
                  <Button size="small" onClick={() => setPayloadDialog({ id: u.payloadId, title: `Pull in changes: ${u.name}` })}>Review</Button>
                </div>
              ))}
            </Notice>
          )}
          selectedKey={selection?.kind === "entry" ? selection.key : undefined}
          onSelect={(k) => setSelection({ kind: "entry", key: k })} onRemove={remove} onFix={applyFix}
          onRecheck={() => runAnalysis(true)} onSuggest={addMany} onDismiss={dismiss} adding={batchAdding}
          onMatchAll={matchAll} matching={matching}
          onCrossCheck={(v) => update((p) => ({ ...p, crossCheckDeps: v }))}
          overrides={<Overrides pack={pack} overview={overview} error={overviewError} onUpdate={update} onSelect={(k) => setSelection({ kind: "entry", key: k })} />}
          overrideAlerts={(overview?.updates.length ?? 0) + (overview?.missing.length ?? 0)}
          view={view} onView={setView} grouping={listGrouping} onGrouping={setGrouping("list")}
          graph={view === "graph" && (
            <Suspense fallback={<p className="muted">Loading graph…</p>}><Graph pack={pack} analysis={analysis} selectedKey={selection?.kind === "entry" ? selection.key : undefined}
              grouping={graphGrouping} onGrouping={setGrouping("graph")} onBulkVersions={bulkVersions} onBulkTag={bulkTag}
              onSelect={(k) => setSelection(k ? { kind: "entry", key: k } : null)} onOpen={(k) => setSelection({ kind: "entry", key: k })}
              onRemove={remove} onTogglePin={togglePin} onTag={(k) => { setSelection({ kind: "entry", key: k }); setTagFocus((n) => n + 1); }}
              onPositions={setPositions} onDropHit={(h, at) => addHit(h, at)}
              onDropPayload={(id) => setPayloadDialog({ id })}
              onSaveAsPayload={(keys) => { setSavePayloadName("New payload"); setSavePayloadKeys(keys); }} /></Suspense>
          )} />
        <Details pack={pack} analysis={analysis} selection={selection} onAdd={addHit} onAddProject={(id, pv, forKey) => addProject(id, pv, undefined, forKey)}
          onUseSource={useSource} onLink={link} onUnlink={unlink} tagFocus={tagFocus}
          onTags={(k, tags) => update((p) => (p.entries[k] ? { ...p, entries: { ...p.entries, [k]: { ...p.entries[k], tags } } } : p))}
          onNotes={(k, notes) => update((p) => (p.entries[k] ? { ...p, entries: { ...p.entries, [k]: { ...p.entries[k], notes: notes || undefined } } } : p))}
          onRemove={remove} onPin={pin} onFix={applyFix} onSelect={(k) => setSelection({ kind: "entry", key: k })}
          onToggleLibrary={(k, v) => update((p) => ({ ...p, entries: { ...p.entries, [k]: { ...p.entries[k], libraryOverride: v } } }))} />
      </div>
      {payloadDialog && <PayloadDialog pack={pack} payloadId={payloadDialog.id} title={payloadDialog.title} onClose={() => setPayloadDialog(null)} onApply={applyPayload} />}
      {savePayloadKeys && (
        <Modal title="Save as payload" width={460} onClose={() => setSavePayloadKeys(null)}>
          <form className="stack-lg" onSubmit={async (e) => {
            e.preventDefault();
            try { await api.payloadFromEntries(latest.current!, savePayloadKeys, savePayloadName); setSavePayloadKeys(null); setNotice({ title: `Saved payload "${savePayloadName}"`, lines: ["Find it in Browse → Payloads, or edit it on the Payloads tab."], suggestions: [], tone: "ok" }); }
            catch (err) { setError(err as AppError); }
          }}>
            <p className="muted" style={{ margin: 0 }}>
              {savePayloadKeys.filter((k) => pack.entries[k]?.reason !== "dependency").length} mod(s), with dependencies left out: they come back on their own when the payload is used.
            </p>
            <Input label="Payload name" autoFocus value={savePayloadName} onChange={(e) => setSavePayloadName(e.target.value)} />
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setSavePayloadKeys(null)}>Cancel</Button>
              <Button type="submit" variant="primary" disabled={!savePayloadName.trim()}>Save payload</Button>
            </div>
          </form>
        </Modal>
      )}
      {duplicating && <DuplicateDialog pack={pack} onClose={() => setDuplicating(false)} onCreated={(id) => { setDuplicating(false); onOpen(id); }} />}
      {exporting && (
        <ExportDialog pack={pack} onClose={() => setExporting(false)}
          onFixes={(fixes) => fixes.forEach(applyFix)}
          onSaveProject={async () => { await flush(); try { await api.exportProject(latest.current!); } catch (e) { setError(e as AppError); } }} />
      )}
      {matchReview && (
        <Modal title="Copies on the other site" width={640} onClose={() => setMatchReview(null)}>
          <div className="stack-lg">
            {matchReview.results.length === 0 ? <p>No copies found for the mods that are only linked to one site.</p> : (
              <>
                <p className="muted">Tick the ones that are the same mod. Linking stores both ids, so the mod can switch site later without losing its place.
                  Only <span className="conf conf--verified">verified</span> matches (an identical file on both sites) are ticked for you.</p>
                <ul className="match-list">
                  {matchReview.results.map((r) => {
                    const c = matchReview.picked.get(r.key) ?? r.candidates[0];
                    return (
                      <li key={r.key}>
                        <label className="mc-check">
                          <input type="checkbox" checked={matchReview.picked.has(r.key)} onChange={(e) => {
                            const picked = new Map(matchReview.picked);
                            if (e.target.checked) picked.set(r.key, c); else picked.delete(r.key);
                            setMatchReview({ ...matchReview, picked });
                          }} />
                        </label>
                        <span className="grow"><strong>{entryName(pack, r.key)}</strong> <span className="muted">→</span></span>
                        <ModIcon url={c.iconUrl} label={c.name} size={28} />
                        <span>{c.name}</span>
                        <SourceBadge source={c.source} />
                        <span className={`conf conf--${c.confidence}`} title={c.reason}>{c.confidence}</span>
                      </li>
                    );
                  })}
                </ul>
              </>
            )}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setMatchReview(null)}>Close</Button>
              {matchReview.picked.size > 0 && (
                <Button variant="primary" onClick={() => { for (const [k, c] of matchReview.picked) link(k, c); setMatchReview(null); }}>
                  Link {matchReview.picked.size}
                </Button>
              )}
            </div>
          </div>
        </Modal>
      )}
      {orphanPrompt && (
        <Modal title="Remove unneeded dependencies too?" width={520} onClose={() => setOrphanPrompt(null)}>
          <div className="stack-lg">
            <p>Removing <strong>{entryName(pack, orphanPrompt.keys[0])}</strong> leaves {orphanPrompt.orphans.length === 1 ? "this dependency" : "these dependencies"} with nothing in the pack that needs {orphanPrompt.orphans.length === 1 ? "it" : "them"}:</p>
            <ul className="dep-list">
              {orphanPrompt.orphans.map((k) => (
                <li key={k}>
                  <label className="mc-check">
                    <input type="checkbox" checked={orphanPrompt.picked.has(k)} onChange={(e) => {
                      const picked = new Set(orphanPrompt.picked);
                      if (e.target.checked) picked.add(k); else picked.delete(k);
                      setOrphanPrompt({ ...orphanPrompt, picked });
                    }} />
                    {entryName(pack, k)}
                  </label>
                </li>
              ))}
            </ul>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => { removeKeys(orphanPrompt.keys); setOrphanPrompt(null); }}>Keep them</Button>
              <Button variant="danger" autoFocus onClick={() => { removeKeys([...orphanPrompt.keys, ...orphanPrompt.picked]); setOrphanPrompt(null); }}>
                Remove {orphanPrompt.picked.size ? `${orphanPrompt.picked.size + 1} mods` : "just this one"}
              </Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}
