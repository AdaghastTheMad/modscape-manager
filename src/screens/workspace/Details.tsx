import { useEffect, useRef, useState } from "react";
import { api, type Analysis, type AppError, type Candidate, type DepLink, type FileInfo, type Fix, type ProjectDetails, type SearchHit, type Source } from "../../api";
import { Button, ErrorNotice, ModIcon, Notice, Panel, SourceBadge } from "../../components/kit";
import { Markdown } from "../../components/Markdown";
import { fmtCompact, fmtDate, useDebounced } from "../../hooks";
import { entryCatalogId, entryName, findHit, isLibrary, loaderLabel, prettyCategory, type Pack } from "../../model";
import { fixLabel } from "./PackList";

export type Selection = { kind: "hit"; hit: SearchHit } | { kind: "entry"; key: string } | null;

function DepList({ title, deps, pack, onSelect, onAdd, addLabel }: {
  title: string; deps: DepLink[]; pack: Pack; onSelect: (k: string) => void; onAdd?: (d: DepLink) => void; addLabel?: string;
}) {
  if (!deps.length) return null;
  return (
    <div className="stack" style={{ gap: 4 }}>
      <h3 style={{ margin: 0 }}>{title}</h3>
      <ul className="dep-list">
        {deps.map((d) => (
          <li key={d.projectId} className="row">
            {d.inPack ? (
              <button type="button" className="linkish" onClick={() => onSelect(d.inPack!)}>✔ {entryName(pack, d.inPack)}</button>
            ) : (
              <span className="muted">○ {d.name}</span>
            )}
            {!d.inPack && onAdd && <Button size="small" variant="primary" onClick={() => onAdd(d)}>{addLabel ?? "Add"}</Button>}
          </li>
        ))}
      </ul>
    </div>
  );
}

const siteName = (s: Source) => (s === "modrinth" ? "Modrinth" : "CurseForge");

