// Calculator Tools > General system design > Regulators (`PANEL_REGULATOR`, pcb_calculator/calculator_panels/panel_regulator{,_base}.cpp at KiCad
// 25ec2335): the regulator's Type (Standard Type with a sense pin, or 3 Terminal Type) with its picture and Formula; the "Regulator" box (the known
// regulators, the regulators data file with Browse, Edit / Add / Remove Regulator); the min / typ / max grid of R1, R2 and Vout (the radio button
// says which one is calculated), Vref, Iadj (3 terminal only) and the Overall tolerance; the Resistor tolerance; the Power Comment with Copy to
// Clipboard; the message line; Calculate and Reset to Defaults. The logic is kicad-port/calc/regulators.ts.
//
// Settings as `LoadSettings` / `SaveSettings` (`regulators.*`); like KiCad, `SaveSettings` does not store Vout typ (it writes the stored value back
// into the box instead), so Vout typ starts at its stored value, "5" unless set elsewhere. The data file: components/calculator/regulatorFiles.ts.
// While mounted the panel answers `OnClosePcbCalc`'s question about an unsaved regulator list (`setPcbCalculatorCloseGuard`).
import { useEffect, useRef, useState } from "react";
import { DEFAULT_REGULATOR } from "../../kicad-port/calc/calcSettings";
import {
  dataFilename,
  formatDataFile,
  isoCombinedNow,
  parseDataFile,
  regulatorFormValid,
  regulatorFormula,
  regulatorFromForm,
  regulatorListAdd,
  regulatorListGet,
  regulatorListRemove,
  regulatorListReplace,
  regulatorToForm,
  regulatorsSolve,
  type RegulatorData,
  type RegulatorForm,
} from "../../kicad-port/calc/regulators";
import { formatG } from "../../kicad-port/sim/spiceValue";
import { getPcbCalculator, setPcbCalculatorCloseGuard, updatePcbCalculatorSettings } from "../../state/pcbCalculator";
import { useIconScheme } from "../../hooks/useColorScheme";
import { CalcBox, CalcInput, CalcPanel, CalcResult, Choice } from "./calcKit";
import { downloadRegulatorFile, readRegulatorFile, storeRegulatorFile } from "./regulatorFiles";

const REG_TYPES = ["Standard Type", "3 Terminal Type"] as const;

/** `Pgm().App().GetAppName() + ' ' + GetBuildVersion()` in the file's (tool ..) entry. */
const TOOL_NAME = "pcb_calculator EDA Studio";

/** `ReadDataFile`: the list in the named file, or null (no such file, or a parse error, which is shown as KiCad shows it). */
function readDataFile(name: string): RegulatorData[] | null {
  const text = readRegulatorFile(name);
  if (text === null) return null;
  try {
    return parseDataFile(text);
  } catch (e) {
    window.alert(`${e instanceof Error ? e.message : String(e)}\nData file error.`);
    return null;
  }
}

