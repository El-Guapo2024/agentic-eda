// Which texts, graphics and dimensions "Edit Text & Graphics Properties..." changes: the dialog's Scope box and Filter Items box
// (pcbnew/dialogs/dialog_global_edit_text_and_graphics.cpp at 8303b2ad: `TransferDataFromWindow` walks the footprints and then `Drawings()` and picks the kind each
// scope box names, `visitItem` applies the filters), and what its Action rows send (`processItem`). The change itself is `Cmd::EditTextAndGraphics` on the ids this
// module returns.
//
//     TransferDataFromWindow   for every footprint: "Reference designators" -> its Reference, "Values" -> its Value; its other fields with "Other footprint fields"
//                              (or, without it, one whose text is `${REFERENCE}` / `${VALUE}` with the Reference / Value box); its own texts with "Footprint
//                              text items" (or a `${REFERENCE}` / `${VALUE}` one with those boxes); its graphics with "Footprint graphic items"
//                              then the board: "PCB text items" -> texts, text boxes (`PCB_TEXT_T || PCB_TEXTBOX_T`) and every table's cells (`RunOnChildren`),
//                              "PCB graphic items" -> shapes and barcodes (`PCB_SHAPE_T || PCB_BARCODE_T`), "PCB dimensions" -> dimensions
//     visitItem                selected items only (the item, else its footprint, else a group above that, selected) -> by layer (`GetLayer()`) -> by parent
//                              reference designator and by parent footprint library link (`WildCompareString`, case-blind, on an item that has a parent footprint)
//
// A footprint's own texts are the ones the board draws (`Part.texts`: a hidden one is not sent, so it is not reached), its graphics `Part.graphics` (ids
// `REF~g<k>`, the k-th of that list). The footprints' dimensions, text boxes, tables and barcodes are kept as the text of their forms (GAPS.md item 25), so
// "Footprint dimensions" has nothing to pick here.
//
// Pure: no store, no DOM. Unit-tested in globalEditTextGraphics.test.ts.
import type { BoardState, GlobalEditTextRows, Part, TextGraphicsDefaults } from "../api/types";
import { enabledOptionalLayers } from "./boardLayers";
import { ancestors } from "./groupTree";
import { wildCompare } from "./libeval";
import { cellItemId } from "./tables";

/** The Scope box, in the base dialog's order (two columns: the footprints' items on the left, the board's on the right). */
export interface TextGraphicsScope {
  /** "Reference designators". */
  references: boolean;
  /** "Values". */
  values: boolean;
  /** "Other footprint fields": "Footprint fields that are not the reference designator or value field". */
  otherFields: boolean;
  /** "Footprint graphic items". */
  footprintGraphics: boolean;
  /** "Footprint text items": "Footprint text items not associated with a field". */
  footprintTexts: boolean;
  /** "Footprint dimensions". */
  footprintDimensions: boolean;
  /** "PCB graphic items". */
  graphics: boolean;
  /** "PCB text items". */
  text: boolean;
  /** "PCB dimensions". */
  dimensions: boolean;
}

export const NOTHING_IN_TEXT_SCOPE: TextGraphicsScope = {
  references: false,
  values: false,
  otherFields: false,
  footprintGraphics: false,
  footprintTexts: false,
  footprintDimensions: false,
  graphics: false,
  text: false,
  dimensions: false,
};

/** The Filter Items box. */
export interface TextGraphicsFilter {
  /** "By layer:". */
  layerOn: boolean;
  layer: string;
  /** "By parent reference designator:" (`m_referenceFilterOpt`, `m_referenceFilter`). */
  referenceOn: boolean;
  reference: string;
  /** "By parent footprint library link:". */
  footprintOn: boolean;
  footprint: string;
  /** "Selected items only". */
  selectedOnly: boolean;
}

/** The ids the dialog changes, by the list of `Cmd::EditTextAndGraphics` each goes in. */
export interface TextGraphicsMatch {
  shapeIds: string[];
  textIds: string[];
  dimensionIds: string[];
  /** Footprint fields, `REF:Name`. */
  fieldIds: string[];
  /** A footprint's own texts, `REF~t<k>`. */
  fpTextIds: string[];
  /** A footprint's graphics, `REF~g<k>`. */
  fpShapeIds: string[];
}

/** The id of the k-th of a footprint's graphics (`Part.graphics`), as `Cmd::EditTextAndGraphics` names it (crates/ops/src/pcb_global_edit.rs `fp_shape_id`). */
export const fpShapeId = (ref: string, k: number): string => `${ref}~g${k}`;

