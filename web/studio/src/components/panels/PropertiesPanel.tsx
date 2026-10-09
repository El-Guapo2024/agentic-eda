// The Properties panel (docked left -- pcb_edit_frame.cpp's AUI layout): KiCad's property grid (common/widgets/properties_panel.cpp, pcbnew/widgets/
// pcb_properties_panel.cpp, eeschema/widgets/sch_properties_panel.cpp). The grid shows the properties every selected item has, under the groups KiCad
// puts them in, with the value they share or `<...>` where they differ; a row is edited in place and applies to every selected item as ONE undo step
// (kicad-port/{pcbProperties,schItemProperties}.ts register the properties; components/panels/PropertyGrid.tsx draws them).
import { useMemo } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { pcbEdit, pcbGrid } from "../../kicad-port/pcbProperties";
import { schEdit, schGrid } from "../../kicad-port/schItemProperties";
import type { PropValue } from "../../kicad-port/propertyManager";
import type { Rule } from "../../api/types";
import { DockPanel } from "./Dock";
import { PropertyGrid } from "./PropertyGrid";

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
 * The schematic tab's Properties pane (`SCH_PROPERTIES_PANEL`): the grid of the selected items. PCB and schematic share the reference-designator identity and
 * `state.selection` (so clicking U1 in either view cross-probes to the other -- see SchematicView.tsx), but the schematic has its own item classes and registrations.
 */
export function SchematicProperties() {
  const state = useStudioState();
  const api = useStudioApi();
  const sch = state.schematic;
  const ids = useMemo(() => [...state.selection], [state.selection]);
  // The grid is rebuilt when the sheet, the selection or the units change, not on every move of the cursor (a big selection has a lot of rows to merge).
  const model = useMemo(() => (sch ? schGrid(sch, ids, state.units) : null), [sch, ids, state.units]);
  if (!sch || !model || model.count === 0) {
    return (
      <div className="panel-section">
        <div className="panel-empty">No objects selected</div>
      </div>
    );
  }
  const commit = async (name: string, value: PropValue): Promise<string | null> => {
    const plan = schEdit(sch, ids, name, value, state.units);
    if (!plan.ok) return plan.error;
    if (plan.cmds.length > 0) await api.cmdBatch(plan.cmds);
    return null;
  };
  return <PropertyGrid model={model} units={state.units} onCommit={commit} selectionKey={ids.join("\n")} />;
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
  const ids = useMemo(() => [...state.selection], [state.selection]);
  const model = useMemo(() => (board ? pcbGrid(board, ids, state.units) : null), [board, ids, state.units]);

  if (!board || !model || model.count === 0) {
    return (
      <>
        <div className="panel-section">
          <div className="panel-empty">No objects selected</div>
        </div>
        <UnplacedList />
      </>
    );
  }

  const commit = async (name: string, value: PropValue): Promise<string | null> => {
    const plan = pcbEdit(board, ids, name, value, state.units);
    if (!plan.ok) return plan.error;
    // One edit, one undo step: every command of the edit goes in one batch (`BOARD_COMMIT::Push( "Edit Properties" )`).
    if (plan.cmds.length > 0) await api.cmdBatch(plan.cmds);
    return null;
  };

  // The one app-specific section KiCad's grid does not have: the placement rules the intent states for a single footprint.
  const part = ids.length === 1 ? board.parts.find((x) => x.ref === ids[0]) : undefined;
  const rules = part ? board.rules.map((r) => ruleLine(r, part.ref)).filter((l): l is string => !!l) : [];

  return (
    <>
      <PropertyGrid model={model} units={state.units} onCommit={commit} selectionKey={ids.join("\n")} />
      {rules.length > 0 && (
        <div className="panel-section">
          <h3>Placement rules</h3>
          {rules.map((line, i) => (
            <div key={i} style={{ fontSize: 11 }}>
              {line}
            </div>
          ))}
        </div>
      )}
    </>
  );
}
