// The Footprint Editor's verbs on any item of the open footprint (`crates/ops/src/fp_items.rs`, `{"op": "fp_item", "verb": ...}`) and the items a library
// footprint holds beyond its pads, graphics, texts and fields (`crates/model/src/fp_items.rs`: dimensions, zones, text boxes, points, images, barcodes, tables,
// groups). `GET /api/footprint` sends them twice: as the IR has them (`items`, what the verbs take back) and in the board's display shapes (`display`, what the
// board's painters draw: `crates/cli/src/fp_items_json.rs`).
import type { FillZone } from "./types";
import type { CmdDimension, CmdShape, CmdText, CmdZone, Dimension, FieldLayoutCmd, Group, Nm, PointXY, Zone } from "./types";
import type { BoardBarcode, BoardPoint, BoardRefImage, BoardTextBox, CmdBarcode, CmdPoint, CmdRefImage, CmdTextBox } from "./pcbItemTypes";
import type { BoardTable } from "./types";

/** `eda_model::fp_items::FootprintItems`, as the IR has them (each list left out when empty). */
export interface FootprintItemsIr {
  dimensions?: CmdDimension[];
  zones?: CmdZone[];
  text_boxes?: CmdTextBox[];
  points?: CmdPoint[];
  images?: CmdRefImage[];
  barcodes?: CmdBarcode[];
  /** `eda_model::table::Table` as the IR has it (`TableCmdWire`'s `table`). */
  tables?: unknown[];
  groups?: Group[];
}

/** A zone of the footprint in its display shape: the IR's zone with its outline and holes as `[x, y]` pairs (`outline_xy`, `holes_xy`). */
export type FpZoneDisplay = Omit<Zone, "outline" | "holes"> & { outline: PointXY[]; holes?: PointXY[][]; outline_xy: [Nm, Nm][]; holes_xy: [Nm, Nm][][] };

/** `display` of `GET /api/footprint` (absent when the footprint has none of these items). */
export interface FootprintItemsDisplay {
  dimensions: Dimension[];
  zones: FpZoneDisplay[];
  text_boxes: BoardTextBox[];
  images: BoardRefImage[];
  points: BoardPoint[];
  barcodes: BoardBarcode[];
  tables: BoardTable[];
  /** The fills the zones brought (`LibraryFootprintKeep::zone_fills`, as they hold for each zone now): one per zone and layer, in `GET /api/fill`'s shape. */
  zone_fills?: FillZone[];
}

/** `fp_items::FpItem`: one item of the kinds `eda_model::fp_items` adds, as `add` and `set` carry it. */
export type FpItemWire = { dimension: CmdDimension } | { zone: CmdZone } | { text_box: CmdTextBox } | { point: CmdPoint } | { image: CmdRefImage } | { barcode: CmdBarcode } | { table: unknown };

export type FlipDirectionWire = "left_right" | "top_bottom";

/** `fp_items::FpItemCmd`. A field is named `field:<name>`; a group stands for the items below it. */
export type FpItemCmd =
  | { verb: "move"; footprint: string; ids: string[]; dx: Nm; dy: Nm }
  /** Clockwise-positive on the screen, as `rotate_items`. */
  | { verb: "rotate"; footprint: string; ids: string[]; pivot: PointXY; angle_millideg: number }
  | { verb: "mirror"; footprint: string; ids: string[]; pivot: PointXY; direction: FlipDirectionWire }
  /** `EDIT_TOOL::Swap` (Alt+S), `ids` in selection order; `centres`: each group's `GetPosition()`, the centre of its box. */
  | { verb: "swap"; footprint: string; ids: string[]; centres?: Record<string, PointXY> }
  | { verb: "delete"; footprint: string; ids: string[] }
  | { verb: "edit_field"; footprint: string; name: string; text: string; layout: FieldLayoutCmd }
  /** `look`: the shape dialog's Line style and hatched Fill (UI-LEFT F5); left out, the graphic keeps its own. */
  | { verb: "set_graphic"; footprint: string; id: string; shape: CmdShape; look?: import("./fpEditorTypes").ShapeLook }
  /** `look`: a footprint text's bold, italic, vertical justification, width, knockout and keep-upright (UI-LEFT F5); left out, the text keeps its own. */
  | { verb: "set_text"; footprint: string; id: string; text: CmdText; look?: import("./fpEditorTypes").TextLook }
  | { verb: "add"; footprint: string; item: FpItemWire }
  | { verb: "set"; footprint: string; id: string; item: FpItemWire }
  | { verb: "group"; footprint: string; ids: string[] }
  | { verb: "ungroup"; footprint: string; ids: string[] }
  | { verb: "set_group_name"; footprint: string; id: string; name: string }
  /** `EDIT_TOOL::Duplicate` in the footprint editor: a copy of every item but a field, with its looks, a group with its members; `pad_number` the number Duplicate and
   *  Increment gives the copied pads (crates/ops/src/fp_copy.rs, UI-LEFT-2 FF2). */
  | { verb: "duplicate"; footprint: string; ids: string[]; pad_number?: string }
  /** `PCB_CONTROL::Paste` in the footprint editor: the items of KiCad's clipboard footprint text, new, its origin at `at` (UI-LEFT-2 FF1). */
  | { verb: "paste"; footprint: string; text: string; at: PointXY }
  /** `PCB_CONTROL::Paste` with table cells selected and a table on a board's clipboard: its cells into the selected ones ("Paste Cells"). */
  | { verb: "paste_cells"; footprint: string; ids: string[]; text: string }
  | { verb: "add_to_group"; footprint: string; group_id: string; ids: string[] }
  | { verb: "remove_from_group"; footprint: string; ids: string[] }
  /** A library pad's backdrill and post-machining and its own teardrop parameters, each set whole, left out to stay (UI-LEFT-2 FF3). */
  | { verb: "set_pad_extras"; footprint: string; id: string; drill_props?: import("./types").PadDrillProps; teardrops?: import("./types").TeardropParams };
