import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AddPlan, type Analysis, type AppError, type Candidate, type EntryMatches, type Fix, type SearchHit, type Source, type Suggestion } from "../api";
import { Button, ErrorNotice, Modal, Notice, Select } from "../components/kit";
import { typingIn } from "../hooks";
import { applyPlan, applyPreference, catalogId, entryFromHit, entryName, findByCatalogId, findHit, loaderLabel, nowIso, syncPulledInBy, withSource, type ChannelPolicy, type Pack, type PackEntry, type SourcePreference } from "../model";
import { ModIcon, SourceBadge } from "../components/kit";
import { Browse } from "./workspace/Browse";
import { Details, type Selection } from "./workspace/Details";
import { PackList } from "./workspace/PackList";
import { ExportDialog } from "./workspace/ExportDialog";
import { SuggestionPicker } from "./workspace/Suggestions";

type AddNotice = { title: string; lines: string[]; suggestions: Suggestion[]; tone: "ok" | "warn" | "error" };

export function Workspace({ packId, onClose }: { packId: string; onClose: () => void }) {
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
    pack.mcVersion, pack.loader, pack.channelPolicy, pack.dismissedSuggestions,
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

  const addProject = useCallback(async (projectId: string, pin?: string | null, hit?: SearchHit, forKey?: string | null, altId?: string | null) => {
    const p = latest.current;
    if (!p || pending.has(projectId)) return;
    setPending((s) => new Set(s).add(projectId));
    try {
      const plan = await api.planAdd(p, projectId, pin, forKey, altId);
      update((cur) => applyPlan(cur, plan));
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

  const addHit = useCallback((h: SearchHit) => {
    addProject(catalogId(h.source, h.id), null, h, null, h.also ? catalogId(h.also.source, h.also.id) : null);
  }, [addProject]);

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
        <Button size="small" variant="primary" onClick={async () => { await flush(); setExporting(true); }}>Export…</Button>
      </div>
      {error && <div className="row"><div className="grow"><ErrorNotice error={error} /></div><Button size="small" onClick={() => setError(null)}>Dismiss</Button></div>}
      <div className="workspace">
        <Browse pack={pack} onAdd={addHit} searchRef={searchRef} pending={pending} selectedId={selection?.kind === "hit" ? selection.hit.id : undefined}
          onSelect={(h) => { const k = findHit(pack, h); setSelection(k ? { kind: "entry", key: k } : { kind: "hit", hit: h }); }} />
        <PackList pack={pack} analysis={analysis} analyzing={analyzing} undo={undo} notice={noticeEl}
          selectedKey={selection?.kind === "entry" ? selection.key : undefined}
          onSelect={(k) => setSelection({ kind: "entry", key: k })} onRemove={remove} onFix={applyFix}
          onRecheck={() => runAnalysis(true)} onSuggest={addMany} onDismiss={dismiss} adding={batchAdding}
          onMatchAll={matchAll} matching={matching} />
        <Details pack={pack} analysis={analysis} selection={selection} onAdd={addHit} onAddProject={(id, pv, forKey) => addProject(id, pv, undefined, forKey)}
          onUseSource={useSource} onLink={link} onUnlink={unlink}
          onRemove={remove} onPin={pin} onFix={applyFix} onSelect={(k) => setSelection({ kind: "entry", key: k })}
          onToggleLibrary={(k, v) => update((p) => ({ ...p, entries: { ...p.entries, [k]: { ...p.entries[k], libraryOverride: v } } }))} />
      </div>
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
