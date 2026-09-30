import { useEffect, useState } from "react";
import { StudioProvider, useStudioDispatch, useStudioState } from "./state/store";
import { EditorTabs } from "./components/EditorTabs";
import { MenuBar } from "./components/MenuBar";
import { Toolbar } from "./components/Toolbar";
import { QuickActions } from "./components/QuickActions";
import { PropertiesPanel } from "./components/panels/PropertiesPanel";
import { RightDock } from "./components/panels/RightDock";
import { MessagePanel } from "./components/MessagePanel";
import { StatusBar } from "./components/StatusBar";
import { Canvas } from "./components/canvas/Canvas";
import { SchematicView } from "./components/SchematicView";
import { DrcDialog } from "./components/DrcDialog";
import { HotkeysDialog } from "./components/HotkeysDialog";
import { FootprintPropertiesDialog } from "./components/FootprintPropertiesDialog";
import { NetInspectorDialog } from "./components/NetInspectorDialog";
import { ZoneDialog } from "./components/ZoneDialog";
import { TextDialog } from "./components/TextDialog";
import { Viewer3D, type Viewer3DApi } from "./components/viewer3d/Viewer3D";
import { Viewer3DToolbar } from "./components/viewer3d/Viewer3DToolbar";
import { useGlobalHotkeys } from "./actions/useGlobalHotkeys";
import "./styles/global.css";
import "./styles/layout.css";
import "./styles/panels.css";

function Toast() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  useEffect(() => {
    if (!state.toast) return;
    const t = setTimeout(() => dispatch({ type: "TOAST_CLEAR" }), state.toast.kind === "info" ? 2500 : 6000);
    return () => clearTimeout(t);
  }, [state.toast, dispatch]);
  if (!state.toast) return null;
  return (
    <div
      style={{
        position: "absolute",
        left: "50%",
        bottom: 16,
        transform: "translateX(-50%)",
        background: state.toast.kind === "error" ? "#2b1a1a" : "#16263a",
        border: `1px solid ${state.toast.kind === "error" ? "#ef5b5b" : "#4aa3ff"}`,
        color: state.toast.kind === "error" ? "#ffd6d6" : "#d6e8ff",
        padding: "8px 12px",
        borderRadius: 8,
        maxWidth: "80%",
        boxShadow: "0 6px 20px rgba(0,0,0,.4)",
        zIndex: 3000,
      }}
    >
      {state.toast.message}
    </div>
  );
}

function StudioFrame() {
  const state = useStudioState();
  useGlobalHotkeys();
  const [viewer3d, setViewer3d] = useState<Viewer3DApi | null>(null);
  // The 3D tab has no real "own chrome" to extract (3d-viewer/'s source
  // wasn't available this session) and no properties/appearance concept
  // that applies to it the way PCB/Schematic's docks do, so it drops
  // those columns entirely rather than showing panels that would have
  // nothing real to say for this tab. Its view-preset toolbar takes the
  // normal <Toolbar id="main"/> row's place instead.
  const is3d = state.tab === "3d";
  return (
    <div className="app-frame">
      <div className="menubar-row">
        <MenuBar />
      </div>
      <div className="tabs-row">
        <EditorTabs />
      </div>
      <div className="main-toolbar-row">
        {is3d ? <Viewer3DToolbar api={viewer3d} /> : <Toolbar id="main" />}
        <QuickActions />
      </div>
      {!is3d && (
        <div className="aux-toolbar-row">
          <Toolbar id="auxiliary" />
        </div>
      )}
      <div className="app-body">
        {!is3d && (
          <div className="properties-col">
            <div className="dock">
              <PropertiesPanel />
            </div>
          </div>
        )}
        {!is3d && (
          <div className="options-toolbar-col">
            <Toolbar id="options" />
          </div>
        )}
        <div className="canvas-col">
          {state.tab === "pcb" && <Canvas />}
          {state.tab === "schematic" && <SchematicView />}
          {is3d && <Viewer3D onReady={setViewer3d} />}
          <Toast />
        </div>
        {!is3d && (
          <div className="drawing-toolbar-col">
            <Toolbar id="drawing" />
          </div>
        )}
        {!is3d && (
          <div className="right-dock-col">
            <RightDock />
          </div>
        )}
      </div>
      <div className="message-panel-row">
        <MessagePanel />
      </div>
      <div className="status-bar-row">
        <StatusBar />
      </div>
      <DrcDialog />
      <HotkeysDialog />
      <FootprintPropertiesDialog />
      <NetInspectorDialog />
      <ZoneDialog />
      <TextDialog />
    </div>
  );
}

export default function App() {
  return (
    <StudioProvider>
      <StudioFrame />
    </StudioProvider>
  );
}
