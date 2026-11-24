import { useEffect, useState } from "react";
import type { Suggestion } from "../../api";
import { Button, ModIcon } from "../../components/kit";

/** Optional-dependency picker: tick any number, then "Add selected" (or "Add all" when none are ticked). */
export function SuggestionPicker({ suggestions, describe, onAdd, onDismiss, busy }: {
  suggestions: Suggestion[];
  /** Secondary text per suggestion, e.g. "optional for Sodium Extra". */
  describe?: (s: Suggestion) => string;
  onAdd: (ids: string[]) => void;
  onDismiss?: (s: Suggestion) => void;
  busy?: boolean;
}) {
  const [picked, setPicked] = useState<Set<string>>(new Set());
  // Drop ticks for suggestions that went away (added or dismissed elsewhere).
  const ids = suggestions.map((s) => s.projectId).join("|");
  useEffect(() => {
    setPicked((p) => new Set([...p].filter((id) => suggestions.some((s) => s.projectId === id))));
  }, [ids]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!suggestions.length) return null;
  const toggle = (id: string, on: boolean) => setPicked((p) => { const n = new Set(p); if (on) n.add(id); else n.delete(id); return n; });
  const chosen = picked.size ? [...picked] : suggestions.map((s) => s.projectId);
  return (
    <div className="stack" style={{ gap: 4 }}>
      <ul className="issue-list">
        {suggestions.map((s) => (
          <li key={s.projectId} className="issue issue--info">
            <label className="mc-check" title={s.summary}>
              <input type="checkbox" checked={picked.has(s.projectId)} onChange={(e) => toggle(s.projectId, e.target.checked)} aria-label={`Select ${s.name}`} />
            </label>
            <ModIcon url={s.iconUrl} label={s.name} size={28} />
            <span className="issue__msg" title={s.summary} onClick={() => toggle(s.projectId, !picked.has(s.projectId))}>
              <strong>{s.name}</strong>{describe && <span className="muted"> · {describe(s)}</span>}
            </span>
            {onDismiss && <Button size="small" onClick={() => onDismiss(s)} aria-label={`Dismiss ${s.name}`} title="Don't suggest this again">✕</Button>}
          </li>
        ))}
      </ul>
      <div className="row">
        <Button size="small" variant="primary" disabled={busy} onClick={() => { onAdd(chosen); setPicked(new Set()); }}>
          {busy ? "Adding…" : picked.size ? `Add selected (${picked.size})` : `Add all (${suggestions.length})`}
        </Button>
        {picked.size > 0 && <Button size="small" onClick={() => setPicked(new Set())}>Clear</Button>}
      </div>
    </div>
  );
}
