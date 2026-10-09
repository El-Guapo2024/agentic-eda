// The Appearance panel (docked right, with the selection filter and Activity -- see RightDock.tsx), KiCad's `APPEARANCE_CONTROLS`
// (pcbnew/widgets/appearance_controls.cpp): tabs for the layers, the objects, the nets and the net classes, and under them the layer presets and the viewports.
// The model is kicad-port/appearance*.ts; what is chosen here is kept per project in appearance.json (components/panels/appearance/useAppearanceSync.ts), the way
// KiCad keeps it in the project's local settings, and never in the design.
//
// KiCad has three tabs, Layers, Objects and Nets, with the net classes under the nets; they are a tab of their own here so each list has the dock's width to itself.
import { useState } from "react";
import { LayersTab } from "./appearance/LayersTab";
import { ObjectsTab } from "./appearance/ObjectsTab";
import { NetsTab } from "./appearance/NetsTab";
import { NetClassesTab } from "./appearance/NetClassesTab";
import { PresetBar } from "./appearance/PresetBar";
import "../../styles/appearance.css";

type SubTab = "layers" | "objects" | "nets" | "classes";

const TABS: ReadonlyArray<{ id: SubTab; label: string }> = [
  { id: "layers", label: "Layers" },
  { id: "objects", label: "Objects" },
  { id: "nets", label: "Nets" },
  { id: "classes", label: "Net Classes" },
];

export function AppearancePanel() {
  const [tab, setTab] = useState<SubTab>("layers");
  return (
    <div className="ap-root" style={{ flex: 1, minHeight: 0 }}>
      <div className="dock-tabs">
        {TABS.map((t) => (
          <div key={t.id} className={`dock-tab${tab === t.id ? " active" : ""}`} data-tab={t.id} onClick={() => setTab(t.id)}>
            {t.label}
          </div>
        ))}
      </div>
      <div style={{ flex: 1, minHeight: 0, overflowY: "auto" }}>
        {tab === "layers" && <LayersTab />}
        {tab === "objects" && <ObjectsTab />}
        {tab === "nets" && <NetsTab />}
        {tab === "classes" && <NetClassesTab />}
      </div>
      <PresetBar />
    </div>
  );
}
