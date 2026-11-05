// KiCad's simulation models (`SIM_MODEL` and its classes, eeschema/sim/sim_model*.cpp, KiCad 8303b2ad), as the Simulation Model dialog's
// "Built-in SPICE model" uses them: the device and type registry, each model's parameters and pins, the `Sim.*` fields read and written
// (`SIM_MODEL_SERIALIZER`), the inference of a passive or a DC source from its Value (`InferSimModel`), and the SPICE each model generates
// (`SPICE_GENERATOR` and its subclasses), which the dialog's Code tab shows and kicad-cli writes in the netlist.
//
// The tables -- `DeviceInfo`, `TypeInfo`, `SpiceInfo`, which class `SIM_MODEL::Create` makes of each type, and every model's `PARAM::INFO`s
// (the ngspice ones of `sim_model_ngspice_data_*.cpp` among them) -- are not typed here: tools/extract-sim-models.js reads them out of the C++
// into src/kicad/sim_models.json, and `decodeTables` takes that file. What is code in KiCad is ported here, function by function:
// - `SIM_MODEL`: `AddParam` (an enum parameter starts at its default), `GetParam` (the base model's when the override is empty), `doFindParam`
//   (case-insensitive), `SetParamValue` (a `,` or a non-SI notation converted), `createPins`, `AssignSymbolPinNumberToModelPin`,
//   `ReadTypeFromFields`, `ReadDataFields`, `WriteFields`, `requiresSpiceModelLine`, `InferSimModel`.
// - the classes: `SIM_MODEL_IDEAL`, `SIM_MODEL_R_POT`, `SIM_MODEL_L_MUTUAL`, `SIM_MODEL_BEHAVIORAL`, `SIM_MODEL_TLINE`, `SIM_MODEL_SWITCH`,
//   `SIM_MODEL_SOURCE` (its `doSetParamValue` fill, autofill, `HasPrimaryValue` for DC only), `SIM_MODEL_NGSPICE` (the instance parameters it
//   takes, `doFindParam`'s second pass for escaped names), `SIM_MODEL_RAW_SPICE`, `SIM_MODEL_XSPICE`, `SIM_MODEL_SUBCKT`, `SIM_MODEL_IBIS`
//   (its pins, `SwitchSingleEndedDiff`).
// - `SIM_MODEL_SERIALIZER`: `GenerateDevice`, `GenerateDeviceSubtype`, `GenerateValue`, `GenerateParams`, `GeneratePins`, `GenerateEnable`,
//   `ParseValue`, `ParseParams`, `ParsePins`, `ParseEnable`, `generateParamValuePair`, with the PEG grammars of sim_model_serializer.h read by
//   hand (their error messages, PEGTL's "source:line:column: message").
// - `SPICE_GENERATOR` (`ModelName`, `ModelLine`, `ItemLine`, `ItemName`, `ItemPins`, `ItemModelName`, `ItemParams`, `Preview`) and
//   `SPICE_GENERATOR_IDEAL`, `_SOURCE` (`formatPwlValues`, `rewriteBraceExpression`, the PULSE "0 1" of ngspice 46), `_BEHAVIORAL`, `_SWITCH`,
//   `_TLINE`, `_R_POT`, `_L_MUTUAL`, `_SPICE` (and `_NGSPICE`), `_RAW_SPICE`, `_IBIS`.
//
// Where it differs:
// - Field texts are taken as written: KiCad reads them resolved (`GetShownText`), so a `${VAR}` in a `Sim.*` field is not expanded here.
// - IBIS: `SPICE_GENERATOR_IBIS::IbisDevice` (the KIBIS SPICE of a pin's model) is not ported; kicad-cli writes it in the netlist. The dialog
//   needs only the model's parameters and pins.
// - Design variants (`WriteFields`' sheet path and variant name) are not taken: the fields are the symbol's own.
//
// Pure: no React, no DOM.
import { strNumCmp } from "../schSheetPins";
import { formatG } from "./spiceValue";
import { convertNotation, isPlainNumber, toDouble, toSpice, normalize, type Notation } from "./simValue";

// ---------------------------------------------------------------------------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** `SIM_VALUE::TYPE`. */
export type ValueType = "TYPE_BOOL" | "TYPE_INT" | "TYPE_FLOAT" | "TYPE_COMPLEX" | "TYPE_STRING" | "TYPE_BOOL_VECTOR" | "TYPE_INT_VECTOR" | "TYPE_FLOAT_VECTOR" | "TYPE_COMPLEX_VECTOR";

/** `SIM_MODEL::PARAM::CATEGORY`. */
export type Category =
  | "PRINCIPAL"
  | "GEOMETRY"
  | "AC"
  | "DC"
  | "CAPACITANCE"
  | "TEMPERATURE"
  | "NOISE"
  | "DISTRIBUTED_QUANTITIES"
  | "LIMITING_VALUES"
  | "S_PARAM"
  | "ADVANCED"
  | "FLAGS"
  | "WAVEFORM"
  | "INITIAL_CONDITIONS"
  | "SUPERFLUOUS";

/** `SIM_MODEL::PARAM::INFO`. */
export interface ParamInfo {
  name: string;
  id: number;
  dir: "DIR_IN" | "DIR_OUT" | "DIR_INOUT";
  type: ValueType;
  unit: string;
  category: Category;
  defaultValue: string;
  description: string;
  isSpiceInstanceParam: boolean;
  isInstanceParam: boolean;
  spiceModelName: string;
  spiceInstanceName: string;
  enumValues: string[];
}

/** `SIM_MODEL::DEVICE_INFO` with the `DEVICE_T` it is of. */
export interface DeviceInfo {
  name: string;
  fieldValue: string;
  description: string;
  showInMenu: boolean;
}

/** `SIM_MODEL::SPICE_INFO`. */
export interface SpiceInfo {
  itemType: string;
  modelType: string;
  functionName: string;
  level: string;
  isDefaultLevel: boolean;
  hasExpression: boolean;
  version: string;
}

/** One `SIM_MODEL::TYPE`: `TypeInfo`, `SpiceInfo`, the class `Create( TYPE )` makes, and its parameters (or, for an ngspice model, which table). */
export interface TypeInfo {
  name: string;
  deviceType: string;
  fieldValue: string;
  description: string;
  spice: SpiceInfo;
  class: string;
  params?: ParamInfo[];
  ngspiceModel?: string;
}

/** `SIM_MODEL_NGSPICE::MODEL_INFO`. */
export interface NgspiceModelInfo {
  name: string;
  variant1: string;
  variant2: string;
  pinNames: string[];
  description: string;
  modelParams: ParamInfo[];
  instanceParams: ParamInfo[];
}

export interface SimModelTables {
  commit: string;
  /** `DEVICE_T_ITERATOR()` order. */
  devices: DeviceInfo[];
  /** `TYPE_ITERATOR()` order. */
  types: TypeInfo[];
  ngspice: Record<string, NgspiceModelInfo>;
  device: Map<string, DeviceInfo>;
  type: Map<string, TypeInfo>;
}

/** src/kicad/sim_models.json as tools/extract-sim-models.js writes it. */
export interface SimModelsJson {
  commit: string;
  paramFields: string[];
  devices: DeviceInfo[];
  types: (Omit<TypeInfo, "params"> & { params?: unknown[][] })[];
  ngspice: Record<string, Omit<NgspiceModelInfo, "modelParams" | "instanceParams"> & { modelParams: unknown[][]; instanceParams: unknown[][] }>;
}

/** `PARAM::INFO()`'s defaults, for the fields a row leaves out. */
const INFO_DEFAULTS: Record<string, unknown> = {
  name: "",
  id: 0,
  dir: "DIR_INOUT",
  type: "TYPE_FLOAT",
  unit: "",
  category: "PRINCIPAL",
  defaultValue: "",
  description: "",
  isSpiceInstanceParam: false,
  isInstanceParam: false,
  spiceModelName: "",
  spiceInstanceName: "",
  enumValues: [],
};

