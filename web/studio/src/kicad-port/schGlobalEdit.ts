// Edit Text & Graphics Properties -- `DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS` (eeschema/dialogs/dialog_global_edit_text_and_graphics.cpp and
// dialog_global_edit_text_and_graphics_base.cpp at 8303b2ad).
//
// Since UI-LEFT-2 SS14 the dialog sends one verb, `global_edit_text_graphics` (crates/ops/src/sch_global_edit.rs), which walks every sheet of the hierarchy
// (`TransferDataFromWindow`'s `for( const SCH_SHEET_PATH& sheetPath : Schematic().Hierarchy() )`) with KiCad's `visitItem` / `processItem`: the Scope, Filter
// Items and Set To boxes as the dialog holds them become the verb's spec here ([`globalEditSpec`]); the rules of the boxes (the orientation's cast, the three
// state boxes, the text size check, the filters kept for the session) are below.
//
// INACTIVE (kept, not called since UI-LEFT-2 SS14): `planGlobalEdit`, `editedGraphic`, `emptyEdit` and their types, the first port, which planned the edits of
// the sheet in view only as a batch of the per-item verbs (UI-LEFT S16). The backend verb replaced it: it reaches every sheet, filters by reference, library
// id, type and net, and sets fonts, colours and a field's alignment.
import type { Cmd, Schematic } from "../api/types";
import type { SchFill, SchGraphic, SchGraphicShape, SchHAlign, SchLineStyle, SchVAlign } from "../api/schEditTypes";
import { stringFromValue } from "./fpFields";

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

// ============================================================================================================== the dialog of UI-LEFT-2 SS14

/** `dialog_global_edit_text_and_graphics_base.cpp`'s Scope box, in its order, with the verb's names. Every box starts unticked (the base sets none). */
export const SCOPE_BOXES = [
  ["references", "Reference designators"],
  ["values", "Values"],
  ["other_fields", "Other symbol fields"],
  ["wires", "Wires & wire labels"],
  ["buses", "Buses & bus labels"],
  ["global_labels", "Global labels"],
  ["hier_labels", "Hierarchical labels"],
  ["label_fields", "Label fields"],
  ["sheet_titles", "Sheet titles"],
  ["sheet_fields", "Other sheet fields"],
  ["sheet_pins", "Sheet pins"],
  ["sheet_borders", "Sheet borders & backgrounds"],
  ["sch_text_graphics", "Schematic text & graphics"],
] as const;

export type ScopeKey = (typeof SCOPE_BOXES)[number][0];

/** A three-state box (`wxCHK_3STATE | wxCHK_ALLOW_3RD_STATE_FOR_USER`): null is undetermined, "leave unchanged". */
export type Tri = boolean | null;

/** A click on a three-state box: wx cycles unchecked, checked, undetermined. */
export const nextTri = (t: Tri): Tri => (t === false ? true : t === true ? null : false);

/** `INDETERMINATE_ACTION`, the "-- leave unchanged --" entry of every choice. */
export const LEAVE_UNCHANGED = "-- leave unchanged --";

/** `m_orientationChoices`: "Right", "Up", "Left", "Down". */
export const ORIENTATION_CHOICES = ["Right", "Up", "Left", "Down"] as const;

/**
 * `sch_label->SetSpinStyle( (SPIN_STYLE::SPIN) m_orientation->GetSelection() )`: the choice's index is cast to `SPIN_STYLE::SPIN`, whose order is LEFT, UP,
 * RIGHT, BOTTOM (sch_label.h) -- so "Right" gives `SPIN_STYLE::LEFT` (horizontal, right-justified: the text ends at the anchor) and "Left" gives
 * `SPIN_STYLE::RIGHT`, as KiCad 8303b2ad and 25ec2335 do. The studio's spins are named as `SPIN_STYLE`'s.
 */
export const ORIENTATION_SPIN: readonly LabelSpinName[] = ["left", "up", "right", "bottom"];

/** `m_hAlignChoices` / `m_vAlignChoices`: `MapHorizJustify( GetSelection() - 1 )`. */
export const H_ALIGN_CHOICES: ReadonlyArray<[SchHAlign, string]> = [
  ["left", "Left"],
  ["center", "Center"],
  ["right", "Right"],
];
export const V_ALIGN_CHOICES: ReadonlyArray<[SchVAlign, string]> = [
  ["top", "Top"],
  ["center", "Center"],
  ["bottom", "Bottom"],
];

