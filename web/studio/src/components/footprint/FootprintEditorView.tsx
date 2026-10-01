// The Footprint Editor tab's chrome (GAPS.md #8): a small, hand-built
// toolbar (this editor's own tool set is a fraction of the PCB tab's --
// see Toolbar.tsx's own doc on why that one is data-driven from an
// extracted KiCad toolbar/action catalog this editor has no equivalent
// extraction for) plus the canvas and this editor's own dialogs.
import { useEffect, useState } from "react";
import { useFpApi, useFpDispatch, useFpState, FP_TOOL_MESSAGES, type FpToolId } from "../../state/footprintEditorStore";
import { fetchFootprintLibraryNames } from "../../api/client";
import { FootprintCanvas } from "./FootprintCanvas";
import { PadPropertiesDialog } from "./PadPropertiesDialog";
import { FootprintLibraryPropertiesDialog } from "./FootprintPropertiesDialog";

const TOOL_BUTTONS: { id: FpToolId; label: string }[] = [
  { id: "select", label: "Select" },
  { id: "move", label: "Move" },
  { id: "pad", label: "Pad" },
  { id: "draw_segment", label: "Line" },
  { id: "draw_arc", label: "Arc" },
  { id: "draw_rect", label: "Rect" },
  { id: "draw_circle", label: "Circle" },
  { id: "draw_polygon", label: "Polygon" },
  { id: "text", label: "Text" },
];

const GRAPHIC_LAYERS = ["F.SilkS", "F.Fab", "F.CrtYd"];

/** The "Open from Library" picker -- `GET /api/footprint_library`'s name list plus a free-text "or type a new name" field (opening a never-seen name just starts a blank footprint, see `Cmd::OpenFootprintForEdit`'s own doc). */
function OpenFootprintPicker() {
  const api = useFpApi();
  const [names, setNames] = useState<string[]>([]);
  const [text, setText] = useState("");

  useEffect(() => {
    fetchFootprintLibraryNames()
      .then((r) => setNames(r.names))
      .catch(() => setNames([]));
  }, []);

  const open = (name: string) => {
    if (name.trim()) void api.openFootprint(name.trim());
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10, padding: 24 }}>
      <p style={{ color: "var(--chrome-text-dim)" }}>Open a footprint from the project library, or type a new name to start one from scratch.</p>
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

function RenumberPadsDialog() {
  const state = useFpState();
  const dispatch = useFpDispatch();
  const api = useFpApi();
  const [start, setStart] = useState(1);
  const [prefix, setPrefix] = useState("");
  const [step, setStep] = useState(1);
  if (!state.renumberDialogOpen) return null;
  const close = () => dispatch({ type: "SET_RENUMBER_DIALOG_OPEN", open: false });
  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 340 }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span>Renumber Pads</span>
        </div>
        <div className="dialog-body">
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>Orders pads top-to-bottom, then left-to-right (reading order) -- see PARITY-fpedit.md for how this differs from KiCad's click-sequence tool.</p>
          <div className="kv-grid" style={{ gridTemplateColumns: "100px 1fr" }}>
            <span>Prefix</span>
            <input value={prefix} onChange={(e) => setPrefix(e.target.value)} style={{ width: 80 }} />
            <span>Start at</span>
            <input type="number" value={start} onChange={(e) => setStart(Math.max(0, Math.round(Number(e.target.value) || 0)))} style={{ width: 80 }} />
            <span>Step</span>
            <input type="number" min={1} value={step} onChange={(e) => setStep(Math.max(1, Math.round(Number(e.target.value) || 1)))} style={{ width: 80 }} />
          </div>
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button
            className="primary"
            onClick={() => {
              void api.renumberPads(start, prefix, step);
              close();
            }}
          >
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
  const api = useFpApi();

  return (
    <div className="footprint-editor-view" style={{ display: "flex", flexDirection: "column", width: "100%", height: "100%" }}>
      <div className="toolbar" data-toolbar="footprint">
        {TOOL_BUTTONS.map((t) => (
          <button key={t.id} className="toolbar-button" title={FP_TOOL_MESSAGES[t.id]} aria-pressed={state.activeTool === t.id} style={state.activeTool === t.id ? { outline: "1px solid var(--chrome-accent, #4aa3ff)" } : undefined} onClick={() => dispatch({ type: "SET_ACTIVE_TOOL", tool: t.id })} disabled={!state.footprint}>
            {t.label}
          </button>
        ))}
        <div className="toolbar-separator" role="separator" />
        <div className="toolbar-control" title="Layer new graphics/text are drawn on">
          <select value={state.activeLayer} onChange={(e) => dispatch({ type: "SET_ACTIVE_LAYER", layer: e.target.value })}>
            {GRAPHIC_LAYERS.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
        </div>
        <div className="toolbar-separator" role="separator" />
        <button className="toolbar-button" onClick={() => void api.undo()} disabled={!state.footprint}>
          Undo
        </button>
        <button className="toolbar-button" onClick={() => void api.redo()} disabled={!state.footprint}>
          Redo
        </button>
        <div className="toolbar-separator" role="separator" />
        <button className="toolbar-button" onClick={() => dispatch({ type: "SET_RENUMBER_DIALOG_OPEN", open: true })} disabled={!state.footprint || (state.footprint?.pads.length ?? 0) === 0}>
          Renumber Pads
        </button>
        <button className="toolbar-button" onClick={() => dispatch({ type: "SET_FOOTPRINT_PROPERTIES_OPEN", open: true })} disabled={!state.footprint}>
          Properties
        </button>
        <button className="toolbar-button" onClick={() => void api.updateOnBoard()} disabled={!state.footprint} title="Push this library definition to every board instance naming it (GAPS.md #8's explicit Update Footprint from Library)">
          Update on Board
        </button>
        <button className="toolbar-button" onClick={() => void api.exportKicadMod()} disabled={!state.footprint} title="Save a derived, standalone .kicad_mod for this footprint">
          Export .kicad_mod
        </button>
        <div style={{ flex: 1 }} />
        <span style={{ padding: "4px 10px", color: "var(--chrome-text-dim)" }}>{state.name ?? "(no footprint open)"}</span>
        <button className="toolbar-button" onClick={() => api.closeFootprint()} disabled={!state.name}>
          Open...
        </button>
      </div>
      <div style={{ flex: 1, position: "relative", display: "flex", minHeight: 0 }}>
        {state.name ? <FootprintCanvas /> : <OpenFootprintPicker />}
      </div>
      <PadPropertiesDialog />
      <FootprintLibraryPropertiesDialog />
      <RenumberPadsDialog />
      {state.toast && <div style={{ position: "absolute", left: "50%", bottom: 16, transform: "translateX(-50%)", background: "#16263a", border: "1px solid #4aa3ff", color: "#d6e8ff", padding: "8px 12px", borderRadius: 8 }}>{state.toast.message}</div>}
    </div>
  );
}
