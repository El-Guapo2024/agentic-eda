// The Footprint Editor's drawing tools for the items `eda_model::fp_items` adds (UI-LEFT F2): what each tool makes from its clicks and the `fp_item` verb that puts it in the
// footprint. KiCad's footprint editor runs the board editor's `DRAWING_TOOL` on its one footprint (pcbnew/toolbars_footprint_editor.cpp lists the tools); ported from
// pcbnew/tools/drawing_tool.cpp at KiCad 8303b2ad:
//   DrawDimension            pcbnew.InteractiveDrawing.alignedDimension / orthogonalDimension / centerDimension / radialDimension / leader: SET_ORIGIN, SET_END, SET_HEIGHT
//                            (the aligned and orthogonal dimensions take a third click for their height; the others are done at the second), `constrainDimension`
//                            (`GetVectorSnapped45` in the 45- and 90-degree line modes unless Ctrl, always for a centre mark), the text place of a radial and a leader
//                            (`GetKnee() + textOffset`, `GetEnd() + textOffset`), each kind's constructor (pcbnew/pcb_dimension.cpp) and `setMeasurementAttributes`
//   DrawZone( ruleArea )     ZONE_CREATE_HELPER::createNewZone: the active layer, `ZONE_SETTINGS`' defaults (pcbnew/zone_settings.cpp: no tracks, vias or pads; zone fills and
//                            footprints allowed), no net
//   DrawRectangle            with `drawTextBox`: the box the layer's text defaults make (kicad-port/pcbTextBox.ts `newTextBox`)
//   PlacePoint               `POINT_PLACER`: a point of the default size on the active layer
//   DrawTable                `new PCB_TABLE( m_frame->GetModel(), bds.GetLineThickness( layer ) )` sized by the cursor (kicad-port/tables.ts `drawTableSession`), then
//                            DIALOG_TABLE_PROPERTIES; the IR table is built here as `Table::new_grid` and `table_edit::patch_table` build the board's (`TableCmd::Draw`),
//                            since `fp_item add` takes the table itself
//   DrawBarcode              `PCB_BARCODE( parent )` at the click on the active layer with the layer's text size (kicad-port/barcodeShapes.ts `newBarcode`)
//   PlaceReferenceImage      the image at the click on the active layer, at scale 1
//
// The dimension style is the footprint editor's own `BOARD_DESIGN_SETTINGS` (FOOTPRINT_EDITOR_SETTINGS, pcbnew/footprint_editor_settings.cpp "Dimension settings": automatic
// units, precision X_XXXX, no suffix, trailing zeros suppressed, text outside and aligned, arrows 50 mil, extension offset 0.5 mm) and its Text & Graphics defaults (the
// factory table, kicad-port/layerDefaults.ts), which the studio has no preferences page for.
//
// Pure: no store, no DOM. Unit-tested in fpDrawTools.test.ts.
import type { CmdDimension, CmdDimensionKind, CmdZone, PointXY, TextGraphicsDefaults, ZoneSettingsFields, RuleAreaFields } from "../api/types";
import type { CmdBarcode, CmdPoint, CmdRefImage } from "../api/pcbItemTypes";
import type { FpItemWire } from "../api/fpItemTypes";
import type { TableLineStyleName, TablePatch } from "../api/tableTypes";
import { isBackLayer, lineThicknessNm, textStyleOf } from "./layerDefaults";
import { snapVector45 } from "./polygonGeom";
import { DEFAULT_POINT_SIZE_NM } from "./pcbExtraItems";
import { newBarcode } from "./barcodeShapes";
import { SCALE_PPM } from "./refImage";

type P = readonly [number, number];
const xy = (p: P): PointXY => ({ x: Math.round(p[0]), y: Math.round(p[1]) });

// ------------------------------------------------------------------------------------------------------------------------------ verbs

/** `fp_item add`: the item a tool made, as one undo step of the footprint editor. */
export function fpAddCmd(footprint: string, item: FpItemWire) {
  return { op: "fp_item" as const, verb: "add" as const, footprint, item };
}

