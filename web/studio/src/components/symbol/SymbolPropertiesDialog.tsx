// Port of eeschema/dialogs/dialog_lib_symbol_properties.cpp's General + Units&&Body Styles + Footprint Filters tabs (one scrolling panel here, not a real tab control -- same
// "simpler single-panel dialog" choice this app's other multi-section dialogs already make, e.g. FootprintPropertiesDialog.tsx).
//
// The fields grid is the General tab's `m_grid`: the symbol's fields -- Reference (the prefix), Value, Footprint, Datasheet, Description and the user fields -- each with its look
// and place (components/symbol/SymbolFieldsGrid.tsx). Below it "Keywords" and "Derive from symbol", which is the parent choice of a derived symbol (disabled, "<None>", for a
// root symbol: a symbol is made derived with Derive from Existing Symbol). A derived symbol (`syncControlStates( true )`) hides the lower General panel -- power, pin text and
// attributes are its parent's -- and, here, the units: the drawing is the parent's. Its grid also lists the parent's fields as inherited rows, and a reference may be empty.
// OK is one undo step: `set_symbol_parent`, `edit_symbol_properties` and `edit_library_symbol_fields`.
//
// The lower General panel has KiCad's groups and words: General (Define as power symbol, Define as local power symbol), Attributes (the four "Exclude from" boxes, checked and
// disabled for a power symbol, `onPowerCheckBox`), Pin Text Options (Show pin number, Show pin name, Place pin names inside with its Position offset), Symbol Units (Number of units,
// All units are interchangeable -- `LockUnits`) and Body Styles (single or De Morgan).
//
// Not ported: per-body-style custom names (KiCad's "Custom" body-style mode beyond plain DeMorgan Standard/Alternate), the unit display names grid (the names are kept in the
// file, `LibrarySymbolKeep::unit_names`), the Pin Connections tab (jumper pin groups, kept in the file too) and embedded files.
//
// Named `LibrarySymbolPropertiesDialog` (not `SymbolPropertiesDialog`, already taken by the top-level placed-instance Reference/Value/Footprint/Datasheet dialog) -- same naming
// split `components/footprint/FootprintPropertiesDialog.tsx`'s own `FootprintLibraryPropertiesDialog` already establishes for the Footprint Editor.
import { storedPinNameOffsetMm } from "../../kicad-port/symPinDialog";
import { useEffect, useState } from "react";
import type { Cmd, LibrarySymbol, SymbolPropertiesFields } from "../../api/types";
import { fetchLibrarySymbolFlat, fetchSymbolEditorNames } from "../../api/client";
import { bareName, parentCandidates, wireField } from "../../kicad-port/symFields";
import { gridProblem, gridRowsFor, savedFields, type GridRow } from "../../kicad-port/symFieldsGrid";
import { useSymApi, useSymDispatch, useSymState } from "../../state/symbolEditorStore";
import { notifyLibraryChanged } from "../library/useLibraryNames";
import { SymbolFieldsGrid } from "./SymbolFieldsGrid";

const FORM_KEYS: (keyof SymbolPropertiesFields)[] = ["reference_prefix", "description", "keywords", "datasheet", "power", "in_bom", "on_board", "pin_numbers_hidden", "pin_names_hidden", "pin_name_offset_mm", "unit_count", "has_alternate_body_style", "footprint_filters"];
/** The flags the backend leaves out of the JSON when they are off. */
const FLAG_KEYS = ["units_locked", "power_local", "exclude_from_sim", "exclude_from_pos_files"] as const;


/** The properties `edit_symbol_properties` takes, from a symbol. */
function formOf(sym: LibrarySymbol): SymbolPropertiesFields {
  const out: Record<string, unknown> = {};
  for (const k of FORM_KEYS) out[k] = sym[k];
  for (const k of FLAG_KEYS) out[k] = sym[k] ?? false;
  return out as unknown as SymbolPropertiesFields;
}

