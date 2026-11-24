import { useEffect, useMemo, useState } from "react";
import { api, type AppError, type Overview, type Template, type TemplateChange } from "../../api";
import { Button, ErrorNotice, Modal, Notice, Select } from "../../components/kit";
import type { AppliedTemplate, Pack } from "../../model";
import { describeTemplate, fmtBytes } from "../Templates";

/** The pack's Overrides tab: attached templates in order, clashes, mod awareness, updates. */
export function Overrides({ pack, overview, error, onUpdate, onSelect }: {
  pack: Pack; overview: Overview | null; error: AppError | null;
  onUpdate: (fn: (p: Pack) => Pack) => void; onSelect: (key: string) => void;
}) {
  const [library, setLibrary] = useState<Template[] | null>(null);
  const [adding, setAdding] = useState("");
  const [review, setReview] = useState<TemplateChange | null>(null);
  const [err, setErr] = useState<AppError | null>(null);
  const [showAll, setShowAll] = useState(false);
  useEffect(() => { api.listTemplates().then(setLibrary, setErr); }, []);
  const ids = pack.overrideTemplates;
  const byId = useMemo(() => new Map([...(library ?? []), ...(overview?.attached ?? [])].map((t) => [t.id, t])), [library, overview]);
  const name = (id: string) => byId.get(id)?.name ?? "Missing template";
  const available = (library ?? []).filter((t) => !ids.includes(t.id));

  const setRef = (ref: AppliedTemplate) => onUpdate((p) => ({ ...p, appliedTemplates: [...p.appliedTemplates.filter((a) => a.id !== ref.id), ref] }));
  const attach = async (id: string) => {
    try {
      const ref = await api.templateAppliedRef(id);
      onUpdate((p) => p.overrideTemplates.includes(id) ? p : ({ ...p, overrideTemplates: [...p.overrideTemplates, id], appliedTemplates: [...p.appliedTemplates.filter((a) => a.id !== id), ref] }));
      setAdding("");
    } catch (e) { setErr(e as AppError); }
  };
  const detach = (id: string) => onUpdate((p) => ({ ...p, overrideTemplates: p.overrideTemplates.filter((x) => x !== id), appliedTemplates: p.appliedTemplates.filter((a) => a.id !== id) }));
  const move = (i: number, d: -1 | 1) => onUpdate((p) => {
    const l = [...p.overrideTemplates]; [l[i], l[i + d]] = [l[i + d], l[i]];
    return { ...p, overrideTemplates: l };
  });

  const clashes = overview?.merged.filter((m) => m.from.length > 1) ?? [];
  const unused = overview?.mods.filter((m) => !m.entry) ?? [];
  const used = overview?.mods.filter((m) => m.entry) ?? [];
  return (
    <div className="pane__body stack-lg">
      <p className="muted" style={{ margin: 0, fontSize: 12 }}>
        Templates are copied into the export's overrides/ folder, in this order: when two have the same file, the one lower in the list wins. Edit templates on the Templates tab.
      </p>
      {(error || err) && <ErrorNotice error={(error ?? err)!} />}
      {overview?.updates.map((u) => (
        <Notice key={u.id} title={`Template "${u.name}" changed (v${u.fromVersion} → v${u.toVersion})`}>
          <div className="row" style={{ justifyContent: "space-between" }}>
            <span className="muted">{[u.added.length && `${u.added.length} added`, u.changed.length && `${u.changed.length} changed`, u.removed.length && `${u.removed.length} removed`].filter(Boolean).join(", ") || "Files changed"}</span>
            <Button size="small" onClick={() => setReview(u)}>Review</Button>
          </div>
        </Notice>
      ))}
      {overview?.missing.map((id) => (
        <Notice key={id} tone="error" title="An attached template no longer exists">
          <Button size="small" onClick={() => detach(id)}>Detach it</Button>
        </Notice>
      ))}

      <section className="stack">
        <div className="pixel">Attached ({ids.length})</div>
        {ids.length === 0 && <div className="empty-state"><p>No templates attached. Attach one below; the pack's exports will carry its files.</p></div>}
        <ol className="tpl-order">
          {ids.map((id, i) => {
            const t = byId.get(id);
            return (
              <li key={id} className="entry">
                <span className="muted cand__n">{i + 1}.</span>
                <div className="grow">
                  <div className="hit__name">{name(id)}</div>
                  <div className="hit__meta">{t ? `${describeTemplate(t)} · v${t.version}` : "deleted from the library"}</div>
                </div>
                <Button size="small" disabled={i === 0} onClick={() => move(i, -1)} aria-label={`Move ${name(id)} up`}>↑</Button>
                <Button size="small" disabled={i === ids.length - 1} onClick={() => move(i, 1)} aria-label={`Move ${name(id)} down (wins clashes)`}>↓</Button>
                <Button size="small" variant="danger" onClick={() => detach(id)}>Detach</Button>
              </li>
            );
          })}
        </ol>
        <div className="row">
          <Select aria-label="Template to attach" value={adding} onChange={(e) => setAdding(e.target.value)}>
            <option value="">{available.length ? "Choose a template…" : library?.length ? "All templates are attached" : "No templates in the library yet"}</option>
            {available.map((t) => <option key={t.id} value={t.id}>{t.name} ({describeTemplate(t)})</option>)}
          </Select>
          <Button variant="primary" disabled={!adding} onClick={() => attach(adding)}>Attach</Button>
        </div>
      </section>

      {overview && overview.merged.length > 0 && (
        <section className="stack">
          <div className="pixel">In overrides/ · {overview.merged.length} file{overview.merged.length === 1 ? "" : "s"}{clashes.length ? ` · ${clashes.length} clash${clashes.length === 1 ? "" : "es"}` : ""}</div>
          {clashes.length > 0 && (
            <ul className="plan-rows">
              {clashes.map((m) => (
                <li key={m.path} className="plan-row">
                  <span className="warn">⚠</span>
                  <span className="grow" style={{ wordBreak: "break-all" }}><strong>{m.path}</strong>
                    <span className="muted"> · {m.from.slice(0, -1).map(name).join(", ")} overridden by </span><strong>{name(m.from[m.from.length - 1])}</strong>
                  </span>
                </li>
              ))}
            </ul>
          )}
          <details className="export-list" open={showAll} onToggle={(e) => setShowAll((e.target as HTMLDetailsElement).open)}>
            <summary>All files</summary>
            {showAll && (
              <ul>
                {overview.merged.map((m) => <li key={m.path}>{m.path} <span className="muted">· {fmtBytes(m.size)} · {name(m.from[m.from.length - 1])}</span></li>)}
              </ul>
            )}
          </details>
        </section>
      )}

      {overview && overview.mods.length > 0 && (
        <section className="stack">
          <div className="pixel">Configs by mod</div>
          {unused.length > 0 && (
            <Notice title={`${unused.length} config${unused.length === 1 ? "" : "s"} for mods not in this pack`}>
              <div className="muted" style={{ fontSize: 12 }}>They're still exported (nothing is deleted); the game just ignores them. Matching is by name, so a mod with an unusual config name can show up here too.</div>
              <div style={{ marginTop: 4 }}>{unused.map((m) => <span key={m.hint} className="mini-badge" title={m.files.join("\n")}>{m.hint}</span>)}</div>
            </Notice>
          )}
          <div className="row" style={{ gap: 4, flexWrap: "wrap" }}>
            {used.map((m) => (
              <button key={m.hint} type="button" className="chip" title={m.files.join("\n")} onClick={() => m.entry && onSelect(m.entry)}>
                {m.entryName} <span className="muted">· {m.files.length}</span>
              </button>
            ))}
          </div>
        </section>
      )}

      {review && (
        <Modal title={`Review changes: ${review.name}`} width={560} onClose={() => setReview(null)}>
          <div className="stack-lg">
            <p className="muted" style={{ margin: 0 }}>
              Since you last reviewed it (v{review.fromVersion}), the template changed. Exports always use its current files; marking it reviewed clears the notice.
            </p>
            {([["Added", review.added], ["Changed", review.changed], ["Removed", review.removed]] as const).map(([label, files]) => files.length > 0 && (
              <div key={label}>
                <div className="pixel">{label} ({files.length})</div>
                <ul className="tpl-change">{files.map((f) => <li key={f}>{f}</li>)}</ul>
              </div>
            ))}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => { detach(review.id); setReview(null); }}>Detach instead</Button>
              <Button variant="primary" onClick={async () => { try { setRef(await api.templateAppliedRef(review.id)); setReview(null); } catch (e) { setErr(e as AppError); } }}>Mark reviewed</Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}
