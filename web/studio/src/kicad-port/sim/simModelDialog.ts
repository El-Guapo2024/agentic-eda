// The Simulation Model dialog's built-in half (`DIALOG_SIM_MODEL`, eeschema/dialogs/dialog_sim_model.cpp, KiCad 8303b2ad), pure: what the
// dialog shows and writes for "Built-in SPICE model" and IBIS, over the models of simModelBuiltin.ts.
// - `openBuiltin` is `TransferDataToWindow`: a passive or a DC source with no model is inferred (`InferSimModel`) into the dialog's fields, its
//   Value becoming "${SIM.PARAMS}" and its primary parameter stored in the Value; a symbol with a `Sim.Device` or `Sim.Type` opens on
//   "Built-in SPICE model" at the fields' type; one model is made of every type (`m_builtinModelsMgr`), the current one from the fields, each
//   device remembering its first type (`m_curModelTypeOfDeviceType`).
// - `deviceChoices` / `typeChoices` are `updateBuiltinModelWidgets`' Device and Device type lists (`equivalent`: E and H are voltage sources, F
//   and G current ones; the KIBIS types not listed), `chooseDevice` / `chooseType` `onDeviceTypeChoice` / `onTypeChoice`.
// - `paramGrid` is `updateModelParamsTab`, `addParamPropertyIfRelevant` and `newParamProperty`: the principal parameters first, then the
//   categories in KiCad's order (Geometry, AC, DC, S-Parameters, Capacitance, Temperature, Noise, Distributed Quantities, Waveform, Limiting
//   Values, Advanced, Flags), AC and Waveform expanded; output, superfluous and initial-condition parameters left out; each row "Description
//   (name)", its value, unit, default and type, a checkbox for a bool, a choice for an enum.
// - `saveInValueLabel` the checkbox's label ("Save parameter '%s (%s)' in Value field"), `codeTab` `updateModelCodeTab`, `builtinOk`
//   `TransferDataFromWindow` for a built-in or IBIS model.
//
// - `gridText` is `SIM_STRING_PROPERTY`'s arithmetic (`NUMERIC_EVALUATOR( EDA_UNITS::UNSCALED )`, SI mode) on an Int or Float row: "1k*2"
//   becomes "2k" when Enter commits it (`StringToValue`: `SIM_VALUE::Normalize`), "2000" when the row loses the focus (`OnEvent`'s
//   `wxEVT_KILL_FOCUS`: the evaluator's result); a text that does not work out, or works out to the value it already is, is kept as typed.
// - The K device's "l1" / "l2" are a choice of the circuit's inductors (`newParamProperty`: `SPICE_CIRCUIT_MODEL::ReadSchematicAndLibraries`'
//   items of device L, `StrNumCmp` order), shown with no selection (`SIM_ENUM_PROPERTY` looks the value up in the parameter's own enum values,
//   which are none); text when the circuit has no inductor.
// - Field texts are read resolved where KiCad reads them so (`GetFieldValue( .., aResolve = true, .. )`: the type, the model's fields, the
//   library and the IBIS choices; the server resolves them as the symbol's `GetShownText` does), and written as the model gives them.
// - A field the dialog adds is visible or not as KiCad's `SetFieldValue` makes it: the inference's `Sim.Device`, `Sim.Type`, `Sim.Params` and
//   `Sim.Pins`, and the IBIS pin, model and differential fields visible; `Sim.Library`, `Sim.Name` and `WriteFields`' own hidden.
// - KiCad 25ec2335's `RestoreInferredValue`: an inferred model whose primary parameter is not saved in the Value gets its Value back as it was
//   (8303b2ad leaves "${SIM.PARAMS}").
//
// Pure: no React, no DOM.
import {
  createModelFromFields,
  createModelOfType,
  getFieldValue,
  inferSimModel,
  readTypeFromFields,
  setFieldValue,
  SIM_DEVICE_FIELD,
  SIM_DEVICE_SUBTYPE_FIELD,
  SIM_IBIS_DIFF_FIELD,
  SIM_IBIS_MODEL_FIELD,
  SIM_IBIS_PIN_FIELD,
  SIM_LIBRARY_FIELD,
  SIM_NAME_FIELD,
  SIM_PARAMS_FIELD,
  SIM_PINS_FIELD,
  SIM_REFERENCE_FIELD,
  SIM_VALUE_FIELD,
  SimModelError,
  SimModelIbis,
  spiceItem,
  type Category,
  type Fields,
  type SimModel,
  type SimModelTables,
} from "./simModelBuiltin";
import { modelPinIndex } from "./simModel";
import { NumericEvaluator } from "../numericEvaluator";
import { normalize, simValueEqual, toDouble } from "./simValue";
import { strNumCmp } from "../schSheetPins";

