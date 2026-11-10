// "Bulk Edit Symbol Fields..." (eeschema.EditorControl.editSymbolFields --
// eeschema/dialogs/dialog_symbol_fields_table.cpp, grid model in
// eeschema/fields_data_model.cpp): a table of every symbol x field.
//
// Edit tab: the grid, with
//   * "Group symbols" + per-column "Group By" (default: Value + Footprint,
//     the "Grouped By Value and Footprint" shape) -- grouped rows show a
//     collapsed reference text like "R1-R3, R5" (`SCH_REFERENCE_LIST::
//     Shorthand`) and a Qty, and expand into one child row per symbol;
//   * show/hide per column, click a header to sort (`m_sortColumn`);
//   * add / rename / remove user field columns;
//   * inline cell edits (not Reference, not the generated Qty): typing in a
//     group row sets the field on every symbol in it (`SetValue`), and a
//     group whose members disagree shows "-- mixed values --".
// Like the source's data store, edits are *staged* -- the grid regroups on
// them (the server overlays them on the schematic without saving, see
// sch_api.rs) -- and only "Apply" commits them, as ONE undoable
// `set_symbol_fields` verb (`ApplyData` + one SCH_COMMIT).
//
// Export tab: `BOM_FMT_PRESET` options (CSV / TSV / Semicolons presets,
// field/string/reference/range delimiters, keep tabs / line breaks), a live
// preview (`PreviewRefresh`) and "Export" writing the file
// (`OnExport`) -- relative to the board directory, see sch_api.rs.
//
// The attribute columns (`${DNP}`, `${EXCLUDE_FROM_BOM}`, `${EXCLUDE_FROM_BOARD}`,
// `${EXCLUDE_FROM_SIM}`: `FIELDS_EDITOR_GRID_DATA_MODEL::isAttribute`) are checkbox
// cells that stage "1" or "0" like any cell (a group of mixed symbols shows an
// indeterminate box and a click sets them all); the options "Include 'DNP' Symbols"
// and "Include 'Exclude from BOM' Symbols" (the dialog's options menu) filter the
// rows by the flags the sheet has, and KiCad's built-in views (`BOM_PRESET::
// BuiltInPresets`) can be applied from the Preset box.
//
// Design variants (KiCad 25ec2335 `DIALOG_SYMBOL_FIELDS_TABLE`, `common/dialogs/dialog_fields_table_base.cpp`): the "Schematic variants:" list below
// the fields, with Add, Rename, Copy, Edit Description and Delete (the last four for a variant, not the default: `updateVariantButtonStates`). The list
// selects the variant the grid shows and makes it the schematic's current variant (`onVariantSelectionChange`); a cell whose value in that variant is the
// variant's own is highlighted (`GetAttr`: light yellow, dark olive in a dark theme). An edit while a variant is shown is staged for that variant
// (`fieldVariant`), but the Reference and `${EXCLUDE_FROM_BOARD}` are the symbol's (`fieldSupportsVariants`); adding, renaming and removing a field is
// the symbol's, in every variant. Apply sends the symbol's edits and each variant's as one step (`ApplyData`'s loop over `storedVariants`); the export
// is the shown variant's.
//
// UI-LEFT S11 (kicad-port/bomPresets.ts): the scope ("Entire Project", "Current Sheet Only", "Current Sheet and Down" and KiCad 25ec2335's "Selection": `OnScope`,
// `RebuildRows`; every open starts on the whole project, as 25ec2335 does not save it); the view presets of the project (`rebuildBomPresetsWidget`: the
// built-in and the project's in name order, "---", "Save preset..." -- "BOM preset name:" / "Save BOM Preset", a built-in name refused, "Overwrite existing
// preset?" -- and "Delete preset..." -- "Delete Preset" / "Select preset:"), the box showing the preset the view is (`syncBomPresetSelection`); the field name
// templates as hidden columns and the project's saved view (`schematic.bom_settings`) from the server; "Save changes?" with Save / Discard Changes / Cancel on a close with staged edits (`HandleUnsavedChanges`).
// There is no "Rename" of a preset (KiCad has none: save under the new name and delete the old one).
//
// UI-LEFT-2 SS13 (kicad-port/fieldsTableDialog.ts): the field list's columns are KiCad's "Field", "BOM Name" (an edited label that names the grid's column and the
// export's header), "Include" and "Group By", with the refusals of the two generated counts ("The Quantity column cannot be grouped by.", "The Item Number
// column cannot be grouped by."); the mandatory Description is a column (not shown at first, `LoadFieldNames`), and so is the generated "#"; the grid's right-button
// menus (`FIELDS_EDITOR_GRID_TRICKS`: "Select Footprint...", "Show Datasheet", Cut, Copy, Paste, Delete, Select All; a column header's: show or hide each column;
// the field list's header: its own columns); the export's format presets with Save preset... / Delete preset... (`onBomFmtPresetChanged`). The presets are kept
// in the dialog and written when it closes, with the view and the format it closes on (`savePresetsToSchematic`), one `set_bom_view`, no undo step (`OnModify()`, no commit).
//
// Not ported (PARITY-sch.md): the variant symbol overrides (Set / Clear Variant Symbol).
import { useEffect, useRef, useState } from "react";
import { exportBom, fetchFieldsTable } from "../api/client";
import { DELETE_PRESET, matchingPreset, PRESET_SEPARATOR, presetChoices, READ_ONLY_PRESET, SAVE_PRESET, savePresetOutcome, SCOPE_CHOICES, UNSAVED_CHANGES, type FieldsTableScope } from "../kicad-port/bomPresets";
import { cellsAsText, closeCmd, columnMenu, FIELD_LIST_COLUMNS, fmtPresetChoices, fmtWithSaved, gridCellMenu, groupByRefusal, heldWithSaved, heldWithout, matchingFmtPreset, saveFmtOutcome, specSetLabel, textAsCells, withPresetSpec, type HeldPreset, type MenuItem, type PresetState } from "../kicad-port/fieldsTableDialog";
import { FootprintChooserDialog } from "./FootprintChooserDialog";
import { SchDialogShell } from "./SchDialogShell";
import { useColorScheme } from "../hooks/useColorScheme";
import { DEFAULT_VARIANT_NAME, trimName, validateCopyNameInTable, validateNewVariantName, variantDescription, variantNamesForUI } from "../kicad-port/designVariants";
import { AddVariantDialog, DescriptionEntryDialog, TextEntryDialog } from "./DesignVariantDialogs";
import { postBomFile } from "../api/schControlClient";
import { useSchControlDispatch, useSchControlState } from "../state/schControlStore";
import type { BomFmt, Cmd, FieldsTableReply, FieldsTableRow, FieldsTableSpec } from "../api/types";
import {
  attributeState,
  bomFmtPresets,
  emptyChanges,
  isAttributeColumn,
  isEditableColumn,
  isEmptyChanges,
  specAddColumn,
  specRemoveColumn,
  specRenameColumn,
  stageAddField,
  stageEdit,
  stageRemoveField,
  stageRenameField,
  toggledAttribute,
  validNewFieldName,
  deleteStoredVariant,
  fieldSupportsVariants,
  renameStoredVariant,
  stageVariantEdit,
  variantApplyCmds,
  variantEditsRemoved,
  variantEditsRenamed,
  VARIANT_OVERRIDE_COLOR,
  type StagedChanges,
  type StagedVariantEdit,
} from "../kicad-port/fieldsTableStage";
import { useStudioApi, useStudioDispatch, useStudioState } from "../state/store";

