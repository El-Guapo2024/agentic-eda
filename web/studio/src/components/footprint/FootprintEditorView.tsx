// The Footprint Editor tab's frame (pcbnew/footprint_edit_frame.cpp): the top toolbar (`FOOTPRINT_EDIT_TOOLBAR_SETTINGS`, TOP_MAIN), then the Libraries
// tree (`FOOTPRINT_TREE_PANE`, far left), the left (options) toolbar, the canvas and the right (drawing) toolbar. The toolbars are the extracted KiCad ones
// (fp_toolbars.json, drawn by components/Toolbar.tsx with KiCad's icons); the canvas is always there -- with nothing open it is the empty canvas and its
// grid, like KiCad's, and a footprint is opened from the tree.
import { useEffect, useRef, useState } from "react";
import { useFpDispatch, useFpState } from "../../state/footprintEditorStore";
import { ENUM_PREFIX_MAX_LENGTH, enumSpinValue, enumeratePopupText, enumerateStart } from "../../kicad-port/padEnumeration";
import { inPadEditMode, PAD_EDIT_MODE_MESSAGE, padEditLayer } from "../../kicad-port/padEditMode";
import { FootprintCanvas } from "./FootprintCanvas";
import { PadPropertiesDialog } from "./PadPropertiesDialog";
import { FootprintLibraryPropertiesDialog } from "./FootprintPropertiesDialog";
import { FootprintLibraryPanel } from "./FootprintLibraryPanel";
import { PadTableDialog } from "./PadTableDialog";
import { PushPadPropertiesDialog } from "./PushPadPropertiesDialog";
import { LoadFromBoardDialog } from "./LoadFromBoardDialog";
import { FootprintCheckerDialog } from "./FootprintCheckerDialog";
import { FootprintCreateArrayDialog } from "./FootprintArrayDialog";
import { FpTextDialog } from "./FpTextDialog";
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
  // `dialog_enum_pads_base.cpp`: the title, the note, the three labels, the prefix limited to 4 characters and the spin controls' 0..999.
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 360 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Pad Enumeration Settings" data-testid="enum-pads-dialog">
        <div className="dialog-header">
          <span>Pad Enumeration Settings</span>
        </div>
        <div className="dialog-body">
          <p style={{ margin: "0 0 10px" }}>Pad names are restricted to 4 characters (including number).</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "130px 1fr" }}>
            <span>Pad name prefix:</span>
            <input value={prefix} autoFocus maxLength={ENUM_PREFIX_MAX_LENGTH} data-testid="enum-pads-prefix" onChange={(e) => setPrefix(e.target.value.slice(0, ENUM_PREFIX_MAX_LENGTH))} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
            <span>First pad number:</span>
            <input type="number" min={0} max={999} step={1} value={start} data-testid="enum-pads-start" onChange={(e) => setStart(enumSpinValue(Number(e.target.value)))} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
            <span>Numbering step:</span>
            <input type="number" min={0} max={999} step={1} value={step} data-testid="enum-pads-step" onChange={(e) => setStep(enumSpinValue(Number(e.target.value)))} onKeyDown={(e) => e.key === "Enter" && ok()} style={{ width: 80 }} />
          </div>
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

  // The pad edit mode is the footprint's own (`LibraryFootprint.pad_edit`, the pad's `ENTERED` flag): whenever it begins or ends -- Ctrl+E, an undo, a redo, a
  // footprint opened in the mode -- the editor follows, as `PAD_TOOL::OnUndoRedo` does by looking for the flagged pad.
  const editing = inPadEditMode(state.footprint);
  const wasEditing = useRef(false);
  const editedPad = state.footprint?.pads.find((p) => p.id === state.footprint?.pad_edit?.pad);
  useEffect(() => {
    if (editing && !wasEditing.current && editedPad) dispatch({ type: "PAD_EDIT_ENTER", layer: padEditLayer(editedPad) });
    if (!editing && wasEditing.current) dispatch({ type: "PAD_EDIT_EXIT" });
    wasEditing.current = editing;
  }, [editing, editedPad, dispatch]);

  return (
    <div className="editor-frame footprint-editor-view">
      <div className="main-toolbar-row">
        <Toolbar id="main" editor="footprint" />
      </div>
      {editing && (
        <div role="status" data-testid="pad-edit-banner" style={{ padding: "4px 12px", background: "var(--chrome-bg-raised)", borderBottom: "1px solid var(--chrome-border)", fontSize: 12 }}>
          {PAD_EDIT_MODE_MESSAGE}
        </div>
      )}
      {state.enumerate && (
        <div role="status" style={{ padding: "4px 12px", background: "var(--chrome-bg-raised)", borderBottom: "1px solid var(--chrome-border)", fontSize: 12, whiteSpace: "pre-line" }}>
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
      <FpTextDialog />
      <PadTableDialog />
      <PushPadPropertiesDialog />
      <LoadFromBoardDialog />
      <FootprintCheckerDialog />
      <FootprintCreateArrayDialog />
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
