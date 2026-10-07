// The Symbol Editor tab's chrome (eeschema's Symbol Editor) -- a small,
// hand-built toolbar (this editor's own tool set has no extracted KiCad
// toolbar/action catalog of its own, same reasoning
// `components/footprint/FootprintEditorView.tsx`'s own doc gives) plus
// the library tree, the canvas and this editor's own dialogs. The toolbar's
// groups follow `SYMBOL_EDIT_TOOLBAR_SETTINGS::DefaultToolbarConfig`: the
// drawing tools of the right toolbar, the view toggles of the left one
// (Show Pin Electrical Types, Show Hidden Pins) and the top toolbar's
// Synchronized Pins Mode and Add Symbol to Schematic.
import { useEffect, useState } from "react";
import { useSymApi, useSymDispatch, useSymState, SYM_TOOL_MESSAGES, type SymToolId } from "../../state/symbolEditorStore";
import { fetchSymbolEditorNames } from "../../api/client";
import { useActionRunner } from "../../actions/useActionRunner";
import { SymbolEditorCanvas } from "./SymbolEditorCanvas";
import { PinPropertiesDialog } from "./PinPropertiesDialog";
import { PinTableDialog } from "./PinTableDialog";
import { LibrarySymbolPropertiesDialog } from "./SymbolPropertiesDialog";
import { SymbolLibraryPanel } from "./SymbolLibraryPanel";
import { SaveSymbolAsDialog } from "./SaveSymbolAsDialog";
import { ImportSymbolDialog } from "./ImportSymbolDialog";
import { LibraryFieldsTableDialog } from "./LibraryFieldsTableDialog";
import { SymbolTextDialog } from "./SymbolTextDialog";
import { LibraryDialogHost } from "../library/libraryDialogs";

const TOOL_BUTTONS: { id: SymToolId; label: string }[] = [
  { id: "select", label: "Select" },
  { id: "move", label: "Move" },
  { id: "pin", label: "Pin" },
  { id: "draw_segment", label: "Line" },
  { id: "draw_lines", label: "Lines" },
  { id: "draw_arc", label: "Arc" },
  { id: "draw_rect", label: "Rect" },
  { id: "draw_circle", label: "Circle" },
  { id: "draw_polygon", label: "Polygon" },
  { id: "text", label: "Text" },
  { id: "anchor", label: "Anchor" },
];