export function LibrarySymbolPropertiesDialog() {
  const state = useSymState();
  const dispatch = useSymDispatch();
  const api = useSymApi();
  const own = state.derivedOwn;
  /** What the dialog edits: the symbol's own entry (a derived symbol's fields are its own; the editor draws the flat one). */
  const subject = own ?? state.symbol;
  const derived = own !== null;
  const close = () => dispatch({ type: "SET_PROPERTIES_OPEN", open: false });

  const [form, setForm] = useState<SymbolPropertiesFields | null>(null);
  const [filtersText, setFiltersText] = useState("");
  const [rows, setRows] = useState<GridRow[]>([]);
  const [parent, setParent] = useState("");
  const [candidates, setCandidates] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** "Place pin names inside": `m_PinsNameInsideButt->SetValue( GetPinNameOffset() != 0 )`. */
  const [namesInside, setNamesInside] = useState(true);

  useEffect(() => {
    if (!subject || !state.propertiesOpen) return;
    let stopped = false;
    setForm(formOf(subject));
    setNamesInside(subject.pin_name_offset_mm !== 0);
    setFiltersText(subject.footprint_filters.join(" "));
    setError(null);
    setParent(subject.extends ?? "");
    setRows(gridRowsFor(subject, null));
    if (subject.extends) {
      // the parent's own fields join the grid as inherited rows (`addInheritedFields`)
      void fetchLibrarySymbolFlat(subject.extends)
        .then((flat) => !stopped && setRows(gridRowsFor(subject, flat)))
        .catch(() => undefined);
      // the symbols it can derive from (`m_inheritanceSelectCombo`)
      void fetchSymbolEditorNames()
        .then((r) => !stopped && setCandidates(parentCandidates(subject.lib_id, r.names, r.derived ?? {})))
        .catch(() => undefined);
    }
    return () => {
      stopped = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.propertiesOpen]);

  if (!state.propertiesOpen || !subject || !form) return null;

  const set = <K extends keyof SymbolPropertiesFields>(key: K, value: SymbolPropertiesFields[K]) => setForm((f) => (f ? { ...f, [key]: value } : f));
  /** `onPowerCheckBox`: a power symbol is excluded from simulation, the BOM, the board and the position files (the boxes checked and disabled); unchecked, they are enabled
   * again as they are. */
  const setPower = (power: boolean) =>
    setForm((f) => (f ? (power ? { ...f, power, exclude_from_sim: true, in_bom: false, on_board: false, exclude_from_pos_files: true } : { ...f, power }) : f));

  const submit = async () => {
    const problem = gridProblem(rows, !derived);
    if (problem) return setError(problem);
    if (derived && parent === "") return setError("Derived symbol must have a parent selected");
    const footprint_filters = filtersText
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);
    // "Power symbols must have value matching name for now"
    const out = !derived && form.power ? rows.map((r) => (r.name === "Value" ? { ...r, text: bareName(subject.lib_id) } : r)) : rows;
    const text = (name: string) => out.find((r) => r.name === name)?.text ?? "";
    // "Place pin names inside": the offset, or `offset == 0 ? 20 : offset` in internal units (0.002 mm, "We interpret an offset of 0 as outside"); unchecked, 0
    const pin_name_offset_mm = storedPinNameOffsetMm(namesInside, form.pin_name_offset_mm, "symbol_properties");
    // `LockUnits( GetUnitCount() > 1 && !interchangeable )`; `SetLocalPower` only for a power symbol
    const units_locked = form.unit_count > 1 && !!form.units_locked;
    const power_local = form.power && !!form.power_local;
    const props: SymbolPropertiesFields = { ...form, pin_name_offset_mm, units_locked, power_local, reference_prefix: text("Reference"), description: text("Description"), datasheet: text("Datasheet"), footprint_filters };
    const cmds: Cmd[] = [];
    if (derived && parent !== subject.extends) cmds.push({ op: "set_symbol_parent", lib_id: subject.lib_id, parent });
    cmds.push({ op: "edit_symbol_properties", lib_id: subject.lib_id, ...props });
    cmds.push({ op: "edit_library_symbol_fields", lib_id: subject.lib_id, fields: savedFields(out).map(wireField) });
    setBusy(true);
    try {
      const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      notifyLibraryChanged();
      // "if units are locked, graphic items are specific to each unit": `SetDrawSpecificUnit( symbol->UnitsLocked() )` after OK
      if (ok && !derived) dispatch({ type: "SET_DRAW_SPECIFIC", unit: units_locked });
      if (ok) close();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 760, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Symbol Properties" data-testid="symbol-properties">
        <div className="dialog-header">
          <span>Symbol Properties</span>
          <span>{subject.lib_id}</span>
        </div>
        <div className="dialog-body" style={{ maxHeight: "74vh", overflowY: "auto" }}>
          <SymbolFieldsGrid rows={rows} onChange={setRows} parentName={parent ? bareName(parent) : undefined} onError={setError} initialField={state.propertiesField} />
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginTop: 10 }}>
            <span>Keywords:</span>
            <input value={form.keywords} onChange={(e) => set("keywords", e.target.value)} style={{ width: 300 }} data-testid="props-keywords" />
            <span title={"Select symbol to derive this symbol from or select\n<None> for root symbol."} style={{ opacity: derived ? 1 : 0.5 }}>
              Derive from symbol:
            </span>
            <select value={derived ? parent : ""} disabled={!derived} onChange={(e) => setParent(e.target.value)} aria-label="Derive from symbol" data-testid="props-parent" style={{ width: 306 }}>
              {!derived && <option value="">&lt;None&gt;</option>}
              {derived && !candidates.includes(parent) && <option value={parent}>{bareName(parent)}</option>}
              {candidates.map((c) => (
                <option key={c} value={c}>
                  {bareName(c)}
                </option>
              ))}
            </select>
          </div>
          {!derived && (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "6px 24px", marginTop: 10 }}>
              <fieldset>
                <legend>General</legend>
                <label title={"Power symbols define a global net with the value as a netname.\nThey will not be included in the BOM and cannot be assigned a footprint."}>
                  <input type="checkbox" checked={form.power} onChange={(e) => setPower(e.target.checked)} data-testid="props-power" /> Define as power symbol
                </label>
                <br />
                <label title="Local power symbols create labels that are limited to the sheet in which they are used" style={{ opacity: form.power ? 1 : 0.5 }}>
                  <input type="checkbox" checked={!!form.power_local} disabled={!form.power} onChange={(e) => set("power_local", e.target.checked)} data-testid="props-power-local" /> Define as local power symbol
                </label>
              </fieldset>
              <fieldset>
                <legend>Attributes</legend>
                {(
                  [
                    ["exclude_from_sim", "Exclude from simulation", !!form.exclude_from_sim],
                    ["in_bom", "Exclude from bill of materials", !form.in_bom],
                    ["on_board", "Exclude from board", !form.on_board],
                    ["exclude_from_pos_files", "Exclude from position files", !!form.exclude_from_pos_files],
                  ] as const
                ).map(([key, label, checked]) => (
                  <div key={key}>
                    <label style={{ opacity: form.power ? 0.5 : 1 }}>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={form.power}
                        data-testid={`props-${key}`}
                        onChange={(e) => (key === "in_bom" || key === "on_board" ? set(key, !e.target.checked) : set(key, e.target.checked))}
                      />{" "}
                      {label}
                    </label>
                  </div>
                ))}
              </fieldset>
              <fieldset>
                <legend>Pin Text Options</legend>
                <label title="Show or hide pin numbers">
                  <input type="checkbox" checked={!form.pin_numbers_hidden} onChange={(e) => set("pin_numbers_hidden", !e.target.checked)} /> Show pin number
                </label>
                <br />
                <label title="Show or hide pin names">
                  <input type="checkbox" checked={!form.pin_names_hidden} onChange={(e) => set("pin_names_hidden", !e.target.checked)} /> Show pin name
                </label>
                <br />
                <label title={"Check this option to have pin names inside the body and pin number outside.\nIf not checked pins names and pins numbers are outside."}>
                  <input type="checkbox" checked={namesInside} onChange={(e) => setNamesInside(e.target.checked)} data-testid="props-names-inside" /> Place pin names inside
                </label>
                <br />
                <label title="Margin between the pin name position and the symbol body." style={{ opacity: namesInside ? 1 : 0.5 }}>
                  Position offset:{" "}
                  <input type="number" step="any" min={0} disabled={!namesInside} value={form.pin_name_offset_mm} onChange={(e) => Number.isFinite(Number(e.target.value)) && set("pin_name_offset_mm", Number(e.target.value))} style={{ width: 80 }} data-testid="props-pin-name-offset" />{" "}
                  mm
                </label>
              </fieldset>
              <fieldset>
                <legend>Symbol Units</legend>
                <label title="Enter the number of units for a symbol that contains more than one unit">
                  Number of units: <input type="number" min={1} max={64} value={form.unit_count} onChange={(e) => set("unit_count", Math.max(1, Math.min(64, Math.round(Number(e.target.value) || 1))))} style={{ width: 60 }} data-testid="props-unit-count" />
                </label>
                <br />
                <label
                  title={"Check this option when all symbol units have the same function.\nFor instance, this should be checked for a quad NAND gate, while it should not be checked for a dual triode (where unit C is the filament)."}
                  style={{ opacity: form.unit_count > 1 ? 1 : 0.5 }}
                >
                  <input type="checkbox" checked={!form.units_locked || form.unit_count === 1} disabled={form.unit_count <= 1} onChange={(e) => set("units_locked", !e.target.checked)} data-testid="props-interchangeable" /> All units are interchangeable
                </label>
              </fieldset>
              <fieldset>
                <legend>Body Styles</legend>
                <label>
                  <input type="radio" name="body-styles" checked={!form.has_alternate_body_style} onChange={() => set("has_alternate_body_style", false)} /> Single body style
                </label>
                <br />
                <label>
                  <input type="radio" name="body-styles" checked={form.has_alternate_body_style} onChange={() => set("has_alternate_body_style", true)} data-testid="props-demorgan" /> ‘Standard’ and ‘Alternate’ De Morgan body styles
                </label>
              </fieldset>
            </div>
          )}
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginTop: 10 }}>
            <span>Footprint filters</span>
            <input value={filtersText} onChange={(e) => setFiltersText(e.target.value)} placeholder="SOIC* TSSOP*" style={{ width: 300 }} data-testid="props-filters" />
          </div>
          {error && (
            <p style={{ color: "var(--chrome-danger)", margin: "8px 0 0" }} data-testid="props-error">
              {error}
            </p>
          )}
        </div>
        <div className="dialog-footer">
          <button onClick={close}>Cancel</button>
          <button className="primary" disabled={busy} onClick={() => void submit()} data-testid="props-ok">
            OK
          </button>
        </div>
      </div>
    </div>
  );
}
