// Port of the library half of eeschema/dialogs/dialog_sim_model.cpp (`DIALOG_SIM_MODEL`, "Simulation Model Editor"), the "T2-lite" GAPS 34 names:
// "SPICE model from file": the File box and Browse, the model list with its filter, the subcircuit's code, and the Pin Assignments tab (Symbol Pin,
// Model Pin); OK writes `Sim.Library`, `Sim.Name`, `Sim.Device`, `Sim.Type` and `Sim.Pins` as `TransferDataFromWindow` and `WriteFields` do
// (kicad-port/sim/simModel.ts). The library is read by the server (`POST /api/sim/model/library`, `eda_sim::library`).
//
// Where it differs from KiCad:
// - KiCad's dialog is opened from Symbol Properties' "Simulation Model..." button and writes into that dialog's fields, which its OK commits; here
//   OK is its own undo step (one batch: the Symbol Fields Table's `set_symbol_fields`, which reaches a sheet read from a KiCad file too, and
//   `remove_user_field`), and Symbol Properties takes the new texts.
// - "Built-in SPICE model" (the device and type choices, the parameter grid, the Code tab), the IBIS controls and "Save parameter in Value field"
//   are not ported (GAPS 34): with no library the dialog writes nothing.
// - Browse lists the library files of the board folder (`GET /api/sim/model/files`) instead of a file dialog; a path can be typed as in KiCad.
import { useEffect, useMemo, useState } from "react";
import type { Cmd } from "../../api/types";
import { fetchSimLibrary, fetchSimLibraryFiles, fetchSimModelSymbol, type SimModelSymbolReply } from "../../api/simModelClient";
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
import { useStudioApi } from "../../state/store";

interface Props {
  reference: string;
  onClose: () => void;
  /** After OK wrote the fields: what each simulation field is now ("" for one removed). */
  onWritten?: (fields: Record<string, string>) => void;
}

interface Loaded {
  /** The path as typed and as stored. */
  path: string;
  stored: string;
  models: LibraryModelInfo[];
}

