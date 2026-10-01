// `dialog_pin_properties.cpp`'s Orientation combo (Right/Left/Up/Down) <->
// `LibPin::angle_deg`/`LibrarySymbolPin.angle_deg` (0/90/180/270, the
// direction *from* the pin's outer end *into* the symbol body -- see
// `components/schematic/transform.ts`'s own `pinDirectionInternal` doc).
// Confirmed against this app's own already-correct builtin library
// transcriptions (`crates/model/src/symbol.rs`'s `device_r`/`device_d`/
// `conn_01x`, read directly rather than guessed): a pin drawn to the
// *left* of the body (its stub pointing further left, e.g. `conn_01x`'s
// pins at x=-5.08 with the body at x in [-1.27,1.27]) has `angle_deg ===
// 0`, i.e. "Left" maps to 0, not 180 as the bare word might suggest --
// the angle names which way the pin points *into* the body (east, here),
// the opposite of which way its own stub visually points (west).
export type PinOrientation = "right" | "left" | "up" | "down";

const ORIENTATION_TO_ANGLE: Record<PinOrientation, number> = {
  right: 180,
  left: 0,
  up: 270,
  down: 90,
};

const ANGLE_TO_ORIENTATION: Record<number, PinOrientation> = {
  180: "right",
  0: "left",
  270: "up",
  90: "down",
};

/** `dialog_pin_properties.cpp`'s own combo order. */
export const PIN_ORIENTATIONS: PinOrientation[] = ["right", "left", "up", "down"];

export function orientationToAngleDeg(o: PinOrientation): number {
  return ORIENTATION_TO_ANGLE[o];
}

/** Nearest cardinal orientation for an arbitrary angle (every real pin is exactly one of the four; this just normalizes/clamps a stray value defensively). */
export function angleDegToOrientation(angleDeg: number): PinOrientation {
  const norm = (((Math.round(angleDeg / 90) * 90) % 360) + 360) % 360;
  return ANGLE_TO_ORIENTATION[norm] ?? "right";
}
