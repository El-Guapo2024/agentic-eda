// Port of eeschema/dialogs/dialog_sim_model.cpp (`DIALOG_SIM_MODEL`, "Simulation Model Editor", KiCad 8303b2ad): the Model tab's two halves
// and the Pin Assignments tab.
// - "SPICE model from file (*.lib, *.sub or *.ibs)": the File box and Browse, the model list with its filter, the subcircuit's code; the library
//   is read by the server (`POST /api/sim/model/library`, `eda_sim::library`; kicad-port/sim/simModel.ts). An IBIS file (`eda_sim::ibis`) lists
//   its components ("Component:"), and the Pin, Pin model, Differential and Waveform choices of `updateIbisWidgets`, `onModelNameChoice`,
//   `onPinCombobox`, `onDifferentialCheckbox` and `onWaveformChoice`, with the IBIS model's parameters.
// - "Built-in SPICE model": the Device and Device type choices, the Parameters grid and the Code tab, "Save parameter '%s (%s)' in Value field"
//   (`TransferDataToWindow`, `updateBuiltinModelWidgets`, `updateModelParamsTab`, `updateModelCodeTab`; kicad-port/sim/simModelDialog.ts over
//   the models of simModelBuiltin.ts, whose tables tools/extract-sim-models.js copied from the C++ into src/kicad/sim_models.json, loaded when
//   the dialog opens).
// - Pin Assignments: Symbol Pin and Model Pin, one grid whichever model is current (`updatePinAssignments( model, false )` keeps a row's model
//   pin index across models).
// - OK (`TransferDataFromWindow`): `Sim.Library`, `Sim.Name`, the IBIS pin, model and differential fields, then `WriteFields` -- `Sim.Device`,
//   `Sim.Type`, `Sim.Enable`, `Sim.Pins`, `Sim.Params`, and the Value when the primary parameter is saved there.
//
// Where it differs from KiCad:
// - KiCad's dialog is opened from Symbol Properties' "Simulation Model..." button and writes into that dialog's fields, which its OK commits; here
//   OK is its own undo step (one batch: the Symbol Fields Table's `set_symbol_fields`, which reaches a sheet read from a KiCad file too, and
//   `remove_user_field`), and Symbol Properties takes the new texts. A field the dialog adds is hidden (KiCad's inference adds `Sim.Device`,
//   `Sim.Type`, `Sim.Params` and `Sim.Pins` visible, `SetFieldValue`'s default).
// - A library `.model`'s parameters are not shown in the grid (its base model's values, `SPICE_MODEL_PARSER::ReadModel`); its overrides in
//   `Sim.Params` are kept for the same model and dropped for another (simModel.ts `fieldsOnOk`).
// - The grid does not evaluate arithmetic ("1k*2"); the K device's l1 / l2 are text, not a choice of the circuit's inductors.
// - Browse lists the library files of the board folder (`GET /api/sim/model/files`) instead of a file dialog; a path can be typed as in KiCad.
// - Error boxes (`DisplayErrorMessage`) are shown in the info bar.
import { useEffect, useMemo, useReducer, useState } from "react";
import type { Cmd } from "../../api/types";
import { fetchSimLibrary, fetchSimLibraryFiles, fetchSimModelSymbol, loadSimModelTables, type SimModelSymbolReply } from "../../api/simModelClient";
import { removeUserFieldCmd } from "../../kicad-port/schFieldEdit";
import {
  assignRow,
  defaultModelPins,
  fieldsOnOk,
  fieldVerbs,
  filterModelNames,
  gridModelColumn,
  initialModel,
  modelPinChoices,
  modelPinIndex,
  modelPinString,
  NOT_CONNECTED,
  parsePins,
  pinsOnOk,
  SIM_FIELDS,
  symbolPinString,
  type LibraryModelInfo,
  type ModelPin,
} from "../../kicad-port/sim/simModel";
import { ibisComponentModel, referencePrefix, type Fields, type SimModel, type SimModelIbis, type SimModelTables } from "../../kicad-port/sim/simModelBuiltin";
import {
  builtinOk,
  chooseDevice,
  chooseType,
  codeTab,
  currentBuiltin,
  deviceChoices,
  ibisWaveformModel,
  isRawSpice,
  openBuiltin,
  paramGrid,
  saveInValueLabel,
  setGridValue,
  typeChoices,
  waveformChoices,
  type OpenedDialog,
  type ParamRow,
} from "../../kicad-port/sim/simModelDialog";
import { useStudioApi } from "../../state/store";

