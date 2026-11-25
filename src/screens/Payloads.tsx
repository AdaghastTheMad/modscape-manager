import { useEffect, useMemo, useState } from "react";
import { unsaved, useUnsavedGuard } from "../unsaved";
import { api, type AnyVersion, type AppError, type EntryKindT, type Loader, type Payload, type PCandidate, type PChoiceOption, type PItem, type PSlot, type SearchHit } from "../api";
import { Button, ErrorNotice, Input, ModIcon, Modal, Notice, Panel, Select, SourceBadge } from "../components/kit";
import { fmtCompact, useDebounced } from "../hooks";
import { candidateFromHit, KINDS, LOADERS, nowIso } from "../model";

const uid = () => crypto.randomUUID().slice(0, 8);

export function newPayload(name = "New payload"): Payload {
  const t = nowIso();
  return { id: crypto.randomUUID(), name, description: "", items: [], includes: [], version: 1, starter: false, createdAt: t, updatedAt: t };
}

export function countSlots(p: Payload): number {
  return p.items.reduce((n, it) => n + (it.kind === "mod" ? 1 : it.options.reduce((m, o) => m + o.slots.length, 0) + it.common.length), 0);
}

/** "11 mods", "1 choice (4 options) + 2 common", "+ 3 nested" — what you'd actually get. */
export function describePayload(p: Payload): string {
  const mods = p.items.filter((i) => i.kind === "mod").length;
  const choices = p.items.filter((i): i is Extract<PItem, { kind: "choice" }> => i.kind === "choice");
  const parts: string[] = [];
  if (mods) parts.push(`${mods} mod${mods === 1 ? "" : "s"}`);
  for (const c of choices) {
    parts.push(`1 choice (${c.options.length} options)${c.common.length ? ` + ${c.common.length} common` : ""}`);
  }
  if (p.includes.length) parts.push(`${p.includes.length} nested`);
  return parts.join(" + ") || "empty";
}

