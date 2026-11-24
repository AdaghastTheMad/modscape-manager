import { Panel } from "../components/kit";

function Empty({ title, children }: { title: string; children: string }) {
  return <div className="empty-state"><div><div className="pixel">{title}</div><p>{children}</p></div></div>;
}

export function Workspace() {
  return (
    <div className="workspace">
      <Panel title="Browse"><Empty title="Search arrives in Milestone 2">Modrinth search with version, loader and category filters.</Empty></Panel>
      <Panel title="Pack"><Empty title="No pack open">Pack creation and the mod list arrive in Milestone 2.</Empty></Panel>
      <Panel title="Details"><Empty title="Nothing selected">Select a mod to see its details here.</Empty></Panel>
    </div>
  );
}
