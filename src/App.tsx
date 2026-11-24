import { useState } from "react";
import { Tabs } from "./components/kit";
import { Kit } from "./screens/Kit";
import { Settings } from "./screens/Settings";
import { Workspace } from "./screens/Workspace";

type Screen = "workspace" | "kit" | "settings";

export default function App() {
  const [screen, setScreen] = useState<Screen>("settings");
  return (
    <div className="app">
      <header className="app-header">
        <h1>Modpack Planner</h1>
        <Tabs label="Main" value={screen} onChange={setScreen} tabs={[
          { id: "workspace", label: "Workspace" },
          { id: "kit", label: "Component kit" },
          { id: "settings", label: "Settings" },
        ]} />
      </header>
      <main className="app-main">
        {screen === "workspace" && <Workspace />}
        {screen === "kit" && <Kit />}
        {screen === "settings" && <Settings />}
      </main>
    </div>
  );
}
