// The board items GAPS.md item 24 (package P16) added -- mirrors crates/model/src/pcb_items.rs (`TextBox`, ..) and crates/ops/src/pcb_items.rs (`PcbItemCmd`, sent as
// `{ op: "pcb_item", verb: ..., ... }`). `Board*` types are the display shape `GET /api/state` sends in `drawings` (crates/cli/src/pcb_items_json.rs: points are `[x, y]`
// pairs); `Cmd*` types are the IR's own shape, which a verb takes.
import type { SchLineStyle } from "./schEditTypes";
import type { PointXY, Nm } from "./types";

export type TextBoxHAlign = "left" | "center" | "right";
export type TextBoxVAlign = "top" | "center" | "bottom";

/** `PCB_TEXTBOX`: wrapped text in a rectangle (or, turned by an angle that is not a multiple of 90 degrees, in a polygon of four corners). */
export interface BoardTextBox {
  id: string;
  layer: string;
  /** The text as stored: `${VARIABLES}` unexpanded, lines separated by a line feed. */
  text: string;
  /** What `${NAME}` stands for, when that differs from `text`. */
  shown?: string;
  start: [Nm, Nm];
  end: [Nm, Nm];
  /** The four corners of a box turned by an angle that is not a multiple of 90 degrees; empty for a rectangle. */
  pts: [Nm, Nm][];
  /** The text angle in the file's sense (counter-clockwise on the screen), millidegrees, 0 to 360000. */
  angle: number;
  /** The margins, left, top, right, bottom. */
  margins: [Nm, Nm, Nm, Nm];
  border: boolean;
  stroke_width: Nm;
  line_style: SchLineStyle;
  stroke_color: string | null;
  knockout: boolean;
  /** The glyph height and width, nm. */
  size: Nm;
  width: Nm;
  /** The pen the text is drawn with, nm (the automatic one when `thickness_auto`). */
  thickness: Nm;
  thickness_auto: boolean;
  halign: TextBoxHAlign;
  valign: TextBoxVAlign;
  mirror: boolean;
  bold: boolean;
  italic: boolean;
  /** The line spacing in thousandths (1000 is KiCad's 1.0). */
  line_spacing_permille: number;
  face: string | null;
}

/** `crates/model/src/pcb_items.rs` `TextBox`, IR field names. `id` is ignored by `add_text_box`. */
export interface CmdTextBox {
  id?: string;
  layer: string;
  text: string;
  start: PointXY;
  end: PointXY;
  pts?: PointXY[];
  angle?: number;
  margin_left_nm: Nm;
  margin_top_nm: Nm;
  margin_right_nm: Nm;
  margin_bottom_nm: Nm;
  border?: boolean;
  stroke_width_nm: Nm;
  line_style?: SchLineStyle;
  stroke_color?: string | null;
  knockout?: boolean;
  size_nm: Nm;
  width_nm?: Nm | null;
  /** 0 is KiCad's automatic pen. */
  thickness_nm?: Nm;
  halign?: TextBoxHAlign;
  valign?: TextBoxVAlign;
  mirror?: boolean;
  bold?: boolean;
  italic?: boolean;
  line_spacing_permille?: number;
  face?: string | null;
}

/**
 * `PCB_REFERENCE_IMAGE`: a bitmap on a graphic layer, kept out of every plot. `x`, `y` are the image's centre, `w`, `h` its size in nm (the pixels at their definition, `ppi`, and the
 * `scale`). The pixels are not in the state: `GET /api/pcb_image?id=..&h=<hash>` serves them (the PNG or JPEG file the board embeds), and `hash` changes when they do.
 */
export interface BoardRefImage {
  id: string;
  layer: string;
  x: Nm;
  y: Nm;
  w: Nm;
  h: Nm;
  /** `(scale s)` of the file in millionths (1.0 is 1000000). */
  scale_ppm: number;
  /** The size in pixels, and the definition in pixels per inch (`BITMAP_BASE::GetPPI`, 300 when the file says none). */
  px: [number, number];
  ppi: number;
  hash: string;
}