/** The JSON's tables as objects. */
export function decodeTables(json: SimModelsJson): SimModelTables {
  const decode = (row: unknown[]): ParamInfo => {
    const info: Record<string, unknown> = {};
    json.paramFields.forEach((f, k) => (info[f] = k < row.length ? row[k] : INFO_DEFAULTS[f]));
    return Object.freeze(info) as unknown as ParamInfo;
  };
  const types: TypeInfo[] = json.types.map((t) => ({ ...t, params: t.params?.map(decode) }));
  const ngspice: Record<string, NgspiceModelInfo> = {};
  for (const [k, v] of Object.entries(json.ngspice)) ngspice[k] = { ...v, modelParams: v.modelParams.map(decode), instanceParams: v.instanceParams.map(decode) };
  return {
    commit: json.commit,
    devices: json.devices,
    types,
    ngspice,
    device: new Map(json.devices.map((d) => [d.name, d])),
    type: new Map(types.map((t) => [t.name, t])),
  };
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// The fields
// ---------------------------------------------------------------------------------------------------------------------------------------------

export const SIM_DEVICE_FIELD = "Sim.Device";
export const SIM_DEVICE_SUBTYPE_FIELD = "Sim.Type";
export const SIM_PINS_FIELD = "Sim.Pins";
export const SIM_PARAMS_FIELD = "Sim.Params";
export const SIM_LIBRARY_FIELD = "Sim.Library";
export const SIM_NAME_FIELD = "Sim.Name";
export const SIM_LEGACY_ENABLE_FIELD_V7 = "Sim.Enable";
export const SIM_REFERENCE_FIELD = "Reference";
export const SIM_VALUE_FIELD = "Value";
/** `SIM_LIBRARY_IBIS::PIN_FIELD`, `MODEL_FIELD`, `DIFF_FIELD`. */
export const SIM_IBIS_PIN_FIELD = "Sim.Ibis.Pin";
export const SIM_IBIS_MODEL_FIELD = "Sim.Ibis.Model";
export const SIM_IBIS_DIFF_FIELD = "Sim.Ibis.Diff";

/** A symbol's fields by name, in their order (`std::vector<SCH_FIELD>`; the Reference and Value among them). */
export type Fields = Record<string, string>;

/** `GetFieldValue( &aFields, aName, .. )`: the text, or "" for a field that is not there (or no fields). */
export function getFieldValue(fields: Fields | null, name: string): string {
  return fields?.[name] ?? "";
}

/** `SetFieldValue( aFields, aName, aValue )` without a sheet path: "" erases the field, else the text is set (a new field at the end). */
export function setFieldValue(fields: Fields, name: string, value: string): void {
  if (value === "") delete fields[name];
  else fields[name] = value;
}

/** An error KiCad throws as `IO_ERROR` (`THROW_IO_ERROR`): `message` is its `Problem()`. */
export class SimModelError extends Error {}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// SIM_MODEL
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** `SIM_MODEL_PIN`. */
export interface SimModelPin {
  readonly modelPinName: string;
  symbolPinNumber: string;
}

/** `SIM_MODEL::PARAM`: the value an instance gives and the info it is of. */
export interface Param {
  readonly info: ParamInfo;
  value: string;
}

/** `SPICE_ITEM`. */
export interface SpiceItem {
  refName: string;
  pinNumbers: string[];
  pinNetNames: string[];
  baseModelName: string;
  modelName: string;
}

export function spiceItem(partial: Partial<SpiceItem> = {}): SpiceItem {
  return { refName: "", pinNumbers: [], pinNetNames: [], baseModelName: "", modelName: "", ...partial };
}

const iequals = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** `SIM_MODEL`. */
export class SimModel {
  params: Param[] = [];
  modelPins: SimModelPin[] = [];
  baseModel: SimModel | null = null;
  isEnabled = true;
  protected storedInValue = false;
  readonly generator: SpiceGenerator;

  constructor(
    readonly tables: SimModelTables,
    readonly type: string,
    generator?: (m: SimModel) => SpiceGenerator
  ) {
    this.generator = generator ? generator(this) : new SpiceGenerator(this);
  }

  typeInfo(): TypeInfo {
    const t = this.tables.type.get(this.type);
    if (!t) throw new Error(`no simulation model type ${this.type}`);
    return t;
  }
  deviceType(): string {
    return this.typeInfo().deviceType;
  }
  deviceInfo(): DeviceInfo {
    return this.tables.device.get(this.deviceType())!;
  }
  spiceInfo(): SpiceInfo {
    return this.typeInfo().spice;
  }

  /** `AddParam`: enums start at their default value. */
  addParam(info: ParamInfo): void {
    this.params.push({ info, value: info.enumValues.length >= 1 ? info.defaultValue : "" });
  }

  /** `GetPinNames`: none by default (`createPins` then adds nothing). */
  getPinNames(): string[] {
    return [];
  }
  hasAutofill(): boolean {
    return false;
  }
  hasPrimaryValue(): boolean {
    return false;
  }
  /** `SetIsStoredInValue`: only a model with a primary value can be. */
  setIsStoredInValue(v: boolean): void {
    if (this.hasPrimaryValue()) this.storedInValue = v;
  }
  isStoredInValue(): boolean {
    return this.storedInValue;
  }
  switchSingleEndedDiff(_diff: boolean): void {
    // `SIM_MODEL::SwitchSingleEndedDiff`: nothing but for IBIS
  }

  setBaseModel(base: SimModel): void {
    this.baseModel = base;
  }

  addPin(pin: SimModelPin): void {
    this.modelPins.push({ ...pin });
  }
  clearPins(): void {
    this.modelPins = [];
  }
  getPinCount(): number {
    return this.modelPins.length;
  }
  getPin(i: number): SimModelPin {
    const p = this.modelPins[i];
    if (!p) throw new RangeError(`no model pin ${i}`);
    return p;
  }
  getPins(): SimModelPin[] {
    return this.modelPins;
  }
  findModelPinIndex(symbolPinNumber: string): number {
    return this.modelPins.findIndex((p) => p.symbolPinNumber === symbolPinNumber);
  }

  /** `AssignSymbolPinNumberToModelPin( int, .. )`: out-of-range indices are ignored. */
  assignSymbolPinNumberToModelPinIndex(index: number, symbolPinNumber: string): void {
    if (index >= 0 && index < this.modelPins.length) this.modelPins[index]!.symbolPinNumber = symbolPinNumber;
  }

  /** `AssignSymbolPinNumberToModelPin( name, .. )`: by name, else a 1-based index ("Unknown simulation model pin '%s'"). */
  assignSymbolPinNumberToModelPin(modelPinName: string, symbolPinNumber: string): void {
    const pin = this.modelPins.find((p) => p.modelPinName === modelPinName);
    if (pin) {
      pin.symbolPinNumber = symbolPinNumber;
      return;
    }
    const index = strtol(modelPinName);
    if (index < 1 || index > this.modelPins.length) throw new SimModelError(`Unknown simulation model pin '${modelPinName}'`);
    this.modelPins[index - 1]!.symbolPinNumber = symbolPinNumber;
  }

  getParamCount(): number {
    return this.params.length;
  }

  /** `GetParam`: the base model's parameter when this one's value is empty. */
  getParam(i: number): Param {
    const p = this.params[i];
    if (!p) throw new RangeError(`no parameter ${i}`);
    if (this.baseModel && p.value === "") return this.baseModel.getParam(i);
    return p;
  }
  getParamOverride(i: number): Param {
    const p = this.params[i];
    if (!p) throw new RangeError(`no parameter ${i}`);
    return p;
  }
  getBaseParam(i: number): Param {
    return this.baseModel ? this.baseModel.getParam(i) : this.getParamOverride(i);
  }

  /** `doFindParam`: case-insensitive, -1 when none. */
  doFindParam(name: string): number {
    for (let i = 0; i < this.params.length; i++) if (iequals(this.getParam(i).info.name, name)) return i;
    return -1;
  }
  findParam(name: string): Param | null {
    const i = this.doFindParam(name);
    return i >= 0 ? this.getParam(i) : null;
  }

  doSetParamValue(i: number, value: string): void {
    this.getParamOverride(i).value = value;
  }

  /** `SetParamValue`: a value with a `,`, or not in SI notation and not a plain number, is converted to SI first. */
  setParamValue(index: number | string, value: string, notation: Notation = "SI"): void {
    let i: number;
    if (typeof index === "string") {
      i = this.doFindParam(index);
      if (i < 0) throw new SimModelError(`Unknown simulation model parameter '${index}'`);
    } else i = index;
    if (value.includes(",")) this.doSetParamValue(i, convertNotation(value, notation, "SI"));
    else if (notation !== "SI" && !isPlainNumber(value)) this.doSetParamValue(i, convertNotation(value, notation, "SI"));
    else this.doSetParamValue(i, value);
  }

  /** `createPins`: model pin i on the i-th symbol pin (sorted by number); past the symbol's pins a required pin is unconnected and an optional
   * one ("<S>") left out. */
  createPins(symbolPins: readonly { number: string }[]): void {
    const names = this.getPinNames();
    names.forEach((name, i) => {
      const optional = name.startsWith("<") && name.endsWith(">");
      if (i < symbolPins.length) this.addPin({ modelPinName: name, symbolPinNumber: symbolPins[i]!.number });
      else if (!optional) this.addPin({ modelPinName: name, symbolPinNumber: "" });
    });
  }

  /** `requiresSpiceModelLine`. */
  requiresSpiceModelLine(item: SpiceItem): boolean {
    if (this.type === "SUBCKT") return false;
    if (!this.baseModel || item.baseModelName === "") return true;
    for (let i = 0; i < this.params.length; i++) {
      const p = this.params[i]!;
      if (p.info.isSpiceInstanceParam) continue;
      if (p.value === "") continue;
      const baseValue = this.baseModel.params[i]?.value ?? "";
      if (p.value === baseValue) continue;
      if (p.value === normalize(toDouble(baseValue))) continue;
      return true;
    }
    return false;
  }

  /** `ReadDataFields`: the IBIS differential flag, `Sim.Enable`, the pins (`createPins` then `Sim.Pins`), and `Sim.Params` -- or, when that
   * does not give a model with a primary value its value, the Value field. `null` fields read as all empty. */
  readDataFields(fields: Fields | null, symbolPins: readonly { number: string }[]): void {
    this.switchSingleEndedDiff(getFieldValue(fields, SIM_IBIS_DIFF_FIELD) === "1");
    parseEnable(this, getFieldValue(fields, SIM_LEGACY_ENABLE_FIELD_V7));
    this.createPins(symbolPins);
    parsePins(this, getFieldValue(fields, SIM_PINS_FIELD));
    if (!parseParams(this, getFieldValue(fields, SIM_PARAMS_FIELD))) parseValue(this, getFieldValue(fields, SIM_VALUE_FIELD));
  }

  /** `WriteFields`: `Sim.Device`, `Sim.Type`, `Sim.Enable`, `Sim.Pins`, `Sim.Params` (an empty one erased), and the Value when the primary
   * parameter is stored there. */
  writeFields(fields: Fields): void {
    setFieldValue(fields, SIM_DEVICE_FIELD, generateDevice(this));
    setFieldValue(fields, SIM_DEVICE_SUBTYPE_FIELD, generateDeviceSubtype(this));
    setFieldValue(fields, SIM_LEGACY_ENABLE_FIELD_V7, generateEnable(this));
    setFieldValue(fields, SIM_PINS_FIELD, generatePins(this));
    setFieldValue(fields, SIM_PARAMS_FIELD, generateParams(this));
    if (this.isStoredInValue()) setFieldValue(fields, SIM_VALUE_FIELD, generateValue(this));
  }
}

/** C's `strtol( s, nullptr, 10 )`: the leading integer (blanks skipped), 0 when there is none. */
function strtol(s: string): number {
  const m = /^\s*([+-]?\d+)/.exec(s);
  return m ? Number.parseInt(m[1]!, 10) : 0;
}

// --- the classes -----------------------------------------------------------------------------------------------------------------------------

function addTypeParams(m: SimModel): void {
  for (const p of m.typeInfo().params ?? []) m.addParam(p);
}

/** `SIM_MODEL_IDEAL`: R, C, L -- one parameter, the primary value, on pins "+" and "-". */
export class SimModelIdeal extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorIdeal(m));
    addTypeParams(this);
  }
  override hasPrimaryValue() {
    return true;
  }
  override getPinNames() {
    return ["+", "-"];
  }
}

