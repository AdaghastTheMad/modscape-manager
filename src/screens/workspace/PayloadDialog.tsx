import { useEffect, useState } from "react";
import { api, type AppError, type PayloadPlan, type PlanRow } from "../../api";
import { Button, ErrorNotice, ModIcon, Modal, Notice } from "../../components/kit";
import { kindLabel, type Pack } from "../../model";

export interface PayloadApply {
  plan: PayloadPlan;
  /** Rows (by index) to add. */
  add: Extract<PlanRow, { status: "add" }>[];
  /** Entries already in the pack that this payload now also covers. */
  covers: string[];
  choices: Record<string, string>;
  /** Mods removed from the payload that you chose to remove from the pack too. */
  remove: string[];
  /** Mods removed from the payload that stay in the pack (no longer tagged with it). */
  untag: string[];
}

const rowKey = (r: PlanRow, i: number) => `${r.slotId}:${i}`;

/** Drop-in preview: what a payload would add, what's already there, what has no build. */
export function PayloadDialog({ pack, payloadId, onClose, onApply, title }: {
  pack: Pack; payloadId: string; onClose: () => void; onApply: (a: PayloadApply) => Promise<void>; title?: string;
}) {
  const [choices, setChoices] = useState<Record<string, string>>({});
  const [plan, setPlan] = useState<PayloadPlan | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [unticked, setUnticked] = useState<Set<string>>(new Set());
  const [toRemove, setToRemove] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    setPlan(null); setError(null);
    api.planPayload(pack, payloadId, choices).then((p) => {
      if (!live) return;
      setPlan(p);
      setUnticked(new Set()); // everything that can be added starts ticked
      setToRemove(new Set(p.removed.filter((r) => r.suggestRemove).map((r) => r.key))); // only the safe ones
    }, (e) => live && setError(e));
    return () => { live = false; };
  }, [payloadId, JSON.stringify(choices)]); // eslint-disable-line react-hooks/exhaustive-deps

  const addable = plan?.rows.map((r, i) => [r, rowKey(r, i)] as const).filter(([r]) => r.status === "add") ?? [];
  const picked = addable.filter(([, k]) => !unticked.has(k));
  const counts = plan && {
    add: addable.length,
    inPack: plan.rows.filter((r) => r.status === "inPack").length,
    noBuild: plan.rows.filter((r) => r.status === "noBuild").length,
  };

  return (
    <Modal title={title ?? `Add payload: ${plan?.name ?? "…"}`} width={720} onClose={onClose}>
      <div className="stack-lg">
        {error && <ErrorNotice error={error} />}
        {!plan && !error && <p className="muted">Resolving every mod for Minecraft {pack.mcVersion}…</p>}
        {plan && counts && (
          <>
            <p className="muted" style={{ margin: 0 }}>
              {counts.add} to add · {counts.inPack} already in the pack · {counts.noBuild} with no build here.
              Dependencies are added automatically.
            </p>
            {plan.notes.map((n, i) => <Notice key={i} title={n} />)}
            {plan.choices.map((c) => (
              <div key={c.id} className="choice-pick">
                <div className="pixel">{c.label} <span className="muted">· {c.from}</span></div>
                <div className="row" role="radiogroup" aria-label={c.label}>
                  {c.options.map(([id, label]) => (
                    <Button key={id} size="small" className="mc-choice" aria-pressed={c.chosen === id} onClick={() => setChoices({ ...choices, [c.id]: id })}>{label}</Button>
                  ))}
                </div>
                {c.otherOptionInPack.length > 0 && (
                  <div className="warn" style={{ fontSize: 12 }}>The pack already has {c.otherOptionInPack.join(", ")} from another option; switching won't remove them.</div>
                )}
              </div>
            ))}
            {plan.removed.length > 0 && (
              <div className="removed-box">
                <div className="pixel">No longer in this payload ({plan.removed.length})</div>
                <p className="muted" style={{ margin: "2px 0 6px", fontSize: 12 }}>
                  Tick the ones to remove from the pack as well. Unticked ones stay, as your own mods. Dependencies nothing else needs are removed with them, and you can undo.
                </p>
                <ul className="plan-rows">
                  {plan.removed.map((r) => {
                    // "Needed by" ignores mods that are being removed in the same go.
                    const removingNames = new Set(plan.removed.filter((x) => toRemove.has(x.key)).map((x) => x.name));
                    const stillNeeded = r.neededBy.filter((n) => !removingNames.has(n));
                    return (
                      <li key={r.key} className="plan-row">
                        <input type="checkbox" checked={toRemove.has(r.key)} aria-label={`Remove ${r.name} from the pack`}
                          onChange={(e) => { const n = new Set(toRemove); if (e.target.checked) n.add(r.key); else n.delete(r.key); setToRemove(n); }} />
                        <span className="grow">
                          <strong>{r.name}</strong>
                          {stillNeeded.length > 0 && <span className="err"> · needed by {stillNeeded.join(", ")}: removing it will break {stillNeeded.length === 1 ? "that mod" : "those mods"}</span>}
                          {r.otherPayloads.length > 0 && <span className="warn"> · also in {r.otherPayloads.join(", ")}</span>}
                          {r.userAdded && <span className="muted"> · you added this yourself</span>}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              </div>
            )}
            <ul className="plan-rows">
              {plan.rows.map((r, i) => {
                const k = rowKey(r, i);
                return (
                  <li key={k} className={`plan-row plan-row--${r.status}`}>
                    {r.status === "add" ? (
                      <input type="checkbox" checked={!unticked.has(k)} aria-label={`Add ${r.name}`}
                        onChange={(e) => { const n = new Set(unticked); if (e.target.checked) n.delete(k); else n.add(k); setUnticked(n); }} />
                    ) : <span className="plan-row__mark">{r.status === "inPack" ? "✔" : "✖"}</span>}
                    {r.status === "add" && <ModIcon url={r.iconUrl} label={r.name} size={24} />}
                    <span className="grow">
                      <strong>{r.status === "add" ? r.name : r.label}</strong>
                      {r.status === "add" && r.fallback && <span className="muted"> · fallback for {r.label}</span>}
                      {r.status === "add" && r.pinNumber && <span className="muted"> · exactly {r.pinNumber}{r.forcePin ? " (made for another version/loader, on purpose)" : ""}</span>}
                      {r.status === "inPack" && <span className="muted"> · already in the pack{r.name !== r.label ? ` as ${r.name}` : ""}</span>}
                      {r.status === "noBuild" && <span className="err"> · no build for {pack.mcVersion} (tried {r.tried.join(", ")})</span>}
                      {r.status === "add" && r.kind !== "mod" && <span className="mini-badge">{kindLabel(r.kind)}</span>}
                    </span>
                    {r.from !== plan.name && <span className="muted" style={{ fontSize: 11 }}>from {r.from}</span>}
                  </li>
                );
              })}
            </ul>
          </>
        )}
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={onClose}>Cancel</Button>
          <Button variant="primary" disabled={!plan || busy} onClick={async () => {
            if (!plan) return;
            setBusy(true);
            try {
              await onApply({
                plan,
                add: picked.map(([r]) => r as Extract<PlanRow, { status: "add" }>),
                covers: plan.rows.filter((r): r is Extract<PlanRow, { status: "inPack" }> => r.status === "inPack").map((r) => r.key),
                choices: Object.fromEntries(plan.choices.map((c) => [c.id, c.chosen])),
                remove: plan.removed.filter((r) => toRemove.has(r.key)).map((r) => r.key),
                untag: plan.removed.filter((r) => !toRemove.has(r.key)).map((r) => r.key),
              });
              onClose();
            } catch (e) { setError(e as AppError); setBusy(false); }
          }}>{busy ? "Applying…" : [picked.length && `Add ${picked.length}`, toRemove.size && `remove ${toRemove.size}`].filter(Boolean).join(", ").replace(/^./, (m) => m.toUpperCase()) || "Apply"}</Button>
        </div>
      </div>
    </Modal>
  );
}
