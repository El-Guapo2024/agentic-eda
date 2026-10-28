// The Simulation Model dialog's library part (`eeschema/dialogs/dialog_sim_model.cpp`, `DIALOG_SIM_MODEL`), pure: a library model's pins and
// which symbol pin each is on, the Pin Assignments grid, and the fields OK writes.
//
// The model's pins (`SIM_MODEL::m_modelPins`, `SIM_MODEL_PIN { modelPinName, symbolPinNumber }`):
// - a subcircuit's are its ports, the n-th on symbol pin number "n" (`SIM_MODEL_SUBCKT::SetBaseModel` copies the base model's pins, made by
//   `SPICE_MODEL_PARSER_SUBCKT::ReadModel` as `AddPin( { name, fmt::format( "{}", GetPinCount() + 1 ) } )`);
// - a built-in device's are its pin names, the n-th on the n-th symbol pin by number (`SIM_MODEL::createPins`), an optional one ("<S>") left out
//   when the symbol has no pin for it;
// - the model the symbol names already (its `Sim.Library` and `Sim.Name`) then takes the symbol's `Sim.Pins` (`SIM_MODEL_SERIALIZER::ParsePins`,
//   `AssignSymbolPinNumberToModelPin` by name, else by a 1-based index: "Unknown simulation model pin '%s'").
// The grid (`updatePinAssignments`, `getSymbolPinString`, `getModelPinString`, `onPinAssignmentsGridCellChange`) and OK
// (`TransferDataFromWindow`: `removeOrphanedPinAssignments`, `WriteFields`, `GeneratePins`) work on that list.
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import { strNumCmp } from "../schSheetPins";

/** `SIM_MODEL_PIN`. */
export interface ModelPin {
  modelPinName: string;
  symbolPinNumber: string;
}

/** A symbol pin as the grid lists it (`m_sortedPartPins`). */
export interface SymbolPin {
  number: string;
  name: string;
}

/** One model of a library as `POST /api/sim/model/library` sends it (`crates/cli/src/sim_model_api.rs`). */
export interface LibraryModelInfo {
  name: string;
  kind: "subckt" | "model";
  /** `Sim.Device` / `Sim.Type` the model writes (`GenerateDevice`, `GenerateDeviceSubtype`). */
  device: string;
  type: string;
  /** The model's pins in order: a subcircuit's ports, or a device's pin names ("<S>" optional); none for a raw SPICE element. */
  pins: string[];
  code: string;
}

/** `_( "Not Connected" )`. */
export const NOT_CONNECTED = "Not Connected";

/** The fields this dialog reads and writes (`SIM_LIBRARY::LIBRARY_FIELD`, `NAME_FIELD`, `SIM_DEVICE_FIELD`, `SIM_DEVICE_SUBTYPE_FIELD`, `SIM_PINS_FIELD`,
 * `SIM_PARAMS_FIELD`, `SIM_LEGACY_ENABLE_FIELD_V7`). */
export const SIM_FIELDS = { library: "Sim.Library", name: "Sim.Name", device: "Sim.Device", type: "Sim.Type", pins: "Sim.Pins", params: "Sim.Params", enable: "Sim.Enable" } as const;

/** The model's pins before the symbol's `Sim.Pins` is read. */
export function defaultModelPins(model: LibraryModelInfo, sortedSymbolPins: readonly SymbolPin[]): ModelPin[] {
  if (model.kind === "subckt") return model.pins.map((p, i) => ({ modelPinName: p, symbolPinNumber: String(i + 1) }));
  // `createPins`: model pin i on symbol pin i; past the symbol's pins a required pin stays unconnected and an optional one is left out
  const out: ModelPin[] = [];
  model.pins.forEach((name, i) => {
    const optional = name.startsWith("<") && name.endsWith(">");
    if (i < sortedSymbolPins.length) out.push({ modelPinName: name, symbolPinNumber: sortedSymbolPins[i]!.number });
    else if (!optional) out.push({ modelPinName: name, symbolPinNumber: "" });
  });
  return out;
}

/** `SIM_MODEL_SERIALIZER::ParsePins`: "1=in+ 2=in-" assigns each named (or 1-based numbered) model pin to the symbol pin before it. Throws KiCad's
 * "Unknown simulation model pin '%s'". */
