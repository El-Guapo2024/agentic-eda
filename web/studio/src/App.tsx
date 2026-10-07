import { useEffect, useState } from "react";
import { StudioProvider, useStudioDispatch, useStudioState } from "./state/store";
import { FootprintEditorProvider } from "./state/footprintEditorStore";
import { SymbolEditorProvider } from "./state/symbolEditorStore";
import { useFootprintEditHotkey } from "./actions/useFootprintEditHotkey";
import { FootprintEditorView } from "./components/footprint/FootprintEditorView";
import { SymbolEditorView } from "./components/symbol/SymbolEditorView";
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
import { ErcDialog } from "./components/ErcDialog";
import { HotkeysDialog } from "./components/HotkeysDialog";
import { PreferencesDialog } from "./components/PreferencesDialog";
import { ZoomAreaOverlay } from "./components/ZoomAreaOverlay";
import { FootprintPropertiesDialog } from "./components/FootprintPropertiesDialog";
import { NetInspectorDialog } from "./components/NetInspectorDialog";
import { ZoneDialog } from "./components/ZoneDialog";
import { TextDialog } from "./components/TextDialog";
import { ItemPropertiesDialog } from "./components/ItemPropertiesDialog";
import { MoveExactDialog } from "./components/MoveExactDialog";
import { PcbParityDialogs } from "./components/PcbParityDialogs";
import { PcbSweepDialogs } from "./components/PcbSweepDialogs";
import { VertexEditorPane } from "./components/PcbPointEditDialogs";
import { PcbPickerPrompt } from "./components/PcbPickerPrompt";
import { RouterSettingsDialog } from "./components/RouterSettingsDialog";
import { LengthTuningDialog } from "./components/LengthTuningDialog";
import { CleanupTracksDialog } from "./components/CleanupTracksDialog";
import { BoardStatisticsDialog } from "./components/BoardStatisticsDialog";
import { SwapLayersDialog } from "./components/SwapLayersDialog";
import { GlobalEditTracksAndViasDialog } from "./components/GlobalEditTracksAndViasDialog";
import { GlobalEditTextAndGraphicsDialog } from "./components/GlobalEditTextAndGraphicsDialog";
import { CreateArrayDialog } from "./components/CreateArrayDialog";
import { DimensionPropertiesDialog } from "./components/DimensionPropertiesDialog";
import { BoardSetupDialog } from "./components/BoardSetupDialog";
import { LabelDialog } from "./components/LabelDialog";
import { SheetDialog } from "./components/SheetDialog";
import { BusUnfoldDialog } from "./components/BusUnfoldDialog";
import { PowerSymbolDialog } from "./components/PowerSymbolDialog";
import { SchTextDialog } from "./components/SchTextDialog";
import { SymbolChooserDialog } from "./components/SymbolChooserDialog";
import { SymbolPropertiesDialog } from "./components/SymbolPropertiesDialog";
import { AnnotateDialog } from "./components/AnnotateDialog";
import { SymbolFieldsTableDialog } from "./components/SymbolFieldsTableDialog";
import { FindReplaceDialog } from "./components/FindReplaceDialog";
import { SchematicSetupDialog } from "./components/SchematicSetupDialog";
import { PlotDialog } from "./components/PlotDialog";
import { PlotSchematicDialog } from "./components/PlotSchematicDialog";
import { ExportNetlistDialog } from "./components/ExportNetlistDialog";
import { GenerateDrillDialog } from "./components/GenerateDrillDialog";
import { FootprintPositionDialog } from "./components/FootprintPositionDialog";
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
  useFootprintEditHotkey();
  // Ctrl+Shift+E / Ctrl+E on the Schematic tab are now the registered actions
  // eeschema.EditorControl.editLibSymbolWithSymbolEditor / editWithSymbolEditor
  // (useActionRunner.ts); useSymbolEditHotkey.ts is superseded and no longer called.
  const [viewer3d, setViewer3d] = useState<Viewer3DApi | null>(null);
  // The 3D tab has no real "own chrome" to extract (3d-viewer/'s source
  // wasn't available this session) and no properties/appearance concept
  // that applies to it the way PCB/Schematic's docks do, so it drops
  // those columns entirely rather than showing panels that would have
  // nothing real to say for this tab. Its view-preset toolbar takes the
  // normal <Toolbar id="main"/> row's place instead.
  const is3d = state.tab === "3d";
  // The Footprint Editor tab (GAPS.md #8) is a genuinely separate little
  // editor with its own complete toolbar+canvas (FootprintEditorView),
  // same reasoning the 3D tab already gets dropped out of this frame's
  // own PCB-shaped chrome for -- its own state/footprintEditorStore.tsx
  // even carries a `null` board document, so PropertiesPanel/options-
  // drawing toolbars/RightDock would have nothing real to show here.
  const isFootprint = state.tab === "footprint";
  // The Symbol Editor tab (eeschema's Symbol Editor) is the schematic-side
  // counterpart of the Footprint Editor tab just above -- its own
  // complete toolbar+canvas (SymbolEditorView), same reasoning: its own
  // state/symbolEditorStore.tsx carries a `null` symbol document, so
  // PropertiesPanel/drawing toolbars/RightDock would have nothing real to
  // show here either.
  const isSymbolEditor = state.tab === "symbol";
  const hideBoardChrome = is3d || isFootprint || isSymbolEditor;
  // eeschema's default AUI layout has no layer/appearance manager at all
  // (that's a pcbnew-only concept -- a schematic has no copper/technical
  // layers to toggle) and no selection-filter-by-item-type panel either
  // (pcbnew's exists because tracks/zones/vias/footprints overlap on
  // different layers; a schematic sheet has no such overlap problem).
  // Its right-hand dock is just the drawing/placement toolbar
  // (drawing-toolbar-col below, already tab-aware via Toolbar's own
  // schToolbarsFile lookup) -- so the tabbed Appearance/Filter/Activity
  // dock only makes sense on the PCB tab.
  const showRightDock = state.tab === "pcb";
  return (
    <div className="app-frame">
      <div className="menubar-row">
        <MenuBar />
      </div>
      <div className="tabs-row">
        <EditorTabs />
      </div>
      {!isFootprint && !isSymbolEditor && (
        <div className="main-toolbar-row">
          {is3d ? <Viewer3DToolbar api={viewer3d} /> : <Toolbar id="main" />}
          <QuickActions />
        </div>
      )}
      {!hideBoardChrome && (
        <div className="aux-toolbar-row">
          <Toolbar id="auxiliary" />
        </div>
      )}
      <div className="app-body">
        {!hideBoardChrome && (
          <div className="properties-col">
            <div className="dock">
              <PropertiesPanel />
            </div>
          </div>
        )}
        {!hideBoardChrome && (
          <div className="options-toolbar-col">
            <Toolbar id="options" />
          </div>
        )}
        <div className="canvas-col">
          {state.tab === "pcb" && <Canvas />}
          {state.tab === "schematic" && <SchematicView />}
          {isFootprint && <FootprintEditorView />}
          {isSymbolEditor && <SymbolEditorView />}
          {is3d && <Viewer3D onReady={setViewer3d} />}
          <Toast />
        </div>
        {!hideBoardChrome && (
          <div className="drawing-toolbar-col">
            <Toolbar id="drawing" />
          </div>
        )}
        {showRightDock && (
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
      <ErcDialog />
      <HotkeysDialog />
      <PreferencesDialog />
      <ZoomAreaOverlay />
      <FootprintPropertiesDialog />
      <NetInspectorDialog />
      <ZoneDialog />
      <TextDialog />
      <ItemPropertiesDialog />
      <MoveExactDialog />
      <PcbParityDialogs />
      <PcbSweepDialogs />
      <VertexEditorPane />
      <PcbPickerPrompt />
      <RouterSettingsDialog />
      <LengthTuningDialog />
      <CleanupTracksDialog />
      <BoardStatisticsDialog />
      <SwapLayersDialog />
      <GlobalEditTracksAndViasDialog />
      <GlobalEditTextAndGraphicsDialog />
      <CreateArrayDialog />
      <DimensionPropertiesDialog />
      <BoardSetupDialog />
      <LabelDialog />
      <SheetDialog />
      <BusUnfoldDialog />
      <PowerSymbolDialog />
      <SchTextDialog />
      <SymbolChooserDialog />
      <SymbolPropertiesDialog />
      <AnnotateDialog />
      <SymbolFieldsTableDialog />
      <FindReplaceDialog />
      <SchematicSetupDialog />
      <PlotDialog />
      <PlotSchematicDialog />
      <ExportNetlistDialog />
      <GenerateDrillDialog />
      <FootprintPositionDialog />
    </div>
  );
}

export default function App() {
  return (
    <StudioProvider>
      <FootprintEditorProvider>
        <SymbolEditorProvider>
          <StudioFrame />
        </SymbolEditorProvider>
      </FootprintEditorProvider>
    </StudioProvider>
  );
}