/** `DIALOG_REGULATOR_FORM` ("Regulator Parameters"): Name (fixed when editing), Vref (min/typ/max) Volt, Type, Iadj (typ/max) uA for 3 terminal. */
function RegulatorFormDialog(props: { initial: RegulatorForm; editing: boolean; onOk: (f: RegulatorForm) => void; onCancel: () => void }) {
  const [f, setF] = useState(props.initial);
  const set = (patch: Partial<RegulatorForm>) => setF({ ...f, ...patch });
  const three = f.type === 1;
  return (
    <div className="dialog-backdrop" style={{ zIndex: 2300 }} onClick={props.onCancel}>
      <div className="dialog" role="dialog" aria-label="Regulator Parameters" data-testid="regulator-form" onClick={(e) => e.stopPropagation()} style={{ width: 420 }}>
        <div className="dialog-header">
          <span>Regulator Parameters</span>
        </div>
        <div className="dialog-body">
          <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto" }}>
            <span>Name:</span>
            <CalcInput value={f.name} onChange={(v) => set({ name: v })} disabled={props.editing} testid="regulator-form-name" label="Name" width={220} />
            <span />
            <span>Vref (min/typ/max):</span>
            <span style={{ display: "flex", gap: 4 }}>
              <CalcInput value={f.vrefMin} onChange={(v) => set({ vrefMin: v })} testid="regulator-form-vref-min" label="Vref min" width={70} />
              <CalcInput value={f.vrefTyp} onChange={(v) => set({ vrefTyp: v })} testid="regulator-form-vref-typ" label="Vref typ" width={70} />
              <CalcInput value={f.vrefMax} onChange={(v) => set({ vrefMax: v })} testid="regulator-form-vref-max" label="Vref max" width={70} />
            </span>
            <span>Volt</span>
            <span>Type:</span>
            <Choice options={REG_TYPES} value={f.type} onChange={(i) => set({ type: i })} testid="regulator-form-type" label="Type" />
            <span />
            {three ? (
              <>
                <span>Iadj (typ/max):</span>
                <span style={{ display: "flex", gap: 4 }}>
                  <CalcInput value={f.iadjTyp} onChange={(v) => set({ iadjTyp: v })} testid="regulator-form-iadj-typ" label="Iadj typ" width={70} />
                  <CalcInput value={f.iadjMax} onChange={(v) => set({ iadjMax: v })} testid="regulator-form-iadj-max" label="Iadj max" width={70} />
                </span>
                <span>uA</span>
              </>
            ) : null}
          </div>
        </div>
        <div className="dialog-footer">
          <button type="button" data-testid="regulator-form-cancel" onClick={props.onCancel}>
            Cancel
          </button>
          {/* `TransferDataFromWindow` failing keeps the dialog open, with no message. */}
          <button type="button" className="primary" data-testid="regulator-form-ok" onClick={() => regulatorFormValid(f) && props.onOk(f)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

/** `wxGetSingleChoice( _( "Remove Regulator" ), wxEmptyString, list )`: the name chosen, or nothing on Cancel. */
function RemoveRegulatorDialog(props: { names: string[]; onOk: (name: string) => void; onCancel: () => void }) {
  const [sel, setSel] = useState(0);
  return (
    <div className="dialog-backdrop" style={{ zIndex: 2300 }} onClick={props.onCancel}>
      <div className="dialog" role="dialog" aria-label="Remove Regulator" data-testid="regulator-remove" onClick={(e) => e.stopPropagation()} style={{ width: 320 }}>
        <div className="dialog-body">
          <div style={{ marginBottom: 6 }}>Remove Regulator</div>
          <select size={8} style={{ width: "100%" }} value={sel} data-testid="regulator-remove-list" onChange={(e) => setSel(Number(e.target.value))} onDoubleClick={() => props.names[sel] !== undefined && props.onOk(props.names[sel]!)}>
            {props.names.map((n, i) => (
              <option key={`${i}:${n}`} value={i}>
                {n}
              </option>
            ))}
          </select>
        </div>
        <div className="dialog-footer">
          <button type="button" onClick={props.onCancel} data-testid="regulator-remove-cancel">
            Cancel
          </button>
          <button type="button" className="primary" data-testid="regulator-remove-ok" onClick={() => props.names[sel] !== undefined && props.onOk(props.names[sel]!)}>
            OK
          </button>
        </div>
      </div>
    </div>
  );
}

export function PanelRegulator() {
  const scheme = useIconScheme();
  // `LoadSettings`.
  const [init] = useState(() => {
    const r = getPcbCalculator().settings.regulators;
    return { ...r, last_param: r.last_param >= 3 || r.last_param < 0 ? 0 : r.last_param };
  });
  const [resTol, setResTol] = useState(init.resTol);
  const [r1, setR1] = useState({ min: "", typ: init.r1, max: "" });
  const [r2, setR2] = useState({ min: "", typ: init.r2, max: "" });
  const [vout, setVout] = useState({ min: "", typ: init.voutTyp, max: "" });
  const [vref, setVref] = useState({ min: init.vrefMin, typ: init.vrefTyp, max: init.vrefMax });
  const [iadj, setIadj] = useState({ typ: init.iadjTyp, max: init.iadjMax });
  const [tolTotal, setTolTotal] = useState({ min: "", max: "" });
  const [comment, setComment] = useState("");
  const [message, setMessage] = useState("");
  const [type, setType] = useState(init.type === 0 ? 0 : 1);
  const [solveFor, setSolveFor] = useState(init.last_param);
  const [fileName, setFileName] = useState(dataFilename(init.data_file));
  const [list, setList] = useState<RegulatorData[]>([]);
  const [lastSelected, setLastSelected] = useState(init.selected_regulator);
  const [changed, setChanged] = useState(false);
  const [form, setForm] = useState<{ initial: RegulatorForm; editing: boolean } | null>(null);
  const [removing, setRemoving] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  /** `OnRegulatorSelection` for an item: its type, Vref and Iadj into the boxes (`%g`). */
  const showItem = (item: RegulatorData | null) => {
    if (!item) return;
    setLastSelected(item.name);
    setType(item.type);
    setVref({ min: formatG(item.vrefMin), typ: formatG(item.vrefTyp), max: formatG(item.vrefMax) });
    setIadj({ typ: formatG(item.iadjTyp), max: formatG(item.iadjMax) });
  };
  /** `SelectLastSelectedRegulator`: the remembered name (matched exactly) selected and shown, else nothing selected. */
  const selectLast = (l: readonly RegulatorData[], name: string) => showItem(name ? (l.find((r) => r.name === name) ?? null) : null);

  // `PCB_CALCULATOR_FRAME::loadPages`: `regPanel->ReadDataFile()` after the settings are loaded.
  useEffect(() => {
    const l = readDataFile(dataFilename(init.data_file));
    if (l) {
      setList(l);
      selectLast(l, init.selected_regulator);
    }
  }, []);

  // `SaveSettings` (Vout typ is not stored: KiCad's SaveSettings writes the stored value into the box instead).
  useEffect(() => {
    updatePcbCalculatorSettings((s) => {
      const r = s.regulators;
      r.resTol = resTol;
      r.r1 = r1.typ;
      r.r2 = r2.typ;
      r.vrefMin = vref.min;
      r.vrefTyp = vref.typ;
      r.vrefMax = vref.max;
      r.iadjTyp = iadj.typ;
      r.iadjMax = iadj.max;
      r.data_file = fileName;
      r.selected_regulator = lastSelected;
      r.type = type;
      r.last_param = solveFor;
    });
  }, [resTol, r1.typ, r2.typ, vref, iadj, fileName, lastSelected, type, solveFor]);

  /** `WriteDataFile`: the list into the named file (and to the browser's downloads); false when it cannot be written. */
  const writeDataFile = (): boolean => {
    const name = dataFilename(fileName);
    const text = formatDataFile(list, isoCombinedNow(), TOOL_NAME);
    if (!storeRegulatorFile(name, text)) return false;
    downloadRegulatorFile(name, text);
    setChanged(false);
    return true;
  };

  // `PCB_CALCULATOR_FRAME::OnClosePcbCalc`: an unsaved list is written to the data file, or the person is asked whether to abandon it.
  const guardState = useRef({ changed, fileName, writeDataFile });
  guardState.current = { changed, fileName, writeDataFile };
  useEffect(() => {
    setPcbCalculatorCloseGuard(() => {
      const g = guardState.current;
      if (!g.changed) return true;
      const name = dataFilename(g.fileName);
      if (!name) return window.confirm("No data filename to save modifications.\nDo you want to exit and abandon your changes?");
      if (!g.writeDataFile()) return window.confirm(`Unable to write file '${name}'\nDo you want to exit and abandon your changes?`);
      return true;
    });
    return () => setPcbCalculatorCloseGuard(null);
  }, []);

  // `OnDataFileSelection`: KiCad's save-style file dialog names the file; a browser opens one (a new name is typed into the box).
  const onFilePicked = async (file: File) => {
    const full = dataFilename(file.name);
    if (full === dataFilename(fileName)) return;
    setFileName(full);
    if (list.length > 0 && !window.confirm("Do you want to load this file and replace current regulator list?")) return;
    const text = await file.text();
    storeRegulatorFile(full, text);
    const l = readDataFile(full);
    if (l) {
      setChanged(false);
      setList(l);
      selectLast(l, lastSelected);
    } else window.alert(`Unable to read data file '${full}'.`);
  };

  // `OnRegulatorResetButtonClick`.
  const reset = () => {
    setResTol(DEFAULT_REGULATOR.resTol);
    setR1({ min: "", typ: DEFAULT_REGULATOR.r1, max: "" });
    setR2({ min: "", typ: DEFAULT_REGULATOR.r2, max: "" });
    setVref({ min: DEFAULT_REGULATOR.vrefMin, typ: DEFAULT_REGULATOR.vrefTyp, max: DEFAULT_REGULATOR.vrefMax });
    setVout({ min: "", typ: DEFAULT_REGULATOR.voutTyp, max: "" });
    setIadj({ typ: DEFAULT_REGULATOR.iadjTyp, max: DEFAULT_REGULATOR.iadjMax });
    setTolTotal({ min: "", max: "" });
    setType(1);
    setSolveFor(2);
  };

  // `RegulatorsSolve`.
  const calculate = () => {
    setMessage("");
    const s = regulatorsSolve({ type, solveFor, resTol, r1Typ: r1.typ, r2Typ: r2.typ, vrefMin: vref.min, vrefTyp: vref.typ, vrefMax: vref.max, voutTyp: vout.typ, iadjTyp: iadj.typ, iadjMax: iadj.max });
    if (s.kind === "message") {
      setMessage(s.message);
      return;
    }
    setR1({ min: s.r1Min, typ: s.r1Typ, max: s.r1Max });
    setR2({ min: s.r2Min, typ: s.r2Typ, max: s.r2Max });
    setVout({ min: s.voutMin, typ: s.voutTyp, max: s.voutMax });
    setTolTotal({ min: s.tolTotalMin, max: s.tolTotalMax });
    setComment(s.powerComment);
  };

  const selectedIndex = list.findIndex((r) => r.name === lastSelected);
  const hasItems = list.length > 0;
  const three = type === 1;

  const triple = (label: string, radio: number | null, v: { min: string; typ: string; max: string }, set: (v: { min: string; typ: string; max: string }) => void, unit: string, id: string) => (
    <>
      <span>{radio === null ? null : <input type="radio" name="calc-reg-solve" data-testid={`regulator-solve-${id}`} checked={solveFor === radio} onChange={() => setSolveFor(radio)} />}</span>
      <span>{label}</span>
      {/* min and max are read-only (wxTE_READONLY); typ is typed, or calculated for the row chosen */}
      <CalcResult value={v.min} testid={`regulator-${id}-min`} label={`${label} min`} width={80} />
      <CalcInput value={v.typ} onChange={(t) => set({ ...v, typ: t })} testid={`regulator-${id}-typ`} label={`${label} typ`} width={80} />
      <CalcResult value={v.max} testid={`regulator-${id}-max`} label={`${label} max`} width={80} />
      <span>{unit}</span>
    </>
  );

  return (
    <CalcPanel testid="calc-panel-regulators">
      <div style={{ display: "flex", gap: 14, alignItems: "flex-start", flexWrap: "wrap" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span title={"Type of the regulator.\nThere are 2 types:\n- regulators which have a dedicated sense pin for the voltage regulation.\n- 3 terminal pins."}>Type:</span>
            <Choice options={REG_TYPES} value={type} onChange={setType} testid="regulator-type" label="Type" />
          </div>
          <img src={`/icons/${scheme}/${three ? "regul_3pins" : "regul"}.svg`} alt="" draggable={false} data-testid="regulator-picture" style={{ maxWidth: 300 }} />
          <CalcBox title="Formula">
            <div style={{ fontWeight: 700 }} data-testid="regulator-formula">
              {regulatorFormula(type)}
            </div>
          </CalcBox>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 420 }}>
          <CalcBox title="Regulator">
            <select
              className="calc-choice"
              data-testid="regulator-selector"
              aria-label="Regulator"
              value={selectedIndex}
              style={{ width: "100%" }}
              onChange={(e) => showItem(list[Number(e.target.value)] ?? null)}
            >
              {selectedIndex < 0 ? <option value={-1} /> : null}
              {list.map((r, i) => (
                <option key={`${i}:${r.name}`} value={i}>
                  {r.name}
                </option>
              ))}
            </select>
            <div title="The name of the data file which stores known regulators parameters." style={{ marginTop: 6 }}>
              Regulators data file:
            </div>
            <div style={{ display: "flex", gap: 4 }}>
              <input type="text" className="calc-input" style={{ flex: 1 }} data-testid="regulator-data-file" aria-label="Regulators data file" value={fileName} onChange={(e) => setFileName(e.target.value)} onKeyDown={(e) => e.stopPropagation()} />
              <button type="button" data-testid="regulator-browse" onClick={() => fileInput.current?.click()}>
                Browse
              </button>
              <input
                ref={fileInput}
                type="file"
                accept=".pcbcalc"
                hidden
                data-testid="regulator-file-input"
                onChange={(e) => {
                  const f = e.target.files?.[0];
                  e.target.value = "";
                  if (f) void onFilePicked(f);
                }}
              />
            </div>
            <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
              <button type="button" title="Edit the current selected regulator." data-testid="regulator-edit" disabled={!hasItems} onClick={() => {
                const item = regulatorListGet(list, list[selectedIndex]?.name ?? "");
                if (item) setForm({ initial: regulatorToForm(item), editing: true });
              }}>
                Edit Regulator
              </button>
              <button type="button" title="Enter a new item to the current list of available regulators" data-testid="regulator-add" onClick={() => setForm({ initial: { name: "", vrefMin: "", vrefTyp: "", vrefMax: "", type: 0, iadjTyp: "", iadjMax: "" }, editing: false })}>
                Add Regulator
              </button>
              <button type="button" title="Remove an item from the current list of available regulators" data-testid="regulator-remove-button" disabled={!hasItems} onClick={() => setRemoving(true)}>
                Remove Regulator
              </button>
            </div>
          </CalcBox>
          <div className="calc-grid" style={{ gridTemplateColumns: "auto auto auto auto auto auto" }} data-testid="regulator-params">
            <span />
            <span />
            <span style={{ textAlign: "center" }}>min</span>
            <span style={{ textAlign: "center" }}>typ</span>
            <span style={{ textAlign: "center" }}>max</span>
            <span />
            {triple("R1:", 0, r1, setR1, "kΩ", "r1")}
            {triple("R2:", 1, r2, setR2, "kΩ", "r2")}
            {triple("Vout:", 2, vout, setVout, "V", "vout")}
            <span />
            <span title={"The internal reference voltage of the regulator.\nShould not be 0."}>Vref:</span>
            <CalcInput value={vref.min} onChange={(t) => setVref({ ...vref, min: t })} testid="regulator-vref-min" label="Vref min" width={80} />
            <CalcInput value={vref.typ} onChange={(t) => setVref({ ...vref, typ: t })} testid="regulator-vref-typ" label="Vref typ" width={80} />
            <CalcInput value={vref.max} onChange={(t) => setVref({ ...vref, max: t })} testid="regulator-vref-max" label="Vref max" width={80} />
            <span>V</span>
            {three ? (
              <>
                <span />
                <span title="For 3 terminal regulators only, the  Adjust pin current.">Iadj:</span>
                <span />
                <CalcInput value={iadj.typ} onChange={(t) => setIadj({ ...iadj, typ: t })} testid="regulator-iadj-typ" label="Iadj typ" width={80} />
                <CalcInput value={iadj.max} onChange={(t) => setIadj({ ...iadj, max: t })} testid="regulator-iadj-max" label="Iadj max" width={80} />
                <span>uA</span>
              </>
            ) : null}
            <span />
            <span>Overall tolerance:</span>
            <CalcResult value={tolTotal.min} testid="regulator-tol-min" label="Overall tolerance min" width={80} />
            <span />
            <CalcResult value={tolTotal.max} testid="regulator-tol-max" label="Overall tolerance max" width={80} />
            <span>%</span>
          </div>
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span>Resistor tolerance:</span>
            <CalcInput value={resTol} onChange={setResTol} testid="regulator-restol" label="Resistor tolerance" width={45} />
            <span>%</span>
          </div>
          <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span>Power Comment:</span>
            <input type="text" readOnly className="calc-input calc-result" style={{ width: 200, textAlign: "center" }} data-testid="regulator-comment" aria-label="Power Comment" value={comment} />
            <button type="button" data-testid="regulator-copy" onClick={() => void navigator.clipboard?.writeText(comment).catch(() => undefined)}>
              Copy to Clipboard
            </button>
          </div>
          <div data-testid="regulator-message" style={{ minHeight: 16, padding: "0 10px" }}>
            {message}
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button type="button" className="primary" data-testid="regulator-calculate" onClick={calculate}>
              Calculate
            </button>
            <button type="button" data-testid="regulator-reset" onClick={reset}>
              Reset to Defaults
            </button>
          </div>
        </div>
      </div>
      {form ? (
        <RegulatorFormDialog
          initial={form.initial}
          editing={form.editing}
          onCancel={() => setForm(null)}
          onOk={(f) => {
            const item = regulatorFromForm(f);
            setForm(null);
            if (form.editing) {
              // `OnEditRegulator`: replaced, then `SelectLastSelectedRegulator`.
              const l = regulatorListReplace(list, item);
              setList(l);
              setChanged(true);
              selectLast(l, lastSelected);
            } else if (regulatorListGet(list, item.name) === null) {
              // `OnAddRegulator`
              const l = regulatorListAdd(list, item);
              setList(l);
              setChanged(true);
              setLastSelected(item.name);
              selectLast(l, item.name);
            } else window.alert("This regulator is already in list. Aborted");
          }}
        />
      ) : null}
      {removing ? (
        <RemoveRegulatorDialog
          names={list.map((r) => r.name)}
          onCancel={() => setRemoving(false)}
          onOk={(name) => {
            // `OnRemoveRegulator`
            setRemoving(false);
            const l = regulatorListRemove(list, name);
            setList(l);
            setChanged(true);
            const last = lastSelected === name ? "" : lastSelected;
            setLastSelected(last);
            selectLast(l, last);
          }}
        />
      ) : null}
    </CalcPanel>
  );
}