export function parsePins(pins: ModelPin[], field: string): ModelPin[] {
  const out = pins.map((p) => ({ ...p }));
  for (const pair of field.trim().split(/\s+/).filter(Boolean)) {
    const eq = pair.indexOf("=");
    if (eq < 0) continue;
    const symbolPin = pair.slice(0, eq);
    const modelPin = pair.slice(eq + 1);
    const byName = out.find((p) => p.modelPinName === modelPin);
    if (byName) {
      byName.symbolPinNumber = symbolPin;
      continue;
    }
    const index = parseInt(modelPin, 10);
    if (!(index >= 1 && index <= out.length)) throw new Error(`Unknown simulation model pin '${modelPin}'`);
    out[index - 1]!.symbolPinNumber = symbolPin;
  }
  return out;
}

/** `getSymbolPinString`: `1 ("IN+")`, or the number alone when the name is empty or the number. */
export function symbolPinString(pin: SymbolPin): string {
  return pin.name && pin.name !== pin.number ? `${pin.number} ("${pin.name}")` : pin.number;
}

/** `getModelPinString`: `1 ("inp")` -- the 1-based index, and the name unless it is the index. */
export function modelPinString(pins: readonly ModelPin[], index: number): string {
  const name = index >= 0 && index < pins.length ? pins[index]!.modelPinName : "";
  const n = String(index + 1);
  return name && name !== n ? `${n} ("${name}")` : n;
}

/** `getModelPinIndex`: the index a model column string names, -1 for "Not Connected". */
export function modelPinIndex(s: string): number {
  if (s === NOT_CONNECTED) return -1;
  const head = s.split(" ")[0] ?? "";
  const n = parseInt(head, 10);
  return Number.isFinite(n) ? n - 1 : -1;
}

/** `updatePinAssignments( model, true )`: the Model Pin column, row by symbol pin. */
export function gridModelColumn(pins: readonly ModelPin[], sortedSymbolPins: readonly SymbolPin[]): string[] {
  const col = sortedSymbolPins.map(() => NOT_CONNECTED);
  pins.forEach((p, i) => {
    if (p.symbolPinNumber === "") return;
    const row = sortedSymbolPins.findIndex((s) => s.number === p.symbolPinNumber);
    if (row >= 0) col[row] = modelPinString(pins, i);
  });
  return col;
}

/** The Model Pin cell's choices: each model pin, then "Not Connected". */
export function modelPinChoices(pins: readonly ModelPin[]): string[] {
  return [...pins.map((_, i) => modelPinString(pins, i)), NOT_CONNECTED];
}

/** `onPinAssignmentsGridCellChange`: the old model pin of the row is let go, the new one put on the row's symbol pin. */
export function assignRow(pins: readonly ModelPin[], sortedSymbolPins: readonly SymbolPin[], row: number, oldValue: string, newValue: string): ModelPin[] {
  const out = pins.map((p) => ({ ...p }));
  const oldIndex = modelPinIndex(oldValue);
  const newIndex = modelPinIndex(newValue);
  if (oldIndex >= 0 && oldIndex < out.length) out[oldIndex]!.symbolPinNumber = "";
  if (newIndex >= 0 && newIndex < out.length) out[newIndex]!.symbolPinNumber = sortedSymbolPins[row]?.number ?? "";
  return out;
}

/** `TransferDataFromWindow`'s loop over the grid, then `removeOrphanedPinAssignments`: a model pin on a symbol pin the symbol does not have is let go. */
export function pinsOnOk(pins: readonly ModelPin[], sortedSymbolPins: readonly SymbolPin[], column: readonly string[]): ModelPin[] {
  const out = pins.map((p) => ({ ...p }));
  column.forEach((cell, row) => {
    const i = modelPinIndex(cell);
    if (i >= 0 && i < out.length) out[i]!.symbolPinNumber = sortedSymbolPins[row]!.number;
  });
  for (const p of out) if (!sortedSymbolPins.some((s) => s.number === p.symbolPinNumber)) p.symbolPinNumber = "";
  return out;
}

/** `SIM_MODEL_SERIALIZER::GeneratePins`: "symbolPin=modelPin" pairs sorted by the symbol pin (`StrNumCmp`, without case), the unconnected left out. */
export function generatePins(pins: readonly ModelPin[]): string {
  return [...pins]
    .filter((p) => p.symbolPinNumber !== "")
    .sort((a, b) => strNumCmp(a.symbolPinNumber, b.symbolPinNumber, true))
    .map((p) => `${p.symbolPinNumber}=${p.modelPinName}`)
    .join(" ");
}