/** `fp_item set`: an item replaced by its Properties dialog, as one undo step. */
export function fpSetCmd(footprint: string, id: string, item: FpItemWire) {
  return { op: "fp_item" as const, verb: "set" as const, footprint, id, item };
}

// ------------------------------------------------------------------------------------------------------------------------- dimensions

export type FpDimKind = CmdDimensionKind["kind"];

/** FOOTPRINT_EDITOR_SETTINGS' "Dimension settings" (`design_settings.dimensions.*`), the footprint editor's `BOARD_DESIGN_SETTINGS::m_Dimension*`. */
export const FP_DIMENSION_DEFAULTS = {
  units: "automatic",
  precision: 4,
  units_format: "no_suffix",
  suppress_trailing_zeros: true,
  text_position: "outside",
  keep_text_aligned: true,
  /** `pcbIUScale.MilsToIU( DEFAULT_DIMENSION_ARROW_LENGTH )`: 50 mil. */
  arrow_length: 1_270_000,
  /** `pcbIUScale.mmToIU( DEFAULT_DIMENSION_EXTENSION_OFFSET )`: 0.5 mm. */
  extension_offset: 500_000,
} as const;

/**
 * The editor's dimension style a new dimension takes (`BOARD_DESIGN_SETTINGS`' `m_Dimension*` and its Text & Graphics defaults): the Footprint Editor's own
 * (`FP_DIMENSION_STYLE`), or the board's (Board Setup > Dimension Properties and Text & Graphics, kicad-port/dimensionDraw.ts `boardDimensionStyle`) -- the
 * board editor runs the same `DRAWING_TOOL::DrawDimension`.
 */
export interface DimensionEditorStyle {
  settings: {
    units: CmdDimension["units"];
    precision: number;
    units_format: CmdDimension["units_format"];
    suppress_trailing_zeros: boolean;
    text_position: CmdDimension["text_position"];
    keep_text_aligned: boolean;
    arrow_length: number;
    extension_offset: number;
  };
  /** The layers' Text & Graphics defaults (`bds.GetTextSize( layer )`, `GetTextThickness`, `GetLineThickness`); absent: KiCad's factory table. */
  textDefaults?: TextGraphicsDefaults;
}

export const FP_DIMENSION_STYLE: DimensionEditorStyle = { settings: FP_DIMENSION_DEFAULTS };

/** `PCB_DIMENSION_BASE`'s constructor: 50 mil arrows, which `PCB_DIM_ALIGNED`'s constructor turns into the extension height (`m_arrowLength * s_arrowAngle.Sin()`, 27.5 degrees). */
const CTOR_ARROW_NM = 1_270_000;
export const DIM_EXTENSION_HEIGHT_NM = Math.trunc(CTOR_ARROW_NM * Math.sin((27.5 * Math.PI) / 180));

/** A dimension the tool is drawing: its kind, the step it is at (`SET_END`, `SET_HEIGHT`) and the dimension as the preview has it. */
export interface FpDimDraw {
  kind: FpDimKind;
  step: "end" | "height";
  dim: CmdDimension;
}

/**
 * `DrawDimension`'s SET_ORIGIN click: the dimension of the tool's kind on `layer`, its start and end at the cursor. The text and the line take the layer's defaults
 * (`SetTextSize( bds.GetTextSize( layer ) )`, `SetTextThickness`, `SetLineThickness`), the arrows and the extension offset the editor's; the aligned, orthogonal and
 * radial kinds take the editor's units and text settings (`setMeasurementAttributes`), the centre mark and the leader keep their constructors' (a leader's text is set by its
 * dialog: `m_overrideTextEnabled = true`, no suffix, not aligned; a centre mark has no text).
 */
