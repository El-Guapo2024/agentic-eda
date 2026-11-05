// Request building for File > Plot... (eeschema's DIALOG_PLOT_SCHEMATIC, dialog_plot_schematic.cpp and dialog_plot_schematic_base.cpp ->
// SCH_PLOT_OPTS) and File > Export > Netlist... (DIALOG_EXPORT_NETLIST, dialog_export_netlist.cpp), KiCad 8303b2ad -- the pure half of
// PlotSchematicDialog.tsx / ExportNetlistDialog.tsx, split out so the form -> POST body mapping and the enable rules are unit-testable
// without a React render. Dependency-free on purpose (see tsconfig.test.json's note on what belongs under kicad-port/).
//
// The bodies are exactly what crates/cli/src/sch_output_api.rs reads: `plot` takes SCH_PLOT_OPTS' fields by name (m_plotAll -> plot_all,
// m_plotDrawingSheet -> plot_drawing_sheet, m_blackAndWhite -> !color, m_theme -> theme, m_useBackgroundColor -> background,
// m_PDFPropertyPopups / m_PDFHierarchicalLinks / m_PDFMetadata, m_pngDPI / m_pngAntialias), and maps each onto kicad-cli's flag;
// `netlist` takes the exporter page.
//
// kicad-cli is the only plot and netlist engine: the controls it has no flag for are shown disabled with the reason (`SCH_CLI_LIMITS`).

/** `m_plotFormatOptChoices`: Postscript, PDF, SVG, DXF, PNG (HPGL is no longer offered). */
export type SchPlotFormat = "ps" | "pdf" | "svg" | "dxf" | "png";
export type SchPlotScope = "all" | "current";

export const SCH_PLOT_FORMATS: ReadonlyArray<{ id: SchPlotFormat; label: string }> = [
  { id: "ps", label: "Postscript" },
  { id: "pdf", label: "PDF" },
  { id: "svg", label: "SVG" },
  { id: "dxf", label: "DXF" },
  { id: "png", label: "PNG" },
];

/** "Color theme:" -- `SETTINGS_MANAGER::GetColorSettingsList()`, sorted by name, of the themes kicad-cli knows (it runs with a settings folder of
 * its own, so the two built-in ones), by the key `--theme` takes. Outside job mode the plot uses KiCad Default: `TransferDataToWindow` never
 * selects a theme (the `SetSelection( 0 )` fallback at dialog_plot_schematic.cpp:123-124 is the job branch's, and the base's `SetSelection( 0 )`
 * runs on the still empty choice), so `getColorSettings()` sees `selection < 0` and returns `::GetColorSettings( COLOR_BUILTIN_DEFAULT )`
 * (dialog_plot_schematic.cpp:280-289; `COLOR_BUILTIN_DEFAULT = "_builtin_default"`, color_settings.cpp:34). The studio shows that theme
 * selected rather than wx's blank choice. */
export const SCH_COLOR_THEMES: ReadonlyArray<{ key: string; name: string }> = [
  { key: "_builtin_classic", name: "KiCad Classic" },
  { key: "_builtin_default", name: "KiCad Default" },
];

/** "Page size:" (`m_paperSizeOptionChoices`). */
export const SCH_PAGE_SIZES = ["Schematic size", "A4", "A"] as const;

/** Why a control of the dialog is disabled here. */
export const SCH_CLI_LIMITS = {
  pageSize: "kicad-cli plots at the schematic's own page size (it has no page size flag)",
  lineWidth: "kicad-cli has no flag for the minimum line width",
  dxfUnits: "kicad-cli always exports DXF in mm",
  openFile: "the studio runs in a browser and does not open files on the server; the files are listed below",
  variant: "the studio has no design variants",
  browse: "the studio runs in a browser: type the folder, relative to the board's folder",
} as const;

/** The dialog's own state, one field per control. */
export interface SchPlotForm {
  format: SchPlotFormat;
  /** "Output mode:" Color (true) or Black and White (`m_blackAndWhite`). */
  color: boolean;
  /** A `SCH_COLOR_THEMES` key. */
  theme: string;
  plotDrawingSheet: boolean;
  useBackgroundColor: boolean;
  pdfPropertyPopups: boolean;
  pdfHierarchicalLinks: boolean;
  pdfMetadata: boolean;
  pngDpi: number;
  pngAntialias: boolean;
  /** "Output directory:", relative to the board's folder; empty is the studio's `export/kicad/sch-<format>/`. */
  outputDirectory: string;
  /** Placement ids from the root down to the sheet being viewed (`Schematic.sheet_path`), used for Plot Current Page. */
  currentSheetIds: string[];
}

/** What `DIALOG_PLOT_SCHEMATIC` starts with (`dialog_plot_schematic_base.cpp`): PDF (`m_plotFormatOpt->SetSelection( 1 )`), Color, KiCad
 * Default (see `SCH_COLOR_THEMES`), "Plot drawing sheet" checked, "Plot background color" unchecked (`m_plotBackgroundColor` has no SetValue), the three PDF options
 * checked, 300 DPI anti-aliased. */
export const DEFAULT_SCH_PLOT_FORM: SchPlotForm = {
  format: "pdf",
  color: true,
  theme: "_builtin_default",
  plotDrawingSheet: true,
  useBackgroundColor: false,
  pdfPropertyPopups: true,
  pdfHierarchicalLinks: true,
  pdfMetadata: true,
  pngDpi: 300,
  pngAntialias: true,
  outputDirectory: "",
  currentSheetIds: [],
};

