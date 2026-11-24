import { useCallback, useEffect, useRef, useState } from "react";
import { api, type AppError, type Category, type MergedPage, type SearchHit, type Sort, type Source } from "../../api";
import { Button, ErrorNotice, Input, ModIcon, Notice, Panel, Select, SourceBadge, Tooltip } from "../../components/kit";
import { fmtCompact, fmtDate, useDebounced } from "../../hooks";
import { findHit, loaderLabel, preferredSource, prettyCategory, type Pack } from "../../model";

type SiteFilter = "both" | Source;

function hitKey(h: SearchHit) {
  return `${h.source}:${h.id}`;
}

export function Browse({ pack, onAdd, onSelect, selectedId, searchRef, pending }: {
  pack: Pack; onAdd: (h: SearchHit) => void; onSelect: (h: SearchHit) => void; selectedId?: string; pending: Set<string>;
  searchRef: React.RefObject<HTMLInputElement | null>;
}) {
  const [text, setText] = useState("");
  const [category, setCategory] = useState("");
  const [side, setSide] = useState<"" | "client" | "server">("");
  const [sort, setSort] = useState<Sort>("relevance");
  const [site, setSite] = useState<SiteFilter>("both");
  const [showIncompatible, setShowIncompatible] = useState(false);
  const [cats, setCats] = useState<Category[]>([]);
  const [page, setPage] = useState<MergedPage | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const q = useDebounced(text, 300);
  const reqId = useRef(0);
  const prefer = preferredSource(pack);

  useEffect(() => { api.categories().then(setCats, () => {}); }, []);

  const run = useCallback(async (more: MergedPage | null, force = false) => {
    const id = ++reqId.current;
    setBusy(true); setError(null);
    const sources: Source[] = site === "both" ? ["modrinth", "curseforge"] : [site];
    // "Load more" only asks the sites that still have results.
    const live = more ? sources.filter((s) => more.next[s] != null) : sources;
    try {
      const res = await api.searchAll({
        text: q, gameVersion: pack.mcVersion, loader: pack.loader, categories: category ? [category] : [],
        side: side || undefined, sort, includeIncompatible: showIncompatible, limit: 20, force,
      }, live, prefer, more?.next.modrinth ?? 0, more?.next.curseforge ?? 0);
      if (id !== reqId.current) return;
      setPage((prev) => {
        if (!more || !prev) return res;
        const seen = new Set(prev.hits.flatMap((h) => [hitKey(h), ...(h.also ? [`${h.also.source}:${h.also.id}`] : [])]));
        return {
          ...res,
          totals: { modrinth: res.totals.modrinth ?? prev.totals.modrinth, curseforge: res.totals.curseforge ?? prev.totals.curseforge },
          next: { modrinth: live.includes("modrinth") ? res.next.modrinth : null, curseforge: live.includes("curseforge") ? res.next.curseforge : null },
          hits: [...prev.hits, ...res.hits.filter((h) => !seen.has(hitKey(h)))],
          notes: [...new Set([...prev.notes, ...res.notes])],
        };
      });
      if (!more) setHighlight(0);
    } catch (e) { if (id === reqId.current) setError(e as AppError); }
    finally { if (id === reqId.current) setBusy(false); }
  }, [q, pack.mcVersion, pack.loader, category, side, sort, showIncompatible, site, prefer]);

  useEffect(() => { run(null); }, [run]);

  const hits = page?.hits ?? [];
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setHighlight((h) => Math.min(h + 1, hits.length - 1)); }
    if (e.key === "ArrowUp") { e.preventDefault(); setHighlight((h) => Math.max(h - 1, 0)); }
    if (e.key === "Enter" && hits[highlight]) { e.preventDefault(); onAdd(hits[highlight]); }
  };
  useEffect(() => { document.getElementById(`hit-${highlight}`)?.scrollIntoView({ block: "nearest" }); }, [highlight]);

  const totals = page && [
    page.totals.modrinth != null && `${fmtCompact.format(page.totals.modrinth)} on Modrinth`,
    page.totals.curseforge != null && `${fmtCompact.format(page.totals.curseforge)} on CurseForge`,
  ].filter(Boolean).join(" · ");
  const hasMore = page && (page.next.modrinth != null || page.next.curseforge != null);

  return (
    <Panel title="Browse" className="pane" actions={
      <Select aria-label="Sites to search" value={site} onChange={(e) => setSite(e.target.value as SiteFilter)}>
        <option value="both">Both sites</option><option value="modrinth">Modrinth</option><option value="curseforge">CurseForge</option>
      </Select>
    }>
      <div className="pane__fixed stack">
        <Input ref={searchRef} placeholder="Search mods…  ( / )" value={text} onChange={(e) => setText(e.target.value)} onKeyDown={onKey}
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
          <Button size="small" onClick={() => run(null, true)} disabled={busy} title="Bypass the 24 h cache">Refresh</Button>
        </div>
        {error && <ErrorNotice error={error} />}
        {page?.stale && <Notice title="Offline: showing saved results" />}
        {page?.notes.map((n) => <div key={n} className="muted" style={{ fontSize: 12 }}>ⓘ {n}</div>)}
        <div className="muted" style={{ fontSize: 12 }}>
          {page ? `${totals || "No results"}${showIncompatible ? ` · greyed ones have no ${pack.mcVersion} ${loaderLabel(pack.loader)} build` : ` for ${pack.mcVersion} · ${loaderLabel(pack.loader)}`}` : busy ? "Searching…" : ""}
        </div>
      </div>
      <div className="pane__body stack">
        <ul id="results" className="hit-list" role="listbox" aria-label="Search results">
          {hits.map((h, i) => {
            const inPack = !!findHit(pack, h);
            const resolving = pending.has(h.id) || pending.has(`cf:${h.id}`);
            const incompatible = h.compatible === false;
            const downloads = h.downloads + (h.also?.downloads ?? 0);
            return (
              <li key={hitKey(h)} id={`hit-${i}`} role="option" aria-selected={i === highlight}
                className={`hit${i === highlight ? " hit--hl" : ""}${incompatible ? " hit--incompatible" : ""}${selectedId === h.id ? " hit--sel" : ""}`}
                onMouseEnter={() => setHighlight(i)} onClick={() => onSelect(h)}>
                <Tooltip name={h.name} badge={<span className="row" style={{ gap: 4 }}><SourceBadge source={h.source} />{h.also && <SourceBadge source={h.also.source} />}</span>} lines={[
                  h.summary, `${fmtCompact.format(downloads)} downloads${h.also ? " across both sites" : ""}`, `Updated ${fmtDate(h.updated)}`, `License: ${h.license ?? "unknown"}`,
                  ...(h.distributionBlocked ? ["Author blocks third-party downloads: launchers ask you to fetch it by hand"] : []),
                  ...(incompatible ? [`No build for ${pack.mcVersion} · ${loaderLabel(pack.loader)}`] : []),
                ]}>
                  <ModIcon url={h.iconUrl} label={h.name} size={40} />
                </Tooltip>
                <div className="grow">
                  <div className="hit__name">{h.name}</div>
                  <div className="hit__meta">{h.authors[0] ?? "unknown"} · {fmtCompact.format(downloads)}{incompatible ? " · incompatible" : ""}{h.distributionBlocked ? " · manual download" : ""}</div>
                </div>
                <span className="badge-stack"><SourceBadge source={h.source} />{h.also && <SourceBadge source={h.also.source} />}</span>
                <Button size="small" variant={inPack ? "default" : "primary"} disabled={inPack || resolving}
                  onClick={(e) => { e.stopPropagation(); onAdd(h); }} aria-label={inPack ? `${h.name} is in the pack` : resolving ? `Adding ${h.name}` : `Add ${h.name}`}>
                  {inPack ? "✓" : resolving ? "…" : "+"}
                </Button>
              </li>
            );
          })}
        </ul>
        {hasMore && <Button onClick={() => run(page)} disabled={busy}>{busy ? "Loading…" : "Load more"}</Button>}
        {page && hits.length === 0 && !busy && <div className="empty-state"><p>No mods match. Try another search, or tick "Show incompatible".</p></div>}
      </div>
    </Panel>
  );
}
