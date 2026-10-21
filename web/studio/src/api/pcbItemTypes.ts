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

/** The verbs of `{ op: "pcb_item", verb, ... }` (crates/ops/src/pcb_items.rs). */
export type PcbItemCmd =
  /** The Draw Text Box tool's OK (`DRAWING_TOOL::DrawRectangle`, `DIALOG_TEXTBOX_PROPERTIES`). */
  | { verb: "add_text_box"; text_box: CmdTextBox }
  /** Text Box Properties' OK and the Properties panel: every field replaced, in place. */
  | { verb: "edit_text_box"; id: string; text_box: CmdTextBox }
  | { verb: "delete_text_box"; id: string };
