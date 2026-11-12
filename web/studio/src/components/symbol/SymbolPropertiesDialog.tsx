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
// disabled when "Define as power symbol" is ticked, `onPowerCheckBox`, and enabled when the dialog opens, power symbol or not), Pin Text Options (Show pin number, Show pin name, Place pin names inside with its Position offset), Symbol Units (Number of units,
// All units are interchangeable -- `LockUnits`) and Body Styles (single or De Morgan).
//
// The Units & Body Styles tab's "Unit display names (optional):" grid (a row per unit, its letters and its name: `updateUnitCount` adds and drops rows with the count) and
// the "Custom body styles:" radio with its grid of names (Add, Move up, Move down, Delete; enabled only for custom styles, `syncBodyStyleControls`), and the Pin Connections
// tab ("All pins with duplicate numbers are jumpers", "Explicit jumper pin groups:" with Add and Remove) are KiCad 25ec2335's (UI-LEFT Y4): `Validate`'s "The unit name '%s'
// is already in use." and "Custom body styles must have at least 2 entries", `OnBodyStyle`'s refusal of several body styles on a symbol with alternate pin functions, and
// `TransferDataFromWindow`'s order -- the names, the body styles (`SetBodyStyleCount` first), the jumper groups ("Pin '%s' in jumper pin group %d does not exist in this
// symbol.") -- in `edit_symbol_units_and_connections`, after `edit_symbol_properties` in the same step. A derived symbol's are its parent's: hidden, as the lower panel is.
// The Embedded Files tab (`PANEL_EMBEDDED_FILES` on `m_libEntry->GetEmbeddedFiles()`, UI-LEFT-2 YY3) is a section of the panel: the library symbol's list, sent with OK
// in the same step before a rename (which takes the list along, `rename_library_symbol`). Not ported: KiCad 25ec2335's Pin Map tab (named pin-to-pad maps, file
// version 20260629, which a KiCad 10 file -- this writer's 20251024 -- cannot hold: 25ec2335's own KiCad 10 writer drops them).
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
import { askConfirm, showError } from "../library/libraryDialogs";
import { escapeLibIdName, joinLibName, PROJECT_LIBRARY, splitLibName, symbolNameInUse } from "../../kicad-port/libraryNames";
import { unescapeString } from "../../kicad-port/schNetName";
import { symbolPropertiesQuestions } from "../../kicad-port/symUnits";
import { SymbolFieldsGrid } from "./SymbolFieldsGrid";
import { bodyStyleChoiceRefusal, bodyStyleCountOf, bodyStyleModeOf, jumperRowsOf, resizeUnitNameRows, unitNameRowsOf, unitsAndBodyStylesProblem, type BodyStyleMode } from "../../kicad-port/symBodyStyles";
import { letterSubReference } from "../../kicad-port/unitLetter";
import { PinNumbers } from "../../kicad-port/pinNumbersSummary";
import { EmbeddedFilesPanel } from "../EmbeddedFilesPanel";
import { applyCmd as embeddedApplyCmd, EMPTY_DRAFT, type EmbeddedDraft } from "../../kicad-port/embeddedFiles";
import type { EmbeddedListing } from "../../api/embeddedTypes";

/** `PIN_NUMBERS::GetSummary` of some pin numbers. */
function pinSummary(numbers: string[]): string {
  const set = new PinNumbers();
  for (const n of numbers) set.insert(n);
  return set.summary();
}

