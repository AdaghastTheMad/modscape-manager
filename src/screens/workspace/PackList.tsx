import { useMemo, useState } from "react";
import type { Analysis, Fix, Issue, Severity, Suggestion } from "../../api";
import { Button, ModIcon, Notice, Panel, Select, SourceBadge, Tabs, Tooltip } from "../../components/kit";
import { SuggestionPicker } from "./Suggestions";
import { GROUPINGS, groupKey, groupRank, groupTitle, isLibraryGroup, type Grouping } from "../../grouping";
import { entryName, isLibrary, prettyCategory, type Pack, type PackEntry } from "../../model";


const SEV_ICON: Record<Severity, string> = { error: "✖", warning: "!", info: "i" };

export function fixLabel(f: Fix): string {
  switch (f.kind) {
    case "addProject": return `Add ${f.name}`;
    case "remove": return "Remove";
    case "unpin": return "Unpin";
    case "pinTo": return `Pin ${f.versionNumber}`;
    case "useSource": return `Use ${f.source === "modrinth" ? "Modrinth" : "CurseForge"}`;
  }
}

function worst(issues: Issue[]): Severity | null {
  if (issues.some((i) => i.severity === "error")) return "error";
  if (issues.some((i) => i.severity === "warning")) return "warning";
  return issues.length ? "info" : null;
}