/** `PCB_POINT`: a marker on the board (a cross with a ring), a snap anchor for the grid helper. `x`, `y` are its centre, `size` its extent in nm. */
/**
 * KiCad 10's grid item (`PCB_GRID_ITEM`, crates/model/src/grid_item.rs) as the state lists it: a sub-grid the cursor, the router and the placement tools snap to where it covers the
 * board. `extent` is the half-extent (centre to edge) and `spacing` the pitch, nm; on a polar grid `[0]` of each is the radius and the radial pitch and `phi_extent` / `phi_spacing`
 * the sweep and the angular pitch, degrees. `angle` is the orientation, degrees counter-clockwise on the screen.
 */
export interface BoardGridItem {
  id: string;
  kind: "cartesian" | "polar";
  x: Nm;
  y: Nm;
  extent: [Nm, Nm];
  spacing: [Nm, Nm];
  angle: number;
  phi_extent: number;
  phi_spacing: number;
  priority: number;
  tick_interval: number;
  affects: { cursor: boolean; routing: boolean; placement: boolean };
  custom_properties?: [string, string][];
}

/** A grid item as `add_grid_item` / `edit_grid_item` take it (`eda_model::grid_item::GridItem`). */
export interface CmdGridItem {
  kind: "cartesian" | "polar";
  at: PointXY;
  extent: PointXY;
  spacing: PointXY;
  angle: number;
  phi_extent_deg: number;
  phi_spacing_deg: number;
  priority: number;
  tick_interval: number;
  affects: { cursor: boolean; routing: boolean; placement: boolean };
  custom_properties?: [string, string][];
}

/**
 * KiCad 10's via stitching (`PCB_VIA_STITCH`, crates/model/src/via_stitch.rs) as the state lists it -- the model's own form, which `set_via_stitch` takes back. nm; an absent number
 * is 0 (the pitch 0 is the default, twice the via and at least 2 mm), an absent layout or mode the first.
 */
export interface BoardViaStitch {
  id: string;
  layer?: string;
  name?: string;
  outline: PointXY[];
  pitch_nm?: number;
  layout?: "plain" | "staggered" | "poisson";
  mode?: "stitch" | "guard";
  seed?: number;
  origin_offset?: PointXY;
  origin?: PointXY;
  net?: string;
  guarded_net?: string;
  excluded_cells?: [number, number][];
  excluded_positions?: PointXY[];
  via?: { size_nm: number; drill_nm: number; from_layer: string; to_layer: string; text?: string };
  member_ids?: string[];
  custom_properties?: [string, string][];
}

export interface BoardPoint {
  id: string;
  layer: string;
  x: Nm;
  y: Nm;
  size: Nm;
}

/** `crates/model/src/pcb_items.rs` `PcbPoint`, IR field names. `id` is ignored by `add_point`. */
export interface CmdPoint {
  id?: string;
  layer: string;
  at: PointXY;
  size_nm: Nm;
}

export type BarcodeKind = "code39" | "code128" | "datamatrix" | "qr" | "microqr";
export type BarcodeEcc = "L" | "M" | "Q" | "H";

/**
 * `PCB_BARCODE`: a one-dimensional code, a Data Matrix or a QR code of copper, silkscreen or any other layer (`crates/model/src/barcode.rs`). `x`, `y` are its centre, `w`, `h` the size of the symbol
 * and `angle` its turn (the file's sense, millidegrees). KiCad works the modules out with zint when the board is loaded; the studio draws what kicad-cli plots of it (`GET /api/barcode_shape`).
 */
export interface BoardBarcode {
  id: string;
  layer: string;
  x: Nm;
  y: Nm;
  angle: number;
  w: Nm;
  h: Nm;
  text: string;
  /** What `${NAME}` in `text` stands for, when that differs from it. */
  shown?: string;
  text_height: Nm;
  kind: BarcodeKind;
  ecc: BarcodeEcc;
  show_text: boolean;
  knockout: boolean;
  margin_x: Nm;
  margin_y: Nm;
}