/** A one-column grid of rows with the buttons KiCad gives it (`WX_GRID::OnAddRow`, `OnMoveRowUp` / `Down`, `OnDeleteRows`): the selected row moves or goes. */
function RowsGrid({ rows, onChange, disabled, testid, buttons }: { rows: string[]; onChange: (rows: string[]) => void; disabled?: boolean; testid: string; buttons: "all" | "add-remove" }) {
  const [sel, setSel] = useState<number | null>(null);
  const swap = (a: number, b: number) => {
    const next = [...rows];
    [next[a], next[b]] = [next[b]!, next[a]!];
    onChange(next);
    setSel(b);
  };
  return (
    <div style={{ opacity: disabled ? 0.5 : 1 }}>
      <div style={{ border: "1px solid var(--chrome-border, #888)", minHeight: 48, maxHeight: 120, overflowY: "auto" }} data-testid={testid}>
        {rows.map((r, i) => (
          <input
            key={i}
            value={r}
            disabled={disabled}
            onFocus={() => setSel(i)}
            onChange={(e) => onChange(rows.map((x, k) => (k === i ? e.target.value : x)))}
            style={{ display: "block", width: "100%", boxSizing: "border-box", outline: sel === i ? "1px solid var(--chrome-accent, #48f)" : undefined }}
            data-testid={`${testid}-row-${i}`}
          />
        ))}
      </div>
      <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
        <button type="button" title="Add field" disabled={disabled} onClick={() => (onChange([...rows, ""]), setSel(rows.length))} data-testid={`${testid}-add`}>
          +
        </button>
        {buttons === "all" && (
          <>
            <button type="button" title="Move up" disabled={disabled || sel === null || sel <= 0} onClick={() => sel !== null && swap(sel, sel - 1)} data-testid={`${testid}-up`}>
              ↑
            </button>
            <button type="button" title="Move down" disabled={disabled || sel === null || sel >= rows.length - 1} onClick={() => sel !== null && swap(sel, sel + 1)} data-testid={`${testid}-down`}>
              ↓
            </button>
          </>
        )}
        <span style={{ width: 20 }} />
        <button
          type="button"
          title={buttons === "all" ? "Delete field" : "Remove group"}
          disabled={disabled || sel === null || sel >= rows.length}
          onClick={() => {
            if (sel === null) return;
            onChange(rows.filter((_, k) => k !== sel));
            setSel(rows.length > 1 ? Math.min(sel, rows.length - 2) : null);
          }}
          data-testid={`${testid}-delete`}
        >
          🗑
        </button>
      </div>
    </div>
  );
}

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
  /** The Number of units field's text while it is typed in (`null`: it shows the form's count). */
  const [unitCountText, setUnitCountText] = useState<string | null>(null);
  const [filtersText, setFiltersText] = useState("");
  const [rows, setRows] = useState<GridRow[]>([]);
  const [parent, setParent] = useState("");
  const [candidates, setCandidates] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** "Symbol name:" (`m_SymbolNameCtrl`), shown unescaped (`UnescapeString( m_libEntry->GetName() )`). */
  const [symbolName, setSymbolName] = useState("");
  /** "Place pin names inside": `m_PinsNameInsideButt->SetValue( GetPinNameOffset() != 0 )`. */
  const [namesInside, setNamesInside] = useState(true);
  /** The four "Exclude from" boxes are disabled by `onPowerCheckBox` only: `TransferDataToWindow` sets their values and leaves them enabled, so a power symbol
   *  opens with them editable, and ticking "Define as power symbol" checks and disables them (r7 E7-S9). */
  const [excludeLocked, setExcludeLocked] = useState(false);
  /** "Unit display names (optional):" (`m_unitNamesGrid`): one row per unit. */
  const [unitRows, setUnitRows] = useState<string[]>([]);
  /** The Body Styles radio buttons and the "Custom body styles:" grid (`m_bodyStyleNamesGrid`). */
  const [bodyMode, setBodyMode] = useState<BodyStyleMode>("single");
  const [bodyNames, setBodyNames] = useState<string[]>([]);
  /** The Pin Connections tab: `m_cbDuplicatePinsAreJumpers` and `m_jumperGroupsGrid`. */
  const [dupJumpers, setDupJumpers] = useState(false);
  const [jumperRows, setJumperRows] = useState<string[]>([]);
  /** The Embedded Files tab's copy of the symbol's list (`PANEL_EMBEDDED_FILES::m_localFiles`), which OK sends. */
  const [embedded, setEmbedded] = useState<{ draft: EmbeddedDraft; listing: EmbeddedListing | null }>({ draft: EMPTY_DRAFT, listing: null });

  useEffect(() => {
    if (!subject || !state.propertiesOpen) return;
    let stopped = false;
    setForm(formOf(subject));
    setSymbolName(unescapeString(bareName(subject.lib_id)));
    setNamesInside(subject.pin_name_offset_mm !== 0);
    setExcludeLocked(false);
    setFiltersText(subject.footprint_filters.join(" "));
    setError(null);
    setParent(subject.extends ?? "");
    setRows(gridRowsFor(subject, null));
    setUnitRows(unitNameRowsOf(subject));
    const styles = bodyStyleModeOf(subject);
    setBodyMode(styles.mode);
    setBodyNames(styles.names);
    setDupJumpers(!!(subject.kicad as { duplicate_pin_numbers_are_jumpers?: boolean } | undefined)?.duplicate_pin_numbers_are_jumpers);
    setJumperRows(jumperRowsOf(subject));
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
  const setPower = (power: boolean) => {
    setExcludeLocked(power);
    setForm((f) => (f ? (power ? { ...f, power, exclude_from_sim: true, in_bom: false, on_board: false, exclude_from_pos_files: true } : { ...f, power }) : f));
  };

  /** `OnBodyStyle` (KiCad 25ec2335): several body styles are refused while a pin has alternate functions, and the choice goes back to one. */
  const chooseBodyStyle = (mode: BodyStyleMode) => {
    const refusal = bodyStyleChoiceRefusal(mode, subject.pins, pinSummary);
    if (refusal) {
      // `DisplayErrorMessage( this, NO_BODY_STYLES_WITH_ALTERNATE_PIN_FUNCTIONS, "(Pins %s.)" )` (UI-LEFT-2 YY6: KiCad's message box, not a line of the dialog)
      const [text, extra] = refusal.split("\n");
      setBodyMode("single");
      void showError(text!, extra);
      return;
    }
    setError(null);
    setBodyMode(mode);
  };
  /** What OK sets of the body styles: De Morgan's flag, or the grid's non-empty names. */
  const demorganOut = bodyMode === "demorgan";
  const namesOut = bodyMode === "custom" ? bodyNames.filter((n) => n !== "") : [];

  const submit = async () => {
    // `Validate`'s `m_delayedErrorMessage`, shown by `DisplayError` once the dialog is back (UI-LEFT-2 YY6)
    const problem = gridProblem(rows, !derived);
    if (problem) return void showError(problem);
    if (derived && parent === "") return void showError("Derived symbol must have a parent selected");
    // KiCad 25ec2335 `Validate`: the unit names in use and the custom body styles' two entries
    const unitsProblem = derived ? null : unitsAndBodyStylesProblem(unitRows, bodyMode, bodyNames);
    if (unitsProblem) return void showError(unitsProblem);
    // `Validate`: "Confirm destructive actions." -- the dropped units' and body styles' items are deleted (`SetUnitCount`, `SetBodyStyleCount`)
    const after = { ...form, has_alternate_body_style: demorganOut, body_style_names: namesOut };
    for (const question of symbolPropertiesQuestions(subject, after, derived)) {
      if (!(await askConfirm({ title: "Confirmation", message: question, okLabel: "Yes", cancelLabel: "No" }))) return;
    }
    // `TransferDataFromWindow`: the name, escaped for a LIB_ID; an empty one, or one the library has (`SymbolNameInUse`, any case), is refused
    const { lib } = splitLibName(subject.lib_id);
    const newName = escapeLibIdName(symbolName);
    // `wxMessageBox( _( "Symbol must have a name." ) )`
    if (newName === "") return void showError("Symbol must have a name.", undefined, "Message");
    const newLibId = joinLibName(lib, newName);
    let renamed = false;
    if (newLibId !== subject.lib_id) {
      try {
        const { names } = await fetchSymbolEditorNames();
        if (symbolNameInUse(newLibId, names.filter((n) => n !== subject.lib_id))) return void showError(`Symbol name '${unescapeString(newName)}' already in use in library '${lib || PROJECT_LIBRARY}'.`);
      } catch {
        /* the server refuses a taken name as well */
      }
      renamed = true;
    }
    const target = renamed ? newLibId : subject.lib_id;
    const footprint_filters = filtersText
      .split(/\s+/)
      .map((s) => s.trim())
      .filter(Boolean);
    // "Power symbols must have value matching name for now": `GetValueField().SetText( newName )`
    const out = !derived && form.power ? rows.map((r) => (r.name === "Value" ? { ...r, text: newName } : r)) : rows;
    const text = (name: string) => out.find((r) => r.name === name)?.text ?? "";
    // "Place pin names inside": the offset, or `offset == 0 ? 20 : offset` in internal units (0.002 mm, "We interpret an offset of 0 as outside"); unchecked, 0
    const pin_name_offset_mm = storedPinNameOffsetMm(namesInside, form.pin_name_offset_mm, "symbol_properties");
    // `LockUnits( GetUnitCount() > 1 && !interchangeable )`; `SetLocalPower` only for a power symbol
    const units_locked = form.unit_count > 1 && !!form.units_locked;
    const power_local = form.power && !!form.power_local;
    const props: SymbolPropertiesFields = { ...form, pin_name_offset_mm, units_locked, power_local, reference_prefix: text("Reference"), description: text("Description"), datasheet: text("Datasheet"), footprint_filters };
    const cmds: Cmd[] = [];
    // `m_embeddedFiles->TransferDataFromWindow()`, on the symbol's list under the name it has before the rename (which carries it)
    const filesCmd = embeddedApplyCmd({ kind: "library_symbol", lib_id: subject.lib_id }, embedded.listing, embedded.draft);
    if (filesCmd) cmds.push(filesCmd);
    // `UNDO_REDO::LIB_RENAME`: the rename comes first, so the fields the grid says (the Value among them) are set on the renamed symbol as typed
    if (renamed) cmds.push({ op: "rename_library_symbol", lib_id: subject.lib_id, new_lib_id: target, overwrite: false });
    if (derived && parent !== subject.extends) cmds.push({ op: "set_symbol_parent", lib_id: target, parent });
    // the body styles go through `edit_symbol_units_and_connections` (`SetBodyStyleCount` from the symbol's own count): this one keeps the De Morgan flag as it is
    cmds.push({ op: "edit_symbol_properties", lib_id: target, ...props, has_alternate_body_style: derived ? form.has_alternate_body_style : !!subject.has_alternate_body_style });
    if (!derived) {
      cmds.push({
        op: "edit_symbol_units_and_connections",
        lib_id: target,
        unit_names: Object.fromEntries(resizeUnitNameRows(unitRows, form.unit_count).flatMap((n, i) => (n ? [[String(i + 1), n]] : []))),
        demorgan: demorganOut,
        body_style_names: namesOut,
        duplicate_pin_numbers_are_jumpers: dupJumpers,
        jumper_rows: jumperRows,
      });
    }
    cmds.push({ op: "edit_library_symbol_fields", lib_id: target, fields: savedFields(out).map(wireField) });
    setBusy(true);
    try {
      const ok = await api.cmd(cmds.length === 1 ? cmds[0]! : { op: "batch", cmds });
      notifyLibraryChanged();
      if (ok && renamed) await api.openSymbol(target, undefined, { keepHistory: true }); // `UNDO_REDO::LIB_RENAME` is an undo step: the reopening keeps the history
      // `RebuildSymbolUnitAndBodyStyleLists`: a unit or body style the symbol no longer has goes back to the first
      if (ok && !derived && state.activeUnit > form.unit_count) dispatch({ type: "SET_ACTIVE_UNIT", unit: 1 });
      if (ok && !derived && state.activeBodyStyle > bodyStyleCountOf(bodyMode, bodyNames)) dispatch({ type: "SET_ACTIVE_BODY_STYLE", style: 1 });
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
          <div className="kv-grid" style={{ gridTemplateColumns: "180px 1fr", marginBottom: 8 }}>
            <span>Symbol name:</span>
            <input
              value={symbolName}
              onChange={(e) => {
                setSymbolName(e.target.value);
                // `OnSymbolNameText`: a power symbol's Value follows its name
                if (!derived && form.power) setRows((rs) => rs.map((r) => (r.name === "Value" ? { ...r, text: e.target.value } : r)));
              }}
              style={{ width: 300 }}
              aria-label="Symbol name"
              data-testid="props-symbol-name"
            />
          </div>
          <SymbolFieldsGrid rows={rows} onChange={setRows} parentName={parent ? bareName(parent) : undefined} onError={(m) => (m ? void showError(m) : setError(null))} initialField={state.propertiesField} datasheetStack={[...(subject.extends ? [{ kind: "library_symbol" as const, lib_id: subject.extends }] : []), { kind: "library_symbol", lib_id: subject.lib_id }]} />
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
                    <label style={{ opacity: excludeLocked ? 0.5 : 1 }}>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={excludeLocked}
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
                  Number of units:{" "}
                  {/* a `wxSpinCtrl` takes what is typed and keeps it in range when it is left: the text is the field's while it is edited (an empty field
                      was put back to 1 at once, so selecting "1" and typing "2" made 12 -- UI-LEFT-2 YY8's click-through) */}
                  <input
                    type="number"
                    min={1}
                    max={64}
                    value={unitCountText ?? form.unit_count}
                    onChange={(e) => {
                      setUnitCountText(e.target.value);
                      if (e.target.value.trim() === "" || !Number.isFinite(Number(e.target.value))) return;
                      const n = Math.max(1, Math.min(64, Math.round(Number(e.target.value))));
                      set("unit_count", n);
                      // `OnUnitSpinCtrl` -> `updateUnitCount`: the names grid follows the count
                      setUnitRows((r) => resizeUnitNameRows(r, n));
                    }}
                    onBlur={() => setUnitCountText(null)}
                    style={{ width: 60 }}
                    data-testid="props-unit-count"
                  />
                </label>
                <br />
                <label
                  title={"Check this option when all symbol units have the same function.\nFor instance, this should be checked for a quad NAND gate, while it should not be checked for a dual triode (where unit C is the filament)."}
                  style={{ opacity: form.unit_count > 1 ? 1 : 0.5 }}
                >
                  <input type="checkbox" checked={!form.units_locked || form.unit_count === 1} disabled={form.unit_count <= 1} onChange={(e) => set("units_locked", !e.target.checked)} data-testid="props-interchangeable" /> All units are interchangeable
                </label>
                <div style={{ marginTop: 10 }}>Unit display names (optional):</div>
                <div style={{ border: "1px solid var(--chrome-border, #888)", maxHeight: 120, overflowY: "auto" }} data-testid="props-unit-names">
                  {unitRows.map((name, i) => (
                    <div key={i} style={{ display: "grid", gridTemplateColumns: "36px 1fr" }}>
                      <span style={{ padding: "0 4px" }}>{letterSubReference(i + 1)}</span>
                      <input value={name} onChange={(e) => setUnitRows((r) => r.map((x, k) => (k === i ? e.target.value : x)))} aria-label={`Unit ${letterSubReference(i + 1)} display name`} data-testid={`props-unit-name-${i + 1}`} />
                    </div>
                  ))}
                </div>
              </fieldset>
              <fieldset>
                <legend>Body Styles</legend>
                <label>
                  <input type="radio" name="body-styles" checked={bodyMode === "single"} onChange={() => chooseBodyStyle("single")} data-testid="props-single-style" /> Single body style
                </label>
                <br />
                <label>
                  <input type="radio" name="body-styles" checked={bodyMode === "demorgan"} onChange={() => chooseBodyStyle("demorgan")} data-testid="props-demorgan" /> ‘Standard’ and ‘Alternate’ De Morgan body styles
                </label>
                <br />
                <label>
                  <input type="radio" name="body-styles" checked={bodyMode === "custom"} onChange={() => chooseBodyStyle("custom")} data-testid="props-custom-styles" /> Custom body styles:
                </label>
                <div style={{ marginLeft: 20 }}>
                  <RowsGrid rows={bodyNames} onChange={setBodyNames} disabled={bodyMode !== "custom"} testid="props-body-style-names" buttons="all" />
                </div>
              </fieldset>
              <fieldset style={{ gridColumn: "1 / span 2" }} data-testid="props-pin-connections">
                <legend>Pin Connections</legend>
                <label title="When enabled, this symbol can have more than one pin with the same number.  Pins with the same number will be considered to be jumpered together internally.">
                  <input type="checkbox" checked={dupJumpers} onChange={(e) => setDupJumpers(e.target.checked)} data-testid="props-duplicate-jumpers" /> All pins with duplicate numbers are jumpers
                </label>
                <div style={{ marginTop: 6 }}>Explicit jumper pin groups:</div>
                <RowsGrid rows={jumperRows} onChange={setJumperRows} testid="props-jumper-groups" buttons="add-remove" />
              </fieldset>
            </div>
          )}
          <fieldset style={{ marginTop: 10 }} data-testid="props-embedded-files">
            <legend>Embedded Files</legend>
            <EmbeddedFilesPanel owner={{ kind: "library_symbol", lib_id: subject.lib_id }} run={api.cmd} draft={embedded.draft} onDraft={(draft, listing) => setEmbedded({ draft, listing })} />
          </fieldset>
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
