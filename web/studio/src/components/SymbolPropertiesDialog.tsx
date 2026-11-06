// Symbol Properties (`E` on a symbol, a double-click; `SCH_EDIT_TOOL::Properties` -> `DIALOG_SYMBOL_PROPERTIES`, eeschema/dialogs/dialog_symbol_properties.cpp and
// dialog_symbol_properties_base.cpp at 8303b2ad). Two tabs, General and Pin Functions:
//
//   * General, the Fields grid (`FIELDS_GRID_TABLE`): each field's Name, Value, Show, Show Name, H Align, V Align, Italic, Bold, Text Size, Font and Color. The
//     Reference, Value, Footprint and Datasheet first, then the user-defined fields and a row for each template of Schematic Setup > Field Name Templates the
//     symbol lacks ("Add in any template fieldnames not yet defined"; OK adds it). Add Field makes one, hidden (`OnAddField`), the x beside a user field removes
//     it. The Footprint's Browse... opens the Footprint Chooser. A field's looks are the field verbs (`edit_field`, `set_field_looks`).
//   * General, the General box: the Unit (`m_unitChoice`, the units' display names, disabled for one unit) and the Body style (`m_bodyStyleChoice`: Standard /
//     Alternate, disabled for one body style). The Attributes box: the five exclusion flags.
//   * General, the buttons: Update Symbol from Library... and Change Symbol... (OK, then `DIALOG_CHANGE_SYMBOLS` in update or change mode on this symbol), Edit
//     Symbol... and Edit Library Symbol... (OK, then the Symbol Editor on the symbol, its unit and body style: `SYMBOL_PROPS_EDIT_SCHEMATIC_SYMBOL` /
//     `SYMBOL_PROPS_EDIT_LIBRARY_SYMBOL`; the studio keeps one copy of a library symbol, so both open the same one).
//   * Pin Functions (`SCH_PIN_TABLE_DATA_MODEL`, kicad-port/symbolPropsDialog.ts): every pin, sorted by number (a header click sorts), the alternate assigned to it
//     chosen from its own name and its alternates, its electrical type and graphic style as the alternate makes them; disabled for a symbol of several body styles.
//
// OK is one undo step: the texts, the fields added and removed, the looks, the flags, the body style, the pin assignments (every placed unit of the reference on the
// sheet, `SyncOtherUnits`), the unit, and a new reference last (a rename moves what is kept under the old one).
//
// Design variants (KiCad 25ec2335 `DIALOG_SYMBOL_PROPERTIES`): the title names the current variant (" - <variant> Design Variant"), the texts and the Attributes box
// ("Exclude from simulation", "Exclude from board", "Do not populate", "Exclude from bill of materials", "Exclude from position files") show the variant's values
// (`GetFieldText( .., variant )`, `GetDNP( sheet, variant )`) and OK sets them in that variant (`SetText( text, sheet, variant )`, `SetDNP( .., variant )`: one
// `set_symbol_variant`); the Reference is set only in the default variant, and adding or removing a field, its visibility, its looks and the body style are the symbol's.
//
// U, V and F open Field Properties on the one field (`SCH_EDIT_TOOL::EditField` -> `editFieldText`, components/SchFieldDialog.tsx), not this dialog.
//
// **Differs**: the Angle and Mirror choices, Show pin numbers / Show pin names (one library symbol per lib_id: S10), the grid's Orientation, X and Y Position,
// Allow Autoplacement and Private columns, Move Up / Move Down, and Embedded Files are not here; a field added here takes its looks after OK; a unit already placed
// on the sheet is refused (KiCad makes a second placement of it).
import { useEffect, useMemo, useRef, useState } from "react";
import { fetchFontFamilies, fetchSchSetup } from "../api/client";
import { fetchAnySymbol } from "../api/libraryClient";
import type { Cmd } from "../api/types";
import { uniquePinCount } from "../kicad-port/footprintFilter";
import { bodyStyleCount, setBodyStyleCmd } from "../kicad-port/schBodyStyle";
import { addUserFieldCmd, fieldTextCmd, fieldVisibilityCmds, newUserFieldName, ownerKey, removeUserFieldCmd, STANDARD_SYMBOL_FIELDS, userFieldNameError, userFieldsOf } from "../kicad-port/schFieldEdit";
import { setSymbolVariantCmd, symbolInVariant, symbolPropertiesTitle, variantFlags, type VariantFlagKey, type VariantFlags } from "../kicad-port/designVariants";
import { expandStackedPinNotation } from "../kicad-port/stackedPins";
import { assignAlt, fieldLookCmds, fieldLookError, fontChoices, lookOf, pinAlternatesCmd, pinCell, PIN_COLUMN_LABELS, PIN_TABLE_DISABLED_TIP, pinRows, sortPinRows, unitChangeError, unitChoices, type FieldLook, type PinColumn, type PinRow } from "../kicad-port/symbolPropsDialog";
import { setSymbolUnitCmd } from "../kicad-port/schSymbolMenus";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";
import { useSymApi } from "../state/symbolEditorStore";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import { SimModelDialog } from "./simulator/SimModelDialog";

