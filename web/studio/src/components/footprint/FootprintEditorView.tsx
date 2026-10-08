// The Footprint Editor tab's frame (pcbnew/footprint_edit_frame.cpp): the top toolbar (`FOOTPRINT_EDIT_TOOLBAR_SETTINGS`, TOP_MAIN), then the Libraries
// tree (`FOOTPRINT_TREE_PANE`, far left), the left (options) toolbar, the canvas and the right (drawing) toolbar. The toolbars are the extracted KiCad ones
// (fp_toolbars.json, drawn by components/Toolbar.tsx with KiCad's icons); the canvas is always there -- with nothing open it is the empty canvas and its
// grid, like KiCad's, and a footprint is opened from the tree.
import { useEffect, useState } from "react";
import { useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { enumeratePopupText, enumerateStart } from "../../kicad-port/padEnumeration";
import { FootprintCanvas } from "./FootprintCanvas";
import { PadPropertiesDialog } from "./PadPropertiesDialog";
import { FootprintLibraryPropertiesDialog } from "./FootprintPropertiesDialog";
import { FootprintLibraryPanel } from "./FootprintLibraryPanel";
import { PadTableDialog } from "./PadTableDialog";
import { PushPadPropertiesDialog } from "./PushPadPropertiesDialog";
import { LoadFromBoardDialog } from "./LoadFromBoardDialog";
import { LibraryDialogHost } from "../library/libraryDialogs";
import { DockColumn } from "../panels/Dock";
import { Toolbar } from "../Toolbar";

/**
 * `DIALOG_ENUM_PADS` (pcbnew/dialogs/dialog_enum_pads.cpp): the prefix, start number and step of `pcbnew.PadTool.enumeratePads`. OK arms the click-to-number
 * tool (`EnumeratePads` -> the click loop, `padEnumeration.ts`); the parameters are remembered for the next run (`s_lastUsedParams`).
 */
function RenumberPadsDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const [prefix, setPrefix] = useState("");
  const [start, setStart] = useState(1);
  const [step, setStep] = useState(1);
  useEffect(() => {
    if (state.renumberDialogOpen) {
      setPrefix(state.enumerateParams.prefix);
      setStart(state.enumerateParams.start);
      setStep(state.enumerateParams.step);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.renumberDialogOpen]);
  if (!state.renumberDialogOpen) return null;
  const close = () => dispatch({ type: "SET_RENUMBER_DIALOG_OPEN", open: false });
  const ok = () => {
    const params = { prefix, start, step };
    dispatch({ type: "SET_ENUMERATE_PARAMS", params });
    close();
    dispatch({ type: "CLEAR_SELECTION" }); // `ACTIONS::selectionClear`
    dispatch({ type: "SET_ENUMERATE", state: enumerateStart(params) });
    dispatch({ type: "SET_ACTIVE_TOOL", tool: "enumerate" });
  };
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Renumber Pads">
        <div className="dialog-header">
          <span>Renumber Pads</span>
        </div>
        <div className="dialog-body">
          <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
            <span>Prefix</span>
            <input value={prefix} autoFocus onChange={(e) => setPrefix(e.target.value)} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
            <span>Start number</span>
            <input type="number" value={start} onChange={(e) => setStart(Math.round(Number(e.target.value) || 0))} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
            <span>Number step</span>
            <input type="number" value={step} onChange={(e) => setStep(Math.round(Number(e.target.value) || 0))} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
          </div>
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, margin: "10px 0 0" }}>Then click the pads in the order they should be numbered.</p>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={ok}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

export function FootprintEditorView() {
  const state = useFpState();
  const dispatch = useFpDispatch();

  // The editor's own toast (every library action reports through it) goes away by itself, like the studio's (App.tsx's `Toast`).
  useEffect(() => {
    if (!state.toast) return;
    const t = setTimeout(() => dispatch({ type: "TOAST_CLEAR" }), state.toast.kind === "info" ? 3500 : 7000);
    return () => clearTimeout(t);
  }, [state.toast, dispatch]);

  return (
    <div className="editor-frame footprint-editor-view">
      <div className="main-toolbar-row">
        <Toolbar id="main" editor="footprint" />
      </div>
      {state.enumerate && (
        <div role="status" style={{ padding: "4px 12px", background: "var(--chrome-bg-raised)", borderBottom: "1px solid var(--chrome-border)", fontSize: 12 }}>
          {enumeratePopupText(state.enumerate)}
        </div>
      )}
      <div className="editor-frame-body">
        {/* FOOTPRINT_TREE_PANE: the project's own footprints and KiCad's installed libraries; folds to a handle (Show Library Tree). */}
        <DockColumn side="left" column="tree" label="Libraries">
          <FootprintLibraryPanel />
        </DockColumn>
        <Toolbar id="options" editor="footprint" />
        <div className="editor-canvas-col">
          <FootprintCanvas />
        </div>
        <Toolbar id="drawing" editor="footprint" />
      </div>
      <PadPropertiesDialog />
      <FootprintLibraryPropertiesDialog />
      <RenumberPadsDialog />
      <PadTableDialog />
      <PushPadPropertiesDialog />
      <LoadFromBoardDialog />
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
