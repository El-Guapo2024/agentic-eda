// The Symbol Editor tab's frame (eeschema/symbol_editor/symbol_edit_frame.cpp): the top toolbar (`SYMBOL_EDIT_TOOLBAR_SETTINGS`, TOP_MAIN), then the
// Libraries tree (`SYMBOL_TREE_PANE`, far left) with the Properties pane under it (`SCH_PROPERTIES_PANEL`), the left (options) toolbar, the canvas and the right (drawing) toolbar. The toolbars are the extracted KiCad
// ones (sym_toolbars.json, drawn by components/Toolbar.tsx with KiCad's icons); the canvas is always there -- with nothing open it is the empty canvas and
// its grid, like KiCad's, and a symbol is opened from the tree. The studio's own "Update Symbol on Board" and "Show Pin Numbers" are in the menus
// (kicad/menuExtras.ts).
import { useEffect } from "react";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { useLibraryWatch } from "../../state/libraryWatch";
import { SymbolEditorCanvas } from "./SymbolEditorCanvas";
import { PinPropertiesDialog } from "./PinPropertiesDialog";
import { SymbolShapePropertiesDialog } from "./SymbolShapePropertiesDialog";
import { SymbolTextBoxDialog } from "./SymbolTextBoxDialog";
import { PinTableDialog } from "./PinTableDialog";
import { SymbolFindDialog } from "./SymbolFindDialog";
import { LibrarySymbolPropertiesDialog } from "./SymbolPropertiesDialog";
import { SymbolLibraryPanel } from "./SymbolLibraryPanel";
import { SaveSymbolAsDialog } from "./SaveSymbolAsDialog";
import { ImportSymbolDialog } from "./ImportSymbolDialog";
import { LibraryFieldsTableDialog } from "./LibraryFieldsTableDialog";
import { SymbolTextDialog } from "./SymbolTextDialog";
import { DeriveSymbolDialog } from "./DeriveSymbolDialog";
import { NewSymbolDialog } from "./NewSymbolDialog";
import { UpdateSymbolFieldsDialog } from "./UpdateSymbolFieldsDialog";
import { DerivedSymbolBanner } from "./DerivedSymbolBanner";
import { LibraryDialogHost } from "../library/libraryDialogs";
import { DockColumn, DockPanel } from "../panels/Dock";
import { SymbolPropertiesPane } from "./SymbolPropertiesPane";
import { Toolbar } from "../Toolbar";

export function SymbolEditorView() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  // `setSymWatcher( &libId )`: the open symbol's library file is watched while the editor is shown, and offered for reloading when it changes (UI-LEFT-2 LL3)
  useLibraryWatch(
    "symbol",
    state.libId,
    useStudioState().tab === "symbol",
    async (gone) => (gone ? api.closeSymbol() : api.refresh()),
    (message, kind = "info") => dispatch({ type: "TOAST", message, kind })
  );

  // The editor's own toast (every library action reports through it) goes away by itself, like the studio's (App.tsx's `Toast`).
  useEffect(() => {
    if (!state.toast) return;
    const t = setTimeout(() => dispatch({ type: "TOAST_CLEAR" }), state.toast.kind === "info" ? 3500 : 7000);
    return () => clearTimeout(t);
  }, [state.toast, dispatch]);

  return (
    <div className="editor-frame footprint-editor-view">
      <div className="main-toolbar-row">
        <Toolbar id="main" editor="symbol" />
      </div>
      <div className="editor-frame-body">
        {/* SYMBOL_TREE_PANE: the project's own symbols and KiCad's installed libraries; folds to a handle (Show Library Tree). */}
        <DockColumn side="left" column="tree" label="Libraries">
          {/* the Libraries tree and, below it, the frame's Properties pane (`defaultPropertiesPaneInfo`, shown by default; Show Properties toggles it) */}
          <div style={{ display: "flex", flexDirection: "column", minHeight: 0, height: "100%" }}>
            <div style={{ display: "flex", flex: "1 1 0", minHeight: 120 }}>
              <SymbolLibraryPanel />
            </div>
            <div style={{ flex: "0 1 auto", maxHeight: "55%", overflowY: "auto", width: 236, background: "var(--chrome-bg-raised)", borderRight: "1px solid var(--chrome-border)" }}>
              <DockPanel id="symProperties" title="Properties">
                <SymbolPropertiesPane />
              </DockPanel>
            </div>
          </div>
        </DockColumn>
        <Toolbar id="options" editor="symbol" />
        <div className="editor-canvas-col">
          <SymbolEditorCanvas />
          <DerivedSymbolBanner />
        </div>
        <Toolbar id="drawing" editor="symbol" />
      </div>
      <PinPropertiesDialog />
      <SymbolShapePropertiesDialog />
      <SymbolTextBoxDialog />
      <PinTableDialog />
      <SymbolFindDialog />
      <LibrarySymbolPropertiesDialog />
      <SaveSymbolAsDialog />
      <ImportSymbolDialog />
      <LibraryFieldsTableDialog />
      <SymbolTextDialog />
      <DeriveSymbolDialog />
      <NewSymbolDialog />
      <UpdateSymbolFieldsDialog />
      <LibraryDialogHost />
      {state.toast && (
        <div
          role="status"
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
            zIndex: 3000,
          }}
        >
          {state.toast.message}
        </div>
      )}
    </div>
  );
}