function placedParts(board: BoardState | null): Part[] {
  return (board?.parts ?? []).filter((p) => p.placed);
}

/** Every layer a text, a graphic item or a dimension can be on: the board's copper, then the technical and user layers it has (Board Editor Layers, in the order of the layer selector `PCB_LAYER_BOX_SELECTOR`), then any other layer an item of the board or of a footprint is on. */
export function textGraphicsLayers(board: BoardState | null): string[] {
  const out = [...(board?.layers ?? []), ...enabledOptionalLayers(board?.board_rules?.layer_setup)];
  const known = new Set(out);
  const dr = board?.drawings;
  const fp = placedParts(board).flatMap((p) => [...(p.fields ?? []).map((f) => f.layer), ...(p.texts ?? []).map((t) => t.layer), ...(p.graphics ?? []).map((g) => g.layer)]);
  for (const layer of [...(dr?.shapes ?? []).map((s) => s.layer), ...(dr?.texts ?? []).map((t) => t.layer), ...(dr?.dimensions ?? []).map((d) => d.layer), ...fp]) {
    if (layer && !known.has(layer)) {
      known.add(layer);
      out.push(layer);
    }
  }
  return out;
}

/**
 * The dialog as it opens: nothing in scope, every filter off, the layer list on the active layer (else the first layer), the two parent filters on what they
 * held when the dialog was last closed (`g_referenceFilter`, `g_footprintFilter`: "SetValue() generates events, ChangeValue() does not", so their boxes stay off).
 */
export function initialTextGraphicsFilter(board: BoardState | null, activeLayer: string | null, remembered: { reference: string; footprint: string } = { reference: "", footprint: "" }): TextGraphicsFilter {
  const layers = textGraphicsLayers(board);
  return {
    layerOn: false,
    layer: activeLayer != null && layers.includes(activeLayer) ? activeLayer : (layers[0] ?? ""),
    referenceOn: false,
    reference: remembered.reference,
    footprintOn: false,
    footprint: remembered.footprint,
    selectedOnly: false,
  };
}

/** The ids of the items the dialog would change: the footprints' (in board order, each footprint's fields, then its texts, then its graphics), then the board's. */
export function matchingTextAndGraphics(board: BoardState | null, scope: TextGraphicsScope, filter: TextGraphicsFilter, selection: ReadonlySet<string>): TextGraphicsMatch {
  const out: TextGraphicsMatch = { shapeIds: [], textIds: [], dimensionIds: [], fieldIds: [], fpTextIds: [], fpShapeIds: [] };
  if (!board) return out;
  const dr = board.drawings;
  const groups = dr?.groups ?? [];
  // `aItem->IsSelected()`, or a group above it is
  const isSelected = (id: string) => selection.has(id) || ancestors(groups, id).some((g) => selection.has(g.id));
  // `visitItem`'s filters; `parent` is the item's footprint (`GetParentFootprint()`), absent for a board item
  const passes = (id: string, layer: string, parent?: Part): boolean => {
    // the item, else its footprint, else a group above that (`candidate = candidate->GetParent()`, then `GetParentGroup()`)
    if (filter.selectedOnly && !selection.has(id) && !(parent ? isSelected(parent.ref) : isSelected(id))) return false;
    if (filter.layerOn && filter.layer !== "" && layer !== filter.layer) return false;
    if (parent && filter.referenceOn && filter.reference !== "" && !wildCompare(filter.reference, parent.ref, false)) return false;
    if (parent && filter.footprintOn && filter.footprint !== "" && !wildCompare(filter.footprint, parent.footprint ?? "", false)) return false;
    return true;
  };

  // Go through the footprints
  for (const part of placedParts(board)) {
    const fields = part.fields ?? [];
    const visitField = (id: string, layer: string) => {
      if (passes(id, layer, part)) out.fieldIds.push(id);
    };
    const reference = fields.find((f) => f.name === "Reference");
    const value = fields.find((f) => f.name === "Value");
    if (scope.references && reference) visitField(reference.id, reference.layer);
    if (scope.values && value) visitField(value.id, value.layer);
    for (const f of fields) {
      if (f.name === "Reference" || f.name === "Value") continue;
      if (scope.otherFields) visitField(f.id, f.layer);
      else if (scope.references && f.text === "${REFERENCE}") visitField(f.id, f.layer);
      else if (scope.values && f.text === "${VALUE}") visitField(f.id, f.layer);
    }
    // Go through all other footprint items
    for (const t of part.texts ?? []) {
      const wanted = scope.footprintTexts || (scope.references && t.role === "reference") || (scope.values && t.role === "value");
      if (wanted && passes(t.id, t.layer, part)) out.fpTextIds.push(t.id);
    }
    if (scope.footprintGraphics) {
      (part.graphics ?? []).forEach((g, k) => {
        const id = fpShapeId(part.ref, k);
        if (passes(id, g.layer, part)) out.fpShapeIds.push(id);
      });
    }
  }

  // Go through the PCB text & graphic items
  if (!dr) return out;
  if (scope.graphics) {
    for (const s of dr.shapes) if (passes(s.id, s.layer)) out.shapeIds.push(s.id);
    for (const b of dr.barcodes ?? []) if (passes(b.id, b.layer)) out.shapeIds.push(b.id);
  }
  if (scope.text) {
    for (const t of dr.texts) if (passes(t.id, t.layer)) out.textIds.push(t.id);
    for (const t of dr.text_boxes ?? []) if (passes(t.id, t.layer)) out.textIds.push(t.id);
    // a cell is on its table's layer; it is selected only as itself (a cell has no group: its table has)
    for (const tb of dr.tables ?? [])
      for (const c of tb.cells) {
        const id = cellItemId(tb.id, c.row, c.col);
        if ((!filter.selectedOnly || selection.has(id)) && (!filter.layerOn || filter.layer === "" || tb.layer === filter.layer)) out.textIds.push(id);
      }
  }
  if (scope.dimensions) for (const d of dr.dimensions ?? []) if (passes(d.id, d.layer)) out.dimensionIds.push(d.id);
  return out;
}

