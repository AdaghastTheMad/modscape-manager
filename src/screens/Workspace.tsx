import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type AppError, type Category, type ProjectDetails, type SearchHit, type SearchPage, type Sort } from "../api";
import { Button, ErrorNotice, Input, ModIcon, Notice, Panel, Select, SourceBadge, Tabs, Tooltip } from "../components/kit";
import { Markdown } from "../components/Markdown";
import { fmtCompact, fmtDate, typingIn, useDebounced } from "../hooks";
import { entryFromHit, findEntry, isLibrary, loaderLabel, nowIso, prettyCategory, type Pack, type PackEntry } from "../model";

type Selection = { kind: "hit"; hit: SearchHit } | { kind: "entry"; key: string } | null;

// ======================================================================= Browse
function Browse({ pack, onAdd, onSelect, selectedId, searchRef }: {
  pack: Pack; onAdd: (h: SearchHit) => void; onSelect: (h: SearchHit) => void; selectedId?: string;
  searchRef: React.RefObject<HTMLInputElement | null>;
}) {
  const [text, setText] = useState("");
  const [category, setCategory] = useState("");
  const [side, setSide] = useState<"" | "client" | "server">("");
  const [sort, setSort] = useState<Sort>("relevance");
  const [showIncompatible, setShowIncompatible] = useState(false);
  const [cats, setCats] = useState<Category[]>([]);
  const [page, setPage] = useState<SearchPage | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const q = useDebounced(text, 300);
  const reqId = useRef(0);

  useEffect(() => { api.categories().then(setCats, () => {}); }, []);

  const run = useCallback(async (offset: number, force = false) => {
    const id = ++reqId.current;
    setBusy(true); setError(null);
    try {
      const res = await api.search("modrinth", {
        text: q, gameVersion: pack.mcVersion, loader: pack.loader, categories: category ? [category] : [],
        side: side || undefined, sort, includeIncompatible: showIncompatible, limit: 20, offset, force,
      });
      if (id !== reqId.current) return;
      setPage((prev) => (offset > 0 && prev ? { ...res, hits: [...prev.hits, ...res.hits.filter((h) => !prev.hits.some((p) => p.id === h.id))] } : res));
      if (offset === 0) setHighlight(0);
    } catch (e) { if (id === reqId.current) setError(e as AppError); }
    finally { if (id === reqId.current) setBusy(false); }
  }, [q, pack.mcVersion, pack.loader, category, side, sort, showIncompatible]);

  useEffect(() => { run(0); }, [run]);

  const hits = page?.hits ?? [];
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setHighlight((h) => Math.min(h + 1, hits.length - 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setHighlight((h) => Math.max(h - 1, 0)); }
    if (e.key === "Enter" && hits[highlight]) { e.preventDefault(); onAdd(hits[highlight]); }
  };
  useEffect(() => { document.getElementById(`hit-${highlight}`)?.scrollIntoView({ block: "nearest" }); }, [highlight]);

  return (
    <Panel title="Browse" className="pane" actions={<SourceBadge source="modrinth" />}>
      <div className="pane__fixed stack">
        <Input ref={searchRef} placeholder="Search Modrinth…  ( / )" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={onKey}
          aria-label="Search mods" aria-controls="results" aria-activedescendant={hits[highlight] ? `hit-${highlight}` : undefined} />
        <div className="filters">
          <Select aria-label="Category" value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">All categories</option>
            {cats.map((c) => <option key={c.name} value={c.name}>{prettyCategory(c.name)}</option>)}
          </Select>
          <Select aria-label="Side" value={side} onChange={(e) => setSide(e.target.value as typeof side)}>
            <option value="">Any side</option><option value="client">Works on client</option><option value="server">Works on server</option>
          </Select>
          <Select aria-label="Sort" value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
            <option value="relevance">Relevance</option><option value="downloads">Downloads</option><option value="follows">Follows</option>
            <option value="updated">Last updated</option><option value="newest">Newest</option>
          </Select>
        </div>
        <div className="row" style={{ justifyContent: "space-between" }}>
          <label className="mc-check"><input type="checkbox" checked={showIncompatible} onChange={(e) => setShowIncompatible(e.target.checked)} /> Show incompatible</label>
          <Button size="small" onClick={() => run(0, true)} disabled={busy} title="Bypass the 24 h cache">Refresh</Button>
        </div>
        {error && <ErrorNotice error={error} />}
        {page?.freshness?.stale && <Notice title="Offline: showing saved results">From {new Date(page.freshness.fetchedAt * 1000).toLocaleString()}.</Notice>}
        <div className="muted" style={{ fontSize: 12 }}>
          {page ? (showIncompatible
            ? `${fmtCompact.format(page.total)} results on any version · greyed ones have no ${pack.mcVersion} ${loaderLabel(pack.loader)} build`
            : `${fmtCompact.format(page.total)} results for ${pack.mcVersion} · ${loaderLabel(pack.loader)}`) : busy ? "Searching…" : ""}
        </div>
      </div>
      <div className="pane__body stack">
        <ul id="results" className="hit-list" role="listbox" aria-label="Search results">
          {hits.map((h, i) => {
            const inPack = !!findEntry(pack, h.source, h.id);
            const incompatible = h.compatible === false;
            return (
              <li key={h.id} id={`hit-${i}`} role="option" aria-selected={i === highlight}
                className={`hit${i === highlight ? " hit--hl" : ""}${incompatible ? " hit--incompatible" : ""}${selectedId === h.id ? " hit--sel" : ""}`}
                onMouseEnter={() => setHighlight(i)} onClick={() => onSelect(h)}>
                <Tooltip name={h.name} badge={<SourceBadge source={h.source} />} lines={[
                  h.summary, `${fmtCompact.format(h.downloads)} downloads`, `Updated ${fmtDate(h.updated)}`, `License: ${h.license ?? "unknown"}`,
                  ...(incompatible ? [`No build for ${pack.mcVersion} · ${loaderLabel(pack.loader)}`] : []),
                ]}>
                  <ModIcon url={h.iconUrl} label={h.name} size={40} />
                </Tooltip>
                <div className="grow">
                  <div className="hit__name">{h.name}</div>
                  <div className="hit__meta">{h.authors[0] ?? "unknown"} · {fmtCompact.format(h.downloads)}{incompatible ? " · incompatible" : ""}</div>
                </div>
                <Button size="small" variant={inPack ? "default" : "primary"} disabled={inPack}
                  onClick={(e) => { e.stopPropagation(); onAdd(h); }} aria-label={inPack ? `${h.name} is in the pack` : `Add ${h.name}`}>
                  {inPack ? "✓" : "+"}
                </Button>
              </li>
            );
          })}
        </ul>
        {page && hits.length < page.total && <Button onClick={() => run(hits.length)} disabled={busy}>{busy ? "Loading…" : "Load more"}</Button>}
        {page && hits.length === 0 && !busy && <div className="empty-state"><p>No mods match. Try another search, or tick "Show incompatible".</p></div>}
      </div>
    </Panel>
  );
}