export function fpNewDimension(kind: FpDimKind, layer: string, at: P, editor: DimensionEditorStyle = FP_DIMENSION_STYLE): CmdDimension {
  const style = textStyleOf(editor.textDefaults, layer);
  const d = editor.settings;
  const measured = kind === "aligned" || kind === "orthogonal" || kind === "radial";
  const kindObj: CmdDimensionKind =
    kind === "aligned"
      ? { kind: "aligned", height: 0 }
      : kind === "orthogonal"
        ? { kind: "orthogonal", height: 0, horizontal: true }
        : kind === "radial"
          ? { kind: "radial", leader_length: CTOR_ARROW_NM * 3 } // `m_leaderLength = m_arrowLength * 3`
          : kind === "leader"
            ? { kind: "leader" }
            : { kind: "center" };
  const dim: CmdDimension = {
    layer,
    kind: kindObj,
    start: xy(at),
    end: xy(at),
    prefix: kind === "radial" ? "R " : "",
    suffix: "",
    // `PCB_DIM_LEADER` / `PCB_DIM_CENTER`: `m_overrideTextEnabled = true` with no text yet
    override_text: kind === "leader" || kind === "center" ? "" : null,
    // `PCB_DIMENSION_BASE`: inches, not automatic (`m_units( EDA_UNITS::INCH )`, `m_autoUnits( false )`), precision X_XXXX, zeros kept
    units: measured ? d.units : "inch",
    units_format: measured ? d.units_format : "no_suffix",
    precision: d.precision,
    suppress_trailing_zeros: measured ? d.suppress_trailing_zeros : false,
    text_position: d.text_position,
    keep_text_aligned: kind === "leader" ? false : measured ? d.keep_text_aligned : true,
    text_angle: 0,
    text_size_nm: style.heightNm,
    text_thickness_nm: style.thicknessNm,
    stroke_width: lineThicknessNm(editor.textDefaults, layer),
    arrow_length: d.arrow_length,
    extension_offset: d.extension_offset,
    extension_height: DIM_EXTENSION_HEIGHT_NM,
    arrow_direction: "outward",
  };
  // `PCB_DIM_LEADER`: `dimension->SetTextPos( cursorPos )`
  if (kind === "leader") dim.text_pos = xy(at);
  return dim;
}

/** `PCB_DIM_RADIAL::GetKnee`: the end pushed out along the radius by the leader length (`radial.Resize( m_leaderLength )`). */
export function radialKnee(dim: Pick<CmdDimension, "start" | "end">, leaderLength: number): [number, number] {
  const [rx, ry] = [dim.end.x - dim.start.x, dim.end.y - dim.start.y];
  const len = Math.hypot(rx, ry);
  if (len === 0) return [dim.end.x, dim.end.y];
  return [dim.end.x + Math.round((rx * leaderLength) / len), dim.end.y + Math.round((ry * leaderLength) / len)];
}

/** `textOffset( arrowLength * 10, 0 )`, to the left when the end is left of the start. */
function textOffset(dim: CmdDimension): number {
  const off = dim.arrow_length * 10;
  return dim.end.x < dim.start.x ? -off : off;
}

/**
 * `DrawDimension`'s motion. At SET_END the end follows the cursor, held to 45 degrees (`constrainDimension`) when `constrained` (a 45- or 90-degree line mode without
 * Ctrl) and always for a centre mark; an orthogonal dimension measures the longer side ("Create a nice preview by measuring the longer dimension"); a radial dimension's
 * text sits past its knee and a leader's past its end. At SET_HEIGHT an aligned dimension's height is the cursor's distance from the end across the measured line, and an
 * orthogonal one turns horizontal or vertical as the cursor leaves its box ("Only change the orientation when we move outside the bbox") and takes the cursor's offset
 * from the start across it.
 */
