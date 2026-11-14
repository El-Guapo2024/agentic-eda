// Board Setup > Board Stackup > Physical Stackup -- port of pcbnew/board_stackup_manager/panel_board_stackup.cpp (Board Finish is its own page, BoardFinishPage.tsx): the
// number of copper layers, "Impedance controlled", one row per sublayer of each layer (`lazyBuildRowUI`: a dielectric's Core / PrePreg choice on its first
// sublayer only, its thickness lock), Add and Remove Dielectric Layer with KiCad's lists, Adjust Dielectric Thickness and the checks of the "Errors" box. The table is the `(stackup ..)` of the derived `.kicad_pcb`; the board thickness is what the layers add up to (`BuildBoardThicknessFromStackup`), and
// the copper layers are the board's layers. The pure parts are kicad-port/physicalStackup.ts.
//
// Fewer copper layers while something is on a layer that goes asks KiCad's question (`PANEL_SETUP_LAYERS::TransferDataFromWindow`: "Warning", "Items have been found
// on removed layers ...", "Show Details" with each item's description); OK sends `set_stackup` with `delete_items_on_removed_layers`, and the backend deletes them as
// `BOARD::RemoveAllItemsOnLayer` does, in the same step, and the board directory then clears the board's undo and redo history as KiCad's `ClearUndoRedoList` does
// (crates/cli/src/board.rs `clear_board_history`), so the question's "cannot be undone" holds. A click on an item of "Show Details" selects it on the board
// (`SetSelectionCallback`: `selectionClear`, then `selectItem`).
//
// Colours: a mask, a silkscreen and a dielectric have KiCad's colour box (`createColorBox`: `GetStandardColors`' names with their swatches, "User defined" last,
// which opens the colour picker), stored as KiCad's colour name or `#RRGGBBAA`; the 3D viewer's "Use board stackup colors" paints the board with them
// (kicad-port/appearance3d.ts). Each row starts with its colour icon (`getColorIconItem`). The "..." beside a material opens KiCad's material dialog
// (`onMaterialChange`, `DIALOG_DIELECTRIC_MATERIAL`); "Export to Clipboard" puts `BuildStackupReport`'s text on the clipboard. Epsilon R and loss tangent always
// show a number, the type's default where the board file says none, and Apply writes every value as KiCad's `transferDataFromUIToStackup` does
// (`withKicadValues`). A different copper layer count rebuilds the table from the default stackup, keeping what the layers that remain say (`withCopperLayers`),
// and shares the board's old thickness among the unlocked dielectrics (`onCopperLayersSelCount`'s `setDefaultLayerWidths( oldBoardWidth )`).
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useStudioDispatch, useStudioState } from "../../state/store";
import { defaultStackup, mmText, validateStackup, withCopperLayers, withKnownKinds } from "../../kicad-port/boardSetupRules";
import {
  CORE_PREPREG,
  REMOVED_LAYERS_DETAILS,
  REMOVED_LAYERS_MESSAGE,
  REMOVED_LAYERS_TITLE,
  addDielectricChoices,
  addDielectricSublayer,
  adjustDielectricThickness,
  adjustThicknessPrompt,
  addMaterialsInUse,
  applySubstrate,
  buildStackupReport,
  canRemoveDielectric,
  colorIndex,
  colorListType,
  colorToHexString,
  defaultUserColor,
  formatPrm,
  hexColorCss,
  iconColorCss,
  kicadPrms,
  MATERIAL_DIALOG,
  materialDialogPick,
  materialDialogResult,
  materialDialogStart,
  materialListType,
  newMaterialLists,
  NOT_SPECIFIED,
  prmSpecified,
  removedLayerItems,
  standardColorCss,
  standardColors,
  toDouble,
  withKicadValues,
  type ColorListType,
  type DielectricSubstrate,
  type RemovedLayerItem,
  corePrepregOf,
  itemHasEpsilonR,
  itemMaterialEditable,
  itemThicknessEditable,
  kindOfChoice,
  removeDielectricChoices,
  removeDielectricSublayer,
  stackupEntryErrors,
  stackupRows,
  stackupTotalThicknessNm,
  sublayerOf,
  withCopperLayerCount,
  withSublayer,
  type DielectricChoice,
  type StackupRow,
} from "../../kicad-port/physicalStackup";
import { layerDisplayName } from "../../kicad-port/boardLayers";
import { parseLength } from "../../kicad-port/importGraphics";
import type { StackupSettings, StackupSublayer } from "../../api/types";
import { DialogShell } from "../pcbDialogKit";
import { ColorDialog, MessageDialog, NameDialog, Pane, PickDialog } from "../panels/appearance/shared";
import { Check, FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps, useSetupImport } from "./fields";
import { finishDraftOf, withFinish } from "../../kicad-port/boardFinish";

