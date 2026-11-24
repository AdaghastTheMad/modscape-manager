import { useState } from "react";
import { api, type AppError, type Loader, type PortPlan } from "../../api";
import { Button, ErrorNotice, Input, Modal, Notice } from "../../components/kit";
import { LOADERS, loaderLabel, type Pack } from "../../model";
import { LoaderVersionPicker, VersionPicker } from "../NewPack";

function Section({ title, items, tone, open }: { title: string; items: React.ReactNode[]; tone?: "ok" | "warn" | "err"; open?: boolean }) {
  if (!items.length) return null;
  return (
    <details className="export-list" open={open}>
      <summary className={tone}>{title} ({items.length})</summary>
      <ul>{items.map((x, i) => <li key={i}>{x}</li>)}</ul>
    </details>
  );
}

/** Copy a pack to another Minecraft version / loader, with a report first. */
export function DuplicateDialog({ pack, onClose, onCreated }: { pack: Pack; onClose: () => void; onCreated: (id: string) => void }) {
  const suggested = pack.mcVersion === "1.21.1" ? "26.1.2" : pack.mcVersion === "26.1.2" ? "1.21.1" : "";
  const [mc, setMc] = useState(suggested);
  const [loader, setLoader] = useState<Loader>(pack.loader);
  const [loaderVersion, setLoaderVersion] = useState("");
  const [name, setName] = useState("");
  const [keepMissing, setKeepMissing] = useState(false);
  // Dependencies to carry over as your own mods (ticked in the report).
  const [keep, setKeep] = useState<Set<string>>(new Set());
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [plan, setPlan] = useState<PortPlan | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);
  const same = mc === pack.mcVersion && loader === pack.loader;

  const check = async (keepKeys: Set<string> = keep) => {
    setBusy(true); setError(null);
    try {
      setPlan(await api.planPort(pack, { mcVersion: mc, loader, loaderVersion, name: name.trim() || undefined, keepMissing, keep: [...keepKeys] }));
      setKeep(keepKeys); setTicked(new Set());
    }
    catch (e) { setError(e as AppError); }
    finally { setBusy(false); }
  };
  const create = async () => {
    if (!plan) return;
    setBusy(true);
    try { const saved = await api.savePack(plan.pack); onCreated(saved.pack.id); }
    catch (e) { setError(e as AppError); setBusy(false); }
  };
  const r = plan?.report;

  return (
    <Modal title={`Duplicate "${pack.name}" to another version`} width={760} onClose={onClose}>
      <div className="stack-lg">
        {!plan ? (
          <>
            <p className="muted">Makes a new pack; this one stays as it is. Your mods carry over with their tags and notes, dependencies are worked out fresh for the target, and you get a report before anything is saved.</p>
            <VersionPicker value={mc} onChange={(v) => { setMc(v); setPlan(null); }} />
            <div className="row" role="group" aria-label="Loader">
              {LOADERS.map((l) => <Button key={l.id} className="mc-choice" aria-pressed={loader === l.id} onClick={() => setLoader(l.id)}>{l.label}</Button>)}
            </div>
            {mc && <LoaderVersionPicker loader={loader} mc={mc} value={loaderVersion} onChange={setLoaderVersion} />}
            <div className="row" style={{ alignItems: "flex-end" }}>
              <div className="grow"><Input label="New pack name" value={name} onChange={(e) => setName(e.target.value)} placeholder={`${pack.name} (${mc || "…"})`} /></div>
              <label className="mc-check" style={{ paddingBottom: 6 }}>
                <input type="checkbox" checked={keepMissing} onChange={(e) => setKeepMissing(e.target.checked)} /> Keep mods with no build (flagged)
              </label>
            </div>
            {same && <Notice title="That's the pack's current version and loader." />}
            {error && <ErrorNotice error={error} />}
            <div className="row" style={{ justifyContent: "flex-end" }}>
              <Button onClick={onClose}>Cancel</Button>
              <Button variant="primary" disabled={!mc || same || busy} onClick={() => check(new Set())}>{busy ? "Checking every mod…" : "Check port"}</Button>
            </div>
          </>
        ) : (
          <>
            <Notice tone={r!.missing.some((m) => !m.otherSite) ? "warn" : "ok"}
              title={`${r!.ported.length} of ${r!.ported.length + r!.missing.length} of your mods have a build for ${mc} · ${loaderLabel(loader)}`}>
              <div className="muted">New pack: {plan.pack.name} · {Object.keys(plan.pack.entries).length} mods{plan.pack.loaderVersion ? ` · ${loaderLabel(loader)} ${plan.pack.loaderVersion}` : ""}</div>
              {r!.notes.map((n, i) => <div key={i}>{n}</div>)}
            </Notice>
            <Section title="No build on the target" tone="err" open items={r!.missing.map((m) => (
              <><strong>{m.name}</strong>{m.otherSite ? <span className="ok"> · switched to its {m.otherSite === "modrinth" ? "Modrinth" : "CurseForge"} copy, which has one</span>
                : <span className="muted">{keepMissing ? " · kept, flagged" : " · left out"}{m.neededBy.length ? ` · needed by ${m.neededBy.join(", ")}` : ""}</span>}</>
            ))} />
            <Section title="Dependencies changed" tone="warn" open items={r!.changedDeps.map((c) => (
              <><strong>{c.name}</strong>{c.added.length > 0 && <span className="ok"> + {c.added.join(", ")}</span>}{c.removed.length > 0 && <span className="err"> − {c.removed.join(", ")}</span>}</>
            ))} />
            <Section title="New dependencies" items={r!.newDeps} />
            <Section title="Kept on purpose" tone="ok" items={r!.kept} />
            {r!.droppedDeps.length > 0 && (
              <details className="export-list" open>
                <summary>Dependencies no longer needed ({r!.droppedDeps.length})</summary>
                <p className="muted" style={{ margin: "6px 0" }}>
                  Nothing on {mc} needs these, usually because the mod that pulled them in has no build there. Tick any you'd still like (they become your own mods) and re-check.
                </p>
                <ul className="dropped-list">
                  {r!.droppedDeps.map((d) => (
                    <li key={d.key}>
                      <label className="mc-check">
                        <input type="checkbox" checked={ticked.has(d.key)} onChange={(e) => {
                          const n = new Set(ticked); if (e.target.checked) n.add(d.key); else n.delete(d.key); setTicked(n);
                        }} />
                        <strong className={d.library ? "muted" : undefined}>{d.name}</strong>
                      </label>
                      <span className={d.hasBuild ? "ok" : "err"}>{d.hasBuild ? `has a ${mc} build` : `no ${mc} build`}</span>
                      {d.library && <span className="muted">library</span>}
                      {d.wasNeededBy.length > 0 && <span className="muted">· was for {d.wasNeededBy.join(", ")}</span>}
                    </li>
                  ))}
                </ul>
                <div className="row" style={{ marginTop: 6 }}>
                  <Button size="small" variant="primary" disabled={!ticked.size || busy} onClick={() => check(new Set([...keep, ...ticked]))}>
                    {busy ? "Re-checking…" : `Keep ${ticked.size || ""} and re-check`.replace("  ", " ")}
                  </Button>
                  <Button size="small" disabled={busy} onClick={() => setTicked(new Set(r!.droppedDeps.filter((d) => d.hasBuild && !d.library).map((d) => d.key)))}>
                    Tick mods with builds
                  </Button>
                </div>
              </details>
            )}
            <Section title="Carried over" tone="ok" items={r!.ported} />
            {r!.pinsDropped > 0 && <p className="muted" style={{ margin: 0 }}>{r!.pinsDropped} pinned version{r!.pinsDropped === 1 ? "" : "s"} reset to "newest compatible" (pins don't carry across versions).</p>}
            {error && <ErrorNotice error={error} />}
            <div className="row" style={{ justifyContent: "space-between" }}>
              <Button onClick={() => { setPlan(null); setKeep(new Set()); }}>Back</Button>
              <div className="row">
                <Button onClick={onClose}>Cancel</Button>
                <Button variant="primary" disabled={busy} onClick={create}>Create pack</Button>
              </div>
            </div>
          </>
        )}
      </div>
    </Modal>
  );
}