/** The built-in models of the dialog (`m_builtinModelsMgr`, `m_curModelType`, `m_curModelTypeOfDeviceType`). */
export interface BuiltinModels {
  /** One per type, in `TYPE_ITERATOR` order. */
  models: Map<string, SimModel>;
  curModelType: string;
  curModelTypeOfDeviceType: Map<string, string>;
}

export interface OpenedDialog {
  /** `m_fields` after the inference. */
  fields: Fields;
  /** The visibility of each field the inference added (`SetFieldValue`'s `aIsVisible`, true). */
  visible: Record<string, boolean>;
  /** KiCad 25ec2335's `m_inferredValueRestore`: the Value as it was before the inference made it "${SIM.PARAMS}" (null: not inferred). */
  inferredValueRestore: string | null;
  /** Which radio button is on (`m_rbLibraryModel` / `m_rbBuiltinModel`); neither for a symbol with no model ("undetermined"). */
  radio: "library" | "builtin" | null;
  builtin: BuiltinModels;
  /** Whether the inference stored the primary parameter in the Value (`storeInValue`). */
  inferred: boolean;
  /** `m_saveInValueCheckbox`'s first state, when the current model is a built-in one. */
  saveInValue: boolean;
  /** What KiCad shows in an error box (`DisplayErrorMessage`). */
  errors: string[];
}

/** `TransferDataToWindow` up to the model choices (the library is `loadLibrary`'s, simModel.ts). `fields` hold Reference, Value and the
 * symbol's `Sim.*` fields; `pins` the symbol's pins sorted by number (`m_sortedPartPins`). */
/** `GetFieldValue( &m_fields, .., aResolve = true, .. )` for every field: each text as `resolve` gives it (`SCH_FIELD::GetShownText`). */
export function resolvedFields(fields: Fields, resolve: (raw: string) => string): Fields {
  const out: Fields = {};
  for (const [k, v] of Object.entries(fields)) out[k] = resolve(v);
  return out;
}

export function openBuiltin(tables: SimModelTables, prefix: string, fieldsIn: Fields, pins: readonly { number: string }[], resolve: (raw: string) => string = (t) => t): OpenedDialog {
  const fields: Fields = { ...fieldsIn };
  const errors: string[] = [];
  const visible: Record<string, boolean> = {};
  let inferred = false;
  let inferredValueRestore: string | null = null;
  // `InferSimModel( m_symbol, &m_fields, false, .. )`: the texts as written
  const inf = inferSimModel(prefix, fields, pins, "SI");
  if (inf) {
    // `SetFieldValue( m_fields, .. )` (a field it adds is visible): Sim.Type only when not empty
    const set = (name: string, value: string) => {
      if (!(name in fields) && value !== "") visible[name] = true;
      setFieldValue(fields, name, value);
    };
    set(SIM_DEVICE_FIELD, inf.deviceType);
    if (inf.modelType !== "") set(SIM_DEVICE_SUBTYPE_FIELD, inf.modelType);
    set(SIM_PARAMS_FIELD, inf.modelParams);
    set(SIM_PINS_FIELD, inf.pinMap);
    inferred = true;
    // KiCad 25ec2335: "WriteFields() only rewrites Value when storeInValue stays on, so remember the original text first to restore it
    // verbatim if the user turns storeInValue off."
    inferredValueRestore = fields[SIM_VALUE_FIELD] ?? "";
    fields[SIM_VALUE_FIELD] = "${SIM.PARAMS}";
  }
  const shown = resolvedFields(fields, resolve);
  let radio: OpenedDialog["radio"] = null;
  let curModelType = "NONE";
  // the library's file name (`wxFileName( libraryFilename ).GetFullName()`)
  const library = getFieldValue(shown, SIM_LIBRARY_FIELD);
  if (library.replace(/^.*[\\/]/, "") !== "") radio = "library";
  else if (getFieldValue(fields, SIM_DEVICE_FIELD) !== "" || getFieldValue(fields, SIM_DEVICE_SUBTYPE_FIELD) !== "") {
    radio = "builtin";
    const msgs: string[] = [];
    curModelType = readTypeFromFields(tables, shown, (m) => msgs.push(m));
    if (msgs.length) errors.push(msgs.join("\n"));
  }
  const models = new Map<string, SimModel>();
  const curModelTypeOfDeviceType = new Map<string, string>();
  for (const t of tables.types) {
    if (radio === "builtin" && t.name === curModelType) {
      const msgs: string[] = [];
      models.set(t.name, createModelFromFields(tables, shown, pins, (m) => msgs.push(m)));
      if (msgs.length) errors.push(`Failed to read simulation model from fields.\n\n${msgs.join("\n")}`);
    } else models.set(t.name, createModelOfType(tables, t.name, pins));
    if (!curModelTypeOfDeviceType.has(t.deviceType)) curModelTypeOfDeviceType.set(t.deviceType, t.name);
  }
  const builtin: BuiltinModels = { models, curModelType, curModelTypeOfDeviceType };
  const cur = currentBuiltin(builtin);
  if (inferred && radio !== "library") cur.setIsStoredInValue(true);
  return { fields, visible, inferredValueRestore, radio, builtin, inferred, saveInValue: cur.isStoredInValue(), errors };
}