const MAIN_FIELDS = STANDARD_SYMBOL_FIELDS;

/** `DIALOG_SYMBOL_PROPERTIES_BASE`'s Attributes box, in its order, with the tooltips it gives three of them. */
const ATTRIBUTE_BOXES: ReadonlyArray<[VariantFlagKey, string, string | undefined]> = [
  ["exclude_from_sim", "Exclude from simulation", undefined],
  ["exclude_from_board", "Exclude from board", "This is useful for adding symbols that only get exported to the bill of materials but\nnot required to layout the board such as mechanical fasteners and enclosures"],
  ["dnp", "Do not populate", undefined],
  ["exclude_from_bom", "Exclude from bill of materials", "This is useful for adding symbols for board footprints such as fiducials\nand logos that you do not want to appear in the bill of materials export"],
  ["exclude_from_pos_files", "Exclude from position files", "This is useful for adding symbols that should not be included in the \nexported position files used for pick and place machines"],
];

/** A user-defined field's row: what the symbol has (`was`), or a field added here (`isNew`), as it stands now. */
interface UserRow {
  key: number;
  name: string;
  text: string;
  shown: boolean;
  isNew: boolean;
  removed: boolean;
  was?: { text: string; visible: boolean };
}

export function SymbolPropertiesDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const symApi = useSymApi();
  const open = state.symbolProperties;
  const variant = state.schVariant;
  const baseSymbol = open ? api.symbolById(open.id) : undefined;
  // the symbol as the current variant shows it (`GetFieldText( .., variant )`)
  const symbol = useMemo(() => (baseSymbol ? symbolInVariant(baseSymbol, variant) : undefined), [baseSymbol, variant]);

  const [reference, setReference] = useState("");
  const [value, setValue] = useState("");
  const [footprint, setFootprint] = useState("");
  const [datasheet, setDatasheet] = useState("");
  const [error, setError] = useState<string | null>(null);
  /** The "Show" check of each main field, by name; `undefined` for a field the server did not send (an older one). */
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [bodyStyle, setBodyStyle] = useState(1);
  const [unit, setUnit] = useState(1);
  /** The user-defined fields of the symbol, the ones it has and the ones added here. */
  const [userRows, setUserRows] = useState<UserRow[]>([]);
  /** The Attributes box, as the current variant has them (`m_cbExcludeFromSim->SetValue( GetExcludedFromSim( &sheetPath, variantName ) )` ...). */
  const [flags, setFlags] = useState<VariantFlags | null>(null);
  /** The look columns of each field the symbol has (Show Name .. Color), by name, as they stand and as they were. */
  const [looks, setLooks] = useState<Record<string, FieldLook>>({});
  const [wasLooks, setWasLooks] = useState<Record<string, FieldLook>>({});
  /** The Pin Functions tab's rows, as edited and as they were; the sort column and direction. */
  const [pins, setPins] = useState<PinRow[]>([]);
  const [wasPins, setWasPins] = useState<PinRow[]>([]);
  const [pinSort, setPinSort] = useState<{ col: PinColumn; asc: boolean }>({ col: "number", asc: true });
  const [tab, setTab] = useState<"general" | "pins">("general");
  const [families, setFamilies] = useState<string[]>([]);
  const nextKey = useRef(0);
  // The Footprint field's browse button: the Footprint Chooser, narrowed by this symbol's pin count and its library symbol's footprint filters.
  const [choosingFootprint, setChoosingFootprint] = useState(false);
  // "Simulation Model..." (`OnEditSpiceModel`, the button `dialog_symbol_properties.cpp` shows for a symbol that is not a power symbol)
  const [editingSimModel, setEditingSimModel] = useState(false);
  const [libInfo, setLibInfo] = useState<{ filters: string[]; pins: number } | null>(null);
  const libId = symbol?.lib_id ?? null;
  useEffect(() => {
    setLibInfo(null);
    if (!libId) return;
    let cancelled = false;
    // The pins of the whole library symbol, every unit (`GetGraphicalPins( 0, 1 )`): the placed instance lists the pins of its own unit only.
    fetchAnySymbol(libId)
      .then((r) => !cancelled && setLibInfo({ filters: r.symbol.footprint_filters ?? [], pins: uniquePinCount(r.symbol.pins, (n) => expandStackedPinNotation(n).numbers) }))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [libId]);
  // The Font column's choices: "Default Font", "KiCad Font" and the machine's families (`Fontconfig()->ListFonts`).
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchFontFamilies()
      .then((f) => !cancelled && setFamilies(f))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [open?.id]);
  const instancePins = useMemo(() => new Set((symbol?.pins ?? []).map((p) => p.number)).size, [symbol]);

  const refInput = useRef<HTMLInputElement>(null);
  const valueInput = useRef<HTMLInputElement>(null);
  const footprintInput = useRef<HTMLInputElement>(null);
  const datasheetInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!symbol) return;
    setReference(symbol.id);
    setValue(symbol.value ?? "");
    setFootprint(symbol.footprint ?? "");
    setDatasheet(symbol.datasheet ?? "");
    setShown(Object.fromEntries((symbol.fields ?? []).filter((f) => (MAIN_FIELDS as readonly string[]).includes(f.name)).map((f) => [f.name, f.visible])));
    setBodyStyle(symbol.body_style || 1);
    setUnit(symbol.unit || 1);
    const own = userFieldsOf(symbol.fields).map((f) => ({ key: nextKey.current++, name: f.name, text: f.text, shown: f.visible, isNew: false, removed: false, was: { text: f.text, visible: f.visible } }));
    setUserRows(own);
    setFlags(baseSymbol ? variantFlags(baseSymbol, variant) : null);
    // the look columns of the fields the symbol has
    const l = Object.fromEntries((symbol.fields ?? []).filter((f) => f.id).map((f) => [f.name, lookOf(f)]));
    setLooks(l);
    setWasLooks(l);
    // the Pin Functions tab: a copy of the pins (`for( pin : m_symbol->GetRawPins() ) m_dataModel->push_back( *pin )`), sorted by number
    const sch = state.schematic;
    const rows = sch ? pinRows(sch, symbol) : [];
    setPins(rows);
    setWasPins(rows);
    setPinSort({ col: "number", asc: true });
    setTab("general");
    // "Add in any template fieldnames not yet defined" (`TransferDataToWindow`, Schematic Setup > Field Name Templates): a row for each template the symbol lacks, shown as
    // the template's Visible box says; OK adds it, empty, as KiCad's OK does. Not on a sheet whose fields have no places yet (it could not show one).
    if ((symbol.fields?.length ?? 0) > 0) {
      const has = new Set([...MAIN_FIELDS, ...own.map((r) => r.name)]);
      void fetchSchSetup()
        .then((r) => {
          if (!r.ok) return;
          const missing = r.field_name_templates.filter((t) => !has.has(t.name));
          if (missing.length > 0) setUserRows((rows) => [...rows, ...missing.map((t) => ({ key: nextKey.current++, name: t.name, text: "", shown: t.visible, isNew: true, removed: false }))]);
        })
        .catch(() => undefined);
    }
    setError(null);
    const focusTarget = { reference: refInput, value: valueInput, footprint: footprintInput, datasheet: datasheetInput }[open?.field ?? "reference"];
    // Let the dialog mount before stealing focus.
    const t = setTimeout(() => focusTarget.current?.select(), 0);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open?.id, open?.field, variant]);

  if (!open || !symbol) return null;
  const sch = state.schematic;
  const close = () => dispatch({ type: "SET_SYMBOL_PROPERTIES", value: null });

  /** `TransferDataFromWindow`: every change as one command batch, one undo step. Returns whether it was applied (or there was nothing to apply). */
  const transfer = async (): Promise<boolean> => {
    const trimmedRef = reference.trim();
    if (!trimmedRef) {
      setError("Reference cannot be blank.");
      return false;
    }
    // `if( currentVariant.IsEmpty() ) m_symbol->SetRef( .. )`: a variant keeps the reference
    const renamed = variant === "" && trimmedRef !== symbol.id;
    if (renamed && api.symbolById(trimmedRef)) {
      setError(`"${trimmedRef}" is already used by another symbol.`);
      return false;
    }
    // the fields added here need names that no field of the symbol has (`FIELDS_EDITOR_GRID_DATA_MODEL`: the same name twice is no second field)
    const kept = userRows.filter((r) => !r.removed);
    const added = kept.filter((r) => r.isNew);
    for (const r of added) {
      const taken = kept.filter((o) => o !== r).map((o) => o.name);
      const why = userFieldNameError(r.name, taken);
      if (why) {
        setError(why);
        return false;
      }
    }
    for (const [name, l] of Object.entries(looks)) {
      const why = fieldLookError(name, l);
      if (why) {
        setError(why);
        return false;
      }
    }
    const unitWhy = sch ? unitChangeError(sch, symbol, unit) : null;
    if (unitWhy) {
      setError(unitWhy);
      return false;
    }
    // One submit, one undo step: the texts (the footprint is the Browse... pick too), the user-defined fields taken away, added and edited, the shown fields and their
    // looks, the body style and the pin assignments, all by the reference and unit the symbol has now; then the unit; and the new reference last (a rename moves what
    // is kept under the old one; the server's parts are not renamed until the step is done, so a field edit after it would miss its symbol)
    const owner = ownerKey({ id: symbol.id, unit: symbol.unit });
    const cmds: Cmd[] = variant === "" ? [{ op: "edit_symbol_fields", id: symbol.id, value, footprint, datasheet }] : [];
    for (const r of userRows) if (r.removed && !r.isNew) cmds.push(removeUserFieldCmd(symbol.id, r.name));
    // a field added here is the symbol's, with its text (`AddField( field )`), in a variant too
    for (const r of added) cmds.push(addUserFieldCmd(symbol.id, r.name, r.text));
    // the texts: the symbol's own in the default variant; in a variant, the ones changed here are the variant's (`tmp->SetText( variantText, &currentSheet, currentVariant )`)
    const variantFields: Record<string, string> = {};
    if (variant !== "") {
      const was = { Value: symbol.value ?? "", Footprint: symbol.footprint ?? "", Datasheet: symbol.datasheet ?? "" };
      if (value !== was.Value) variantFields.Value = value;
      if (footprint !== was.Footprint) variantFields.Footprint = footprint;
      if (datasheet !== was.Datasheet) variantFields.Datasheet = datasheet;
    }
    for (const r of kept) {
      if (r.isNew || !r.was || r.text === r.was.text) continue;
      if (variant === "") cmds.push(fieldTextCmd(owner, r.name, r.text));
      else variantFields[r.name] = r.text;
    }
    // the Attributes box, in the current variant (`SetExcludedFromSim( .., &currentSheet, currentVariant )` ...); only what changed
    const was = baseSymbol ? variantFlags(baseSymbol, variant) : null;
    const changedFlags: Partial<VariantFlags> = {};
    if (flags && was) for (const k of Object.keys(flags) as VariantFlagKey[]) if (flags[k] !== was[k]) changedFlags[k] = flags[k];
    if (variant !== "") {
      const v = setSymbolVariantCmd(variant, [symbol.id], { flags: changedFlags, fields: variantFields });
      if (v) cmds.push(v);
    } else {
      const { exclude_from_pos_files: pos, ...attrs } = changedFlags;
      if (Object.keys(attrs).length > 0) cmds.push({ op: "set_symbol_attrs", ids: [symbol.id], ...attrs });
      if (pos !== undefined) cmds.push({ op: "sch_edit", verb: "set_exclude_from_pos_files", ids: [symbol.id], exclude: pos });
    }
    const wanted: Record<string, boolean> = { ...shown };
    for (const r of kept) wanted[r.name.trim()] = r.shown;
    cmds.push(...fieldVisibilityCmds(owner, [...(symbol.fields ?? []), ...added.map((r) => ({ name: r.name.trim(), visible: false }))], wanted));
    // the look columns of the fields the symbol keeps (a field removed here takes its look with it)
    const removed = new Set(userRows.filter((r) => r.removed).map((r) => r.name));
    for (const f of symbol.fields ?? []) {
      const now = looks[f.name];
      const before = wasLooks[f.name];
      if (f.id && now && before && !removed.has(f.name)) cmds.push(...fieldLookCmds(f.id, before, now));
    }
    if (sch && bodyStyle !== (symbol.body_style || 1)) {
      const change = setBodyStyleCmd(sch, [symbol.id], bodyStyle);
      if (change) cmds.push(change);
    }
    // "Update any assignments": `SetAlt` on every pin of the number, then `SyncOtherUnits`
    const alts = pinAlternatesCmd(symbol.id, symbol.unit, wasPins, pins);
    if (alts) cmds.push(alts);
    // `SetUnitSelection( .., unit_selection ); SetUnit( unit_selection )`, after everything named by the placement's key
    if (unit !== symbol.unit) cmds.push(setSymbolUnitCmd(symbol.id, symbol.unit, unit, false));
    if (renamed) cmds.push({ op: "rename_symbol", id: symbol.id, new_id: trimmedRef });
    if (cmds.length > 0 && !(await api.cmdBatch(cmds))) {
      // the server says why (a reference another sheet has, a field of a sheet read from a KiCad file) in a toast
      setError(renamed ? `The changes were not applied ("${trimmedRef}" may be used by another symbol).` : "The changes were not applied.");
      return false;
    }
    return true;
  };
  const submit = async () => {
    if (await transfer()) close();
  };
  /** `OnUpdateSymbol` / `OnExchangeSymbol` / `OnEditSymbol` / `OnEditLibrarySymbol`: `if( TransferDataFromWindow() ) EndQuasiModal( .. )`, then what the button asked for. */
  const thenDo = (after: () => void) => async () => {
    if (!(await transfer())) return;
    close();
    after();
  };
  const changeSymbols = (mode: "change" | "update") => thenDo(() => dispatch({ type: "SET_SCH_TOOL_DIALOG", dialog: { kind: "change_symbols", mode, selected: [reference.trim() || symbol.id] } }));
  // `editor->LoadSymbol( symbol->GetLibId(), symbol->GetUnit(), symbol->GetBodyStyle() )`; `onUpdateEditSymbol`: enabled when the symbol has its library symbol
  const canEditSymbol = !!symbol.lib_id && !!sch?.lib_symbols[symbol.lib_id];
  const editSymbol = thenDo(() => {
    if (!symbol.lib_id) return;
    dispatch({ type: "SET_TAB", tab: "symbol" });
    void symApi.openSymbol(symbol.lib_id, { unit: Math.max(1, unit), bodyStyle: Math.max(1, bodyStyle) });
  });

  const bodyStyles = sch ? bodyStyleCount(sch, symbol) : 1;
  const units = sch ? unitChoices(sch, symbol) : [];
  // a sheet read from a KiCad file has no places for its fields yet: the server lists none, and a field added there could not be shown
  const hasPlaces = (symbol.fields?.length ?? 0) > 0;
  const fonts = fontChoices(families);
  /** The "Show" check of a main field's row; nothing for a field the server did not send. */
  const showBox = (name: string) =>
    shown[name] === undefined ? (
      <span />
    ) : (
      <input type="checkbox" title={`Show the ${name} field on the sheet`} aria-label={`Show ${name}`} checked={shown[name]} onChange={(e) => setShown({ ...shown, [name]: e.target.checked })} />
    );
  /** The look cells of the field `name` (Show Name .. Color); empty cells for a field that has no place on the sheet (added here, or a sheet read from a file). */
  const lookCells = (name: string) => {
    const l = looks[name];
    if (!l) return <td colSpan={8} />;
    const set = (patch: Partial<FieldLook>) => setLooks((all) => ({ ...all, [name]: { ...l, ...patch } }));
    return (
      <>
        <td>
          <input type="checkbox" aria-label={`${name} show name`} checked={l.nameShown} onChange={(e) => set({ nameShown: e.target.checked })} />
        </td>
        <td>
          <select aria-label={`${name} h align`} value={l.h} onChange={(e) => set({ h: e.target.value as FieldLook["h"] })}>
            <option value="left">Left</option>
            <option value="center">Center</option>
            <option value="right">Right</option>
          </select>
        </td>
        <td>
          <select aria-label={`${name} v align`} value={l.v} onChange={(e) => set({ v: e.target.value as FieldLook["v"] })}>
            <option value="top">Top</option>
            <option value="center">Center</option>
            <option value="bottom">Bottom</option>
          </select>
        </td>
        <td>
          <input type="checkbox" aria-label={`${name} italic`} checked={l.italic} onChange={(e) => set({ italic: e.target.checked })} />
        </td>
        <td>
          <input type="checkbox" aria-label={`${name} bold`} checked={l.bold} onChange={(e) => set({ bold: e.target.checked })} />
        </td>
        <td>
          <input aria-label={`${name} text size`} style={{ width: 52 }} type="number" step="0.01" min={0.01} value={l.sizeMm} onChange={(e) => set({ sizeMm: Number(e.target.value) })} />
        </td>
        <td>
          <input aria-label={`${name} font`} style={{ width: 110 }} list="symbol-props-fonts" value={l.font} onChange={(e) => set({ font: e.target.value })} />
        </td>
        <td style={{ whiteSpace: "nowrap" }}>
          <input aria-label={`${name} color`} type="color" value={l.color || "#000000"} onChange={(e) => set({ color: e.target.value })} style={{ width: 28, padding: 0, opacity: l.color ? 1 : 0.35 }} />
          {l.color && (
            <button type="button" title="The layer's colour (unspecified)" aria-label={`${name} color clear`} onClick={() => set({ color: "" })}>
              ×
            </button>
          )}
        </td>
      </>
    );
  };
  const textInput = (ref: React.RefObject<HTMLInputElement | null>, label: string, v: string, onChange: (s: string) => void, disabled = false, title?: string) => (
    <input
      ref={ref}
      aria-label={label}
      style={{ width: "100%", minWidth: 0 }}
      value={v}
      disabled={disabled}
      title={title}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") void submit();
      }}
    />
  );
  const multiBody = bodyStyles > 1;
  const sortedPins = sortPinRows(pins, pinSort.col, pinSort.asc);

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div className="dialog" style={{ width: 960, maxWidth: "96vw" }} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-header">
          <span data-testid="symbol-properties-title">{symbolPropertiesTitle(variant)}</span>
        </div>
        <div className="dialog-body">
          <div style={{ display: "flex", gap: 2, marginBottom: 6 }} role="tablist">
            <button role="tab" aria-selected={tab === "general"} className={tab === "general" ? "primary" : undefined} onClick={() => setTab("general")} data-testid="symbol-tab-general">
              General
            </button>
            <button
              role="tab"
              aria-selected={tab === "pins"}
              className={tab === "pins" ? "primary" : undefined}
              disabled={multiBody}
              title={multiBody ? PIN_TABLE_DISABLED_TIP : undefined}
              onClick={() => setTab("pins")}
              data-testid="symbol-tab-pins"
            >
              Pin Functions
            </button>
          </div>
          {tab === "general" && (
            <>
              <fieldset style={{ overflowX: "auto" }}>
                <legend>Fields</legend>
                <datalist id="symbol-props-fonts">
                  {fonts.map((f) => (
                    <option key={f} value={f} />
                  ))}
                </datalist>
                <table className="symbol-fields-grid" style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }} data-testid="symbol-fields-grid">
                  <thead>
                    <tr>
                      {["Name", "Value", "Show", "Show Name", "H Align", "V Align", "Italic", "Bold", "Text Size", "Font", "Color"].map((h) => (
                        <th key={h} style={{ textAlign: "left", fontWeight: 600, padding: "2px 4px", whiteSpace: "nowrap" }}>
                          {h === "Text Size" ? "Text Size (mm)" : h}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    <tr>
                      <td>Reference</td>
                      <td style={{ minWidth: 160 }}>{textInput(refInput, "Reference", reference, setReference, variant !== "", variant !== "" ? "The reference is the same in every design variant: set it in the default variant." : undefined)}</td>
                      <td>{showBox("Reference")}</td>
                      {lookCells("Reference")}
                    </tr>
                    <tr>
                      <td>Value</td>
                      <td>{textInput(valueInput, "Value", value, setValue)}</td>
                      <td>{showBox("Value")}</td>
                      {lookCells("Value")}
                    </tr>
                    <tr>
                      <td>Footprint</td>
                      <td>
                        <div style={{ display: "flex", gap: 4 }}>
                          {textInput(footprintInput, "Footprint", footprint, setFootprint)}
                          <button onClick={() => setChoosingFootprint(true)} title="Choose a footprint from KiCad's libraries (Footprint Chooser)" data-browse-footprint>
                            Browse…
                          </button>
                        </div>
                      </td>
                      <td>{showBox("Footprint")}</td>
                      {lookCells("Footprint")}
                    </tr>
                    <tr>
                      <td>Datasheet</td>
                      <td>{textInput(datasheetInput, "Datasheet", datasheet, setDatasheet)}</td>
                      <td>{showBox("Datasheet")}</td>
                      {lookCells("Datasheet")}
                    </tr>
                    {userRows
                      .filter((r) => !r.removed)
                      .map((r) => (
                        <tr key={r.key}>
                          <td>
                            {r.isNew ? (
                              <input aria-label="New field name" value={r.name} onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, name: e.target.value } : o)))} style={{ width: 110 }} />
                            ) : (
                              <span title="A user-defined field">{r.name}</span>
                            )}
                          </td>
                          <td>
                            <div style={{ display: "flex", gap: 4 }}>
                              <input
                                aria-label={`${r.name} value`}
                                style={{ flex: 1, minWidth: 0 }}
                                value={r.text}
                                onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, text: e.target.value } : o)))}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter") void submit();
                                }}
                              />
                              <button type="button" title="Remove this field" aria-label={`Remove ${r.name}`} onClick={() => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, removed: true } : o)))}>
                                ×
                              </button>
                            </div>
                          </td>
                          <td>
                            <input type="checkbox" title={`Show the ${r.name} field on the sheet`} aria-label={`Show ${r.name}`} checked={r.shown} onChange={(e) => setUserRows((rows) => rows.map((o) => (o === r ? { ...o, shown: e.target.checked } : o)))} />
                          </td>
                          {r.isNew ? <td colSpan={8} style={{ opacity: 0.6 }} title="A field added here takes its looks once it is on the symbol (OK)" /> : lookCells(r.name)}
                        </tr>
                      ))}
                  </tbody>
                </table>
                {hasPlaces && (
                  <p style={{ margin: "6px 0 0" }}>
                    <button
                      type="button"
                      title="Add field"
                      onClick={() =>
                        setUserRows((rows) => [
                          ...rows,
                          { key: nextKey.current++, name: newUserFieldName([...MAIN_FIELDS, ...rows.filter((r) => !r.removed).map((r) => r.name)]), text: "", shown: false, isNew: true, removed: false },
                        ])
                      }
                    >
                      Add Field
                    </button>
                  </p>
                )}
              </fieldset>
              <div style={{ display: "flex", gap: 12, marginTop: 8, flexWrap: "wrap" }}>
                <fieldset style={{ display: "grid", gridTemplateColumns: "auto 1fr", gap: "4px 8px", alignItems: "center", minWidth: 220 }} data-testid="symbol-general">
                  <legend>General</legend>
                  <span style={{ opacity: units.length > 0 ? 1 : 0.5 }}>Unit:</span>
                  <select aria-label="Unit" value={unit} disabled={units.length === 0} onChange={(e) => setUnit(Number(e.target.value))} data-testid="symbol-unit">
                    {units.length === 0 ? (
                      <option value={unit}>{""}</option>
                    ) : (
                      units.map((u) => (
                        <option key={u.unit} value={u.unit}>
                          {u.label}
                        </option>
                      ))
                    )}
                  </select>
                  <span style={{ opacity: multiBody ? 1 : 0.5 }}>Body style:</span>
                  <select aria-label="Body style" value={bodyStyle} disabled={!multiBody} onChange={(e) => setBodyStyle(Number(e.target.value))} data-testid="symbol-body-style">
                    {Array.from({ length: Math.max(1, bodyStyles) }, (_, i) => i + 1).map((n) => (
                      <option key={n} value={n}>
                        {/* `LIB_SYMBOL::GetBodyStyleDescription`: a De Morgan pair is Standard and Alternate */}
                        {!multiBody ? "" : n === 1 ? "Standard" : n === 2 ? "Alternate" : `Style ${n}`}
                      </option>
                    ))}
                  </select>
                </fieldset>
                {flags && (
                  <fieldset style={{ display: "flex", flexDirection: "column", gap: 2 }} data-testid="symbol-attributes">
                    <legend>Attributes</legend>
                    {ATTRIBUTE_BOXES.map(([key, label, tip]) => (
                      <label key={key} title={tip} className="toggle">
                        <input type="checkbox" checked={flags[key]} onChange={(e) => setFlags({ ...flags, [key]: e.target.checked })} data-testid={`symbol-attr-${key}`} /> {label}
                      </label>
                    ))}
                  </fieldset>
                )}
                <div style={{ display: "flex", flexDirection: "column", gap: 4, alignSelf: "flex-start" }}>
                  <button onClick={changeSymbols("update")} data-testid="symbol-update-from-library">
                    Update Symbol from Library...
                  </button>
                  <button onClick={changeSymbols("change")} data-testid="symbol-change">
                    Change Symbol...
                  </button>
                  <button onClick={editSymbol} disabled={!canEditSymbol} data-testid="symbol-edit">
                    Edit Symbol...
                  </button>
                  <button onClick={editSymbol} disabled={!canEditSymbol} data-testid="symbol-edit-library">
                    Edit Library Symbol...
                  </button>
                </div>
              </div>
            </>
          )}
          {tab === "pins" && (
            <div style={{ maxHeight: 360, overflow: "auto" }}>
              <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 12 }} data-testid="symbol-pin-grid">
                <thead>
                  <tr>
                    {(Object.keys(PIN_COLUMN_LABELS) as PinColumn[]).map((c) => (
                      <th
                        key={c}
                        style={{ textAlign: "left", fontWeight: 600, padding: "2px 6px", cursor: "pointer", whiteSpace: "nowrap" }}
                        // `OnPinTableColSort`: the same column inverts, another starts ascending
                        onClick={() => setPinSort((s) => ({ col: c, asc: s.col === c ? !s.asc : true }))}
                      >
                        {PIN_COLUMN_LABELS[c]}
                        {pinSort.col === c ? (pinSort.asc ? " ▲" : " ▼") : ""}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {sortedPins.map((r, i) => (
                    <tr key={`${r.number}:${i}`} data-pin={r.number}>
                      <td style={{ padding: "1px 6px" }}>{pinCell(r, "number")}</td>
                      <td style={{ padding: "1px 6px" }}>{pinCell(r, "base")}</td>
                      <td style={{ padding: "1px 6px" }}>
                        {r.alternates.length === 0 ? (
                          <span />
                        ) : (
                          <select aria-label={`Pin ${r.number} alternate`} value={r.alt || r.base} onChange={(e) => setPins((rows) => rows.map((o) => (o === r ? assignAlt(o, e.target.value) : o)))}>
                            <option value={r.base}>{r.base}</option>
                            {r.alternates
                              .filter((a) => a.name !== r.base)
                              .map((a) => (
                                <option key={a.name} value={a.name}>
                                  {a.name}
                                </option>
                              ))}
                          </select>
                        )}
                      </td>
                      <td style={{ padding: "1px 6px" }}>{pinCell(r, "type")}</td>
                      <td style={{ padding: "1px 6px" }}>{pinCell(r, "shape")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {error && (
            <p style={{ color: "var(--chrome-danger)", fontSize: 11, marginTop: 8 }} data-testid="symbol-properties-error">
              {error}
            </p>
          )}
          <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginTop: 10 }}>
            Renaming a symbol renames its footprint on the board with it (the pose, the copper on its pads, its fields and edits stay with the part), and one Undo takes both back.
          </p>
        </div>
        <div className="dialog-footer">
          <span style={{ fontSize: 11, opacity: 0.8 }}>Library link:</span>
          <span style={{ fontSize: 11, opacity: 0.7, marginRight: 8 }} data-testid="symbol-library-link">
            {symbol.lib_id ?? "(generic)"}
          </span>
          <button style={{ marginRight: "auto" }} onClick={() => setEditingSimModel(true)} data-testid="symbol-sim-model">
            Simulation Model...
          </button>
          <button onClick={close}>Cancel</button>
          <button className="primary" onClick={() => void submit()}>
            OK
          </button>
        </div>
      </div>
      {editingSimModel && (
        <SimModelDialog
          reference={symbol.id}
          onClose={() => setEditingSimModel(false)}
          // the Simulation Model dialog wrote the fields itself (its own undo step): the rows show them as the symbol has them now
          onWritten={(all) => {
            // the Value (the primary parameter saved there) is the dialog's own box; the rest are user rows
            const { Value: writtenValue, ...written } = all;
            if (writtenValue !== undefined) setValue(writtenValue);
            setUserRows((rows) => {
              let out = rows.filter((r) => !(r.name in written && written[r.name] === "" && !r.isNew));
              out = out.map((r) => (r.name in written && written[r.name] !== "" ? { ...r, text: written[r.name]!, removed: false, was: { text: written[r.name]!, visible: r.was?.visible ?? r.shown } } : r));
              for (const [name, text] of Object.entries(written)) {
                if (text !== "" && !out.some((r) => r.name === name)) out.push({ key: nextKey.current++, name, text, shown: false, isNew: false, removed: false, was: { text, visible: false } });
              }
              return out;
            });
          }}
        />
      )}
      {choosingFootprint && (
        <FootprintChooserDialog
          preselect={footprint.includes(":") ? footprint : null}
          pinCount={libInfo?.pins ?? instancePins}
          fpFilters={libInfo?.filters}
          onCancel={() => setChoosingFootprint(false)}
          onChoose={(pick) => {
            setChoosingFootprint(false);
            if (pick.kind === "footprint") setFootprint(pick.name);
          }}
        />
      )}
    </div>
  );
}
