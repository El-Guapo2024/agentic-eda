// Edit Text & Graphics Properties -- `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS::visitItem` / `processItem` (eeschema/dialogs/dialog_global_edit_text_and_graphics.cpp at
// 8303b2ad): set the chosen properties on every item of the kinds ticked (or just the selected ones, an item whose parent is selected counting); a property left
// "unchanged" is not touched.
//
//   Symbol references / values / other symbol fields   a field's text size, bold, italic, Visible and Show field name (`SCH_FIELD`), a field name filter for the others
//   Wires & wire labels, Buses & bus labels            the line's width and style (`HasLineStroke`), the labels on it (text, spin), a junction on it (its size)
//   Global labels, Hierarchical labels                 text size, bold, italic, the spin (Orientation, `SetSpinStyle`)
//   Sheet pins                                          text size, bold, italic
//   Schematic text & graphics                           free texts, text boxes, shapes, rule areas and graphic lines: text size, bold, italic, alignment, line, fill
//
// UI-LEFT S16 added the fields, labels, wires, buses, junctions and sheet pins (the verbs were there: `edit_field`, `edit_label`, `set_stroke`, `edit_sheet_pin`).
// Left: the sheet in view only (KiCad goes through every sheet of the hierarchy); colours, fonts and a field's alignment; the reference, symbol, net and power
// filters; sheet titles, other sheet fields, label fields and sheet borders.
import type { Cmd, Schematic } from "../api/types";
import type { SchFill, SchGraphic, SchGraphicShape, SchHAlign, SchLineStyle, SchVAlign } from "../api/schEditTypes";

export type LabelSpinName = "right" | "up" | "left" | "bottom";

/** `null` = "<indeterminate>": leave the property as it is. */
export interface GlobalEditSpec {
  /** `m_selectedFilterOpt`: only the selected items. */
  selectedOnly: boolean;
  /** Which kinds of item the edit reaches. The five of "Schematic text & graphics" are kept apart here; the others are KiCad's checkboxes. */
  kinds: {
    texts: boolean;
    textBoxes: boolean;
    shapes: boolean;
    ruleAreas: boolean;
    lines: boolean;
    references?: boolean;
    values?: boolean;
    otherFields?: boolean;
    wires?: boolean;
    buses?: boolean;
    globalLabels?: boolean;
    hierLabels?: boolean;
    sheetPins?: boolean;
  };
  /** `m_fieldnameFilter` (wildcards): the other symbol fields it names only. Empty: all of them. */
  fieldNameFilter?: string;
  /** `m_orientation`: a label's spin. */
  orientation?: LabelSpinName | null;
  /** `m_visible`, `m_showFieldNames`: a field's. */
  visible?: boolean | null;
  showFieldNames?: boolean | null;
  /** `m_junctionSize`: a junction's diameter, nm. */
  junctionSize?: number | null;
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
  // the dialog's checkboxes as they start (`dialog_global_edit_text_and_graphics_base.cpp`: every one ticked)
  kinds: { texts: true, textBoxes: true, shapes: true, ruleAreas: true, lines: true, references: true, values: true, otherFields: true, wires: true, buses: true, globalLabels: true, hierLabels: true, sheetPins: true },
  fieldNameFilter: "",
  orientation: null,
  visible: null,
  showFieldNames: null,
  junctionSize: null,
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
  const reached = kind === "text_box" ? spec.kinds.textBoxes : kind === "rule_area" ? spec.kinds.ruleAreas : kind === "directive" || kind === "image" ? false : spec.kinds.shapes;
  if (!reached) return null;
  const next: SchGraphic = { ...g, shape: { ...g.shape } as SchGraphicShape };
  if (next.shape.type === "text_box") {
    if (spec.textSize !== null) next.shape.size_nm = spec.textSize;
    if (spec.bold !== null) next.shape.bold = spec.bold;
    if (spec.italic !== null) next.shape.italic = spec.italic;
    if (spec.hAlign !== null) next.shape.h_align = spec.hAlign;
    if (spec.vAlign !== null) next.shape.v_align = spec.vAlign;
  }
  if (spec.lineWidth !== null) next.width_nm = spec.lineWidth;
  if (spec.lineStyle !== null) next.line_style = spec.lineStyle;
  if (spec.fill !== null && FILLABLE.has(kind)) next.fill = spec.fill;
  return JSON.stringify(next) === JSON.stringify(g) ? null : next;
}