/** `curModel()` on "Built-in SPICE model" (or neither): the current type's model, else NONE's. */
export function currentBuiltin(b: BuiltinModels): SimModel {
  return b.models.get(b.curModelType) ?? b.models.get("NONE")!;
}

/** `equivalent`: the same device, or one of the same description ("Voltage Source" for V, E and H). */
export function equivalent(tables: SimModelTables, a: string, b: string): boolean {
  return a === b || tables.device.get(a)?.description === tables.device.get(b)?.description;
}

const KIBIS_TYPES = ["KIBIS_DEVICE", "KIBIS_DRIVER_DC", "KIBIS_DRIVER_RECT", "KIBIS_DRIVER_PRBS"];

/** The Device choice (`updateBuiltinModelWidgets`): every device shown in the menu, by description, the model's one selected. */
export function deviceChoices(tables: SimModelTables, model: SimModel): { items: string[]; selected: number } {
  const items: string[] = [];
  let selected = -1;
  for (const d of tables.devices) {
    if (!d.showInMenu) continue;
    items.push(d.description);
    if (equivalent(tables, d.name, model.deviceType())) selected = items.length - 1;
  }
  return { items, selected };
}

/** The Device type choice: the model's device's types (or a device of its description), by description; shown only with more than one. */
export function typeChoices(tables: SimModelTables, model: SimModel): { items: string[]; selected: number; shown: boolean } {
  const items: string[] = [];
  let selected = -1;
  const desc = model.deviceInfo().description;
  for (const t of tables.types) {
    if (KIBIS_TYPES.includes(t.name)) continue;
    if (t.deviceType === model.deviceType() || tables.device.get(t.deviceType)?.description === desc) {
      items.push(t.description);
      if (t.name === model.type) selected = items.length - 1;
    }
  }
  return { items, selected, shown: items.length > 1 };
}

/** `onDeviceTypeChoice`: the device of that description takes the type it had last (its first, at the start). */
export function chooseDevice(tables: SimModelTables, b: BuiltinModels, description: string): void {
  for (const d of tables.devices) {
    if (d.description === description) {
      b.curModelType = b.curModelTypeOfDeviceType.get(d.name) ?? b.curModelType;
      break;
    }
  }
}

/** `onTypeChoice`: the first type of the current device (or an equivalent one) with that description; the device remembers it. */
export function chooseType(tables: SimModelTables, b: BuiltinModels, description: string): void {
  const device = currentBuiltin(b).deviceType();
  for (const t of tables.types) {
    if (equivalent(tables, device, t.deviceType) && description === t.description) {
      b.curModelType = t.name;
      break;
    }
  }
  b.curModelTypeOfDeviceType.set(device, b.curModelType);
}

/** The categories of the grid, in the order `updateModelParamsTab` appends them, with their titles. */
export const PARAM_CATEGORIES: [Category, string][] = [
  ["GEOMETRY", "Geometry"],
  ["AC", "AC"],
  ["DC", "DC"],
  ["S_PARAM", "S-Parameters"],
  ["CAPACITANCE", "Capacitance"],
  ["TEMPERATURE", "Temperature"],
  ["NOISE", "Noise"],
  ["DISTRIBUTED_QUANTITIES", "Distributed Quantities"],
  ["WAVEFORM", "Waveform"],
  ["LIMITING_VALUES", "Limiting Values"],
  ["ADVANCED", "Advanced"],
  ["FLAGS", "Flags"],
];