// ---------------------------------------------------------------- search picker
export function ModPicker({ onPick, onClose, placeholder = "Search Modrinth and CurseForge…" }: { onPick: (c: PCandidate) => void; onClose?: () => void; placeholder?: string }) {
  const [text, setText] = useState("");
  const [kind, setKind] = useState<EntryKindT>("mod");
  const q = useDebounced(text, 300);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [error, setError] = useState<AppError | null>(null);
  useEffect(() => {
    if (!q.trim()) { setHits([]); return; }
    let live = true;
    // Payloads are version-agnostic: search everything.
    api.searchAll({ text: q, includeIncompatible: true, limit: 8, kind }, ["modrinth", "curseforge"], "modrinth")
      .then((r) => live && setHits(r.hits.map((h) => ({ ...h, kind }))), (e) => live && setError(e));
    return () => { live = false; };
  }, [q, kind]);
  return (
    <div className="mod-picker stack" style={{ gap: 4 }}>
      <div className="row">
        <Select aria-label="Content type" value={kind} onChange={(e) => setKind(e.target.value as EntryKindT)}>
          {KINDS.map((k) => <option key={k.id} value={k.id}>{k.label}</option>)}
        </Select>
        <div className="grow"><Input autoFocus placeholder={placeholder} value={text} onChange={(e) => setText(e.target.value)} aria-label="Search for a mod" /></div>
        {onClose && <Button size="small" onClick={onClose}>Cancel</Button>}
      </div>
      {error && <ErrorNotice error={error} />}
      {hits.length > 0 && (
        <ul className="hit-list">
          {hits.map((h) => (
            <li key={`${h.source}:${h.id}`} className="hit" onClick={() => onPick(candidateFromHit(h))}>
              <ModIcon url={h.iconUrl} label={h.name} size={28} />
              <div className="grow"><div className="hit__name">{h.name}</div><div className="hit__meta">{h.authors[0] ?? ""} · {fmtCompact.format(h.downloads)}</div></div>
              <span className="badge-stack"><SourceBadge source={h.source} />{h.also && <SourceBadge source={h.also.source} />}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- reordering
const swap = <T,>(xs: T[], i: number, d: -1 | 1): T[] => { const c = [...xs]; [c[i], c[i + d]] = [c[i + d], c[i]]; return c; };
type Mover = { onMove: (d: -1 | 1) => void; first: boolean; last: boolean; what: string };
function MoveButtons({ onMove, first, last, what }: Mover) {
  return (
    <span className="row" style={{ gap: 2 }}>
      <Button size="small" disabled={first} onClick={() => onMove(-1)} aria-label={`Move ${what} up`} title="Move up">↑</Button>
      <Button size="small" disabled={last} onClick={() => onMove(1)} aria-label={`Move ${what} down`} title="Move down">↓</Button>
    </span>
  );
}

// ---------------------------------------------------------------- exact-file pins
/** Pick one exact file for a candidate, from every version the project has (any Minecraft version or loader). */
function PinPicker({ c, onChange, onClose }: { c: PCandidate; onChange: (c: PCandidate) => void; onClose: () => void }) {
  const [list, setList] = useState<AnyVersion[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [filter, setFilter] = useState("");
  const [force, setForce] = useState(!!c.forcePin);
  useEffect(() => { if (c.modrinthId) api.allVersions(c.modrinthId).then(setList, setError); }, [c.modrinthId]);
  const shown = (list ?? []).filter((v) => !filter || `${v.versionNumber} ${v.gameVersions.join(" ")} ${v.loaders.join(" ")}`.toLowerCase().includes(filter.toLowerCase()));
  return (
    <Modal title={`Pin a file: ${c.name}`} width={620} onClose={onClose}>
      <div className="stack-lg">
        <p className="muted" style={{ margin: 0 }}>The payload then always adds exactly this file. Limit the candidate to the versions/loaders it's meant for (the chips and field on its row), and add an unpinned fallback below it for everything else.</p>
        <label className="mc-check">
          <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} />
          <span>Use it even if it isn't built for the pack's version or loader <span className="muted">(for files another mod needs exactly, like Voxy's for Roxy)</span></span>
        </label>
        {!c.modrinthId ? <Notice title="Pinning needs the Modrinth copy">This candidate is only linked on CurseForge.</Notice> : (
          <>
            <input className="mc-input" placeholder="Filter: version, Minecraft version, loader" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filter versions" />
            {error && <ErrorNotice error={error} />}
            {!list && !error && <p className="muted">Loading every version…</p>}
            <ul className="entry-list" style={{ maxHeight: 340, overflow: "auto" }}>
              {shown.slice(0, 200).map((v) => (
                <li key={v.versionId} className={`entry${c.pinVersion === v.versionId ? " entry--sel" : ""}`}
                  onClick={() => { onChange({ ...c, pinVersion: v.versionId, pinNumber: v.versionNumber, forcePin: force || undefined }); onClose(); }}>
                  <span className="grow"><strong>{v.versionNumber}</strong> <span className="muted">· {v.versionType}</span></span>
                  <span className="muted" style={{ fontSize: 12 }}>{v.gameVersions.slice(-3).join(", ")} · {v.loaders.join(", ")}</span>
                </li>
              ))}
            </ul>
          </>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          {c.pinVersion && <Button onClick={() => { onChange({ ...c, pinVersion: undefined, pinNumber: undefined, forcePin: undefined }); onClose(); }}>Remove the pin</Button>}
          {c.pinVersion && force !== !!c.forcePin && <Button variant="primary" onClick={() => { onChange({ ...c, forcePin: force || undefined }); onClose(); }}>Save</Button>}
          <Button onClick={onClose}>Close</Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- slot editor
function CandidateRow({ c, index, count, onChange, onMove, onRemove }: {
  c: PCandidate; index: number; count: number; onChange: (c: PCandidate) => void; onMove: (d: -1 | 1) => void; onRemove: () => void;
}) {
  const loaders = c.loaders ?? [];
  const [versions, setVersions] = useState((c.mcVersions ?? []).join(", "));
  const toggle = (l: Loader) => onChange({ ...c, loaders: loaders.includes(l) ? loaders.filter((x) => x !== l) : [...loaders, l] });
  const [pinning, setPinning] = useState(false);
  return (
    <li className="cand">
      <span className="muted cand__n" title={index === 0 ? "Tried first" : "Fallback"}>{index + 1}.</span>
      <ModIcon url={c.iconUrl} label={c.name} size={24} />
      <span className="grow">
        <strong>{c.name}</strong>
        {c.modrinthId && <span className="mini-badge mr">MR</span>}{c.curseforgeId != null && <span className="mini-badge cf">CF</span>}
        {c.kind && c.kind !== "mod" && <span className="mini-badge">{c.kind}</span>}
      </span>
      <button type="button" className="chip" aria-pressed={!!c.pinVersion} onClick={() => setPinning(true)}
        title={c.pinVersion ? `Always this exact file${c.forcePin ? ", even when it isn't built for the pack" : ""}` : "Pin an exact file"}>
        {c.pinVersion ? `= ${c.pinNumber ?? c.pinVersion}${c.forcePin ? " (forced)" : ""}` : "Pin…"}
      </button>
      {pinning && <PinPicker c={c} onChange={onChange} onClose={() => setPinning(false)} />}
      <span className="row" style={{ gap: 2 }} title="Only use on these loaders (none ticked = any)">
        {LOADERS.map((l) => (
          <button key={l.id} type="button" className="chip" aria-pressed={loaders.includes(l.id)} onClick={() => toggle(l.id)}>{l.label}</button>
        ))}
      </span>
      <input className="mc-input versions" value={versions} placeholder="any version" title='Only use on these Minecraft versions, comma-separated; "26.*" matches a prefix'
        onChange={(e) => setVersions(e.target.value)}
        onBlur={() => onChange({ ...c, mcVersions: versions.split(",").map((s) => s.trim()).filter(Boolean) })} aria-label="Minecraft versions" />
      <Button size="small" disabled={index === 0} onClick={() => onMove(-1)} aria-label="Try earlier">↑</Button>
      <Button size="small" disabled={index === count - 1} onClick={() => onMove(1)} aria-label="Try later">↓</Button>
      <Button size="small" variant="danger" onClick={onRemove} aria-label={`Remove ${c.name}`}>✕</Button>
    </li>
  );
}

function SlotEditor({ slot, onChange, onRemove, mover }: { slot: PSlot; onChange: (s: PSlot) => void; onRemove: () => void; mover?: Mover }) {
  const [adding, setAdding] = useState(false);
  const set = (cands: PCandidate[]) => onChange({ ...slot, candidates: cands });
  const move = (i: number, d: -1 | 1) => { const c = [...slot.candidates]; [c[i], c[i + d]] = [c[i + d], c[i]]; set(c); };
  return (
    <div className="slot-ed">
      <div className="row">
        <input className="mc-input slot-label" placeholder={slot.candidates[0]?.name ?? "Slot label"} value={slot.label}
          onChange={(e) => onChange({ ...slot, label: e.target.value })} aria-label="Slot label" />
        <span className="grow muted" style={{ fontSize: 12 }}>{slot.candidates.length > 1 ? "first candidate that fits the pack and has a build wins" : ""}</span>
        {mover && <MoveButtons {...mover} />}
        <Button size="small" onClick={() => setAdding(true)}>+ Fallback</Button>
        <Button size="small" variant="danger" onClick={onRemove} aria-label="Remove slot">Remove</Button>
      </div>
      <ul className="cand-list">
        {slot.candidates.map((c, i) => (
          <CandidateRow key={`${c.modrinthId ?? ""}:${c.curseforgeId ?? ""}:${i}`} c={c} index={i} count={slot.candidates.length}
            onChange={(n) => set(slot.candidates.map((x, j) => (j === i ? n : x)))} onMove={(d) => move(i, d)}
            onRemove={() => set(slot.candidates.filter((_, j) => j !== i))} />
        ))}
      </ul>
      {adding && <ModPicker placeholder="Add a fallback mod…" onClose={() => setAdding(false)} onPick={(c) => { set([...slot.candidates, c]); setAdding(false); }} />}
    </div>
  );
}

function SlotList({ slots, onChange, addLabel }: { slots: PSlot[]; onChange: (s: PSlot[]) => void; addLabel: string }) {
  const [adding, setAdding] = useState(false);
  return (
    <div className="stack" style={{ gap: 4 }}>
      {slots.map((s, i) => (
        <SlotEditor key={s.id} slot={s} onChange={(n) => onChange(slots.map((x, j) => (j === i ? n : x)))} onRemove={() => onChange(slots.filter((_, j) => j !== i))}
          mover={slots.length > 1 ? { onMove: (d) => onChange(swap(slots, i, d)), first: i === 0, last: i === slots.length - 1, what: s.label || s.candidates[0]?.name || "mod" } : undefined} />
      ))}
      {adding ? <ModPicker onClose={() => setAdding(false)} onPick={(c) => { onChange([...slots, { id: uid(), label: "", candidates: [c] }]); setAdding(false); }} />
        : <div><Button size="small" onClick={() => setAdding(true)}>{addLabel}</Button></div>}
    </div>
  );
}

function ChoiceEditor({ item, onChange, onRemove, mover }: { item: Extract<PItem, { kind: "choice" }>; onChange: (i: PItem) => void; onRemove: () => void; mover?: Mover }) {
  const setOpt = (i: number, o: PChoiceOption) => onChange({ ...item, options: item.options.map((x, j) => (j === i ? o : x)) });
  return (
    <div className="choice-ed">
      <div className="row">
        <span className="pixel">One of:</span>
        <input className="mc-input slot-label" value={item.label} onChange={(e) => onChange({ ...item, label: e.target.value })} placeholder="Choice label" aria-label="Choice label" />
        <span className="grow" />
        {mover && <MoveButtons {...mover} />}
        <Button size="small" onClick={() => onChange({ ...item, options: [...item.options, { id: uid(), label: `Option ${item.options.length + 1}`, slots: [] }] })}>+ Option</Button>
        <Button size="small" variant="danger" onClick={onRemove}>Remove choice</Button>
      </div>
      {item.options.map((o, i) => (
        <div key={o.id} className="option-ed">
          <div className="row">
            <input className="mc-input slot-label" value={o.label} onChange={(e) => setOpt(i, { ...o, label: e.target.value })} aria-label="Option label" />
            <span className="grow" />
            {item.options.length > 1 && <MoveButtons onMove={(d) => onChange({ ...item, options: swap(item.options, i, d) })} first={i === 0} last={i === item.options.length - 1} what={`option ${o.label}`} />}
            <Button size="small" variant="danger" onClick={() => onChange({ ...item, options: item.options.filter((_, j) => j !== i) })}>Remove option</Button>
          </div>
          <SlotList slots={o.slots} onChange={(slots) => setOpt(i, { ...o, slots })} addLabel="+ Mod in this option" />
        </div>
      ))}
      <div className="option-ed option-ed--common">
        <div className="muted" style={{ fontSize: 12 }}>Comes with every option:</div>
        <SlotList slots={item.common} onChange={(common) => onChange({ ...item, common })} addLabel="+ Common mod" />
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- payload editor
function PayloadEditor({ initial, all, onSaved, onDeleted }: { initial: Payload; all: Payload[]; onSaved: (p: Payload) => void; onDeleted: () => void }) {
  const [p, setP] = useState(initial);
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [notes, setNotes] = useState<string[]>([]);
  const [adding, setAdding] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => { setP(initial); setDirty(false); setNotes([]); setError(null); }, [initial.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const edit = (fn: (p: Payload) => Payload) => { setP(fn); setDirty(true); };
  const setItem = (i: number, it: PItem) => edit((x) => ({ ...x, items: x.items.map((y, j) => (j === i ? it : y)) }));
  const removeItem = (i: number) => edit((x) => ({ ...x, items: x.items.filter((_, j) => j !== i) }));
  const save = async () => {
    setError(null);
    try { const r = await api.savePayload(p); setP(r.payload); setNotes(r.notes); setDirty(false); onSaved(r.payload); return true; }
    catch (e) { setError(e as AppError); return false; }
  };
  // Publish state for the leave-without-saving prompt; cleared when the editor goes away.
  unsaved.isDirty = () => dirty; unsaved.name = p.name; unsaved.nameNow = null; unsaved.save = save; unsaved.hint = "Payloads only change when you save them.";
  useEffect(() => () => { unsaved.isDirty = () => false; }, []);
  const others = all.filter((x) => x.id !== p.id);
  return (
    <div className="stack-lg">
      <div className="row" style={{ alignItems: "flex-end" }}>
        <div className="grow"><Input label="Name" value={p.name} onChange={(e) => edit((x) => ({ ...x, name: e.target.value }))} /></div>
        <span className="muted" style={{ fontSize: 12, paddingBottom: 8 }}>v{p.version}{p.starter ? " · starter" : ""} · {describePayload(p)}</span>
      </div>
      <Input label="Description" value={p.description} onChange={(e) => edit((x) => ({ ...x, description: e.target.value }))} />
      {others.length > 0 && (
        <details className="export-list" open={p.includes.length > 0}>
          <summary>Includes other payloads ({p.includes.length})</summary>
          <div className="row" style={{ gap: 6, marginTop: 6 }}>
            {others.map((o) => (
              <label key={o.id} className="mc-check" style={{ fontSize: 13 }}>
                <input type="checkbox" checked={p.includes.includes(o.id)} onChange={(e) => edit((x) => ({ ...x, includes: e.target.checked ? [...x.includes, o.id] : x.includes.filter((i) => i !== o.id) }))} />
                {o.name}
              </label>
            ))}
          </div>
        </details>
      )}
      <div className="stack">
        {p.items.map((it, i) => {
          const mover: Mover | undefined = p.items.length > 1 ? {
            onMove: (d) => edit((x) => ({ ...x, items: swap(x.items, i, d) })), first: i === 0, last: i === p.items.length - 1,
            what: it.label || (it.kind === "mod" ? it.candidates[0]?.name : "choice") || "item",
          } : undefined;
          return it.kind === "mod"
            ? <SlotEditor key={it.id} slot={it} onChange={(s) => setItem(i, { kind: "mod", ...s })} onRemove={() => removeItem(i)} mover={mover} />
            : <ChoiceEditor key={it.id} item={it} onChange={(n) => setItem(i, n)} onRemove={() => removeItem(i)} mover={mover} />;
        })}
        {p.items.length === 0 && <p className="muted">No mods yet. Add one below, or nest other payloads above.</p>}
        {adding ? <ModPicker onClose={() => setAdding(false)} onPick={(c) => { edit((x) => ({ ...x, items: [...x.items, { kind: "mod", id: uid(), label: "", candidates: [c] }] })); setAdding(false); }} /> : (
          <div className="row">
            <Button variant="primary" onClick={() => setAdding(true)}>+ Mod</Button>
            <Button onClick={() => edit((x) => ({ ...x, items: [...x.items, { kind: "choice", id: uid(), label: "Choice", options: [{ id: uid(), label: "Option 1", slots: [] }, { id: uid(), label: "Option 2", slots: [] }], common: [] }] }))}>+ Choice ("one of")</Button>
          </div>
        )}
      </div>
      {error && <ErrorNotice error={error} />}
      {notes.length > 0 && <Notice title="Saved with some tidying">{notes.map((n, i) => <div key={i}>{n}</div>)}</Notice>}
      <div className="row" style={{ justifyContent: "space-between" }}>
        <div className="row">
          <Button variant="danger" onClick={() => setConfirmDelete(true)}>Delete</Button>
          <Button onClick={async () => { try { await api.exportPayload(p.id); } catch (e) { setError(e as AppError); } }} disabled={dirty} title={dirty ? "Save first" : undefined}>Export…</Button>
          <Button onClick={async () => {
            try { const r = await api.savePayload({ ...p, id: crypto.randomUUID(), name: `${p.name} (copy)`, starter: false, version: 1 }); onSaved(r.payload); }
            catch (e) { setError(e as AppError); }
          }}>Duplicate</Button>
        </div>
        <Button variant="primary" disabled={!dirty} onClick={save}>{dirty ? "Save" : "Saved"}</Button>
      </div>
      {confirmDelete && (
        <Modal title="Delete payload?" width={440} onClose={() => setConfirmDelete(false)}>
          <div className="stack-lg">
            <p>Delete <strong>{p.name}</strong>? Packs that used it keep their mods.{p.starter ? " Starter payloads can be restored from the Payloads screen." : ""}</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirmDelete(false)}>Cancel</Button>
              <Button variant="danger" onClick={async () => { await api.deletePayload(p.id); setConfirmDelete(false); onDeleted(); }}>Delete</Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- screen
export function Payloads() {
  const [list, setList] = useState<Payload[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const refresh = async (select?: string) => {
    try { const l = await api.listPayloads(); setList(l); if (select) setSel(select); else if (!sel && l[0]) setSel(l[0].id); }
    catch (e) { setError(e as AppError); }
  };
  useEffect(() => { refresh(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const current = useMemo(() => list?.find((p) => p.id === sel) ?? null, [list, sel]);
  const { guard, prompt } = useUnsavedGuard();
  const pick = (id: string) => { if (id !== sel) guard(() => setSel(id)); };
  return (
    <div className="payloads-screen">
      <Panel title="Payloads" className="pane" actions={<>
        <Button size="small" onClick={() => guard(async () => { try { const r = await api.importPayload(); if (r) refresh(r.payload.id); } catch (e) { setError(e as AppError); } })}>Import…</Button>
        <Button size="small" variant="primary" onClick={() => guard(async () => { const r = await api.savePayload(newPayload()); refresh(r.payload.id); })}>New</Button>
      </>}>
        <div className="pane__body stack">
          <p className="muted" style={{ fontSize: 12, margin: 0 }}>Reusable bundles you drop into any pack. They store projects, not files, so one payload works on every version and loader.</p>
          {error && <ErrorNotice error={error} />}
          {note && <Notice tone="ok" title={note} />}
          <ul className="entry-list">
            {(list ?? []).map((p) => (
              <li key={p.id} className={`entry${sel === p.id ? " entry--sel" : ""}`} tabIndex={0} onClick={() => pick(p.id)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(p.id); } }}>
                <div className="grow">
                  <div className="hit__name">{p.name}</div>
                  <div className="hit__meta">{describePayload(p)} · v{p.version}{p.starter ? " · starter" : ""}</div>
                </div>
              </li>
            ))}
          </ul>
          {list && list.length === 0 && <div className="empty-state"><p>No payloads. Press New to bundle mods you always use, Import one someone shared, or bring back the starters below.</p></div>}
          <Button size="small" onClick={async () => { const n = await api.restoreStarterPayloads(); setNote(n ? `Restored ${n} starter payload${n === 1 ? "" : "s"}.` : "All starter payloads are already here."); refresh(); }}>Restore starter payloads</Button>
        </div>
      </Panel>
      <Panel title={current ? current.name : "Payload"} className="pane">
        <div className="pane__body">
          {current ? <PayloadEditor initial={current} all={list ?? []} onSaved={(p) => refresh(p.id)} onDeleted={() => { setSel(null); refresh(); }} />
            : <div className="empty-state"><p>Pick a payload, or make a new one.</p></div>}
        </div>
      </Panel>
      {prompt}
    </div>
  );
}
