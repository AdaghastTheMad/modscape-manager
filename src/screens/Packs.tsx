import { useEffect, useState } from "react";
import { api, type AppError, type ImportReport } from "../api";
import { Button, ErrorNotice, Modal, Notice, Panel, Slot } from "../components/kit";
import { loaderLabel, type PackSummary } from "../model";
import { NewPackWizard } from "./NewPack";
import { ImportTemplateDialog } from "./Templates";

export function Packs({ onOpen }: { onOpen: (id: string) => void }) {
  const [packs, setPacks] = useState<PackSummary[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [notes, setNotes] = useState<string[]>([]);
  const [creating, setCreating] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<PackSummary | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [importing, setImporting] = useState(false);
  const [tplFrom, setTplFrom] = useState<ImportReport | null>(null);
  const [tplNote, setTplNote] = useState<string | null>(null);
  const runImport = async (fn: () => Promise<ImportReport | null>) => {
    setError(null); setNotes([]); setImporting(true); setTplNote(null);
    try { const r = await fn(); if (r) { setReport(r); await refresh(); } }
    catch (e) { setError(e as AppError); }
    finally { setImporting(false); }
  };

  const refresh = () => api.listPacks().then(setPacks, setError);
  useEffect(() => { refresh(); }, []);

  const importProject = async () => {
    setError(null); setNotes([]);
    try {
      const saved = await api.importProject();
      if (saved) { setNotes(saved.notes); await refresh(); if (!saved.notes.length) onOpen(saved.pack.id); }
    } catch (e) { setError(e as AppError); }
  };

  const exportProject = async (id: string) => {
    setError(null);
    try { const pack = await api.getPack(id); await api.exportProject(pack); } catch (e) { setError(e as AppError); }
  };

  return (
    <div className="settings stack-lg">
      <Panel title="Packs" actions={<>
        <Button onClick={importProject}>Open project…</Button>
        <Button onClick={() => runImport(api.importPackFile)} disabled={importing} title=".mrpack or CurseForge zip">Import pack…</Button>
        <Button onClick={() => runImport(api.importPrismInstance)} disabled={importing} title="Reads a Prism instance folder without changing it">Import Prism instance…</Button>
        <Button variant="primary" onClick={() => setCreating(true)}>New pack</Button>
      </>}>
        <div className="stack">
          {importing && <Notice title="Importing…">Identifying every mod from its metadata.</Notice>}
          {error && <ErrorNotice error={error} />}
          {notes.length > 0 && <Notice title="Imported, with some repairs">{notes.map((n, i) => <div key={i}>{n}</div>)}</Notice>}
          {packs === null ? <p className="muted">Loading…</p> : packs.length === 0 ? (
            <div className="empty-state" style={{ padding: "48px 0" }}>
              <div><div className="pixel">No packs yet</div><p>Create a new pack, or open a planner project file.</p></div>
            </div>
          ) : (
            <ul className="result-list">
              {packs.map((p) => (
                <li key={p.id} className="result pack-row">
                  <Slot src={p.icon} label={p.name} />
                  <button type="button" className="pack-row__main" onClick={() => onOpen(p.id)}>
                    <span className="result__name">{p.name}</span>
                    <span className="muted">Minecraft {p.mcVersion} · {loaderLabel(p.loader)} · {p.entryCount} mod{p.entryCount === 1 ? "" : "s"} · edited {new Date(p.updatedAt).toLocaleString()}</span>
                  </button>
                  <div className="row">
                    <Button size="small" variant="primary" onClick={() => onOpen(p.id)}>Open</Button>
                    <Button size="small" onClick={() => exportProject(p.id)}>Save as file…</Button>
                    <Button size="small" variant="danger" onClick={() => setConfirmDelete(p)}>Delete</Button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Panel>
      {creating && <NewPackWizard onClose={() => setCreating(false)} onCreated={(id) => { setCreating(false); onOpen(id); }} />}
      {report && (
        <Modal title={`Imported ${report.pack.name}`} width={560} onClose={() => setReport(null)}>
          <div className="stack-lg">
            <p className="muted">Minecraft {report.pack.mcVersion} · {loaderLabel(report.pack.loader)} {report.pack.loaderVersion} · {Object.keys(report.pack.entries).length} mods, pinned to the versions the pack used.</p>
            <ul className="dep-list">{report.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
            {report.unknown.length > 0 && (
              <Notice title={`${report.unknown.length} file${report.unknown.length === 1 ? "" : "s"} couldn't be identified`}>
                <div className="muted">They aren't on Modrinth or CurseForge (or need a CurseForge key). Add them by hand if you need them:</div>
                <ul className="dep-list">{report.unknown.map((u) => <li key={u}><code>{u}</code></li>)}</ul>
              </Notice>
            )}
            {report.overrideSource && !tplNote && (
              <Notice title="It also has config files">
                <div className="muted">{report.overrideSource.entries.map((e) => e.name + (e.isDir ? "/" : "")).join(", ")}. Save them as an override template, attached to this pack, so its exports carry them.</div>
                <Button size="small" onClick={() => setTplFrom(report)}>Save as template…</Button>
              </Notice>
            )}
            {tplNote && <Notice tone="ok" title={tplNote} />}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setReport(null)}>Close</Button>
              <Button variant="primary" onClick={() => { const id = report.pack.id; setReport(null); onOpen(id); }}>Open pack</Button>
            </div>
          </div>
        </Modal>
      )}
      {tplFrom?.overrideSource && (
        <ImportTemplateDialog initial={{ ...tplFrom.overrideSource, suggestedName: `${tplFrom.pack.name} configs` }} onClose={() => setTplFrom(null)} onDone={async (t) => {
          try {
            const p = await api.getPack(tplFrom.pack.id);
            const ref = await api.templateAppliedRef(t.id);
            await api.savePack({ ...p, overrideTemplates: [...p.overrideTemplates, t.id], appliedTemplates: [...p.appliedTemplates.filter((a) => a.id !== t.id), ref] });
            setTplNote(`Saved template "${t.name}" (${t.files.length} files) and attached it to ${p.name}.`);
          } catch (e) { setError(e as AppError); }
        }} />
      )}
      {confirmDelete && (
        <Modal title="Delete pack?" width={460} onClose={() => setConfirmDelete(null)}>
          <div className="stack-lg">
            <p>Delete <strong>{confirmDelete.name}</strong> from the app? This can't be undone. Any planner files you saved stay where they are.</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirmDelete(null)}>Cancel</Button>
              <Button variant="danger" onClick={async () => {
                try { await api.deletePack(confirmDelete.id); } catch (e) { setError(e as AppError); }
                setConfirmDelete(null); refresh();
              }}>Delete</Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}