export function fpDimensionMotion(draw: FpDimDraw, cursor: P, constrained: boolean): FpDimDraw {
  const dim: CmdDimension = { ...draw.dim, kind: { ...draw.dim.kind } as CmdDimensionKind, start: { ...draw.dim.start }, end: { ...draw.dim.end } };
  const c = xy(cursor);
  if (draw.step === "end") {
    dim.end = c;
    if (constrained || draw.kind === "center") {
      const s = snapVector45([dim.end.x - dim.start.x, dim.end.y - dim.start.y]);
      dim.end = { x: dim.start.x + s[0], y: dim.start.y + s[1] };
    }
    if (dim.kind.kind === "orthogonal") dim.kind.horizontal = !(Math.abs(dim.end.x - dim.start.x) < Math.abs(dim.end.y - dim.start.y));
    else if (dim.kind.kind === "radial") {
      const knee = radialKnee(dim, dim.kind.leader_length);
      dim.text_pos = { x: knee[0] + textOffset(dim), y: knee[1] };
    } else if (dim.kind.kind === "leader") dim.text_pos = { x: dim.end.x + textOffset(dim), y: dim.end.y };
    return { ...draw, dim };
  }
  if (dim.kind.kind === "aligned") {
    const angle = Math.atan2(dim.end.y - dim.start.y, dim.end.x - dim.start.x) + Math.PI / 2;
    const [dx, dy] = [c.x - dim.end.x, c.y - dim.end.y];
    dim.kind.height = Math.trunc(dx * Math.cos(angle) + dy * Math.sin(angle)); // `SetHeight( double )` into an int
  } else if (dim.kind.kind === "orthogonal") {
    // `BOX2I bbox( GetStart(), GetEnd() - GetStart() )`, not normalised: `Contains` normalises, but `GetLeft()` / `GetRight()` are the start's and the end's x (and
    // `GetTop()` / `GetBottom()` their y), so "between" holds only for a dimension drawn left to right (top to bottom); `Centre()` halves the size in integers.
    const [x0, x1] = [Math.min(dim.start.x, dim.end.x), Math.max(dim.start.x, dim.end.x)];
    const [y0, y1] = [Math.min(dim.start.y, dim.end.y), Math.max(dim.start.y, dim.end.y)];
    const inside = c.x >= x0 && c.x <= x1 && c.y >= y0 && c.y <= y1; // `BOX2I::Contains`
    let vert = !dim.kind.horizontal;
    if (!inside) {
      const [w, h] = [dim.end.x - dim.start.x, dim.end.y - dim.start.y];
      const [cx, cy] = [dim.start.x + Math.trunc(w / 2), dim.start.y + Math.trunc(h / 2)];
      if (w === 0) vert = true;
      else if (h === 0) vert = false;
      else if (c.x > dim.start.x && c.x < dim.end.x) vert = false;
      else if (c.y > dim.start.y && c.y < dim.end.y) vert = true;
      else vert = Math.abs(c.y - cy) < Math.abs(c.x - cx);
      dim.kind.horizontal = !vert;
    }
    dim.kind.height = vert ? c.x - dim.start.x : c.y - dim.start.y;
  }
  return { ...draw, dim };
}

/**
 * `DrawDimension`'s left click (a double click is one too). Without a dimension it starts one at the cursor (SET_ORIGIN). At SET_END a dimension whose start and end are
 * one point stays where it is ("Dimensions that have origin and end in the same spot are not valid"); a centre mark, a radial dimension and a leader are then done, an
 * aligned or orthogonal one goes on to its height. At SET_HEIGHT it is done. `commit` is the dimension to add ("Draw Dimension"), after which the tool starts afresh.
 */
export function fpDimensionClick(draw: FpDimDraw | null, kind: FpDimKind, layer: string, cursor: P, constrained: boolean, editor: DimensionEditorStyle = FP_DIMENSION_STYLE): { draw: FpDimDraw | null; commit: CmdDimension | null } {
  if (!draw) return { draw: { kind, step: "end", dim: fpNewDimension(kind, layer, cursor, editor) }, commit: null };
  const moved = fpDimensionMotion(draw, cursor, constrained);
  if (moved.step === "end") {
    if (moved.dim.start.x === moved.dim.end.x && moved.dim.start.y === moved.dim.end.y) return { draw: moved, commit: null };
    if (kind === "center" || kind === "radial" || kind === "leader") return { draw: null, commit: moved.dim };
    return { draw: { ...moved, step: "height" }, commit: null };
  }
  return { draw: null, commit: moved.dim };
}