const COPPER_COUNTS = Array.from({ length: 16 }, (_, i) => (i + 1) * 2);


/** A colour swatch (`LAYER_PRESENTATION::DrawColorSwatch`, `m_colorSwatchesSize` 14 x 14). */
function ColorSwatch({ css }: { css: string }) {
  return <span aria-hidden style={{ display: "inline-block", width: 14, height: 14, border: "1px solid var(--chrome-border, #888)", background: css, flex: "none" }} />;
}

/**
 * The colour box of a stackup row (`createColorBox`, `lazyBuildRowUI`, `onColorSelected`): the names of `GetStandardColors( type )` with "User defined" last (shown as
 * the `#RRGGBBAA` colour when the layer has one), the selected entry's swatch beside it; "User defined" opens the colour picker (`DIALOG_COLOR_PICKER`, "Reset to
 * Default" giving `GetDefaultUserColor`) and keeps its colour (`m_UserColor`) even when the picker is cancelled. A colour name the list does not have shows as "Not
 * specified", as KiCad's box does. A browser's `<option>` cannot hold a picture, so the swatches of the entries that are not selected are not drawn, and the last
 * entry is not renamed "Custom..." while the list is open (`wxEVT_COMBOBOX_DROPDOWN`).
 */
function StackupColor({ type, value, ariaLabel, onChange }: { type: ColorListType; value: string; ariaLabel: string; onChange: (color: string | undefined) => void }) {
  const list = standardColors(type);
  const user = list.length - 1;
  const selected = colorIndex(type, value);
  const custom = value.startsWith("#");
  // `m_UserColor`: the layer's own colour when it has one, else `GetDefaultUserColor( type )`
  const [userColor, setUserColor] = useState(custom ? value : defaultUserColor(type));
  const [picking, setPicking] = useState(false);
  const swatch = selected === user ? (hexColorCss(userColor) ?? standardColorCss(type, user)) : standardColorCss(type, selected);
  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
      <ColorSwatch css={swatch} />
      <select
        aria-label={ariaLabel}
        value={selected}
        onChange={(e) => {
          const idx = Number(e.target.value);
          if (idx === user) {
            onChange(userColor);
            setPicking(true);
          } else onChange(idx === 0 ? undefined : list[idx]!.name);
        }}
      >
        {list.map((c, i) => (
          <option key={c.name} value={i}>
            {i === user && custom ? value : c.name}
          </option>
        ))}
      </select>
      {picking && (
        <ColorDialog
          title="Color Picker"
          initial={userColor}
          defaultColor={defaultUserColor(type)}
          onCancel={() => setPicking(false)}
          onPick={(css) => {
            setPicking(false);
            const hex = css ? colorToHexString(css) : null;
            if (!hex) return;
            setUserColor(hex);
            onChange(hex);
          }}
        />
      )}
    </span>
  );
}

/** An epsilon R or loss tangent box (a `wxTextCtrl`, `m_numericFieldsSize`): the value as `UIDouble2Str` writes it; the text is read at Apply, a text that is no number
 * kept as NaN for the "Errors" box (`transferDataFromUIToStackup`). */
