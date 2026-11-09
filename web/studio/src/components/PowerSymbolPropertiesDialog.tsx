// Symbol Properties of a power symbol (`E`, a double-click; `SCH_EDIT_TOOL::Properties` opens `DIALOG_SYMBOL_PROPERTIES` for it, as it does for every `SCH_SYMBOL_T`, with the
// Spice model button hidden because the library symbol is a power one: eeschema/dialogs/dialog_symbol_properties.cpp at 8303b2ad). A power symbol's Reference is `#PWR..`,
// its Value the net it asserts.
//
// The General tab has the Fields grid -- the Reference (as it is), the Value (renaming the net of this symbol), the Footprint, the Datasheet and the user-defined fields
// (Add Field makes one, the x removes it), the Show checks of the Reference and Value, their look columns (Italic, Bold, Text Size, Font, Color) -- and the Attributes
// box's five flags; the Pin Functions tab lists the pin (a power pin has no alternates in KiCad's libraries; a pin that has some is listed read-only here). The library
// link is at the foot. OK is one undo step: the Value and Show checks (kicad-port/schProperties.ts `powerEditCmd`), the looks (`edit_field`, `set_field_looks`), the
// rest one `set_power_props` (`SchExtras::power_props`); nothing is sent when nothing changed.
//
// The grid has the symbol dialog's columns (kicad-port/symbolFieldsGrid.ts: Name .. Bold shown at open, the others from the column labels' right-click menu), with
// Move Up / Move Down over the user fields; the General box has Unit and Body style (disabled: a power symbol has one of each here) and the Angle and Mirror choices
// (`set_symbol_orientation`). The user fields are items of the sheet, drawn where Autoplace Fields or the file puts them (`eda_engine::fields::power_placements`);
// the step ends as `SCH_EDIT_TOOL::Properties` does after OK (`autoplace_fields_kept`) (UI-LEFT-2 SS7).
//
// **Differs**: the four symbol buttons are not offered for a power symbol; an alternate assignment of its pin cannot be set; a power symbol has no mirror flag here, so
// a mirror that leaves its picture mirrored is kept as a turn, and the dialog reads it back as that angle, "Not mirrored"; Angle and Mirror are not offered for the power
// symbol in the hand (R, X and Y turn it); the Footprint and Datasheet rows have no look columns (they are written hidden at the anchor).
import { schematicWithCarry, useSchCarry } from "../state/schCarryStore";
import { editCarried } from "../actions/schPlaceActions";
import { useMemo, useState } from "react";
import { ANGLE_CHOICES, autoplaceKeptCmd, canMoveDown, canMoveUp, columnMenu, DEFAULT_SHOWN_COLUMNS, FIELD_COLUMNS, MIRROR_CHOICES, moveRow, orientationChoice, orientationCmd, shownColumns, toggleColumn, type FieldColumn } from "../kicad-port/symbolFieldsGrid";
import { ContextMenu } from "./canvas/ContextMenu";
import { lookCellOf } from "./fieldGridCells";
import type { Cmd, PowerSymbol, SchematicSymbol } from "../api/types";
import { powerEditCmd } from "../kicad-port/schProperties";
import { userFieldNameError } from "../kicad-port/schFieldEdit";
import { fieldLookCmds, fieldLookError, lookOf, pinCell, PIN_COLUMN_LABELS, pinRows, type FieldLook, type PinColumn } from "../kicad-port/symbolPropsDialog";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

/** `carried`: the power symbol Place Power holds (E while it rides): read off the page's drawing of it, and OK changes the power symbol in the hand. */
export function PowerSymbolPropertiesDialog({ id, carried }: { id: string; carried?: boolean }) {
  const base = useStudioState().schematic;
  const carry = useSchCarry();
  const sch = carried && base && carry ? schematicWithCarry(base, carry) : base;
  const power = sch?.power_symbols.find((p) => p.id === id);
  return power ? <PowerForm key={id} power={power} carried={!!carried} /> : null;
}

