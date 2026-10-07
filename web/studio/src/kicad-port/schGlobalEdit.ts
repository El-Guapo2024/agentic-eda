// Edit Text & Graphics Properties -- `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::processItem` (eeschema/dialogs/dialog_global_edit_text_and_graphics.cpp at
// 8303b2ad): set the chosen properties on every text, text box, shape, rule area or graphic line of the sheet (or just the selected ones); a property
// left "unchanged" is not touched.
//
// KiCad's dialog also edits fields, labels, junctions and wires (sizes, orientation, colours, visibility); the studio stores none of those per item, so
// only what it does store is here: text size, bold, italic and alignment of text boxes, text size of free texts, line width and style, fill.
import type { Cmd, Schematic } from "../api/types";
import type { SchFill, SchGraphic, SchGraphicShape, SchHAlign, SchLineStyle, SchVAlign } from "../api/schEditTypes";

/** `null` = "<indeterminate>": leave the property as it is. */
export interface GlobalEditSpec {
  /** `m_selectedFilterOpt`: only the selected items. */
  selectedOnly: boolean;
  /** Which kinds of item the edit reaches. */
  kinds: { texts: boolean; textBoxes: boolean; shapes: boolean; ruleAreas: boolean; lines: boolean };
  textSize: number | null;
  bold: boolean | null;
  italic: boolean | null;
  hAlign: SchHAlign | null;
  vAlign: SchVAlign | null;
  lineWidth: number | null;
  lineStyle: SchLineStyle | null;
  fill: SchFill | null;
}

export const emptyEdit = (): GlobalEditSpec => ({
  selectedOnly: false,
  kinds: { texts: true, textBoxes: true, shapes: true, ruleAreas: true, lines: true },
  textSize: null,
  bold: null,
  italic: null,
  hAlign: null,
  vAlign: null,
  lineWidth: null,
  lineStyle: null,
  fill: null,
});

const FILLABLE: ReadonlySet<SchGraphicShape["type"]> = new Set(["rectangle", "circle", "polygon", "text_box", "rule_area"]);

/** The graphic with the spec applied, or null when nothing about it would change. */
export function editedGraphic(g: SchGraphic, spec: GlobalEditSpec): SchGraphic | null {
  const kind = g.shape.type;
  const reached = kind === "text_box" ? spec.kinds.textBoxes : kind === "rule_area" ? spec.kinds.ruleAreas : kind === "directive" ? false : spec.kinds.shapes;
  if (!reached) return null;
  const next: SchGraphic = { ...g, shape: { ...g.shape } as SchGraphicShape };
  if (next.shape.type === "text_box") {
    if (spec.textSize !== null) next.shape.size_um = spec.textSize;
    if (spec.bold !== null) next.shape.bold = spec.bold;
    if (spec.italic !== null) next.shape.italic = spec.italic;
    if (spec.hAlign !== null) next.shape.h_align = spec.hAlign;
    if (spec.vAlign !== null) next.shape.v_align = spec.vAlign;
  }
  if (spec.lineWidth !== null) next.width_um = spec.lineWidth;
  if (spec.lineStyle !== null) next.line_style = spec.lineStyle;
  if (spec.fill !== null && FILLABLE.has(kind)) next.fill = spec.fill;
  return JSON.stringify(next) === JSON.stringify(g) ? null : next;
}

/** `processItem` over the sheet: the verbs that apply the spec, one undo step. */
export function planGlobalEdit(sch: Schematic, selected: ReadonlySet<string>, spec: GlobalEditSpec): Cmd[] {
  const wanted = (id: string) => !spec.selectedOnly || selected.has(id);
  const cmds: Cmd[] = [];
  if (spec.kinds.texts && spec.textSize !== null) {
    for (const t of sch.texts) {
      if (!wanted(t.id) || t.size_um === spec.textSize) continue;
      // A text's size cannot be edited in place: it is replaced by one of the new size at the same spot (its id follows its content and place, so it keeps it).
      cmds.push({ op: "delete_sch_text", id: t.id }, { op: "add_sch_text", content: t.content, at: { x: t.at[0], y: t.at[1] }, angle_millideg: Math.round(t.angle * 1000), size_um: spec.textSize });
    }
  }
  for (const g of sch.graphics ?? []) {
    if (!wanted(g.id)) continue;
    const next = editedGraphic(g, spec);
    if (next) cmds.push({ op: "sch_edit", verb: "edit_graphic", id: g.id, graphic: next });
  }
  if (spec.kinds.lines && spec.lineWidth !== null) {
    for (const l of sch.lines ?? []) {
      if (!wanted(l.id) || l.width_um === spec.lineWidth) continue;
      cmds.push({ op: "delete_sch_line", id: l.id }, { op: "add_sch_line", pts: l.pts.map(([x, y]) => ({ x, y })), width_um: spec.lineWidth });
    }
  }
  return cmds;
}
