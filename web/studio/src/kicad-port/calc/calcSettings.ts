// The Calculator Tools' settings (`PCB_CALCULATOR_SETTINGS`, pcb_calculator/pcb_calculator_settings.{h,cpp} at KiCad 25ec2335): what KiCad writes to
// `pcb_calculator.json` in the user's settings folder, with its keys, nesting and defaults. KiCad loads them into the panels when the frame opens
// (`PCB_CALCULATOR_FRAME::LoadSettings` -> each `CALCULATOR_PANEL::LoadSettings`) and stores them back when it closes; the studio keeps the same object
// in the browser's local storage (state/pcbCalculator.ts) and each panel writes its part as it changes.
//
// The wxString params stay strings: KiCad keeps what was typed, not the number read from it. `regulators.data_file` names the regulator list's file;
// a browser page has no file path, so it holds the name of the file last opened or saved (the list itself is kept beside it, kicad-port/calc/regulators.ts).
// `MigrateFromLegacy` (wxConfig of KiCad 5) is not ported: there is no legacy configuration in a browser.
//
// Pure: no React, no DOM.

/** `PCB_CALCULATOR_SETTINGS::ATTENUATOR`. */
export interface AttenuatorSettings {
  attenuation: number;
  zin: number;
  zout: number;
}

/** The four attenuators' names in the settings (`att_names`). */
export const ATTENUATOR_NAMES = ["att_pi", "att_tee", "att_bridge", "att_splitter"] as const;
export type AttenuatorName = (typeof ATTENUATOR_NAMES)[number];

/** The nine transmission lines' names in the settings (`transline_names`), in `TRANSLINE_TYPE_ID` order. */
export const TRANSLINE_NAMES = ["MicroStrip", "CoPlanar", "GrCoPlanar", "RectWaveGuide", "Coax", "Coupled_MicroStrip", "Coupled_Stripline", "StripLine", "TwistedPair"] as const;
export type TranslineName = (typeof TRANSLINE_NAMES)[number];

export interface PcbCalculatorSettings {
  board_class_units: number;
  color_code_tolerance: number;
  /** The tree page shown last (`m_treebook->GetSelection()`: the group rows count, so 1 is Regulators). */
  last_page: number;
  translines: {
    type: number;
    dielectric_model: number;
    spec_frequency: string;
    spec_frequency_unit: number;
    soldermask_present: number;
    soldermask_thickness: number;
    soldermask_epsilonr: number;
    soldermask_tand: number;
    soldermask_fills_gaps: number;
  };
  attenuators: { type: number } & Record<AttenuatorName, AttenuatorSettings>;
  electrical: {
    spacing_units: number;
    spacing_voltage: string;
    iec60664_ratedVoltage: number;
    iec60664_OVC: number;
    iec60664_RMSvoltage: number;
    iec60664_transientOV: number;
    iec60664_peakOV: number;
    iec60664_insulationType: number;
    iec60664_pollutionDegree: number;
    iec60664_materialGroup: number;
    iec60664_pcbMaterial: number;
    iec60664_altitude: number;
  };
  regulators: {
    resTol: string;
    r1: string;
    r2: string;
    vrefMin: string;
    vrefTyp: string;
    vrefMax: string;
    voutTyp: string;
    iadjTyp: string;
    iadjMax: string;
    data_file: string;
    selected_regulator: string;
    type: number;
    last_param: number;
  };
  cable_size: {
    conductorMaterialResitivity: string;
    conductorTemperature: string;
    conductorThermalCoef: string;
    currentDensityChoice: number;
    diameterUnit: number;
    linResUnit: number;
    frequencyUnit: number;
    lengthUnit: number;
  };
  wavelength: {
    frequency: number;
    permeability: number;
    permittivity: number;
    frequencyUnit: number;
    periodUnit: number;
    wavelengthVacuumUnit: number;
    wavelengthMediumUnit: number;
    speedUnit: number;
  };
  track_width: {
    current: string;
    delta_tc: string;
    track_len: string;
    track_len_units: number;
    resistivity: string;
    ext_track_width: string;
    ext_track_width_units: number;
    ext_track_thickness: string;
    ext_track_thickness_units: number;
    int_track_width: string;
    int_track_width_units: number;
    int_track_thickness: string;
    int_track_thickness_units: number;
  };
  /** `trans_line.<name>.values` / `.units`: `PARAM_MAP<double>` / `PARAM_MAP<int>` keyed by the parameter's `PRMS_ID` written as a string. */
  trans_line: Record<TranslineName, { values: Record<string, number>; units: Record<string, number> }>;
  via_size: {
    hole_diameter: string;
    hole_diameter_units: number;
    thickness: string;
    thickness_units: number;
    length: string;
    length_units: number;
    pad_diameter: string;
    pad_diameter_units: number;
    clearance_diameter: string;
    clearance_diameter_units: number;
    characteristic_impedance: string;
    characteristic_impedance_units: number;
    applied_current: string;
    plating_resistivity: string;
    permittivity: string;
    temp_rise: string;
    pulse_rise_time: string;
  };
  corrosion_table: {
    threshold_voltage: string;
    show_symbols: boolean;
  };
}

