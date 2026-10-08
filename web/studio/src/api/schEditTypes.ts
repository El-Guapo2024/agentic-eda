// The schematic editor's tool verbs and drawn items -- mirrors crates/model/src/sch_extras.rs (SchGraphic) and
// crates/ops/src/sch_edit.rs (SchCmd, sent as `{ op: "sch_edit", verb: ..., ... }`).
import type { LabelShape, PointXY, Um } from "./types";

/** Stroke style of a graphic (`LINE_STYLE`); `default` follows the sheet's own default. */
export type SchLineStyle = "default" | "solid" | "dash" | "dot" | "dash_dot" | "dash_dot_dot";
/** How a closed graphic is filled (`FILL_T`): `outline` with the stroke colour, `background` with the body background, `color` with `fill_color`. */
export type SchFill = "none" | "outline" | "background" | "color";
export interface SchColor {
  r: number;
  g: number;
  b: number;
  /** 0..255. */
  a: number;
}
export type SchHAlign = "left" | "center" | "right";
export type SchVAlign = "top" | "center" | "bottom";
/** The outline of a directive label's flag (`LABEL_FLAG_SHAPE`'s `F_*`). */
export type DirectiveShape = "dot" | "round" | "diamond" | "rectangle";

export type SchGraphicShape =
  | { type: "rectangle"; start: PointXY; end: PointXY; corner_radius_um?: Um }
  | { type: "circle"; center: PointXY; radius_um: Um }
  | { type: "arc"; start: PointXY; mid: PointXY; end: PointXY }
  | { type: "bezier"; start: PointXY; c1: PointXY; c2: PointXY; end: PointXY }
  | { type: "polygon"; pts: PointXY[] }
  | {
      type: "text_box";
      start: PointXY;
      end: PointXY;
      text: string;
      /** 0 or 90000 (millidegrees): a text box keeps its text horizontal or vertical only. */
      angle?: number;
      size_um: Um;
      bold?: boolean;
      italic?: boolean;
      h_align?: SchHAlign;
      v_align?: SchVAlign;
      /** Border-to-text gap; 0/absent means KiCad's default (`SCH_TEXTBOX::GetLegacyTextMargin`). */
      margin_um?: Um;
    }
  | { type: "rule_area"; pts: PointXY[]; exclude_from_sim?: boolean; exclude_from_bom?: boolean; exclude_from_board?: boolean; dnp?: boolean }
  | {
      type: "directive";
      at: PointXY;
      /** The pole's direction in millidegrees: 0 right, 90000 up, 180000 left, 270000 down. */
      orientation?: number;
      shape?: DirectiveShape;
      pin_length_um: Um;
      netclass?: string;
      component_class?: string;
    };

/** One drawn shape / text box / rule area / directive label (`crates/model/src/sch_extras.rs` `SchGraphic`). */
export interface SchGraphic {
  id: string;
  shape: SchGraphicShape;
  /** Stroke width, um; 0/absent means the default line width. */
  width_um?: Um;
  line_style?: SchLineStyle;
  color?: SchColor;
  fill?: SchFill;
  fill_color?: SchColor;
}

/** A graphic as the add/edit verbs take it: the id is assigned by the backend. */
export type SchGraphicInput = Omit<SchGraphic, "id"> & { id?: string };

/** What `convert_text` turns the selected labels / texts / text boxes into (`SCH_EDIT_TOOL::ChangeTextType`'s `convertTo`). */
export type SchTextKind = "label" | "global_label" | "hier_label" | "directive_label" | "text" | "text_box";

/** A dialog one of the schematic's tools opens (`StudioState.schToolDialog`; components/SchToolDialogs.tsx). */
export type SchToolDialog =
  /** A drawn text box waiting for its text (`DrawShape` -> `DIALOG_TEXT_PROPERTIES`). */
  | { kind: "text_box"; start: PointXY; end: PointXY }
  /** A directive label waiting for its fields (`createNewLabel` -> `DIALOG_LABEL_PROPERTIES`). */
  | { kind: "directive"; at: PointXY }
  /** Cleanup Sheet Pins asking before it deletes the unreferenced pins of a sheet (`IsOK`). */
  | { kind: "cleanup_pins"; sheetId: string; sheetName: string; pins: Array<{ id: string; name: string }> }
  /** The Sync Sheet Pins dialog (`DIALOG_SYNC_SHEET_PINS`) for these sheets, `first` the one whose page opens. */
  | { kind: "sync_pins"; sheetIds: string[]; first?: string }
  /** Change Symbols / Update Symbols (`DIALOG_CHANGE_SYMBOLS`), `selected` the references selected when it opened. */
  | { kind: "change_symbols"; mode: "change" | "update"; selected: string[] }
  /** Edit Text & Graphics Properties (`DIALOG_GLOBAL_EDIT_TEXT_AND_GRAPHICS`), `selected` the ids selected when it opened. */
  | { kind: "edit_text_graphics"; selected: string[] };