const MIXED = "-- mixed values --";
const BUILTIN = new Set(["Reference", "Value", "Footprint", "Datasheet", "Description"]);

function isGenerated(name: string): boolean {
  return name.startsWith("${") && name.endsWith("}");
}

/** A checkbox cell of an attribute column: checked, unchecked, or indeterminate for a group whose symbols differ. */
function AttributeCell({ text, mixed, label, onToggle }: { text: string; mixed: boolean; label: string; onToggle: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const state = attributeState(text, mixed);
  useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === "mixed";
  }, [state]);
  return <input ref={ref} type="checkbox" aria-label={label} checked={state === "checked"} onChange={onToggle} />;
}

/** A right-button menu where it was asked for (`wxMenu` / `PopupMenu`): items, separators and check items; a click outside closes it. */
function PopupMenu({ at, items, onPick, onClose }: { at: { x: number; y: number }; items: MenuItem[]; onPick: (id: string) => void; onClose: () => void }) {
  return (
    // a click outside closes the menu only (not the table under it, whose backdrop closes the table)
    <div style={{ position: "fixed", inset: 0, zIndex: 2000 }} onClick={(e) => (e.stopPropagation(), onClose())} onContextMenu={(e) => (e.preventDefault(), e.stopPropagation(), onClose())}>
      <div className="context-menu" role="menu" data-testid="fields-table-menu" style={{ position: "fixed", left: at.x, top: at.y, background: "var(--chrome-bg-raised)", border: "1px solid var(--chrome-border)", borderRadius: 4, padding: "4px 0", minWidth: 180, fontSize: 12 }} onClick={(e) => e.stopPropagation()}>
        {items.map((it, i) =>
          it.kind === "separator" ? (
            <div key={`sep${i}`} style={{ borderTop: "1px solid var(--chrome-border)", margin: "3px 0" }} />
          ) : (
            <button
              key={it.id}
              role="menuitem"
              disabled={!it.enabled}
              style={{ display: "flex", width: "100%", justifyContent: "space-between", gap: 16, background: "none", border: "none", padding: "3px 12px", textAlign: "left", opacity: it.enabled ? 1 : 0.45 }}
              onClick={() => (onClose(), onPick(it.id))}
            >
              <span>
                {it.checked !== undefined ? (it.checked ? "✓ " : "\u2003 ") : ""}
                {it.label.split("\t")[0]}
              </span>
              <span style={{ color: "var(--chrome-text-dim)" }}>{it.label.split("\t")[1] ?? ""}</span>
            </button>
          )
        )}
      </div>
    </div>
  );
}

/** The grid cell a right click or a selection names: a row's key and a column; or every cell (Select All). */
type CellSel = { row: string; col: string } | "all" | null;

/** KiCad's text clipboard as the grid uses it; kept here too when the browser will not hand its own back (a page not focused, a refused read). */
let gridClipboard = "";