function Checks({ pack, analysis, analyzing, onFix, onSelect, onRecheck, suggestions, onSuggest, onDismiss, adding }: {
  pack: Pack; analysis: Analysis | null; analyzing: boolean; onFix: (f: Fix) => void; onSelect: (k: string) => void; onRecheck: () => void;
  suggestions: Suggestion[]; onSuggest: (ids: string[]) => void; onDismiss: (s: Suggestion) => void; adding: boolean;
}) {
  const [open, setOpen] = useState<boolean | null>(null);
  const [showSugg, setShowSugg] = useState(false);
  const issues = analysis?.issues ?? [];
  const n = (s: Severity) => issues.filter((i) => i.severity === s).length;
  const errors = n("error"), warnings = n("warning"), infos = n("info");
  const expanded = open ?? errors > 0;
  const count = Object.keys(pack.entries).length;
  if (count === 0) return null;

  const summary = !analysis ? (analyzing ? "Checking…" : "Not checked yet")
    : errors + warnings === 0 ? "All checks pass"
    : [errors && `${errors} error${errors > 1 ? "s" : ""}`, warnings && `${warnings} warning${warnings > 1 ? "s" : ""}`].filter(Boolean).join(" · ");

  return (
    <section className={`checks checks--${errors ? "error" : warnings ? "warning" : "ok"}`} aria-label="Pack checks">
      <div className="row checks__head">
        <button type="button" className="checks__toggle" onClick={() => setOpen(!expanded)} aria-expanded={expanded}>
          <span className="pixel">{errors ? "✖" : warnings ? "!" : "✔"} {summary}</span>
          {infos > 0 && <span className="muted"> · {infos} note{infos > 1 ? "s" : ""}</span>}
          {analyzing && analysis && <span className="muted"> · rechecking…</span>}
          {analysis && !analysis.complete && <span className="muted"> · some mods couldn't be checked</span>}
        </button>
        <span className="grow" />
        <Button size="small" onClick={onRecheck} disabled={analyzing} title="Re-fetch everything, bypassing the cache">Recheck</Button>
      </div>
      {expanded && issues.length > 0 && (
        <ul className="issue-list">
          {issues.map((i) => (
            <li key={i.id} className={`issue issue--${i.severity}`}>
              <span className="issue__icon" aria-label={i.severity}>{SEV_ICON[i.severity]}</span>
              <button type="button" className="issue__msg" onClick={() => i.entryKey && onSelect(i.entryKey)}>{i.message}</button>
              {i.fix && <Button size="small" variant={i.fix.kind === "remove" ? "danger" : "primary"} onClick={() => onFix(i.fix!)}>{fixLabel(i.fix)}</Button>}
            </li>
          ))}
        </ul>
      )}
      {suggestions.length > 0 && (
        <div className="suggestions">
          <button type="button" className="checks__toggle muted" onClick={() => setShowSugg(!showSugg)} aria-expanded={showSugg}>
            {showSugg ? "▾" : "▸"} {suggestions.length} optional dependenc{suggestions.length > 1 ? "ies" : "y"} suggested
          </button>
          {showSugg && (
            <SuggestionPicker suggestions={suggestions} onAdd={onSuggest} onDismiss={onDismiss} busy={adding}
              describe={(s) => `optional for ${s.suggestedBy.map((k) => entryName(pack, k)).join(", ")}`} />
          )}
        </div>
      )}
    </section>
  );
}

export type PackView = "list" | "graph";

export function PackList({ pack, analysis, analyzing, selectedKey, onSelect, onRemove, undo, notice, onFix, onRecheck, onSuggest, onDismiss, onMatchAll, matching, adding, onCrossCheck, view, onView, graph, grouping, onGrouping }: {
  pack: Pack; analysis: Analysis | null; analyzing: boolean; selectedKey?: string; onSelect: (k: string) => void; onRemove: (k: string) => void;
  undo: { label: string; run: () => void } | null; notice: React.ReactNode; onFix: (f: Fix) => void; onRecheck: () => void;
  onSuggest: (ids: string[]) => void; onDismiss: (s: Suggestion) => void; onMatchAll: () => void; matching: boolean; adding: boolean;
  onCrossCheck: (v: boolean) => void;
  view: PackView; onView: (v: PackView) => void; graph: React.ReactNode;
  grouping: Grouping; onGrouping: (g: Grouping) => void;
}) {
  const by = grouping;
  const issuesByEntry = useMemo(() => {
    const m = new Map<string, Issue[]>();
    for (const i of analysis?.issues ?? []) {
      for (const k of [i.entryKey, i.kind === "incompatible" ? i.otherKey : null]) {
        if (k) m.set(k, [...(m.get(k) ?? []), i]);
      }
    }
    return m;
  }, [analysis]);

  const groups = useMemo(() => {
    const m = new Map<string, [string, PackEntry][]>();
    for (const [k, e] of Object.entries(pack.entries)) {
      const g = groupKey(e, by, pack);
      m.set(g, [...(m.get(g) ?? []), [k, e]]);
    }
    for (const list of m.values()) list.sort((a, b) => a[1].display.name.localeCompare(b[1].display.name));
    // "Added by you" first; libraries, unneeded and untagged last.
    return [...m.entries()].sort((a, b) => groupRank(a[0]) - groupRank(b[0]) || a[0].localeCompare(b[0]));
  }, [pack, by]);
  const count = Object.keys(pack.entries).length;
  const deps = Object.values(pack.entries).filter((e) => e.reason === "dependency").length;
  const all = Object.values(pack.entries);
  const sites = {
    modrinth: all.filter((e) => e.ref.preferred === "modrinth").length,
    curseforge: all.filter((e) => e.ref.preferred === "curseforge").length,
    oneSite: all.filter((e) => !e.ref.modrinthId || e.ref.curseforgeId == null).length,
  };

  return (
    <Panel className="pane" title={`Pack · ${count} mod${count === 1 ? "" : "s"}`} actions={
      <Tabs label="View (G)" value={view} onChange={onView} tabs={[{ id: "list", label: "List" }, { id: "graph", label: "Graph" }]} />
    }>
      <div className="pane__fixed stack">
        <Checks pack={pack} analysis={analysis} analyzing={analyzing} onFix={onFix} onSelect={onSelect} onRecheck={onRecheck}
          suggestions={analysis?.suggestions ?? []} onSuggest={onSuggest} onDismiss={onDismiss} adding={adding} />
        {notice}
        {undo && <Notice title={undo.label}><Button size="small" onClick={undo.run}>Undo</Button></Notice>}
        {count > 0 && (
          <div className="row" style={{ justifyContent: "space-between" }}>
            <span className="muted" style={{ fontSize: 12 }}>
              {sites.modrinth} from Modrinth · {sites.curseforge} from CurseForge
              {sites.oneSite > 0 && <> · {sites.oneSite} linked on one site only</>}
            </span>
            {sites.oneSite > 0 && <Button size="small" onClick={onMatchAll} disabled={matching} title="Look for each mod's copy on the other site">{matching ? "Matching…" : "Match sites"}</Button>}
          </div>
        )}
        {count > 0 && (
          <label className="mc-check" style={{ fontSize: 12 }} title="For mods linked on both sites, also read the other copy's required dependencies. Uses extra CurseForge API calls, so it's off unless you turn it on.">
            <input type="checkbox" checked={!!pack.crossCheckDeps} onChange={(e) => onCrossCheck(e.target.checked)} />
            Double-check dependencies with each mod's other-site copy
          </label>
        )}
        {count > 0 && (
          <div className="row" style={{ justifyContent: "space-between" }}>
            <span className="muted" style={{ fontSize: 12 }}>{count - deps} added by you · {deps} dependenc{deps === 1 ? "y" : "ies"}</span>
            <Select aria-label="Group by" value={by} onChange={(e) => onGrouping(e.target.value as Grouping)}>
              {GROUPINGS.map((g) => <option key={g.id} value={g.id}>Group by {g.label}</option>)}
            </Select>
          </div>
        )}
      </div>
      {view === "graph" ? graph : <div className="pane__body stack">
        {count === 0 ? (
          <div className="empty-state"><div><div className="pixel">This pack is empty</div><p>Search on the left and press + (or Enter) to add mods. Their dependencies come along automatically.</p></div></div>
        ) : groups.map(([g, list]) => (
          <section key={g} className={isLibraryGroup(g) ? "group group--lib" : "group"}>
            <h3 className="group__title">{groupTitle(g, by)} <span className="muted">({list.length})</span></h3>
            <ul className="entry-list">
              {list.map(([k, e]) => {
                const iss = issuesByEntry.get(k) ?? [];
                const w = worst(iss);
                const parents = e.reason === "dependency" ? e.pulledInBy.filter((p) => p in pack.entries).map((p) => entryName(pack, p)) : [];
                const file = analysis?.entries[k]?.file;
                return (
                  <li key={k} className={`entry${selectedKey === k ? " entry--sel" : ""}${isLibrary(e) && by === "pulledInBy" ? " entry--lib" : ""}`}
                    tabIndex={0} aria-selected={selectedKey === k} onClick={() => onSelect(k)} onFocus={() => onSelect(k)}>
                    <ModIcon url={e.display.iconUrl} label={e.display.name} size={36} selected={selectedKey === k} error={w === "error"} />
                    <div className="grow">
                      <div className="hit__name">
                        {e.display.name}
                        {file?.pinned && <span className="pin-badge" title={`Pinned to ${file.versionNumber}`}>📌</span>}
                      </div>
                      <div className="hit__meta">
                        {parents.length ? `pulled in by ${parents.join(", ")}` : e.display.categories.map(prettyCategory).join(", ") || "no category"}
                        {file && ` · ${file.versionNumber}`}
                      </div>
                    </div>
                    {w && w !== "info" && (
                      <Tooltip name={`${iss.length} problem${iss.length > 1 ? "s" : ""}`} lines={iss.map((i) => i.message)} placement="below">
                        <span className={`sev-badge sev-badge--${w}`} tabIndex={0} aria-label={`${w}: ${iss.map((i) => i.message).join(" ")}`}>{SEV_ICON[w]}</span>
                      </Tooltip>
                    )}
                    <span className="badge-stack" title={e.ref.modrinthId && e.ref.curseforgeId != null ? "Linked on both sites; the bright badge is the one in use" : undefined}>
                      <SourceBadge source={e.ref.preferred} />
                      {e.ref.modrinthId && e.ref.curseforgeId != null && <span style={{ opacity: 0.45 }}><SourceBadge source={e.ref.preferred === "modrinth" ? "curseforge" : "modrinth"} /></span>}
                    </span>
                    <Button size="small" variant="danger" aria-label={`Remove ${e.display.name}`} onClick={(ev) => { ev.stopPropagation(); onRemove(k); }}>✕</Button>
                  </li>
                );
              })}
            </ul>
          </section>
        ))}
      </div>}
    </Panel>
  );
}