const FLAGS: ReadonlyArray<["exclude_from_sim" | "exclude_from_board" | "dnp" | "exclude_from_bom" | "exclude_from_pos_files", string]> = [
  ["exclude_from_sim", "Exclude from simulation"],
  ["exclude_from_board", "Exclude from board"],
  ["dnp", "Do not populate"],
  ["exclude_from_bom", "Exclude from bill of materials"],
  ["exclude_from_pos_files", "Exclude from position files"],
];

interface UserRow {
  key: number;
  name: string;
  text: string;
  shown: boolean;
  /** The name the power symbol had the field by (its look and place cells are that field's); absent for a field added here. */
  was?: string;
}

function PowerForm({ power, carried }: { power: PowerSymbol; carried: boolean }) {
  const api = useStudioApi();
  const dispatch = useStudioDispatch();
  const sch = useStudioState().schematic;
  const close = () => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: null });
  const shownOf = (name: string) => power.fields?.find((f) => f.name === name)?.visible;
  const props = power.props ?? {};
  const [net, setNet] = useState(power.net);
  const [showReference, setShowReference] = useState(shownOf("Reference") ?? false);
  const [showValue, setShowValue] = useState(shownOf("Value") ?? true);
  const [footprint, setFootprint] = useState(props.footprint ?? "");
  const [datasheet, setDatasheet] = useState(props.datasheet ?? "");
  const [flags, setFlags] = useState(() => Object.fromEntries(FLAGS.map(([k]) => [k, !!props[k]])) as Record<(typeof FLAGS)[number][0], boolean>);
  const [rows, setRows] = useState<UserRow[]>(() => (props.fields ?? []).map((f, i) => ({ key: i, name: f.name, text: f.text, shown: f.visible ?? true, was: f.name })));
  // the grid's shown columns and their menu, the selected row (`main:<name>`, `user:<key>`), the Angle and Mirror choices
  const [shownCols, setShownCols] = useState<Set<FieldColumn>>(() => shownColumns(DEFAULT_SHOWN_COLUMNS));
  const [columnsMenu, setColumnsMenu] = useState<{ x: number; y: number } | null>(null);
  const [selectedRow, setSelectedRow] = useState<string | null>(null);
  const wasOrient = orientationChoice(power.rot, null);
  const [orient, setOrient] = useState(wasOrient);
  const [nextKey, setNextKey] = useState((props.fields ?? []).length);
  const wasLooks = useMemo(() => Object.fromEntries((power.fields ?? []).filter((f) => f.id).map((f) => [f.name, lookOf(f)])), [power.fields]);
  const [looks, setLooks] = useState<Record<string, FieldLook>>(wasLooks);
  const [tab, setTab] = useState<"general" | "pins">("general");
  const [error, setError] = useState<string | null>(null);
  // the Pin Functions tab: the power symbol's pins as a placed symbol's (`GetRawPins`)
  const pins = useMemo(
    () => (sch ? pinRows(sch, { id: power.id, lib_id: power.lib_id, at: power.at, rot: power.rot, mirror: null, unit: 1, body_style: 1, value: power.net, mpn: null, package: null, footprint: null, datasheet: null, pins: [] } as unknown as SchematicSymbol) : []),
    [sch, power]
  );
  // a Value cannot be emptied (`FIELD_VALIDATOR`): a power symbol needs the net it asserts
  const canOk = net.trim().length > 0;
  const submit = () => {
    if (!canOk) return;
    for (const [name, l] of Object.entries(looks)) {
      const why = fieldLookError(name, l);
      if (why) return setError(why);
    }
    for (const [i, r] of rows.entries()) {
      const why = userFieldNameError(r.name, ["Reference", "Value", "Footprint", "Datasheet", ...rows.filter((_, j) => j !== i).map((o) => o.name)]);
      if (why) return setError(why);
    }
    const cmds: Cmd[] = [];
    const main = powerEditCmd(power, { net, showReference, showValue });
    if (main) cmds.push(...(main.op === "batch" ? main.cmds : [main]));
    // the look and place columns of the fields it keeps (Symbol Properties' edits: the step ends with `autoplace_fields_kept`)
    const keptNames = new Set(["Reference", "Value", ...rows.map((r) => r.was).filter((n): n is string => !!n)]);
    for (const f of power.fields ?? []) if (f.id && keptNames.has(f.name) && looks[f.name] && wasLooks[f.name]) cmds.push(...fieldLookCmds(f.id, wasLooks[f.name]!, looks[f.name]!, true));
    const edit: Record<string, unknown> = {};
    if (footprint !== (props.footprint ?? "")) edit.footprint = footprint;
    if (datasheet !== (props.datasheet ?? "")) edit.datasheet = datasheet;
    for (const [k] of FLAGS) if (flags[k] !== !!props[k]) edit[k] = flags[k];
    const fields = rows.map((r) => ({ name: r.name.trim(), text: r.text, visible: r.shown }));
    const wasFields = (props.fields ?? []).map((f) => ({ name: f.name, text: f.text, visible: f.visible ?? true }));
    if (JSON.stringify(fields) !== JSON.stringify(wasFields)) edit.fields = fields;
    if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "set_power_props", id: power.id, ...edit } as Cmd);
    // Angle and Mirror (`SetOrientation`), not for the power symbol in the hand
    const turn = carried ? null : orientationCmd(power.id, undefined, wasOrient, orient);
    if (turn) cmds.push(turn);
    if (cmds.length === 0) return close();
    // `SCH_EDIT_TOOL::Properties` after OK: `AutoplaceFields( screen, fieldsAutoplaced )`
    cmds.push(autoplaceKeptCmd(power.id));
    // the power symbol in the hand takes the changes (actions/schPlaceActions.ts `editCarried`)
    if (carried) {
      editCarried(cmds);
      return close();
    }
    void api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }).then((ok) => (ok ? close() : setError("The changes were not applied.")));
  };
  const showBox = (name: string, checked: boolean, set: (b: boolean) => void) =>
    shownOf(name) === undefined ? <span /> : <input type="checkbox" aria-label={`Show ${name}`} title={`Show the ${name} field on the sheet`} checked={checked} onChange={(e) => set(e.target.checked)} />;
  /** The look and place cell of `col` for the field the power symbol has as `name` (components/fieldGridCells.tsx); none for a field it has not yet. */
  const lookCell = (name: string | undefined, col: FieldColumn): React.ReactNode => {
    const l = name ? looks[name] : undefined;
    if (!name || !l) return null;
    return lookCellOf(name, col, l, (patch) => setLooks((all) => ({ ...all, [name]: { ...all[name]!, ...patch } })), "power-props-fonts");
  };
  const gridCols = FIELD_COLUMNS.filter((c) => shownCols.has(c.key));
  const MANDATORY = 4;
  const rowIds = ["main:Reference", "main:Value", "main:Footprint", "main:Datasheet", ...rows.map((r) => `user:${r.key}`)];
  const selectedIndex = selectedRow ? rowIds.indexOf(selectedRow) : -1;
  const gridRow = (rowId: string, lookName: string | undefined, own: Partial<Record<FieldColumn, React.ReactNode>>) => (
    <tr key={rowId} data-row={rowId} aria-selected={selectedRow === rowId} onMouseDown={() => setSelectedRow(rowId)} style={selectedRow === rowId ? { background: "var(--chrome-selection, rgba(74, 144, 226, 0.18))" } : undefined}>
      {gridCols.map((c) => (
        <td key={c.key}>{own[c.key] !== undefined ? own[c.key] : lookCell(lookName, c.key)}</td>
      ))}
    </tr>
  );
  /** `OnMoveUp` / `OnMoveDown`: a user field swaps with its neighbour (the list `set_power_props` keeps, in its order). */
  const moveSelected = (up: boolean) => {
    if (selectedIndex < MANDATORY) return;
    setRows((all) => moveRow(all, selectedIndex - MANDATORY, up));
  };
  return (
    <SchDialogShell title="Symbol Properties" width={720} onCancel={close} onOk={submit} okLabel="OK" canOk={canOk}>
      <datalist id="power-props-fonts">
        <option value="Default Font" />
        <option value="KiCad Font" />
      </datalist>
      <div style={{ display: "flex", gap: 2, marginBottom: 6 }} role="tablist">
        <button role="tab" aria-selected={tab === "general"} className={tab === "general" ? "primary" : undefined} onClick={() => setTab("general")}>
          General
        </button>
        <button role="tab" aria-selected={tab === "pins"} className={tab === "pins" ? "primary" : undefined} onClick={() => setTab("pins")} data-testid="power-tab-pins">
          Pin Functions
        </button>
      </div>
      {tab === "general" && (
        <>
          <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }} data-testid="power-fields-grid">
            <thead
              onContextMenu={(e) => {
                e.preventDefault();
                setColumnsMenu({ x: e.clientX, y: e.clientY });
              }}
              title="Right-click to show or hide columns"
            >
              <tr>
                {gridCols.map((c) => (
                  <th key={c.key} data-col={c.key} style={{ textAlign: "left", fontWeight: 600, padding: "2px 4px", whiteSpace: "nowrap" }}>
                    {c.label}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {gridRow("main:Reference", "Reference", {
                name: "Reference",
                value: <input aria-label="Reference" value={power.id} readOnly disabled title="A power symbol's reference is #PWR with a number" style={{ width: "100%" }} />,
                shown: showBox("Reference", showReference, setShowReference),
              })}
              {gridRow("main:Value", "Value", {
                name: "Value",
                value: <input aria-label="Value" autoFocus value={net} onChange={(e) => setNet(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="The net this power symbol asserts" style={{ width: "100%" }} />,
                shown: showBox("Value", showValue, setShowValue),
              })}
              {gridRow("main:Footprint", undefined, { name: "Footprint", value: <input aria-label="Footprint" value={footprint} onChange={(e) => setFootprint(e.target.value)} style={{ width: "100%" }} /> })}
              {gridRow("main:Datasheet", undefined, { name: "Datasheet", value: <input aria-label="Datasheet" value={datasheet} onChange={(e) => setDatasheet(e.target.value)} style={{ width: "100%" }} /> })}
              {rows.map((r) =>
                gridRow(`user:${r.key}`, r.was, {
                  name: <input aria-label="Field name" value={r.name} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, name: e.target.value } : o)))} style={{ width: 100 }} />,
                  value: (
                    <div style={{ display: "flex", gap: 4 }}>
                      <input aria-label={`${r.name} value`} value={r.text} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, text: e.target.value } : o)))} style={{ flex: 1, minWidth: 0 }} />
                      <button type="button" aria-label={`Remove ${r.name}`} title="Delete field" onClick={() => setRows((all) => all.filter((o) => o !== r))}>
                        ×
                      </button>
                    </div>
                  ),
                  shown: <input type="checkbox" aria-label={`Show ${r.name}`} checked={r.shown} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, shown: e.target.checked } : o)))} />,
                })
              )}
            </tbody>
          </table>
          {columnsMenu && (
            <ContextMenu
              x={columnsMenu.x}
              y={columnsMenu.y}
              entries={columnMenu(shownCols).map((c) => ({ label: `${c.checked ? "\u2713" : "\u2003"} ${c.label}`, checked: c.checked, onSelect: () => setShownCols((cols) => toggleColumn(cols, c.key)) }))}
              onClose={() => setColumnsMenu(null)}
            />
          )}
          <p style={{ margin: "6px 0", display: "flex", gap: 4 }}>
            <button
              type="button"
              title="Add field"
              onClick={() => {
                // `OnAddField`: `GetUserFieldName( m_fields->size(), DO_TRANSLATE )` -- "Field%d" -- hidden
                const taken = new Set(["Reference", "Value", "Footprint", "Datasheet", ...rows.map((r) => r.name)]);
                let n = 4 + rows.length;
                while (taken.has(`Field${n}`)) n++;
                setRows((all) => [...all, { key: nextKey, name: `Field${n}`, text: "", shown: false }]);
                setNextKey(nextKey + 1);
              }}
            >
              Add Field
            </button>
            <button type="button" title="Move up" aria-label="Move up" disabled={selectedIndex < 0 || !canMoveUp(selectedIndex, MANDATORY)} onClick={() => moveSelected(true)} data-testid="power-field-up">
              ▲
            </button>
            <button type="button" title="Move down" aria-label="Move down" disabled={selectedIndex < 0 || !canMoveDown(selectedIndex, rowIds.length, MANDATORY)} onClick={() => moveSelected(false)} data-testid="power-field-down">
              ▼
            </button>
          </p>
          <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
            {/* `sbGeneralProps`: Unit and Body style (one each here: disabled, as KiCad's are for such a symbol), Angle and Mirror */}
            <fieldset style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 8px", alignItems: "center", minWidth: 220 }} data-testid="power-general">
              <legend>General</legend>
              <span style={{ opacity: 0.5 }}>Unit:</span>
              <select aria-label="Unit" disabled data-testid="power-unit">
                <option>{""}</option>
              </select>
              <span style={{ opacity: 0.5 }}>Body style:</span>
              <select aria-label="Body style" disabled data-testid="power-body-style">
                <option>{""}</option>
              </select>
              <span style={{ opacity: carried ? 0.5 : 1 }}>Angle:</span>
              <select aria-label="Angle" value={orient.angle} disabled={carried} title={carried ? "Turn the power symbol in the hand with R, X and Y." : undefined} onChange={(e) => setOrient({ ...orient, angle: Number(e.target.value) })} data-testid="power-angle">
                {ANGLE_CHOICES.map((a) => (
                  <option key={a.angle} value={a.angle}>
                    {a.label}
                  </option>
                ))}
              </select>
              <span style={{ opacity: carried ? 0.5 : 1 }}>Mirror:</span>
              <select aria-label="Mirror" value={orient.mirror ?? ""} disabled={carried} title={carried ? "Turn the power symbol in the hand with R, X and Y." : undefined} onChange={(e) => setOrient({ ...orient, mirror: (e.target.value || null) as "x" | "y" | null })} data-testid="power-mirror">
                {MIRROR_CHOICES.map((m) => (
                  <option key={m.label} value={m.mirror ?? ""}>
                    {m.label}
                  </option>
                ))}
              </select>
            </fieldset>
          <fieldset style={{ display: "flex", flexDirection: "column", gap: 2 }} data-testid="power-attributes">
            <legend>Attributes</legend>
            {FLAGS.map(([k, label]) => (
              <label key={k} className="toggle">
                <input type="checkbox" checked={flags[k]} onChange={(e) => setFlags({ ...flags, [k]: e.target.checked })} data-testid={`power-attr-${k}`} /> {label}
              </label>
            ))}
          </fieldset>
          </div>
        </>
      )}
      {tab === "pins" && (
        <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }} data-testid="power-pin-grid">
          <thead>
            <tr>
              {(Object.keys(PIN_COLUMN_LABELS) as PinColumn[]).map((c) => (
                <th key={c} style={{ textAlign: "left", fontWeight: 600, padding: "2px 6px" }}>
                  {PIN_COLUMN_LABELS[c]}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pins.map((r, i) => (
              <tr key={`${r.number}:${i}`}>
                {(Object.keys(PIN_COLUMN_LABELS) as PinColumn[]).map((c) => (
                  <td key={c} style={{ padding: "1px 6px" }}>
                    {pinCell(r, c)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {error && (
        <p style={{ color: "var(--chrome-danger)", fontSize: 11 }} data-testid="power-properties-error">
          {error}
        </p>
      )}
      <p style={{ fontSize: 11, opacity: 0.75, margin: "8px 0 0" }}>
        Library link: <span data-testid="power-library-link">{power.lib_id}</span>
      </p>
    </SchDialogShell>
  );
}