/** `onPlotFormatSelection` and `onColorMode`: the PDF options for PDF, the PNG options for PNG, the theme in colour, the background colour in
 * colour for PostScript, PDF, SVG and PNG. */
export function schPlotEnables(form: SchPlotForm): { pdfOptions: boolean; pngOptions: boolean; theme: boolean; background: boolean } {
  const f = form.format;
  return { pdfOptions: f === "pdf", pngOptions: f === "png", theme: form.color, background: form.color && (f === "ps" || f === "pdf" || f === "svg" || f === "png") };
}

/** The `POST /api/sch/plot` body. */
export interface SchPlotRequest {
  format: SchPlotFormat;
  color: boolean;
  theme: string;
  plot_drawing_sheet: boolean;
  background: boolean;
  pdf_property_popups: boolean;
  pdf_hierarchical_links: boolean;
  pdf_metadata: boolean;
  png_dpi: number;
  png_antialias: boolean;
  plot_all: boolean;
  /** `"<id>/<id>/..."` -- only meaningful when `plot_all` is false; empty = the root sheet. */
  sheet_path: string;
  output_directory: string;
}

/** "Plot All Pages" (`OnPlotAll`) or "Plot Current Page" (`OnPlotCurrent`). */
export function buildSchPlotRequest(form: SchPlotForm, scope: SchPlotScope = "all"): SchPlotRequest {
  const plotAll = scope === "all";
  return {
    format: form.format,
    color: form.color,
    theme: form.theme,
    plot_drawing_sheet: form.plotDrawingSheet,
    // `m_useBackgroundColor` only has an effect in colour mode (plotOneSheetSVG/PDF: `&& plotter->GetColorMode()`).
    background: form.useBackgroundColor,
    pdf_property_popups: form.pdfPropertyPopups,
    pdf_hierarchical_links: form.pdfHierarchicalLinks,
    pdf_metadata: form.pdfMetadata,
    png_dpi: Math.min(2400, Math.max(72, Math.round(form.pngDpi))),
    png_antialias: form.pngAntialias,
    plot_all: plotAll,
    sheet_path: plotAll ? "" : form.currentSheetIds.join("/"),
    output_directory: form.outputDirectory.replace(/\\/g, "/"),
  };
}

/** DIALOG_EXPORT_NETLIST's pages, in the notebook's order (`PANELPCBNEW` .. `PANELSPICEMODEL`), each with its label and `FilenamePrms`'
 * extension; then the generic XML (kicad-cli's `kicadxml`, which feeds the BOM and custom exporters but has no page of its own). */
export type SchNetlistFormat = "kicad" | "orcadpcb2" | "allegro" | "cadstar" | "pads" | "spice" | "spicemodel" | "xml";

export const SCH_NETLIST_FORMATS: ReadonlyArray<{ id: SchNetlistFormat; label: string; text: string; extension: string }> = [
  { id: "kicad", label: "KiCad", text: "Export netlist in KiCad format", extension: ".net" },
  { id: "orcadpcb2", label: "OrcadPCB2", text: "Export netlist in OrcadPCB2 format", extension: ".net" },
  { id: "allegro", label: "Allegro", text: "Export netlist in Allegro format", extension: ".txt" },
  { id: "cadstar", label: "CadStar", text: "Export netlist in CadStar format", extension: ".frp" },
  { id: "pads", label: "PADS", text: "Export netlist in PADS format", extension: ".asc" },
  { id: "spice", label: "SPICE", text: "Export netlist in SPICE format", extension: ".cir" },
  { id: "spicemodel", label: "SPICE Model", text: "Export netlist as a SPICE .subckt model", extension: ".cir" },
  { id: "xml", label: "Generic XML", text: "Export the intermediate netlist (kicad-cli's kicadxml)", extension: ".xml" },
];

/** The SPICE page's controls (`InstallPageSpice`, `InstallPageSpiceModel`) with their tips. kicad-cli's netlist job writes the simulation command
 * and none of these (`EESCHEMA_JOBS_HANDLER::JobExportNetlist`: `OPTION_SIM_COMMAND`), so they stay as the page starts, unchecked. */
export const SPICE_OPTIONS: ReadonlyArray<{ label: string; tip: string; models: boolean }> = [
  { label: "Use current sheet as root", tip: "Export netlist only for the current sheet", models: true },
  { label: "Save all voltages", tip: "Write a directive to save all voltages (.save all)", models: false },
  { label: "Save all currents", tip: "Write a directive to save all currents (.probe alli)", models: false },
  { label: "Save all power dissipations", tip: "Write directives to save power dissipation of all items (.probe p(<item>))", models: false },
  { label: "Save all digital event data", tip: "If not set, write a directive to prevent the saving of digital event data (esave none)", models: false },
];

export const NETLIST_CLI_LIMITS = {
  spice: "kicad-cli sch export netlist has no flag for it: the netlist is written with the simulation command only, as with every box unchecked",
  simulator: "the studio runs no external simulator command",
  exporters: "custom exporters run external scripts, which the studio does not",
} as const;

/** The `POST /api/sch/netlist` body. */
export interface SchNetlistRequest {
  format: SchNetlistFormat;
}

export function buildSchNetlistRequest(format: SchNetlistFormat): SchNetlistRequest {
  return { format };
}

/** One-line summary of a finished plot/export for the dialog's result box. */
export function summarizeOutputs(files: readonly string[]): string {
  if (files.length === 0) return "Nothing was written.";
  if (files.length === 1) return `Wrote ${files[0]}`;
  return `Wrote ${files.length} files`;
}
