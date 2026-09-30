import { useEffect } from "react";
import { StudioProvider, useStudioDispatch, useStudioState } from "./state/store";
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
  return (
    <div className="app-frame">
      <div className="menubar-row">
        <MenuBar />
      </div>
      <div className="main-toolbar-row">
        <Toolbar id="main" />
        <QuickActions />
      </div>
      <div className="aux-toolbar-row">
        <Toolbar id="auxiliary" />
      </div>
      <div className="app-body">
        <div className="properties-col">
          <div className="dock">
            <PropertiesPanel />
          </div>
        </div>
        <div className="options-toolbar-col">
          <Toolbar id="options" />
        </div>
        <div className="canvas-col">
          {state.tab === "pcb" ? <Canvas /> : <SchematicView />}
          <Toast />
        </div>
        <div className="drawing-toolbar-col">
          <Toolbar id="drawing" />
        </div>
        <div className="right-dock-col">
          <RightDock />
        </div>
      </div>
      <div className="message-panel-row">
        <MessagePanel />
      </div>
      <div className="status-bar-row">
        <StatusBar />
      </div>
      <DrcDialog />
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
