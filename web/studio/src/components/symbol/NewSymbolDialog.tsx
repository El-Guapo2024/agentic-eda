// `DIALOG_LIB_NEW_SYMBOL` as `SYMBOL_EDIT_FRAME::CreateNewSymbol` opens it with no parent (eeschema/dialogs/dialog_lib_new_symbol.cpp):
// the symbol's name -- the validator's "Symbol must have a name.", and a name the library already has is refused -- and the properties the
// new symbol starts with: the default reference designator, the units per package with "All units are interchangeable", the alternate body
// style, "Create symbol as power symbol" (which, as KiCad's dialog does, also excludes it from the BOM and the board and locks those two
// boxes), the pin name position offset and the pin number/name text. "Place pin names inside" has no field in this model, and a parent
// symbol is the Derive from Existing Symbol dialog's question, so this one doesn't ask them. OK is one `new_symbol` and its properties (one
// undo step, `SymbolEditorApi.newSymbol`); the new symbol opens in the editor.
import { useEffect, useState } from "react";
import { fetchSymbolEditorNames } from "../../api/client";
import { splitLibName } from "../../kicad-port/libraryNames";
import { newSymbolNameCheck, uniqueSymbolLibId } from "../../kicad-port/symEditActions";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { useStudioState } from "../../state/store";
import { nmFrom, nmTo } from "../../state/units";

/** KiCad's dialog starts the reference at "U". */
const DEFAULT_REFERENCE = "U";
/** `DEFAULT_PIN_NAME_OFFSET` (20 mil) in mm -- `LibrarySymbol::pin_name_offset_mm`'s own default. */
const DEFAULT_PIN_NAME_OFFSET_MM = 0.508;

