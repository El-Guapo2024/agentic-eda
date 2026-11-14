// Port of eeschema/dialogs/dialog_sim_model.cpp (`DIALOG_SIM_MODEL`, "Simulation Model Editor", KiCad 8303b2ad): the Model tab's two halves
// and the Pin Assignments tab.
// - "SPICE model from file (*.lib, *.sub or *.ibs)": the File box and Browse, the model list with its filter, the subcircuit's code; the library
//   is read by the server (`POST /api/sim/model/library`, `eda_sim::library`) and each of its models made as `loadLibrary` makes them
//   (kicad-port/sim/simLibraryModel.ts: a `.model`'s parameters and an "ako:" model's parent's, a subcircuit's ports and `params:`, each
//   model with the library's behind it, so the grid shows the library's values and `Sim.Params` holds what the instance changes). An IBIS file
//   (`eda_sim::ibis`) lists its components ("Component:"), and the Pin, Pin model, Differential and Waveform choices of `updateIbisWidgets`,
//   `onModelNameChoice`, `onPinCombobox`, `onDifferentialCheckbox` and `onWaveformChoice`, with the IBIS model's parameters.
// - "Built-in SPICE model": the Device and Device type choices, the Parameters grid (arithmetic in an Int or Float row, the K device's
//   inductors) and the Code tab, "Save parameter '%s (%s)' in Value field" (`TransferDataToWindow`, `updateBuiltinModelWidgets`,
//   `updateModelParamsTab`, `updateModelCodeTab`; kicad-port/sim/simModelDialog.ts over the models of simModelBuiltin.ts, whose tables
//   tools/extract-sim-models.js copied from the C++ into src/kicad/sim_models.json, loaded when the dialog opens).
// - Pin Assignments: Symbol Pin and Model Pin, one grid whichever model is current (`updatePinAssignments( model, false )` keeps a row's model
//   pin index across models).
// - OK (`TransferDataFromWindow`): `Sim.Library`, `Sim.Name`, the IBIS pin, model and differential fields, then `WriteFields` -- `Sim.Device`,
//   `Sim.Type`, `Sim.Enable`, `Sim.Pins`, `Sim.Params`, and the Value when the primary parameter is saved there -- into the copy of Symbol
//   Properties' fields it was given; Symbol Properties takes them into its grid (`OnEditSpiceModel`) and its own OK applies them, one undo step.
// - The texts are read resolved where KiCad reads them so (the server's `GetShownText` of each, `shown`); the error boxes are KiCad's
//   (`DisplayErrorMessage`: "No simulation model definition found ..", "Failed to read simulation model from fields.", a waveform's fields),
//   the library's messages its info bar.
//
// Where it differs from KiCad:
// - Browse lists the library files of the board folder (`GET /api/sim/model/files`) instead of a file dialog (a browser has no file system to
//   open); a path can be typed as in KiCad.
// - The K device's inductors are read from the design, the schematic in memory, as KiCad's `ReadSchematicAndLibraries` reads it (`GET
//   /api/sim/model/inductors`, UI-LEFT-3 M3-1), when a K model's grid first shows.
import { useEffect, useMemo, useReducer, useState } from "react";
import { fetchSimInductors, fetchSimLibrary, fetchSimLibraryFiles, fetchSimModelSymbol, loadSimModelTables, type SimModelSymbolReply } from "../../api/simModelClient";
import { showError } from "../library/libraryDialogs";
import {
  filterModelNames,
  gridModelColumn,
  initialModel,
  modelPinChoices,
  modelPinIndex,
  modelPinString,
  NOT_CONNECTED,
  SIM_FIELDS,
  symbolPinString,
  type LibraryModelInfo,
  type ModelPin,
} from "../../kicad-port/sim/simModel";
import { ibisComponentModel, readTypeFromFields, referencePrefix, SimModelSpiceFallback, SimModelSubckt, SimModelError, type Fields, type SimModel, type SimModelIbis, type SimModelTables } from "../../kicad-port/sim/simModelBuiltin";
import { libraryBaseModels, libraryModel } from "../../kicad-port/sim/simLibraryModel";
import {
  builtinOk,
  chooseDevice,
  chooseType,
  codeTab,
  currentBuiltin,
  deviceChoices,
  gridText,
  ibisWaveformModel,
  isRawSpice,
  openBuiltin,
  paramGrid,
  resolvedFields,
  saveInValueLabel,
  setGridValue,
  typeChoices,
  waveformChoices,
  type OpenedDialog,
  type ParamRow,
} from "../../kicad-port/sim/simModelDialog";