/** The categories `updateModelParamsTab` leaves expanded. */
export const EXPANDED_CATEGORIES = ["AC", "Waveform"];

/** One row of the Parameters grid (`newParamProperty`). */
export interface ParamRow {
  /** The model's parameter index. */
  index: number;
  name: string;
  /** "Description (name)", or the name. */
  label: string;
  kind: "bool" | "string" | "enum";
  value: string;
  /** An enum's choices. */
  choices: string[];
  /** The choice shown selected (`SIM_ENUM_PROPERTY`'s constructor: the value among the parameter's enum values, else none, -1). */
  selected: number;
  /** `SIM_VALUE::TYPE` of a text row: an Int or Float one evaluates arithmetic (`SIM_STRING_PROPERTY::allowEval`). */
  valueType: string;
  unit: string;
  default: string;
  /** "Bool", "Int", "Float", .. (`PARAM_COLUMN::TYPE`). */
  type: string;
}

const TYPE_STRINGS: Record<string, string> = {
  TYPE_BOOL: "Bool",
  TYPE_INT: "Int",
  TYPE_FLOAT: "Float",
  TYPE_COMPLEX: "Complex",
  TYPE_STRING: "String",
  TYPE_BOOL_VECTOR: "Bool Vector",
  TYPE_INT_VECTOR: "Int Vector",
  TYPE_FLOAT_VECTOR: "Float Vector",
  TYPE_COMPLEX_VECTOR: "Complex Vector",
};

/** `newParamProperty`: a bool a checkbox, an enum a choice of its values, the K device's "l1" / "l2" a choice of the circuit's `inductors` (text
 * when there are none), anything else text. */
function paramRow(model: SimModel, index: number, inductors: readonly string[]): ParamRow {
  const p = model.getParam(index);
  const info = p.info;
  const label = info.description === "" ? info.name : `${info.description} (${info.name})`;
  let kind: ParamRow["kind"] = "string";
  let choices: string[] = [];
  if (info.type === "TYPE_BOOL") kind = "bool";
  else if (info.type === "TYPE_STRING") {
    // "Special case: K-line mutual inductance statement parameters l1 and l2 are references to other inductors in the circuit."
    if (model.type === "K" && (info.name === "l1" || info.name === "l2")) {
      if (inductors.length > 0) {
        kind = "enum";
        choices = [...inductors];
      }
    } else if (info.enumValues.length > 0) {
      kind = "enum";
      choices = [...info.enumValues];
    }
  }
  const selected = kind === "enum" ? info.enumValues.indexOf(p.value) : -1;
  return { index, name: info.name, label, kind, value: p.value, choices, selected, valueType: info.type, unit: info.unit, default: info.defaultValue, type: TYPE_STRINGS[info.type] ?? "" };
}

/** The K device's choices (`newParamProperty`): the circuit's items of device L by reference, `StrNumCmp` order. */
export function inductorChoices(items: readonly { ref: string; sim_device?: string }[]): string[] {
  return items
    .filter((i) => i.sim_device === "L")
    .map((i) => i.ref)
    .sort((a, b) => strNumCmp(a, b, true));
}

/** `SIM_STRING_PROPERTY`'s arithmetic on an Int or Float row (`allowEval`), `NUMERIC_EVALUATOR( EDA_UNITS::UNSCALED )`: on Enter
 * (`StringToValue`, `m_needsEval`) the result normalized ("2k"), when the row loses the focus (`OnEvent`, `wxEVT_KILL_FOCUS`) the evaluator's
 * result ("2000"); the text as typed when it is empty, does not work out, or works out to what it is already ("Don't mess up user formatting if
 * eval'ing didn't actually change the value."). */
export function gridText(text: string, valueType: string, how: "enter" | "blur"): string {
  if (valueType !== "TYPE_INT" && valueType !== "TYPE_FLOAT") return text;
  if (text === "") return text;
  const evaluator = new NumericEvaluator("si");
  if (!evaluator.process(text)) return text;
  const value = toDouble(evaluator.result());
  if (Number.isNaN(value) || simValueEqual(value, text)) return text;
  return how === "enter" ? normalize(value) : evaluator.result();
}

