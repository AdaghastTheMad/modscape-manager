import { useEffect, useState } from "react";
import { api, type AppError, type Payload } from "../../api";
import { Button, ErrorNotice, Panel } from "../../components/kit";
import type { Pack } from "../../model";
import { describePayload } from "../Payloads";

/** Browse-pane list of payloads: drag one onto the pack (or graph), or press Add. */
export function PayloadsPane({ pack, modeSwitch, onOpen }: { pack: Pack; modeSwitch: React.ReactNode; onOpen: (id: string) => void }) {
  const [list, setList] = useState<Payload[] | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  useEffect(() => { api.listPayloads().then(setList, setError); }, []);
  const applied = new Map(pack.appliedPayloads.map((a) => [a.id, a.version]));
  return (
    <Panel title="Browse" className="pane" actions={modeSwitch}>
      <div className="pane__body stack">
        <p className="muted" style={{ fontSize: 12, margin: 0 }}>Drag a payload onto the pack or the graph, or press Add. You'll see a preview first. Edit payloads on the Payloads tab.</p>
        {error && <ErrorNotice error={error} />}
        <ul className="hit-list">
          {(list ?? []).map((p) => (
            <li key={p.id} className="hit" draggable
              onDragStart={(e) => { e.dataTransfer.setData("application/x-mpp-payload", p.id); e.dataTransfer.effectAllowed = "copy"; }}
              title={p.description}>
              <div className="grow">
                <div className="hit__name">{p.name}</div>
                <div className="hit__meta">
                  {describePayload(p)}
                  {applied.has(p.id) && (applied.get(p.id)! < p.version ? " · updated since you added it" : " · added")}
                </div>
              </div>
              <Button size="small" variant="primary" onClick={() => onOpen(p.id)}>Add…</Button>
            </li>
          ))}
        </ul>
        {list?.length === 0 && <div className="empty-state"><p>No payloads yet. Make one on the Payloads tab.</p></div>}
      </div>
    </Panel>
  );
}