/** `crates/model/src/barcode.rs` `Barcode`, IR field names. `id` is ignored by `add_barcode`. */
export interface CmdBarcode {
  id?: string;
  layer: string;
  at: PointXY;
  angle?: number;
  width_nm: Nm;
  height_nm: Nm;
  text: string;
  text_height_nm: Nm;
  kind?: BarcodeKind;
  ecc?: BarcodeEcc;
  show_text?: boolean;
  knockout?: boolean;
  margin_x_nm?: Nm;
  margin_y_nm?: Nm;
}

/** `crates/model/src/ref_image.rs` `RefImage`, IR field names. `data` is the image file in base64 (no line breaks). `id` is ignored by `add_ref_image`. */
export interface CmdRefImage {
  id?: string;
  layer: string;
  at: PointXY;
  scale_ppm?: number;
  data: string;
}

/** The verbs of `{ op: "pcb_item", verb, ... }` (crates/ops/src/pcb_items.rs). */
export type PcbItemCmd =
  /** The Draw Text Box tool's OK (`DRAWING_TOOL::DrawRectangle`, `DIALOG_TEXTBOX_PROPERTIES`). */
  | { verb: "add_text_box"; text_box: CmdTextBox }
  /** Text Box Properties' OK and the Properties panel: every field replaced, in place. */
  | { verb: "edit_text_box"; id: string; text_box: CmdTextBox }
  | { verb: "delete_text_box"; id: string }
  /** Place Reference Image's click: the image, its centre and the layer it is on (`DRAWING_TOOL::PlaceReferenceImage`). */
  | { verb: "add_ref_image"; image: CmdRefImage }
  /** Reference Image Properties' OK (`DIALOG_REFERENCE_IMAGE_PROPERTIES::TransferDataFromWindow`): the position, the layer and the scale; the pixels stay. */
  | { verb: "edit_ref_image"; id: string; layer: string; at: PointXY; scale_ppm: number }
  /** The pixels replaced (Convert to Greyscale): the position, scale and layer stay. */
  | { verb: "set_ref_image_data"; id: string; data: string }
  | { verb: "delete_ref_image"; id: string }
  /** Place Point's click (`DRAWING_TOOL::PlacePoint`) and Point Properties' OK. */
  | { verb: "add_point"; point: CmdPoint }
  | { verb: "edit_point"; id: string; layer: string; at: PointXY; size_nm: Nm }
  | { verb: "delete_point"; id: string }
  /** Add Barcode's click and Barcode Properties' OK (`DIALOG_BARCODE_PROPERTIES`). */
  | { verb: "add_barcode"; barcode: CmdBarcode }
  | { verb: "edit_barcode"; id: string; barcode: CmdBarcode }
  | { verb: "delete_barcode"; id: string }
  /**
   * The fill of rectangles, circles and polygons: none, solid, hatch, reverse hatch or cross-hatch (`EDA_SHAPE::SetFillModeProp`; the Fill row of the Properties panel and the shape
   * dialog's choice), for all of `ids` as one undo step. The hatch lines are worked out when the board is described -- `pcbnew.Control.rehatchShapes` (`PCB_CONTROL::RehatchShapes`,
   * which KiCad posts after every commit, undo and file load) is that, so there is no verb for it. A fill left out changes nothing.
   */
  | { verb: "set_shape_style"; ids: string[]; fill?: ShapeFill; start_ending?: CmdLineEnding; end_ending?: CmdLineEnding }
  /** Place Sub-grid's second click (`DRAWING_TOOL::PlaceSubGrid`) and Grid Item Properties' OK (`DIALOG_GRID_ITEM_PROPERTIES`). */
  | { verb: "add_grid_item"; grid_item: CmdGridItem }
  | { verb: "edit_grid_item"; id: string; grid_item: CmdGridItem }
  | { verb: "delete_grid_item"; id: string }
  /**
   * A via stitching's settings and its vias made again (Via Stitching Properties' OK, the Properties panel, Exclude From Stitching, Clear Via Stitching Exclusions), or a new one
   * (Draw Via Stitch Area: no `id`); the server places the vias from the zone fills when `vias` is left out. One undo step.
   */
  | { verb: "set_via_stitch"; id?: string; stitch: Omit<BoardViaStitch, "id"> & { id?: string }; vias?: { cell: [number, number]; at: PointXY }[] }
  | { verb: "delete_via_stitch"; id: string }
  /** KiCad 10's Place Drill Chart: a chart of the default template on a documentation layer, built from the board, its first corner at `at`. One undo step. */
  | { verb: "place_drill_chart"; layer: string; at: PointXY }
  /** The Properties panel's Units, Decimal Places and Show Totals of a drill chart (`id` is its table's): the chart rebuilt from the board. */
  | { verb: "set_drill_chart"; id: string; units: TableUnits; precision: number; show_totals: boolean; filter: DrillChartFilter }
  /** KiCad 10's Place Drill Map: a map on a documentation layer that has none. */
  | { verb: "place_drill_map"; layer: string }
  | { verb: "edit_drill_map"; id: string; map: BoardDrillMap }
  | { verb: "delete_drill_map"; id: string };

