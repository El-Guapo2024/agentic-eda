// The Wavelength panel's arithmetic (pcb_calculator/calculator_panels/panel_wavelength.cpp, KiCad 25ec2335): one frequency shown as frequency,
// period, wavelength in vacuum, wavelength in the medium (er, mur) and the speed in the medium. Any of the first four boxes typed sets the
// frequency (`OnFrequencyChange` ... `OnWavelengthMediumChange`, only for a value > 0) and `update` rewrites the others, each with
// `wxString( "" ) << value` ("%g").
//
// Pure: no React, no DOM.

/** `SPEED_LIGHT`. */
export const SPEED_LIGHT = 299792458;

export interface WavelengthValues {
  frequency: number;
  period: number;
  wavelengthVacuum: number;
  wavelengthMedium: number;
  speed: number;
}

/** `PANEL_WAVELENGTH::update`, in normalized units (Hz, s, m, m, m/s); the panel divides each by its unit's scale. */
export function wavelengthValues(aFrequency: number, permittivity: number, permeability: number): WavelengthValues {
  return {
    frequency: aFrequency,
    period: 1 / aFrequency,
    wavelengthVacuum: SPEED_LIGHT / aFrequency,
    wavelengthMedium: SPEED_LIGHT / aFrequency / Math.sqrt(permittivity * permeability),
    speed: SPEED_LIGHT / Math.sqrt(permittivity * permeability),
  };
}

/** The frequency each typed box gives (the value already multiplied by its unit's scale), as the panel's `On*Change` handlers compute it. */
export function frequencyFromPeriod(period: number): number {
  return 1 / period;
}

export function frequencyFromWavelengthVacuum(wavelength: number): number {
  return SPEED_LIGHT / wavelength;
}

export function frequencyFromWavelengthMedium(wavelength: number, permittivity: number, permeability: number): number {
  return SPEED_LIGHT / wavelength / Math.sqrt(permittivity * permeability);
}