export function SimModelDialog({ reference, onClose, onWritten }: Props) {
  const api = useStudioApi();
  const [sym, setSym] = useState<SimModelSymbolReply | null>(null);
  const [tab, setTab] = useState<"model" | "pins">("model");
  const [useLibrary, setUseLibrary] = useState(false);
  const [pathText, setPathText] = useState("");
  const [lib, setLib] = useState<Loaded | null>(null);
  const [info, setInfo] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  /** Each model's pins (`m_libraryModelsMgr`'s models), by name. */
  const [modelPins, setModelPins] = useState<Record<string, ModelPin[]>>({});
  /** The Pin Assignments grid's Model Pin column, row by symbol pin; `null` until it has rows. */
  const [column, setColumn] = useState<string[] | null>(null);
  const [files, setFiles] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);

  /** `loadLibrary`: the models, the one the fields name (with the symbol's `Sim.Pins`) or the first, and the info bar. */
  const loadLibrary = async (path: string, from: SimModelSymbolReply) => {
    if (path.trim() === "") {
      setLib(null);
      setSelected(null);
      setInfo(null);
      return;
    }
    const r = await fetchSimLibrary(path);
    setPathText(path);
    setUseLibrary(true);
    if (!r.ok) {
      setLib(null);
      setSelected(null);
      setInfo(r.messages.join("\n"));
      return;
    }
    const pins: Record<string, ModelPin[]> = {};
    const simName = from.fields[SIM_FIELDS.name] ?? "";
    let message: string | null = null;
    for (const m of r.models) {
      let p = defaultModelPins(m, from.pins);
      // `CreateModel( &baseModel, m_sortedPartPins, m_fields, .. )` for the model the fields name: its `Sim.Pins`
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
    setLib({ path, stored: r.path, models: r.models });
    setSelected(first.name);
    setInfo(message ?? first.message);
    // `updatePinAssignments( model, false )`: a grid without rows is filled from the model
    setColumn((c) => c ?? (first.name ? gridModelColumn(pins[first.name] ?? [], from.pins) : null));
  };

  useEffect(() => {
    let stopped = false;
    void fetchSimModelSymbol(reference).then(async (s) => {
      if (stopped) return;
      if (!s.ok) {
        setInfo(s.message ?? "The symbol could not be read.");
        return;
      }
      setSym(s);
      const path = s.fields[SIM_FIELDS.library] ?? "";
      // `TransferDataToWindow`: a symbol with a library is opened on it; any other on the built-in model
      if (path) await loadLibrary(path, s);
    });
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reference]);

  const model = useMemo(() => lib?.models.find((m) => m.name === selected) ?? null, [lib, selected]);
  const names = useMemo(() => filterModelNames(lib?.models.map((m) => m.name) ?? [], filter), [lib, filter]);

  if (!sym) {
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

  const curPins = selected ? modelPins[selected] ?? [] : [];
  const grid = column ?? sym.pins.map(() => NOT_CONNECTED);

  /** `onModelNameChoice` -> `updatePinAssignments( model, false )`: the rows keep their model pin indices, shown with the new model's names. */
  const chooseModel = (name: string) => {
    setSelected(name);
    setUseLibrary(true);
    const pins = modelPins[name] ?? [];
    setColumn((c) => (c ?? gridModelColumn(pins, sym.pins)).map((cell) => (modelPinIndex(cell) >= 0 ? modelPinString(pins, modelPinIndex(cell)) : cell)));
  };

  /** `onPinAssignmentsGridCellChange`. */
  const changeCell = (row: number, value: string) => {
    if (!selected) return;
    const pins = assignRow(curPins, sym.pins, row, grid[row]!, value);
    setModelPins((m) => ({ ...m, [selected]: pins }));
    setColumn(gridModelColumn(pins, sym.pins));
  };

  const ok = async () => {
    if (!useLibrary) return onClose(); // the built-in models are not ported: nothing to write
    let wanted: Record<string, string>;
    if (model && lib) {
      wanted = fieldsOnOk(sym.fields, lib.stored, model, pinsOnOk(curPins, sym.pins, grid));
    } else {
      // a library that could not be read: its path is stored with the name the symbol had (`SIM_MODEL_SPICE_FALLBACK`)
      wanted = { [SIM_FIELDS.library]: pathText.trim(), [SIM_FIELDS.name]: pathText.trim() ? sym.fields[SIM_FIELDS.name] ?? "" : "" };
    }
    const { edits, removes } = fieldVerbs(sym.ref, sym.fields, sym.own, wanted);
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

  return (
    <div className="dialog-backdrop" onClick={onClose}>
      <div className="dialog" style={{ width: 640, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Simulation Model Editor" data-testid="sim-model-dialog">
        <div className="dialog-header">
          <span>Simulation Model Editor</span>
          <span>{sym.ref}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
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
                <input type="radio" checked={useLibrary} onChange={() => setUseLibrary(true)} /> SPICE model from file (*.lib, *.sub or *.ibs)
              </label>
              <div className="kv-grid" style={{ gridTemplateColumns: "60px 1fr auto", marginTop: 6, opacity: useLibrary ? 1 : 0.6 }}>
                <span>File:</span>
                <input
                  value={pathText}
                  aria-label="File"
                  data-testid="sim-model-file"
                  onChange={(e) => {
                    setPathText(e.target.value);
                    setUseLibrary(true); // `onLibraryPathText`
                  }}
                  onKeyDown={(e) => e.key === "Enter" && void loadLibrary(pathText, sym)}
                  onBlur={() => pathText !== (lib?.path ?? "") && void loadLibrary(pathText, sym)}
                />
                <button onClick={() => void browse()} title="Browse Models" data-testid="sim-model-browse">
                  Browse...
                </button>
                <span>Model:</span>
                <input value={filter} placeholder="Filter" aria-label="Filter" onChange={(e) => setFilter(e.target.value)} />
                <span />
                <span />
                <select size={8} value={selected ?? ""} aria-label="Model" data-testid="sim-model-list" onChange={(e) => chooseModel(e.target.value)} style={{ minHeight: 120 }}>
                  {names.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
                <span />
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
                          void loadLibrary(f, sym);
                        }}
                      >
                        {f}
                      </a>
                    </div>
                  ))}
                </div>
              )}
              {model?.kind === "subckt" && (
                <textarea readOnly value={model.code} aria-label="Subcircuit" style={{ width: "100%", height: 110, marginTop: 6, fontFamily: "monospace", fontSize: 11 }} />
              )}
              <label style={{ display: "block", marginTop: 8 }}>
                <input type="radio" checked={!useLibrary} onChange={() => setUseLibrary(false)} /> Built-in SPICE model
              </label>
              {!useLibrary && <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>The built-in models (the Device and Type choices and their parameters) are not ported yet: edit their Sim.* fields in Symbol Properties.</p>}
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
                      <select value={grid[row]} aria-label={`Model pin of ${p.number}`} disabled={!model} onChange={(e) => changeCell(row, e.target.value)}>
                        {[...new Set([...modelPinChoices(curPins), grid[row]!])].map((c) => (
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
