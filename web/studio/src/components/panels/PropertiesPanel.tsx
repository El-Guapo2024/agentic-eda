// The Properties panel (docked left -- pcb_edit_frame.cpp's AUI layout): KiCad's property grid (common/widgets/properties_panel.cpp, pcbnew/widgets/
// pcb_properties_panel.cpp, eeschema/widgets/sch_properties_panel.cpp). The grid shows the properties every selected item has, under the groups KiCad
// puts them in, with the value they share or `<...>` where they differ; a row is edited in place and applies to every selected item as ONE undo step
// (kicad-port/{pcbProperties,schItemProperties}.ts register the properties; components/panels/PropertyGrid.tsx draws them).
import { useMemo } from "react";
import { useStudioApi, useStudioDispatch, useStudioState } from "../../state/store";
import { pcbEdit, pcbGrid } from "../../kicad-port/pcbProperties";
import { schEdit, schGrid } from "../../kicad-port/schItemProperties";
import { withVariant } from "../../kicad-port/designVariants";
import type { PropValue } from "../../kicad-port/propertyManager";
import type { Cmd, Rule } from "../../api/types";
import { DockPanel } from "./Dock";
import { PropertyGrid, type CustomPropertyHandlers } from "./PropertyGrid";
import { addCustomPropertyCmd, labelEdited, newCustomPropertyName, removeCustomPropertyCmd, setCustomPropertyCmd } from "../../kicad-port/pcbCustomProperties";
import { useOriginTransforms } from "../../state/originTransforms";
import { stillToPlace } from "../../kicad-port/updatePcb";

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
  // (a part excluded from the board by its symbol or a sheet is no footprint to place: `UpdateNetlist` skips it, UI-LEFT-3 S3-33)
  const unplaced = state.board?.parts.filter(stillToPlace) ?? [];
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
  // The sheet as the current design variant shows it (KiCad 25ec2335 `GetDNPProp` and kin read `Schematic()->GetCurrentVariant()`), and its rows set that variant's values.
  const variant = state.schVariant;
  const sch = useMemo(() => (state.schematic ? withVariant(state.schematic, variant) : null), [state.schematic, variant]);
  const ids = useMemo(() => [...state.selection], [state.selection]);
  // The grid is rebuilt when the sheet, the selection or the units change, not on every move of the cursor (a big selection has a lot of rows to merge).
  const model = useMemo(() => (sch ? schGrid(sch, ids, state.units, variant) : null), [sch, ids, state.units, variant]);
  if (!sch || !model || model.count === 0) {
    return (
      <div className="panel-section">
        <div className="panel-empty">No objects selected</div>
      </div>
    );
  }
  const commit = async (name: string, value: PropValue): Promise<string | null> => {
    const plan = schEdit(sch, ids, name, value, state.units, variant);
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
  // `PCB_BASE_FRAME::GetOriginTransforms()`: the coordinate rows (`PT_COORD` with a coord type) count from Origins & Axes' origin (UI-LEFT-2 PP9).
  const xf = useOriginTransforms();

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

  // KiCad 25ec2335's "Custom Properties" (`PCB_PROPERTIES_PANEL`'s handlers): one `set_custom_property` each, one undo step (kicad-port/pcbCustomProperties.ts).
  const send = async (cmd: Cmd): Promise<string | null> => ((await api.cmd(cmd)) ? null : "The board refused the custom property.");
  const rowNames = model.groups.filter((g) => !g.custom).flatMap((g) => g.rows.map((r) => r.name));
  const custom: CustomPropertyHandlers = {
    add: async () => {
      const name = newCustomPropertyName(board, ids);
      return (await api.cmd(addCustomPropertyCmd(ids, name))) ? name : null;
    },
    commit: (name, text) => send(setCustomPropertyCmd(ids, name, text)),
    label: async (oldName, newName, pending) => {
      const cmd = labelEdited(board, ids, rowNames, oldName, newName, pending);
      return cmd ? send(cmd) : null;
    },
    remove: (name) => send(removeCustomPropertyCmd(ids, name)),
  };

  // The one app-specific section KiCad's grid does not have: the placement rules the intent states for a single footprint.
  const part = ids.length === 1 ? board.parts.find((x) => x.ref === ids[0]) : undefined;
  const rules = part ? board.rules.map((r) => ruleLine(r, part.ref)).filter((l): l is string => !!l) : [];

  return (
    <>
      <PropertyGrid model={model} units={state.units} xf={xf} onCommit={commit} selectionKey={ids.join("\n")} custom={custom} />
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