/** Search one site yourself, for when matching misses an obvious copy. */
function ManualLink({ pack, entryKey, site, onPick, onClose }: { pack: Pack; entryKey: string; site: Source; onPick: (c: Candidate) => void; onClose: () => void }) {
  const e = pack.entries[entryKey];
  const [text, setText] = useState(e.display.name.replace(/\[[^\]]*\]/g, "").split(" - ")[0].trim());
  const q = useDebounced(text, 300);
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  useEffect(() => {
    if (!q.trim()) { setHits([]); return; }
    let live = true;
    setError(null);
    api.searchAll({ text: q, includeIncompatible: true, limit: 8, kind: e.kind }, [site], site)
      .then((r) => live && setHits(r.hits.filter((h) => h.source === site)), (err) => live && setError(err));
    return () => { live = false; };
  }, [q, site]); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <div className="stack" style={{ gap: 4 }}>
      <div className="row" style={{ gap: 4 }}>
        <input className="mc-input grow" autoFocus value={text} onChange={(ev) => setText(ev.target.value)} aria-label={`Search ${siteName(site)}`} placeholder={`Search ${siteName(site)}…`}
          onKeyDown={(ev) => { if (ev.key === "Escape") { ev.stopPropagation(); onClose(); } }} />
        <Button size="small" onClick={onClose}>Cancel</Button>
      </div>
      {error && <ErrorNotice error={error} />}
      {hits?.length === 0 && q.trim() && <span className="muted" style={{ fontSize: 13 }}>Nothing found on {siteName(site)}.</span>}
      {hits && hits.length > 0 && (
        <ul className="match-list">
          {hits.map((h) => (
            <li key={h.id}>
              <ModIcon url={h.iconUrl} label={h.name} size={28} />
              <span className="grow">{h.name} <span className="muted">· {h.slug} · {fmtCompact.format(h.downloads)}</span></span>
              <Button size="small" variant="primary" title="Link it; the app remembers this for every pack"
                onClick={() => onPick({ source: site, id: h.id, slug: h.slug, name: h.name, iconUrl: h.iconUrl, downloads: h.downloads, confidence: "manual", reason: "Linked by you" })}>Link</Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Sites({ pack, entryKey, onUseSource, onLink, onUnlink }: {
  pack: Pack; entryKey: string; onUseSource: (k: string, s: Source) => void; onLink: (k: string, c: Candidate) => void; onUnlink: (k: string, s: Source) => void;
}) {
  const e = pack.entries[entryKey];
  const [cands, setCands] = useState<Candidate[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const [manual, setManual] = useState(false);
  useEffect(() => { setCands(null); setError(null); setManual(false); }, [entryKey]);
  const both = !!e.ref.modrinthId && e.ref.curseforgeId != null;
  const missing: Source = e.ref.modrinthId ? "curseforge" : "modrinth";
  const find = async () => {
    setBusy(true); setError(null);
    try { setCands(await api.findMatches(pack, entryKey)); } catch (err) { setError(err as AppError); } finally { setBusy(false); }
  };
  return (
    <div className="stack" style={{ gap: 4 }}>
      <h3 style={{ margin: 0 }}>Sites</h3>
      {(["modrinth", "curseforge"] as Source[]).map((s) => {
        const id = s === "modrinth" ? e.ref.modrinthId : e.ref.curseforgeId;
        if (id == null) return null;
        return (
          <div key={s} className="source-row">
            <SourceBadge source={s} /><code>{id}</code>
            {e.ref.preferred === s ? <span className="ok" style={{ fontSize: 12 }}>in use</span>
              : <Button size="small" onClick={() => onUseSource(entryKey, s)}>Use {siteName(s)}</Button>}
            {both && <Button size="small" onClick={() => onUnlink(entryKey, s)} title="This isn't the same mod">Unlink</Button>}
          </div>
        );
      })}
      {!both && (
        <>
          <div className="row">
            <Button size="small" onClick={find} disabled={busy}>{busy ? "Looking…" : `Find on ${siteName(missing)}`}</Button>
            {!manual && <Button size="small" onClick={() => setManual(true)} title="Pick the copy yourself; the link is remembered">Search {siteName(missing)} yourself…</Button>}
          </div>
          {manual && <ManualLink pack={pack} entryKey={entryKey} site={missing} onClose={() => setManual(false)} onPick={(c) => { onLink(entryKey, c); setManual(false); setCands(null); }} />}
          {error && <ErrorNotice error={error} />}
          {cands?.length === 0 && <span className="muted" style={{ fontSize: 13 }}>No likely copy on {siteName(missing)}. If you know it's there, search for it yourself.</span>}
          {cands && cands.length > 0 && (
            <ul className="match-list">
              {cands.slice(0, 5).map((c) => (
                <li key={c.id}>
                  <ModIcon url={c.iconUrl} label={c.name} size={28} />
                  <span className="grow">{c.name} <span className="muted">· {c.slug}</span></span>
                  <span className={`conf conf--${c.confidence}`} title={c.reason}>{c.confidence}</span>
                  <Button size="small" variant="primary" onClick={() => { onLink(entryKey, c); setCands(null); }}>Link</Button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}

function TagsNotes({ pack, entryKey, onTags, onNotes, focusSignal }: {
  pack: Pack; entryKey: string; onTags: (k: string, t: string[]) => void; onNotes: (k: string, n: string) => void; focusSignal: number;
}) {
  const e = pack.entries[entryKey];
  const [draft, setDraft] = useState("");
  const [notes, setNotes] = useState(e.notes ?? "");
  const input = useRef<HTMLInputElement | null>(null);
  useEffect(() => { setNotes(pack.entries[entryKey]?.notes ?? ""); setDraft(""); }, [entryKey]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (focusSignal) input.current?.focus(); }, [focusSignal]);
  // Tags already used in this pack, to pick from.
  const known = [...new Set(Object.values(pack.entries).flatMap((x) => x.tags))].filter((t) => !e.tags.includes(t)).sort();
  const add = (t: string) => {
    const v = t.trim().replace(/^#/, "");
    if (v && !e.tags.includes(v)) onTags(entryKey, [...e.tags, v]);
    setDraft("");
  };
  return (
    <div className="stack" style={{ gap: 4 }}>
      <h3 style={{ margin: 0 }}>Tags & notes</h3>
      <div className="row" style={{ gap: 4 }}>
        {e.tags.map((t) => (
          <span key={t} className="tag-chip">#{t}<button type="button" aria-label={`Remove tag ${t}`} onClick={() => onTags(entryKey, e.tags.filter((x) => x !== t))}>✕</button></span>
        ))}
        <input ref={input} className="mc-input tag-input" list={`tags-${entryKey}`} placeholder="add tag…" value={draft}
          onChange={(ev) => setDraft(ev.target.value)} onKeyDown={(ev) => { if (ev.key === "Enter") { ev.preventDefault(); add(draft); } }} aria-label="Add a tag" />
        <datalist id={`tags-${entryKey}`}>{known.map((t) => <option key={t} value={t} />)}</datalist>
      </div>
      <textarea className="mc-input notes" rows={2} placeholder="Notes, e.g. replace with X if it doesn't port to 26.1" value={notes}
        onChange={(ev) => setNotes(ev.target.value)} onBlur={() => { if ((e.notes ?? "") !== notes) onNotes(entryKey, notes); }} aria-label="Notes" />
    </div>
  );
}

function FileControl({ pack, entryKey, file, onPin }: { pack: Pack; entryKey: string; file?: FileInfo | null; onPin: (versionId: string | null) => void }) {
  const e = pack.entries[entryKey];
  const [versions, setVersions] = useState<FileInfo[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => { setVersions(null); setOpen(false); }, [entryKey]);
  const load = () => {
    setOpen(true);
    const id = entryCatalogId(e);
    if (versions || !id) return;
    api.entryVersions(id, pack.mcVersion, pack.loader, e.kind).then(setVersions, setError);
  };
  return (
    <div className="stack" style={{ gap: 4 }}>
      <h3 style={{ margin: 0 }}>File</h3>
      {file ? (
        <p style={{ margin: 0 }}>
          <strong>{file.versionNumber}</strong> <span className={file.versionType === "release" ? "muted" : "warn"}>({file.versionType})</span>
          <span className="muted"> · {fmtDate(file.datePublished)} · {file.pinned ? (e.pinForced ? "pinned on purpose: made for another version/loader (another mod needs exactly this file)" : "pinned") : pack.channelPolicy === "newest" ? "newest build" : file.versionType === "release" ? "newest release" : `no release yet, newest ${file.versionType}`}</span>
        </p>
      ) : <p className="muted" style={{ margin: 0 }}>No file chosen yet.</p>}
      <div className="row">
        {e.fileId
          ? <Button size="small" onClick={() => onPin(null)}>Float to newest</Button>
          : file && <Button size="small" onClick={() => onPin(file.versionId)}>Pin this version</Button>}
        <Button size="small" onClick={() => (open ? setOpen(false) : load())}>{open ? "Hide versions" : "Choose version…"}</Button>
      </div>
      {open && (
        <div className="mc-panel mc-panel--inset version-pick">
          {error && <ErrorNotice error={error} />}
          {!versions && !error && <span className="muted">Loading versions…</span>}
          {versions?.length === 0 && <span className="muted">No builds for {pack.mcVersion} · {loaderLabel(pack.loader)}.</span>}
          {versions?.map((v) => (
            <button key={v.versionId} type="button" className="version-row" aria-pressed={e.fileId === v.versionId}
              onClick={() => { onPin(v.versionId); setOpen(false); }}>
              <span>{v.versionNumber}</span>
              <span className={v.versionType === "release" ? "muted" : "warn"}>{v.versionType}</span>
              <span className="muted">{fmtDate(v.datePublished)}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Details({ pack, analysis, selection, onAdd, onAddProject, onRemove, onToggleLibrary, onPin, onFix, onSelect, onUseSource, onLink, onUnlink, onTags, onNotes, tagFocus }: {
  pack: Pack; analysis: Analysis | null; selection: Selection; onAdd: (h: SearchHit) => void; onAddProject: (id: string, pin?: string | null, forKey?: string) => void;
  onRemove: (k: string) => void; onToggleLibrary: (k: string, value: boolean) => void; onPin: (k: string, versionId: string | null) => void;
  onFix: (f: Fix) => void; onSelect: (k: string) => void;
  onUseSource: (k: string, s: Source) => void; onLink: (k: string, c: Candidate) => void; onUnlink: (k: string, s: Source) => void;
  onTags: (k: string, t: string[]) => void; onNotes: (k: string, n: string) => void; tagFocus: number;
}) {
  const entryKey = selection?.kind === "entry" && pack.entries[selection.key] ? selection.key : undefined;
  const ref = selection?.kind === "hit" ? { source: selection.hit.source, id: selection.hit.id }
    : entryKey ? (() => {
      const r = pack.entries[entryKey].ref;
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

  if (!ref) return <Panel className="pane" title="Details"><div className="empty-state"><p>Select a mod in the pack to see its files, dependencies, tags and notes. Click a search result to preview it before adding.</p></div></Panel>;
  const key = entryKey ?? (selection?.kind === "hit" ? findHit(pack, selection.hit) : undefined);
  const entry = key ? pack.entries[key] : undefined;
  const rep = key ? analysis?.entries[key] : undefined;
  const fallback = selection?.kind === "hit" ? selection.hit : null;
  const name = details?.name ?? fallback?.name ?? entry?.display.name ?? "";
  const issues = key ? (analysis?.issues ?? []).filter((i) => i.entryKey === key || (i.kind === "incompatible" && i.otherKey === key)) : [];
  const supports = !entry && details && {
    version: details.gameVersions.includes(pack.mcVersion),
    loader: details.loaders.includes(pack.loader) || (pack.loader === "quilt" && details.loaders.includes("fabric")),
  };

  return (
    <Panel className="pane" title="Details">
      <div className="pane__body stack-lg">
        <div className="row" style={{ alignItems: "flex-start", flexWrap: "nowrap" }}>
          <ModIcon url={details?.iconUrl ?? fallback?.iconUrl ?? entry?.display.iconUrl} label={name} size={64} />
          <div className="grow stack" style={{ gap: 4 }}>
            <h2 style={{ margin: 0 }}>{name}</h2>
            <div className="row"><SourceBadge source={ref.source} />
              {entry && <span className="muted" style={{ fontSize: 12 }}>{entry.reason === "dependency" ? "added as a dependency" : "added by you"}</span>}
              {details && <span className="muted" style={{ fontSize: 12 }}>{fmtCompact.format(details.downloads)} downloads</span>}</div>
          </div>
        </div>
        <div className="row">
          {key && entry && (
            <Button size="small" onClick={() => onToggleLibrary(key, !isLibrary(entry))} title="Libraries are dimmed and grouped at the bottom">
              {isLibrary(entry) ? "Not a library" : "Treat as library"}
            </Button>
          )}
          {key ? <Button size="small" variant="danger" onClick={() => onRemove(key)}>Remove from pack</Button>
            : fallback && <Button size="small" variant="primary" onClick={() => onAdd(fallback)}>Add to pack</Button>}
          {details && <Button size="small" onClick={() => api.openExternal(details.pageUrl)}>{siteName(ref.source)} ↗</Button>}
        </div>

        {issues.map((i) => (
          <Notice key={i.id} tone={i.severity === "error" ? "error" : i.severity === "warning" ? "warn" : "ok"} title={i.message}>
            {i.fix && <Button size="small" onClick={() => onFix(i.fix!)}>{fixLabel(i.fix)}</Button>}
          </Notice>
        ))}

        {key && <TagsNotes pack={pack} entryKey={key} onTags={onTags} onNotes={onNotes} focusSignal={tagFocus} />}
        {key && <Sites pack={pack} entryKey={key} onUseSource={onUseSource} onLink={onLink} onUnlink={onUnlink} />}
        {key && <FileControl pack={pack} entryKey={key} file={rep?.file} onPin={(v) => onPin(key, v)} />}
        {!key && fallback?.also && <p className="muted" style={{ margin: 0, fontSize: 13 }}>Also on {siteName(fallback.also.source)}: adding it links both.</p>}
        {details?.distributionBlocked && <Notice title="Manual download">This mod's author blocks third-party downloads on CurseForge. Launchers will ask you to download it by hand.</Notice>}
        {rep && (
          <>
            <DepList title="Requires" deps={rep.required} pack={pack} onSelect={onSelect} onAdd={(d) => onAddProject(d.projectId, d.versionId, key)} />
            <DepList title="Optional" deps={rep.optional} pack={pack} onSelect={onSelect} onAdd={(d) => onAddProject(d.projectId)} />
            <DepList title="Incompatible with" deps={rep.incompatible} pack={pack} onSelect={onSelect} />
            {rep.requiredBy.length > 0 && (
              <div className="stack" style={{ gap: 4 }}>
                <h3 style={{ margin: 0 }}>Needed by</h3>
                <ul className="dep-list">{rep.requiredBy.map((k) => <li key={k}><button type="button" className="linkish" onClick={() => onSelect(k)}>{entryName(pack, k)}</button></li>)}</ul>
              </div>
            )}
          </>
        )}

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
