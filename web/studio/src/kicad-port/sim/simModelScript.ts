// The Simulation Model dialog's built-in half driven by a script, as a user drives SimModelDialog.tsx: open on a symbol's fields, pick a device
// and a type, type parameter values, tick or untick "Save .. in Value field", press OK -- the same functions the dialog calls
// (simModelDialog.ts), so what it writes is the dialog's. And the SPICE kicad-cli's netlist exporter writes for a symbol's fields
// (`NETLIST_EXPORTER_SPICE::readModel`, `writeModels`, `writeItems`: `ModelName`, `ModelLine`, `ItemLine` of the item kicad-cli builds).
//
// Used by the slow proof `sim_model_api::tests::the_dialogs_built_in_models_are_what_kicad_cli_writes` (through tools/sim-model-proof.mjs),
// which applies the dialog's verbs and holds kicad-cli's netlist to these lines and to the Code tab.
//
// Pure: no React, no DOM.
import { createModelFromFields, referencePrefix, spiceItem, type Fields, type SimModelTables } from "./simModelBuiltin";
import { builtinOk, chooseDevice, chooseType, codeTab, currentBuiltin, openBuiltin, saveInValueLabel, setGridValue } from "./simModelDialog";
import { fieldVerbs, gridModelColumn, modelPinIndex, modelPinString } from "./simModel";
import { removeUserFieldCmd } from "../schFieldEdit";

/** What the user does in the dialog. */
export interface DialogScript {
  /** The symbol as `POST /api/sim/model/symbol` gives it. */
  symbol: { ref: string; value: string; fields: Record<string, string>; own: string[]; pins: { number: string; name: string }[] };
  /** The Device choice's description ("Voltage Source"), when changed. */
  device?: string;
  /** The Device type choice's description ("Sine"), when changed. */
  type?: string;
  /** Parameter values typed in the grid, by name, in this order. */
  params?: [string, string][];
  /** The checkbox, when changed. */
  saveInValue?: boolean;
}

export interface DialogResult {
  /** `m_fields` after OK (Reference and Value among them). */
  fields: Fields;
  /** The Code tab as shown before OK. */
  code: string;
  saveInValueLabel: string;
  /** The verbs OK sends (one batch). */
  cmds: unknown[];
}

/** Open, choose, type, OK. */
export function runDialog(tables: SimModelTables, s: DialogScript): DialogResult {
  const sym = s.symbol;
  const start: Fields = { Reference: sym.ref, Value: sym.value, ...sym.fields };
  const o = openBuiltin(tables, referencePrefix(sym.ref), start, sym.pins);
  // the Pin Assignments grid as it is filled at the start, its rows keeping their model pin indices across models (`updatePinAssignments`)
  let column = gridModelColumn(currentBuiltin(o.builtin).getPins(), sym.pins);
  const rename = () => {
    const pins = currentBuiltin(o.builtin).getPins();
    column = column.map((cell) => (modelPinIndex(cell) >= 0 ? modelPinString(pins, modelPinIndex(cell)) : cell));
  };
  if (s.device !== undefined) {
    chooseDevice(tables, o.builtin, s.device);
    rename();
  }
  if (s.type !== undefined) {
    chooseType(tables, o.builtin, s.type);
    rename();
  }
  const model = currentBuiltin(o.builtin);
  for (const [name, value] of s.params ?? []) {
    const i = model.doFindParam(name);
    if (i < 0) throw new Error(`no parameter ${name} in ${model.type}`);
    setGridValue(model, i, value);
  }
  const label = saveInValueLabel(model);
  const saveInValue = label.enabled && (s.saveInValue ?? o.saveInValue);
  const code = codeTab(model, o.fields, true);
  const fields = builtinOk(model, o.fields, { saveInValue, column, pins: sym.pins });
  // the dialog's verbs (SimModelDialog.tsx `ok`)
  const wanted: Record<string, string> = {};
  const sims = (f: Fields) => Object.keys(f).filter((n) => n.startsWith("Sim."));
  for (const n of new Set([...sims(start), ...sims(fields)])) wanted[n] = fields[n] ?? "";
  wanted.Value = fields.Value ?? start.Value ?? "";
  const { edits, removes } = fieldVerbs(sym.ref, { ...sym.fields, Value: sym.value }, sym.own, wanted);
  const cmds: unknown[] = [];
  if (edits.length) cmds.push({ op: "set_symbol_fields", edits });
  for (const name of removes) cmds.push(removeUserFieldCmd(sym.ref, name));
  return { fields, code, saveInValueLabel: label.label, cmds };
}

/** A symbol's lines in kicad-cli's SPICE netlist: its model's `ModelLine` and `ItemLine`, for the item `readModel` builds (the reference, the
 * symbol's pins by number with their nets, the model name `ModelName` gives). */
export function netlistLines(
  tables: SimModelTables,
  fields: Fields,
  pins: readonly { number: string }[],
  item: { refName: string; pinNumbers: string[]; pinNetNames: string[] }
): { modelLine: string; itemLine: string } {
  const model = createModelFromFields(tables, fields, pins);
  const it = spiceItem({ ...item });
  it.modelName = model.generator.modelName(it);
  return { modelLine: model.generator.modelLine(it), itemLine: model.generator.itemLine(it) };
}