/** Default value for TI LM317 (`DEFAULT_REGULATOR_*`). */
export const DEFAULT_REGULATOR = {
  resTol: "1",
  r1: "0.240",
  r2: "0.720",
  vrefMin: "1.20",
  vrefTyp: "1.25",
  vrefMax: "1.30",
  voutTyp: "5",
  iadjTyp: "50",
  iadjMax: "100",
} as const;

const att = (): AttenuatorSettings => ({ attenuation: 6.0, zin: 50.0, zout: 50.0 });

/** The constructor's `PARAM` defaults. */
export function defaultPcbCalculatorSettings(): PcbCalculatorSettings {
  const trans_line = {} as PcbCalculatorSettings["trans_line"];
  for (const n of TRANSLINE_NAMES) trans_line[n] = { values: {}, units: {} };
  return {
    board_class_units: 0,
    color_code_tolerance: 0,
    last_page: 1,
    translines: {
      type: 0,
      dielectric_model: 0,
      spec_frequency: "1",
      spec_frequency_unit: 0,
      soldermask_present: 0,
      soldermask_thickness: 20.0e-6,
      soldermask_epsilonr: 3.5,
      soldermask_tand: 0.025,
      soldermask_fills_gaps: 1,
    },
    attenuators: { type: 0, att_pi: att(), att_tee: att(), att_bridge: att(), att_splitter: att() },
    electrical: {
      spacing_units: 0,
      spacing_voltage: "500",
      iec60664_ratedVoltage: 230,
      iec60664_OVC: 0,
      iec60664_RMSvoltage: 230,
      iec60664_transientOV: 1,
      iec60664_peakOV: 0.5,
      iec60664_insulationType: 0,
      iec60664_pollutionDegree: 0,
      iec60664_materialGroup: 0,
      iec60664_pcbMaterial: 1,
      iec60664_altitude: 2000,
    },
    regulators: { ...DEFAULT_REGULATOR, data_file: "", selected_regulator: "", type: 1, last_param: 0 },
    cable_size: {
      conductorMaterialResitivity: "",
      conductorTemperature: "",
      conductorThermalCoef: "",
      currentDensityChoice: 0,
      diameterUnit: 0,
      linResUnit: 0,
      frequencyUnit: 0,
      lengthUnit: 0,
    },
    wavelength: { frequency: 1e9, permeability: 1, permittivity: 4.5, frequencyUnit: 0, periodUnit: 0, wavelengthVacuumUnit: 0, wavelengthMediumUnit: 0, speedUnit: 0 },
    track_width: {
      current: "1.0",
      delta_tc: "10.0",
      track_len: "20",
      track_len_units: 0,
      resistivity: "1.72e-8",
      ext_track_width: "0.2",
      ext_track_width_units: 0,
      ext_track_thickness: "35",
      ext_track_thickness_units: 1,
      int_track_width: "0.2",
      int_track_width_units: 0,
      int_track_thickness: "35",
      int_track_thickness_units: 1,
    },
    trans_line,
    via_size: {
      hole_diameter: "0.4",
      hole_diameter_units: 0,
      thickness: "0.035",
      thickness_units: 0,
      length: "1.6",
      length_units: 0,
      pad_diameter: "0.6",
      pad_diameter_units: 0,
      clearance_diameter: "1.0",
      clearance_diameter_units: 0,
      characteristic_impedance: "50",
      characteristic_impedance_units: 0,
      applied_current: "1",
      plating_resistivity: "1.72e-8",
      permittivity: "4.5",
      temp_rise: "10",
      pulse_rise_time: "1",
    },
    corrosion_table: { threshold_voltage: "0", show_symbols: true },
  };
}

function isObj(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

/**
 * Reads a stored object onto the defaults, the way `JSON_SETTINGS::Load` fills each `PARAM` from its path: a key of the wrong type, or missing, keeps the
 * default; keys KiCad does not have are dropped. The `PARAM_MAP`s take every entry whose value has the map's type.
 */
export function sanitizePcbCalculatorSettings(raw: unknown): PcbCalculatorSettings {
  const out = defaultPcbCalculatorSettings();
  if (!isObj(raw)) return out;
  const fill = (dst: Record<string, unknown>, src: unknown): void => {
    if (!isObj(src)) return;
    for (const [k, d] of Object.entries(dst)) {
      const v = src[k];
      if (v === undefined) continue;
      if (isObj(d)) fill(d, v);
      else if (typeof d === typeof v && (typeof v !== "number" || Number.isFinite(v))) dst[k] = v;
    }
  };
  const { trans_line, ...rest } = out;
  fill(rest as unknown as Record<string, unknown>, raw);
  const tl = isObj(raw.trans_line) ? raw.trans_line : {};
  for (const n of TRANSLINE_NAMES) {
    const src = tl[n];
    if (!isObj(src)) continue;
    for (const [field, want] of [["values", "double"], ["units", "int"]] as const) {
      const m = src[field];
      if (!isObj(m)) continue;
      for (const [k, v] of Object.entries(m)) {
        if (typeof v === "number" && Number.isFinite(v) && (want === "double" || Number.isInteger(v))) trans_line[n][field][k] = v;
      }
    }
  }
  return { ...(rest as Omit<PcbCalculatorSettings, "trans_line">), trans_line };
}
