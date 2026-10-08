// The Properties panel (docked left -- per the task's reading of
// pcb_edit_frame.cpp's AUI layout). KiCad builds this as a live property
// grid (common/widgets/properties_panel.cpp, pcbnew/widgets/
// pcb_properties_panel.cpp) that this session could not read; this is a
// reasonable approximation (a key/value grid plus the same rotate/rip
// actions the message panel and canvas expose) rather than a
// transcription of KiCad's exact grid rows.
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { formatXY } from "../../state/units";
import type { Rule } from "../../api/types";
import { DockPanel } from "./Dock";

/**
 * KiCad picks up an unplaced footprint through the Add Footprint tool's
 * modal chooser (pcbnew's own dialog, triggered from a toolbar action
 * this app hasn't wired -- its real action name is one more thing this
 * session couldn't read out of pcb_actions.cpp). Until that's ported,
 * placing a part works the way the old studio.html did it: pick a part
 * here, then click the board -- same underlying `place_at` command
 * either way, just a plain list instead of a modal.
 */
function UnplacedList() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const unplaced = state.board?.parts.filter((p) => !p.placed) ?? [];
  if (unplaced.length === 0) return null;
  return (
    <div className="panel-section">
      <h3>Unplaced ({unplaced.length})</h3>
      {unplaced.map((p) => (
        <div key={p.ref} className={`unplaced-row${state.armed === p.ref ? " armed" : ""}`} onClick={() => dispatch({ type: "SET_ARMED", ref: state.armed === p.ref ? null : p.ref })}>
          <b>{p.ref}</b> {p.value ?? ""}
          <small>
            {p.package ?? ""}
            {p.block ? ` · ${p.block}` : ""}
          </small>
        </div>
      ))}
      {state.armed && <div className="panel-empty">Click the board to place {state.armed}.</div>}
    </div>
  );
}

function ruleLine(rule: Rule, ref: string): string | null {
  switch (rule.kind) {
    case "proximity":
      if (rule.a !== ref && rule.b !== ref) return null;
      return `within ${rule.max_mm} mm of ${rule.a === ref ? rule.b : rule.a}`;
    case "separation":
      if (rule.a !== ref && rule.b !== ref) return null;
      return `at least ${rule.min_mm} mm from ${rule.a === ref ? rule.b : rule.a}`;
    case "keepout":
      return rule.refs.includes(ref) ? `keepout "${rule.zone}"` : null;
    case "thermal_group":
      return rule.refs.includes(ref) ? "thermal group" : null;
  }
}

/**
 * The schematic tab's own properties view: PCB and schematic share the
 * same reference-designator identity and the same state.selection (so
 * clicking U1 in either view cross-probes to the other -- see
 * SchematicView.tsx), but a schematic symbol has pins/nets, not a PCB
 * position/side/courtyard, so it gets its own small render rather than
 * pretending a symbol is a placed PCB part.
 */
export function SchematicProperties() {
  const state = useStudioState();
  const sch = state.schematic;
  const refs = [...state.selection];
  if (!sch || refs.length === 0) {
    return (
      <div className="panel-section">
        <div className="panel-empty">Nothing selected.</div>
      </div>
    );
  }
  const sym = sch.symbols.find((s) => s.id === refs[0]);
  if (!sym) return null;
  const nets = [...new Set(sch.wires.filter((w) => w.pins.some((p) => p.startsWith(`${sym.id}.`))).map((w) => w.net))];
  return (
    <div className="panel-section">
      <div className="kv-grid">
        <span>Reference</span>
        <span>{sym.id}</span>
        <span>Value</span>
        <span>{sym.value ?? "–"}</span>
        {sym.package && (
          <>
            <span>Footprint</span>
            <span>{sym.package}</span>
          </>
        )}
        {sym.mpn && (
          <>
            <span>MPN</span>
            <span>{sym.mpn}</span>
          </>
        )}
        <span>Pins</span>
        <span>{sym.pins.length}</span>
        <span>Nets</span>
        <span>{nets.join(", ") || "–"}</span>
      </div>
    </div>
  );
}

/** The board editor's Properties pane, docked left (`PCB_PROPERTIES_PANEL`); the schematic's is `SchematicDock`'s. */
export function PropertiesPanel() {
  return (
    <DockPanel id="properties" title="Properties">
      <PropertiesBody />
    </DockPanel>
  );
}

function PropertiesBody() {
  const state = useStudioState();
  const api = useStudioApi();
  const board = state.board;
  const refs = [...state.selection];

  if (!board || refs.length === 0) {
    return (
      <>
        <div className="panel-section">
          <div className="panel-empty">Nothing selected.</div>
        </div>
        <UnplacedList />
      </>
    );
  }

  if (refs.length > 1) {
    return (
      <div className="panel-section">
        <div className="kv-grid">
          <span>Selected</span>
          <span>{refs.length} parts</span>
        </div>
        <div className="row" style={{ marginTop: 8, display: "flex", gap: 6 }}>
          <button onClick={() => api.rotateSelection(1)}>Rotate (R)</button>
          <button onClick={() => api.ripSelection()}>Delete (Del)</button>
        </div>
      </div>
    );
  }

  const ref = refs[0]!;
  const p = board.parts.find((x) => x.ref === ref);
  if (!p) return null;
  const nets = [...new Set((p.pads ?? []).map((q) => q.net).filter((n): n is string => !!n))];
  const rules = board.rules.map((r) => ruleLine(r, ref)).filter((l): l is string => !!l);

  return (
    <div className="panel-section">
      <div className="kv-grid">
        <span>Reference</span>
        <span>{p.ref}</span>
        <span>Value</span>
        <span>{p.value ?? "–"}</span>
        <span>Footprint</span>
        <span>{p.package ?? "–"}</span>
        {p.mpn && (
          <>
            <span>MPN</span>
            <span>{p.mpn}</span>
          </>
        )}
        {p.block && (
          <>
            <span>Block</span>
            <span>{p.block}</span>
          </>
        )}
        {p.placed && p.at && (
          <>
            <span>Position</span>
            <span>{formatXY(p.at[0], p.at[1], state.units)}</span>
            <span>Orientation</span>
            <span>{`${p.rot ?? 0}°`}</span>
            <span>Side</span>
            <span>{p.side === "bottom" ? "Bottom" : "Top"}</span>
          </>
        )}
        <span>Nets</span>
        <span>{nets.join(", ") || "–"}</span>
      </div>
      {rules.length > 0 && (
        <div style={{ marginTop: 8 }}>
          <h3>Placement rules</h3>
          {rules.map((line, i) => (
            <div key={i} style={{ fontSize: 11 }}>
              {line}
            </div>
          ))}
        </div>
      )}
      {p.placed && (
        <div style={{ marginTop: 10, display: "flex", gap: 6 }}>
          <button onClick={() => api.rotateSelection(1)}>Rotate (R)</button>
          <button onClick={() => api.ripSelection()}>Delete (Del)</button>
        </div>
      )}
    </div>
  );
}
