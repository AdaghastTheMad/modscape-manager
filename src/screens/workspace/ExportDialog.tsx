import { useEffect, useState } from "react";
import { api, type AppError, type ExportFormat, type ExportPlan, type Fix } from "../../api";
import { Button, ErrorNotice, Modal, Notice, Tabs } from "../../components/kit";
import { fmtCompact } from "../../hooks";
import type { Pack } from "../../model";

type Tab = ExportFormat | "project";

const FORMAT_INFO: Record<ExportFormat, string> = {
  mrpack: "Modrinth's format. Imports into Prism, the Modrinth App and ATLauncher. Holds Modrinth-sourced files.",
  curseforgeZip: "CurseForge's format. Imports into the CurseForge app and Prism. Holds CurseForge-sourced files (no datapacks).",
  packwiz: "A packwiz folder: one small .pw.toml per file, each from the site it's set to, so mixed packs work. For packwiz-installer and git-based workflows. You pick a folder; a new \"<pack> packwiz\" folder is made inside it.",
  markdown: "A readable list (a table per content type) with names, versions, sites and links, for a pack page or sharing.",
  csv: "A spreadsheet-friendly list: name, type, version, site, link, category, and why it's in the pack.",
};
const DONE_HINT: Partial<Record<ExportFormat, string>> = {
  mrpack: "In Prism: Add Instance → Import → pick this file.",
  curseforgeZip: "In Prism: Add Instance → Import → pick this file.",
  packwiz: "Serve or commit this folder; packwiz-installer reads pack.toml.",
};

export function ExportDialog({ pack, onClose, onFixes, onSaveProject }: {
  pack: Pack; onClose: () => void; onFixes: (fixes: Fix[]) => void; onSaveProject: () => Promise<void>;
}) {
  const [tab, setTab] = useState<Tab>(pack.sourcePreference === "curseforge" ? "curseforgeZip" : "mrpack");
  const [plan, setPlan] = useState<ExportPlan | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [showFiles, setShowFiles] = useState(false);

  // Re-plan whenever the format or the pack changes (fixes update the pack).
  const sig = JSON.stringify([tab, pack.loaderVersion, pack.channelPolicy, Object.entries(pack.entries).map(([k, e]) => [k, e.ref, e.fileId])]);
  useEffect(() => {
    if (tab === "project") return;
    let live = true;
    setPlan(null); setError(null); setDone(null);
    api.exportPlan(pack, tab).then((p) => live && setPlan(p), (e) => live && setError(e));
    return () => { live = false; };
  }, [sig]); // eslint-disable-line react-hooks/exhaustive-deps

  const doExport = async () => {
    if (tab === "project") return;
    setBusy(true); setError(null);
    try { const path = await api.exportPack(pack, tab); if (path) setDone(path); }
    catch (e) { setError(e as AppError); }
    finally { setBusy(false); }
  };

  const total = plan?.files.reduce((n, f) => n + f.size, 0) ?? 0;
  return (
    <Modal title="Export pack" width={720} onClose={onClose}>
      <div className="stack-lg">
        <Tabs label="Format" underline value={tab} onChange={setTab} tabs={[
          { id: "mrpack", label: ".mrpack" }, { id: "curseforgeZip", label: "CurseForge zip" }, { id: "packwiz", label: "packwiz" },
          { id: "markdown", label: "Markdown list" }, { id: "csv", label: "CSV list" }, { id: "project", label: "Planner project" },
        ]} />
        {tab === "project" ? (
          <div className="stack">
            <p className="muted">The full planner file: every link, pin, note, tag and setting. Open it again with "Open project…".</p>
            <div className="row"><Button variant="primary" onClick={onSaveProject}>Save project file…</Button></div>
          </div>
        ) : (
          <div className="stack">
            <p className="muted">{FORMAT_INFO[tab]}{tab === "markdown" || tab === "csv" ? "" : " The launcher downloads the files when it installs the pack; nothing is downloaded here."}</p>
            {error && <ErrorNotice error={error} />}
            {!plan && !error && <p className="muted">Checking every mod's file…</p>}
            {plan && (
              <>
                <Notice tone={plan.canExport ? (plan.problems.length ? "warn" : "ok") : "error"}
                  title={plan.canExport ? `Ready: ${plan.files.length} file${plan.files.length === 1 ? "" : "s"}${tab === "markdown" || tab === "csv" ? "" : ` · ${plan.loaderLabel}`}` : "Can't export yet"}>
                  {plan.files.length > 0 && tab !== "markdown" && tab !== "csv" && <div className="muted">Launchers will download about {fmtCompact.format(total / 1024 / 1024)} MB.</div>}
                </Notice>
                {plan.problems.length > 0 && (
                  <ul className="issue-list">
                    {plan.problems.map((p, i) => (
                      <li key={i} className={`issue issue--${p.severity}`}>
                        <span className="issue__icon">{p.severity === "error" ? "✖" : p.severity === "warning" ? "!" : "i"}</span>
                        <span className="grow">{p.message}</span>
                        {p.fixes.length > 0 && <Button size="small" variant="primary" onClick={() => onFixes(p.fixes)}>{p.fixLabel ?? "Fix"}</Button>}
                      </li>
                    ))}
                  </ul>
                )}
                {plan.excluded.length > 0 && (
                  <details className="export-list">
                    <summary>{plan.excluded.length} not included</summary>
                    <ul>{plan.excluded.map((x) => <li key={x.key}><strong>{x.name}</strong> <span className="muted">· {x.reason}</span></li>)}</ul>
                  </details>
                )}
                <details className="export-list" open={showFiles} onToggle={(e) => setShowFiles((e.target as HTMLDetailsElement).open)}>
                  <summary>{plan.files.length} included</summary>
                  <ul>{plan.files.map((f) => <li key={f.key}>{f.name} <span className="muted">· {f.versionNumber} · {f.path.slice(0, f.path.lastIndexOf("/") + 1)}{f.source === "curseforge" && tab === "packwiz" ? " · CurseForge" : ""}{f.distributionBlocked ? " · manual download" : ""}</span></li>)}</ul>
                </details>
              </>
            )}
            {done && <Notice tone="ok" title="Exported">{done}{DONE_HINT[tab as ExportFormat] && <div className="muted">{DONE_HINT[tab as ExportFormat]}</div>}</Notice>}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={onClose}>Close</Button>
              <Button variant="primary" disabled={!plan?.canExport || busy} onClick={doExport}>{busy ? "Exporting…" : "Export…"}</Button>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
}
