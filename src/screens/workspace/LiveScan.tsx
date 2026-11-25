import { useEffect, useState } from "react";
import { api, type AppError, type Fix, type ScanEstimate, type ScanReport } from "../../api";
import { Button, ErrorNotice, Modal, Notice } from "../../components/kit";
import type { Pack } from "../../model";
import { fixLabel } from "./PackList";

const SEV_ICON = { error: "✖", warning: "!", info: "i" } as const;
const mb = (b: number) => (b / 1024 / 1024).toFixed(b < 10 * 1024 * 1024 ? 1 : 0);

/**
 * Live Scan: read the mods' own jar metadata and predict what the loader will
 * refuse at startup. Opt-in, and the only place the app looks inside jars.
 */
export function LiveScanDialog({ pack, onClose, onFix, onSelect }: {
  pack: Pack; onClose: () => void; onFix: (f: Fix) => void; onSelect: (key: string) => void;
}) {
  const [est, setEst] = useState<ScanEstimate | null>(null);
  const [report, setReport] = useState<ScanReport | null>(null);
  const [busy, setBusy] = useState<null | "download" | "folder">(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [fixed, setFixed] = useState<Set<string>>(new Set());

  useEffect(() => { api.scanEstimate(pack).then(setEst, setError); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    // Progress comes as events from the core (not in the browser dev bridge).
    let off: (() => void) | undefined;
    import("@tauri-apps/api/event").then(({ listen }) => listen<{ done: number; total: number }>("scan-progress", (e) => setProgress(e.payload)).then((u) => { off = u; })).catch(() => {});
    return () => off?.();
  }, []);

  const run = async (how: "download" | "folder") => {
    setBusy(how); setError(null); setProgress(null); setFixed(new Set());
    try {
      const r = how === "download" ? await api.scanPack(pack) : await api.scanInstance(pack);
      if (r) setReport(r);
    } catch (e) { setError(e as AppError); }
    finally { setBusy(null); }
  };
  const problems = report?.issues.filter((i) => i.severity !== "info") ?? [];
  const notes = report?.issues.filter((i) => i.severity === "info") ?? [];

  return (
    <Modal title="Live Scan" width={720} onClose={() => !busy && onClose()}>
      <div className="stack-lg">
        <p className="muted" style={{ margin: 0 }}>
          Reads each mod's own metadata inside its jar (<code>mods.toml</code>, <code>fabric.mod.json</code>, bundled libraries) and predicts what the loader will refuse at startup:
          missing requirements, version-ranged incompatibilities, wrong loader versions and duplicate mods, including ones Modrinth and CurseForge don't list. Nothing is run.
        </p>
        {error && <ErrorNotice error={error} />}
        {!report && (
          <div className="scan-sources">
            <section className="stack">
              <div className="pixel">Download this pack's mods</div>
              {!est ? <p className="muted">Working out what's needed…</p> : (
                <p style={{ margin: 0 }}>
                  {est.files} jar{est.files === 1 ? "" : "s"}: {est.downloadBytes > 0 ? <>about <strong>{mb(est.downloadBytes)} MB</strong> to download</> : "all already downloaded"}
                  {est.cached > 0 && est.downloadBytes > 0 ? `, ${est.cached} already downloaded` : ""}.
                  <span className="muted"> From Modrinth's and CurseForge's servers only, each checked against its hash, kept in the app's scan cache (clear it in Settings).</span>
                  {est.unavailable.length > 0 && <span className="warn"> {est.unavailable.length} can't be downloaded and won't be scanned: {est.unavailable.join(", ")}.</span>}
                </p>
              )}
              <div><Button variant="primary" disabled={!est || !!busy || est.files === 0} onClick={() => run("download")}>{busy === "download" ? "Scanning…" : est && est.downloadBytes > 0 ? "Download and scan" : "Scan"}</Button></div>
            </section>
            <section className="stack">
              <div className="pixel">Or scan a Prism instance</div>
              <p className="muted" style={{ margin: 0 }}>Reads the jars in an instance's <code>mods</code> folder as they are (for example one you imported this pack into). Nothing is downloaded or changed.</p>
              <div><Button disabled={!!busy} onClick={() => run("folder")}>{busy === "folder" ? "Scanning…" : "Choose an instance…"}</Button></div>
            </section>
          </div>
        )}
        {busy === "download" && progress && (
          <div className="scan-progress" role="progressbar" aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.done}>
            <div style={{ width: `${(100 * progress.done) / Math.max(1, progress.total)}%` }} />
            <span>{progress.done} / {progress.total} jars</span>
          </div>
        )}
        {report && (
          <>
            <Notice tone={problems.some((i) => i.severity === "error") ? "error" : problems.length ? "warn" : "ok"}
              title={problems.length ? `${problems.length} problem${problems.length === 1 ? "" : "s"} in ${report.jars} jars` : `No problems found in ${report.jars} jars (${report.modIds} mods and libraries)`}>
              {report.notes.map((n, i) => <div key={i} className="muted" style={{ fontSize: 12 }}>{n}</div>)}
            </Notice>
            {problems.length > 0 && (
              <ul className="issue-list">
                {problems.map((i) => (
                  <li key={i.id} className={`issue issue--${i.severity}`}>
                    <span className="issue__icon" aria-label={i.severity}>{SEV_ICON[i.severity]}</span>
                    <button type="button" className="issue__msg" onClick={() => i.entryKey && onSelect(i.entryKey)}>{i.message}</button>
                    {i.fix && (fixed.has(i.id)
                      ? <span className="ok" style={{ fontSize: 12 }}>done</span>
                      : <Button size="small" variant={i.fix.kind === "remove" ? "danger" : "primary"} onClick={() => { onFix(i.fix!); setFixed(new Set(fixed).add(i.id)); }}>{fixLabel(i.fix)}</Button>)}
                  </li>
                ))}
              </ul>
            )}
            {notes.length > 0 && (
              <details className="export-list">
                <summary>{notes.length} note{notes.length === 1 ? "" : "s"}</summary>
                <ul>{notes.map((i) => <li key={i.id}>{i.message}</li>)}</ul>
              </details>
            )}
            {report.skipped.length > 0 && (
              <details className="export-list">
                <summary>{report.skipped.length} not scanned</summary>
                <ul>{report.skipped.map((s) => <li key={s}>{s}</li>)}</ul>
              </details>
            )}
            {fixed.size > 0 && <p className="muted" style={{ margin: 0, fontSize: 12 }}>Fixes change the pack (undoable). Scan again to check the result.</p>}
          </>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          {report && <Button disabled={!!busy} onClick={() => { setReport(null); api.scanEstimate(pack).then(setEst, setError); }}>Scan again</Button>}
          <Button onClick={onClose} disabled={!!busy}>Close</Button>
        </div>
      </div>
    </Modal>
  );
}
