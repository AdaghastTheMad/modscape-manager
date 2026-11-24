import { useEffect, useState } from "react";
import { ShortcutsHelp } from "./components/Shortcuts";
import { typingIn } from "./hooks";
import { usePrefs } from "./prefs";
import { playClick } from "./sound";
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
  useEffect(() => {
    if (!prefs.clickSound) return;
    const onDown = (e: PointerEvent) => {
      const b = (e.target as HTMLElement | null)?.closest("button, [role=tab]");
      if (b && !(b as HTMLButtonElement).disabled) playClick();
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [prefs.clickSound]);
  const go = (s: Screen) => { if (s !== screen) guard(() => setScreen(s)); };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typingIn(e) || document.querySelector(".mc-modal-backdrop")) return;
      if (e.key === "?") { e.preventDefault(); setHelp(true); return; }
      const order: Screen[] = ["packs", "payloads", "templates", "settings"];
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && /^[1-4]$/.test(e.key)) { e.preventDefault(); go(order[Number(e.key) - 1]); }
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
