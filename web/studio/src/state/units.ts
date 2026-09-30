// Unit conversion/formatting. The backend always speaks micrometres
// (µm, integers) -- see crates/model/src/ir.rs `pub type Um = i64`. This
// module only ever touches display: every Cmd sent back to the API is
// still in µm regardless of which unit the status bar shows.

export type LengthUnit = "mm" | "mil" | "in";

const UM_PER_MM = 1000;
const UM_PER_MIL = 25.4;
const UM_PER_IN = 25_400;

export function umTo(value: number, unit: LengthUnit): number {
  switch (unit) {
    case "mm":
      return value / UM_PER_MM;
    case "mil":
      return value / UM_PER_MIL;
    case "in":
      return value / UM_PER_IN;
  }
}

export function umFrom(value: number, unit: LengthUnit): number {
  switch (unit) {
    case "mm":
      return value * UM_PER_MM;
    case "mil":
      return value * UM_PER_MIL;
    case "in":
      return value * UM_PER_IN;
  }
}

const DECIMALS: Record<LengthUnit, number> = { mm: 3, mil: 1, in: 4 };

export function formatLength(valueUm: number, unit: LengthUnit): string {
  return `${umTo(valueUm, unit).toFixed(DECIMALS[unit])} ${unit}`;
}

export function formatXY(xUm: number, yUm: number, unit: LengthUnit): string {
  const d = DECIMALS[unit];
  return `${umTo(xUm, unit).toFixed(d)}, ${umTo(yUm, unit).toFixed(d)} ${unit}`;
}

/** Cartesian (dx, dy) in µm -> polar (radius in `unit`, angle in degrees, 0=east, CCW positive -- KiCad's convention). */
export function toPolar(dxUm: number, dyUm: number, unit: LengthUnit): { r: string; theta: string } {
  const r = Math.hypot(dxUm, dyUm);
  const theta = (Math.atan2(-dyUm, dxUm) * 180) / Math.PI; // screen Y grows downward; flip so CCW is positive
  return { r: formatLength(r, unit), theta: `${theta.toFixed(1)}°` };
}