export function NewSymbolDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const units = useStudioState().units;
  const open = state.newSymbolOpen;

  const [name, setName] = useState("");
  const [reference, setReference] = useState(DEFAULT_REFERENCE);
  const [unitCount, setUnitCount] = useState(1);
  const [interchangeable, setInterchangeable] = useState(true);
  const [alternateBodyStyle, setAlternateBodyStyle] = useState(false);
  const [power, setPower] = useState(false);
  const [excludeFromBom, setExcludeFromBom] = useState(false);
  const [excludeFromBoard, setExcludeFromBoard] = useState(false);
  const [pinNameOffsetMm, setPinNameOffsetMm] = useState(DEFAULT_PIN_NAME_OFFSET_MM);
  const [showPinNumbers, setShowPinNumbers] = useState(true);
  const [showPinNames, setShowPinNames] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Each opening starts fresh, on the free name the action has always made ("Untitled", "Untitled_1", ...) until the person types one.
  useEffect(() => {
    if (!open) return;
    setName("");
    setReference(DEFAULT_REFERENCE);
    setUnitCount(1);
    setInterchangeable(true);
    setAlternateBodyStyle(false);
    setPower(false);
    setExcludeFromBom(false);
    setExcludeFromBoard(false);
    setPinNameOffsetMm(DEFAULT_PIN_NAME_OFFSET_MM);
    setShowPinNumbers(true);
    setShowPinNames(true);
    setError(null);
    setBusy(false);
    let stopped = false;
    void fetchSymbolEditorNames()
      .then((r) => {
        if (!stopped) setName((cur) => (cur === "" ? splitLibName(uniqueSymbolLibId(r.names)).item : cur));
      })
      .catch(() => {
        if (!stopped) setName((cur) => (cur === "" ? "Untitled" : cur));
      });
    return () => {
      stopped = true;
    };
  }, [open]);

  if (!open) return null;
  const close = () => dispatch({ type: "SET_NEW_SYMBOL_OPEN", open: false });

  const ok = async () => {
    // `TransferDataFromWindow` runs the validator against the library's names; it is fetched again here, as the Derive dialog does.
    let existing: string[] = [];
    try {
      const r = await fetchSymbolEditorNames();
      existing = [...r.names, ...(r.project ?? [])];
    } catch {
      /* backend busy: the `new_symbol` verb refuses a duplicate name anyway */
    }
    const check = newSymbolNameCheck(name, existing);
    if (check.error) return setError(check.error);
    // The verb's own words for a taken name (`ops_symbol_exists`). KiCad's dialog would offer "Overwrite" here; `new_symbol` has no such
    // option (the project library's entry would be replaced by an empty one), so the dialog asks for another name instead.
    if (check.exists) return setError("a symbol with this name already exists; pick another name");
    setBusy(true);
    setError(null);
    try {
      const done = await api.newSymbol({
        name: check.name,
        reference,
        unitCount,
        unitsInterchangeable: interchangeable,
        alternateBodyStyle,
        power,
        excludeFromBom,
        excludeFromBoard,
        pinNameOffsetMm,
        showPinNumbers,
        showPinNames,
      });
      if (done) close();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const offset = nmTo(pinNameOffsetMm * 1e6, units);

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 480 }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="New Symbol" data-testid="new-symbol">
        <div className="dialog-header">
          <span>New Symbol</span>
        </div>
        <div className="dialog-body" onKeyDown={(e) => e.key === "Enter" && e.target instanceof HTMLInputElement && e.target.type === "text" && void ok()}>
          <div className="kv-grid" style={{ gridTemplateColumns: "210px 1fr" }}>
            <span title={"The symbol name in library and also the default\nsymbol value when loaded in the schematic."}>Symbol name</span>
            <input
              value={name}
              autoFocus
              onChange={(e) => setName(e.target.value)}
              aria-label="Symbol name"
              data-testid="new-symbol-name"
              style={{ width: 220 }}
            />
            <span>Default reference designator</span>
            <input value={reference} onChange={(e) => setReference(e.target.value)} aria-label="Default reference designator" style={{ width: 80 }} />
            <span>Number of units per package</span>
            <input
              type="number"
              min={1}
              max={64}
              value={unitCount}
              onChange={(e) => Number.isFinite(Number(e.target.value)) && setUnitCount(Math.min(64, Math.max(1, Math.round(Number(e.target.value)))))}
              aria-label="Number of units per package"
              style={{ width: 80 }}
            />
            <span>Pin name position offset</span>
            <span>
              <input
                type="number"
                step="any"
                value={offset}
                onChange={(e) => Number.isFinite(Number(e.target.value)) && setPinNameOffsetMm(Math.max(0, nmFrom(Number(e.target.value), units) / 1e6))}
                aria-label="Pin name position offset"
                style={{ width: 80 }}
              />{" "}
              {units}
            </span>
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 4 }}>
            <label>
              <input type="checkbox" checked={interchangeable} onChange={(e) => setInterchangeable(e.target.checked)} /> All units are interchangeable
            </label>
            <label>
              <input type="checkbox" checked={alternateBodyStyle} onChange={(e) => setAlternateBodyStyle(e.target.checked)} /> Create symbol with alternate body style (De Morgan)
            </label>
            <label>
              <input
                type="checkbox"
                checked={power}
                onChange={(e) => {
                  // `onPowerCheckBox`: a power symbol leaves the BOM and the board, and those two boxes lock while it is one.
                  setPower(e.target.checked);
                  if (e.target.checked) {
                    setExcludeFromBom(true);
                    setExcludeFromBoard(true);
                  }
                }}
              />{" "}
              Create symbol as power symbol
            </label>
            <label style={{ marginLeft: 20 }}>
              <input type="checkbox" checked={excludeFromBom} disabled={power} onChange={(e) => setExcludeFromBom(e.target.checked)} /> Exclude from bill of materials
            </label>
            <label style={{ marginLeft: 20 }}>
              <input type="checkbox" checked={excludeFromBoard} disabled={power} onChange={(e) => setExcludeFromBoard(e.target.checked)} /> Exclude from board
            </label>
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 4 }}>
            <label>
              <input type="checkbox" checked={showPinNumbers} onChange={(e) => setShowPinNumbers(e.target.checked)} /> Show pin number text
            </label>
            <label>
              <input type="checkbox" checked={showPinNames} onChange={(e) => setShowPinNames(e.target.checked)} /> Show pin name text
            </label>
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="new-symbol-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close} data-testid="new-symbol-cancel">
            Cancel
          </button>
          <button className="primary" disabled={busy} onClick={() => void ok()} data-testid="new-symbol-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