/** `WildCompareString( pattern, text, false )`: `*` any run, `?` one character, case folded. */
export function wildMatch(pattern: string, text: string): boolean {
  const re = new RegExp(`^${pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".")}$`, "i");
  return re.test(text);
}

/** A label is on a bus when its text names one (`SCH_LABEL_LOCATE_BUS_T`: the item it connects to is on `LAYER_BUS`): a vector `D[0..7]` or a group `{..}`. */
const isBusName = (net: string): boolean => /\[[^\]]*\.\.[^\]]*\]/.test(net) || /\{[^}]*\}/.test(net);

/** `processItem` over the sheet: the verbs that apply the spec, one undo step. */
export function planGlobalEdit(sch: Schematic, selected: ReadonlySet<string>, spec: GlobalEditSpec): Cmd[] {
  const wanted = (id: string, parent?: string) => !spec.selectedOnly || selected.has(id) || (parent !== undefined && selected.has(parent));
  const cmds: Cmd[] = [];
  const k = spec.kinds;

  // symbols' fields (`SCH_SYMBOL_T`: m_references, m_values, m_otherFields with the field name filter)
  for (const sym of sch.symbols) {
    for (const f of sym.fields ?? []) {
      if (!f.id) continue;
      const take = f.name === "Reference" ? k.references : f.name === "Value" ? k.values : k.otherFields && (!spec.fieldNameFilter || wildMatch(spec.fieldNameFilter, f.name));
      if (!take || !wanted(f.id, sym.id)) continue;
      const edit: Record<string, unknown> = {};
      if (spec.textSize !== null && (f.size_nm ?? 1_270_000) !== spec.textSize) edit.size_nm = spec.textSize;
      if (spec.bold !== null && (f.bold ?? false) !== spec.bold) edit.bold = spec.bold;
      if (spec.italic !== null && (f.italic ?? false) !== spec.italic) edit.italic = spec.italic;
      if (spec.visible != null && f.visible !== spec.visible) edit.visible = spec.visible;
      if (spec.showFieldNames != null && (f.name_shown ?? false) !== spec.showFieldNames) edit.name_shown = spec.showFieldNames;
      if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "edit_field", id: f.id, ...edit } as Cmd);
    }
  }

  // labels: on a wire or a bus, global, hierarchical (`SCH_LABEL_T` .. `SCH_HIER_LABEL_T`)
  for (const l of sch.labels) {
    const onBus = isBusName(l.net);
    const take = (l.scope === "local" && (onBus ? k.buses : k.wires)) || (l.scope === "global" && (k.globalLabels || (onBus ? k.buses : k.wires))) || (l.scope === "hierarchical" && (k.hierLabels || (onBus ? k.buses : k.wires)));
    if (!take || !wanted(l.id)) continue;
    const edit: Record<string, unknown> = {};
    if (spec.textSize !== null && (l.size_nm ?? 1_270_000) !== spec.textSize) edit.size_nm = spec.textSize;
    if (spec.bold !== null && (l.bold ?? false) !== spec.bold) edit.bold = spec.bold;
    if (spec.italic !== null && (l.italic ?? false) !== spec.italic) edit.italic = spec.italic;
    if (spec.orientation != null && l.spin !== spec.orientation) edit.spin = spec.orientation;
    if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "edit_label", id: l.id, ...edit } as Cmd);
  }

  // wires and buses (`SCH_LINE_T` on LAYER_WIRE / LAYER_BUS): `HasLineStroke`
  if (spec.lineWidth !== null || spec.lineStyle !== null) {
    const strokes: string[] = [];
    for (const w of sch.wires) {
      if (!(w.bus ? k.buses : k.wires) || !wanted(w.id)) continue;
      const width = w.stroke?.width_nm ?? 0;
      const style = w.stroke?.style ?? "default";
      if ((spec.lineWidth !== null && width !== spec.lineWidth) || (spec.lineStyle !== null && style !== spec.lineStyle)) strokes.push(w.id);
    }
    if (strokes.length > 0) cmds.push({ op: "sch_edit", verb: "set_stroke", ids: strokes, ...(spec.lineWidth !== null ? { width_nm: spec.lineWidth } : {}), ...(spec.lineStyle !== null ? { style: spec.lineStyle } : {}) } as Cmd);
  }
  // junctions (`SCH_JUNCTION_T`): on a wire with Wires ticked, on a bus with Buses
  if (spec.junctionSize != null) {
    const onLine = (at: readonly [number, number], bus: boolean) => sch.wires.some((w) => w.bus === bus && w.pts.some((p, i) => i > 0 && onSegment(at, w.pts[i - 1]!, p)));
    const dots = (sch.junctions ?? []).filter((j) => wanted(j.id) && (j.look?.diameter_nm ?? 0) !== spec.junctionSize && ((k.wires && onLine(j.at, false)) || (k.buses && onLine(j.at, true)))).map((j) => j.id);
    if (dots.length > 0) cmds.push({ op: "sch_edit", verb: "set_stroke", ids: dots, diameter_nm: spec.junctionSize } as Cmd);
  }

  // sheet pins (`m_sheetPins`): their text
  if (k.sheetPins) {
    for (const sh of sch.sheets) {
      for (const pin of sh.pins) {
        if (!wanted(pin.id, sh.id)) continue;
        const look = pin as { size_nm?: number; bold?: boolean; italic?: boolean };
        const edit: Record<string, unknown> = {};
        if (spec.textSize !== null && (look.size_nm ?? 1_270_000) !== spec.textSize) edit.size_nm = spec.textSize;
        if (spec.bold !== null && (look.bold ?? false) !== spec.bold) edit.bold = spec.bold;
        if (spec.italic !== null && (look.italic ?? false) !== spec.italic) edit.italic = spec.italic;
        if (Object.keys(edit).length > 0) cmds.push({ op: "sch_edit", verb: "edit_sheet_pin", id: pin.id, ...edit } as Cmd);
      }
    }
  }

  if (spec.kinds.texts && spec.textSize !== null) {
    for (const t of sch.texts) {
      if (!wanted(t.id) || t.size_nm === spec.textSize) continue;
      // Text Properties' own verb: the text keeps its id, its place and its justification
      cmds.push({ op: "sch_edit", verb: "edit_text", id: t.id, size_nm: spec.textSize });
    }
  }
  for (const g of sch.graphics ?? []) {
    if (!wanted(g.id)) continue;
    const next = editedGraphic(g, spec);
    if (next) cmds.push({ op: "sch_edit", verb: "edit_graphic", id: g.id, graphic: next });
  }
  if (spec.kinds.lines && spec.lineWidth !== null) {
    for (const l of sch.lines ?? []) {
      if (!wanted(l.id) || l.width_nm === spec.lineWidth) continue;
      cmds.push({ op: "delete_sch_line", id: l.id }, { op: "add_sch_line", pts: l.pts.map(([x, y]) => ({ x, y })), width_nm: spec.lineWidth });
    }
  }
  return cmds;
}

/** `TestSegmentHit( p, a, b, 0 )`. */
function onSegment(p: readonly [number, number], a: readonly [number, number], b: readonly [number, number]): boolean {
  if ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) !== 0) return false;
  return Math.min(a[0], b[0]) <= p[0] && p[0] <= Math.max(a[0], b[0]) && Math.min(a[1], b[1]) <= p[1] && p[1] <= Math.max(a[1], b[1]);
}
