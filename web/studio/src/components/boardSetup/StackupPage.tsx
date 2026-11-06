// Board Setup > Board Stackup > Physical Stackup (+ Board Finish) -- port of pcbnew/board_stackup_manager/panel_board_stackup.cpp and panel_board_finish.cpp: the
// number of copper layers, "Impedance controlled", one row per sublayer of each layer (`lazyBuildRowUI`: a dielectric's Core / PrePreg choice on its first
// sublayer only, its thickness lock), Add and Remove Dielectric Layer with KiCad's lists, Adjust Dielectric Thickness, the checks of the "Errors" box, and the
// finish. The table is the `(stackup ..)` of the derived `.kicad_pcb`; the board thickness is what the layers add up to (`BuildBoardThicknessFromStackup`), and
// the copper layers are the board's layers. The pure parts are kicad-port/physicalStackup.ts.
//
// Fewer copper layers while something is on a layer that goes asks KiCad's question (`PANEL_SETUP_LAYERS::TransferDataFromWindow`: "Warning", "Items have been found
// on removed layers ...", "Show Details" with each item's description); OK sends `set_stackup` with `delete_items_on_removed_layers`, and the backend deletes them as
// `BOARD::RemoveAllItemsOnLayer` does, in the same step -- which one Undo brings back here, where KiCad clears its undo list.
//
// Colours: a mask, a silkscreen and a dielectric carry KiCad's colour name (or a typed `#RRGGBB`), which the 3D viewer's "Use board stackup colors" paints the board
// with (kicad-port/appearance3d.ts). Not ported: the material list ("..." beside a material, `DIALOG_DIELECTRIC_MATERIAL`) and "Export to Clipboard"
// (`BuildStackupReport`). A different copper layer count rebuilds the table from the default stackup, keeping what the layers that remain say (`withCopperLayers`),
// and shares the board's old thickness among the unlocked dielectrics (`onCopperLayersSelCount`'s `setDefaultLayerWidths( oldBoardWidth )`).
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useStudioState } from "../../state/store";
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
  canRemoveDielectric,
  corePrepregOf,
  itemHasEpsilonR,
  itemMaterialEditable,
  itemThicknessEditable,
  itemsOnRemovedLayers,
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
import { NOT_SPECIFIED, STACKUP_COLOR_NAMES, parseColor, stackupColorKind, toRgbHex } from "../../kicad-port/appearance3d";
import type { StackupSettings, StackupSublayer } from "../../api/types";
import { DialogShell } from "../pcbDialogKit";
import { MessageDialog, NameDialog, Pane, PickDialog } from "../panels/appearance/shared";
import { Check, FieldRow, NumberField, PageFrame, usePageApply, usePageDraft, type PageProps } from "./fields";

const COPPER_COUNTS = Array.from({ length: 16 }, (_, i) => (i + 1) * 2);

/** `GetStandardCopperFinishes`: the standard names (the ones a `.gbrjob` file knows); "Not specified" writes none. */
const COPPER_FINISHES = ["ENIG", "ENEPIG", "HAL SnPb", "HAL lead-free", "Hard gold", "Immersion tin", "Immersion nickel", "Immersion silver", "Immersion gold", "HT_OSP", "OSP", "None"];

const EDGE_CONNECTORS = ["No", "Yes", "Yes, bevelled"];

const USER_DEFINED = "User defined";

/** The colour cell of a stackup row (`PANEL_BOARD_STACKUP`'s colour combo): KiCad's names for the item's kind, "User defined" for a typed `#RRGGBB`. */
function StackupColor({ kind, value, ariaLabel, onChange }: { kind: "silk" | "mask" | "dielectric"; value: string | null | undefined; ariaLabel: string; onChange: (color: string | undefined) => void }) {
  const names: readonly string[] = STACKUP_COLOR_NAMES[kind];
  const custom = typeof value === "string" && value.startsWith("#");
  const selected = custom ? USER_DEFINED : (value ?? NOT_SPECIFIED);
  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
      <select
        aria-label={ariaLabel}
        value={selected}
        onChange={(e) => {
          const v = e.target.value;
          onChange(v === NOT_SPECIFIED ? undefined : v === USER_DEFINED ? (custom ? value! : "#808080") : v);
        }}
      >
        {names.map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
        {!custom && typeof value === "string" && !names.includes(value) && <option value={value}>{value}</option>}
        <option value={USER_DEFINED}>{USER_DEFINED}</option>
      </select>
      {custom && <input type="color" aria-label={`${ariaLabel} (user defined)`} value={toRgbHex(parseColor(value!) ?? { r: 0.5, g: 0.5, b: 0.5, a: 1 })} onChange={(e) => onChange(e.target.value)} style={{ width: 22, height: 18, padding: 0 }} />}
    </span>
  );
}

