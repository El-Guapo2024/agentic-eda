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
// **Differs**: the Unit, Body style, Angle and Mirror choices and the four symbol buttons are not offered for a power symbol (it has one unit and body style here,
// and is turned with R / X / Y); its user fields are kept and written at its anchor but not drawn; an alternate assignment of its pin cannot be set.
import { useMemo, useState } from "react";
import type { Cmd, PowerSymbol, SchematicSymbol } from "../api/types";
import { powerEditCmd } from "../kicad-port/schProperties";
import { userFieldNameError } from "../kicad-port/schFieldEdit";
import { fieldLookCmds, fieldLookError, lookOf, pinCell, PIN_COLUMN_LABELS, pinRows, type FieldLook, type PinColumn } from "../kicad-port/symbolPropsDialog";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { SchDialogShell } from "./SchDialogShell";

export function PowerSymbolPropertiesDialog({ id }: { id: string }) {
  const power = useStudioState().schematic?.power_symbols.find((p) => p.id === id);
  return power ? <PowerForm key={id} power={power} /> : null;
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
}

function PowerForm({ power }: { power: PowerSymbol }) {
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
  const [rows, setRows] = useState<UserRow[]>(() => (props.fields ?? []).map((f, i) => ({ key: i, name: f.name, text: f.text, shown: f.visible ?? true })));
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
    for (const f of power.fields ?? []) if (f.id && looks[f.name] && wasLooks[f.name]) cmds.push(...fieldLookCmds(f.id, wasLooks[f.name]!, looks[f.name]!));
    const edit: Record<string, unknown> = {};
    if (footprint !== (props.footprint ?? "")) edit.footprint = footprint;
    if (datasheet !== (props.datasheet ?? "")) edit.datasheet = datasheet;
    for (const [k] of FLAGS) if (flags[k] !== !!props[k]) edit[k] = flags[k];
    const fields = rows.map((r) => ({ name: r.name.trim(), text: r.text, visible: r.shown }));
    const wasFields = (props.fields ?? []).map((f) => ({ name: f.name, text: f.text, visible: f.visible ?? true }));
    if (JSON.stringify(fields) !== JSON.stringify(wasFields)) edit.fields = fields;
    if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "set_power_props", id: power.id, ...edit } as Cmd);
    if (cmds.length === 0) return close();
    void api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds }).then((ok) => (ok ? close() : setError("The changes were not applied.")));
  };
  const showBox = (name: string, checked: boolean, set: (b: boolean) => void) =>
    shownOf(name) === undefined ? <span /> : <input type="checkbox" aria-label={`Show ${name}`} title={`Show the ${name} field on the sheet`} checked={checked} onChange={(e) => set(e.target.checked)} />;
  const lookCells = (name: string) => {
    const l = looks[name];
    if (!l) return <td colSpan={5} />;
    const set = (patch: Partial<FieldLook>) => setLooks((all) => ({ ...all, [name]: { ...l, ...patch } }));
    return (
      <>
        <td>
          <input type="checkbox" aria-label={`${name} italic`} checked={l.italic} onChange={(e) => set({ italic: e.target.checked })} />
        </td>
        <td>
          <input type="checkbox" aria-label={`${name} bold`} checked={l.bold} onChange={(e) => set({ bold: e.target.checked })} />
        </td>
        <td>
          <input aria-label={`${name} text size`} style={{ width: 52 }} type="number" step="0.01" min={0.01} value={l.sizeMm} onChange={(e) => set({ sizeMm: Number(e.target.value) })} />
        </td>
        <td>
          <input aria-label={`${name} font`} style={{ width: 100 }} list="power-props-fonts" value={l.font} onChange={(e) => set({ font: e.target.value })} />
        </td>
        <td>
          <input aria-label={`${name} color`} type="color" value={l.color || "#000000"} onChange={(e) => set({ color: e.target.value })} style={{ width: 28, padding: 0, opacity: l.color ? 1 : 0.35 }} />
        </td>
      </>
    );
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
            <thead>
              <tr>
                {["Name", "Value", "Show", "Italic", "Bold", "Text Size (mm)", "Font", "Color"].map((h) => (
                  <th key={h} style={{ textAlign: "left", fontWeight: 600, padding: "2px 4px" }}>
                    {h}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              <tr>
                <td>Reference</td>
                <td>
                  <input aria-label="Reference" value={power.id} readOnly disabled title="A power symbol's reference is #PWR with a number" style={{ width: "100%" }} />
                </td>
                <td>{showBox("Reference", showReference, setShowReference)}</td>
                {lookCells("Reference")}
              </tr>
              <tr>
                <td>Value</td>
                <td>
                  <input aria-label="Value" autoFocus value={net} onChange={(e) => setNet(e.target.value)} onKeyDown={(e) => e.key === "Enter" && submit()} title="The net this power symbol asserts" style={{ width: "100%" }} />
                </td>
                <td>{showBox("Value", showValue, setShowValue)}</td>
                {lookCells("Value")}
              </tr>
              <tr>
                <td>Footprint</td>
                <td>
                  <input aria-label="Footprint" value={footprint} onChange={(e) => setFootprint(e.target.value)} style={{ width: "100%" }} />
                </td>
                <td colSpan={6} />
              </tr>
              <tr>
                <td>Datasheet</td>
                <td>
                  <input aria-label="Datasheet" value={datasheet} onChange={(e) => setDatasheet(e.target.value)} style={{ width: "100%" }} />
                </td>
                <td colSpan={6} />
              </tr>
              {rows.map((r) => (
                <tr key={r.key}>
                  <td>
                    <input aria-label="Field name" value={r.name} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, name: e.target.value } : o)))} style={{ width: 100 }} />
                  </td>
                  <td>
                    <div style={{ display: "flex", gap: 4 }}>
                      <input aria-label={`${r.name} value`} value={r.text} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, text: e.target.value } : o)))} style={{ flex: 1, minWidth: 0 }} />
                      <button type="button" aria-label={`Remove ${r.name}`} title="Delete field" onClick={() => setRows((all) => all.filter((o) => o !== r))}>
                        ×
                      </button>
                    </div>
                  </td>
                  <td>
                    <input type="checkbox" aria-label={`Show ${r.name}`} checked={r.shown} onChange={(e) => setRows((all) => all.map((o) => (o === r ? { ...o, shown: e.target.checked } : o)))} />
                  </td>
                  <td colSpan={5} />
                </tr>
              ))}
            </tbody>
          </table>
          <p style={{ margin: "6px 0" }}>
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
          </p>
          <fieldset style={{ display: "flex", flexDirection: "column", gap: 2 }} data-testid="power-attributes">
            <legend>Attributes</legend>
            {FLAGS.map(([k, label]) => (
              <label key={k} className="toggle">
                <input type="checkbox" checked={flags[k]} onChange={(e) => setFlags({ ...flags, [k]: e.target.checked })} data-testid={`power-attr-${k}`} /> {label}
              </label>
            ))}
          </fieldset>
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
