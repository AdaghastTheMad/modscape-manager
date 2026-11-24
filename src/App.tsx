import { useState } from "react";
import { Tabs } from "./components/kit";
import { Kit } from "./screens/Kit";
import { Packs } from "./screens/Packs";
import { Settings } from "./screens/Settings";
import { Workspace } from "./screens/Workspace";

type Screen = "packs" | "kit" | "settings";

export default function App() {
  const [screen, setScreen] = useState<Screen>("packs");
  const [openPack, setOpenPack] = useState<string | null>(null);
  return (
    <div className="app">
      <header className="app-header">
        <h1>Modpack Planner</h1>
        <Tabs label="Main" value={screen} onChange={setScreen} tabs={[
          { id: "packs", label: openPack ? "Workspace" : "Packs" },
          { id: "kit", label: "Component kit" },
          { id: "settings", label: "Settings" },
        ]} />
      </header>
      <main className="app-main">
        {screen === "packs" && (openPack
          ? <Workspace key={openPack} packId={openPack} onClose={() => setOpenPack(null)} onOpen={setOpenPack} />
          : <Packs onOpen={setOpenPack} />)}
        {screen === "kit" && <Kit />}
        {screen === "settings" && <Settings />}
      </main>
    </div>
  );
}
