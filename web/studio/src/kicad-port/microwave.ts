// The microwave tools' pure half (`pcbnew/microwave/microwave_tool.cpp`, `microwave_footprint.cpp`, `microwave_inductor.cpp`, `microwave_polygon.cpp` at KiCad 8303b2ad): the words of
// their dialogs, what the dialogs' answers are read as, and where the inductor's end point is held. The footprints themselves are built by the server
// (`crates/ops/src/microwave.rs`, one verb each). Unit-tested in microwave.test.ts.
import type { LengthUnit } from "../state/units";
import { umFrom, umTo } from "../state/units";

/** The title every one of the tools' text prompts has (`_( "Create Microwave Footprint" )`). */
export const MICROWAVE_TITLE = "Create Microwave Footprint";

/** The tools that ask one size: `createFootprint`'s `msg`. */
export type SizedTool = "gap" | "stub" | "arc_stub";
export const SIZE_LABEL: Record<SizedTool, string> = { gap: "Gap Size:", stub: "Stub Size:", arc_stub: "Arc Stub Radius Value:" };

/** The arc stub's second prompt: `msg = wxT( "0.0" )` in `WX_TEXT_ENTRY_DIALOG angledlg( editFrame, _( "Angle in degrees:" ), .. )`. */
export const ANGLE_LABEL = "Angle in degrees:";
export const ANGLE_DEFAULT = "0.0";

/** What `DisplayError( editFrame, _( "Incorrect number, abort" ) )` says when the angle is not a number. */
export const INCORRECT_NUMBER = "Incorrect number, abort";

/** `wxString::ToDouble`: the whole text is one number (blanks before it are fine, after it are not), `null` when it is not. */
export function parseDouble(text: string): number | null {
  const m = /^\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)$/.exec(text);
  if (!m) return null;
  const v = Number(m[1]);
  return Number.isFinite(v) ? v : null;
}

/**
 * The arc stub's angle once typed: `angle = EDA_ANGLE( fval, DEGREES_T ); if( angle < ANGLE_0 ) angle = -angle; if( angle > ANGLE_180 ) angle = ANGLE_180;`, in the millidegrees the verb takes
 * (the server clamps again).
 */
export function arcStubAngleMillideg(degrees: number): number {
  return Math.round(Math.min(Math.abs(degrees), 180) * 1000);
}

const DECIMALS: Record<LengthUnit, number> = { mm: 4, mil: 2, in: 5 };

/** `StringFromValue( value )`: a length in the display units without their name, to the digits the units show. */
export function lengthText(valueUm: number, units: LengthUnit): string {
  return String(Number(umTo(valueUm, units).toFixed(DECIMALS[units])));
}

/** `ValueFromString`: "1.5", "0.2mm", "10mil", `0.01"` as micrometres (a bare number is in the display units); 0 for text that is not a length, as KiCad reads it. */
export function lengthFromText(text: string, units: LengthUnit): number {
  const m = /^\s*(-?\d*\.?\d+)\s*(mm|mil|in|")?\s*$/i.exec(text);
  if (!m) return 0;
  const unit = m[2] ? (m[2] === '"' ? "in" : (m[2].toLowerCase() as LengthUnit)) : units;
  return Math.round(umFrom(parseFloat(m[1]!), unit));
}
