// The wire types of the Footprint Editor's verbs of UI-LEFT F3, F4 and F8 (`crates/ops/src/fp_editor.rs`) and of what a library footprint keeps of itself beyond its items
// (`crates/model/src/fp_settings.rs`, `crates/model/src/lib_keep.rs`).
import type { Nm } from "./types";

/** `eda_model::fp_settings::FootprintSettings`: Footprint Properties' Clearance Overrides, Private Layers, Net Ties and jumper groups. Absent fields are none. */
export interface FootprintSettings {
  /** `FOOTPRINT::GetLocalClearance`, nm. */
  clearance?: Nm | null;
  solder_mask_margin?: Nm | null;
  solder_paste_margin?: Nm | null;
  /** A fraction of the pad's size (-0.05 is 5% smaller). */
  solder_paste_margin_ratio?: number | null;
  private_layers?: string[];
  net_tie_pad_groups?: string[];
  duplicate_pad_numbers_are_jumpers?: boolean;
  jumper_pad_groups?: string[][];
}

/** A footprint loaded from the board (`FOOTPRINT_EDIT_FRAME::IsCurrentFPFromBoard`): the instance it is linked to and its own `Lib:Name` (`GetFPID()`). */
export interface BoardLink {
  reference: string;
  fpid: string;
}

/** `crates/ops/src/fp_editor.rs` `FpEditorCmd`. */
export type FpEditorCmd =
  | { verb: "open_from_board"; reference: string }
  | { verb: "save_to_board"; footprint: string }
  | { verb: "set_settings"; footprint: string; settings: FootprintSettings }
  /** File > Import > Graphics... into the footprint (`crate::ImportGraphics` without its `op`). */
  | { verb: "import_graphics"; footprint: string; import: Omit<import("./types").ImportGraphicsCmd, "op"> };

/** `fp_items::ShapeLook`: a graphic's line style (`solid`, `default`, `dash`, `dot`, `dash_dot`, `dash_dot_dot`) and hatched fill. */
export interface ShapeLook {
  line_style: string;
  hatch: "hatch" | "reverse_hatch" | "cross_hatch" | null;
  /** KiCad 10's line endings of an open shape (UI-LEFT-2 FF3); left out, the graphic keeps its own. */
  endings?: { start?: LineEndingIr; end?: LineEndingIr };
}

/** `eda_model::line_ending::LineEnding` as the IR serializes it (each field left out when it is the default). */
export interface LineEndingIr {
  style?: "none" | "arrow" | "circle" | "square" | "arrow_open";
  length_nm?: number;
  width_nm?: number;
  stroke_width_nm?: number;
  stroke_style?: string;
}

/** `fp_items::TextLook`: a footprint text's bold, italic, vertical justification (-1 top, 0 centre, 1 bottom), glyph width, knockout and keep-upright. */
export interface TextLook {
  bold: boolean;
  italic: boolean;
  valign: -1 | 0 | 1;
  width_nm: Nm | null;
  knockout: boolean;
  keep_upright: boolean;
  /** The font to set (`SetFont`): a face, or "" for KiCad's stroke font; left out, the text keeps its own (UI-LEFT-2 CC1). */
  face?: string;
}

/** The key a board footprint's copy is held by in the editor (`BoardFootprintCopy::key_of`): no footprint name can have `<`, `>` or `/`. */
export const BOARD_COPY_PREFIX = "<board>/";

export const boardCopyKey = (reference: string): string => `${BOARD_COPY_PREFIX}${reference}`;

/** The reference a board copy's key names, or null for a library footprint's name. */
export const boardCopyReference = (key: string | null | undefined): string | null => (key && key.startsWith(BOARD_COPY_PREFIX) ? key.slice(BOARD_COPY_PREFIX.length) : null);