/** `m_lineStyleChoices`, `(LINE_STYLE) m_lineStyle->GetSelection()`: SOLID is 0. */
export const LINE_STYLE_CHOICES: ReadonlyArray<[SchLineStyle, string]> = [
  ["solid", "Solid"],
  ["dash", "Dashed"],
  ["dot", "Dotted"],
  ["dash_dot", "Dash-Dot"],
  ["dash_dot_dot", "Dash-Dot-Dot"],
];

/** `m_typeFilterChoices`. */
export const TYPE_FILTER_CHOICES: ReadonlyArray<["non_power" | "power", string]> = [
  ["non_power", "Non-power symbols"],
  ["power", "Power symbols"],
];

/** The four filter texts KiCad keeps for the session (`g_fieldnameFilter`, `g_referenceFilter`, `g_symbolFilter`, `g_netFilter`: set by the destructor, read back by
 *  `TransferDataToWindow`). */
export const rememberedFilters = { fieldName: "", reference: "", symbol: "", net: "" };

/** `TransferDataToWindow`'s net filter: the one kept (`ChangeValue`, no event: its box stays as it is), else the highlighted net (`if( g_netFilter.IsEmpty() &&
 *  !GetHighlightedConnection().IsEmpty() ) m_netFilter->SetValue( .. )`, whose text event ticks the box: `OnNetFilterText`). */
export function initialNetFilter(kept: string, highlighted: string | null | undefined): { text: string; ticked: boolean } {
  if (kept === "" && highlighted) return { text: highlighted, ticked: true };
  return { text: kept, ticked: false };
}

/** The dialog as it stands: the boxes, the filters (each with its tick) and the Set To values (null, "" or -1 for "-- leave unchanged --"; lengths as typed, mm). */
export interface GlobalEditForm {
  scope: Record<ScopeKey, boolean>;
  fieldNameOpt: boolean;
  fieldName: string;
  referenceOpt: boolean;
  reference: string;
  symbolOpt: boolean;
  symbol: string;
  typeOpt: boolean;
  type: "non_power" | "power";
  netOpt: boolean;
  net: string;
  selectedOnly: boolean;
  /** The font choice: "" leaves it, else "Default Font", "KiCad Font" or a family. */
  font: string;
  setTextColor: boolean;
  /** A colour as `#rrggbb`; "" is `COLOR4D::UNSPECIFIED`. */
  textColor: string;
  textSize: string;
  bold: Tri;
  italic: Tri;
  /** An index of [`ORIENTATION_CHOICES`]; -1 leaves it. */
  orientation: number;
  hAlign: SchHAlign | "";
  vAlign: SchVAlign | "";
  visible: Tri;
  showFieldNames: Tri;
  lineWidth: string;
  setLineColor: boolean;
  lineColor: string;
  lineStyle: SchLineStyle | "";
  setFillColor: boolean;
  fillColor: string;
  junctionSize: string;
  setJunctionColor: boolean;
  junctionColor: string;
}

/** `TransferDataToWindow`: nothing ticked, every Set To value "-- leave unchanged --", the filters as the session kept them. */
export function initialForm(highlightedNet?: string | null): GlobalEditForm {
  const net = initialNetFilter(rememberedFilters.net, highlightedNet);
  return {
    scope: Object.fromEntries(SCOPE_BOXES.map(([k]) => [k, false])) as Record<ScopeKey, boolean>,
    fieldNameOpt: false,
    fieldName: rememberedFilters.fieldName,
    referenceOpt: false,
    reference: rememberedFilters.reference,
    symbolOpt: false,
    symbol: rememberedFilters.symbol,
    typeOpt: false,
    type: "non_power",
    netOpt: net.ticked,
    net: net.text,
    selectedOnly: false,
    font: "",
    setTextColor: false,
    textColor: "",
    textSize: "",
    bold: null,
    italic: null,
    orientation: -1,
    hAlign: "",
    vAlign: "",
    visible: null,
    showFieldNames: null,
    lineWidth: "",
    setLineColor: false,
    lineColor: "",
    lineStyle: "",
    setFillColor: false,
    fillColor: "",
    junctionSize: "",
    setJunctionColor: false,
    junctionColor: "",
  };
}

