import { useMemo, useState } from "react";
import { Button, Input, Modal, Notice, Select } from "../../components/kit";
import { diffPacks, snapshotsOf, stateOf, type PackDiff, type Snapshot } from "../../history";
import { loaderLabel, nowIso, type Pack } from "../../model";

const fmtWhen = (iso: string) => { try { return new Date(iso).toLocaleString(); } catch { return iso; } };
const modCount = (s: Snapshot) => Object.keys(s.state.entries).length;

export function DiffView({ diff, fromLabel, toLabel }: { diff: PackDiff; fromLabel: string; toLabel: string }) {
  if (diff.same) return <Notice tone="ok" title={`No differences between ${fromLabel} and ${toLabel}`} />;
  const user = (xs: PackDiff["added"]) => xs.filter((x) => !x.dependency);
  const deps = (xs: PackDiff["added"]) => xs.filter((x) => x.dependency);
  const names = (xs: { name: string }[]) => xs.map((x) => x.name).join(", ");
  return (
    <div className="stack diff">
      <div className="muted" style={{ fontSize: 12 }}>Going from {fromLabel} to {toLabel}:</div>
      {diff.settings.length > 0 && (
        <section><div className="pixel">Settings</div><ul className="dep-list">{diff.settings.map((s) => <li key={s}>{s}</li>)}</ul></section>
      )}
      {diff.added.length > 0 && (
        <section>
          <div className="pixel diff-add">+ Added ({diff.added.length})</div>
          {user(diff.added).length > 0 && <p>{names(user(diff.added))}</p>}
          {deps(diff.added).length > 0 && <p className="muted">Dependencies: {names(deps(diff.added))}</p>}
        </section>
      )}
      {diff.removed.length > 0 && (
        <section>
          <div className="pixel diff-del">− Removed ({diff.removed.length})</div>
          {user(diff.removed).length > 0 && <p>{names(user(diff.removed))}</p>}
          {deps(diff.removed).length > 0 && <p className="muted">Dependencies: {names(deps(diff.removed))}</p>}
        </section>
      )}
      {diff.changed.length > 0 && (
        <section>
          <div className="pixel">~ Changed ({diff.changed.length})</div>
          <ul className="dep-list">{diff.changed.map((c) => <li key={c.key}><strong>{c.name}</strong>: {c.changes.join("; ")}</li>)}</ul>
        </section>
      )}
    </div>
  );
}

