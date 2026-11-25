import { useEffect, useState } from "react";
import { ShortcutsHelp } from "./components/Shortcuts";
import { typingIn } from "./hooks";
import { usePrefs } from "./prefs";
import { uiClick } from "./sound";
import { Tabs } from "./components/kit";
import { Kit } from "./screens/Kit";
import { Packs } from "./screens/Packs";
import { Payloads } from "./screens/Payloads";
import { useUnsavedGuard } from "./unsaved";
import { Settings } from "./screens/Settings";
import { Templates } from "./screens/Templates";
import { Workspace } from "./screens/Workspace";

type Screen = "packs" | "payloads" | "templates" | "kit" | "settings";

export default function App() {
  const [screen, setScreen] = useState<Screen>("packs");
  const [openPack, setOpenPack] = useState<string | null>(null);
  const { guard, prompt } = useUnsavedGuard();
  const [help, setHelp] = useState(false);
  const prefs = usePrefs();
  // Clicks: "click" also fires for Enter/Space on a focused button, so keyboard and mouse sound alike.
  useEffect(() => {
    if (!prefs.clickSound) return;
    const LIST = ".entry, .hit, .tpl-file";
    const onClick = (e: MouseEvent) => {
      const t = e.target as HTMLElement | null;
      const b = t?.closest("button, [role=tab]") as HTMLButtonElement | null;
      if (b) { if (!b.disabled) uiClick("button"); return; }
      if (t?.closest(LIST)) uiClick("list");
    };
    // List items are picked from the keyboard with Enter/Space (they aren't buttons).
    const onKey = (e: KeyboardEvent) => {
      if ((e.key === "Enter" || e.key === " ") && (e.target as HTMLElement | null)?.matches?.(LIST)) uiClick("list");
    };
    document.addEventListener("click", onClick, true);
    document.addEventListener("keydown", onKey, true);
    return () => { document.removeEventListener("click", onClick, true); document.removeEventListener("keydown", onKey, true); };
  }, [prefs.clickSound]);
  const go = (s: Screen) => { if (s !== screen) guard(() => setScreen(s)); };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typingIn(e) || document.querySelector(".mc-modal-backdrop")) return;
      if (e.key === "?") { e.preventDefault(); uiClick(); setHelp(true); return; }
      const order: Screen[] = ["packs", "payloads", "templates", "settings"];
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && /^[1-4]$/.test(e.key)) { e.preventDefault(); if (order[Number(e.key) - 1] !== screen) uiClick(); go(order[Number(e.key) - 1]); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
  return (
    <div className={`app${prefs.backdrop ? " app--backdrop" : ""}`}>
      <header className="app-header">
        <h1>Modpack Planner</h1>
        <Tabs label="Main" value={screen} onChange={go} tabs={[
          { id: "packs", label: openPack ? "Workspace" : "Packs" },
          { id: "payloads", label: "Payloads" },
          { id: "templates", label: "Templates" },
          ...(prefs.showKit ? [{ id: "kit" as const, label: "Component kit" }] : []),
          { id: "settings", label: "Settings" },
        ]} />
        <span className="grow" />
        <button type="button" className="mc-btn mc-btn--small app-help" onClick={() => setHelp(true)} aria-label="Keyboard shortcuts" title="Keyboard shortcuts (?)">?</button>
      </header>
      <main className="app-main">
        {screen === "packs" && (openPack
          ? <Workspace key={openPack} packId={openPack} onClose={() => setOpenPack(null)} onOpen={setOpenPack} />
          : <Packs onOpen={setOpenPack} />)}
        {screen === "payloads" && <Payloads />}
        {screen === "templates" && <Templates />}
        {screen === "kit" && prefs.showKit && <Kit />}
        {screen === "settings" && <Settings />}
      </main>
      {prompt}
      {help && <ShortcutsHelp onClose={() => setHelp(false)} />}
    </div>
  );
}
