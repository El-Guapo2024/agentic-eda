// The microwave tools' verbs (GAPS.md item 9, package P19) -- mirrors crates/ops/src/microwave.rs (`MicrowaveCmd`, sent as `{ op: "microwave", verb: ..., ... }`).
// `Um` lengths are micrometres, angles millidegrees.
import type { PointXY, Um } from "./types";

/** What was done to the footprint while it was in the hand (`doInteractiveItemPlacement`'s `IPO_ROTATE | IPO_FLIP`): the turn (clockwise-positive millidegrees, `rotate_items`'s sense), then the flip. */
export interface MicrowaveHand {
  rotate_millideg?: number;
  flip?: "left_right" | "top_bottom";
}

export type MicrowaveCmd =
  /** Draw Microwave Gaps / Stubs: two pads of the track width, `size_um` apart (a gap) or one of them `size_um` long (a stub). */
  | { verb: "gap" | "stub"; at: PointXY; size_um: Um; track_width_um: Um; hand?: MicrowaveHand }
  /** Draw Microwave Arc Stubs: one custom pad, a pie slice of `radius_um` and `angle_millideg` (0 to 180 degrees). */
  | { verb: "arc_stub"; at: PointXY; radius_um: Um; angle_millideg: number; track_width_um: Um; hand?: MicrowaveHand };