// ===================================================================== PackList
function groupOf(e: PackEntry): string {
  if (isLibrary(e)) return "library";
  return e.display.categories.find((c) => c !== "library") ?? "uncategorized";
}

function PackList({ pack, selectedKey, onSelect, onRemove, undo }: {
  pack: Pack; selectedKey?: string; onSelect: (k: string) => void; onRemove: (k: string) => void;
  undo: { label: string; run: () => void } | null;
}) {
  const groups = useMemo(() => {
    const m = new Map<string, [string, PackEntry][]>();
    for (const [k, e] of Object.entries(pack.entries)) {
      const g = groupOf(e);
      m.set(g, [...(m.get(g) ?? []), [k, e]]);
    }
    for (const list of m.values()) list.sort((a, b) => a[1].display.name.localeCompare(b[1].display.name));
    return [...m.entries()].sort((a, b) => (a[0] === "library" ? 1 : b[0] === "library" ? -1 : a[0].localeCompare(b[0])));
  }, [pack.entries]);
  const count = Object.keys(pack.entries).length;

  return (
    <Panel className="pane" title={`Pack · ${count} mod${count === 1 ? "" : "s"}`} actions={
      <Tabs label="View" value="list" onChange={() => {}} tabs={[{ id: "list", label: "List" }, { id: "graph", label: "Graph (M7)", disabled: true }]} />
    }>
      <div className="pane__body stack">
        {undo && <Notice title={undo.label}><Button size="small" onClick={undo.run}>Undo</Button></Notice>}
        {count === 0 ? (
          <div className="empty-state"><div><div className="pixel">This pack is empty</div><p>Search on the left and press + (or Enter) to add mods.</p></div></div>
        ) : groups.map(([g, list]) => (
          <section key={g} className={g === "library" ? "group group--lib" : "group"}>
            <h3 className="group__title">{g === "library" ? "Libraries" : prettyCategory(g)} <span className="muted">({list.length})</span></h3>
            <ul className="entry-list">
              {list.map(([k, e]) => (
                <li key={k} className={`entry${selectedKey === k ? " entry--sel" : ""}`} tabIndex={0} aria-selected={selectedKey === k}
                  onClick={() => onSelect(k)} onFocus={() => onSelect(k)}>
                  <ModIcon url={e.display.iconUrl} label={e.display.name} size={36} selected={selectedKey === k} />
                  <div className="grow">
                    <div className="hit__name">{e.display.name}</div>
                    <div className="hit__meta">{e.display.categories.map(prettyCategory).join(", ") || "no category"}</div>
                  </div>
                  <SourceBadge source={e.ref.preferred} />
                  <Button size="small" variant="danger" aria-label={`Remove ${e.display.name}`} onClick={(ev) => { ev.stopPropagation(); onRemove(k); }}>✕</Button>
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </Panel>
  );
}

// ====================================================================== Details
function Details({ pack, selection, onAdd, onRemove, onToggleLibrary }: {
  pack: Pack; selection: Selection; onAdd: (h: SearchHit) => void; onRemove: (k: string) => void;
  onToggleLibrary: (k: string, value: boolean) => void;
}) {
  const ref = selection?.kind === "hit" ? { source: selection.hit.source, id: selection.hit.id }
    : selection?.kind === "entry" && pack.entries[selection.key] ? (() => {
      const r = pack.entries[selection.key].ref;
      return r.preferred === "modrinth" && r.modrinthId ? { source: "modrinth" as const, id: r.modrinthId } : { source: r.preferred, id: String(r.curseforgeId ?? r.modrinthId) };
    })() : null;
  const [details, setDetails] = useState<ProjectDetails | null>(null);
  const [error, setError] = useState<AppError | null>(null);

  useEffect(() => {
    setDetails(null); setError(null);
    if (!ref) return;
    let live = true;
    api.projectDetails(ref.source, ref.id).then((d) => live && setDetails(d), (e) => live && setError(e));
    return () => { live = false; };
  }, [ref?.source, ref?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!ref) return <Panel className="pane" title="Details"><div className="empty-state"><p>Select a mod to see its details here.</p></div></Panel>;
  const key = findEntry(pack, ref.source, ref.id);
  const fallback = selection?.kind === "hit" ? selection.hit : null;
  const name = details?.name ?? fallback?.name ?? pack.entries[key ?? ""]?.display.name ?? "";
  const supports = details && {
    version: details.gameVersions.includes(pack.mcVersion),
    loader: details.loaders.includes(pack.loader) || (pack.loader === "quilt" && details.loaders.includes("fabric")),
  };

  return (
    <Panel className="pane" title="Details">
      <div className="pane__body stack-lg">
        <div className="row" style={{ alignItems: "flex-start", flexWrap: "nowrap" }}>
          <ModIcon url={details?.iconUrl ?? fallback?.iconUrl} label={name} size={64} />
          <div className="grow stack" style={{ gap: 4 }}>
            <h2 style={{ margin: 0 }}>{name}</h2>
            <div className="row"><SourceBadge source={ref.source} />
              {details && <span className="muted" style={{ fontSize: 12 }}>{fmtCompact.format(details.downloads)} downloads · {fmtCompact.format(details.follows)} follows</span>}</div>
          </div>
        </div>
        <div className="row">
          {key && (() => { const lib = isLibrary(pack.entries[key]); return (
            <Button onClick={() => onToggleLibrary(key, !lib)} title="Libraries are dimmed and grouped at the bottom">
              {lib ? "Not a library" : "Treat as library"}
            </Button>); })()}
          {key ? <Button variant="danger" onClick={() => onRemove(key)}>Remove from pack</Button>
            : fallback && <Button variant="primary" onClick={() => onAdd(fallback)}>Add to pack</Button>}
          {details && <Button onClick={() => api.openExternal(details.pageUrl)}>Open on Modrinth ↗</Button>}
        </div>
        {error && <ErrorNotice error={error} />}
        {!details && !error && <p className="muted">Loading…</p>}
        {details && (
          <>
            {supports && (!supports.version || !supports.loader) && (
              <Notice tone="error" title={`No ${!supports.version ? pack.mcVersion : loaderLabel(pack.loader)} build listed`}>
                This mod lists {details.loaders.join(", ") || "no loaders"} for {details.gameVersions.length} game versions.
              </Notice>
            )}
            <p>{details.summary}</p>
            <dl className="facts">
              <dt>License</dt><dd>{details.license ? (details.license.name || details.license.id) : "unknown"}</dd>
              <dt>Updated</dt><dd>{fmtDate(details.updated)}</dd>
              <dt>Loaders</dt><dd>{details.loaders.join(", ") || "none listed"}</dd>
              <dt>Client / server</dt><dd>{details.clientSide ?? "?"} / {details.serverSide ?? "?"}</dd>
              <dt>Categories</dt><dd>{details.categories.map(prettyCategory).join(", ") || "none"}</dd>
            </dl>
            {details.links.length > 0 && <div className="row">{details.links.map(([l, u]) => <Button key={l} size="small" onClick={() => api.openExternal(u)}>{l} ↗</Button>)}</div>}
            <Markdown source={details.body} />
            {details.freshness.stale && <p className="muted" style={{ fontSize: 12 }}>Offline: saved copy from {new Date(details.freshness.fetchedAt * 1000).toLocaleString()}.</p>}
          </>
        )}
      </div>
    </Panel>
  );
}

// ==================================================================== Workspace
export function Workspace({ packId, onClose }: { packId: string; onClose: () => void }) {
  const [pack, setPack] = useState<Pack | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [saveState, setSaveState] = useState<"saved" | "pending" | "error">("saved");
  const [selection, setSelection] = useState<Selection>(null);
  const [undo, setUndo] = useState<{ label: string; run: () => void } | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const dirty = useRef(false);
  const latest = useRef<Pack | null>(null);

  useEffect(() => { api.getPack(packId).then(setPack, setError); }, [packId]);

  // Autosave: debounce changes, flush on close.
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

  // State updaters must stay pure (React may run them twice), so anything
  // random or time-based is computed before calling update().
  useEffect(() => { latest.current = pack; }, [pack]);
  const update = useCallback((fn: (p: Pack) => Pack) => {
    const at = nowIso();
    setPack((p) => (p ? { ...fn(p), updatedAt: at } : p));
    dirty.current = true;
    setSaveState("pending");
  }, []);

  const add = useCallback((h: SearchHit) => {
    const key = crypto.randomUUID();
    const entry = entryFromHit(h);
    update((p) => (findEntry(p, h.source, h.id) ? p : { ...p, entries: { ...p.entries, [key]: entry } }));
  }, [update]);

  const remove = useCallback((key: string) => {
    const p = latest.current;
    const entry = p?.entries[key];
    if (!entry) return;
    update((p) => { const { [key]: _, ...rest } = p.entries; return { ...p, entries: rest }; });
    setSelection((s) => (s?.kind === "entry" && s.key === key ? null : s));
    setUndo({ label: `Removed ${entry.display.name}`, run: () => { update((p) => ({ ...p, entries: { ...p.entries, [key]: entry } })); setUndo(null); } });
  }, [update]);
  useEffect(() => { if (!undo) return; const t = setTimeout(() => setUndo(null), 10000); return () => clearTimeout(t); }, [undo]);

  // Keyboard: "/" focuses search; Delete/Backspace removes the selected pack entry.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typingIn(e)) return;
      if (e.key === "/") { e.preventDefault(); searchRef.current?.focus(); }
      if ((e.key === "Delete" || e.key === "Backspace") && selection?.kind === "entry") { e.preventDefault(); remove(selection.key); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [selection, remove]);

  if (error && !pack) return <ErrorNotice error={error} />;
  if (!pack) return <p className="muted">Opening pack…</p>;

  return (
    <div className="workspace-wrap">
      <div className="workspace-bar">
        <Button size="small" onClick={async () => { await flush(); onClose(); }}>← Packs</Button>
        <h2 style={{ margin: 0 }}>{pack.name}</h2>
        <span className="muted">Minecraft {pack.mcVersion} · {loaderLabel(pack.loader)} {pack.loaderVersion}</span>
        <span className="grow" />
        <span className={saveState === "error" ? "err" : "muted"} style={{ fontSize: 12 }} aria-live="polite">
          {saveState === "saved" ? "All changes saved" : saveState === "pending" ? "Saving…" : "Save failed"}
        </span>
        <Button size="small" onClick={async () => { await flush(); try { await api.exportProject(latest.current!); } catch (e) { setError(e as AppError); } }}>Save as file…</Button>
      </div>
      {error && <ErrorNotice error={error} />}
      <div className="workspace">
        <Browse pack={pack} onAdd={add} searchRef={searchRef} selectedId={selection?.kind === "hit" ? selection.hit.id : undefined}
          onSelect={(h) => { const k = findEntry(pack, h.source, h.id); setSelection(k ? { kind: "entry", key: k } : { kind: "hit", hit: h }); }} />
        <PackList pack={pack} undo={undo} selectedKey={selection?.kind === "entry" ? selection.key : undefined}
          onSelect={(k) => setSelection({ kind: "entry", key: k })} onRemove={remove} />
        <Details pack={pack} selection={selection} onAdd={add} onRemove={remove}
          onToggleLibrary={(k, v) => update((p) => ({ ...p, entries: { ...p.entries, [k]: { ...p.entries[k], libraryOverride: v } } }))} />
      </div>
    </div>
  );
}
