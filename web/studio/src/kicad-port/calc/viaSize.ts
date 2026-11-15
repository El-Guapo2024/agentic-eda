// The Via Size panel's arithmetic (`PANEL_VIA_SIZE::OnViaCalculate`, pcb_calculator/calculator_panels/panel_via_size.cpp, KiCad 25ec2335), after
// the CircuitCalculator.com PCB via calculator: resistance, voltage drop, power loss, thermal resistance and IPC-2221 ampacity of the plated barrel,
// and Johnson & Graham's capacitance, rise time degradation, inductance and reactance (equations 7.6, 7.8, 7.9, 7.11). The C++'s order of
// operations is kept, and so are its units: the lengths in metres go straight into 7.6 and 7.9, whose results the panel labels pF and nH.
//
// Pure: no React, no DOM.
import { UNIT_MIL } from "./unitSelectors";

/** The inputs, already `std::abs`-ed and normalized (lengths in m, impedance in ohm; resistivity in ohm*m, temperature in deg C, rise time in ns). */
export interface ViaInputs {
  finishedHoleDia: number;
  platingThickness: number;
  viaLength: number;
  padDia: number;
  clearanceDia: number;
  charImpedance: number;
  appliedCurrent: number;
  platingResistivity: number;
  relativePermitivity: number;
  temperatureDiff: number;
  pulseRiseTime: number;
}

export interface ViaResults {
  viaResistance: number;
  voltageDrop: number;
  powerLoss: number;
  estimatedAmpacity: number;
  thermalResistance: number;
  capacitance: number;
  timeDegradation: number;
  inductance: number;
  reactance: number;
}

/** `OnViaCalculate` from "Calculate cross-sectional area" on. */
export function calculateVia(p: ViaInputs): ViaResults {
  let area = Math.PI * (p.finishedHoleDia + p.platingThickness) * p.platingThickness; // m^2
  const viaResistance = (p.platingResistivity * p.viaLength) / area; // Ohms
  // Thermal resistivity 2.49e-3 m K/W: a thermal conductivity of 401 W/(m K).
  const thermalResistivity = 2.49e-3;
  const thermalResistance = (thermalResistivity * p.viaLength) / area; // deg C/W
  const voltageDrop = p.appliedCurrent * viaResistance;
  const powerLoss = p.appliedCurrent * voltageDrop;
  // IPC-2221's formula, also behind the Track Width calculator.
  area /= Math.pow(UNIT_MIL, 2); // m^2 to mil^2
  const k = 0.048;
  const b = 0.44;
  const c = 0.725;
  const estimatedAmpacity = k * Math.pow(p.temperatureDiff, b) * Math.pow(area, c);
  let capacitance = 55.51 * p.relativePermitivity * p.viaLength * p.padDia;
  capacitance /= p.clearanceDia - p.padDia;
  const timeDegradation = (2.2 * capacitance * p.charImpedance) / 2;
  let inductance = 200 * p.viaLength;
  inductance *= Math.log((4 * p.viaLength) / p.finishedHoleDia) + 1;
  const reactance = (Math.PI * inductance) / p.pulseRiseTime;
  return { viaResistance, voltageDrop, powerLoss, estimatedAmpacity, thermalResistance, capacitance, timeDegradation, inductance, reactance };
}