/** The fields Symbol Properties hands the dialog (`OnEditSpiceModel`'s copy of `m_fields`): the Value box and the user fields as its grid has them. */
export interface SimModelDialogFields {
  value: string;
  user: { name: string; text: string }[];
}

/** What OK hands back: the fields after `TransferDataFromWindow` (Reference and Value among them), the copy it was given, and the visibility of
 * each field it added. */
export interface SimModelResult {
  input: Fields;
  output: Fields;
  visible: Record<string, boolean>;
}

interface Props {
  reference: string;
  /** Symbol Properties' fields as they stand (unapplied edits among them); the symbol's own when absent. */
  fields?: SimModelDialogFields;
  onClose: () => void;
  /** OK: the fields, for Symbol Properties to take (`OnEditSpiceModel`). */
  onOk: (result: SimModelResult) => void;
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

/** `m_fields`: the server's (the instance's over the library symbol's simulation fields) with Symbol Properties' rows over them -- a field its
 * grid no longer has is not there -- and its Value. */
function dialogFields(s: SimModelSymbolReply, given: SimModelDialogFields | undefined): Fields {
  const out: Fields = { Reference: s.ref, Value: given ? given.value : s.value, ...s.fields };
  if (given) {
    for (const name of s.own) if (!given.user.some((u) => u.name === name)) delete out[name];
    for (const u of given.user) out[u.name] = u.text;
  }
  return out;
}

export function SimModelDialog({ reference, fields: given, onClose, onOk }: Props) {
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
  /** `m_libraryModelsMgr`'s models by name: each SPICE library model's, each IBIS component's (a waveform's model replacing it: `SetModel`);
   * a library that could not be read has one, its fallback (`CreateModel( nullptr, .. )`), under "". */
  const [libModels, setLibModels] = useState<Record<string, SimModel>>({});
  /** The K device's inductors (`newParamProperty`), read once a K model's grid shows. */
  const [inductors, setInductors] = useState<string[] | null>(null);
  /** `GetShownText` of each text that holds a variable (the server's). */
  const [shown, setShown] = useState<Record<string, string>>({});
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

  const resolve = (raw: string) => shown[raw] ?? raw;

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

  /** `loadLibrary`: the models (the one the fields name given the fields, `CreateModel( &baseModel, m_sortedPartPins, m_fields, true, .. )`),
   * the one the fields name or the first, and the info bar. `fields` are read resolved. */
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
      // `TransferDataToWindow` when `loadLibrary` fails: the model is the fields' fallback (`CreateModel( nullptr, .., m_fields, .. )`)
      const fallback = new SimModelSpiceFallback(t, readTypeFromFields(t, fields));
      try {
        fallback.readDataFields(fields, from.pins);
      } catch (e) {
        if (!(e instanceof SimModelError)) throw e;
      }
      setLibModels({ "": fallback });
      setLib(null);
      setSelected(null);
      setInfo(r.messages.join("\n"));
      return;
    }
    const simName = fields[SIM_FIELDS.name] ?? "";
    const models: Record<string, SimModel> = {};
    const bases = r.ibis ? null : libraryBaseModels(t, r.models);
    for (const m of r.models) {
      if (m.name in models) continue;
      // `CreateModel( &baseModel, m_sortedPartPins[, m_fields] )`: the fields go to the model they name
      const named = m.name === simName ? fields : null;
      if (r.ibis) models[m.name] = ibisComponentModel(t, { name: m.name, ibisPins: m.ibisPins ?? [] }, from.pins, named);
      else models[m.name] = libraryModel(t, bases!.get(m.name)!, from.pins, named);
    }
    const first = initialModel(r.models.map((m) => m.name), simName, path);
    setLibModels(models);
    setLib({ path, stored: r.path, models: r.models, ibis: !!r.ibis });
    setSelected(first.name);
    setInfo(first.message);
    if (r.ibis && first.name) openIbisComponent(r.models.find((m) => m.name === first.name)!, models[first.name] as SimModelIbis, fields);
    const firstPins = first.name ? models[first.name]!.getPins() : [];
    // `updatePinAssignments( model, false )`: a grid without rows is filled from the model
    setColumn((c) => c ?? (first.name ? gridModelColumn(firstPins, from.pins) : null));
  };

  useEffect(() => {
    let stopped = false;
    const texts = given ? [given.value, ...given.user.map((u) => u.text)] : [];
    void Promise.all([fetchSimModelSymbol(reference, texts), loadTables()]).then(async ([s, t]) => {
      if (stopped) return;
      if (!s.ok) {
        setInfo(s.message ?? "The symbol could not be read.");
        return;
      }
      const shownTexts = s.shown ?? {};
      const res = (raw: string) => shownTexts[raw] ?? raw;
      const o = openBuiltin(t, referencePrefix(s.ref), dialogFields(s, given), s.pins, res);
      setShown(shownTexts);
      setTables(t);
      setSym(s);
      setOpened(o);
      setRadio(o.radio);
      setSaveInValue(o.saveInValue);
      // `DisplayErrorMessage( this, .. )`
      for (const e of o.errors) void showError(e);
      const rf = resolvedFields(o.fields, res);
      if (o.radio === "library") await loadLibrary(rf[SIM_FIELDS.library] ?? "", s, rf, t);
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
  const libraryCur = selected ? libModels[selected] ?? null : lib ? null : libModels[""] ?? null;
  const ibisModel = ibisLoaded ? ((libraryCur as SimModelIbis | null) ?? null) : null;
  const builtinModel = opened ? currentBuiltin(opened.builtin) : null;
  /** `curModel()`: the library's selected model on "SPICE model from file", else the built-in one. */
  const curModel: SimModel | null = radio === "library" ? libraryCur : builtinModel;
  const modelKey = radio === "library" ? `lib:${selected}:${ibisModel?.type ?? ""}` : `builtin:${opened?.builtin.curModelType}`;

  /** The current model's pins. */
  const curPins = (): ModelPin[] => curModel?.getPins() ?? [];

  // the K device's inductors, read the first time a K model shows
  useEffect(() => {
    if (curModel?.type !== "K" || inductors !== null) return;
    let stopped = false;
    void fetchSimInductors()
      .then((list) => !stopped && setInductors(list))
      .catch(() => !stopped && setInductors([]));
    return () => {
      stopped = true;
    };
  }, [curModel?.type, inductors]);

  // `updateWidgets` on another model: the grid's categories as `updateModelParamsTab` leaves them, the Code tab (`updateModelCodeTab`: not for a
  // fallback), the checkbox, the pin names
  useEffect(() => {
    if (!sym || !opened) return;
    setExpanded({});
    if (curModel) {
      if (!(curModel instanceof SimModelSpiceFallback)) setCode(codeTab(curModel, opened.fields, radio === "builtin", radio === "library" ? selected ?? "" : ""));
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
  const refreshCode = (m: SimModel) => {
    if (!(m instanceof SimModelSpiceFallback)) setCode(codeTab(m, opened.fields, radio === "builtin", radio === "library" ? selected ?? "" : ""));
  };

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
    if (!curModel) return;
    const old = modelPinIndex(grid[row]!);
    const now = modelPinIndex(value);
    if (old >= 0) curModel.assignSymbolPinNumberToModelPinIndex(old, "");
    if (now >= 0) curModel.assignSymbolPinNumberToModelPinIndex(now, sym.pins[row]!.number);
    setColumn(gridModelColumn(curModel.getPins(), sym.pins));
    refreshCode(curModel);
    bump();
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
    const { model: m, error } = ibisWaveformModel(tables, ibisModel, description, resolvedFields(opened.fields, resolve), sym.pins);
    if (error) void showError(error);
    setLibModels((all) => ({ ...all, [selected]: m }));
  };

  /** `TransferDataFromWindow`, then `OnEditSpiceModel` takes the fields. */
  const ok = () => {
    const visible: Record<string, boolean> = { ...opened.visible };
    const common = { saveInValue, column: grid, pins: sym.pins, visible, inferredValueRestore: opened.inferredValueRestore };
    let after: Fields;
    if (radio === "library" && lib && curModel && selected) {
      const ibis = lib.ibis && ibisModel ? { pin: ibisModel.getIbisPins()[ibisPin]?.[0] ?? "", model: ibisPinModel, differential: ibisDiff } : undefined;
      after = builtinOk(curModel, opened.fields, { ...common, library: { path: lib.stored, name: selected, ibis } });
    } else if (radio === "library" && curModel) {
      // a library that could not be read: its path, and the name the symbol had (`SIM_MODEL_SPICE_FALLBACK`)
      const name = pathText.trim() ? resolvedFields(opened.fields, resolve)[SIM_FIELDS.name] ?? "" : "";
      after = builtinOk(curModel, opened.fields, { ...common, library: { path: pathText.trim(), name } });
    } else {
      after = builtinOk(currentBuiltin(opened.builtin), opened.fields, { ...common, code: page === "code" ? code : undefined });
    }
    onOk({ input: dialogFields(sym, given), output: after, visible });
    onClose();
  };

  const browse = async () => setFiles(await fetchSimLibraryFiles());

  const dev = builtinModel && radio !== "library" ? deviceChoices(tables, builtinModel) : { items: [], selected: -1 };
  const types = builtinModel && radio !== "library" ? typeChoices(tables, builtinModel) : { items: [], selected: -1, shown: false };
  const save = curModel ? saveInValueLabel(curModel) : { label: "Save primary parameter in Value field", enabled: false };
  const gridRows = curModel ? paramGrid(curModel, inductors ?? []) : null;
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
          <select aria-label={row.label} value={row.selected >= 0 ? row.choices[row.selected] : ""} onChange={(e) => editParam(row, e.target.value)}>
            {row.selected < 0 && <option value="" />}
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
            // `SIM_STRING_PROPERTY::OnEvent`'s `wxEVT_KILL_FOCUS`: the arithmetic's result
            onBlur={(e) => {
              const text = gridText(e.target.value, row.valueType, "blur");
              e.target.value = text;
              if (text !== row.value) editParam(row, text);
            }}
            // Enter commits (`StringToValue`: the result normalized)
            onKeyDown={(e) => {
              if (e.key !== "Enter") return;
              const input = e.target as HTMLInputElement;
              const text = gridText(input.value, row.valueType, "enter");
              input.value = text;
              if (text !== row.value) editParam(row, text);
              input.blur();
            }}
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
                  onKeyDown={(e) => e.key === "Enter" && void loadLibrary(pathText, sym, resolvedFields(opened.fields, resolve), tables)}
                  onBlur={() => pathText !== (lib?.path ?? "") && void loadLibrary(pathText, sym, resolvedFields(opened.fields, resolve), tables)}
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
                          void loadLibrary(f, sym, resolvedFields(opened.fields, resolve), tables);
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
                  ) : null}
                </div>
              )}
              {page === "code" && (
                // `m_codePreview`: it takes typing (read back on OK for a raw SPICE element; another model's preview is made again on the next
                // change, the typing lost)
                <textarea
                  value={code}
                  onChange={(e) => setCode(e.target.value)}
                  aria-label="Code"
                  data-testid="sim-model-code"
                  style={{ width: "100%", height: 160, fontFamily: "monospace", fontSize: 11 }}
                />
              )}
              {curModel instanceof SimModelSubckt && page === "params" && (
                <textarea readOnly value={curModel.getSpiceCode()} aria-label="Subcircuit" style={{ width: "100%", height: 110, marginTop: 6, fontFamily: "monospace", fontSize: 11 }} />
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
                      <select value={grid[row]} aria-label={`Model pin of ${p.number}`} disabled={!curModel} onChange={(e) => changeCell(row, e.target.value)}>
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
          <button className="primary" onClick={ok} data-testid="sim-model-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