/** `SIM_MODEL_R_POT`. */
export class SimModelRPot extends SimModel {
  constructor(tables: SimModelTables) {
    super(tables, "R_POT", (m) => new SpiceGeneratorRPot(m));
    addTypeParams(this);
  }
  override hasPrimaryValue() {
    return true;
  }
  override getPinNames() {
    return ["r0", "wiper", "r1"];
  }
}

/** `SIM_MODEL_L_MUTUAL` (K): no pins. */
export class SimModelLMutual extends SimModel {
  constructor(tables: SimModelTables) {
    super(tables, "K", (m) => new SpiceGeneratorLMutual(m));
    addTypeParams(this);
  }
}

/** `SIM_MODEL_BEHAVIORAL`. */
export class SimModelBehavioral extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorBehavioral(m));
    addTypeParams(this);
  }
  override getPinNames() {
    return ["+", "-"];
  }
}

/** `SIM_MODEL_TLINE`. */
export class SimModelTline extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorTline(m));
    addTypeParams(this);
  }
  override getPinNames() {
    return ["1+", "1-", "2+", "2-"];
  }
}

/** `SIM_MODEL_SWITCH`: always a `.model` line. */
export class SimModelSwitch extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorSwitch(m));
    addTypeParams(this);
  }
  override getPinNames() {
    return ["ctrl+", "ctrl-", "no+", "no-"];
  }
  override requiresSpiceModelLine() {
    return true;
  }
}

/** `SIM_MODEL_SOURCE`. */
export class SimModelSource extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorSource(m));
    addTypeParams(this);
  }
  override hasAutofill() {
    return true;
  }
  override hasPrimaryValue() {
    return this.type === "V" || this.type === "I";
  }
  override getPinNames() {
    const d = this.deviceType();
    return d === "E" || d === "G" ? ["+", "-", "C+", "C-"] : ["+", "-"];
  }

  /** `SIM_MODEL_SOURCE::doSetParamValue`: "All preceding parameter values must be filled": an empty one before the one set takes the set one's
   * default when that is a number, else "0" (KiCad's code reads the default of `aParamIndex`, the parameter being set, for each of them). An
   * emptied one empties only itself (the loop writes `aParamIndex` each time). */
  override doSetParamValue(i: number, value: string): void {
    if (value === "") {
      for (let k = i; k < this.params.length; k++) this.getParamOverride(i).value = "";
    } else {
      for (let k = 0; k < i; k++) {
        if (this.getParam(k).value === "") {
          let def = this.getParamOverride(i).info.defaultValue;
          if (!wxToDouble(def)) def = "0";
          this.getParamOverride(i).value = def;
          super.setParamValue(k, def);
        }
      }
    }
    super.doSetParamValue(i, value);
  }
}

/** `wxString::ToDouble` succeeding: the whole text a C-locale number (`wxStrtod` to the end). */
function wxToDouble(s: string): boolean {
  return /^\s*[+-]?(\d+\.?\d*([eE][+-]?\d+)?|\.\d+([eE][+-]?\d+)?|inf(inity)?|nan)$/i.test(s);
}

/** `SIM_MODEL_NGSPICE`: the instance parameters of the PRINCIPAL, GEOMETRY and FLAGS categories ("For now, only the geometry and flags
 * parameters"), then the model parameters, of its ngspice table. */
export class SimModelNgspice extends SimModel {
  readonly modelInfo: NgspiceModelInfo;
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorSpice(m));
    const t = this.typeInfo();
    this.modelInfo = (t.ngspiceModel && tables.ngspice[t.ngspiceModel]) || { name: "", variant1: "", variant2: "", pinNames: [], description: "", modelParams: [], instanceParams: [] };
    for (const p of this.modelInfo.instanceParams) if (p.category === "PRINCIPAL" || p.category === "GEOMETRY" || p.category === "FLAGS") this.addParam(p);
    for (const p of this.modelInfo.modelParams) this.addParam(p);
  }
  override getPinNames() {
    return this.modelInfo.pinNames;
  }
  /** `SIM_MODEL_NGSPICE::doFindParam`: the name, then an escaped name ("m_" for "m"). */
  override doFindParam(name: string): number {
    for (let i = 0; i < this.params.length; i++) if (iequals(this.getParam(i).info.name, name)) return i;
    for (let i = 0; i < this.params.length; i++) {
      const n = this.getParam(i).info.name;
      if (n.endsWith("_") && iequals(n, name + "_")) return i;
    }
    return -1;
  }
}

/** `SIM_MODEL_RAW_SPICE`: the element's type letter, its model or value, and a library to include; SPICE names no pins, so they are indices. */
export class SimModelRawSpice extends SimModel {
  constructor(tables: SimModelTables, spiceSource = "") {
    super(tables, "RAWSPICE", (m) => new SpiceGeneratorRawSpice(m));
    addTypeParams(this);
    this.setParamValue("model", spiceSource);
  }
  /** `SIM_MODEL_RAW_SPICE::AssignSymbolPinNumberToModelPin`: the name is an index; the pins grow to it. */
  override assignSymbolPinNumberToModelPin(modelPinName: string, symbolPinNumber: string): void {
    const index = strtol(modelPinName);
    if (index > 0) {
      while (this.modelPins.length < index) this.modelPins.push({ modelPinName: String(this.modelPins.length + 1), symbolPinNumber: "" });
      this.modelPins[index - 1]!.symbolPinNumber = symbolPinNumber;
    }
  }
  override requiresSpiceModelLine() {
    return false;
  }
}

