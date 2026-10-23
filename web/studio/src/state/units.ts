// Unit conversion/formatting. The backend always speaks nanometres
// (nm, integers, KiCad's internal unit) -- see crates/model/src/units.rs `pub type Nm = i64`. This
// module only ever touches display: every Cmd sent back to the API is
// still in nm regardless of which unit the status bar shows.

export type LengthUnit = "mm" | "mil" | "in";

const NM_PER_MM = 1_000_000;
const NM_PER_MIL = 25_400;
const NM_PER_IN = 25_400_000;

export function nmTo(value: number, unit: LengthUnit): number {
  switch (unit) {
    case "mm":
      return value / NM_PER_MM;
    case "mil":
      return value / NM_PER_MIL;
    case "in":
      return value / NM_PER_IN;
  }
}

export function nmFrom(value: number, unit: LengthUnit): number {
  switch (unit) {
    case "mm":
      return value * NM_PER_MM;
    case "mil":
      return value * NM_PER_MIL;
    case "in":
      return value * NM_PER_IN;
  }
}

const DECIMALS: Record<LengthUnit, number> = { mm: 3, mil: 1, in: 4 };

export function formatLength(valueNm: number, unit: LengthUnit): string {
  return `${nmTo(valueNm, unit).toFixed(DECIMALS[unit])} ${unit}`;
}

export function formatXY(xNm: number, yNm: number, unit: LengthUnit): string {
  const d = DECIMALS[unit];
  return `${nmTo(xNm, unit).toFixed(d)}, ${nmTo(yNm, unit).toFixed(d)} ${unit}`;
}

/** Cartesian (dx, dy) in nm -> polar (radius in `unit`, angle in degrees, 0=east, CCW positive -- KiCad's convention). */
export function toPolar(dxNm: number, dyNm: number, unit: LengthUnit): { r: string; theta: string } {
  const r = Math.hypot(dxNm, dyNm);
  const theta = (Math.atan2(-dyNm, dxNm) * 180) / Math.PI; // screen Y grows downward; flip so CCW is positive
  return { r: formatLength(r, unit), theta: `${theta.toFixed(1)}°` };
}
