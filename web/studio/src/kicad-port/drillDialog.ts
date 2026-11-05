// The logic of Generate Drill Files (File > Fabrication Outputs > Drill Files..., pcbnew/dialogs/dialog_gendrill.cpp and
// dialog_gendrill_base.cpp, KiCad 8303b2ad), as pure functions: the controls and their defaults, what the format enables
// (`onFileFormatSelection`), the precision shown (`updatePrecisionOptions`), and the request Generate and "Generate Report File..." send
// (POST /api/fab/drill and /api/fab/drill_report, crates/cli/src/fab_api.rs `drill_dialog_args`, which map each control onto
// `kicad-cli pcb export drill`'s flag). The Origin and the output folder are the board's plot settings (`m_plotOpts`), stored back on
// Generate (`DIALOG_GENDRILL::updateConfig`) through the `set_plot_settings` verb. GenerateDrillDialog.tsx is the thin view.
//
// Dependency-free on purpose (tsconfig.test.json).

/** The dialog's controls. The format's, units', zeros' and map's choices are kept between openings, as `DIALOG_SHIM` keeps them. */
export interface DrillForm {
  /** `m_rbExcellon` / `m_rbGerberX2`. */
  format: "excellon" | "gerber";
  mirrorY: boolean;
  minimalHeader: boolean;
  mergePthNpth: boolean;
  altDrillMode: boolean;
  generateTenting: boolean;
  generateMap: boolean;
  /** `m_choiceDrillMap`: Postscript, Gerber X2, DXF, SVG, PDF. */
  mapFormat: "ps" | "gerberx2" | "dxf" | "svg" | "pdf";
  /** "Origin:": Absolute, or the drill/place file origin. */
  useAuxOrigin: boolean;
  /** "Units:": Millimeters (0) or Inches (1). */
  unitsMm: boolean;
  /** "Zeros:" (`EXCELLON_WRITER::ZEROS_FMT`'s order). */
  zeros: "decimal" | "suppressleading" | "suppresstrailing" | "keep";
  outputDirectory: string;
}

export const MAP_FORMATS: ReadonlyArray<{ id: DrillForm["mapFormat"]; label: string }> = [
  { id: "ps", label: "Postscript" },
  { id: "gerberx2", label: "Gerber X2" },
  { id: "dxf", label: "DXF" },
  { id: "svg", label: "SVG" },
  { id: "pdf", label: "PDF" },
];

export const ZEROS: ReadonlyArray<{ id: DrillForm["zeros"]; label: string }> = [
  { id: "decimal", label: "Decimal format (recommended)" },
  { id: "suppressleading", label: "Suppress leading zeros" },
  { id: "suppresstrailing", label: "Suppress trailing zeros" },
  { id: "keep", label: "Keep zeros" },
];

/** `dialog_gendrill_base.cpp`'s defaults: Excellon, nothing checked, map in Gerber X2 (`SetSelection( 1 )`), Absolute, Millimeters, decimal. */
export const DEFAULT_DRILL_FORM: DrillForm = {
  format: "excellon",
  mirrorY: false,
  minimalHeader: false,
  mergePthNpth: false,
  altDrillMode: false,
  generateTenting: false,
  generateMap: false,
  mapFormat: "gerberx2",
  useAuxOrigin: false,
  unitsMm: true,
  zeros: "decimal",
  outputDirectory: "",
};

/** `DIALOG_GENDRILL::TransferDataToWindow`: the Origin and the output folder are the board's plot settings' (`m_plotOpts`); the rest keep
 * what the dialog last had. */
export function drillFormAtOpening(kept: DrillForm, board: { use_aux_origin?: boolean; options?: Readonly<Record<string, string>> } | null | undefined): DrillForm {
  return { ...kept, useAuxOrigin: board?.use_aux_origin ?? false, outputDirectory: board?.options?.outputdirectory ?? "" };
}

/** `onFileFormatSelection`: the Excellon controls for Excellon, "Generate tenting layers" for Gerber X2. */
export function drillEnables(form: DrillForm): { excellon: boolean; tenting: boolean } {
  return { excellon: form.format === "excellon", tenting: form.format === "gerber" };
}

/** "Precision:" (`updatePrecisionOptions`, `onFileFormatSelection`): Excellon's `precisionListForMetric` (3:3) or `precisionListForInches`
 * (2:4), disabled for the decimal format, which uses none; Gerber X2's 4.6 or 4.5 from the board's `gerberprecision`. */
export function drillPrecision(form: DrillForm, gerberPrecision: number): { text: string; enabled: boolean } {
  if (form.format === "gerber") return { text: gerberPrecision === 6 ? "4.6" : "4.5", enabled: true };
  return { text: form.unitsMm ? "3:3" : "2:4", enabled: form.zeros !== "decimal" };
}

/** The POST /api/fab/drill (and /api/fab/drill_report) body. */
export function drillRequest(form: DrillForm): Record<string, unknown> {
  return {
    format: form.format,
    mirror_y: form.mirrorY,
    minimal_header: form.minimalHeader,
    merge_pth_npth: form.mergePthNpth,
    alt_drill_mode: form.altDrillMode,
    units_mm: form.unitsMm,
    zeros: form.zeros,
    generate_tenting: form.generateTenting,
    generate_map: form.generateMap,
    map_format: form.mapFormat,
    use_aux_origin: form.useAuxOrigin,
  };
}

/** `updateConfig`: the Origin and the output folder stored back when they changed (`SetUseAuxOrigin`, `SetOutputDirectory` with forward
 * slashes, `IsSameAs`), else null. */
export function drillSettingsCommand(form: DrillForm, board: { use_aux_origin?: boolean; options?: Readonly<Record<string, string>> } | null | undefined): { op: "set_plot_settings"; use_aux_origin?: boolean; options?: Record<string, string> } | null {
  const dir = form.outputDirectory.replace(/\\/g, "/");
  const auxChanged = form.useAuxOrigin !== (board?.use_aux_origin ?? false);
  const dirChanged = dir !== (board?.options?.outputdirectory ?? "");
  if (!auxChanged && !dirChanged) return null;
  return { op: "set_plot_settings", ...(auxChanged ? { use_aux_origin: form.useAuxOrigin } : {}), ...(dirChanged ? { options: { outputdirectory: dir } } : {}) };
}