/** `SIM_MODEL_XSPICE`: no parameters of its own here. */
export class SimModelXspice extends SimModel {
  constructor(tables: SimModelTables, type: string) {
    super(tables, type);
    addTypeParams(this);
  }
}

/** `SIM_MODEL_SUBCKT` (a library's subcircuit; the dialog's library part has its own reader). */
export class SimModelSubckt extends SimModel {
  constructor(tables: SimModelTables) {
    super(tables, "SUBCKT", (m) => new SpiceGeneratorSpice(m));
  }
}

/** `SIM_MODEL_IBIS`: GND and IN/OUT, or GND, + and - when differential; its parameters never fall back on a base model's (`GetParam`), and a
 * library's component is its source model (`SetBaseModel`), whose component name and pins it shows (`GetComponentName`, `GetIbisPins`). */
export class SimModelIbis extends SimModel {
  enableDiff = false;
  componentName = "";
  ibisPins: [string, string][] = [];
  ibisModels: string[] = [];
  sourceModel: SimModelIbis | null = null;
  constructor(tables: SimModelTables, type: string) {
    super(tables, type, (m) => new SpiceGeneratorIbis(m));
    addTypeParams(this);
    this.switchSingleEndedDiff(false);
  }
  /** `SIM_MODEL_IBIS( aType, aSource )`: a waveform's model takes the source's parameter values by name, its component, pins and models. */
  static from(tables: SimModelTables, type: string, source: SimModelIbis): SimModelIbis {
    const m = new SimModelIbis(tables, type);
    for (const p of m.params) for (let i = 0; i < source.getParamCount(); i++) if (p.info.name === source.getParam(i).info.name) p.value = source.getParam(i).value;
    m.componentName = source.componentName;
    m.ibisPins = source.getIbisPins().map((x) => [...x] as [string, string]);
    m.ibisModels = [...source.ibisModels];
    m.enableDiff = source.enableDiff;
    return m;
  }
  getIbisPins(): [string, string][] {
    return this.sourceModel ? this.sourceModel.getIbisPins() : this.ibisPins;
  }
  getComponentName(): string {
    return this.sourceModel ? this.sourceModel.getComponentName() : this.componentName;
  }
  canDifferential(): boolean {
    return this.enableDiff;
  }
  override getParam(i: number): Param {
    return this.getParamOverride(i);
  }
  override requiresSpiceModelLine() {
    return true;
  }
  override switchSingleEndedDiff(diff: boolean): void {
    this.clearPins();
    if (diff) {
      this.addPin({ modelPinName: "GND", symbolPinNumber: "1" });
      this.addPin({ modelPinName: "+", symbolPinNumber: "2" });
      this.addPin({ modelPinName: "-", symbolPinNumber: "3" });
    } else {
      this.addPin({ modelPinName: "GND", symbolPinNumber: "1" });
      this.addPin({ modelPinName: "IN/OUT", symbolPinNumber: "2" });
    }
  }
  /** `SIM_MODEL_IBIS::SetBaseModel`: "we just store these models and use the only for Spice code generation"; `m_baseModel` stays null. */
  override setBaseModel(base: SimModel): void {
    this.sourceModel = base instanceof SimModelIbis ? base : null;
  }
}

/** `SIM_LIBRARY_IBIS::ReadFile` and `InitModel` for one component, then the dialog's `CreateModel( &baseModel, pins[, fields] )`: the
 * component's model (its pins, a pin's models and the differential pins as the server read them), and the dialog's model of it -- of the
 * fields' IBIS type when given the fields (`ReadTypeFromFields`' override, "in the case of IBIS models"). */
export function ibisComponentModel(
  tables: SimModelTables,
  component: { name: string; ibisPins: [string, string][] },
  symbolPins: readonly { number: string }[],
  fields: Fields | null
): SimModelIbis {
  const base = new SimModelIbis(tables, "KIBIS_DEVICE");
  base.readDataFields(null, [{ number: "1" }, { number: "2" }]);
  base.componentName = component.name;
  base.ibisPins = component.ibisPins.map((p) => [...p] as [string, string]);
  let type = "KIBIS_DEVICE";
  if (fields) {
    const override = readTypeFromFields(tables, fields);
    if (override !== "NONE") type = override;
  }
  const m = createModel(tables, type);
  const ibis = m instanceof SimModelIbis ? m : new SimModelIbis(tables, "KIBIS_DEVICE");
  ibis.setBaseModel(base);
  try {
    ibis.readDataFields(fields, symbolPins);
  } catch (e) {
    // `SIM_MODEL::Create( aBaseModel, .., aFields, .. )` reports it ("Error reading simulation model from symbol '%s'"), which `loadLibrary`
    // does not show; the model is kept as far as it got
    if (!(e instanceof SimModelError)) throw e;
  }
  return ibis;
}

/** `SIM_MODEL::Create( TYPE )`: the class the table names for the type. */
export function createModel(tables: SimModelTables, type: string): SimModel {
  const cls = tables.type.get(type)?.class ?? "SIM_MODEL_NGSPICE";
  switch (cls) {
    case "SIM_MODEL_IDEAL":
      return new SimModelIdeal(tables, type);
    case "SIM_MODEL_R_POT":
      return new SimModelRPot(tables);
    case "SIM_MODEL_L_MUTUAL":
      return new SimModelLMutual(tables);
    case "SIM_MODEL_BEHAVIORAL":
      return new SimModelBehavioral(tables, type);
    case "SIM_MODEL_TLINE":
      return new SimModelTline(tables, type);
    case "SIM_MODEL_SWITCH":
      return new SimModelSwitch(tables, type);
    case "SIM_MODEL_SOURCE":
      return new SimModelSource(tables, type);
    case "SIM_MODEL_SUBCKT":
      return new SimModelSubckt(tables);
    case "SIM_MODEL_XSPICE":
      return new SimModelXspice(tables, type);
    case "SIM_MODEL_IBIS":
      return new SimModelIbis(tables, type);
    case "SIM_MODEL_RAW_SPICE":
      return new SimModelRawSpice(tables);
    default:
      return new SimModelNgspice(tables, type);
  }
}

/** `SIM_MODEL::ReadTypeFromFields`: the first type (in `TYPE_ITERATOR` order) whose `Sim.Type` and device's `Sim.Device` are the fields', or
 * NONE with KiCad's message ("No simulation model definition found for symbol '%s'." / "No simulation model definition found.") when a
 * `Sim.Type` is given that matches none. */
export function readTypeFromFields(tables: SimModelTables, fields: Fields | null, report?: (message: string) => void): string {
  const device = getFieldValue(fields, SIM_DEVICE_FIELD);
  const type = getFieldValue(fields, SIM_DEVICE_SUBTYPE_FIELD);
  if (device !== "") {
    for (const t of tables.types) if (type === t.fieldValue && device === tables.device.get(t.deviceType)?.fieldValue) return t.name;
  }
  if (type === "") return "NONE";
  const ref = getFieldValue(fields, SIM_REFERENCE_FIELD);
  report?.(ref !== "" ? `No simulation model definition found for symbol '${ref}'.` : "No simulation model definition found.");
  return "NONE";
}

/** `SIM_MODEL::Create( TYPE, aPins, .. )`: a type's model with empty fields. */
export function createModelOfType(tables: SimModelTables, type: string, symbolPins: readonly { number: string }[]): SimModel {
  const m = createModel(tables, type);
  m.readDataFields(null, symbolPins);
  return m;
}

/** `SIM_MODEL::Create( aFields, aResolve = true, .., aPins, aReporter )` as the dialog's `m_builtinModelsMgr.CreateModel( m_fields, true, .. )`
 * makes the current model: the fields' type read and the model given them. A field that does not parse is reported, and the model falls back
 * to a raw SPICE element of the `Sim.Params` (or Value) text, as `aResolve` asks. */