/** Named snapshots of the whole pack: take, rename, compare, roll back, delete. */
export function SnapshotsDialog({ pack, onClose, onSnapshots, onRollback }: {
  pack: Pack; onClose: () => void;
  /** Replace the snapshot list (not an undo step). */
  onSnapshots: (s: Snapshot[]) => void;
  /** Roll the pack back (an undo step), optionally keeping the current state as a snapshot first. */
  onRollback: (s: Snapshot, keepCurrent: Snapshot | null) => void;
}) {
  const snaps = snapshotsOf(pack);
  const [name, setName] = useState(`Snapshot ${new Date().toLocaleDateString()}`);
  const [compare, setCompare] = useState<{ a: string; b: string } | null>(null);
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "rollback" | "delete"; s: Snapshot } | null>(null);
  const [keepCurrent, setKeepCurrent] = useState(true);
  const take = (n: string): Snapshot => ({ id: crypto.randomUUID(), name: n.trim() || "Snapshot", at: nowIso(), state: stateOf(pack) });

  const pick = (id: string) => (id === "now" ? stateOf(pack) : snaps.find((s) => s.id === id)?.state ?? null);
  const label = (id: string) => (id === "now" ? "now" : `“${snaps.find((s) => s.id === id)?.name ?? "?"}”`);
  const diff = useMemo(() => {
    if (!compare) return null;
    const a = pick(compare.a), b = pick(compare.b);
    return a && b ? diffPacks(a, b) : null;
  }, [compare, pack]); // eslint-disable-line react-hooks/exhaustive-deps
  const sinceNow = (s: Snapshot) => diffPacks(s.state, stateOf(pack));

  return (
    <Modal title="Snapshots" width={720} onClose={onClose}>
      <div className="stack-lg">
        <p className="muted" style={{ margin: 0 }}>A snapshot saves the whole pack (mods, pins, tags, notes, settings, templates) so you can compare against it or roll back. Snapshots are kept with the pack and in its planner file, never in exports.</p>
        <form className="row" style={{ alignItems: "flex-end" }} onSubmit={(e) => { e.preventDefault(); onSnapshots([...snaps, take(name)]); setName(`Snapshot ${new Date().toLocaleDateString()} (${snaps.length + 2})`); }}>
          <div className="grow"><Input label="New snapshot" value={name} onChange={(e) => setName(e.target.value)} /></div>
          <Button type="submit" variant="primary" disabled={!name.trim()}>Take snapshot</Button>
        </form>
        {snaps.length === 0 ? <div className="empty-state"><p>No snapshots yet. Take one before a big change (porting, a payload, a cleanup) so you can always get back.</p></div> : (
          <ul className="entry-list">
            {[...snaps].reverse().map((s) => {
              const d = sinceNow(s);
              const delta = d.same ? "same as now" : [d.added.length && `${d.added.length} added since`, d.removed.length && `${d.removed.length} removed since`, d.changed.length && `${d.changed.length} changed`, d.settings.length && "settings differ"].filter(Boolean).join(" · ");
              return (
                <li key={s.id} className="entry" style={{ cursor: "default" }}>
                  <div className="grow">
                    {renaming?.id === s.id ? (
                      <form className="row" style={{ gap: 4 }} onSubmit={(e) => { e.preventDefault(); onSnapshots(snaps.map((x) => (x.id === s.id ? { ...x, name: renaming.name.trim() || x.name } : x))); setRenaming(null); }}>
                        <input className="mc-input grow" autoFocus value={renaming.name} aria-label="Snapshot name" onChange={(e) => setRenaming({ id: s.id, name: e.target.value })} onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setRenaming(null); } }} />
                        <Button size="small" type="submit" variant="primary">Save</Button>
                      </form>
                    ) : <div className="hit__name">{s.name}</div>}
                    <div className="hit__meta">{fmtWhen(s.at)} · {modCount(s)} mods · {s.state.mcVersion} {loaderLabel(s.state.loader)} · {delta}</div>
                  </div>
                  <Button size="small" onClick={() => setCompare({ a: s.id, b: "now" })}>Compare</Button>
                  <Button size="small" onClick={() => setRenaming({ id: s.id, name: s.name })}>Rename</Button>
                  <Button size="small" disabled={d.same} onClick={() => { setKeepCurrent(true); setConfirm({ kind: "rollback", s }); }}>Roll back…</Button>
                  <Button size="small" variant="danger" onClick={() => setConfirm({ kind: "delete", s })} aria-label={`Delete snapshot ${s.name}`}>✕</Button>
                </li>
              );
            })}
          </ul>
        )}
        {compare && (
          <section className="stack">
            <div className="row">
              <span className="pixel">Compare</span>
              <Select aria-label="From" value={compare.a} onChange={(e) => setCompare({ ...compare, a: e.target.value })}>
                {snaps.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
                <option value="now">Now</option>
              </Select>
              <span>→</span>
              <Select aria-label="To" value={compare.b} onChange={(e) => setCompare({ ...compare, b: e.target.value })}>
                {snaps.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
                <option value="now">Now</option>
              </Select>
              <span className="grow" />
              <Button size="small" onClick={() => setCompare(null)}>Close</Button>
            </div>
            {diff && <DiffView diff={diff} fromLabel={label(compare.a)} toLabel={label(compare.b)} />}
          </section>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}><Button onClick={onClose}>Done</Button></div>
      </div>
      {confirm?.kind === "rollback" && (
        <Modal title={`Roll back to “${confirm.s.name}”?`} width={600} onClose={() => setConfirm(null)}>
          <div className="stack-lg">
            <DiffView diff={diffPacks(stateOf(pack), confirm.s.state)} fromLabel="now" toLabel={`“${confirm.s.name}”`} />
            <label className="mc-check"><input type="checkbox" checked={keepCurrent} onChange={(e) => setKeepCurrent(e.target.checked)} /> Take a snapshot of the current state first</label>
            <p className="muted" style={{ margin: 0, fontSize: 12 }}>You can also undo the rollback.</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirm(null)}>Cancel</Button>
              <Button variant="primary" onClick={() => { onRollback(confirm.s, keepCurrent ? take(`Before rolling back to ${confirm.s.name}`) : null); setConfirm(null); onClose(); }}>Roll back</Button>
            </div>
          </div>
        </Modal>
      )}
      {confirm?.kind === "delete" && (
        <Modal title="Delete snapshot?" width={440} onClose={() => setConfirm(null)}>
          <div className="stack-lg">
            <p>Delete <strong>{confirm.s.name}</strong>? The pack itself doesn't change. Snapshots can't be undone once deleted.</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirm(null)}>Cancel</Button>
              <Button variant="danger" onClick={() => { onSnapshots(snaps.filter((x) => x.id !== confirm.s.id)); setConfirm(null); if (compare && (compare.a === confirm.s.id || compare.b === confirm.s.id)) setCompare(null); }}>Delete</Button>
            </div>
          </div>
        </Modal>
      )}
    </Modal>
  );
}