/** Lines of a dimension the tool is drawing, for its preview: the crossbar and the extension lines, a centre mark's cross, a radial dimension's or a leader's leader. */
export function fpDimensionPreview(dim: CmdDimension): [[number, number], [number, number]][] {
  const s: [number, number] = [dim.start.x, dim.start.y];
  const e: [number, number] = [dim.end.x, dim.end.y];
  const k = dim.kind;
  switch (k.kind) {
    case "aligned": {
      const len = Math.hypot(e[0] - s[0], e[1] - s[1]);
      if (len === 0) return [];
      const n: [number, number] = [(-(e[1] - s[1]) / len) * k.height, ((e[0] - s[0]) / len) * k.height];
      const cs: [number, number] = [s[0] + n[0], s[1] + n[1]];
      const ce: [number, number] = [e[0] + n[0], e[1] + n[1]];
      return [[s, cs], [e, ce], [cs, ce]];
    }
    case "orthogonal": {
      const cs: [number, number] = k.horizontal ? [s[0], s[1] + k.height] : [s[0] + k.height, s[1]];
      const ce: [number, number] = k.horizontal ? [e[0], s[1] + k.height] : [s[0] + k.height, e[1]];
      return [[s, cs], [e, ce], [cs, ce]];
    }
    case "center": {
      const [dx, dy] = [e[0] - s[0], e[1] - s[1]];
      return [
        [[s[0] - dx, s[1] - dy], e],
        [[s[0] + dy, s[1] - dx], [s[0] - dy, s[1] + dx]],
      ];
    }
    case "radial": {
      const knee = radialKnee(dim, k.leader_length);
      const t: [number, number] = dim.text_pos ? [dim.text_pos.x, dim.text_pos.y] : knee;
      return [[s, e], [e, knee], [knee, t]];
    }
    case "leader": {
      const t: [number, number] = dim.text_pos ? [dim.text_pos.x, dim.text_pos.y] : e;
      return [[s, e], [e, t]];
    }
  }
}

// ------------------------------------------------------------------------------------------------------------------------- rule areas

/** `ZONE_SETTINGS::ZONE_SETTINGS()` (pcbnew/zone_settings.cpp) for a rule area: no tracks, no vias, no pads; zone fills and footprints allowed. */
export const FP_RULE_AREA_KEEPOUTS: Omit<RuleAreaFields, "is_rule_area" | "placement"> = {
  keepout_tracks: true,
  keepout_vias: true,
  keepout_pads: true,
  keepout_copper_pour: false,
  keepout_footprints: false,
};

/**
 * The rule area `DrawZone( ruleArea )` makes of the outline the tool closed (`ZONE_CREATE_HELPER::createNewZone`: `GetDefaultZoneSettings()`, `m_Layers.reset().set(
 * m_params.m_layer )`, `SetIsRuleArea( true )`): on the active layer, no net (a zone of a library footprint has none), the zone defaults `settings` and the keepouts of
 * `ZONE_SETTINGS`. It is what Rule Area Properties opens on.
 */
export function fpNewRuleArea(layer: string, outline: readonly P[], settings: ZoneSettingsFields): CmdZone {
  return { net: "", layer, outline: outline.map(xy), ...settings, is_rule_area: true, ...FP_RULE_AREA_KEEPOUTS };
}