export function createModelFromFields(tables: SimModelTables, fields: Fields, symbolPins: readonly { number: string }[], report?: (message: string) => void): SimModel {
  const type = readTypeFromFields(tables, fields, report);
  let m = createModel(tables, type);
  try {
    m.readDataFields(fields, symbolPins);
  } catch (e) {
    if (!(e instanceof SimModelError)) throw e;
    // "Just because we can't parse it doesn't mean that a SPICE interpreter can't."
    let data = getFieldValue(fields, SIM_PARAMS_FIELD);
    if (data === "") data = getFieldValue(fields, SIM_VALUE_FIELD);
    m = new SimModelRawSpice(tables, data);
    try {
      m.createPins(symbolPins);
      parsePins(m, getFieldValue(fields, SIM_PINS_FIELD));
    } catch (e2) {
      if (!(e2 instanceof SimModelError)) throw e2;
      report?.(`Error reading simulation model from symbol '${getFieldValue(fields, SIM_REFERENCE_FIELD)}':\n${e2.message}`);
    }
  }
  return m;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// SIM_MODEL_SERIALIZER
// ---------------------------------------------------------------------------------------------------------------------------------------------

export function generateDevice(m: SimModel): string {
  return m.deviceInfo().fieldValue;
}

export function generateDeviceSubtype(m: SimModel): string {
  return m.typeInfo().fieldValue;
}

/** `GenerateValue`: the primary parameter's own value, else the device's letter. */
export function generateValue(m: SimModel): string {
  const v = m.getParamOverride(0).value;
  return v === "" ? m.deviceInfo().fieldValue : v;
}

/** `GenerateParams`: "name=value" of every parameter with a value of its own (the primary one even when empty, unless it is stored in the
 * Value field, then never), not the base model's, not an enum at its default. */
export function generateParams(m: SimModel): string {
  const out: string[] = [];
  for (let i = 0; i < m.getParamCount(); i++) {
    if (i === 0 && m.isStoredInValue()) continue;
    const p = m.getParamOverride(i);
    if (p.value === "" && !(i === 0 && m.hasPrimaryValue() && !m.isStoredInValue())) continue;
    if (m.baseModel && m.baseModel.getParam(i).value === p.value) continue;
    if (p.info.enumValues.length >= 1 && p.value === p.info.defaultValue) continue;
    const pair = generateParamValuePair(p);
    if (pair === "") continue;
    out.push(pair);
  }
  return out.join(" ");
}

/** `generateParamValuePair`: an escaped model name ("m_") without its "_"; a flag is its name when "1"; an empty value or one with a space is
 * quoted. */
export function generateParamValuePair(p: Param): string {
  let name = p.info.name;
  if (name.endsWith("_")) name = name.slice(0, -1);
  let value = p.value;
  if (p.info.category === "FLAGS") return value === "1" ? name : "";
  if (value === "" || value.includes(" ")) value = `"${value}"`;
  return `${name}=${value}`;
}

/** `GeneratePins`: "symbolPin=modelPin" sorted by the symbol pin (`StrNumCmp`, without case), the unconnected left out. */
export function generatePins(m: SimModel): string {
  return [...m.getPins()]
    .sort((a, b) => strNumCmp(a.symbolPinNumber, b.symbolPinNumber, true))
    .filter((p) => p.symbolPinNumber !== "")
    .map((p) => `${p.symbolPinNumber}=${p.modelPinName}`)
    .join(" ");
}

export function generateEnable(m: SimModel): string {
  return m.isEnabled ? "" : "0";
}

/** PEGTL's `space`. */
const isSpace = (c: string | undefined) => c !== undefined && " \t\n\r\v\f".includes(c);
const isAlnum = (c: string | undefined) => c !== undefined && /[A-Za-z0-9]/.test(c);

/** A PEGTL `parse_error`'s `what()`: "source:line:column: message". */
function pegError(source: string, text: string, at: number, message: string): SimModelError {
  const before = text.slice(0, at);
  const line = before.split("\n").length;
  const column = at - (before.lastIndexOf("\n") + 1) + 1;
  return new SimModelError(`${source}:${line}:${column}: ${message}`);
}

/** `SIM_VALUE_GRAMMAR::number< TYPE_FLOAT, SI >` from `i`: the end of the longest match (it may match nothing). */
function matchSiNumber(s: string, i: number): number {
  const m = /^(?:[+-]?\d+\.\d+|[+-]?\d+\.|[+-]?\d+|\.\d+|\.|-)?(?:[eE][+-]?\d*)?[afpnumkKMGTPE]?/.exec(s.slice(i));
  return i + (m ? m[0].length : 0);
}

/** `ParseValue` (`fieldInferValueGrammar`: blanks, one of R C L V I or an SI number, blanks, the end): a number is the primary parameter's value;
 * then the primary parameter is stored in the Value field. */
export function parseValue(m: SimModel, value: string): void {
  const source = "Value field";
  let i = 0;
  while (isSpace(value[i])) i++;
  let number = "";
  if ("RCLVI".includes(value[i] ?? "\0") && value[i] !== undefined) i++;
  else {
    const end = matchSiNumber(value, i);
    number = value.slice(i, end);
    i = end;
  }
  while (isSpace(value[i])) i++;
  if (i !== value.length) throw pegError(source, value, i, "expected end of string");
  if (number !== "") m.setParamValue(0, number);
  m.setIsStoredInValue(true);
}

/** `ParseParams` (`fieldParamValuePairsGrammar`): `name=value` pairs (a value quoted, or up to the next blank) and the flags `off`, `thermal`,
 * `xpart`, each set on the model in SI notation. True when the model has no primary value, fills itself in, or the primary value was given. */
export function parseParams(m: SimModel, params: string): boolean {
  const source = "Sim.Params field";
  const pairs: { kind: "flag" | "value"; name: string; value: string }[] = [];
  let i = 0;
  const sep = () => {
    const j = i;
    while (isSpace(params[i])) i++;
    return i > j;
  };
  /** One `fieldParamValuePair` at `i`, or null (nothing consumed) when none starts there. */
  const pair = (): (typeof pairs)[number] | null => {
    for (const flag of ["off", "thermal", "xpart"]) {
      if (params.slice(i, i + flag.length).toLowerCase() === flag && !isAlnum(params[i + flag.length])) {
        const token = params.slice(i, i + flag.length);
        i += flag.length;
        return { kind: "flag", name: token, value: "1" };
      }
    }
    const id = /^[A-Za-z_][A-Za-z0-9_]*/.exec(params.slice(i));
    if (!id) return null;
    const name = id[0];
    i += name.length;
    // if_must: past the name, the rest must follow
    sep();
    if (params[i] !== "=") throw pegError(source, params, i, "expected '='");
    i++;
    sep();
    if (params[i] === '"') {
      const close = params.indexOf('"', i + 1);
      if (close >= 0) {
        const value = params.slice(i + 1, close);
        i = close + 1;
        return { kind: "value", name, value };
      }
    }
    const j = i;
    while (i < params.length && !isSpace(params[i])) i++;
    if (i === j) throw pegError(source, params, i, "expected quoted or unquoted string");
    return { kind: "value", name, value: params.slice(j, i) };
  };
  sep();
  const first = pair();
  if (first) {
    pairs.push(first);
    for (;;) {
      const save = i;
      if (!sep()) break;
      const next = pair();
      if (!next) {
        i = save;
        break;
      }
      pairs.push(next);
    }
  }
  sep();
  if (i !== params.length) throw pegError(source, params, i, first ? "expected end of string" : "expected end of string");
  let isPrimaryValueSet = false;
  for (const p of pairs) {
    if (p.kind === "flag") {
      m.setParamValue(p.name, "1", "SI");
      continue;
    }
    m.setParamValue(p.name, p.value, "SI");
    if (iequals(m.getParam(0).info.name, p.name)) isPrimaryValueSet = true;
  }
  return !m.hasPrimaryValue() || m.hasAutofill() || isPrimaryValueSet;
}

/** `ParsePins` (`pinSequenceGrammar`): "symbolPin=modelPin" pairs, each model pin (by name, else by index) put on its symbol pin. */
export function parsePins(m: SimModel, pins: string): void {
  if (pins === "") return;
  const source = "Sim.Pins field";
  let i = 0;
  const out: [string, string][] = [];
  const sep = () => {
    const j = i;
    while (isSpace(pins[i])) i++;
    return i > j;
  };
  const assignment = (): [string, string] | null => {
    const j = i;
    while (i < pins.length && !isSpace(pins[i]) && pins[i] !== "=") i++;
    if (i === j || pins[i] !== "=") {
      i = j;
      return null;
    }
    const sym = pins.slice(j, i);
    i++;
    const k = i;
    while (i < pins.length && !isSpace(pins[i])) i++;
    if (i === k) {
      i = j;
      return null;
    }
    return [sym, pins.slice(k, i)];
  };
  sep();
  const first = assignment();
  if (first) {
    out.push(first);
    for (;;) {
      const save = i;
      if (!sep()) break;
      const next = assignment();
      if (!next) {
        i = save;
        break;
      }
      out.push(next);
    }
  }
  sep();
  if (i !== pins.length) throw pegError(source, pins, i, "expected end of string");
  for (const [sym, model] of out) m.assignSymbolPinNumberToModelPin(model, sym);
}

/** `ParseEnable`: "n...", "f..." or "0..." turn the model off. */
export function parseEnable(m: SimModel, enable: string): void {
  if (enable === "") return;
  const c = enable.toLowerCase()[0];
  if (c === "n" || c === "f" || c === "0") m.isEnabled = false;
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// SPICE_GENERATOR
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** `boost::trim_copy`. */
const trim = (s: string) => s.replace(/^\s+|\s+$/g, "");

/** `SPICE_GENERATOR`. */
export class SpiceGenerator {
  constructor(readonly model: SimModel) {}

  /** `ModelName`: `__REF` for a built-in model, `REF.NAME` for a library model with overrides, else the library's name. */
  modelName(item: SpiceItem): string {
    if (item.baseModelName === "") return `__${item.refName}`;
    if (this.model.requiresSpiceModelLine(item)) return `${item.refName}.${item.baseModelName}`;
    return item.baseModelName;
  }

  /** `ModelLine`: ".model NAME TYPE [level=..] [version=..]" and a "+" line per parameter with a value (flags by name); nothing for an empty
   * model, or when no line is needed. */
  modelLine(item: SpiceItem): string {
    const m = this.model;
    if (!m.requiresSpiceModelLine(item)) return "";
    let result = `.model ${item.modelName} `;
    const indentLength = result.length;
    const spice = m.spiceInfo();
    result += spice.modelType;
    if (!spice.isDefaultLevel && spice.level !== "") result += ` level=${spice.level}`;
    if (spice.version !== "") result += ` version=${spice.version}`;
    result += "\n";
    for (let i = 0; i < m.getParamCount(); i++) {
      const p = m.getParam(i);
      if (p.info.isSpiceInstanceParam) continue;
      let name: string;
      if (p.info.spiceModelName !== "") name = p.info.spiceModelName;
      else name = p.info.name.endsWith("_") ? p.info.name.slice(0, -1) : p.info.name;
      const value = toSpice(p.value);
      if (value === "") continue;
      if (p.info.category === "FLAGS") {
        if (value === "1") result += `+${name}\n`;
      } else result += `+${" ".repeat(indentLength - 1)}${name}=${value}\n`;
    }
    // "Don't send SPICE empty models."
    if (result.length === indentLength + 1) result = "";
    return result;
  }

  /** `ItemLine`: name, pins, model name, instance parameters. KiCad fills a copy's pin numbers and net names when the item has none, but hands
   * the original item on, so an item without pins (the dialog's Code tab) writes each model pin as "NC-<ref>-<n>". */
  itemLine(item: SpiceItem): string {
    return `${this.itemName(item)}${this.itemPins(item)}${this.itemModelName(item)}${this.itemParams()}\n`;
  }

  /** `ItemName`: the reference when it starts with the item's letter, else the letter and the reference. */
  itemName(item: SpiceItem): string {
    const letter = this.model.spiceInfo().itemType;
    if (item.refName !== "" && item.refName.startsWith(letter)) return item.refName;
    return `${letter}${item.refName}`;
  }

  /** `ItemPins`: each model pin's net, "NC-<ref>-<n>" for a pin on no symbol pin of the item. */
  itemPins(item: SpiceItem): string {
    let out = "";
    let nc = 0;
    for (const pin of this.getPins()) {
      const k = item.pinNumbers.indexOf(pin.symbolPinNumber);
      if (k < 0) out += ` NC-${item.refName}-${nc++}`;
      else out += ` ${item.pinNetNames[k]}`;
    }
    return out;
  }

  itemModelName(item: SpiceItem): string {
    return ` ${item.modelName}`;
  }

  /** `ItemParams`: " name=value" of each instance parameter with a value (a flag by its name when "1"). */
  itemParams(): string {
    let out = "";
    const m = this.model;
    for (let i = 0; i < m.getParamCount(); i++) {
      const p = m.getParam(i);
      if (!p.info.isSpiceInstanceParam) continue;
      const name = p.info.spiceInstanceName === "" ? p.info.name : p.info.spiceInstanceName;
      const value = toSpice(p.value);
      if (p.info.category === "FLAGS") {
        if (value === "1") out += ` ${name}`;
      } else if (value !== "") out += ` ${name}=${value}`;
    }
    return out;
  }

  getPins(): SimModelPin[] {
    return this.model.getPins();
  }

  /** `Preview` (the Code tab): the model line, a blank line, the item line, trimmed. */
  preview(item: SpiceItem): string {
    let code = this.modelLine(item);
    const itemLine = this.itemLine(item);
    if (code !== "") code += "\n";
    code += itemLine;
    return trim(code);
  }
}

/** `SPICE_GENERATOR_IDEAL`: no model line; the value is the item's model name, and no value no line. */
export class SpiceGeneratorIdeal extends SpiceGenerator {
  override modelLine(): string {
    return "";
  }
  override itemLine(item: SpiceItem): string {
    const modelName = toSpice(this.model.getParam(0).value);
    return modelName !== "" ? super.itemLine({ ...item, modelName }) : "";
  }
}

/** `SPICE_GENERATOR_R_POT`. */
export class SpiceGeneratorRPot extends SpiceGenerator {
  override modelLine(item: SpiceItem): string {
    let r = "0";
    let position = "";
    const rp = this.model.findParam("r");
    if (rp) r = toSpice(rp.value);
    const pos = this.model.findParam("pos");
    if (pos) position = toSpice(pos.value);
    if (position !== "") return `.model ${item.modelName} potentiometer( r=${r} position=${position} )\n`;
    return `.model ${item.modelName} potentiometer( r=${r} )\n`;
  }
}

/** `SPICE_GENERATOR_L_MUTUAL`: the instance parameters by position. */
export class SpiceGeneratorLMutual extends SpiceGenerator {
  override itemParams(): string {
    let out = "";
    for (let i = 0; i < this.model.getParamCount(); i++) {
      const p = this.model.getParam(i);
      if (!p.info.isSpiceInstanceParam) continue;
      out += " " + toSpice(p.value);
    }
    return out;
  }
}

/** `SPICE_GENERATOR_BEHAVIORAL`: the expression as the item's value, "V=" / "I=" for a source. */
export class SpiceGeneratorBehavioral extends SpiceGenerator {
  override modelLine(): string {
    return "";
  }
  override itemLine(item: SpiceItem): string {
    const v = toSpice(this.model.getParam(0).value);
    switch (this.model.type) {
      case "R_BEHAVIORAL":
      case "C_BEHAVIORAL":
      case "L_BEHAVIORAL":
        return super.itemLine({ ...item, modelName: v });
      case "V_BEHAVIORAL":
        return super.itemLine({ ...item, modelName: `V=${v}` });
      case "I_BEHAVIORAL":
        return super.itemLine({ ...item, modelName: `I=${v}` });
      default:
        return "";
    }
  }
}

/** `SPICE_GENERATOR_TLINE`: an LTRA model of the line's L and C per length (from Z0 and the delay), or its RLGC. */
export class SpiceGeneratorTline extends SpiceGenerator {
  override modelLine(item: SpiceItem): string {
    let r = "0";
    let l = "0";
    let g = "0";
    let c = "0";
    let len = "1";
    const m = this.model;
    switch (m.type) {
      case "TLINE_Z0": {
        const z0 = toDouble(m.findParam("z0")!.value);
        const td = toDouble(m.findParam("td")!.value);
        if (Number.isNaN(z0) || Number.isNaN(td)) return `.model ${item.modelName} LTRA()\n`;
        l = formatG(td * z0);
        c = formatG(td / z0);
        break;
      }
      case "TLINE_RLGC": {
        const f = (n: string, d: string) => {
          const p = m.findParam(n);
          return p ? toSpice(p.value) : d;
        };
        r = f("r", r);
        l = f("l", l);
        g = f("g", g);
        c = f("c", c);
        len = f("len", len);
        break;
      }
      default:
        return "";
    }
    return `.model ${item.modelName} LTRA( r=${r} l=${l} g=${g} c=${c} len=${len} )\n`;
  }
}

/** `SPICE_GENERATOR_SWITCH`: the output pins then the controlling ones; a current switch measures its current through a voltage source named
 * "V__<ref>"; the initial state ("ic") is positional. */
export class SpiceGeneratorSwitch extends SpiceGenerator {
  override itemLine(item: SpiceItem): string {
    switch (this.model.type) {
      case "SW_V":
        return super.itemLine(item);
      case "SW_I": {
        const vsource = `V__${item.refName}`;
        if (item.pinNetNames.length < 2) return ""; // wxCHECK_MSG( .., "Missing two pin net names for SW_I" )
        let out = `${item.pinNetNames[0]} ${item.pinNetNames[1]} 0\n`;
        out += super.itemLine({ ...item, modelName: `${vsource} ${item.modelName}` });
        return out;
      }
      default:
        return "";
    }
  }
  override itemParams(): string {
    let out = "";
    for (let i = 0; i < this.model.getParamCount(); i++) {
      const p = this.model.getParam(i);
      if (!p.info.isSpiceInstanceParam) continue;
      if (p.value !== "none") out += " " + p.value;
    }
    return out;
  }
  override getPins(): SimModelPin[] {
    const m = this.model;
    switch (m.type) {
      case "SW_V":
        return [m.getPin(2), m.getPin(3), m.getPin(0), m.getPin(1)];
      case "SW_I":
        return [m.getPin(2), m.getPin(3)];
      default:
        return [];
    }
  }
}

/** `SPICE_GENERATOR_SOURCE`: "DC x", the waveform function with its arguments, "AC mag phase", the S-parameter port; no model line. */
export class SpiceGeneratorSource extends SpiceGenerator {
  override modelLine(): string {
    return "";
  }

  private paramValueString(name: string, def: string): string {
    const p = this.model.findParam(name);
    let r = p ? toSpice(p.value) : "";
    if (r === "") r = def;
    return r;
  }

  override itemLine(item: SpiceItem): string {
    const m = this.model;
    const acParam = m.findParam("ac");
    const dcParam = m.findParam("dc");
    const ac = acParam ? toSpice(acParam.value) : "";
    const dc = dcParam ? toSpice(dcParam.value) : "";
    let emptyLine = true;
    let modelName = "";
    if (dc !== "") {
      emptyLine = false;
      modelName += `DC ${dc} `;
    }
    const fn = m.spiceInfo().functionName;
    if (fn !== "" && m.type !== "V" && m.type !== "I") {
      let args = "";
      const v = (n: string, d: string) => this.paramValueString(n, d);
      switch (m.type) {
        case "V_PWL":
        case "I_PWL":
          args = formatPwlValues(m.getParam(0).value);
          break;
        case "V_WHITENOISE":
        case "I_WHITENOISE":
          args += v("rms", "0") + " " + v("dt", "0") + " " + "0 0 0 0 0 ";
          break;
        case "V_PINKNOISE":
        case "I_PINKNOISE":
          args += "0 " + v("dt", "0") + " " + v("slope", "0") + " " + v("rms", "0") + " " + "0 0 0 ";
          break;
        case "V_BURSTNOISE":
        case "I_BURSTNOISE":
          args += "0 0 0 0 " + v("ampl", "0") + " " + v("tcapt", "0") + " " + v("temit", "0") + " ";
          break;
        case "V_RANDUNIFORM":
        case "I_RANDUNIFORM":
          args += "1 " + v("ts", "0") + " " + v("td", "0") + " " + v("range", "1") + " " + v("offset", "0") + " ";
          break;
        case "V_RANDGAUSSIAN":
        case "I_RANDGAUSSIAN":
          args += "2 " + v("ts", "0") + " " + v("td", "0") + " " + v("stddev", "1") + " " + v("mean", "0") + " ";
          break;
        case "V_RANDEXP":
        case "I_RANDEXP":
          args += "3 " + v("ts", "0") + " " + v("td", "0") + " " + v("mean", "1") + " " + v("offset", "0") + " ";
          break;
        case "V_RANDPOISSON":
        case "I_RANDPOISSON":
          args += "4 " + v("ts", "0") + " " + v("td", "0") + " " + v("lambda", "1") + " " + v("offset", "0") + " ";
          break;
        default: {
          for (let i = 0; i < m.getParamCount(); i++) {
            const p = m.getParam(i);
            if (ac !== "" && (iequals(p.info.name, "ac") || iequals(p.info.name, "ph"))) continue;
            const a = toSpice(p.value);
            if (a !== "") args += a + " ";
          }
          // "ngspice 46+ changed the PULSE PER default from TSTOP to TR+TF+PW": a single pulse unless PER or NP is given
          if (m.type === "V_PULSE" || m.type === "I_PULSE") {
            const per = m.findParam("per");
            const np = m.findParam("np");
            const perSet = !!per && toSpice(per.value) !== "";
            const npSet = !!np && toSpice(np.value) !== "";
            if (!perSet && !npSet) args += "0 1 ";
          }
          break;
        }
      }
      emptyLine = false;
      modelName += `${fn}( ${args}) `;
    } else {
      switch (m.type) {
        case "V_VCL":
        case "I_VCL":
          modelName += `${this.paramValueString("gain", "1.0")} `;
          emptyLine = false;
          break;
        case "V_CCL":
        case "I_CCL":
          modelName += `${this.paramValueString("control", "V?")} ${this.paramValueString("gain", "1.0")} `;
          emptyLine = false;
          break;
        default:
          break;
      }
    }
    if (ac !== "") {
      const ph = m.findParam("ph");
      emptyLine = false;
      modelName += `AC ${ac} ${ph ? toSpice(ph.value) : ""} `;
    }
    const portnumParam = m.findParam("portnum");
    const portnum = portnumParam ? toSpice(portnumParam.value) : "";
    if (portnum !== "") {
      modelName += `portnum ${portnum} `;
      const z0Param = m.findParam("z0");
      const z0 = z0Param ? toSpice(z0Param.value) : "";
      if (z0 !== "") modelName += `z0 ${z0} `;
    }
    if (emptyLine) modelName = toSpice(m.getParam(0).value);
    return super.itemLine({ ...item, modelName });
  }
}

const isWordChar = (c: string | undefined) => c !== undefined && /[A-Za-z0-9_]/.test(c);
const isDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
const isAlpha = (c: string | undefined) => c !== undefined && /[A-Za-z]/.test(c);

/** `rewriteBraceExpression`: the numbers in a `{...}` expression written in SPICE notation, the rest as it is. */
export function rewriteBraceExpression(brace: string): string {
  let out = "";
  let pos = 0;
  while (pos < brace.length) {
    const c = brace[pos]!;
    const prevIsWord = pos > 0 && isWordChar(brace[pos - 1]);
    const digitStart = isDigit(c);
    const dotDigitStart = c === "." && isDigit(brace[pos + 1]);
    if (prevIsWord || (!digitStart && !dotDigitStart)) {
      out += c;
      pos++;
      continue;
    }
    const start = pos;
    while (isDigit(brace[pos])) pos++;
    if (brace[pos] === ".") {
      pos++;
      while (isDigit(brace[pos])) pos++;
    }
    if (brace[pos] === "e" || brace[pos] === "E") {
      const expStart = pos++;
      if (brace[pos] === "+" || brace[pos] === "-") pos++;
      const expDigits = pos;
      while (isDigit(brace[pos])) pos++;
      if (pos === expDigits) pos = expStart;
    }
    if (pos < brace.length && "afpnumkKMGTPE".includes(brace[pos]!) && (pos + 1 >= brace.length || !isAlpha(brace[pos + 1]))) pos++;
    out += toSpice(brace.slice(start, pos));
  }
  return out;
}

/** `formatPwlValues`: the PWL points split on blanks (not inside braces), each in SPICE notation, each followed by a blank. */
export function formatPwlValues(input: string): string {
  let out = "";
  let pos = 0;
  const space = (c: string | undefined) => c !== undefined && /\s/.test(c);
  while (pos < input.length) {
    while (pos < input.length && space(input[pos])) pos++;
    if (pos >= input.length) break;
    const start = pos;
    let depth = 0;
    while (pos < input.length && (depth > 0 || !space(input[pos]))) {
      if (input[pos] === "{") depth++;
      else if (input[pos] === "}" && depth > 0) depth--;
      pos++;
    }
    const token = input.slice(start, pos);
    out += token[0] === "{" ? rewriteBraceExpression(token) : toSpice(token);
    out += " ";
  }
  return out;
}

/** `SPICE_GENERATOR_SPICE` (and `SPICE_GENERATOR_NGSPICE`): the Code tab's preview keeps the model's own SPICE when it writes no model line, and
 * the item has no reference. */
export class SpiceGeneratorSpice extends SpiceGenerator {
  override preview(item: SpiceItem): string {
    const code = this.modelLine(item);
    const itemLine = this.itemLine({ ...item, refName: "" });
    let out = code;
    if (out !== "" && itemLine !== "") out += "\n\n";
    out += itemLine;
    return trim(out);
  }
}

/** `SPICE_GENERATOR_RAW_SPICE`: the element's own letter, the symbol's pins when the model names none, the "model" text. */
export class SpiceGeneratorRawSpice extends SpiceGenerator {
  override modelLine(): string {
    return "";
  }
  override itemName(item: SpiceItem): string {
    const type = this.model.getParam(0).value;
    if (item.refName !== "" && item.refName.startsWith(type)) return item.refName;
    return `${type}${item.refName}`;
  }
  override itemPins(item: SpiceItem): string {
    let out = "";
    let nc = 0;
    const pins = this.getPins();
    if (pins.length > 0) {
      for (const pin of pins) {
        const k = item.pinNumbers.indexOf(pin.symbolPinNumber);
        if (k >= 0) out += ` ${item.pinNetNames[k]}`;
        else out += ` NC-${item.refName}-${nc++}`;
      }
    } else for (const n of item.pinNetNames) out += ` ${n}`;
    return out;
  }
  override itemModelName(): string {
    return "";
  }
  override itemParams(): string {
    let out = "";
    for (let i = 0; i < this.model.getParamCount(); i++) {
      const p = this.model.getParam(i);
      if (!p.info.isSpiceInstanceParam) continue;
      if (p.info.name === "model") out += " " + toSpice(p.value);
    }
    return out;
  }
  override preview(): string {
    return this.itemParams();
  }
}

/** `SPICE_GENERATOR_IBIS`: the model is the KIBIS subcircuit kicad-cli writes (`IbisDevice`, not ported); no model line here. */
export class SpiceGeneratorIbis extends SpiceGenerator {
  override modelName(item: SpiceItem): string {
    return `${item.refName}.${item.baseModelName}`;
  }
  override modelLine(): string {
    return "";
  }
}

// ---------------------------------------------------------------------------------------------------------------------------------------------
// InferSimModel
// ---------------------------------------------------------------------------------------------------------------------------------------------

/** What `InferSimModel` gives a passive or a DC source whose fields say nothing (or only its device): `Sim.Device`, `Sim.Type`, `Sim.Params`
 * and `Sim.Pins`. */
export interface Inferred {
  deviceType: string;
  modelType: string;
  modelParams: string;
  pinMap: string;
}

/** `convertSeparators`: a "," or "." is a decimal point or a thousands separator by where it is; `false` when it can be neither. */
export function convertSeparators(mantissa: string): string | false {
  let m = mantissa.replace(/ /g, "");
  let ambiguous = "?";
  let thousands = "?";
  let thousandsFound = false;
  let decimal = "?";
  let decimalFound = false;
  let digits = 0;
  for (let ii = m.length - 1; ii >= 0; ii--) {
    const c = m[ii]!;
    if (c >= "0" && c <= "9") digits++;
    else if (c === "." || c === ",") {
      if (decimal !== "?" || thousands !== "?") {
        if (c === decimal) {
          if (thousandsFound) return false;
          else if (decimalFound) return false;
          else decimalFound = true;
        } else if (c === thousands) {
          if (digits !== 3) return false;
          else thousandsFound = true;
        }
      } else if (ambiguous !== "?") {
        if (c === ambiguous) {
          thousands = ambiguous;
          thousandsFound = true;
          decimal = c === "." ? "," : ".";
        } else {
          decimal = ambiguous;
          decimalFound = true;
          thousands = c;
          thousandsFound = true;
        }
      } else if ((ii === 1 && m[0] === "0") || digits !== 3) {
        decimal = c;
        decimalFound = true;
        thousands = c === "." ? "," : ".";
      } else ambiguous = c;
      digits = 0;
    } else digits = 0;
  }
  if (decimal === "?" && thousands === "?") {
    decimal = ".";
    thousands = ",";
  }
  m = m.split(thousands).join("");
  m = m.split(decimal).join(".");
  return m;
}

/** `convertNotation` (InferSimModel's): the two micro signs are "u"; in SI notation "Meg" (any case) is "M", in SPICE "M" is "Meg". */
function convertInferredNotation(units: string, notation: Notation): string {
  if (units === "µ" || units === "μ") return "u";
  if (notation === "SPICE") {
    if (units === "M") return "Meg";
  } else if (units !== "" && units[0]!.toUpperCase() + units.slice(1).toLowerCase() === "Meg") return "M";
  return units;
}

// The value patterns, as sim_model.cpp writes them (the micro and ohm signs are code points of the source, the mathematical ones astral).
const IDEAL_VAL = /^([0-9,. ]+)([fFpPnNuUmMkKgGtTμµ\u{1d6cd}\u{1d707}\u{1d741} ]|M(e|E)(g|G))?([fFhHΩΩ\u{1d6c0}\u{1d6fa}\u{1d76e}rR]|ohm)?([-1-9 ]*)([fFhHΩΩ\u{1d6c0}\u{1d6fa}\u{1d76e}rR]|ohm)?$/u;
const SOURCE_VAL = /^([0-9,. ]+)([fFpPnNuUmMkKgGtTμµ\u{1d6cd}\u{1d707}\u{1d741} ]|M(e|E)(g|G))?([vVaA])?([-1-9 ]*)([vVaA])?$/u;

/** `SIM_MODEL::InferSimModel`: a two-pin symbol with no model of its own (or only "R", "L", "C", "V", "I" as its device) is given one from its
 * prefix and Value: an ideal passive of the value ("4k7" -> r="4.7k"), a behavioural one when the value is not a number, a DC (or "AC ...")
 * source. Null when nothing is inferred. `pins` sorted by number as KiCad sorts them (`<`). */
export function inferSimModel(prefix: string, fields: Fields, pins: readonly { number: string }[], notation: Notation = "SI"): Inferred | null {
  const library = getFieldValue(fields, SIM_LIBRARY_FIELD);
  const modelName = getFieldValue(fields, SIM_NAME_FIELD);
  let value = getFieldValue(fields, SIM_VALUE_FIELD);
  const sorted = [...pins].sort((a, b) => (a.number < b.number ? -1 : a.number > b.number ? 1 : 0));
  let deviceType = getFieldValue(fields, SIM_DEVICE_FIELD);
  let modelType = getFieldValue(fields, SIM_DEVICE_SUBTYPE_FIELD);
  let modelParams = getFieldValue(fields, SIM_PARAMS_FIELD);
  let pinMap = getFieldValue(fields, SIM_PINS_FIELD);
  if (sorted.length !== 2) return null;
  const letter = prefix.slice(0, 1);
  const startsRLC = prefix.startsWith("R") || prefix.startsWith("L") || prefix.startsWith("C");
  if (
    ((deviceType === "R" || deviceType === "L" || deviceType === "C") && modelType === "") ||
    (library === "" && modelName === "" && deviceType === "" && modelType === "" && value !== "" && startsRLC)
  ) {
    if (modelParams === "") {
      const m = IDEAL_VAL.exec(value);
      if (m) {
        const mantissa = convertSeparators(m[1] ?? "");
        if (mantissa === false) return null;
        const exponent = m[2] ?? "";
        const fraction = m[6] ?? "";
        if (mantissa.includes(".") || fraction === "") modelParams = `${letter.toLowerCase()}="${mantissa}${convertInferredNotation(exponent, notation)}"`;
        else modelParams = `${letter.toLowerCase()}="${mantissa}.${fraction}${convertInferredNotation(exponent, notation)}"`;
      } else {
        modelType = "=";
        modelParams = `${letter.toLowerCase()}="${value}"`;
      }
    }
    if (deviceType === "") deviceType = letter;
    if (pinMap === "") pinMap = `${sorted[0]!.number}=+ ${sorted[1]!.number}=-`;
    return { deviceType, modelType, modelParams, pinMap };
  }
  if (((deviceType === "V" || deviceType === "I") && (modelType === "" || modelType === "DC")) || (deviceType === "" && modelType === "" && value !== "" && (prefix.startsWith("V") || prefix.startsWith("I")))) {
    if (value !== "") {
      let param = "dc";
      if (value.startsWith("DC ")) value = value.slice(3);
      else if (value.startsWith("AC ")) {
        value = value.slice(3);
        param = "ac";
      }
      const m = SOURCE_VAL.exec(value);
      if (m) {
        const mantissa = convertSeparators(m[1] ?? "");
        if (mantissa === false) return null;
        const exponent = m[2] ?? "";
        const fraction = m[6] ?? "";
        if (mantissa.includes(".") || fraction === "") modelParams = `${param}="${mantissa}${convertInferredNotation(exponent, notation)}" ${modelParams}`;
        else modelParams = `${param}="${mantissa}.${fraction}${convertInferredNotation(exponent, notation)}" ${modelParams}`;
      } else modelParams = `${param}="${value}" ${modelParams}`;
    }
    if (deviceType === "") deviceType = letter;
    if (modelType === "") modelType = "DC";
    if (pinMap === "") pinMap = `${sorted[0]!.number}=+ ${sorted[1]!.number}=-`;
    return { deviceType, modelType, modelParams, pinMap };
  }
  return null;
}

/** `SCH_SYMBOL::UpdatePrefix` of a reference: trailing digits, "?" and "*" off, "~" a space, trimmed. */
export function referencePrefix(reference: string): string {
  let p = reference.replace(/~/g, " ");
  while (p.length && /[0-9?*]$/.test(p)) p = p.slice(0, -1);
  return p.trim();
}