function PrmField({ value, ariaLabel, onChange }: { value: number; ariaLabel: string; onChange: (v: number) => void }) {
  const [text, setText] = useState(() => formatPrm(value));
  useEffect(() => {
    if (!Object.is(toDouble(text) ?? Number.NaN, value)) setText(Number.isNaN(value) ? text : formatPrm(value));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
  return (
    <input
      type="text"
      className={`bs-input${Number.isNaN(value) || value < 0 ? " bs-invalid" : ""}`}
      style={{ width: 54 }}
      aria-label={ariaLabel}
      spellCheck={false}
      value={text}
      onChange={(e) => {
        setText(e.target.value);
        onChange(toDouble(e.target.value) ?? Number.NaN);
      }}
    />
  );
}

/**
 * `DIALOG_DIELECTRIC_MATERIAL` ("Dielectric Material Characteristics"): Name, Epsilon R and Loss Tan boxes, starting empty, "1" and "0"; the "Common materials:" list
 * with its three columns, whose entry fills the boxes when picked (`onListItemSelected`) and is taken out of the list by Delete (`onListKeyDown`); OK checks the two
 * numbers ("Incorrect value for Epsilon R", "Incorrect value for Loss Tangent") and gives the substrate.
 */
function MaterialDialog({ list, onDelete, onOk, onCancel }: { list: DielectricSubstrate[]; onDelete: (idx: number) => void; onOk: (s: DielectricSubstrate) => void; onCancel: () => void }) {
  const [fields, setFields] = useState(materialDialogStart);
  const [sel, setSel] = useState(-1);
  const [error, setError] = useState<string | null>(null);
  const pick = (idx: number) => {
    setSel(idx);
    if (idx >= 0 && idx < list.length) setFields(materialDialogPick(list[idx]!));
  };
  const cell = { padding: "1px 6px", whiteSpace: "nowrap" as const };
  return createPortal(
    <div className="bs-items-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DialogShell
        title={MATERIAL_DIALOG.title}
        width={420}
        onCancel={onCancel}
        onOk={() => {
          const r = materialDialogResult(fields);
          if ("error" in r) setError(r.error);
          else onOk(r.substrate);
        }}
      >
        <div style={{ display: "grid", gridTemplateColumns: "1fr auto auto", gap: "2px 5px", fontSize: 12 }}>
          <span>{MATERIAL_DIALOG.name}</span>
          <span>{MATERIAL_DIALOG.epsilonR}</span>
          <span>{MATERIAL_DIALOG.lossTangent}</span>
          <input type="text" aria-label="Material name" style={{ minWidth: 150 }} value={fields.name} onChange={(e) => setFields({ ...fields, name: e.target.value })} />
          <input type="text" aria-label="Material epsilon r" style={{ width: 70 }} value={fields.epsilonR} onChange={(e) => setFields({ ...fields, epsilonR: e.target.value })} />
          <input type="text" aria-label="Material loss tangent" style={{ width: 70 }} value={fields.lossTangent} onChange={(e) => setFields({ ...fields, lossTangent: e.target.value })} />
        </div>
        <div style={{ fontSize: 12, margin: "8px 0 4px" }}>{MATERIAL_DIALOG.common}</div>
        <div
          tabIndex={0}
          role="listbox"
          aria-label="Common materials"
          style={{ maxHeight: 220, overflow: "auto", border: "1px solid var(--chrome-border, #888)", fontSize: 12 }}
          onKeyDown={(e) => {
            if (e.key === "Delete" && sel >= 0 && sel < list.length) {
              onDelete(sel);
              // "Get the new material information for the next item in the list (or last if the deleted item was the last item)"
              setSel(sel < list.length - 1 ? sel : sel - 1);
            } else if (e.key === "ArrowDown") pick(Math.min(list.length - 1, sel + 1));
            else if (e.key === "ArrowUp") pick(Math.max(0, sel - 1));
          }}
        >
          <table style={{ borderCollapse: "collapse", width: "100%" }}>
            <thead>
              <tr>
                {MATERIAL_DIALOG.columns.map((c) => (
                  <th key={c} style={{ ...cell, textAlign: "left" }}>
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {list.map((m, i) => (
                <tr key={i} role="option" aria-selected={i === sel} data-material={m.name} onClick={() => pick(i)} style={i === sel ? { background: "var(--chrome-select, #3b6fb6)", color: "#fff" } : undefined}>
                  <td style={cell}>{m.name}</td>
                  <td style={cell}>{formatPrm(m.epsilonR)}</td>
                  <td style={cell}>{formatPrm(m.lossTangent)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </DialogShell>
      {error && <MessageDialog title="Message" message={error} onOk={() => setError(null)} onCancel={() => setError(null)} />}
    </div>,
    document.body
  );
}

/** `DIALOG_ITEMS_LIST`: a message, a collapsed "Show Details" pane with the list (one selected at a time, `wxLC_SINGLE_SEL`), OK and Cancel. Picking an entry calls
 * `onSelect` with its index (`SetSelectionCallback`). */
function ItemsListDialog({ title, message, details, items, onSelect, onOk, onCancel }: { title: string; message: string; details: string; items: string[]; onSelect: (index: number) => void; onOk: () => void; onCancel: () => void }) {
  const [sel, setSel] = useState(-1);
  return createPortal(
    <div className="bs-items-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DialogShell title={title} width={460} onCancel={onCancel} onOk={onOk}>
        <div style={{ fontSize: 12, marginBottom: 8 }}>{message}</div>
        <Pane title={details}>
          <ul aria-label="Items on removed layers" style={{ margin: 0, padding: 0, listStyle: "none", maxHeight: 200, overflow: "auto", fontSize: 12 }}>
            {items.map((it, i) => (
              <li
                key={i}
                role="option"
                aria-selected={i === sel}
                style={{ padding: "1px 4px", cursor: "default", ...(i === sel ? { background: "var(--chrome-select, #3b6fb6)", color: "#fff" } : {}) }}
                onClick={() => {
                  setSel(i);
                  onSelect(i);
                }}
              >
                {it}
              </li>
            ))}
          </ul>
        </Pane>
      </DialogShell>
    </div>,
    document.body
  );
}

/** A small padlock: the header of the thickness-lock column (`m_bitmapLockThickness`, `BITMAPS::locked`). */
function LockIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-label="Thickness locked" role="img" fill="none" stroke="currentColor" strokeWidth="1.2">
      <rect x="2" y="5.5" width="8" height="5.5" rx="1" />
      <path d="M4 5.5V4a2 2 0 0 1 4 0v1.5" />
    </svg>
  );
}

type Dialog =
  | { kind: "add" }
  | { kind: "remove" }
  | { kind: "adjust"; prompt: string; title: string }
  | { kind: "message"; title: string; text: string }
  | { kind: "removed"; items: RemovedLayerItem[]; settings: StackupSettings }
  | { kind: "material"; layer: number; sub: number };

export function StackupPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const dispatch = useStudioDispatch();
  const board = state.board;
  const rules = board?.board_rules;
  const units = state.units;
  const boardCopper = rules?.copper_layers ?? rules?.layer_names?.length ?? board?.layers.length ?? 2;
  const source = useMemo<StackupSettings | undefined>(() => {
    if (!rules) return undefined;
    return { copper_layers: boardCopper, stackup: withKnownKinds(rules.stackup ?? defaultStackup(boardCopper, rules.board_thickness_nm ?? 1_600_000)) };
  }, [rules, boardCopper]);
  const page = usePageDraft<StackupSettings>(source);
  // "Import Settings from Another Board..." (`PANEL_SETUP_BOARD_STACKUP::ImportSettingsFrom`, with the layers: UI-LEFT-2 BB4): the other board's copper count and stackup
  useSetupImport(
    "layers",
    (b) => {
      const r = b.board_rules;
      if (!r) return undefined;
      const n = r.copper_layers ?? r.layer_names?.length ?? b.layers.length;
      return { copper_layers: n, stackup: withKnownKinds(r.stackup ?? defaultStackup(n, r.board_thickness_nm ?? 1_600_000)) };
    },
    page.importDraft,
  );
  const apply = usePageApply();
  const [dialog, setDialog] = useState<Dialog | null>(null);
  // `m_delectricMatList`, `m_solderMaskMatList`, `m_silkscreenMatList`: the panel's own lists, added to by each "..." and deleted from in the dialog
  const materialLists = useRef(newMaterialLists());
  const [, setListsVersion] = useState(0);
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const layerName = (file: string) => layerDisplayName(rules?.layer_setup, file);
  const rows = draft ? stackupRows(draft.stackup, layerName) : [];

  const editRow = (r: StackupRow, patch: Partial<StackupSublayer>) =>
    page.edit((d) => ({ ...d, stackup: { ...d.stackup, layers: d.stackup.layers.map((l, i) => (i === r.layer ? withSublayer(l, r.sub, patch) : l)) } }));
  const setKind = (r: StackupRow, kind: string) => page.edit((d) => ({ ...d, stackup: { ...d.stackup, layers: d.stackup.layers.map((l, i) => (i === r.layer ? { ...l, kind } : l)) } }));
  const setStackup = (patch: Partial<StackupSettings["stackup"]>) => page.edit((d) => ({ ...d, stackup: { ...d.stackup, ...patch } }));

  // `transferDataFromUIToStackup`: every row's values written as KiCad writes them; the finish is the Board Finish page's (`m_panelFinish->TransferDataFromWindow`),
  // here the board's as that page last applied it
  const send = (settings: StackupSettings, deleteItems: boolean) => {
    const written = { ...settings, stackup: withFinish(withKicadValues(settings.stackup), finishDraftOf(rules?.stackup ?? undefined)) };
    void apply.run(deleteItems ? { op: "set_stackup", settings: written, delete_items_on_removed_layers: true } : { op: "set_stackup", settings: written }, page.markClean);
  };

  /** `transferDataFromUIToStackup`'s checks: `wxMessageBox( error_msg, _( "Errors" ) )` and false when a value is wrong. */
  const entriesOk = (): boolean => {
    if (!draft) return false;
    const errors = stackupEntryErrors(draft.stackup);
    if (errors) setDialog({ kind: "message", title: "Errors", text: errors });
    return !errors;
  };

  const onApply = () => {
    if (!draft || !entriesOk()) return;
    const problem = validateStackup(draft);
    if (problem) {
      apply.setMessage({ text: problem, bad: true });
      return;
    }
    const items = removedLayerItems(board, boardCopper, draft.copper_layers, units, layerName);
    if (items.length > 0) {
      setDialog({ kind: "removed", items, settings: draft });
      return;
    }
    send(draft, false);
  };

  /** `onMaterialChange`: the checks, the materials in use added to the lists, then the dialog for the row's kind. */
  const onMaterial = (r: StackupRow) => {
    if (!draft || !entriesOk()) return;
    addMaterialsInUse(materialLists.current, draft.stackup);
    setDialog({ kind: "material", layer: r.layer, sub: r.sub });
  };

  /**
   * `onExportToClipboard`: the checks, then `BuildStackupReport`'s text on the clipboard (`wxTheClipboard->SetData`); KiCad says nothing either way
   * (`wxLogNull`), and neither does this. `navigator.clipboard.writeText` needs the browser's clipboard permission (a secure page and a user gesture, which the click is);
   * a refusal is silent, as KiCad's failed `wxTheClipboard->Open()` is.
   */
  const onExport = () => {
    if (!draft || !entriesOk()) return;
    const report = buildStackupReport({ ...draft, stackup: withFinish(draft.stackup, finishDraftOf(rules?.stackup ?? undefined)) }, units, layerName);
    void navigator.clipboard?.writeText(report).catch(() => undefined);
  };

  const pickedChoice = (choices: DielectricChoice[], label: string) => choices.find((c) => c.label === label);

  return (
    <PageFrame title="Physical Stackup" hidden={hidden} dirty={page.dirty} busy={apply.busy} message={apply.message?.text ?? null} bad={apply.message?.bad} onApply={onApply} onRevert={() => (page.revert(), apply.setMessage(null))}>
      {!draft ? (
        <p className="bs-note">The board is not loaded.</p>
      ) : (
        <>
          <FieldRow label="Copper layers:" title="Select the number of copper layers in the stackup">
            <select aria-label="Copper layers" value={draft.copper_layers} onChange={(e) => page.edit((d) => withCopperLayerCount(d, Number(e.target.value), withCopperLayers))}>
              {COPPER_COUNTS.map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </FieldRow>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12, margin: "4px 0" }}>
            <Check checked={draft.stackup.dielectric_constraints ?? false} onChange={(v) => setStackup({ dielectric_constraints: v })} title={"If Impedance Controlled option is set,\nLoss tangent and EpsilonR will be added to constraints."}>
              Impedance controlled
            </Check>
            <button type="button" onClick={() => setDialog({ kind: "add" })}>
              Add Dielectric Layer...
            </button>
            <button type="button" disabled={!canRemoveDielectric(draft.stackup)} onClick={() => setDialog({ kind: "remove" })}>
              Remove Dielectric Layer...
            </button>
          </div>

          <div className="bs-scroll-x">
            <table className="bs-grid">
              <thead>
                <tr>
                  <th>Layer</th>
                  <th>Type</th>
                  <th>Material</th>
                  <th>Thickness (mm)</th>
                  <th>
                    <LockIcon />
                  </th>
                  <th>Color</th>
                  <th>Epsilon R</th>
                  <th>Loss Tan</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const l = draft.stackup.layers[r.layer]!;
                  const sub = sublayerOf(l, r.sub);
                  const id = r.label;
                  const t = sub.thickness_mm;
                  const badThickness = t !== null && t !== undefined && (!Number.isFinite(t) || t < 0);
                  const colorType = colorListType(l.name);
                  const prms = kicadPrms(l, r.sub);
                  return (
                    <tr key={`${r.layer}:${r.sub}`} data-row={id}>
                      <td>
                        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                          {/* `m_Icon`: `getColorIconItem`, `m_colorIconsSize` 24 x 14, drawn half as high */}
                          <span aria-hidden data-icon-color={iconColorCss(l, r.sub)} style={{ display: "inline-block", width: 24, height: 7, background: iconColorCss(l, r.sub), flex: "none" }} />
                          {r.label}
                        </span>
                      </td>
                      <td>
                        {r.typeLabel === "choice" ? (
                          <select aria-label={`${id} type`} value={corePrepregOf(l.kind)} onChange={(e) => setKind(r, kindOfChoice(e.target.value))}>
                            {CORE_PREPREG.map((c) => (
                              <option key={c} value={c}>
                                {c}
                              </option>
                            ))}
                          </select>
                        ) : (
                          r.typeLabel
                        )}
                      </td>
                      <td>
                        {itemMaterialEditable(l.name) && (
                          <span style={{ display: "inline-flex", alignItems: "center" }}>
                            <input
                              type="text"
                              aria-label={`${id} material`}
                              value={sub.material ?? (prmSpecified(prms.material) ? prms.material : NOT_SPECIFIED)}
                              onChange={(e) => editRow(r, { material: e.target.value })}
                              style={{ width: 110 }}
                            />
                            <button type="button" aria-label={`${id} material list`} title="..." onClick={() => onMaterial(r)} style={{ padding: "0 4px" }}>
                              ...
                            </button>
                          </span>
                        )}
                      </td>
                      <td>
                        {itemThicknessEditable(l.name) && (r.sub > 0 || (t !== null && t !== undefined)) ? (
                          <NumberField ariaLabel={`${id} thickness`} width={72} value={t ?? 0} invalid={badThickness} onChange={(v) => editRow(r, { thickness_mm: v ?? Number.NaN })} />
                        ) : (
                          (t ?? "")
                        )}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        {r.type === "dielectric" && (
                          <input type="checkbox" aria-label={`${id} thickness locked`} checked={sub.thickness_locked === true} onChange={(e) => editRow(r, { thickness_locked: e.target.checked ? true : undefined })} />
                        )}
                      </td>
                      <td>{colorType && <StackupColor type={colorType} value={prms.color} ariaLabel={`${id} color`} onChange={(color) => editRow(r, { color })} />}</td>
                      <td>
                        {itemHasEpsilonR(l.name) && <PrmField ariaLabel={`${id} epsilon r`} value={prms.epsilonR} onChange={(v) => editRow(r, { epsilon_r: v })} />}
                      </td>
                      <td>
                        {itemHasEpsilonR(l.name) && <PrmField ariaLabel={`${id} loss tangent`} value={prms.lossTangent} onChange={(v) => editRow(r, { loss_tangent: v })} />}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
          <div style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
            <div className="bs-stack-total" title="BuildBoardThicknessFromStackup: every layer's thickness, every sublayer's, added up">
              Board thickness from stackup: <span aria-label="Board thickness from stackup">{mmText(stackupTotalThicknessNm(draft.stackup))}</span> mm
            </div>
            <button
              type="button"
              onClick={() => {
                const p = adjustThicknessPrompt(draft.stackup, units);
                setDialog({ kind: "adjust", prompt: p.prompt, title: p.title });
              }}
            >
              Adjust Dielectric Thickness
            </button>
            <button type="button" onClick={onExport}>
              Export to Clipboard
            </button>
          </div>

          {dialog?.kind === "add" && (
            <PickDialog
              title="Add Dielectric Layer"
              header="Layers"
              label="Select layer to add:"
              items={addDielectricChoices(draft.stackup).map((c) => c.label)}
              onCancel={() => setDialog(null)}
              onOk={(label) => {
                const c = pickedChoice(addDielectricChoices(draft.stackup), label);
                if (c) page.edit((d) => ({ ...d, stackup: addDielectricSublayer(d.stackup, c.layer, c.sub) }));
                setDialog(null);
              }}
            />
          )}
          {dialog?.kind === "remove" && (
            <PickDialog
              title="Remove Dielectric Layer"
              header="Layers"
              label="Select layer to remove:"
              items={removeDielectricChoices(draft.stackup).map((c) => c.label)}
              onCancel={() => setDialog(null)}
              onOk={(label) => {
                const c = pickedChoice(removeDielectricChoices(draft.stackup), label);
                if (c) page.edit((d) => ({ ...d, stackup: removeDielectricSublayer(d.stackup, c.layer, c.sub) }));
                setDialog(null);
              }}
            />
          )}
          {dialog?.kind === "adjust" && (
            <NameDialog
              title={dialog.title}
              label={dialog.prompt}
              onCancel={() => setDialog(null)}
              onOk={(text) => {
                // `m_frame->ValueFromString( dlg.GetValue() )`: a bare number is in the user's units; text that is no length is 0
                const value = parseLength(text, units) ?? 0;
                const r = adjustDielectricThickness(draft.stackup, draft.copper_layers, value, units);
                if (r.stackup) page.edit((d) => ({ ...d, stackup: r.stackup! }));
                setDialog(r.message ? { kind: "message", title: "Message", text: r.message } : null);
              }}
            />
          )}
          {dialog?.kind === "material" &&
            (() => {
              const l = draft.stackup.layers[dialog.layer];
              const type = l ? materialListType(l.name) : null;
              if (!type) return null;
              return (
                <MaterialDialog
                  list={materialLists.current[type]}
                  onDelete={(idx) => {
                    materialLists.current[type].splice(idx, 1);
                    setListsVersion((v) => v + 1);
                  }}
                  onCancel={() => setDialog(null)}
                  onOk={(substrate) => {
                    setDialog(null);
                    page.edit((d) => ({ ...d, stackup: applySubstrate(d.stackup, dialog.layer, dialog.sub, substrate) }));
                  }}
                />
              );
            })()}
          {dialog?.kind === "message" && <MessageDialog title={dialog.title} message={dialog.text} onOk={() => setDialog(null)} onCancel={() => setDialog(null)} />}
          {dialog?.kind === "removed" && (
            <ItemsListDialog
              title={REMOVED_LAYERS_TITLE}
              message={REMOVED_LAYERS_MESSAGE}
              details={REMOVED_LAYERS_DETAILS}
              items={dialog.items.map((i) => i.text)}
              onSelect={(index) => {
                // `SetSelectionCallback`: `ACTIONS::selectionClear`, then `ACTIONS::selectItem` of the item, the item as it is (no group in its place)
                const item = dialog.items[index];
                if (item) dispatch({ type: "SET_SELECTION", refs: [item.id], raw: true });
              }}
              onCancel={() => setDialog(null)}
              onOk={() => {
                const settings = dialog.settings;
                setDialog(null);
                send(settings, true);
              }}
            />
          )}
        </>
      )}
    </PageFrame>
  );
}