/** The model the dialog opens on (`TransferDataToWindow` -> `loadLibrary`): the one `Sim.Name` names, else the first of the sorted list, with the
 * info bar's message when the name is not in the library ("No model named '%s' in '%s'."). */
export function initialModel(names: readonly string[], simName: string, libraryPath: string): { name: string | null; message: string | null } {
  if (names.includes(simName)) return { name: simName, message: null };
  const sorted = [...names].sort();
  return { name: sorted[0] ?? null, message: simName ? `No model named '${simName}' in '${libraryPath}'.` : null };
}

/** `onModelFilter`: `wxString::Matches( "*" + filter + "*" )` (`*` and `?` wildcards, case as typed), the names sorted. */
export function filterModelNames(names: readonly string[], filter: string): string[] {
  const re = new RegExp(`^${("*" + filter + "*").replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")}$`, "s");
  return names.filter((n) => re.test(n)).sort();
}

/** What OK writes (`TransferDataFromWindow`: `SetFieldValue` of `Sim.Library` and `Sim.Name`, then `WriteFields`): each field's new text, an empty
 * text being a field to remove (`SetFieldValue( .., "" )` erases it). `Sim.Params` keeps the instance's overrides of the model it had (`CreateModel`
 * with the fields) and has none for another model; `Sim.Enable` (KiCad 7's) is written empty. The Value field is not touched. */
export function fieldsOnOk(current: Record<string, string>, libraryPath: string, model: LibraryModelInfo, pins: readonly ModelPin[]): Record<string, string> {
  const sameModel = current[SIM_FIELDS.library] === libraryPath && current[SIM_FIELDS.name] === model.name;
  return {
    [SIM_FIELDS.library]: libraryPath,
    [SIM_FIELDS.name]: model.name,
    [SIM_FIELDS.device]: model.device,
    [SIM_FIELDS.type]: model.type,
    [SIM_FIELDS.enable]: "",
    [SIM_FIELDS.pins]: generatePins(pins),
    [SIM_FIELDS.params]: sameModel ? (current[SIM_FIELDS.params] ?? "") : "",
  };
}

/** The verbs of OK, one batch (one undo step): a text that changes is set through the Symbol Fields Table's verb (`set_symbol_fields`, which reaches
 * every sheet, a sheet read from a KiCad file among them, and makes a missing field hidden); a field emptied is removed when it is the instance's own
 * (`remove_user_field`), and set empty when it is the library symbol's (so the instance's empty field stands for it). */
export function fieldVerbs(ref: string, current: Record<string, string>, own: readonly string[], wanted: Record<string, string>): { edits: { id: string; field: string; value: string }[]; removes: string[] } {
  const edits: { id: string; field: string; value: string }[] = [];
  const removes: string[] = [];
  for (const [field, value] of Object.entries(wanted)) {
    const has = field in current;
    if (value === "") {
      if (!has) continue;
      if (own.includes(field)) removes.push(field);
      else edits.push({ id: ref, field, value: "" });
    } else if (current[field] !== value) {
      edits.push({ id: ref, field, value });
    }
  }
  return { edits, removes };
}

/** `SIMULATOR_FRAME_UI::UpdateTunerValue` (the tuner's Save): the field the tuned value goes to -- the symbol's `Sim.Params` parameter when its fields
 * have one (the instance's, or the library symbol's, which KiCad's instance carries as its own), the parameter's text replaced in place; else its
 * Value. As the Symbol Fields Table's edit (`set_symbol_fields`), which reaches every sheet, a sheet read from a KiCad file among them (r7 R7-SM-2). */
export function tunerSaveEdit(ref: string, param: string, text: string, fields: Record<string, string>): { id: string; field: string; value: string } {
  const params = fields[SIM_FIELDS.params];
  const re = new RegExp(`(^|\\s)${param.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}=("[^"]*"|\\S*)`, "i");
  if (params !== undefined && re.test(params)) return { id: ref, field: SIM_FIELDS.params, value: params.replace(re, `$1${param}=${text}`) };
  return { id: ref, field: "Value", value: text };
}