/** `GENERATED_TABLE_UNITS` (crates/model/src/drill_chart.rs). */
export type TableUnits = "mm" | "inch" | "mils";

/** `DRILL_CHART_FILTER`: which holes a chart reports on. */
export interface DrillChartFilter {
  plated: boolean;
  non_plated: boolean;
  vias: boolean;
  slots: boolean;
  backdrills: boolean;
  castellated: boolean;
}

/** What makes a table of the board a KiCad 10 drill chart (`eda_model::drill_chart::DrillChart`), keyed by the table's id. */
export interface BoardDrillChart {
  table_id: string;
  units?: TableUnits;
  precision?: number;
  columns: { id: string; heading: string; align: "left" | "center" | "right"; width_nm?: number }[];
  /** Table row -> the key of the hole group it reports. */
  row_keys?: Record<string, string>;
  filter?: DrillChartFilter;
  show_totals?: boolean;
  /** The table column the marks are drawn in, -1 for none. */
  symbol_column?: number;
  /** Table row -> the curated shape drawn in its symbol cell. */
  row_shapes?: Record<string, number>;
}

/** `DRILL_SPAN`: the layers a drill runs between, start first. */
export interface BoardDrillSpan {
  start: string;
  end: string;
  backdrill?: boolean;
  npth?: boolean;
}

/** KiCad 10's drill map (`eda_model::drill_chart::DrillMap`): the drill marks drawn at the holes on a documentation layer, slid by `offset`. */
export interface BoardDrillMap {
  id: string;
  layer: string;
  offset: PointXY;
  size_nm: number;
  /** One span only; absent shows every span. */
  span?: BoardDrillSpan | null;
  outline_slots?: boolean;
  guide_cross?: boolean;
  custom_properties?: string[];
}

/** The board's drill symbol profile (`DRILL_SYMBOL_PROFILE`): the size and width of the marks are what the canvas reads. */
export interface BoardDrillProfile {
  size_nm: number;
  width_nm: number;
  mark_policy: string;
  group_by: string[];
  freeze_assignments: boolean;
}

/** One hole's mark as a map draws it (`DRILL_SYMBOL_ENTRY`, crates/ops/src/drill_chart.rs `DrillMark`). */
export interface BoardDrillMark {
  at: PointXY;
  span: BoardDrillSpan;
  mode: "shape" | "letter" | "size_text";
  shape_index: number;
  letter: string;
  diameter_nm: number;
  size_nm: [number, number];
  orientation_deg: number;
  is_slot: boolean;
}

/**
 * One line ending as `set_shape_style` takes it (`eda_model::line_ending::LineEnding`): KiCad 10's `(start_shape ..)` / `(end_shape ..)` of a line, an arc or a curve -- the shape dialog's
 * Start and End rows and the Properties panel's Start Shape, Start Length, Start Width and Start Stroke Width (and the End ones). A size of 0 is automatic; `none` takes the ending away.
 */
export interface CmdLineEnding {
  style: "none" | "arrow" | "circle" | "square" | "arrow_open";
  length_nm?: number;
  width_nm?: number;
  stroke_width_nm?: number;
}

/** `FILL_T`: how a closed shape is filled (`UI_FILL_MODE`). */
export type ShapeFill = "none" | "solid" | "hatch" | "reverse_hatch" | "cross_hatch";