/** How many items a match names. */
export function matchCount(m: TextGraphicsMatch): number {
  return m.shapeIds.length + m.textIds.length + m.dimensionIds.length + m.fieldIds.length + m.fpTextIds.length + m.fpShapeIds.length;
}

// ------------------------------------------------------------------------------------------------------------------------------- the Action box

/** `INDETERMINATE_ACTION`: what a row shows (and a value box is empty for) while it leaves the property alone. */
export const LEAVE_UNCHANGED = "-- leave unchanged --";

/** `KICAD_FONT_NAME`. */
export const KICAD_FONT = "KiCad Font";

/** A `wxCHK_3STATE | wxCHK_ALLOW_3RD_STATE_FOR_USER` box: `null` is `wxCHK_UNDETERMINED`, which leaves the property alone. */
export type TriState = boolean | null;

/** A click on such a box: unchecked, checked, undetermined, and round again (the order wxWidgets cycles a user's 3-state box through). */
export function nextTriState(v: TriState): TriState {
  return v === false ? true : v === true ? null : false;
}

/**
 * The Font row's list (`FONT_CHOICE` with `SetHasIndeterminateChoice()` and the base's one choice, "KiCad Font": no "Default Font" here): "-- leave unchanged --",
 * "KiCad Font", then the machine's families.
 */
export function fontChoices(families: readonly string[]): string[] {
  return [LEAVE_UNCHANGED, KICAD_FONT, ...families.filter((f) => f !== KICAD_FONT && f !== LEAVE_UNCHANGED)];
}

/**
 * `onTextSize` with the auto-thickness button pressed: the pen of the smaller of the two sizes, `GetPenSizeForBold` when Bold is ticked, else `GetPenSizeForNormal`;
 * `null` while either size is "-- leave unchanged --" (the box then says "(auto)").
 */
export function autoThickness(width: number | null, height: number | null, bold: TriState): number | null {
  if (width == null || height == null) return null;
  const size = Math.min(width, height);
  return Math.round(bold === true ? size / 5 : size / 8);
}

/** `TEXT_MIN_SIZE_MM` and `TEXT_MAX_SIZE_MM`, nm. */
export const TEXT_MIN_NM = 1_000;
export const TEXT_MAX_NM = 250_000_000;

/**
 * `TransferDataFromWindow`'s `m_textWidth.Validate( minTextSize, maxTextSize )` and the height's (`UNIT_BINDER::Validate`): "%s must be at least %s." / "%s must be less
 * than %s.", the label without its colon and the bound in millimetres. Null when every given size is in range (a size left unchanged is not checked).
 */
export function textSizeMessage(width: number | null, height: number | null): string | null {
  const mm = (nm: number) => `${String(Number((nm / 1e6).toFixed(10)))} mm`;
  for (const [label, v] of [
    ["Text width", width],
    ["Text height", height],
  ] as const) {
    if (v == null) continue;
    if (!(v >= TEXT_MIN_NM)) return `${label} must be at least ${mm(TEXT_MIN_NM)}.`;
    if (v > TEXT_MAX_NM) return `${label} must be less than ${mm(TEXT_MAX_NM)}.`;
  }
  return null;
}