export function SymbolFieldsTableDialog() {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const api = useStudioApi();
  const open = state.schDialog === "fields_table";

  const [tab, setTab] = useState<"edit" | "export">("edit");
  const [spec, setSpec] = useState<FieldsTableSpec | null>(null);
  const [changes, setChanges] = useState<StagedChanges>(emptyChanges());
  const [table, setTable] = useState<FieldsTableReply | null>(null);
  const presets = table?.presets ?? [];
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [newField, setNewField] = useState("");
  /** The cells staged for each variant (the data model's `FIELD_VALUE_STATE`s of the variants visited). */
  const [variantEdits, setVariantEdits] = useState<StagedVariantEdit[]>([]);
  /** A variant dialog the list's buttons opened over the table. */
  const [variantDialog, setVariantDialog] = useState<{ kind: "add" | "rename" | "copy" | "describe"; name: string } | null>(null);
  const scheme = useColorScheme();
  // The variant the grid shows: the schematic's current one, which the list selects (`resolveVariant`; picking another in the list makes it current).
  const variant = state.schVariant;
  const variants = state.schematic?.variants ?? [];
  const listed = variantNamesForUI(variants);
  const say = (message: string, kind: "error" | "info" = "error") => dispatch({ type: "TOAST", message, kind, ms: 10000 });

  const [fmt, setFmt] = useState<BomFmt>(bomFmtPresets()[0] as BomFmt);
  const [outPath, setOutPath] = useState("");
  const [preview, setPreview] = useState("");
  const [exportMsg, setExportMsg] = useState<string | null>(null);

  // the scope (`m_scope`) and the preset box's dialogs: the name to save under, the overwrite question, the read-only refusal, the preset to delete
  const [scope, setScope] = useState<FieldsTableScope>("all");
  // the scope's request keys: the sheet in view and, for "Selection", what is selected on it (`updateSelectionItems`)
  const scopeKeys = { scope, sheet_path: state.currentSheetPath, selection: scope === "selection" ? [...state.selection] : [] };
  const [presetDialog, setPresetDialog] = useState<{ kind: "save"; name: string } | { kind: "overwrite"; name: string } | { kind: "error"; message: string } | { kind: "delete"; pick: string } | null>(null);
  const [lastPreset, setLastPreset] = useState<string>("");
  const [askClose, setAskClose] = useState(false);
  const [fieldDialog, setFieldDialog] = useState<{ kind: "rename" | "remove"; name: string } | null>(null);
  // `m_bomPresets` / `m_bomFmtPresets` as the dialog holds them (the project's, as they were when it opened, then saved and deleted here), and what the project had
  // when it opened: `savePresetsToSchematic` writes the difference when the dialog closes
  const [held, setHeld] = useState<HeldPreset[] | null>(null);
  const [heldFmt, setHeldFmt] = useState<BomFmt[] | null>(null);
  const [opened, setOpened] = useState<PresetState | null>(null);
  const [fmtDialog, setFmtDialog] = useState<{ kind: "save"; name: string } | { kind: "overwrite"; name: string } | { kind: "error"; message: string } | { kind: "delete"; pick: string } | null>(null);
  const [lastFmt, setLastFmt] = useState("");
  // the right-button menus and the grid's selected cells (the right-clicked one, or Select All)
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; items: MenuItem[]; onPick: (id: string) => void } | null>(null);
  const [sel, setSel] = useState<CellSel>(null);
  const [listCols, setListCols] = useState<boolean[]>([true, true, true, true]);
  const [choosingFootprint, setChoosingFootprint] = useState<{ row: FieldsTableRow; value: string } | null>(null);

  const requestId = useRef(0);

  // A fresh open starts from the dialog's default view with nothing staged -- on the Export tab when Generate Bill of Materials opened it (`ShowExportTab`).
  const schControl = useSchControlState();
  const schControlDispatch = useSchControlDispatch();
  useEffect(() => {
    if (!open) return;
    setTab(schControl.fieldsTableOnExport ? "export" : "edit");
    if (schControl.fieldsTableOnExport) schControlDispatch({ type: "SET_FIELDS_TABLE_ON_EXPORT", on: false });
    setSpec(null);
    setChanges(emptyChanges());
    setTable(null);
    setError(null);
    setExpanded(new Set());
    setNewField("");
    setExportMsg(null);
    setVariantEdits([]);
    setVariantDialog(null);
    setScope("all");
    setPresetDialog(null);
    setAskClose(false);
    setHeld(null);
    setHeldFmt(null);
    setOpened(null);
    setFmtDialog(null);
    setMenu(null);
    setSel(null);
  }, [open]);

  // Re-run the grid whenever the view, the staged changes or the schematic itself changed.
  useEffect(() => {
    if (!open) return;
    const id = ++requestId.current;
    void fetchFieldsTable(spec, isEmptyChanges(changes) ? null : changes, undefined, { variant, variant_edits: variantEdits, ...scopeKeys }).then((reply) => {
      if (id !== requestId.current) return;
      if (!reply.ok) {
        setError(reply.message ?? "Could not build the table.");
        return;
      }
      setError(null);
      setTable(reply);
      if (spec === null) {
        setSpec(reply.spec);
        // `TransferDataToWindow`: `SetUserBomPresets( m_BomPresets )`, `ApplyBomPreset( m_BomSettings )`, `SetUserBomFmtPresets( m_BomFmtPresets )`, `ApplyBomFmtPreset( m_BomFmtSettings )`
        const presetsNow = (reply.user_presets ?? []).map((p) => ({ name: p.name, spec: p.spec }));
        const fmts = reply.fmt_presets ?? [];
        const fmtNow = reply.fmt_settings ?? (bomFmtPresets()[0] as BomFmt);
        setHeld(presetsNow);
        setHeldFmt(fmts);
        setFmt(fmtNow);
        setOpened({ presets: presetsNow, view: reply.spec, fmtPresets: fmts, fmt: fmtNow });
      }
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, spec, changes, state.schematic, variant, variantEdits, scope, state.currentSheetPath, scope === "selection" ? state.selection : null]);

  // Export preview (`PreviewRefresh`) follows the same inputs while that tab is showing.
  useEffect(() => {
    if (!open || tab !== "export" || !spec) return;
    let cancelled = false;
    void exportBom(spec, fmt, isEmptyChanges(changes) ? null : changes, undefined, undefined, { variant, variant_edits: variantEdits, ...scopeKeys }).then((reply) => {
      if (cancelled) return;
      if (reply.ok) {
        setPreview(reply.text ?? "");
        setOutPath((p) => (p === "" ? (reply.default_path ?? "export/bom.csv") : p));
      } else {
        setPreview(reply.message ?? "");
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, tab, spec, fmt, changes, state.schematic, variant, variantEdits, scope, state.currentSheetPath]);

  if (!open) return null;

  const dirty = !isEmptyChanges(changes) || variantEdits.length > 0;
  // `~DIALOG_SYMBOL_FIELDS_TABLE` -> `savePresetsToSchematic`: the presets the dialog holds, the view and the format it closes on, whatever button closed it
  // (`applyCmds`: the edits Save applies first, in the same step)
  const closeNow = async (applyCmds: Cmd[] = []) => {
    const keep = opened && spec ? closeCmd(opened, { presets: held ?? [], view: spec, fmtPresets: heldFmt ?? [], fmt }) : null;
    const cmds = [...applyCmds, ...(keep ? [keep] : [])];
    if (cmds.length > 0 && !(await api.cmdBatch(cmds)) && applyCmds.length > 0) return;
    dispatch({ type: "SET_SCH_DIALOG", dialog: null });
  };
  const close = () => {
    // `OnClose`: `HandleUnsavedChanges( this, _( "Save changes?" ), .. )` -- Save applies, Discard Changes closes, Cancel stays
    if (dirty) return setAskClose(true);
    void closeNow();
  };
  const builtInNames = (table?.built_in_presets ?? []).map((p) => p.name).concat(table?.built_in_presets ? [] : presets);
  const userNames = (held ?? []).map((p) => p.name);
  const presetBox = presetChoices(builtInNames, userNames);
  const currentPreset = spec ? matchingPreset(spec, [...(table?.built_in_presets ?? []), ...(held ?? [])]) : null;
  // `onBomPresetChanged`: the separator is no choice, the two actions open their dialogs, a preset is applied
  const onPresetBox = (value: string) => {
    if (value === PRESET_SEPARATOR) return;
    if (value === SAVE_PRESET) return setPresetDialog({ kind: "save", name: lastPreset });
    if (value === DELETE_PRESET) return setPresetDialog({ kind: "delete", pick: userNames[0] ?? "" });
    if (userNames.includes(value)) setLastPreset(value);
    void applyPreset(value);
  };
  // "Save preset...": into the dialog's list (`m_bomPresets[name] = m_dataModel->GetBomSettings()`); the project has it once the dialog closes
  const savePreset = (name: string) => {
    if (!spec) return;
    setHeld((l) => heldWithSaved(l ?? [], name, spec));
    setLastPreset(name);
  };

  const updateSpec = (patch: Partial<FieldsTableSpec>) => setSpec((s) => (s ? { ...s, ...patch } : s));
  const setColumn = (name: string, patch: { show?: boolean; group_by?: boolean }) => {
    // `OnViewControlsCellChanged`'s GROUP_BY_COLUMN: the two generated counts cannot group (`DisplayError`, the box unticked again)
    const why = patch.group_by !== undefined ? groupByRefusal(name, patch.group_by) : null;
    if (why) return setPresetDialog({ kind: "error", message: why });
    setSpec((s) => (s ? { ...s, columns: s.columns.map((c) => (c.name === name ? { ...c, ...patch } : c)) } : s));
  };

  // `ApplyBomPreset`: a built-in view or one the dialog holds, its columns, grouping and the two filters replacing the current ones (a user field stays, hidden)
  const applyPreset = async (name: string) => {
    if (!spec || name === "") return;
    const preset = [...(table?.built_in_presets ?? []), ...(held ?? [])].find((p) => p.name === name);
    if (preset) return setSpec(withPresetSpec(spec, preset.spec));
    const reply = await fetchFieldsTable(spec, isEmptyChanges(changes) ? null : changes, name, { variant, variant_edits: variantEdits, ...scopeKeys });
    if (!reply.ok) {
      setError(reply.message ?? "Could not apply the preset.");
      return;
    }
    setSpec(reply.spec);
  };

  // the symbols' own edits, then every variant's (`ApplyData`)
  const applyCmds = (): Cmd[] => [
    ...(isEmptyChanges(changes) ? [] : [{ op: "set_symbol_fields" as const, edits: changes.edits, add_fields: changes.add_fields, rename_fields: changes.rename_fields, remove_fields: changes.remove_fields } as Cmd]),
    ...variantApplyCmds(variantEdits),
  ];
  const apply = async (): Promise<boolean> => {
    if (!dirty) return true;
    // as one undo step
    const ok = await api.cmdBatch(applyCmds());
    if (ok) {
      setChanges(emptyChanges());
      setVariantEdits([]);
      dispatch({ type: "TOAST", message: "Symbol fields updated.", kind: "info" });
    }
    return ok;
  };

  /** Several cells staged at once (Cut, Delete and Paste of a selection): each as `commitCell` stages it. */
  const stageCells = (list: ReadonlyArray<{ row: FieldsTableRow; field: string; value: string }>) => {
    let next = changes;
    let nextVariant = variantEdits;
    for (const { row, field, value } of list) {
      // a cell of the shown variant is staged for it (`fieldVariant`), the Reference and `${EXCLUDE_FROM_BOARD}` for the symbol itself
      if (variant !== "" && fieldSupportsVariants(field)) for (const ref of row.refs) nextVariant = stageVariantEdit(nextVariant, variant, ref, field, value);
      else for (const ref of row.refs) next = stageEdit(next, ref, field, value);
    }
    if (next !== changes) setChanges(next);
    if (nextVariant !== variantEdits) setVariantEdits(nextVariant);
  };
  const commitCell = (row: FieldsTableRow, field: string, value: string) => stageCells([{ row, field, value }]);

  // ---- the "Schematic variants:" list (`onVariantSelectionChange`, `onAddVariant`, `onDeleteVariant`, `onRenameVariant`, `onCopyVariant`, `onEditVariantDescription`)
  const selectVariant = (name: string) => dispatch({ type: "SET_SCH_VARIANT", variant: name === DEFAULT_VARIANT_NAME ? "" : name });
  const canModify = variant !== "";
  const deleteVariant = async () => {
    if (!canModify) return say("Cannot delete the default variant.");
    const at = listed.indexOf(variant);
    const next = listed[Math.max(0, at - 1)] ?? DEFAULT_VARIANT_NAME;
    setVariantEdits(deleteStoredVariant(variantEdits, variant));
    if (await api.cmd({ op: "remove_design_variant", name: variant })) selectVariant(next);
  };
  const renameVariant = async (from: string, text: string) => {
    const to = trimName(text);
    if (to === from) return;
    const why = validateNewVariantName(variants, to, from);
    if (why) return say(why);
    const wasCurrent = variant === from;
    setVariantEdits(renameStoredVariant(variantEdits, from, to));
    if ((await api.cmd({ op: "rename_design_variant", name: from, new_name: to })) && wasCurrent) selectVariant(to);
  };
  const copyVariant = async (from: string, text: string) => {
    const to = trimName(text);
    const why = validateCopyNameInTable(listed, to);
    if (why) return say(why);
    if (await api.cmd({ op: "copy_design_variant", name: from, new_name: to })) selectVariant(to);
  };

  const addField = () => {
    if (!spec) return;
    const problem = validNewFieldName(spec, newField);
    if (problem) {
      dispatch({ type: "TOAST", message: problem, kind: "error" });
      return;
    }
    const name = newField.trim();
    setChanges(stageAddField(changes, name));
    setSpec(specAddColumn(spec, name));
    setNewField("");
  };

  // `OnRenameField`: `wxTextEntryDialog( this, _( "New field name:" ), _( "Rename Field" ), fieldName )`; the same name changes nothing, a name in use is refused
  // ("Field name %s already exists.")
  const renameField = (name: string) => setFieldDialog({ kind: "rename", name });
  const doRename = (name: string, to: string) => {
    if (!spec || to.trim() === name) return;
    if (spec.columns.some((c) => c.name.toLowerCase() === to.trim().toLowerCase())) {
      dispatch({ type: "TOAST", message: `Field name ${to.trim()} already exists.`, kind: "error" });
      return;
    }
    const problem = validNewFieldName(spec, to);
    if (problem) {
      dispatch({ type: "TOAST", message: problem, kind: "error" });
      return;
    }
    setChanges(stageRenameField(changes, name, to.trim()));
    setVariantEdits(variantEditsRenamed(variantEdits, name, to.trim()));
    setSpec(specRenameColumn(spec, name, to.trim()));
  };

  // `OnRemoveField`: "Are you sure you want to remove the field '%s'?"
  const removeField = (name: string) => setFieldDialog({ kind: "remove", name });
  const doRemove = (name: string) => {
    if (!spec) return;
    setChanges(stageRemoveField(changes, name));
    setVariantEdits(variantEditsRemoved(variantEdits, name));
    setSpec(specRemoveColumn(spec, name));
  };

  const toggleSort = (name: string) => {
    if (!spec) return;
    if (spec.sort_field === name) updateSpec({ sort_asc: !spec.sort_asc });
    else updateSpec({ sort_field: name, sort_asc: true });
  };

  const toggleExpanded = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const columns = spec?.columns ?? [];
  const shown = columns.map((c, i) => ({ c, i })).filter(({ c }) => c.show);

  // ---- the grid's right-button menus (`FIELDS_EDITOR_GRID_TRICKS`, `GRID_TRICKS`)
  const rowKey = (row: FieldsTableRow) => row.refs.join(",");
  /** A text cell the grid can write (`!isReadOnly && isTextEntry`): not the Reference, not a generated count, not an attribute's check box. */
  const writable = (name: string) => isEditableColumn(name) && !isAttributeColumn(name);
  /** The selected cells, top to bottom, left to right: the right-clicked one, or every shown cell (Select All). */
  const selectedCells = (target: CellSel): Array<{ row: FieldsTableRow; c: (typeof shown)[number]["c"]; i: number }> => {
    if (!table || !target) return [];
    if (target === "all") return table.rows.flatMap((row) => shown.map(({ c, i }) => ({ row, c, i })));
    const row = table.rows.find((r) => rowKey(r) === target.row);
    const col = shown.find(({ c }) => c.name === target.col);
    return row && col ? [{ row, c: col.c, i: col.i }] : [];
  };
  const cellText = (row: FieldsTableRow, i: number) => (row.mixed[i] ? "" : (row.cells[i] ?? ""));
  const copyCells = (target: CellSel) => {
    const cells = selectedCells(target);
    const byRow = new Map<FieldsTableRow, string[]>();
    for (const { row, i } of cells) byRow.set(row, [...(byRow.get(row) ?? []), cellText(row, i)]);
    gridClipboard = cellsAsText([...byRow.values()]);
    void navigator.clipboard?.writeText(gridClipboard).catch(() => undefined);
  };
  const clearCells = (target: CellSel) => stageCells(selectedCells(target).filter(({ c }) => writable(c.name)).map(({ row, c }) => ({ row, field: c.name, value: "" })));
  const pasteCells = async (row: FieldsTableRow, colName: string) => {
    let text = gridClipboard;
    try {
      text = (await navigator.clipboard?.readText()) || gridClipboard;
    } catch {
      // the page's own copy (a browser that will not read the system clipboard)
    }
    const grid = textAsCells(text);
    const rows = table?.rows ?? [];
    const r0 = rows.indexOf(row);
    const c0 = shown.findIndex(({ c }) => c.name === colName);
    const out: Array<{ row: FieldsTableRow; field: string; value: string }> = [];
    grid.forEach((cells, dr) =>
      cells.forEach((value, dc) => {
        const target = rows[r0 + dr];
        const col = shown[c0 + dc];
        if (target && col && writable(col.c.name)) out.push({ row: target, field: col.c.name, value });
      })
    );
    stageCells(out);
  };
  const onCellMenu = (e: React.MouseEvent, row: FieldsTableRow, colName: string) => {
    e.preventDefault();
    const keepAll = sel === "all";
    // the cells the menu acts on: every one after Select All, else the one clicked (which becomes the selection)
    const target: CellSel = keepAll ? "all" : { row: rowKey(row), col: colName };
    setSel(target);
    const anyWritable = keepAll ? shown.some(({ c }) => writable(c.name)) : writable(colName);
    const text = (() => {
      const i = columns.findIndex((c) => c.name === colName);
      return i >= 0 ? cellText(row, i) : "";
    })();
    setMenu({
      at: { x: e.clientX, y: e.clientY },
      items: gridCellMenu(colName, anyWritable),
      onPick: (id) => {
        if (id === "select_footprint") setChoosingFootprint({ row, value: text });
        else if (id === "show_datasheet") {
          // `GetAssociatedDocument`: a URL opens in the browser
          if (text.trim() !== "") window.open(text.trim(), "_blank", "noopener");
        } else if (id === "copy") copyCells(target);
        else if (id === "cut") (copyCells(target), clearCells(target));
        else if (id === "delete") clearCells(target);
        else if (id === "paste") void pasteCells(row, colName);
        else if (id === "select_all") setSel("all");
      },
    });
  };
  // `onGridLabelRightClick`: every column, checked when shown; a pick shows or hides it (`ShowHideColumn`)
  const onHeaderMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setMenu({
      at: { x: e.clientX, y: e.clientY },
      items: columnMenu(columns.map((c) => ({ id: c.name, label: c.label || c.name, shown: c.show }))),
      onPick: (id) => {
        const name = id.slice("showhide:".length);
        const c = columns.find((x) => x.name === name);
        if (c) setColumn(name, { show: !c.show });
      },
    });
  };
  // the field list's header (`VIEW_CONTROLS_GRID_TRICKS`): its own four columns
  const onListHeaderMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setMenu({
      at: { x: e.clientX, y: e.clientY },
      items: columnMenu(FIELD_LIST_COLUMNS.map((label, k) => ({ id: String(k), label, shown: listCols[k] !== false }))),
      onPick: (id) => {
        const k = Number(id.slice("showhide:".length));
        setListCols((cols) => cols.map((v, j) => (j === k ? !v : v)));
      },
    });
  };
  const isSelected = (row: FieldsTableRow, name: string) => sel === "all" || (sel !== null && sel.row === rowKey(row) && sel.col === name);
  const selStyle = (row: FieldsTableRow, name: string) => (isSelected(row, name) ? { outline: "2px solid var(--chrome-accent, #3b82f6)", outlineOffset: -2 } : {});

  // ---- the export's format presets (`onBomFmtPresetChanged`)
  const userFmts = heldFmt ?? [];
  const onFmtBox = (value: string) => {
    if (value === PRESET_SEPARATOR) return;
    if (value === SAVE_PRESET) return setFmtDialog({ kind: "save", name: lastFmt });
    if (value === DELETE_PRESET) return setFmtDialog({ kind: "delete", pick: userFmts[0]?.name ?? "" });
    const p = [...bomFmtPresets(), ...userFmts].find((x) => x.name === value);
    if (!p) return;
    setLastFmt(userFmts.some((x) => x.name === value) ? value : "");
    setFmt({ ...p });
  };

  const renderRow = (row: FieldsTableRow, child: boolean, rowKey: string) => (
    <tr key={rowKey} style={{ background: child ? "var(--chrome-bg)" : undefined }}>
      <td style={{ width: 22, textAlign: "center", padding: "2px 4px" }}>
        {row.flag === "group" && (
          <button title={expanded.has(rowKey) ? "Collapse" : "Expand"} style={{ padding: "0 4px", fontSize: 10 }} onClick={() => toggleExpanded(rowKey)}>
            {expanded.has(rowKey) ? "v" : ">"}
          </button>
        )}
      </td>
      {shown.map(({ c, i }) => {
        const text = row.cells[i] ?? "";
        const mixed = row.mixed[i] ?? false;
        // `GetAttr`'s highlight of a variant's own value (`VARIANT_FIELD_OVERRIDE_LIGHT_YELLOW` / `_DARK_YELLOW`)
        const highlight = variant !== "" && row.overrides?.[i] ? VARIANT_OVERRIDE_COLOR[scheme] : undefined;
        if (isAttributeColumn(c.name)) {
          return (
            <td key={c.name} style={{ padding: "1px 4px", textAlign: "center", background: highlight, ...selStyle(row, c.name) }} data-override={highlight ? "1" : undefined} onContextMenu={(e) => onCellMenu(e, row, c.name)}>
              <AttributeCell text={text} mixed={mixed} label={`${c.label || c.name} of ${row.refs.join(", ")}`} onToggle={() => commitCell(row, c.name, toggledAttribute(text, mixed))} />
            </td>
          );
        }
        if (!isEditableColumn(c.name)) {
          return (
            <td key={c.name} style={{ padding: "2px 8px", color: child ? "var(--chrome-text-dim)" : undefined, paddingLeft: child && c.name === "Reference" ? 22 : 8, ...selStyle(row, c.name) }} onContextMenu={(e) => onCellMenu(e, row, c.name)}>
              {text}
            </td>
          );
        }
        return (
          <td key={c.name} style={{ padding: "1px 4px", background: highlight, ...selStyle(row, c.name) }} data-override={highlight ? "1" : undefined} onContextMenu={(e) => onCellMenu(e, row, c.name)}>
            <input
              // A fresh uncontrolled input whenever the row/cell value changes (regroup, apply), so the committed text always shows.
              key={`${row.refs.join(",")}|${c.name}|${mixed ? MIXED : text}`}
              defaultValue={mixed ? "" : text}
              placeholder={mixed ? MIXED : ""}
              style={{ width: "100%", boxSizing: "border-box", fontSize: 11, background: highlight }}
              aria-label={`${c.label || c.name} of ${row.refs.join(", ")}`}
              onBlur={(e) => {
                const v = e.currentTarget.value;
                if (mixed ? v !== "" : v !== text) commitCell(row, c.name, v);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") e.currentTarget.blur();
              }}
            />
          </td>
        );
      })}
    </tr>
  );

  return (
    <div className="dialog-backdrop" onClick={close}>
      <div
        className="dialog"
        style={{ width: 980, maxWidth: "96vw", maxHeight: "88vh" }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") close();
        }}
      >
        <div className="dialog-header">
          <span>Symbol Fields Table</span>
          <span style={{ display: "flex", gap: 6 }}>
            <button className={tab === "edit" ? "primary" : undefined} onClick={() => setTab("edit")}>
              Edit Fields
            </button>
            <button className={tab === "export" ? "primary" : undefined} onClick={() => setTab("export")}>
              Export
            </button>
          </span>
        </div>

        <div className="dialog-body" style={{ paddingTop: 10 }}>
          {error && <div className="problem-row">{error}</div>}

          {tab === "edit" && spec && (
            <div style={{ display: "flex", gap: 14, alignItems: "flex-start" }}>
              <div style={{ width: 230, flexShrink: 0, fontSize: 11 }}>
                <div style={{ fontWeight: 600, marginBottom: 4 }}>Fields</div>
                <table style={{ width: "100%", borderCollapse: "collapse" }} data-testid="fields-table-field-list">
                  <thead>
                    <tr style={{ color: "var(--chrome-text-dim)" }} onContextMenu={onListHeaderMenu}>
                      {listCols[0] !== false && <th style={{ textAlign: "left", fontWeight: 500 }}>{FIELD_LIST_COLUMNS[0]}</th>}
                      {listCols[1] !== false && <th style={{ textAlign: "left", fontWeight: 500 }}>{FIELD_LIST_COLUMNS[1]}</th>}
                      {listCols[2] !== false && <th title="Show column">{FIELD_LIST_COLUMNS[2]}</th>}
                      {listCols[3] !== false && <th title="Group symbols that share this value">{FIELD_LIST_COLUMNS[3]}</th>}
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {columns.map((c) => (
                      <tr key={c.name}>
                        {listCols[0] !== false && <td style={{ padding: "2px 0", whiteSpace: "nowrap" }}>{c.name}</td>}
                        {listCols[1] !== false && (
                          <td>
                            {/* LABEL_COLUMN: the column's label in the grid and in the export (`SetColLabelValue`) */}
                            <input
                              key={`${c.name}|${c.label}`}
                              aria-label={`BOM Name of ${c.name}`}
                              defaultValue={c.label || c.name}
                              style={{ width: 80, fontSize: 11 }}
                              onBlur={(e) => {
                                const label = e.currentTarget.value;
                                if (spec && label !== (c.label || c.name)) setSpec(specSetLabel(spec, c.name, label));
                              }}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") e.currentTarget.blur();
                              }}
                            />
                          </td>
                        )}
                        {listCols[2] !== false && (
                          <td style={{ textAlign: "center" }}>
                            <input type="checkbox" checked={c.show} aria-label={`Show ${c.name}`} onChange={(e) => setColumn(c.name, { show: e.target.checked })} />
                          </td>
                        )}
                        {listCols[3] !== false && (
                          <td style={{ textAlign: "center" }}>
                            <input type="checkbox" checked={c.group_by} aria-label={`Group by ${c.name}`} onChange={(e) => setColumn(c.name, { group_by: e.target.checked })} />
                          </td>
                        )}
                        <td style={{ whiteSpace: "nowrap" }}>
                          {!BUILTIN.has(c.name) && !isGenerated(c.name) && (
                            <>
                              <button style={{ fontSize: 10, padding: "0 4px" }} title="Rename field" onClick={() => renameField(c.name)}>
                                Ren
                              </button>
                              <button style={{ fontSize: 10, padding: "0 4px", marginLeft: 2 }} title="Remove field from every symbol" onClick={() => removeField(c.name)}>
                                Del
                              </button>
                            </>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
                <div style={{ display: "flex", gap: 4, marginTop: 8 }}>
                  <input
                    placeholder="New field name"
                    value={newField}
                    style={{ flex: 1, minWidth: 0, fontSize: 11 }}
                    onChange={(e) => setNewField(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") addField();
                    }}
                  />
                  <button onClick={addField}>Add Field</button>
                </div>
                {/* `m_variantsPanel`: "Schematic variants:", the list and its buttons */}
                <div style={{ marginTop: 14 }} data-testid="fields-table-variants">
                  <div style={{ marginBottom: 4 }}>Schematic variants:</div>
                  <select size={Math.max(3, Math.min(8, listed.length))} style={{ width: "100%", fontSize: 11 }} aria-label="Schematic variants" value={variant === "" ? DEFAULT_VARIANT_NAME : variant} onChange={(e) => selectVariant(e.target.value)} data-testid="fields-table-variant-list">
                    {listed.map((n) => (
                      <option key={n} value={n} title={variantDescription(variants, n) || undefined}>
                        {n}
                      </option>
                    ))}
                  </select>
                  <div style={{ display: "flex", gap: 4, marginTop: 4 }}>
                    <button style={{ fontSize: 10, padding: "0 6px" }} title="Add Design Variant..." onClick={() => setVariantDialog({ kind: "add", name: "" })} data-testid="fields-table-add-variant">
                      +
                    </button>
                    <button style={{ fontSize: 10, padding: "0 6px" }} title="Rename Design Variant..." disabled={!canModify} onClick={() => setVariantDialog({ kind: "rename", name: variant })}>
                      Rename
                    </button>
                    <button style={{ fontSize: 10, padding: "0 6px" }} title="Copy Design Variant..." disabled={!canModify} onClick={() => setVariantDialog({ kind: "copy", name: variant })}>
                      Copy
                    </button>
                    <button style={{ fontSize: 10, padding: "0 6px" }} title="Edit Variant Description..." disabled={!canModify} onClick={() => setVariantDialog({ kind: "describe", name: variant })}>
                      Description
                    </button>
                    <button style={{ fontSize: 10, padding: "0 6px", marginLeft: "auto" }} title="Remove Design Variant" disabled={!canModify} onClick={() => void deleteVariant()} data-testid="fields-table-delete-variant">
                      Delete
                    </button>
                  </div>
                </div>
              </div>

              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 8, fontSize: 11 }}>
                  <label className="toggle">
                    <input type="checkbox" checked={spec.group_symbols} onChange={(e) => updateSpec({ group_symbols: e.target.checked })} />
                    Group symbols
                  </label>
                  <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    Filter
                    <input value={spec.filter} placeholder="Reference…" style={{ fontSize: 11 }} onChange={(e) => updateSpec({ filter: e.target.value })} />
                  </label>
                  <select aria-label="Scope" value={scope} style={{ fontSize: 11 }} onChange={(e) => setScope(e.target.value as FieldsTableScope)}>
                    {SCOPE_CHOICES.map((c) => (
                      <option key={c.value} value={c.value}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                  <label style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    View presets:
                    <select aria-label="Preset" value={currentPreset ?? PRESET_SEPARATOR} style={{ fontSize: 11 }} onChange={(e) => onPresetBox(e.target.value)}>
                      {presetBox.map((p) => (
                        <option key={p} value={p}>
                          {p}
                        </option>
                      ))}
                    </select>
                  </label>
                  <span style={{ color: "var(--chrome-text-dim)" }}>{table ? `${table.rows.length} row${table.rows.length === 1 ? "" : "s"}` : ""}</span>
                </div>
                {/* the options menu of the dialog: "Include 'DNP' Symbols" and "Include 'Exclude from BOM' Symbols" */}
                <div style={{ display: "flex", alignItems: "center", gap: 14, marginBottom: 8, fontSize: 11 }}>
                  <label className="toggle" title="Show symbols marked 'DNP' in the table. This setting also controls whether or not 'DNP' symbols are included on export.">
                    <input type="checkbox" aria-label="Include DNP symbols" checked={!spec.exclude_dnp} onChange={(e) => updateSpec({ exclude_dnp: !e.target.checked })} />
                    Include 'DNP' symbols
                  </label>
                  <label className="toggle" title="Show symbols marked 'Exclude from BOM' in the table. Symbols marked 'Exclude from BOM' are never included on export.">
                    <input type="checkbox" aria-label="Include Exclude from BOM symbols" checked={!!spec.include_excluded_from_bom} onChange={(e) => updateSpec({ include_excluded_from_bom: e.target.checked })} />
                    Include 'Exclude from BOM' symbols
                  </label>
                </div>
                <div style={{ overflow: "auto", maxHeight: "52vh", border: "1px solid var(--chrome-border)", borderRadius: 6 }}>
                  <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
                    <thead>
                      <tr style={{ position: "sticky", top: 0, background: "var(--chrome-bg-raised)" }} onContextMenu={onHeaderMenu} data-testid="fields-table-header">
                        <th />
                        {shown.map(({ c }) => (
                          <th key={c.name} style={{ textAlign: "left", padding: "4px 8px", cursor: "pointer", whiteSpace: "nowrap" }} title="Sort by this column" onClick={() => toggleSort(c.name)}>
                            {c.label || c.name}
                            {spec.sort_field === c.name ? (spec.sort_asc ? " ▲" : " ▼") : ""}
                          </th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {table?.rows.map((row) => {
                        const key = row.refs[0] ?? String(row.item_number);
                        return [renderRow(row, false, key), ...(row.flag === "group" && expanded.has(key) ? row.children.map((ch) => renderRow(ch, true, `${key}/${ch.refs[0] ?? ""}`)) : [])];
                      })}
                    </tbody>
                  </table>
                  {table && table.rows.length === 0 && <div className="panel-empty">No symbols match.</div>}
                  {!table && <div className="panel-empty">Loading…</div>}
                </div>
                <p style={{ color: "var(--chrome-text-dim)", fontSize: 11, marginBottom: 0 }}>
                  Edits are staged and regroup the table; "Apply" saves them all as one undoable step. A group row edits every symbol in it.
                </p>
              </div>
            </div>
          )}

          {tab === "export" && spec && (
            <div style={{ display: "flex", gap: 14, alignItems: "flex-start" }}>
              <div style={{ width: 280, flexShrink: 0 }}>
                <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <span>Format presets:</span>
                  <select aria-label="Format preset" value={matchingFmtPreset(fmt, userFmts) ?? PRESET_SEPARATOR} onChange={(e) => onFmtBox(e.target.value)} data-testid="fields-table-fmt-preset">
                    {fmtPresetChoices(userFmts).map((name) => (
                      <option key={name} value={name}>
                        {name}
                      </option>
                    ))}
                  </select>
                  <span>Field delimiter</span>
                  <input value={fmt.field_delimiter === "\t" ? "\\t" : fmt.field_delimiter} onChange={(e) => setFmt({ ...fmt, field_delimiter: e.target.value === "\\t" ? "\t" : e.target.value })} />
                  <span>String delimiter</span>
                  <input value={fmt.string_delimiter} onChange={(e) => setFmt({ ...fmt, string_delimiter: e.target.value })} />
                  <span>Reference delimiter</span>
                  <input value={fmt.ref_delimiter} onChange={(e) => setFmt({ ...fmt, ref_delimiter: e.target.value })} />
                  <span>Ref range delimiter</span>
                  <input value={fmt.ref_range_delimiter} placeholder="(none: list every reference)" onChange={(e) => setFmt({ ...fmt, ref_range_delimiter: e.target.value })} />
                </div>
                <label className="toggle" style={{ display: "block", marginTop: 8, fontSize: 11 }}>
                  <input type="checkbox" checked={fmt.keep_tabs} onChange={(e) => setFmt({ ...fmt, keep_tabs: e.target.checked })} />
                  Keep tabs
                </label>
                <label className="toggle" style={{ display: "block", fontSize: 11 }}>
                  <input type="checkbox" checked={fmt.keep_line_breaks} onChange={(e) => setFmt({ ...fmt, keep_line_breaks: e.target.checked })} />
                  Keep line breaks
                </label>
                <div className="kv-grid" style={{ gridTemplateColumns: "120px 1fr", marginTop: 10 }}>
                  <span>Output file</span>
                  <input value={outPath} placeholder="export/bom.csv" title="A file inside this board's export/ folder; kicad-cli writes it from the saved design" onChange={(e) => setOutPath(e.target.value)} />
                </div>
                {exportMsg && <div style={{ marginTop: 8, fontSize: 11, color: "var(--chrome-text-dim)" }}>{exportMsg}</div>}
              </div>
              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontWeight: 600, fontSize: 11, marginBottom: 4 }}>Preview</div>
                <textarea readOnly value={preview} spellCheck={false} style={{ width: "100%", boxSizing: "border-box", height: "46vh", fontFamily: "monospace", fontSize: 11, whiteSpace: "pre" }} />
              </div>
            </div>
          )}
        </div>

        <div className="dialog-footer">
          {tab === "export" ? (
            <button
              className="primary"
              disabled={!spec || outPath.trim() === ""}
              onClick={async () => {
                if (!spec) return;
                // The file is kicad-cli's (`kicad-cli sch export bom`, run on the saved design): edits that have not been applied would not be in it.
                if (dirty) {
                  if (!window.confirm("Changes have not yet been applied. Apply them and export?")) return;
                  if (!(await apply())) return;
                }
                setExportMsg("Exporting with kicad-cli...");
                try {
                  const reply = await postBomFile({ spec, fmt, path: outPath.trim(), ...(variant ? { variant } : {}) });
                  setExportMsg(reply.ok ? `Wrote BOM output to '${reply.files?.[0] ?? outPath}'${reply.engine ? ` (${reply.engine})` : ""}` : (reply.message ?? "Export failed."));
                } catch (e) {
                  setExportMsg(`Export failed: ${e instanceof Error ? e.message : String(e)}`);
                }
              }}
            >
              Export
            </button>
          ) : (
            <>
              <button
                disabled={!dirty}
                onClick={() => {
                  setChanges(emptyChanges());
                  setVariantEdits([]);
                }}
                title="Discard the staged edits"
              >
                Revert
              </button>
              <button className="primary" disabled={!dirty} onClick={() => void apply()}>
                Apply
              </button>
            </>
          )}
          <button onClick={close}>Close</button>
        </div>
      </div>
      {/* the preset box's dialogs and the close question, over the table */}
      {menu && <PopupMenu at={menu.at} items={menu.items} onPick={menu.onPick} onClose={() => setMenu(null)} />}
      {choosingFootprint && (
        <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          <FootprintChooserDialog
            preselect={choosingFootprint.value.includes(":") ? choosingFootprint.value : null}
            onCancel={() => setChoosingFootprint(null)}
            onChoose={(pick) => {
              // "Select Footprint...": `frame->ShowModal( &fpid, m_dlg )`, then `SetCellValue( row, col, fpid )`
              const row = choosingFootprint.row;
              setChoosingFootprint(null);
              if (pick.kind === "footprint") commitCell(row, "Footprint", pick.name);
            }}
          />
        </div>
      )}
      {fmtDialog && (
        <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {fmtDialog.kind === "save" && (
            <TextEntryDialog
              title="Save BOM Preset"
              prompt="BOM preset name:"
              initial={fmtDialog.name}
              onCancel={() => setFmtDialog(null)}
              onOk={(t) => {
                const name = t;
                if (!name.trim()) return setFmtDialog(null);
                const outcome = saveFmtOutcome(name, userFmts);
                if (outcome === "read_only") return setFmtDialog({ kind: "error", message: READ_ONLY_PRESET });
                if (outcome === "overwrite") return setFmtDialog({ kind: "overwrite", name });
                setFmtDialog(null);
                setHeldFmt((l) => fmtWithSaved(l ?? [], name, fmt));
                setLastFmt(name);
              }}
            />
          )}
          {fmtDialog.kind === "overwrite" && (
            <SchDialogShell
              title="Confirmation"
              width={340}
              okLabel="OK"
              onCancel={() => setFmtDialog(null)}
              onOk={() => {
                const name = fmtDialog.name;
                setFmtDialog(null);
                setHeldFmt((l) => fmtWithSaved(l ?? [], name, fmt));
                setLastFmt(name);
              }}
            >
              <p style={{ margin: 0 }} data-testid="fields-table-fmt-overwrite">
                Overwrite existing preset?
              </p>
            </SchDialogShell>
          )}
          {fmtDialog.kind === "error" && (
            <SchDialogShell title="Error" width={340} cancelLabel="OK" onCancel={() => setFmtDialog(null)}>
              <p style={{ margin: 0, whiteSpace: "pre-wrap" }} data-testid="fields-table-fmt-error">
                {fmtDialog.message}
              </p>
            </SchDialogShell>
          )}
          {fmtDialog.kind === "delete" && (
            <SchDialogShell
              title="Delete Preset"
              width={340}
              canOk={fmtDialog.pick !== ""}
              onCancel={() => setFmtDialog(null)}
              onOk={() => {
                const name = fmtDialog.pick;
                setFmtDialog(null);
                if (name) setHeldFmt((l) => heldWithout(l ?? [], name));
              }}
            >
              <div style={{ marginBottom: 6 }}>Select preset:</div>
              <select aria-label="Format presets" size={Math.max(2, Math.min(8, userFmts.length))} style={{ width: "100%" }} value={fmtDialog.pick} onChange={(e) => setFmtDialog({ kind: "delete", pick: e.target.value })}>
                {userFmts.map((f) => (
                  <option key={f.name} value={f.name}>
                    {f.name}
                  </option>
                ))}
              </select>
            </SchDialogShell>
          )}
        </div>
      )}
      {(presetDialog || askClose || fieldDialog) && (
        <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {fieldDialog?.kind === "rename" && (
            <TextEntryDialog
              title="Rename Field"
              prompt="New field name:"
              initial={fieldDialog.name}
              onCancel={() => setFieldDialog(null)}
              onOk={(t) => {
                const from = fieldDialog.name;
                setFieldDialog(null);
                doRename(from, t);
              }}
            />
          )}
          {fieldDialog?.kind === "remove" && (
            <SchDialogShell title="Confirmation" width={360} onCancel={() => setFieldDialog(null)} okLabel="OK" onOk={() => (setFieldDialog(null), doRemove(fieldDialog.name))}>
              <p style={{ margin: 0 }} data-testid="fields-table-remove-field">{`Are you sure you want to remove the field '${fieldDialog.name}'?`}</p>
            </SchDialogShell>
          )}
          {presetDialog?.kind === "save" && (
            <TextEntryDialog
              title="Save BOM Preset"
              prompt="BOM preset name:"
              initial={presetDialog.name}
              onCancel={() => setPresetDialog(null)}
              onOk={(t) => {
                const name = t.trim();
                if (!name) return setPresetDialog(null);
                const outcome = savePresetOutcome(name, builtInNames, userNames);
                if (outcome === "read_only") return setPresetDialog({ kind: "error", message: READ_ONLY_PRESET });
                if (outcome === "overwrite") return setPresetDialog({ kind: "overwrite", name });
                setPresetDialog(null);
                void savePreset(name);
              }}
            />
          )}
          {presetDialog?.kind === "overwrite" && (
            <SchDialogShell title="Confirmation" width={340} cancelLabel="Cancel" okLabel="OK" onCancel={() => setPresetDialog(null)} onOk={() => (setPresetDialog(null), void savePreset(presetDialog.name))}>
              <p style={{ margin: 0 }} data-testid="fields-table-overwrite">
                Overwrite existing preset?
              </p>
            </SchDialogShell>
          )}
          {presetDialog?.kind === "error" && (
            <SchDialogShell title="Error" width={340} cancelLabel="OK" onCancel={() => setPresetDialog(null)}>
              <p style={{ margin: 0, whiteSpace: "pre-wrap" }} data-testid="fields-table-preset-error">
                {presetDialog.message}
              </p>
            </SchDialogShell>
          )}
          {presetDialog?.kind === "delete" && (
            <SchDialogShell
              title="Delete Preset"
              width={340}
              canOk={presetDialog.pick !== ""}
              onCancel={() => setPresetDialog(null)}
              onOk={() => {
                const name = presetDialog.pick;
                setPresetDialog(null);
                // `m_bomPresets.erase( presetName )`: the project loses it when the dialog closes
                if (name) setHeld((l) => heldWithout(l ?? [], name));
              }}
            >
              <div style={{ marginBottom: 6 }}>Select preset:</div>
              <select aria-label="Presets" size={Math.max(2, Math.min(8, userNames.length))} style={{ width: "100%" }} value={presetDialog.pick} onChange={(e) => setPresetDialog({ kind: "delete", pick: e.target.value })}>
                {userNames.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </select>
            </SchDialogShell>
          )}
          {askClose && (
            <div className="dialog-backdrop" onClick={() => setAskClose(false)}>
              <div className="dialog" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
                <div className="dialog-header">
                  <span>{UNSAVED_CHANGES.title}</span>
                </div>
                <div className="dialog-body">
                  <p style={{ marginTop: 0 }} data-testid="fields-table-save-changes">
                    {UNSAVED_CHANGES.message}
                  </p>
                  <p style={{ color: "var(--chrome-text-dim)", fontSize: 11 }}>{UNSAVED_CHANGES.detail}</p>
                </div>
                <div className="dialog-footer">
                  <button
                    onClick={() => {
                      setAskClose(false);
                      void closeNow();
                    }}
                  >
                    {UNSAVED_CHANGES.discard}
                  </button>
                  <button onClick={() => setAskClose(false)}>{UNSAVED_CHANGES.cancel}</button>
                  <button
                    className="primary"
                    onClick={() => {
                      setAskClose(false);
                      // `TransferDataFromWindow`, then the destructor's `savePresetsToSchematic`: one step
                      void closeNow(applyCmds());
                    }}
                  >
                    {UNSAVED_CHANGES.save}
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}
      {/* the variant dialogs over the table: their clicks and keys are theirs, not the table's */}
      {variantDialog && (
        <div onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {variantDialog.kind === "add" && <AddVariantDialog onClose={() => setVariantDialog(null)} say={say} />}
          {variantDialog.kind === "rename" && (
            <TextEntryDialog
              title="Rename Design Variant"
              prompt="Enter new variant name:"
              initial={variantDialog.name}
              onCancel={() => setVariantDialog(null)}
              onOk={(t) => {
                const from = variantDialog.name;
                setVariantDialog(null);
                void renameVariant(from, t);
              }}
            />
          )}
          {variantDialog.kind === "copy" && (
            <TextEntryDialog
              title="Copy Design Variant"
              prompt="Enter name for the copied variant:"
              initial={`${variantDialog.name}_copy`}
              onCancel={() => setVariantDialog(null)}
              onOk={(t) => {
                const from = variantDialog.name;
                setVariantDialog(null);
                void copyVariant(from, t);
              }}
            />
          )}
          {variantDialog.kind === "describe" && (
            <DescriptionEntryDialog
              name={variantDialog.name}
              initial={variantDescription(variants, variantDialog.name)}
              onCancel={() => setVariantDialog(null)}
              onOk={(t) => {
                const name = variantDialog.name;
                setVariantDialog(null);
                void api.cmd({ op: "edit_design_variant_description", name, description: trimName(t) });
              }}
            />
          )}
        </div>
      )}
    </div>
  );
}