/** A length typed in mm, nm; null for "-- leave unchanged --" (empty) and for text that is no number. */
export function lengthNm(text: string): number | null {
  const t = text.trim();
  if (t === "" || t === LEAVE_UNCHANGED) return null;
  const v = Number(t.replace(",", "."));
  return Number.isFinite(v) ? Math.round(v * 1e6) : null;
}

/** `m_textSize.Validate( 1.0, 10000.0, EDA_UNITS::MILS )` ("1 mil .. 10 inches"): "Text size must be at least 0.0254 mm." / "... less than 254 mm." (`UNIT_BINDER::Validate`, the
 *  label without its colon, the bound in the dialog's units). Null when it is fine or left unchanged. */
export function textSizeError(text: string): string | null {
  const nm = lengthNm(text);
  if (nm === null) return null;
  if (nm < 25_400) return `Text size must be at least ${stringFromValue(25_400, "mm")}.`;
  if (nm > 254_000_000) return `Text size must be less than ${stringFromValue(254_000_000, "mm")}.`;
  return null;
}

const hexColor = (hex: string): { r: number; g: number; b: number; a: number } | null => {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  return m ? { r: parseInt(m[1]!, 16), g: parseInt(m[2]!, 16), b: parseInt(m[3]!, 16), a: 255 } : null;
};

/** The verb's spec (`GlobalEditSpec`, crates/ops/src/sch_global_edit.rs) for the dialog as it stands and the ids selected on the sheet in view. A filter whose box
 *  is not ticked, or whose text is empty, is no filter. */
export function globalEditSpec(f: GlobalEditForm, selection: readonly string[]): Record<string, unknown> {
  const filter = (on: boolean, text: string) => (on && text.trim() !== "" ? text : undefined);
  const spec: Record<string, unknown> = { ...f.scope };
  const put = (k: string, v: unknown) => {
    if (v !== undefined && v !== null && v !== "") spec[k] = v;
  };
  put("field_name_filter", filter(f.fieldNameOpt, f.fieldName));
  put("reference_filter", filter(f.referenceOpt, f.reference));
  put("symbol_filter", filter(f.symbolOpt, f.symbol));
  if (f.typeOpt) spec.type_filter = f.type;
  put("net_filter", filter(f.netOpt, f.net));
  if (f.selectedOnly) {
    spec.selected_only = true;
    spec.selection = [...selection];
  }
  put("font", f.font);
  if (f.setTextColor) {
    spec.set_text_color = true;
    put("text_color", hexColor(f.textColor));
  }
  put("text_size_nm", lengthNm(f.textSize));
  put("bold", f.bold);
  put("italic", f.italic);
  if (f.orientation >= 0) spec.orientation = ORIENTATION_SPIN[f.orientation];
  put("h_align", f.hAlign);
  put("v_align", f.vAlign);
  put("visible", f.visible);
  put("show_field_names", f.showFieldNames);
  put("line_width_nm", lengthNm(f.lineWidth));
  if (f.setLineColor) {
    spec.set_line_color = true;
    put("line_color", hexColor(f.lineColor));
  }
  put("line_style", f.lineStyle);
  if (f.setFillColor) {
    spec.set_fill_color = true;
    put("fill_color", hexColor(f.fillColor));
  }
  put("junction_size_nm", lengthNm(f.junctionSize));
  if (f.setJunctionColor) {
    spec.set_junction_color = true;
    put("junction_color", hexColor(f.junctionColor));
  }
  return spec;
}

/** The command OK sends (`TransferDataFromWindow`), or the text size's complaint. */
export function globalEditCmd(f: GlobalEditForm, selection: readonly string[]): { cmd: Cmd } | { error: string } {
  const why = textSizeError(f.textSize);
  if (why) return { error: why };
  return { cmd: { op: "sch_edit", verb: "global_edit_text_graphics", spec: globalEditSpec(f, selection) } as unknown as Cmd };
}