/** The IR zone's fields a zone dialog sets, as `ZONE_SETTINGS::ExportSetting` copies them onto the zone. */
const ZONE_SETTING_KEYS = [
  "clearance", "min_thickness", "thermal_gap", "thermal_spoke_width", "pad_connection", "priority", "island_removal_mode", "min_island_area", "fill_mode",
  "hatch_thickness", "hatch_gap", "hatch_orientation_mdeg", "hatch_smoothing_level", "hatch_smoothing_value", "hatch_hole_min_area", "hatch_border_algorithm",
  "smoothing", "corner_radius", "is_rule_area", "keepout_tracks", "keepout_vias", "keepout_pads", "keepout_copper_pour", "keepout_footprints", "placement",
  // the outline display and the hatch offset overrides (`AcceptOptions`): an unchanged offsets grid sends none, and the zone keeps its own
  "border_style", "border_hatch_pitch", "hatch_offsets",
] as const;

/**
 * `ZONE_SETTINGS::ExportSetting( *zone )` onto a zone of the footprint: `zone` (as the IR has it: `fp_item set` and `add` take nothing else, the zone is
 * `deny_unknown_fields`) with the dialog's settings, its layer set (`layers[0]` is the zone's layer; a set of one layer is the plain layer) and its name (a rule area has
 * none). The net stays empty. The outline, the holes and the id are the zone's.
 */
export function fpZoneWithSettings(zone: CmdZone, settings: Partial<ZoneSettingsFields & RuleAreaFields>, layers: readonly string[], name: string): CmdZone {
  const out: Record<string, unknown> = { ...zone };
  for (const k of ZONE_SETTING_KEYS) if (settings[k] !== undefined) out[k] = settings[k];
  out.net = "";
  out.layer = layers[0] ?? zone.layer;
  if (layers.length > 1) out.layers = [...layers];
  else delete out.layers;
  if (name && !settings.is_rule_area) out.name = name;
  else delete out.name;
  if (!settings.is_rule_area) delete out.placement;
  return out as unknown as CmdZone;
}

// ------------------------------------------------------------------------------------------------------------------------------ points

/** `POINT_PLACER::CreateItem`: a point of the default size on the active layer, at the click. */
export function fpNewPoint(layer: string, at: P): CmdPoint {
  return { layer, at: xy(at), size_nm: DEFAULT_POINT_SIZE_NM };
}

// ---------------------------------------------------------------------------------------------------------------------------- barcodes

/** `DrawBarcode`'s click: `PCB_BARCODE( parent )` at the cursor on the active layer, the layer's text size (`bds.GetTextSize( layer ).y`). */
export function fpNewBarcode(layer: string, at: P): CmdBarcode {
  return newBarcode(layer, xy(at), textStyleOf(undefined, layer).heightNm);
}

// ------------------------------------------------------------------------------------------------------------------------------ images

/** `PlaceReferenceImage`'s click: the image the cursor holds, its centre at the click, on the active layer, at scale 1. */
export function fpNewImage(layer: string, at: P, data: string): CmdRefImage {
  return { layer, at: xy(at), scale_ppm: SCALE_PPM, data };
}

// ------------------------------------------------------------------------------------------------------------------------------ tables

/** `eda_model::table::TableStroke` as the IR has it. */
export interface FpTableStroke {
  width_nm: number;
  style?: TableLineStyleName;
  color?: unknown;
}

/** `eda_model::table::TableCell` as the IR has it (the fields this module sets; the others ride along). */
export interface FpTableCell {
  uuid?: string;
  text: string;
  col_span?: number;
  row_span?: number;
  margins: [number, number, number, number];
  style: { height_nm: number; width_nm: number; h_align: "left" | "center" | "right"; v_align: "top" | "center" | "bottom"; mirror?: boolean; [k: string]: unknown };
  [k: string]: unknown;
}

/** `eda_model::table::Table` as the IR has it. */
export interface FpTable {
  id?: string;
  uuid?: string;
  layer: string;
  at: PointXY;
  angle?: number;
  col_widths: number[];
  row_heights: number[];
  cells: FpTableCell[];
  border: { external: boolean; header: boolean; stroke: FpTableStroke };
  separators: { rows: boolean; cols: boolean; stroke: FpTableStroke };
}

/** What Draw Table has sized when its second click lands (state/tableDialogs.ts `DrawnTable`). */
export interface FpDrawnTable {
  layer: string;
  at: readonly [number, number];
  cols: number;
  rows: number;
  cellW: number;
  cellH: number;
  lineWidth: number;
}

