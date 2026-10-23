// The board items GAPS.md item 24 (package P16) added -- mirrors crates/model/src/pcb_items.rs (`TextBox`, ..) and crates/ops/src/pcb_items.rs (`PcbItemCmd`, sent as
// `{ op: "pcb_item", verb: ..., ... }`). `Board*` types are the display shape `GET /api/state` sends in `drawings` (crates/cli/src/pcb_items_json.rs: points are `[x, y]`
// pairs); `Cmd*` types are the IR's own shape, which a verb takes.
import type { SchLineStyle } from "./schEditTypes";
import type { PointXY, Um } from "./types";

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
  start: [Um, Um];
  end: [Um, Um];
  /** The four corners of a box turned by an angle that is not a multiple of 90 degrees; empty for a rectangle. */
  pts: [Um, Um][];
  /** The text angle in the file's sense (counter-clockwise on the screen), millidegrees, 0 to 360000. */
  angle: number;
  /** The margins, left, top, right, bottom. */
  margins: [Um, Um, Um, Um];
  border: boolean;
  stroke_width: Um;
  line_style: SchLineStyle;
  stroke_color: string | null;
  knockout: boolean;
  /** The glyph height and width, µm. */
  size: Um;
  width: Um;
  /** The pen the text is drawn with, µm (the automatic one when `thickness_auto`). */
  thickness: Um;
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
  margin_left_um: Um;
  margin_top_um: Um;
  margin_right_um: Um;
  margin_bottom_um: Um;
  border?: boolean;
  stroke_width_um: Um;
  line_style?: SchLineStyle;
  stroke_color?: string | null;
  knockout?: boolean;
  size_um: Um;
  width_um?: Um | null;
  /** 0 is KiCad's automatic pen. */
  thickness_um?: Um;
  halign?: TextBoxHAlign;
  valign?: TextBoxVAlign;
  mirror?: boolean;
  bold?: boolean;
  italic?: boolean;
  line_spacing_permille?: number;
  face?: string | null;
}

/**
 * `PCB_REFERENCE_IMAGE`: a bitmap on a graphic layer, kept out of every plot. `x`, `y` are the image's centre, `w`, `h` its size in µm (the pixels at their definition, `ppi`, and the
 * `scale`). The pixels are not in the state: `GET /api/pcb_image?id=..&h=<hash>` serves them (the PNG or JPEG file the board embeds), and `hash` changes when they do.
 */
export interface BoardRefImage {
  id: string;
  layer: string;
  x: Um;
  y: Um;
  w: Um;
  h: Um;
  /** `(scale s)` of the file in millionths (1.0 is 1000000). */
  scale_ppm: number;
  /** The size in pixels, and the definition in pixels per inch (`BITMAP_BASE::GetPPI`, 300 when the file says none). */
  px: [number, number];
  ppi: number;
  hash: string;
}

/** `PCB_POINT`: a marker on the board (a cross with a ring), a snap anchor for the grid helper. `x`, `y` are its centre, `size` its extent in µm. */
export interface BoardPoint {
  id: string;
  layer: string;
  x: Um;
  y: Um;
  size: Um;
}

/** `crates/model/src/pcb_items.rs` `PcbPoint`, IR field names. `id` is ignored by `add_point`. */
export interface CmdPoint {
  id?: string;
  layer: string;
  at: PointXY;
  size_um: Um;
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
  x: Um;
  y: Um;
  angle: number;
  w: Um;
  h: Um;
  text: string;
  /** What `${NAME}` in `text` stands for, when that differs from it. */
  shown?: string;
  text_height: Um;
  kind: BarcodeKind;
  ecc: BarcodeEcc;
  show_text: boolean;
  knockout: boolean;
  margin_x: Um;
  margin_y: Um;
}

/** `crates/model/src/barcode.rs` `Barcode`, IR field names. `id` is ignored by `add_barcode`. */
export interface CmdBarcode {
  id?: string;
  layer: string;
  at: PointXY;
  angle?: number;
  width_um: Um;
  height_um: Um;
  text: string;
  text_height_um: Um;
  kind?: BarcodeKind;
  ecc?: BarcodeEcc;
  show_text?: boolean;
  knockout?: boolean;
  margin_x_um?: Um;
  margin_y_um?: Um;
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
  | { verb: "edit_point"; id: string; layer: string; at: PointXY; size_um: Um }
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
  | { verb: "set_shape_style"; ids: string[]; fill?: ShapeFill; start_ending?: CmdLineEnding; end_ending?: CmdLineEnding };

/**
 * One line ending as `set_shape_style` takes it (`eda_model::line_ending::LineEnding`): KiCad 10's `(start_shape ..)` / `(end_shape ..)` of a line, an arc or a curve -- the shape dialog's
 * Start and End rows and the Properties panel's Start Shape, Start Length, Start Width and Start Stroke Width (and the End ones). A size of 0 is automatic; `none` takes the ending away.
 */
export interface CmdLineEnding {
  style: "none" | "arrow" | "circle" | "square" | "arrow_open";
  length_um?: number;
  width_um?: number;
  stroke_width_um?: number;
}

/** `FILL_T`: how a closed shape is filled (`UI_FILL_MODE`). */
export type ShapeFill = "none" | "solid" | "hatch" | "reverse_hatch" | "cross_hatch";
