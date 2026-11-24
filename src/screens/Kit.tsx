import { useState } from "react";
import { Button, Input, Notice, Panel, Select, Slot, SourceBadge, Tabs, Tooltip } from "../components/kit";

export function Kit() {
  const [tab, setTab] = useState<"mods" | "rp" | "shaders" | "dp">("mods");
  return (
    <div className="kit-grid">
      <Panel title="Buttons">
        <div className="stack">
          <div className="row"><Button>Default</Button><Button variant="primary">Add mod</Button><Button variant="danger">Remove</Button><Button disabled>Disabled</Button></div>
          <div className="row"><Button size="small">Small</Button><Button size="small" variant="primary">Small primary</Button></div>
        </div>
      </Panel>
      <Panel title="Slots and tooltips">
        <div className="stack">
          <div className="slot-row">
            {["Sodium", "Lithium", "FerriteCore", "Jade", "EMI"].map((n, i) => (
              <Tooltip key={n} name={n} badge={<SourceBadge source={i % 2 ? "curseforge" : "modrinth"} />}
                lines={["Example tooltip line", `${(i + 1) * 3}M downloads`]}>
                <span tabIndex={0}><Slot label={n} selected={i <= 1} error={i >= 3} /></span>
              </Tooltip>
            ))}
          </div>
          <p className="muted">Hover or focus a slot. Purple = selected, red = failing check; adjacent highlights share the edge evenly.</p>
          <div className="row"><SourceBadge source="modrinth" /><SourceBadge source="curseforge" /></div>
        </div>
      </Panel>
      <Panel title="Inputs">
        <div className="stack">
          <Input label="Search" placeholder="Search mods…" hint="Press / to focus search (arrives with M2)" />
          <Select label="Loader" defaultValue="neoforge"><option value="neoforge">NeoForge</option><option value="fabric">Fabric</option></Select>
        </div>
      </Panel>
      <Panel title="Tabs">
        <Tabs label="Content type" underline value={tab} onChange={setTab} tabs={[
          { id: "mods", label: "Mods" }, { id: "rp", label: "Resource packs" }, { id: "shaders", label: "Shaders" }, { id: "dp", label: "Datapacks" },
        ]} />
        <Panel inset><p className="muted">Selected: {tab}. Use ← → when a tab has focus.</p></Panel>
      </Panel>
      <Panel title="Notices">
        <div className="stack">
          <Notice tone="ok" title="All checks pass" />
          <Notice title="Not updated in over a year">Soft warning example.</Notice>
          <Notice tone="error" title="Missing required dependency">Example error.</Notice>
        </div>
      </Panel>
      <Panel title="Inset panel" inset>
        <p>Recessed panel with the 2 px inner bevel. Body text stays in the system sans-serif; headings and buttons use the pixel font.</p>
      </Panel>
    </div>
  );
}