/** The "Open from Library" picker -- `GET /api/symbol_editor/names`'s name list plus a free-text "or type a new name" field (opening a never-seen lib_id just starts a blank symbol, see `Cmd::OpenSymbolForEdit`'s own doc). */
function OpenSymbolPicker() {
  const api = useSymApi();
  const [names, setNames] = useState<string[]>([]);
  const [text, setText] = useState("");

  useEffect(() => {
    fetchSymbolEditorNames()
      .then((r) => setNames(r.names))
      .catch(() => setNames([]));
  }, []);

  const open = (libId: string) => {
    if (libId.trim()) void api.openSymbol(libId.trim());
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10, padding: 24 }}>
      <p style={{ color: "var(--chrome-text-dim)" }}>Open a symbol from the project library, or type a new lib_id to start one from scratch.</p>
      <div style={{ display: "flex", gap: 6 }}>
        <input value={text} placeholder="Lib:Name or a new name" onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && open(text)} style={{ width: 260 }} />
        <button className="primary" onClick={() => open(text)} disabled={!text.trim()}>
          Open
        </button>
      </div>
      {names.length > 0 && (
        <div style={{ maxHeight: 220, overflowY: "auto", width: 320, border: "1px solid var(--chrome-border, #333)" }}>
          {names.map((n) => (
            <div key={n} className="toolbar-button" style={{ display: "block", textAlign: "left", width: "100%", padding: "4px 8px", cursor: "pointer" }} onClick={() => open(n)}>
              {n}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const PRESSED = { outline: "1px solid var(--chrome-accent, #4aa3ff)" } as const;

export function SymbolEditorView() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const { run } = useActionRunner();
  const sym = state.symbol;
  const unitOptions = Array.from({ length: Math.max(1, sym?.unit_count ?? 1) }, (_, i) => i + 1);
  const multiUnit = (sym?.unit_count ?? 1) > 1;

  // The editor's own toast (every library action reports through it) goes away by itself, like the studio's (App.tsx's `Toast`).
  useEffect(() => {
    if (!state.toast) return;
    const t = setTimeout(() => dispatch({ type: "TOAST_CLEAR" }), state.toast.kind === "info" ? 3500 : 7000);
    return () => clearTimeout(t);
  }, [state.toast, dispatch]);

  return (
    <div className="footprint-editor-view" style={{ display: "flex", flexDirection: "column", width: "100%", height: "100%" }}>
      <div className="toolbar" data-toolbar="symbol">
        {TOOL_BUTTONS.map((t) => (
          <button key={t.id} className="toolbar-button" title={SYM_TOOL_MESSAGES[t.id]} aria-pressed={state.activeTool === t.id} style={state.activeTool === t.id ? PRESSED : undefined} onClick={() => dispatch({ type: "SET_ACTIVE_TOOL", tool: t.id })} disabled={!sym}>
            {t.label}
          </button>
        ))}
        <div className="toolbar-separator" role="separator" />
        <div className="toolbar-control" title="Which unit new pins/graphics are placed on">
          <span style={{ marginRight: 4, color: "var(--chrome-text-dim)" }}>Unit</span>
          <select value={state.activeUnit} onChange={(e) => dispatch({ type: "SET_ACTIVE_UNIT", unit: Number(e.target.value) })} disabled={!sym}>
            {unitOptions.map((u) => (
              <option key={u} value={u}>
                {u}
              </option>
            ))}
          </select>
        </div>
        {sym?.has_alternate_body_style && (
          <div className="toolbar-control" title="Which body style (DeMorgan) new pins/graphics are placed on">
            <select value={state.activeBodyStyle} onChange={(e) => dispatch({ type: "SET_ACTIVE_BODY_STYLE", style: Number(e.target.value) })}>
              <option value={1}>Standard</option>
              <option value={2}>Alternate</option>
            </select>
          </div>
        )}
        <div className="toolbar-separator" role="separator" />
        <button className="toolbar-button" aria-pressed={state.syncPins && multiUnit} style={state.syncPins && multiUnit ? PRESSED : undefined} onClick={() => run("eeschema.SymbolLibraryControl.toggleSyncedPinsMode")} disabled={!sym || !multiUnit} title="Synchronized Pins Mode: pins that sit on top of each other in different units are edited together">
          Sync Pins
        </button>
        <button className="toolbar-button" aria-pressed={state.showElectricalTypes} style={state.showElectricalTypes ? PRESSED : undefined} onClick={() => run("eeschema.SymbolLibraryControl.showElectricalTypes")} disabled={!sym} title="Show Pin Electrical Types">
          Elec. Types
        </button>
        <button className="toolbar-button" aria-pressed={state.showHiddenPins} style={state.showHiddenPins ? PRESSED : undefined} onClick={() => run("eeschema.SymbolLibraryControl.showHiddenPins")} disabled={!sym} title="Show Hidden Pins">
          Hidden Pins
        </button>
        <button className="toolbar-button" aria-pressed={state.showPinNumbers} style={state.showPinNumbers ? PRESSED : undefined} onClick={() => run("eeschema.SymbolLibraryControl.showPinNumbers")} disabled={!sym} title="Show Pin Numbers: force the numbers on, even where the symbol hides them">
          Pin Numbers
        </button>
        <div className="toolbar-separator" role="separator" />
        <button className="toolbar-button" onClick={() => void api.undo()} disabled={!sym}>
          Undo
        </button>
        <button className="toolbar-button" onClick={() => void api.redo()} disabled={!sym}>
          Redo
        </button>
        <div className="toolbar-separator" role="separator" />
        <button className="toolbar-button" onClick={() => dispatch({ type: "SET_PIN_TABLE_OPEN", open: true })} disabled={!sym}>
          Pin Table
        </button>
        <button className="toolbar-button" onClick={() => dispatch({ type: "SET_PROPERTIES_OPEN", open: true })} disabled={!sym}>
          Properties
        </button>
        <button className="toolbar-button" onClick={() => run("eeschema.SymbolLibraryControl.addSymbolToSchematic")} disabled={!sym} title="Add Symbol to Schematic: the open symbol, at the unit being edited, becomes the symbol being placed">
          Add to Schematic
        </button>
        <button className="toolbar-button" onClick={() => void api.updateOnBoard()} disabled={!sym} title="Push this library definition to every placed instance naming it">
          Update Symbol on Board
        </button>
        <button className="toolbar-button" onClick={() => void api.exportKicadSym()} disabled={!sym} title="Save a derived, standalone .kicad_sym for this symbol">
          Export .kicad_sym
        </button>
        <div style={{ flex: 1 }} />
        <span style={{ padding: "4px 10px", color: "var(--chrome-text-dim)" }}>{state.libId ?? "(no symbol open)"}</span>
        <button className="toolbar-button" onClick={() => api.closeSymbol()} disabled={!state.libId}>
          Open...
        </button>
      </div>
      <div style={{ flex: 1, position: "relative", display: "flex", minHeight: 0 }}>
        <SymbolLibraryPanel />
        <div style={{ flex: 1, position: "relative", display: "flex", minHeight: 0, minWidth: 0 }}>{state.libId ? <SymbolEditorCanvas /> : <OpenSymbolPicker />}</div>
      </div>
      <PinPropertiesDialog />
      <PinTableDialog />
      <LibrarySymbolPropertiesDialog />
      <SaveSymbolAsDialog />
      <ImportSymbolDialog />
      <LibraryFieldsTableDialog />
      <SymbolTextDialog />
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