/** "Set to specified values:" as the dialog holds it: `null` is "-- leave unchanged --", lengths in nm. */
export interface TextGraphicsAction {
  layer: string | null;
  lineWidth: number | null;
  textWidth: number | null;
  textHeight: number | null;
  textThickness: number | null;
  autoThickness: boolean;
  font: string | null;
  bold: TriState;
  italic: TriState;
  keepUpright: TriState;
  visible: TriState;
  centerOnFootprint: boolean;
}

/** `TransferDataToWindow`: every row "-- leave unchanged --", the three-state boxes undetermined, the auto button up, Center on footprint off. */
export const UNCHANGED_ACTION: TextGraphicsAction = {
  layer: null,
  lineWidth: null,
  textWidth: null,
  textHeight: null,
  textThickness: null,
  autoThickness: false,
  font: null,
  bold: null,
  italic: null,
  keepUpright: null,
  visible: null,
  centerOnFootprint: false,
};

/** Whether "Set to specified values:" would change anything at all. */
export function actionChangesSomething(a: TextGraphicsAction): boolean {
  return (
    a.layer != null ||
    a.lineWidth != null ||
    a.textWidth != null ||
    a.textHeight != null ||
    a.textThickness != null ||
    a.autoThickness ||
    a.font != null ||
    a.bold != null ||
    a.italic != null ||
    a.keepUpright != null ||
    a.visible != null ||
    a.centerOnFootprint
  );
}

/** The command for the dialog's "Apply and Close": the match, and either "Set to layer default values:" or the rows of "Set to specified values:". */
export function textAndGraphicsCmd(m: TextGraphicsMatch, layerDefaults: boolean, a: TextGraphicsAction) {
  const footprint = { field_ids: m.fieldIds, text_ids: m.fpTextIds, shape_ids: m.fpShapeIds };
  const base = { op: "edit_text_and_graphics" as const, shape_ids: m.shapeIds, text_ids: m.textIds, dimension_ids: m.dimensionIds, footprint };
  if (layerDefaults) return { ...base, layer_defaults: true };
  const rows: GlobalEditTextRows = {};
  if (a.textWidth != null) rows.width = a.textWidth;
  if (a.textHeight != null) rows.height = a.textHeight;
  if (a.autoThickness) rows.auto_thickness = true;
  if (a.bold != null) rows.bold = a.bold;
  if (a.italic != null) rows.italic = a.italic;
  if (a.font != null) rows.font = a.font;
  if (a.keepUpright != null) rows.keep_upright = a.keepUpright;
  if (a.visible != null) rows.visible = a.visible;
  if (a.centerOnFootprint) rows.center_on_footprint = true;
  return { ...base, layer: a.layer, line_width: a.lineWidth, text_thickness: a.autoThickness ? null : a.textThickness, rows };
}

/** The layer-class grid of "Set to layer default values:" (`ROW_SILK` .. `ROW_OTHERS` x `COL_LINE_THICKNESS` .. `COL_TEXT_UPRIGHT`); null is a cell the grid leaves empty. */
export interface LayerClassRow {
  name: string;
  line: number;
  textWidth: number | null;
  textHeight: number | null;
  textThickness: number | null;
  italic: boolean | null;
  upright: boolean | null;
}

/** `TransferDataToWindow`'s grid: Silk Layers, Copper Layers, Edge Cuts, Courtyards, Fab Layers, Other Layers; the edges and courtyards have a line thickness only. */
export function layerClassRows(d: TextGraphicsDefaults | undefined): LayerClassRow[] {
  if (!d) return [];
  const full = (name: string, c: TextGraphicsDefaults["silk"]): LayerClassRow => ({ name, line: c.line_width_nm, textWidth: c.text_width_nm, textHeight: c.text_height_nm, textThickness: c.text_thickness_nm, italic: c.italic, upright: c.upright });
  const line = (name: string, w: number): LayerClassRow => ({ name, line: w, textWidth: null, textHeight: null, textThickness: null, italic: null, upright: null });
  return [full("Silk Layers", d.silk), full("Copper Layers", d.copper), line("Edge Cuts", d.edge_cuts_line_width_nm), line("Courtyards", d.courtyard_line_width_nm), full("Fab Layers", d.fab), full("Other Layers", d.others)];
}
