// `dialog_pin_properties.cpp`'s Orientation combo (Right/Left/Up/Down) <-> `LibPin::angle_deg`/`LibrarySymbolPin.angle_deg`, which is the file's angle
// (`(pin .. (at x y <angle>))`) as it is.
//
// `sch_io_kicad_sexpr_parser.cpp::parseSymbolPin` reads that angle as
//     case 0:   pin->SetOrientation( PIN_ORIENTATION::PIN_RIGHT ); break;
//     case 90:  pin->SetOrientation( PIN_ORIENTATION::PIN_UP );    break;
//     case 180: pin->SetOrientation( PIN_ORIENTATION::PIN_LEFT );  break;
//     case 270: pin->SetOrientation( PIN_ORIENTATION::PIN_DOWN );  break;
// and the combo shows `PIN_RIGHT` as "Right" (`pin_type.cpp::InitTables`). The orientation names the way the pin runs from its connection point
// to the body: `Device:R`'s pin 1, above the body at file angle 270, is "Down"; a pin left of a connector's body, `(at -5.08 0 0)`, is "Right".
// (Before r7 E7-S1 this table was turned 180 degrees and every label was the opposite one; the stored angles never changed, so a file keeps its
// pins pointing the way they did.)
export type PinOrientation = "right" | "left" | "up" | "down";

const ORIENTATION_TO_ANGLE: Record<PinOrientation, number> = {
  right: 0,
  up: 90,
  left: 180,
  down: 270,
};

const ANGLE_TO_ORIENTATION: Record<number, PinOrientation> = {
  0: "right",
  90: "up",
  180: "left",
  270: "down",
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
