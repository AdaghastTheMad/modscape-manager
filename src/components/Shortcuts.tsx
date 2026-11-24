import { Button, Modal } from "./kit";

export const IS_MAC = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);
const M = IS_MAC ? "⌘" : "Ctrl";
const k = (...keys: string[]) => keys.map((x) => <kbd key={x}>{x}</kbd>);

const GROUPS: [string, [React.ReactNode, string][]][] = [
  ["Everywhere", [
    [k("?"), "Show this list"],
    [k(M, "1"), "Packs / workspace"], [k(M, "2"), "Payloads"], [k(M, "3"), "Templates"], [k(M, "4"), "Settings"],
    [k("Esc"), "Close a dialog"],
  ]],
  ["Workspace", [
    [k("/"), "Search mods"],
    [<>{k("↑")}{k("↓")}</>, "Move through search results"],
    [k("Enter"), "Add the highlighted result"],
    [IS_MAC ? <>{k("⌫")} or {k("Del")}</> : k("Del"), "Remove the selected mod"],
    [k("Esc"), "Clear the selection"],
    [k("G"), "Switch list / graph"],
    [k(M, "Z"), "Undo"],
    [IS_MAC ? k("⇧", "⌘", "Z") : <>{k("Ctrl", "Y")} or {k("Ctrl", "⇧", "Z")}</>, "Redo"],
    [k(M, "S"), "Save now (it also saves on its own)"],
    [k(M, "⇧", "S"), "Snapshots"],
    [k(M, "E"), "Export"],
  ]],
  ["Graph", [
    [<>Click, {k(M)}/{k("⇧")}-click</>, "Select one, add to the selection"],
    [<>{k("⇧")}-drag</>, "Box select"],
    ["Two-finger scroll / pinch", "Pan / zoom"],
    ["Double-click", "Open details"],
  ]],
  ["Editors", [
    [k(M, "S"), "Save the open template file"],
    [k("Tab"), "Indent (Esc first to move focus on)"],
  ]],
];

export function ShortcutsHelp({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="Keyboard shortcuts" width={620} onClose={onClose}>
      <div className="shortcuts">
        {GROUPS.map(([g, rows]) => (
          <section key={g}>
            <div className="pixel">{g}</div>
            <table><tbody>{rows.map(([keys, what], i) => <tr key={i}><td className="shortcuts__keys">{keys}</td><td>{what}</td></tr>)}</tbody></table>
          </section>
        ))}
      </div>
      <div className="row" style={{ justifyContent: "flex-end", marginTop: 12 }}><Button onClick={onClose}>Close</Button></div>
    </Modal>
  );
}