/** `DIALOG_ITEMS_LIST`: a message, a collapsed "Show Details" pane with the list, OK and Cancel. */
function ItemsListDialog({ title, message, details, items, onOk, onCancel }: { title: string; message: string; details: string; items: string[]; onOk: () => void; onCancel: () => void }) {
  return createPortal(
    <div className="bs-items-dialog" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
      <DialogShell title={title} width={460} onCancel={onCancel} onOk={onOk}>
        <div style={{ fontSize: 12, marginBottom: 8 }}>{message}</div>
        <Pane title={details}>
          <ul aria-label="Items on removed layers" style={{ margin: 0, paddingLeft: 16, maxHeight: 200, overflow: "auto", fontSize: 12 }}>
            {items.map((it, i) => (
              <li key={i}>{it}</li>
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
  | { kind: "removed"; items: string[]; settings: StackupSettings };

export function StackupPage({ hidden, onDirty }: PageProps) {
  const state = useStudioState();
  const board = state.board;
  const rules = board?.board_rules;
  const units = state.units;
  const boardCopper = rules?.copper_layers ?? rules?.layer_names?.length ?? board?.layers.length ?? 2;
  const source = useMemo<StackupSettings | undefined>(() => {
    if (!rules) return undefined;
    return { copper_layers: boardCopper, stackup: withKnownKinds(rules.stackup ?? defaultStackup(boardCopper, rules.board_thickness_nm ?? 1_600_000)) };
  }, [rules, boardCopper]);
  const page = usePageDraft<StackupSettings>(source);
  const apply = usePageApply();
  const [dialog, setDialog] = useState<Dialog | null>(null);
  useEffect(() => onDirty(page.dirty), [page.dirty, onDirty]);
  const draft = page.draft;
  const layerName = (file: string) => layerDisplayName(rules?.layer_setup, file);
  const rows = draft ? stackupRows(draft.stackup, layerName) : [];

  const editRow = (r: StackupRow, patch: Partial<StackupSublayer>) =>
    page.edit((d) => ({ ...d, stackup: { ...d.stackup, layers: d.stackup.layers.map((l, i) => (i === r.layer ? withSublayer(l, r.sub, patch) : l)) } }));
  const setKind = (r: StackupRow, kind: string) => page.edit((d) => ({ ...d, stackup: { ...d.stackup, layers: d.stackup.layers.map((l, i) => (i === r.layer ? { ...l, kind } : l)) } }));
  const setStackup = (patch: Partial<StackupSettings["stackup"]>) => page.edit((d) => ({ ...d, stackup: { ...d.stackup, ...patch } }));

  const send = (settings: StackupSettings, deleteItems: boolean) =>
    void apply.run(deleteItems ? { op: "set_stackup", settings, delete_items_on_removed_layers: true } : { op: "set_stackup", settings }, page.markClean);

  const onApply = () => {
    if (!draft) return;
    // `transferDataFromUIToStackup`: `wxMessageBox( error_msg, _( "Errors" ) )`
    const errors = stackupEntryErrors(draft.stackup);
    if (errors) {
      setDialog({ kind: "message", title: "Errors", text: errors });
      return;
    }
    const problem = validateStackup(draft);
    if (problem) {
      apply.setMessage({ text: problem, bad: true });
      return;
    }
    const items = itemsOnRemovedLayers(board, boardCopper, draft.copper_layers, units, layerName);
    if (items.length > 0) {
      setDialog({ kind: "removed", items, settings: draft });
      return;
    }
    send(draft, false);
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
                  const colorKind = stackupColorKind(l.kind);
                  return (
                    <tr key={`${r.layer}:${r.sub}`} data-row={id}>
                      <td>{r.label}</td>
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
                          <input
                            type="text"
                            aria-label={`${id} material`}
                            placeholder={NOT_SPECIFIED}
                            value={sub.material ?? ""}
                            onChange={(e) => editRow(r, { material: e.target.value === "" ? null : e.target.value })}
                            style={{ width: 110 }}
                          />
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
                      <td>{colorKind && <StackupColor kind={colorKind} value={sub.color} ariaLabel={`${id} color`} onChange={(color) => editRow(r, { color })} />}</td>
                      <td>
                        {itemHasEpsilonR(l.name) && (
                          <NumberField ariaLabel={`${id} epsilon r`} width={54} optional value={sub.epsilon_r ?? undefined} onChange={(v) => editRow(r, { epsilon_r: v })} />
                        )}
                      </td>
                      <td>
                        {itemHasEpsilonR(l.name) && (
                          <NumberField ariaLabel={`${id} loss tangent`} width={54} optional value={sub.loss_tangent ?? undefined} onChange={(v) => editRow(r, { loss_tangent: v })} />
                        )}
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
          </div>

          <div className="bs-section">Board Finish</div>
          <FieldRow label="Copper finish:">
            <select aria-label="Copper finish" value={draft.stackup.copper_finish ?? ""} onChange={(e) => setStackup({ copper_finish: e.target.value === "" ? undefined : e.target.value })}>
              <option value="">Not specified</option>
              {COPPER_FINISHES.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
              {draft.stackup.copper_finish && !COPPER_FINISHES.includes(draft.stackup.copper_finish) && <option value={draft.stackup.copper_finish}>{draft.stackup.copper_finish}</option>}
            </select>
          </FieldRow>
          <Check checked={draft.stackup.edge_plating ?? false} onChange={(v) => setStackup({ edge_plating: v })}>
            Plated board edge
          </Check>
          <FieldRow label="Edge card connectors:" title="Options for edge card connectors.">
            <select aria-label="Edge card connectors" value={draft.stackup.edge_connector ?? 0} onChange={(e) => setStackup({ edge_connector: Number(e.target.value) })}>
              {EDGE_CONNECTORS.map((label, i) => (
                <option key={label} value={i}>
                  {label}
                </option>
              ))}
            </select>
          </FieldRow>

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
          {dialog?.kind === "message" && <MessageDialog title={dialog.title} message={dialog.text} onOk={() => setDialog(null)} onCancel={() => setDialog(null)} />}
          {dialog?.kind === "removed" && (
            <ItemsListDialog
              title={REMOVED_LAYERS_TITLE}
              message={REMOVED_LAYERS_MESSAGE}
              details={REMOVED_LAYERS_DETAILS}
              items={dialog.items}
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
