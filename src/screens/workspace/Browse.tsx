import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AppError, type Category, type SearchHit, type SearchPage, type Sort } from "../../api";
import { Button, ErrorNotice, Input, ModIcon, Notice, Panel, Select, SourceBadge, Tooltip } from "../../components/kit";
import { fmtCompact, fmtDate, useDebounced } from "../../hooks";
import { findEntry, loaderLabel, prettyCategory, type Pack } from "../../model";

export function Browse({ pack, onAdd, onSelect, selectedId, searchRef, pending }: {
  pack: Pack; onAdd: (h: SearchHit) => void; onSelect: (h: SearchHit) => void; selectedId?: string; pending: Set<string>;
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
            const resolving = pending.has(h.id);
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
                <Button size="small" variant={inPack ? "default" : "primary"} disabled={inPack || resolving}
                  onClick={(e) => { e.stopPropagation(); onAdd(h); }} aria-label={inPack ? `${h.name} is in the pack` : resolving ? `Adding ${h.name}` : `Add ${h.name}`}>
                  {inPack ? "✓" : resolving ? "…" : "+"}
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