interface Props {
  reference: string;
  onClose: () => void;
  /** After OK wrote the fields: what each simulation field (and the Value) is now ("" for one removed). */
  onWritten?: (fields: Record<string, string>) => void;
}

/** A library model as the server lists it (an IBIS component with its pins, pin models and differential pins). */
type LibModel = LibraryModelInfo;

interface Loaded {
  /** The path as typed and as stored. */
  path: string;
  stored: string;
  models: LibModel[];
  ibis: boolean;
}

/** The model tables (src/kicad/sim_models.json, 0.8 MB): fetched the first time the dialog opens (shared with the tuner's Save). */
const loadTables = loadSimModelTables;

const FIELD_ROWS = [SIM_FIELDS.library, SIM_FIELDS.name, SIM_FIELDS.device, SIM_FIELDS.type, SIM_FIELDS.enable, SIM_FIELDS.pins, SIM_FIELDS.params, "Sim.Ibis.Pin", "Sim.Ibis.Model", "Sim.Ibis.Diff"];

export function SimModelDialog({ reference, onClose, onWritten }: Props) {
  const api = useStudioApi();
  const [, bump] = useReducer((x: number) => x + 1, 0);
  const [tables, setTables] = useState<SimModelTables | null>(null);
  const [sym, setSym] = useState<SimModelSymbolReply | null>(null);
  const [opened, setOpened] = useState<OpenedDialog | null>(null);
  const [tab, setTab] = useState<"model" | "pins">("model");
  const [page, setPage] = useState<"params" | "code">("params");
  /** `m_rbLibraryModel` / `m_rbBuiltinModel` (neither: "undetermined"). */
  const [radio, setRadio] = useState<"library" | "builtin" | null>(null);
  const [pathText, setPathText] = useState("");
  const [lib, setLib] = useState<Loaded | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  /** Each SPICE library model's pins (`m_libraryModelsMgr`'s models), by name. */
  const [modelPins, setModelPins] = useState<Record<string, ModelPin[]>>({});
  /** Each IBIS component's model, by name (`m_libraryModelsMgr`'s, a waveform's model replacing it: `SetModel`). */
  const [ibisModels, setIbisModels] = useState<Record<string, SimModelIbis>>({});
  const [ibisPin, setIbisPin] = useState(-1);
  const [ibisPinModels, setIbisPinModels] = useState<string[]>([]);
  const [ibisPinModel, setIbisPinModel] = useState("");
  const [ibisDiff, setIbisDiff] = useState(false);
  /** The Pin Assignments grid's Model Pin column, row by symbol pin; `null` until it has rows. */
  const [column, setColumn] = useState<string[] | null>(null);
  const [saveInValue, setSaveInValue] = useState(false);
  const [code, setCode] = useState("");
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [files, setFiles] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);

  const fieldsOf = (s: SimModelSymbolReply): Fields => ({ Reference: s.ref, Value: s.value, ...s.fields });

  /** `TransferDataToWindow`'s IBIS part for the component it opens on: the pin the fields name, its models, the differential box. */
  const openIbisComponent = (m: LibModel, model: SimModelIbis, fields: Fields) => {
    const pins = model.getIbisPins();
    const k = pins.findIndex(([n]) => n === (fields["Sim.Ibis.Pin"] ?? ""));
    setIbisPin(k);
    if (k >= 0) {
      const models = m.pinModels?.[k] ?? [];
      model.ibisModels = [...models];
      model.enableDiff = (m.diffPins ?? []).includes(pins[k]![0]);
      setIbisPinModels(models);
      setIbisPinModel(fields["Sim.Ibis.Model"] ?? "");
    } else {
      setIbisPinModels([]);
      setIbisPinModel("");
    }
    const diff = fields["Sim.Ibis.Diff"] === "1";
    model.switchSingleEndedDiff(diff);
    setIbisDiff(diff);
  };

  /** `loadLibrary`: the models, the one the fields name (with the symbol's `Sim.Pins`) or the first, and the info bar. */
  const loadLibrary = async (path: string, from: SimModelSymbolReply, fields: Fields, t: SimModelTables) => {
    if (path.trim() === "") {
      setLib(null);
      setSelected(null);
      setInfo(null);
      return;
    }
    const r = await fetchSimLibrary(path);
    setPathText(path);
    setRadio("library");
    if (!r.ok) {
      setLib(null);
      setSelected(null);
      setInfo(r.messages.join("\n"));
      return;
    }
    const simName = fields[SIM_FIELDS.name] ?? "";
    let message: string | null = null;
    const pins: Record<string, ModelPin[]> = {};
    const ibis: Record<string, SimModelIbis> = {};
    for (const m of r.models) {
      if (r.ibis) {
        // `CreateModel( &baseModel, m_sortedPartPins[, m_fields] )`: the fields go to the model they name
        ibis[m.name] = ibisComponentModel(t, { name: m.name, ibisPins: m.ibisPins ?? [] }, from.pins, m.name === simName ? fields : null);
        continue;
      }
      let p = defaultModelPins(m, from.pins);
      if (m.name === simName && r.path === (from.fields[SIM_FIELDS.library] ?? "")) {
        try {
          p = parsePins(p, from.fields[SIM_FIELDS.pins] ?? "");
        } catch (e) {
          message = `Error reading simulation model from symbol '${from.ref}':\n${(e as Error).message}`;
        }
      }
      if (!(m.name in pins)) pins[m.name] = p;
    }
    const first = initialModel(r.models.map((m) => m.name), simName, path);
    setModelPins(pins);
    setIbisModels(ibis);
    setLib({ path, stored: r.path, models: r.models, ibis: !!r.ibis });
    setSelected(first.name);
    setInfo(message ?? first.message);
    if (r.ibis && first.name) openIbisComponent(r.models.find((m) => m.name === first.name)!, ibis[first.name]!, fields);
    const firstPins = first.name ? (r.ibis ? ibis[first.name]!.getPins() : pins[first.name] ?? []) : [];
    // `updatePinAssignments( model, false )`: a grid without rows is filled from the model
    setColumn((c) => c ?? (first.name ? gridModelColumn(firstPins, from.pins) : null));
  };

  useEffect(() => {
    let stopped = false;
    void Promise.all([fetchSimModelSymbol(reference), loadTables()]).then(async ([s, t]) => {
      if (stopped) return;
      if (!s.ok) {
        setInfo(s.message ?? "The symbol could not be read.");
        return;
      }
      const o = openBuiltin(t, referencePrefix(s.ref), fieldsOf(s), s.pins);
      setTables(t);
      setSym(s);
      setOpened(o);
      setRadio(o.radio);
      setSaveInValue(o.saveInValue);
      if (o.errors.length) setInfo(o.errors.join("\n\n"));
      if (o.radio === "library") await loadLibrary(o.fields[SIM_FIELDS.library] ?? "", s, o.fields, t);
      else {
        const m = currentBuiltin(o.builtin);
        setColumn(gridModelColumn(m.getPins(), s.pins));
        setCode(codeTab(m, o.fields, o.radio === "builtin"));
      }
    });
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reference]);

  const libModel = useMemo(() => lib?.models.find((m) => m.name === selected) ?? null, [lib, selected]);
  const names = useMemo(() => filterModelNames(lib?.models.map((m) => m.name) ?? [], filter), [lib, filter]);
  const ibisLoaded = !!lib?.ibis;
  const ibisModel = ibisLoaded && selected ? ibisModels[selected] ?? null : null;
  const builtinModel = opened ? currentBuiltin(opened.builtin) : null;
  /** `curModel()`: the library's selected model on "SPICE model from file", else the built-in one. A SPICE library model has no `SimModel`. */
  const curModel: SimModel | null = radio === "library" ? ibisModel : builtinModel;
  const modelKey = radio === "library" ? `lib:${selected}:${ibisModel?.type ?? ""}` : `builtin:${opened?.builtin.curModelType}`;

  /** The current model's pins: the built-in or IBIS model's, or a SPICE library model's list. */
  const curPins = (): ModelPin[] => {
    if (radio === "library") return ibisModel ? ibisModel.getPins() : selected ? modelPins[selected] ?? [] : [];
    return builtinModel?.getPins() ?? [];
  };

  // `updateWidgets` on another model: the grid's categories as `updateModelParamsTab` leaves them, the Code tab, the checkbox, the pin names
  useEffect(() => {
    if (!sym || !opened) return;
    setExpanded({});
    if (curModel) {
      setCode(codeTab(curModel, opened.fields, radio === "builtin", radio === "library" ? selected ?? "" : ""));
      if (!curModel.hasPrimaryValue()) setSaveInValue(false);
      if (isRawSpice(curModel)) setPage("code");
    }
    // a library still loading has no model yet: the grid is filled by its first model (`TransferDataToWindow` loads it before `updateWidgets`)
    if (radio === "library" && !selected) return;
    const pins = curPins();
    setColumn((c) => (c ?? gridModelColumn(pins, sym.pins)).map((cell) => (modelPinIndex(cell) >= 0 ? modelPinString(pins, modelPinIndex(cell)) : cell)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelKey]);

  if (!sym || !tables || !opened) {
    return (
      <div className="dialog-backdrop" onClick={onClose}>
        <div className="dialog" style={{ width: 420 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Simulation Model Editor">
          <div className="dialog-header">
            <span>Simulation Model Editor</span>
          </div>
          <div className="dialog-body">{info ?? "Reading the symbol..."}</div>
          <div className="dialog-footer">
            <button onClick={onClose}>Cancel</button>
          </div>
        </div>
      </div>
    );
  }

  const pins = curPins();
  const grid = column ?? sym.pins.map(() => NOT_CONNECTED);
  const builtinOn = radio !== "library";
  const libraryOn = radio !== "builtin";
  const refreshCode = (m: SimModel) => setCode(codeTab(m, opened.fields, radio === "builtin", radio === "library" ? selected ?? "" : ""));

  /** `onModelNameChoice`: another model of the library (for IBIS, its pin list and no pin model yet). */
  const chooseModel = (name: string) => {
    setSelected(name);
    setRadio("library");
    if (lib?.ibis) {
      setIbisPin(-1);
      setIbisPinModels([]);
      setIbisPinModel("");
    }
  };

  /** `onPinAssignmentsGridCellChange`. */
  const changeCell = (row: number, value: string) => {
    if (curModel) {
      const old = modelPinIndex(grid[row]!);
      const now = modelPinIndex(value);
      if (old >= 0) curModel.assignSymbolPinNumberToModelPinIndex(old, "");
      if (now >= 0) curModel.assignSymbolPinNumberToModelPinIndex(now, sym.pins[row]!.number);
      setColumn(gridModelColumn(curModel.getPins(), sym.pins));
      refreshCode(curModel);
      bump();
      return;
    }
    if (!selected) return;
    const p = assignRow(pins, sym.pins, row, grid[row]!, value);
    setModelPins((m) => ({ ...m, [selected]: p }));
    setColumn(gridModelColumn(p, sym.pins));
  };

  /** `onPinCombobox`: the pin's models (`ChangePin`), whether it can be differential (`isPinDiff`); one model is selected at once. */
  const choosePin = (k: number) => {
    if (!ibisModel || !libModel) return;
    setIbisPin(k);
    const models = libModel.pinModels?.[k] ?? [];
    ibisModel.ibisModels = [...models];
    ibisModel.enableDiff = (libModel.diffPins ?? []).includes(ibisModel.getIbisPins()[k]?.[0] ?? "");
    setIbisPinModels(models);
    setIbisPinModel(models.length === 1 ? models[0]! : "");
    bump();
  };

  /** `onWaveformChoice`. */
  const chooseWaveform = (description: string) => {
    if (!ibisModel || !selected) return;
    const { model: m, error } = ibisWaveformModel(tables, ibisModel, description, opened.fields, sym.pins);
    if (error) setInfo(error);
    setIbisModels((all) => ({ ...all, [selected]: m }));
  };

  const ok = async () => {
    const before: Fields = fieldsOf(sym);
    let after: Fields;
    if (radio === "library" && lib && !lib.ibis && libModel) {
      const wanted = fieldsOnOk(opened.fields, lib.stored, libModel, pinsOnOk(pins, sym.pins, grid));
      after = { ...opened.fields };
      for (const [k, v] of Object.entries(wanted)) {
        if (v === "") delete after[k];
        else after[k] = v;
      }
    } else if (radio === "library" && lib?.ibis && ibisModel && selected) {
      const pin = ibisModel.getIbisPins()[ibisPin]?.[0] ?? "";
      after = builtinOk(ibisModel, opened.fields, { saveInValue, column: grid, pins: sym.pins, library: { path: lib.stored, name: selected, ibis: { pin, model: ibisPinModel, differential: ibisDiff } } });
    } else if (radio === "library") {
      // a library that could not be read: its path is stored with the name the symbol had (`SIM_MODEL_SPICE_FALLBACK`)
      after = { ...opened.fields, [SIM_FIELDS.library]: pathText.trim(), [SIM_FIELDS.name]: pathText.trim() ? opened.fields[SIM_FIELDS.name] ?? "" : "" };
    } else {
      after = builtinOk(currentBuiltin(opened.builtin), opened.fields, { saveInValue, column: grid, pins: sym.pins, code: page === "code" ? code : undefined });
    }
    const wanted: Record<string, string> = {};
    const simNames = (f: Fields) => Object.keys(f).filter((n) => n.startsWith("Sim."));
    for (const name of new Set([...FIELD_ROWS, ...simNames(before), ...simNames(after)])) wanted[name] = after[name] ?? "";
    wanted.Value = after.Value ?? before.Value ?? "";
    const { edits, removes } = fieldVerbs(sym.ref, { ...sym.fields, Value: sym.value }, sym.own, wanted);
    const cmds: Cmd[] = [];
    if (edits.length > 0) cmds.push({ op: "set_symbol_fields", edits });
    for (const name of removes) cmds.push(removeUserFieldCmd(sym.ref, name));
    if (cmds.length > 0) {
      setBusy(true);
      try {
        if (!(await api.cmdBatch(cmds))) return;
      } finally {
        setBusy(false);
      }
      onWritten?.(wanted);
    }
    onClose();
  };

  const browse = async () => setFiles(await fetchSimLibraryFiles());

  const dev = builtinModel && radio !== "library" ? deviceChoices(tables, builtinModel) : { items: [], selected: -1 };
  const types = builtinModel && radio !== "library" ? typeChoices(tables, builtinModel) : { items: [], selected: -1, shown: false };
  const save = curModel ? saveInValueLabel(curModel) : { label: "Save primary parameter in Value field", enabled: false };
  const gridRows = curModel ? paramGrid(curModel) : null;
  const waveforms = ibisModel ? waveformChoices(tables, ibisModel) : { items: [], selected: -1 };

  const editParam = (row: ParamRow, value: string | boolean) => {
    if (!curModel) return;
    setGridValue(curModel, row.index, value);
    refreshCode(curModel);
    bump();
  };

  const paramRowView = (row: ParamRow) => (
    <tr key={`${modelKey}:${row.index}`} data-param={row.name}>
      <td style={{ paddingLeft: 14 }}>{row.label}</td>
      <td>
        {row.kind === "bool" ? (
          <input type="checkbox" aria-label={row.label} checked={row.value === "1"} onChange={(e) => editParam(row, e.target.checked)} />
        ) : row.kind === "enum" ? (
          <select aria-label={row.label} value={row.choices.includes(row.value) ? row.value : ""} onChange={(e) => editParam(row, e.target.value)}>
            {!row.choices.includes(row.value) && <option value="" />}
            {row.choices.map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        ) : (
          <input
            key={`${modelKey}:${row.index}:${row.value}`}
            aria-label={row.label}
            defaultValue={row.value}
            style={{ width: "100%" }}
            onBlur={(e) => e.target.value !== row.value && editParam(row, e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
        )}
      </td>
      <td>{row.unit}</td>
      <td>{row.default}</td>
      <td>{row.type}</td>
    </tr>
  );

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" style={{ width: 720, maxWidth: "96vw", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Simulation Model Editor" data-testid="sim-model-dialog">
        <div className="dialog-header">
          <span>Simulation Model Editor</span>
          <span>{sym.ref}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "78vh", overflowY: "auto" }}>
          {info && (
            <div className="panel-empty" style={{ whiteSpace: "pre-wrap", marginBottom: 8 }} data-testid="sim-model-info">
              {info}
            </div>
          )}
          <div style={{ display: "flex", gap: 8, marginBottom: 8 }}>
            <button className={tab === "model" ? "primary" : ""} onClick={() => setTab("model")}>
              Model
            </button>
            <button className={tab === "pins" ? "primary" : ""} onClick={() => setTab("pins")} data-testid="sim-model-pins-tab">
              Pin Assignments
            </button>
          </div>
          {tab === "model" && (
            <div>
              <label>
                <input type="radio" checked={radio === "library"} onChange={() => setRadio("library")} data-testid="sim-model-library" /> SPICE model from file (*.lib, *.sub or *.ibs)
              </label>
              <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr auto", marginTop: 6, opacity: libraryOn ? 1 : 0.6 }}>
                <span>File:</span>
                <input
                  value={pathText}
                  aria-label="File"
                  data-testid="sim-model-file"
                  disabled={!libraryOn}
                  onChange={(e) => {
                    setPathText(e.target.value);
                    setRadio("library"); // `onLibraryPathText`
                  }}
                  onKeyDown={(e) => e.key === "Enter" && void loadLibrary(pathText, sym, opened.fields, tables)}
                  onBlur={() => pathText !== (lib?.path ?? "") && void loadLibrary(pathText, sym, opened.fields, tables)}
                />
                <button onClick={() => void browse()} title="Browse Models" data-testid="sim-model-browse">
                  Browse...
                </button>
                <span>{ibisLoaded ? "Component:" : "Model:"}</span>
                <input value={filter} placeholder="Filter" aria-label="Filter" disabled={!libraryOn || ibisLoaded} onChange={(e) => setFilter(e.target.value)} />
                <span />
                <span />
                <select size={6} value={selected ?? ""} aria-label="Model" data-testid="sim-model-list" disabled={!libraryOn} onChange={(e) => chooseModel(e.target.value)} style={{ minHeight: 100 }}>
                  {names.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
                <span />
                {ibisLoaded && ibisModel && (
                  <>
                    <span>Pin:</span>
                    <span style={{ display: "flex", gap: 8, alignItems: "center" }}>
                      <select aria-label="Pin" data-testid="sim-model-ibis-pin" value={ibisPin} onChange={(e) => choosePin(Number(e.target.value))}>
                        <option value={-1} />
                        {ibisModel.getIbisPins().map(([n, s], k) => (
                          <option key={`${n}:${k}`} value={k}>
                            {`${n} - ${s}`}
                          </option>
                        ))}
                      </select>
                      {ibisModel.canDifferential() && (
                        <label>
                          <input
                            type="checkbox"
                            checked={ibisDiff}
                            data-testid="sim-model-ibis-diff"
                            onChange={(e) => {
                              // `onDifferentialCheckbox`
                              ibisModel.switchSingleEndedDiff(e.target.checked && ibisModel.canDifferential());
                              setIbisDiff(e.target.checked);
                              setColumn(gridModelColumn(ibisModel.getPins(), sym.pins));
                              bump();
                            }}
                          />{" "}
                          Differential
                        </label>
                      )}
                    </span>
                    <span />
                    <span>Pin model:</span>
                    <select aria-label="Pin model" data-testid="sim-model-ibis-pin-model" value={ibisPinModel} onChange={(e) => setIbisPinModel(e.target.value)}>
                      <option value="" />
                      {ibisPinModels.map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                    </select>
                    <span />
                    <span>Waveform:</span>
                    <select aria-label="Waveform" data-testid="sim-model-waveform" value={waveforms.items[waveforms.selected] ?? ""} onChange={(e) => chooseWaveform(e.target.value)}>
                      {waveforms.items.map((w) => (
                        <option key={w} value={w}>
                          {w}
                        </option>
                      ))}
                    </select>
                    <span />
                  </>
                )}
              </div>
              {files && (
                <div className="panel-empty" style={{ marginTop: 6 }} data-testid="sim-model-files">
                  <div style={{ marginBottom: 4 }}>Browse Models</div>
                  {files.length === 0 && <div>No library files in the board folder.</div>}
                  {files.map((f) => (
                    <div key={f}>
                      <a
                        href="#"
                        onClick={(e) => {
                          e.preventDefault();
                          setFiles(null);
                          void loadLibrary(f, sym, opened.fields, tables);
                        }}
                      >
                        {f}
                      </a>
                    </div>
                  ))}
                </div>
              )}
              <label style={{ display: "block", marginTop: 10 }}>
                <input type="radio" checked={radio === "builtin"} onChange={() => setRadio("builtin")} data-testid="sim-model-builtin" /> Built-in SPICE model
              </label>
              <div className="kv-grid" style={{ gridTemplateColumns: "80px 1fr", marginTop: 6, opacity: builtinOn ? 1 : 0.6 }}>
                <span>Device:</span>
                <select
                  aria-label="Device"
                  data-testid="sim-model-device"
                  disabled={!builtinOn}
                  value={dev.items[dev.selected] ?? ""}
                  onChange={(e) => {
                    setRadio("builtin"); // `onDeviceTypeChoice`
                    chooseDevice(tables, opened.builtin, e.target.value);
                    bump();
                  }}
                >
                  {dev.items.map((d, k) => (
                    <option key={`${d}:${k}`} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
                {types.shown && (
                  <>
                    <span>Device type:</span>
                    <select
                      aria-label="Device type"
                      data-testid="sim-model-type"
                      disabled={!builtinOn}
                      value={types.items[types.selected] ?? ""}
                      onChange={(e) => {
                        chooseType(tables, opened.builtin, e.target.value);
                        bump();
                      }}
                    >
                      {types.items.map((d) => (
                        <option key={d} value={d}>
                          {d}
                        </option>
                      ))}
                    </select>
                  </>
                )}
              </div>
              <div style={{ display: "flex", gap: 8, margin: "10px 0 6px" }}>
                <button className={page === "params" ? "primary" : ""} onClick={() => setPage("params")} data-testid="sim-model-params-tab">
                  Parameters
                </button>
                <button
                  className={page === "code" ? "primary" : ""}
                  onClick={() => {
                    // `onPageChanging` -> `updateModelCodeTab`
                    if (curModel) refreshCode(curModel);
                    setPage("code");
                  }}
                  data-testid="sim-model-code-tab"
                >
                  Code
                </button>
              </div>
              {page === "params" && (
                <div style={{ maxHeight: 260, overflowY: "auto", border: "1px solid var(--chrome-border)" }}>
                  {gridRows ? (
                    <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }} data-testid="sim-model-param-grid">
                      <thead>
                        <tr>
                          <th style={{ textAlign: "left" }}>Parameter</th>
                          <th style={{ textAlign: "left", width: 110 }}>Value</th>
                          <th style={{ textAlign: "left" }}>Unit</th>
                          <th style={{ textAlign: "left" }}>Default</th>
                          <th style={{ textAlign: "left" }}>Type</th>
                        </tr>
                      </thead>
                      <tbody>
                        {gridRows.principal.map(paramRowView)}
                        {gridRows.groups.map((g) => {
                          const open = expanded[g.title] ?? g.expanded;
                          return [
                            <tr key={`cat:${g.title}`} data-category={g.title}>
                              <td colSpan={5} style={{ fontWeight: 600, cursor: "pointer", background: "var(--chrome-bg)" }} onClick={() => setExpanded((x) => ({ ...x, [g.title]: !open }))}>
                                {open ? "▾" : "▸"} {g.title}
                              </td>
                            </tr>,
                            ...(open ? g.rows.map(paramRowView) : []),
                          ];
                        })}
                      </tbody>
                    </table>
                  ) : (
                    <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, padding: 6 }}>A library model's parameters are not shown here (GAPS 34): its Sim.Params are kept for the same model.</p>
                  )}
                </div>
              )}
              {page === "code" && (
                <textarea
                  value={libModel && !ibisLoaded && radio === "library" ? libModel.code : code}
                  readOnly={!(curModel && isRawSpice(curModel))}
                  onChange={(e) => setCode(e.target.value)}
                  aria-label="Code"
                  data-testid="sim-model-code"
                  style={{ width: "100%", height: 160, fontFamily: "monospace", fontSize: 11 }}
                />
              )}
              {radio === "library" && libModel?.kind === "subckt" && page === "params" && (
                <textarea readOnly value={libModel.code} aria-label="Subcircuit" style={{ width: "100%", height: 110, marginTop: 6, fontFamily: "monospace", fontSize: 11 }} />
              )}
              <label style={{ display: "block", marginTop: 8, opacity: save.enabled ? 1 : 0.6 }}>
                <input type="checkbox" checked={saveInValue && save.enabled} disabled={!save.enabled} onChange={(e) => setSaveInValue(e.target.checked)} data-testid="sim-model-save-in-value" /> {save.label}
              </label>
            </div>
          )}
          {tab === "pins" && (
            <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12 }} data-testid="sim-model-pin-grid">
              <thead>
                <tr>
                  <th style={{ textAlign: "left" }}>Symbol Pin</th>
                  <th style={{ textAlign: "left" }}>Model Pin</th>
                </tr>
              </thead>
              <tbody>
                {sym.pins.map((p, row) => (
                  <tr key={p.number}>
                    <td>{symbolPinString(p)}</td>
                    <td>
                      <select value={grid[row]} aria-label={`Model pin of ${p.number}`} disabled={!curModel && !libModel} onChange={(e) => changeCell(row, e.target.value)}>
                        {[...new Set([...modelPinChoices(pins), grid[row]!])].map((c) => (
                          <option key={c} value={c}>
                            {c}
                          </option>
                        ))}
                      </select>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={onClose}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void ok()} data-testid="sim-model-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
