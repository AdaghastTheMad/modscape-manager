import { useEffect, useState } from "react";
import { playClick } from "../sound";
import { api, type AppError, type KeyStatus, type Loader, type ResetArea, type SearchPage, type Source } from "../api";
import { Button, Input, Modal, Notice, Panel, Select, Slot, SourceBadge, Tooltip } from "../components/kit";
import type { CacheStats } from "../api";
import { resetPrefs, setPref, usePrefs } from "../prefs";

const fmt = new Intl.NumberFormat(undefined, { notation: "compact" });

function ErrorNotice({ error }: { error: AppError }) {
  return (
    <Notice tone="error" title={error.message}>
      {error.help && <pre>{error.help}</pre>}
    </Notice>
  );
}

function KeySection({ status, setStatus }: { status: KeyStatus | null; setStatus: (s: KeyStatus) => void }) {
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [saved, setSaved] = useState(false);

  const run = async (fn: () => Promise<KeyStatus>) => {
    setBusy(true); setError(null); setSaved(false);
    try { setStatus(await fn()); return true; } catch (e) { setError(e as AppError); return false; } finally { setBusy(false); }
  };

  const unavailable = status && !status.keychainAvailable;
  return (
    <Panel title="CurseForge API key">
      <div className="stack-lg">
        <p className="muted">
          CurseForge needs your own API key (get one at <code>console.curseforge.com</code> → API Keys). It's kept in
          your {status?.backend ?? "system keychain"}, used only by the app's core, and never shown again here.
          Without a key the app runs in Modrinth-only mode.
        </p>
        {status === null ? <p className="muted">Checking keychain…</p> : unavailable ? (
          <Notice tone="error" title={status.problem?.message ?? "Keychain unavailable"}>
            <pre>{status.problem?.help}</pre>
          </Notice>
        ) : (
          <p>Status: {status.hasKey ? <span className="ok">● Key saved</span> : <span className="warn">● No key: Modrinth-only mode</span>}</p>
        )}
        <form className="row" onSubmit={async (e) => {
          e.preventDefault();
          if (await run(() => api.saveCurseforgeKey(draft))) { setDraft(""); setSaved(true); }
        }}>
          <div className="grow">
            <Input label={status?.hasKey ? "Replace key" : "Paste key"} type="password" autoComplete="off" spellCheck={false}
              value={draft} onChange={(e) => setDraft(e.target.value)} disabled={busy || !!unavailable} placeholder="$2a$10$…" />
          </div>
          <div className="row" style={{ alignSelf: "flex-end" }}>
            <Button type="submit" variant="primary" disabled={busy || !draft.trim() || !!unavailable}>Save key</Button>
            <Button variant="danger" disabled={busy || !status?.hasKey} onClick={() => run(api.clearCurseforgeKey)}>Remove</Button>
            <Button disabled={busy} onClick={() => run(api.keyStatus)}>Recheck</Button>
          </div>
        </form>
        {saved && <Notice tone="ok" title="Key saved to the keychain.">Run a test search below to confirm CurseForge accepts it.</Notice>}
        {error && <ErrorNotice error={error} />}
      </div>
    </Panel>
  );
}

