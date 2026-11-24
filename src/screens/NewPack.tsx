import { useEffect, useMemo, useState } from "react";
import { api, type AppError, type GameVersion, type Loader, type LoaderVersions } from "../api";
import { Button, ErrorNotice, Input, Modal, Notice, Slot } from "../components/kit";
import { LOADERS, newPack } from "../model";

const FEATURED = ["1.21.1", "26.1.2"];

export function VersionPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [all, setAll] = useState<GameVersion[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [filter, setFilter] = useState("");
  const [unstable, setUnstable] = useState(false);
  useEffect(() => { api.gameVersions().then(setAll, setError); }, []);

  const shown = useMemo(() => (all ?? [])
    .filter((v) => unstable || v.version_type === "release")
    .filter((v) => !filter || v.version.toLowerCase().includes(filter.toLowerCase().trim())), [all, filter, unstable]);

  return (
    <div className="stack">
      <div className="row">
        <span className="mc-field__label">Quick pick:</span>
        {FEATURED.map((v) => <Button key={v} size="small" className="mc-choice" aria-pressed={value === v} onClick={() => onChange(v)}>{v}</Button>)}
      </div>
      {error && <ErrorNotice error={error} />}
      <div className="row">
        <div className="grow"><Input placeholder="Filter versions, e.g. 1.20" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filter versions" /></div>
        <label className="mc-check"><input type="checkbox" checked={unstable} onChange={(e) => setUnstable(e.target.checked)} /> Snapshots & pre-releases</label>
      </div>
      <div className="version-grid mc-panel mc-panel--inset" role="listbox" aria-label="Minecraft versions">
        {all === null && !error && <span className="muted">Loading versions…</span>}
        {shown.map((v) => (
          <button key={v.version} type="button" role="option" aria-selected={value === v.version}
            className="version-chip" onClick={() => onChange(v.version)} title={new Date(v.date).toLocaleDateString()}>
            {v.version}
          </button>
        ))}
        {all !== null && shown.length === 0 && <span className="muted">No versions match.</span>}
      </div>
      <p className="muted" style={{ fontSize: 12 }}>Every version Modrinth lists is available here. Selected: <strong>{value || "none"}</strong></p>
    </div>
  );
}

export function LoaderVersionPicker({ loader, mc, value, onChange }: { loader: Loader; mc: string; value: string; onChange: (v: string) => void }) {
  const [data, setData] = useState<LoaderVersions | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [unstable, setUnstable] = useState(false);
  useEffect(() => {
    setData(null); setError(null);
    api.loaderVersions(loader, mc).then((d) => {
      setData(d);
      const best = d.versions.find((v) => v.stable) ?? d.versions[0];
      onChange(best?.version ?? "");
    }, setError);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loader, mc]);
  const list = (data?.versions ?? []).filter((v) => unstable || v.stable || v.version === value);
  return (
    <div className="stack">
      {error && <ErrorNotice error={error} />}
      {data?.note && <Notice>{data.note}</Notice>}
      <div className="row" style={{ alignItems: "flex-end" }}>
        <div className="mc-field">
          <label className="mc-field__label" htmlFor="lv-select">Loader version</label>
          <select id="lv-select" className="mc-select" value={list.some((v) => v.version === value) ? value : ""} onChange={(e) => onChange(e.target.value)} disabled={!data || list.length === 0}>
            {!data && <option value="">Loading…</option>}
            {data && list.length === 0 && <option value="">none listed</option>}
            {list.map((v, i) => <option key={v.version} value={v.version}>{v.version}{i === 0 && v.stable ? " (latest)" : ""}{v.stable ? "" : " (unstable)"}</option>)}
          </select>
        </div>
        <div className="grow"><Input label="…or type one" value={value} onChange={(e) => onChange(e.target.value)} placeholder="e.g. 21.1.267" /></div>
        <label className="mc-check" style={{ paddingBottom: 6 }}><input type="checkbox" checked={unstable} onChange={(e) => setUnstable(e.target.checked)} /> Show unstable</label>
      </div>
    </div>
  );
}

export function NewPackWizard({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const [step, setStep] = useState(0);
  const [name, setName] = useState("");
  const [icon, setIcon] = useState<string | null>(null);
  const [mc, setMc] = useState("1.21.1");
  const [loader, setLoader] = useState<Loader>("neoforge");
  const [loaderVersion, setLoaderVersion] = useState("");
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);

  const steps = ["Name", "Minecraft version", "Loader"];
  const canNext = [name.trim().length > 0, mc.length > 0, true][step];

  const create = async () => {
    setBusy(true); setError(null);
    try {
      const saved = await api.savePack(newPack({ name, icon, mcVersion: mc, loader, loaderVersion: loaderVersion.trim() }));
      onCreated(saved.pack.id);
    } catch (e) { setError(e as AppError); setBusy(false); }
  };

  return (
    <Modal title={`New pack · ${step + 1}/3 ${steps[step]}`} onClose={onClose} width={720}>
      <form className="stack-lg" onSubmit={(e) => { e.preventDefault(); if (step < 2) { if (canNext) setStep(step + 1); } else create(); }}>
        {step === 0 && (
          <div className="row" style={{ alignItems: "flex-end" }}>
            <div className="stack" style={{ alignItems: "center" }}>
              <Slot src={icon} label={name || "Pack"} size={64} />
              <div className="row">
                <Button size="small" onClick={async () => { try { const i = await api.pickIcon(); if (i) setIcon(i); } catch (e) { setError(e as AppError); } }}>Icon…</Button>
                {icon && <Button size="small" onClick={() => setIcon(null)} aria-label="Remove icon">✕</Button>}
              </div>
            </div>
            <div className="grow"><Input label="Pack name" autoFocus value={name} maxLength={120} onChange={(e) => setName(e.target.value)} placeholder="My survival pack" hint="Icon is optional (PNG, JPEG, WebP or GIF up to 512 KB)." /></div>
          </div>
        )}
        {step === 1 && <VersionPicker value={mc} onChange={setMc} />}
        {step === 2 && (
          <div className="stack-lg">
            <div className="row" role="group" aria-label="Loader">
              {LOADERS.map((l) => <Button key={l.id} className="mc-choice" aria-pressed={loader === l.id} onClick={() => setLoader(l.id)}>{l.label}</Button>)}
            </div>
            <LoaderVersionPicker loader={loader} mc={mc} value={loaderVersion} onChange={setLoaderVersion} />
            <p className="muted">One loader per pack. Minecraft {mc}, {LOADERS.find((l) => l.id === loader)?.label} {loaderVersion || "(no loader version yet; you can set it later)"}.</p>
          </div>
        )}
        {error && <ErrorNotice error={error} />}
        <div className="row" style={{ justifyContent: "space-between" }}>
          <Button onClick={() => (step === 0 ? onClose() : setStep(step - 1))}>{step === 0 ? "Cancel" : "Back"}</Button>
          {step < 2
            ? <Button type="submit" variant="primary" disabled={!canNext}>Next</Button>
            : <Button type="submit" variant="primary" disabled={busy}>Create pack</Button>}
        </div>
      </form>
    </Modal>
  );
}
