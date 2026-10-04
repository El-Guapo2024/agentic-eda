// Request building for File > Plot... (eeschema's DIALOG_PLOT_SCHEMATIC,
// dialog_plot_schematic.cpp -> SCH_PLOT_OPTS) and File > Export > Netlist...
// (DIALOG_EXPORT_NETLIST) -- the pure half of PlotSchematicDialog.tsx /
// ExportNetlistDialog.tsx, split out so the form -> POST body mapping is
// unit-testable without a React render. Dependency-free on purpose (see
// tsconfig.test.json's note on what belongs under kicad-port/).
//
// The bodies are exactly what crates/cli/src/sch_api.rs reads: `plot`
// takes SCH_PLOT_OPTS' fields by name (m_plotAll -> plot_all,
// m_plotDrawingSheet -> plot_drawing_sheet, m_blackAndWhite -> !color,
// m_useBackgroundColor -> background), and
// `netlist` takes the exporter flavour.

export type SchPlotFormat = "svg" | "pdf";
export type SchPlotScope = "all" | "current";
export type SchNetlistFormat = "kicad" | "xml";

/** The dialog's own state, one field per control. */
export interface SchPlotForm {
  format: SchPlotFormat;
  /** `true` = colour theme, `false` = Black and white (`m_blackAndWhite`). */
  color: boolean;
  plotDrawingSheet: boolean;
  useBackgroundColor: boolean;
  /** "Plot all pages" vs "Plot current page only" (`m_plotAll`). */
  scope: SchPlotScope;
  /** Placement ids from the root down to the sheet being viewed (`Schematic.sheet_path`), used for scope `current`. */
  currentSheetIds: string[];
}

/** `SCH_PLOT_OPTS::SCH_PLOT_OPTS`'s own defaults (colour, drawing sheet, background, all pages), SVG first. */
export const DEFAULT_SCH_PLOT_FORM: SchPlotForm = {
  format: "svg",
  color: true,
  plotDrawingSheet: true,
  useBackgroundColor: true,
  scope: "all",
  currentSheetIds: [],
};

/** The `POST /api/sch/plot` body. */
export interface SchPlotRequest {
  format: SchPlotFormat;
  color: boolean;
  plot_drawing_sheet: boolean;
  background: boolean;
  plot_all: boolean;
  /** `"<id>/<id>/..."` -- only meaningful when `plot_all` is false; empty = the root sheet. */
  sheet_path: string;
}

export function buildSchPlotRequest(form: SchPlotForm): SchPlotRequest {
  const plotAll = form.scope === "all";
  return {
    format: form.format,
    color: form.color,
    plot_drawing_sheet: form.plotDrawingSheet,
    // `m_useBackgroundColor` only has an effect in colour mode (plotOneSheetSVG/PDF: `&& plotter->GetColorMode()`).
    background: form.useBackgroundColor,
    plot_all: plotAll,
    sheet_path: plotAll ? "" : form.currentSheetIds.join("/"),
  };
}

/** The `POST /api/sch/netlist` body. */
export interface SchNetlistRequest {
  format: SchNetlistFormat;
}

export function buildSchNetlistRequest(format: SchNetlistFormat): SchNetlistRequest {
  return { format };
}

/** DIALOG_EXPORT_NETLIST's own format tabs: "KiCad" (`.net`, the `(export (version "E") ...)` s-expression) and the generic XML (`.xml`). */
export const SCH_NETLIST_FORMATS: ReadonlyArray<{ id: SchNetlistFormat; label: string; extension: string }> = [
  { id: "kicad", label: "KiCad", extension: ".net" },
  { id: "xml", label: "Generic XML (intermediate netlist)", extension: ".xml" },
];

/** One-line summary of a finished plot/export for the dialog's result box. */
export function summarizeOutputs(files: readonly string[]): string {
  if (files.length === 0) return "Nothing was written.";
  if (files.length === 1) return `Wrote ${files[0]}`;
  return `Wrote ${files.length} files`;
}