/** The Parameters grid: the principal rows, then each category with rows (`HideProperty( .., false )` only for those), in KiCad's order.
 * `inductors` are the K device's choices ([`inductorChoices`]). */
export function paramGrid(model: SimModel, inductors: readonly string[] = []): { principal: ParamRow[]; groups: { category: Category; title: string; rows: ParamRow[]; expanded: boolean }[] } {
  const principal: ParamRow[] = [];
  const byCategory = new Map<Category, ParamRow[]>();
  for (let i = 0; i < model.getParamCount(); i++) {
    const info = model.getParam(i).info;
    if (info.dir === "DIR_OUT") continue;
    if (info.category === "INITIAL_CONDITIONS" || info.category === "SUPERFLUOUS") continue;
    const known = PARAM_CATEGORIES.some(([c]) => c === info.category);
    if (!known) principal.push(paramRow(model, i, inductors));
    else {
      if (!byCategory.has(info.category)) byCategory.set(info.category, []);
      byCategory.get(info.category)!.push(paramRow(model, i, inductors));
    }
  }
  const groups = PARAM_CATEGORIES.filter(([c]) => byCategory.has(c)).map(([c, title]) => ({ category: c, title, rows: byCategory.get(c)!, expanded: EXPANDED_CATEGORIES.includes(title) }));
  return { principal, groups };
}

/** A grid cell's edit (`SIM_BOOL_PROPERTY::OnSetValue`, `SIM_STRING_PROPERTY::StringToValue`, `SIM_ENUM_PROPERTY::IntToValue`): the text set on
 * the model (a bool "1" / "0"), which a source then fills in before it (autofill). */
export function setGridValue(model: SimModel, index: number, value: string | boolean): void {
  model.setParamValue(index, typeof value === "boolean" ? (value ? "1" : "0") : value);
}

/** The "Save .. in Value field" checkbox (`updateBuiltinModelWidgets`): its label and whether it can be ticked (unticked when it cannot). */
export function saveInValueLabel(model: SimModel): { label: string; enabled: boolean } {
  if (model.hasPrimaryValue()) {
    const primary = model.getParam(0).info;
    return { label: `Save parameter '${primary.description} (${primary.name})' in Value field`, enabled: true };
  }
  return { label: "Save primary parameter in Value field", enabled: false };
}

/** `updateModelCodeTab`: the model's `Preview` with the reference as the model name (a library model's selected name otherwise). */
export function codeTab(model: SimModel, fields: Fields, builtinRadio: boolean, listSelection = ""): string {
  let modelName = listSelection;
  if (builtinRadio || modelName === "") modelName = getFieldValue(fields, SIM_REFERENCE_FIELD);
  return model.generator.preview(spiceItem({ modelName }));
}

/** Whether the model is a raw SPICE element, whose Code tab is the one the dialog opens on and is read back on OK. */
export function isRawSpice(model: SimModel): boolean {
  return model.type === "RAWSPICE";
}

/** IBIS choices the dialog writes (`m_pinCombobox`, `m_pinModelCombobox`, `m_differentialCheckbox`). */
export interface IbisChoice {
  pin: string;
  model: string;
  differential: boolean;
}

/** `TransferDataFromWindow` for a built-in model (or an IBIS one, with its library path, component and choices): the fields after OK.
 * `column` is the Pin Assignments grid's Model Pin column, row by symbol pin; `code` the Code tab's text (read for a raw SPICE element). */
