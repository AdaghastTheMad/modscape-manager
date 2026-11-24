import { useEffect, useMemo, useRef, useState } from "react";
import { api, type AppError, type SourceScan, type Template, type TemplateFile } from "../api";
import { CodeEditor } from "../components/CodeEditor";
import { Button, ErrorNotice, Input, Modal, Notice, Panel } from "../components/kit";
import { fmtCompact } from "../hooks";
import { unsaved, useUnsavedGuard } from "../unsaved";

export function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function describeTemplate(t: Template) {
  const f = t.files.length;
  return `${t.folders.length ? t.folders.join(" ") : "empty"} · ${fmtCompact.format(f)} file${f === 1 ? "" : "s"}`;
}

// ---------------------------------------------------------------- import
const KIND_LABEL: Record<SourceScan["kind"], string> = { folder: "folder", instance: "Prism instance", mrpack: "Modrinth pack", curseforge: "CurseForge pack" };

export function ImportTemplateDialog({ onClose, onDone, initial }: { onClose: () => void; onDone: (t: Template) => void; initial?: SourceScan }) {
  const [scan, setScan] = useState<SourceScan | null>(initial ?? null);
  const [keep, setKeep] = useState<Set<string>>(new Set(initial?.entries.filter((e) => e.suggested).map((e) => e.name)));
  const [name, setName] = useState(initial?.suggestedName ?? "");
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const pick = async (folder: boolean) => {
    setError(null);
    try {
      const s = await api.pickTemplateSource(folder);
      if (!s) return;
      setScan(s); setName(s.suggestedName); setKeep(new Set(s.entries.filter((e) => e.suggested).map((e) => e.name)));
    } catch (e) { setError(e as AppError); }
  };
  return (
    <Modal title="Import a template" width={600} onClose={onClose}>
      <div className="stack-lg">
        <p className="muted" style={{ margin: 0 }}>Copy config files from a folder, a Prism instance, or a modpack's overrides. The source is only read, never changed.</p>
        <div className="row">
          <Button onClick={() => pick(true)}>Folder or Prism instance…</Button>
          <Button onClick={() => pick(false)}>Modpack file (.mrpack / zip)…</Button>
        </div>
        {error && <ErrorNotice error={error} />}
        {scan && (
          <>
            <div className="muted" style={{ fontSize: 12, wordBreak: "break-all" }}>From {KIND_LABEL[scan.kind]}: {scan.path}</div>
            {scan.notes.map((n, i) => <Notice key={i} title={n} />)}
            <Input label="Template name" value={name} onChange={(e) => setName(e.target.value)} />
            <div className="stack" style={{ gap: 2 }}>
              <div className="pixel">Keep</div>
              {scan.entries.map((e) => (
                <label key={e.name} className="mc-check">
                  <input type="checkbox" checked={keep.has(e.name)} onChange={(ev) => { const n = new Set(keep); if (ev.target.checked) n.add(e.name); else n.delete(e.name); setKeep(n); }} />
                  <span className="grow"><strong>{e.name}{e.isDir ? "/" : ""}</strong> <span className="muted">· {e.files} file{e.files === 1 ? "" : "s"} · {fmtBytes(e.bytes)}</span></span>
                </label>
              ))}
            </div>
          </>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!scan || keep.size === 0 || !name.trim() || busy} onClick={async () => {
            if (!scan) return;
            setBusy(true); setError(null);
            try { onDone(await api.importTemplate(scan.path, name, [...keep])); onClose(); }
            catch (e) { setError(e as AppError); setBusy(false); }
          }}>{busy ? "Importing…" : "Import"}</Button>
        </div>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------- file editor
type FileState = { dirty: boolean; save: () => Promise<boolean> };
function FileEditor({ t, file, onChanged, onGone, state }: { t: Template; file: TemplateFile; onChanged: (t: Template) => void; onGone: (t: Template) => void; state: { current: FileState } }) {
  const [text, setText] = useState<string | null>(null);
  const [orig, setOrig] = useState<string | null>(null);
  const [tooBig, setTooBig] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => {
    let live = true;
    setText(null); setOrig(null); setTooBig(false); setError(null); setRenaming(null);
    if (!file.editable) { setTooBig(true); return; }
    api.readTemplateFile(t.id, file.path).then((f) => {
      if (!live) return;
      if (f.text == null) setTooBig(true); else { setText(f.text); setOrig(f.text); }
    }, (e) => live && setError(e));
    return () => { live = false; };
  }, [t.id, file.path]); // eslint-disable-line react-hooks/exhaustive-deps
  const dirty = text != null && text !== orig;
  const save = async () => {
    if (text == null) return true;
    try { const n = await api.writeTemplateFile(t.id, file.path, text); setOrig(text); onChanged(n); return true; }
    catch (e) { setError(e as AppError); return false; }
  };
  state.current = { dirty, save };
  useEffect(() => () => { state.current = { dirty: false, save: async () => true }; }, []); // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <div className="stack" style={{ minWidth: 0 }}>
      <div className="row" style={{ gap: 6 }}>
        {renaming != null ? (
          <>
            <input className="mc-input grow" value={renaming} autoFocus aria-label="New path" onChange={(e) => setRenaming(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Escape") setRenaming(null); }} />
            <Button size="small" variant="primary" disabled={dirty} title={dirty ? "Save first" : undefined} onClick={async () => {
              try { const n = await api.renameTemplateFile(t.id, file.path, renaming); setRenaming(null); onGone(n); } catch (e) { setError(e as AppError); }
            }}>Rename</Button>
            <Button size="small" onClick={() => setRenaming(null)}>Cancel</Button>
          </>
        ) : (
          <>
            <strong className="grow" style={{ wordBreak: "break-all" }}>{file.path}</strong>
            <span className="muted" style={{ fontSize: 12 }}>{fmtBytes(file.size)}</span>
            <Button size="small" onClick={() => setRenaming(file.path)}>Rename…</Button>
            <Button size="small" onClick={() => api.openTemplatePath(t.id, file.path).catch(setError)} title="Open with your own editor">Open externally</Button>
            <Button size="small" variant="danger" onClick={() => setConfirmDelete(true)}>Delete</Button>
          </>
        )}
      </div>
      {error && <ErrorNotice error={error} />}
      {tooBig ? (
        <div className="empty-state"><p>{file.editable ? "This file isn't plain text." : file.size > 256 * 1024 ? "This file is too big for the built-in editor." : "This kind of file can't be edited here."} Use “Open externally” to edit it with your own tools; the app picks up the change.</p></div>
      ) : text == null ? <p className="muted">Loading…</p> : (
        <>
          <CodeEditor path={file.path} value={text} onChange={setText} onSave={save} />
          <div className="row" style={{ justifyContent: "flex-end" }}>
            <span className="muted grow" style={{ fontSize: 12 }}>{dirty ? "Unsaved changes" : "Saved"} · Cmd/Ctrl+S saves · Tab indents, Esc leaves the editor</span>
            <Button disabled={!dirty} onClick={() => setText(orig)}>Revert</Button>
            <Button variant="primary" disabled={!dirty} onClick={save}>Save</Button>
          </div>
        </>
      )}
      {confirmDelete && (
        <Modal title="Delete file?" width={440} onClose={() => setConfirmDelete(false)}>
          <div className="stack-lg">
            <p>Delete <strong>{file.path}</strong> from {t.name}? This removes the file from the template's folder.</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirmDelete(false)}>Cancel</Button>
              <Button variant="danger" onClick={async () => {
                try { state.current = { dirty: false, save: async () => true }; const n = await api.deleteTemplateFile(t.id, file.path); setConfirmDelete(false); onGone(n); } catch (e) { setError(e as AppError); }
              }}>Delete</Button>
            </div>
          </div>
        </Modal>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- template editor
function TemplateEditor({ t, uses, onChanged, onDeleted }: { t: Template; uses: string[]; onChanged: (t: Template) => void; onDeleted: () => void }) {
  const [name, setName] = useState(t.name);
  const [desc, setDesc] = useState(t.description);
  const [sel, setSel] = useState<string | null>(null);
  const [newPath, setNewPath] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [filter, setFilter] = useState("");
  const { guard, prompt } = useUnsavedGuard();
  useEffect(() => { setName(t.name); setDesc(t.description); setSel(null); setNewPath(null); setError(null); setFilter(""); }, [t.id]); // eslint-disable-line react-hooks/exhaustive-deps
  const metaDirty = name !== t.name || desc !== t.description;
  const fileState = useRef<FileState>({ dirty: false, save: async () => true });
  const saveMeta = async () => { try { onChanged(await api.setTemplateMeta(t.id, name, desc)); return true; } catch (e) { setError(e as AppError); return false; } };
  // Leaving (another file, template or tab) asks first if the name/description or the open file has changes.
  unsaved.isDirty = () => metaDirty || fileState.current.dirty;
  unsaved.name = t.name;
  unsaved.nameNow = () => (fileState.current.dirty && sel ? `${sel} (in ${t.name})` : t.name);
  unsaved.hint = "Templates only change when you save them.";
  unsaved.save = async () => (!metaDirty || (await saveMeta())) && (!fileState.current.dirty || (await fileState.current.save()));
  useEffect(() => () => { unsaved.isDirty = () => false; unsaved.nameNow = null; }, []);
  const file = t.files.find((f) => f.path === sel) ?? null;
  const shown = useMemo(() => t.files.filter((f) => !filter || f.path.toLowerCase().includes(filter.toLowerCase())), [t.files, filter]);
  const groups = useMemo(() => {
    const m = new Map<string, TemplateFile[]>();
    for (const f of shown) { const top = f.path.includes("/") ? f.path.split("/")[0] + "/" : "(top level)"; m.set(top, [...(m.get(top) ?? []), f]); }
    return [...m];
  }, [shown]);
  const dir = sel?.includes("/") ? sel.slice(0, sel.lastIndexOf("/") + 1) : "config/";
  return (
    <div className="stack-lg">
      <div className="row" style={{ alignItems: "flex-end" }}>
        <div className="grow"><Input label="Name" value={name} onChange={(e) => setName(e.target.value)} /></div>
        <span className="muted" style={{ fontSize: 12, paddingBottom: 8 }}>v{t.version} · {describeTemplate(t)} · {fmtBytes(t.bytes)}</span>
      </div>
      <Input label="Description" value={desc} onChange={(e) => setDesc(e.target.value)} />
      {metaDirty && (
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={() => { setName(t.name); setDesc(t.description); }}>Revert</Button>
          <Button variant="primary" onClick={saveMeta}>Save name and description</Button>
        </div>
      )}
      <p className="muted" style={{ fontSize: 12, margin: 0 }}>
        Used by {uses.length ? uses.join(", ") : "no packs yet"}. Files live in <code style={{ wordBreak: "break-all" }}>{t.folder}</code>; edit them here or with your own tools. Any change makes a new version, and packs using it are told.
      </p>
      {error && <ErrorNotice error={error} />}
      <div className="tpl-split">
        <div className="stack" style={{ minWidth: 0 }}>
          <div className="row" style={{ gap: 4 }}>
            <input className="mc-input grow" placeholder="Filter files" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filter files" />
          </div>
          <div className="row" style={{ gap: 4 }}>
            <Button size="small" onClick={() => setNewPath(dir)}>New file…</Button>
            <Button size="small" onClick={async () => { try { const n = await api.addTemplateFiles(t.id, dir); if (n) onChanged(n); } catch (e) { setError(e as AppError); } }} title={`Copy files from disk into ${dir}`}>Add files…</Button>
            <Button size="small" onClick={() => api.openTemplatePath(t.id).catch(setError)}>Open folder</Button>
          </div>
          {newPath != null && (
            <div className="row" style={{ gap: 4 }}>
              <input className="mc-input grow" value={newPath} autoFocus aria-label="New file path" placeholder="config/mymod.toml" onChange={(e) => setNewPath(e.target.value)}
                onKeyDown={(e) => { if (e.key === "Escape") setNewPath(null); }} />
              <Button size="small" variant="primary" disabled={!newPath.trim() || newPath.endsWith("/")} onClick={async () => {
                try { const n = await api.writeTemplateFile(t.id, newPath, ""); const p = newPath; setNewPath(null); onChanged(n); setSel(n.files.find((f) => f.path.endsWith(p.replace(/^\.?\/+/, "")))?.path ?? null); }
                catch (e) { setError(e as AppError); }
              }}>Create</Button>
              <Button size="small" onClick={() => setNewPath(null)}>Cancel</Button>
            </div>
          )}
          <div className="tpl-files">
            {t.files.length === 0 && <p className="muted">No files yet. Import from a folder, create one, or add files from disk.</p>}
            {groups.map(([g, files]) => (
              <section key={g}>
                <div className="group__title">{g} <span className="muted">({files.length})</span></div>
                <ul className="entry-list">
                  {files.map((f) => (
                    <li key={f.path} className={`entry tpl-file${sel === f.path ? " entry--sel" : ""}`} tabIndex={0}
                      onClick={() => sel !== f.path && guard(() => setSel(f.path))}
                      onKeyDown={(e) => { if ((e.key === "Enter" || e.key === " ") && sel !== f.path) { e.preventDefault(); guard(() => setSel(f.path)); } }}>
                      <span className="grow" style={{ wordBreak: "break-all" }}>{f.path.includes("/") ? f.path.slice(f.path.indexOf("/") + 1) : f.path}</span>
                      {f.modHint && <span className="mini-badge" title={`Looks like ${f.modHint}'s config`}>{f.modHint}</span>}
                      <span className="muted" style={{ fontSize: 11 }}>{fmtBytes(f.size)}</span>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        </div>
        <div style={{ minWidth: 0 }}>
          {file ? <FileEditor key={file.path} t={t} file={file} state={fileState} onChanged={onChanged} onGone={(n) => { setSel(null); onChanged(n); }} />
            : <div className="empty-state"><p>Pick a file to view or edit it.</p></div>}
        </div>
      </div>
      <div className="row">
        <Button variant="danger" onClick={() => setConfirmDelete(true)}>Delete template</Button>
        <Button onClick={() => guard(async () => { try { onChanged(await api.duplicateTemplate(t.id)); } catch (e) { setError(e as AppError); } })}>Duplicate</Button>
      </div>
      {confirmDelete && (
        <Modal title="Delete template?" width={460} onClose={() => setConfirmDelete(false)}>
          <div className="stack-lg">
            <p>Delete <strong>{t.name}</strong>?{uses.length ? ` ${uses.join(", ")} will lose its files in their exports (they'll show it as missing).` : ""} The folder moves to a <code>.trash</code> folder in the app's data directory, so you can still recover it by hand.</p>
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirmDelete(false)}>Cancel</Button>
              <Button variant="danger" onClick={async () => { unsaved.isDirty = () => false; await api.deleteTemplate(t.id); setConfirmDelete(false); onDeleted(); }}>Delete</Button>
            </div>
          </div>
        </Modal>
      )}
      {prompt}
    </div>
  );
}

// ---------------------------------------------------------------- screen
/** The template left open, kept while the app runs (switching tabs and back reopens it). */
let openTemplate: string | null = null;

export function Templates() {
  const [list, setList] = useState<Template[] | null>(null);
  const [uses, setUses] = useState<Record<string, string[]>>({});
  const [sel, setSelState] = useState<string | null>(openTemplate);
  const setSel = (id: string | null) => { openTemplate = id; setSelState(id); };
  const [error, setError] = useState<AppError | null>(null);
  const [importing, setImporting] = useState(false);
  const [creating, setCreating] = useState<string | null>(null);
  const { guard, prompt } = useUnsavedGuard();
  const refresh = async (select?: string) => {
    try {
      const [l, u] = await Promise.all([api.listTemplates(), api.templateUses()]);
      setList(l); setUses(u);
      if (select) setSel(select);
      else if (sel && !l.some((t) => t.id === sel)) setSel(null);
    } catch (e) { setError(e as AppError); }
  };
  useEffect(() => { refresh(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const current = list?.find((t) => t.id === sel) ?? null;
  const replace = (t: Template) => { setList((l) => { const has = l?.some((x) => x.id === t.id); return has ? l!.map((x) => (x.id === t.id ? t : x)) : [...(l ?? []), t]; }); if (t.id !== sel) setSel(t.id); };
  return (
    <div className="payloads-screen">
      <Panel title="Override templates" className="pane" actions={<>
        <Button size="small" onClick={() => guard(() => setImporting(true))}>Import…</Button>
        <Button size="small" variant="primary" onClick={() => guard(() => setCreating("New template"))}>New</Button>
      </>}>
        <div className="pane__body stack">
          <p className="muted" style={{ fontSize: 12, margin: 0 }}>Your own config/, kubejs/ and similar folders. Attach them to packs on a pack's Overrides tab; exports copy them into overrides/.</p>
          {error && <ErrorNotice error={error} />}
          {creating != null && (
            <div className="row" style={{ gap: 4 }}>
              <input className="mc-input grow" value={creating} autoFocus aria-label="Template name" onChange={(e) => setCreating(e.target.value)} onKeyDown={(e) => { if (e.key === "Escape") setCreating(null); }} />
              <Button size="small" variant="primary" disabled={!creating.trim()} onClick={async () => { try { const t = await api.createTemplate(creating); setCreating(null); refresh(t.id); } catch (e) { setError(e as AppError); } }}>Create</Button>
            </div>
          )}
          <ul className="entry-list">
            {(list ?? []).map((t) => (
              <li key={t.id} className={`entry${sel === t.id ? " entry--sel" : ""}`} tabIndex={0}
                onClick={() => t.id !== sel && guard(() => setSel(t.id))}
                onKeyDown={(e) => { if ((e.key === "Enter" || e.key === " ") && t.id !== sel) { e.preventDefault(); guard(() => setSel(t.id)); } }}>
                <div className="grow">
                  <div className="hit__name">{t.name}</div>
                  <div className="hit__meta">{describeTemplate(t)} · v{t.version}{uses[t.id]?.length ? ` · used by ${uses[t.id].length} pack${uses[t.id].length === 1 ? "" : "s"}` : ""}</div>
                </div>
              </li>
            ))}
          </ul>
          {list && list.length === 0 && <div className="empty-state"><p>No templates yet. Import your config folders from an instance or a pack, or start an empty one.</p></div>}
        </div>
      </Panel>
      <Panel title={current ? current.name : "Template"} className="pane"
        actions={current && <Button size="small" onClick={() => guard(() => setSel(null))} title="Close this template (back to the list)" aria-label="Close template">✕ Close</Button>}>
        <div className="pane__body">
          {current ? <TemplateEditor t={current} uses={uses[current.id] ?? []} onChanged={(t) => { replace(t); }} onDeleted={() => { setSel(null); refresh(); }} />
            : <div className="empty-state"><p>{list === null ? "Loading templates…" : "Pick a template on the left to edit it, or press New or Import."}</p></div>}
        </div>
      </Panel>
      {importing && <ImportTemplateDialog onClose={() => setImporting(false)} onDone={(t) => refresh(t.id)} />}
      {prompt}
    </div>
  );
}