export type SchEditCmd =
  /** Lock / Unlock / Toggle Lock (`SCH_EDIT_TOOL::modifyLockSelected`). */
  | { verb: "set_locked"; ids: string[]; locked: boolean }
  /** Draw a shape, text box, rule area or directive label (`SCH_DRAWING_TOOLS::DrawShape` / `DrawRuleArea`); the backend assigns the id. */
  | { verb: "add_graphic"; graphic: SchGraphicInput }
  | { verb: "delete_graphic"; id: string }
  /** Replace a graphic in place (keeps its id, drawing order and lock). */
  | { verb: "edit_graphic"; id: string; graphic: SchGraphicInput }
  /** Delete a placed sheet (symbol and pins; the file's content stays in the project). */
  | { verb: "delete_sheet"; id: string }
  /** Put a pin on a sheet's border (`SCH_SHEET::AddPin`); `at` must be on the border (kicad-port/schSheetPins.ts `constrainOnEdge`). */
  | { verb: "add_sheet_pin"; sheet: string; name: string; shape: LabelShape; at: PointXY }
  | { verb: "delete_sheet_pin"; id: string }
  /** Rename, reshape or move a sheet pin; a field left out is unchanged. */
  | { verb: "edit_sheet_pin"; id: string; name?: string; shape?: LabelShape; at?: PointXY }
  /** Give every placed unit of reference `id` the library symbol `lib_id` (`DIALOG_CHANGE_SYMBOLS`, change mode). */
  | { verb: "change_symbol"; id: string; lib_id: string }
  /** Update Symbol(s) from Library: the symbols placed with these library ids resolve from the project's edited library symbol (`published`). */
  | { verb: "update_library_symbols"; lib_ids: string[] };

/** One turn made while the items are held (`R`, Shift+`R`, `X`, `Y`). */
export type SchTurn = "rot_ccw" | "rot_cw" | "mirror_h" | "mirror_v";

/**
 * Move, Drag, Rotate, Mirror and Align to Grid for every kind of schematic item -- crates/ops/src/sch_move.rs (`SchMoveCmd`, sent as
 * `{ op: "sch_move", verb: ..., ... }`). `ids` are item ids (a symbol's reference moves every placed unit, `U1#2` names one).
 */
export type SchMoveCmd =
  /** `M`: every item moves rigidly; a wire on a moved pin is left where it is. `turns` are the R / Shift+R / X / Y pressed while the items were held (done after the move, about `about`, the point they are held at). */
  | { verb: "move"; ids: string[]; dx: Um; dy: Um; turns?: SchTurn[]; about?: PointXY }
  /** `G` and a click-drag: the items move and the wires, labels, junctions and no-connects attached follow; `vertices` names the picked points of a wire (`STARTPOINT` / `ENDPOINT`), a wire not in it is picked whole; `ortho` keeps right angles. */
  | { verb: "drag"; ids: string[]; vertices?: Record<string, number[]>; dx: Um; dy: Um; ortho?: boolean; grid?: Um; turns?: SchTurn[]; about?: PointXY }
  /** `R` / Shift+`R`: a quarter turn; `about` overrides the turn point (the cursor, while the selection is held). */
  | { verb: "rotate"; ids: string[]; vertices?: Record<string, number[]>; ccw?: boolean; about?: PointXY; grid?: Um }
  /** `X` (`vertical` false) / `Y`. */
  | { verb: "mirror"; ids: string[]; vertices?: Record<string, number[]>; vertical?: boolean; about?: PointXY; grid?: Um }
  /** Align Items to Grid: each item to the grid by where most of its connection points are, with the wires on it. */
  | { verb: "align_to_grid"; ids: string[]; grid?: Um }
  /** Align Left / Right / Top / Bottom / Center: each item by its own offset, snapped to the connection grid, with its wires. */
  | { verb: "align"; moves: Array<{ id: string; dx: Um; dy: Um }>; grid?: Um };

/**
 * What a move preview changes on the sheet (`POST /api/sch/move_preview`): the moved items' new geometry, which the view lays over the
 * sheet it already has. Collections with ids replace the item of that id; `wires`, `junctions`, `no_connects` and `bus_entries` are the
 * whole list, since a drag adds and removes some.
 */
export interface SchMovePatch {
  symbols: Array<{ id: string; unit: number; at: [number, number]; rot: number; mirror: "x" | "y" | null }>;
  power_symbols: Array<{ id: string; at: [number, number]; rot: number }>;
  wires: Array<{ id: string; net: string; pts: Array<[number, number]>; bus: boolean }>;
  labels: Array<{ id: string; at: [number, number]; spin: "right" | "up" | "left" | "bottom" | null }>;
  texts: Array<{ id: string; at: [number, number]; angle: number }>;
  no_connects: Array<{ id: string; at: [number, number] }>;
  bus_entries: Array<{ id: string; at: [number, number]; size: [number, number] }>;
  junctions: Array<{ id: string; at: [number, number] }>;
  lines: Array<{ id: string; pts: Array<[number, number]> }>;
  graphics: SchGraphic[];
  sheets: Array<{ id: string; at: [number, number]; size: [number, number]; pins: Array<{ id: string; at: [number, number] }> }>;
}