/** `DEFAULT_SIZE_TEXT` (50 mil), what a new `PCB_TABLECELL`'s text is. */
const CELL_TEXT_NM = 1_270_000;
/** `PCB_TEXTBOX::GetLegacyTextMargin` of a new cell (a 0.1 mm stroke): `KiROUND( 0.1 mm / 2.0 ) + KiROUND( 1.27 mm * 0.75 )`. */
const CELL_MARGIN_NM = Math.round(100_000 / 2) + Math.round(CELL_TEXT_NM * 0.75);

/** `new PCB_TABLECELL( table )` (`TableCell::pcb_new`): an empty text box of KiCad's defaults, left and centred, mirrored when the table is on a back layer. */
function newCell(back: boolean): FpTableCell {
  return { text: "", margins: [CELL_MARGIN_NM, CELL_MARGIN_NM, CELL_MARGIN_NM, CELL_MARGIN_NM], style: { height_nm: CELL_TEXT_NM, width_nm: CELL_TEXT_NM, h_align: "left", v_align: "center", ...(back ? { mirror: true } : {}) } };
}

/** `Table::new_grid` (crates/model/src/table.rs), the board's `TableCmd::Draw`: `cols` x `rows` empty cells, the border and the separators `lineWidth` wide in the default style. */
export function fpNewTable(drawn: FpDrawnTable): FpTable {
  const cols = Math.max(1, drawn.cols);
  const rows = Math.max(1, drawn.rows);
  const back = isBackLayer(drawn.layer);
  const stroke = (): FpTableStroke => ({ width_nm: Math.round(drawn.lineWidth), style: "default" });
  return {
    layer: drawn.layer,
    at: xy(drawn.at),
    angle: 0,
    col_widths: Array.from({ length: cols }, () => Math.round(drawn.cellW)),
    row_heights: Array.from({ length: rows }, () => Math.round(drawn.cellH)),
    cells: Array.from({ length: cols * rows }, () => newCell(back)),
    border: { external: true, header: true, stroke: stroke() },
    separators: { rows: true, cols: true, stroke: stroke() },
  };
}

const solid = (s: TableLineStyleName | undefined): TableLineStyleName => (!s || s === "default" ? "solid" : s);
const covered = (c: FpTableCell): boolean => c.col_span === 0 || c.row_span === 0;

/**
 * `table_edit::patch_table` (`DIALOG_TABLE_PROPERTIES::TransferDataFromWindow` for the table's own parts): the cells' text (a cell another covers has none), the border and the
 * separators (a half with neither switch on gets the "no stroke" width -1, a `default` style is written solid). A copy; refused (null) for a grid that is not the table's size.
 */
export function fpPatchTable(t: FpTable, p: TablePatch): FpTable | null {
  const out: FpTable = JSON.parse(JSON.stringify(t));
  if (p.at) out.at = { ...p.at };
  if (p.texts) {
    const cols = out.col_widths.length;
    if (p.texts.length !== out.row_heights.length || p.texts.some((r) => r.length !== cols)) return null;
    p.texts.forEach((line, row) =>
      line.forEach((text, col) => {
        const cell = out.cells[row * cols + col];
        if (cell) cell.text = covered(cell) ? "" : text;
      })
    );
  }
  if (p.border) {
    const b = p.border;
    out.border.external = b.external;
    out.border.header = b.header;
    out.border.stroke = { ...out.border.stroke, width_nm: b.external || b.header ? Math.max(0, b.width_nm) : -1, style: solid(b.style) };
  }
  if (p.separators) {
    const s = p.separators;
    out.separators.rows = s.rows;
    out.separators.cols = s.cols;
    out.separators.stroke = { ...out.separators.stroke, width_nm: s.rows || s.cols ? Math.max(0, s.width_nm) : -1, style: solid(s.style) };
  }
  return out;
}