function ConnectionTest({ hasKey }: { hasKey: boolean }) {
  const [source, setSource] = useState<Source>("curseforge");
  const [text, setText] = useState("jei");
  const [version, setVersion] = useState("1.21.1");
  const [loader, setLoader] = useState<Loader | "">("neoforge");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SearchPage | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [ms, setMs] = useState(0);

  const go = async () => {
    setBusy(true); setError(null); setResult(null);
    const t0 = performance.now();
    try {
      setResult(await api.search(source, { text, gameVersion: version || undefined, loader: loader || undefined, limit: 10 }));
    } catch (e) { setError(e as AppError); }
    finally { setMs(Math.round(performance.now() - t0)); setBusy(false); }
  };

  return (
    <Panel title="Connection test">
      <div className="stack-lg">
        <p className="muted">Runs one search through the Rust core. Results aren't cached or saved yet.</p>
        <form className="row" style={{ alignItems: "flex-end" }} onSubmit={(e) => { e.preventDefault(); go(); }}>
          <Select label="Source" value={source} onChange={(e) => setSource(e.target.value as Source)}>
            <option value="curseforge">CurseForge</option>
            <option value="modrinth">Modrinth</option>
          </Select>
          <div className="grow"><Input label="Search" value={text} onChange={(e) => setText(e.target.value)} /></div>
          <div style={{ width: 110 }}><Input label="MC version" value={version} onChange={(e) => setVersion(e.target.value)} placeholder="any" /></div>
          <Select label="Loader" value={loader} onChange={(e) => setLoader(e.target.value as Loader | "")}>
            <option value="">any</option><option value="neoforge">NeoForge</option><option value="fabric">Fabric</option>
            <option value="forge">Forge</option><option value="quilt">Quilt</option>
          </Select>
          <Button type="submit" variant="primary" disabled={busy}>{busy ? "Searching…" : "Test search"}</Button>
        </form>
        {source === "curseforge" && !hasKey && <Notice title="No CurseForge key saved">Save one above first; this search will fail without it.</Notice>}
        {error && <ErrorNotice error={error} />}
        {result && (
          <>
            <Notice tone="ok" title={`${result.source === "curseforge" ? "CurseForge" : "Modrinth"} answered in ${ms} ms`}>
              {fmt.format(result.total)} total matches; showing {result.hits.length}.
            </Notice>
            <ul className="result-list">
              {result.hits.map((h) => (
                <li key={h.id} className="result">
                  <Tooltip name={h.name} badge={<SourceBadge source={h.source} />} lines={[
                    h.summary,
                    `${fmt.format(h.downloads)} downloads`,
                    h.updated ? `Updated ${new Date(h.updated).toLocaleDateString()}` : "Update date unknown",
                  ]}>
                    {/* Remote icons arrive in M2 via the core's cache; the webview has no network access. */}
                    <span tabIndex={0}><Slot label={h.name} /></span>
                  </Tooltip>
                  <div className="grow">
                    <div className="row"><span className="result__name">{h.name}</span><SourceBadge source={h.source} /></div>
                    <div className="muted">{h.summary}</div>
                    <div className="muted" style={{ fontSize: 12 }}>
                      {h.authors.join(", ") || "unknown author"} · {fmt.format(h.downloads)} downloads
                      {h.categories.length > 0 && ` · ${h.categories.join(", ")}`}
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
    </Panel>
  );
}

function InterfaceSection() {
  const prefs = usePrefs();
  return (
    <Panel title="Interface">
      <label className="mc-check">
        <input type="checkbox" checked={prefs.syncGrouping} onChange={(e) => setPref("syncGrouping", e.target.checked)} />
        Keep the list's grouping and the graph's regions in sync
      </label>
      <p className="muted" style={{ margin: "6px 0 0", fontSize: 12 }}>Off: each view keeps its own grouping. On: changing one changes both.</p>
      <label className="mc-check" style={{ marginTop: 10 }}>
        <input type="checkbox" checked={prefs.clickSound} onChange={(e) => { setPref("clickSound", e.target.checked); if (e.target.checked) playClick(); }} />
        Click sound on buttons
      </label>
      <label className="mc-check" style={{ marginTop: 6, marginLeft: 24, opacity: prefs.clickSound ? 1 : 0.6 }}>
        <input type="checkbox" disabled={!prefs.clickSound} checked={prefs.clickSoundLists} onChange={(e) => setPref("clickSoundLists", e.target.checked)} />
        Also when picking items in lists (mods, payloads, templates)
      </label>
      <label className="mc-check" style={{ marginTop: 6 }}>
        <input type="checkbox" checked={prefs.backdrop} onChange={(e) => setPref("backdrop", e.target.checked)} />
        Dark tiled backdrop behind the panels
      </label>
      <p className="muted" style={{ margin: "6px 0 0", fontSize: 12 }}>Press <kbd>?</kbd> anywhere for the keyboard shortcuts.</p>
      <label className="mc-check" style={{ marginTop: 10 }}>
        <input type="checkbox" checked={prefs.showKit} onChange={(e) => setPref("showKit", e.target.checked)} />
        Developer: show the Component kit tab
      </label>
    </Panel>
  );
}

function CacheSection() {
  const [stats, setStats] = useState<CacheStats | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.cacheStats().then(setStats, () => {}); }, []);
  const mb = stats ? (stats.bytes / 1024 / 1024).toFixed(1) : "?";
  return (
    <Panel title="Cache">
      <div className="row" style={{ justifyContent: "space-between" }}>
        <p className="muted" style={{ margin: 0 }}>
          API answers are kept for 24 hours (icons for 30 days) so reopening packs is instant and rate limits never bite.
          {stats && <> Currently {stats.entries} item{stats.entries === 1 ? "" : "s"}, {mb} MB.</>}
        </p>
        <Button disabled={busy} onClick={async () => { setBusy(true); try { setStats(await api.clearCache()); } finally { setBusy(false); } }}>Clear cache</Button>
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------- danger zone
const AREAS: { id: ResetArea | "prefs"; label: string; what: string }[] = [
  { id: "packs", label: "Packs", what: "every pack, with its snapshots, notes and tags" },
  { id: "payloads", label: "Payloads", what: "your payloads and edits; the starter payloads come back as shipped" },
  { id: "templates", label: "Override templates", what: "every template and its files, the trash folder included" },
  { id: "links", label: "Remembered site links", what: "the Modrinth ↔ CurseForge links you confirmed" },
  { id: "cache", label: "Cache", what: "saved API answers and icons (they're fetched again as needed)" },
  { id: "curseforgeKey", label: "CurseForge key", what: "the key in your system keychain (you can add it again)" },
  { id: "prefs", label: "Interface settings", what: "the toggles on this page, back to their defaults" },
];

function DangerZone() {
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<"some" | "all" | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [done, setDone] = useState<string[] | null>(null);
  const chosen = confirm === "all" ? AREAS : AREAS.filter((a) => picked.has(a.id));
  const run = async () => {
    setBusy(true); setError(null);
    try {
      const areas = chosen.map((a) => a.id).filter((id): id is ResetArea => id !== "prefs");
      const lines = areas.length ? await api.resetData(areas) : [];
      if (chosen.some((a) => a.id === "prefs")) { resetPrefs(); lines.push("Interface settings are back to their defaults."); }
      setDone(lines); setConfirm(null); setPicked(new Set()); setTyped("");
    } catch (e) { setError(e as AppError); }
    finally { setBusy(false); }
  };
  return (
    <details className="danger-zone">
      <summary className="pixel">Danger zone</summary>
      <div className="stack" style={{ marginTop: 8 }}>
        <p className="muted" style={{ margin: 0 }}>Put parts of the app back to how a fresh install has them. These can't be undone, so export anything you want to keep first (packs as planner files, payloads as JSON; template folders are plain files).</p>
        {AREAS.map((a) => (
          <label key={a.id} className="mc-check">
            <input type="checkbox" checked={picked.has(a.id)} onChange={(e) => { const n = new Set(picked); if (e.target.checked) n.add(a.id); else n.delete(a.id); setPicked(n); }} />
            <span><strong>{a.label}</strong> <span className="muted">· {a.what}</span></span>
          </label>
        ))}
        <div className="row" style={{ justifyContent: "space-between" }}>
          <Button variant="danger" disabled={picked.size === 0} onClick={() => setConfirm("some")}>Reset selected ({picked.size})…</Button>
          <Button variant="danger" onClick={() => { setTyped(""); setConfirm("all"); }}>Factory reset…</Button>
        </div>
        {done && (
          <Notice tone="ok" title="Done">
            {done.map((l, i) => <div key={i}>{l}</div>)}
            <div style={{ marginTop: 6 }}><Button size="small" onClick={() => location.reload()}>Reload the app</Button></div>
          </Notice>
        )}
      </div>
      {confirm && (
        <Modal title={confirm === "all" ? "Factory reset?" : "Reset these?"} width={520} onClose={() => !busy && setConfirm(null)}>
          <div className="stack-lg">
            <p style={{ margin: 0 }}>{confirm === "all" ? "Everything goes back to a fresh install:" : "This permanently resets:"}</p>
            <ul className="dep-list">{chosen.map((a) => <li key={a.id}><strong>{a.label}</strong>: {a.what}</li>)}</ul>
            {confirm === "all" && <Input label={'Type RESET to confirm'} value={typed} onChange={(e) => setTyped(e.target.value)} autoFocus />}
            {error && <ErrorNotice error={error} />}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={() => setConfirm(null)} disabled={busy}>Cancel</Button>
              <Button variant="danger" disabled={busy || (confirm === "all" && typed.trim() !== "RESET")} onClick={run}>{busy ? "Resetting…" : confirm === "all" ? "Factory reset" : "Reset"}</Button>
            </div>
          </div>
        </Modal>
      )}
    </details>
  );
}

export function Settings() {
  const [status, setStatus] = useState<KeyStatus | null>(null);
  const [coreError, setCoreError] = useState<AppError | null>(null);
  const [ua, setUa] = useState("");
  useEffect(() => {
    api.keyStatus().then(setStatus, setCoreError);
    api.appInfo().then((i) => setUa(`${i.userAgent} on ${i.os}`), () => {});
  }, []);
  return (
    <div className="settings stack-lg">
      {coreError && <ErrorNotice error={coreError} />}
      <KeySection status={status} setStatus={setStatus} />
      <ConnectionTest hasKey={!!status?.hasKey} />
      <InterfaceSection />
      <CacheSection />
      <DangerZone />
      <Panel title="Privacy" inset>
        <p className="muted">No telemetry, analytics or accounts. The only outside calls are to Modrinth, CurseForge and loader version
          metadata, all made by the app's core{ua && <> with the User-Agent <code>{ua}</code></>}.</p>
      </Panel>
    </div>
  );
}