export function builtinOk(
  model: SimModel,
  fieldsIn: Fields,
  opts: {
    saveInValue: boolean;
    column: readonly string[];
    pins: readonly { number: string }[];
    code?: string;
    library?: { path: string; name: string; ibis?: IbisChoice };
    /** The visibility of the fields added so far; this adds the ones OK adds (`SetFieldValue`'s `aIsVisible`). */
    visible?: Record<string, boolean>;
    /** KiCad 25ec2335's `RestoreInferredValue`: the Value before the inference, null when the model was not inferred. */
    inferredValueRestore?: string | null;
  }
): Fields {
  const fields: Fields = { ...fieldsIn };
  const visible = opts.visible ?? {};
  const set = (name: string, value: string, isVisible: boolean) => {
    if (!(name in fields) && value !== "") visible[name] = isVisible;
    setFieldValue(fields, name, value);
  };
  set(SIM_LIBRARY_FIELD, opts.library?.path ?? "", false);
  set(SIM_NAME_FIELD, opts.library?.name ?? "", false);
  const ibis = opts.library?.ibis;
  if (ibis && model instanceof SimModelIbis) {
    set(SIM_IBIS_PIN_FIELD, ibis.pin, true);
    set(SIM_IBIS_MODEL_FIELD, ibis.model, true);
    set(SIM_IBIS_DIFF_FIELD, model.canDifferential() && ibis.differential ? "1" : "", true);
  }
  if (isRawSpice(model) && opts.code !== undefined) model.setParamValue("model", opts.code.trim());
  model.setIsStoredInValue(opts.saveInValue);
  opts.column.forEach((cell, row) => {
    const pin = opts.pins[row];
    if (pin) model.assignSymbolPinNumberToModelPinIndex(modelPinIndex(cell), pin.number);
  });
  // `removeOrphanedPinAssignments`
  for (let i = 0; i < model.getPinCount(); i++) if (!opts.pins.some((p) => p.number === model.getPin(i).symbolPinNumber)) model.assignSymbolPinNumberToModelPinIndex(i, "");
  // `WriteFields`: the fields it adds hidden
  const before = new Set(Object.keys(fields));
  model.writeFields(fields);
  for (const name of Object.keys(fields)) if (!before.has(name)) visible[name] = false;
  if (opts.inferredValueRestore !== undefined && opts.inferredValueRestore !== null && !model.isStoredInValue()) fields[SIM_VALUE_FIELD] = opts.inferredValueRestore;
  return fields;
}

/** The IBIS waveform choice (`updateIbisWidgets`): the four KIBIS types' descriptions, the model's selected. */
export function waveformChoices(tables: SimModelTables, model: SimModel): { items: string[]; selected: number } {
  const items: string[] = [];
  let selected = -1;
  for (const name of KIBIS_TYPES) {
    const t = tables.type.get(name)!;
    if (t.deviceType === model.deviceType() || tables.device.get(t.deviceType)?.description === model.deviceInfo().description) {
      items.push(t.description);
      if (name === model.type) selected = items.length - 1;
    }
  }
  return { items, selected };
}

/** `onWaveformChoice`: a model of the chosen waveform made from the component's (`SIM_MODEL_IBIS( type, baseModel )`, set in the library's place
 * first) and given the fields; a field that does not read is KiCad's error box (`DisplayErrorMessage( err.What() )`), the model kept as far as
 * it got. */
export function ibisWaveformModel(tables: SimModelTables, base: SimModelIbis, description: string, fields: Fields, pins: readonly { number: string }[]): { model: SimModelIbis; error: string | null } {
  for (const name of KIBIS_TYPES) {
    const t = tables.type.get(name)!;
    if (equivalent(tables, base.deviceType(), t.deviceType) && description === t.description) {
      const m = SimModelIbis.from(tables, name, base);
      try {
        m.readDataFields(fields, pins);
      } catch (e) {
        if (!(e instanceof SimModelError)) throw e;
        return { model: m, error: e.message };
      }
      return { model: m, error: null };
    }
  }
  return { model: base, error: null };
}

/** What `DIALOG_SYMBOL_PROPERTIES::OnEditSpiceModel` does with the fields the Simulation Model dialog hands back (`input` the copy it gave,
 * `output` the copy after OK, Reference and Value aside): "Add in any new fields" -- a field of the same name takes the new text (`set`, only
 * when it changed), one it lacks is appended with the visibility the dialog gave it (`add`) -- and "Remove any deleted fields" (`remove`). */
export function editedFieldsMerge(
  input: Fields,
  output: Fields,
  visible: Record<string, boolean>
): { set: { name: string; text: string }[]; add: { name: string; text: string; visible: boolean }[]; remove: string[] } {
  const skip = (n: string) => n === SIM_REFERENCE_FIELD || n === SIM_VALUE_FIELD;
  const set: { name: string; text: string }[] = [];
  const add: { name: string; text: string; visible: boolean }[] = [];
  const remove: string[] = [];
  for (const [name, text] of Object.entries(output)) {
    if (skip(name)) continue;
    if (name in input) {
      if (input[name] !== text) set.push({ name, text });
    } else add.push({ name, text, visible: visible[name] ?? true });
  }
  for (const name of Object.keys(input)) if (!skip(name) && !(name in output)) remove.push(name);
  return { set, add, remove };
}
