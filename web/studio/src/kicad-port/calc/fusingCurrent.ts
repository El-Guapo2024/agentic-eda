// The Fusing Current calculator (pcb_calculator/calculator_panels/panel_fusing_current.cpp at KiCad 25ec2335, `PANEL_FUSING_CURRENT`): the current a
// copper track carries for a given time before it melts, or the width, thickness or time for the other values -- the energy that heats the copper
// to its melting point and melts it, against what its resistance dissipates (adiabatic; the Adam paper's equation 9b). The radio button picks the
// value to compute; the others must be valid numbers. The comment warns when the fusing time is long against the track's radiative time constant.
//
// Pure: no React, no DOM. The panel is components/calculator/PanelFusingCurrent.tsx.
import { printfF } from "./cformat";
import { toDouble } from "./unitSelectors";

const ABS_ZERO = -273.15;

/** The four radio buttons (`m_widthRadio`, `m_thicknessRadio`, `m_currentRadio`, `m_timeRadio`). */
export type FusingTarget = "width" | "thickness" | "current" | "time";

export interface FusingInputs {
  ambient: string;
  melting: string;
  width: string;
  thickness: string;
  current: string;
  time: string;
  /** `m_widthUnit->GetUnitScale()` (UNIT_SELECTOR_LEN) and `m_thicknessUnit->GetUnitScale()` (UNIT_SELECTOR_THICKNESS). */
  widthScale: number;
  thicknessScale: number;
  target: FusingTarget;
}

export interface FusingOutcome {
  /** The text written into the computed value's box ("Error" when an input is not valid). */
  value: string;
  /** `m_comment`'s label. */
  comment: string;
}

/** The constructor's values: 25 °C, 1084 °C (copper; not editable), 0.1 mm, 0.035 mm, 10 A, 0.01 s (`%i` / `%f`). */
export const FUSING_DEFAULTS = {
  ambient: "25",
  melting: "1084",
  width: printfF(0.1),
  thickness: printfF(0.035),
  current: printfF(10.0),
  time: printfF(0.01),
} as const;

export const FUSING_LONG_TIME_COMMENT = "Current calculation is underestimated due to long fusing time.";

/** `wxString::ToDouble`: false (and NaN here) when the text is not a number. */
const read = (t: string): [number, boolean] => {
  const v = toDouble(t);
  return v === null ? [Number.NaN, false] : [v, true];
};

/** `PANEL_FUSING_CURRENT::m_onCalculateClick`. */
export function calculateFusing(inp: FusingInputs): FusingOutcome {
  const [Tm, validTm0] = read(inp.melting);
  let validTm = validTm0;
  const [Ta, validTa0] = read(inp.ambient);
  let validTa = validTa0;
  let [I, validI] = read(inp.current);
  let [W, validW] = read(inp.width);
  let [T, validT] = read(inp.thickness);
  let [time, validTime] = read(inp.time);
  T *= inp.thicknessScale;
  W *= inp.widthScale;

  validTm &&= Number.isFinite(Tm);
  validTa &&= Number.isFinite(Ta);
  validI &&= Number.isFinite(I);
  validW &&= Number.isFinite(W);
  validT &&= Number.isFinite(T);
  validTime &&= Number.isFinite(time);

  if (validTm && validTa) {
    validTm &&= Tm > Ta;
    validTa &&= Tm > Ta && Ta > ABS_ZERO;
  }
  validI &&= I > 0;
  validW &&= W > 0;
  validT &&= T > 0;
  validTime &&= time > 0;

  let A = W * T;
  // Copper: latent heat of fusion 205.35 kJ/kg, heat capacity 385 J/kg/K, density 8940 kg/m^3.
  const latentHeat = 205350.0;
  const cp = 385;
  const deltaEnthalpy = (Tm - Ta) * cp;
  const density = 8940;
  const volumicEnergy = density * (deltaEnthalpy + latentHeat);
  // Copper resistivity 1.72e-8 ohm m at 20 C (293 K), 0.00393 per degree; the average of ambient and melting.
  const Ra = ((Ta - ABS_ZERO - 293) * 0.00393 + 1) * 1.72e-8;
  const Rm = ((Tm - ABS_ZERO - 293) * 0.00393 + 1) * 1.72e-8;
  const R = (Rm + Ra) / 2;
  const coeff = volumicEnergy / R;

  const valid = validI && validW && validT && validTa && validTm && validTime;
  let value = "Error";
  if (inp.target === "width") {
    if (valid) {
      A = I / Math.sqrt(coeff / time);
      W = A / T;
      value = printfF(W / inp.widthScale);
    }
  } else if (inp.target === "thickness") {
    if (valid) {
      A = I / Math.sqrt(coeff / time);
      T = A / W;
      value = printfF(T / inp.thicknessScale);
    }
  } else if (inp.target === "current") {
    if (valid) {
      I = A * Math.sqrt(coeff / time);
      value = printfF(I);
    }
  } else if (valid) {
    time = (coeff * A * A) / I / I;
    value = printfF(time);
  }

  // The validity domain: the track as a circle of the same area, Stefan-Boltzmann 5.67e-8, emissivity 0.5.
  const r = Math.sqrt(A / Math.PI);
  const epsilon = 5.67e-8;
  const sigma = 0.5;
  const tmKelvin = Tm - ABS_ZERO;
  const frad = 0.5 * (tmKelvin + 293) * (tmKelvin + 293) * (tmKelvin + 293);
  const tau = (cp * density * r) / (epsilon * sigma * frad * 2);
  return { value, comment: 2 * time < tau ? "" : FUSING_LONG_TIME_COMMENT };
}

/** fusing_current_help.md. */
export const FUSING_CURRENT_HELP = `You can use this calculator to check if a small track can handle a large current for a short period of time.<br>
This tool allows you to design a track fuse but should be used as an estimate only.

The calculator estimates the energy required to heat the wire up<br>
to its melting point as well as the energy required for the change of phase.<br>
This energy is then compared to the one dissipated by the wire resistance.`;
