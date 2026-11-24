import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AddPlan, type Analysis, type AppError, type Fix, type SearchHit, type Suggestion } from "../api";
import { Button, ErrorNotice, Modal, Notice, Select } from "../components/kit";
import { typingIn } from "../hooks";
import { applyPlan, entryFromHit, entryName, findEntry, loaderLabel, nowIso, syncPulledInBy, type ChannelPolicy, type Pack, type PackEntry } from "../model";
import { Browse } from "./workspace/Browse";
import { Details, type Selection } from "./workspace/Details";
import { PackList } from "./workspace/PackList";

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

  const addProject = useCallback(async (projectId: string, pin?: string | null, hit?: SearchHit, forKey?: string | null) => {
    const p = latest.current;
    if (!p || pending.has(projectId)) return;
    setPending((s) => new Set(s).add(projectId));
    try {
      const plan = await api.planAdd(p, projectId, pin, forKey);
      update((cur) => applyPlan(cur, plan));
      setNotice(describePlan(p, plan));
      if (!forKey) setSelection({ kind: "entry", key: plan.rootKey });
    } catch (e) {
      const err = e as AppError;
      if (hit) {
        // Offline or API trouble: still add the mod; the checks will list what's missing.
        const key = crypto.randomUUID();
        const entry = entryFromHit(hit);
        update((cur) => (findEntry(cur, hit.source, hit.id) ? cur : { ...cur, entries: { ...cur.entries, [key]: entry } }));
        setNotice({ title: `Added ${hit.name} without its dependencies`, lines: [err.message, "Checks will list anything missing once the connection is back."], suggestions: [], tone: "warn" });
      } else {
        setNotice({ title: "Couldn't add that mod", lines: [err.message, ...(err.help ? [err.help] : [])], suggestions: [], tone: "error" });
      }
    } finally {
      setPending((s) => { const n = new Set(s); n.delete(projectId); return n; });
    }
  }, [pending, update]);

  const addHit = useCallback((h: SearchHit) => {
    if (h.source === "modrinth") addProject(h.id, null, h);
    else {
      const key = crypto.randomUUID();
      const entry = entryFromHit(h);
      update((p) => (findEntry(p, h.source, h.id) ? p : { ...p, entries: { ...p.entries, [key]: entry } }));
    }
  }, [addProject, update]);

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
    }
  }, [addProject, remove, pin]);
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
      {notice.suggestions.filter((s) => !findEntry(pack, "modrinth", s.projectId)).length > 0 && (
        <div className="stack" style={{ gap: 4, marginTop: 6 }}>
          <span className="muted">Optional extras:</span>
          <div className="row">
            {notice.suggestions.filter((s) => !findEntry(pack, "modrinth", s.projectId)).map((s) => (
              <Button key={s.projectId} size="small" onClick={() => addProject(s.projectId)} title={s.summary}>+ {s.name}</Button>
            ))}
          </div>
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
        <Select aria-label="Which builds unpinned mods use" title="Applies to every mod that isn't pinned to a version"
          value={pack.channelPolicy ?? "stable"} onChange={(e) => { const v = e.target.value as ChannelPolicy; update((p) => ({ ...p, channelPolicy: v })); }}>
          <option value="stable">Builds: releases first</option>
          <option value="newest">Builds: newest (incl. beta/alpha)</option>
        </Select>
        <span className={saveState === "error" ? "err" : "muted"} style={{ fontSize: 12 }} aria-live="polite">
          {saveState === "saved" ? "All changes saved" : saveState === "pending" ? "Saving…" : "Save failed"}
        </span>
        <Button size="small" onClick={async () => { await flush(); try { await api.exportProject(latest.current!); } catch (e) { setError(e as AppError); } }}>Save as file…</Button>
      </div>
      {error && <div className="row"><div className="grow"><ErrorNotice error={error} /></div><Button size="small" onClick={() => setError(null)}>Dismiss</Button></div>}
      <div className="workspace">
        <Browse pack={pack} onAdd={addHit} searchRef={searchRef} pending={pending} selectedId={selection?.kind === "hit" ? selection.hit.id : undefined}
          onSelect={(h) => { const k = findEntry(pack, h.source, h.id); setSelection(k ? { kind: "entry", key: k } : { kind: "hit", hit: h }); }} />
        <PackList pack={pack} analysis={analysis} analyzing={analyzing} undo={undo} notice={noticeEl}
          selectedKey={selection?.kind === "entry" ? selection.key : undefined}
          onSelect={(k) => setSelection({ kind: "entry", key: k })} onRemove={remove} onFix={applyFix}
          onRecheck={() => runAnalysis(true)} onSuggest={(s) => addProject(s.projectId)} onDismiss={dismiss} />
        <Details pack={pack} analysis={analysis} selection={selection} onAdd={addHit} onAddProject={(id, pv, forKey) => addProject(id, pv, undefined, forKey)}
          onRemove={remove} onPin={pin} onFix={applyFix} onSelect={(k) => setSelection({ kind: "entry", key: k })}
          onToggleLibrary={(k, v) => update((p) => ({ ...p, entries: { ...p.entries, [k]: { ...p.entries[k], libraryOverride: v } } }))} />
      </div>
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
