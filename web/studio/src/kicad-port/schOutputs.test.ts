import { test } from "node:test";
import assert from "node:assert/strict";
import { buildSchNetlistRequest, buildSchPlotRequest, DEFAULT_SCH_PLOT_FORM, SCH_COLOR_THEMES, SCH_NETLIST_FORMATS, SCH_PLOT_FORMATS, schPlotEnables, summarizeOutputs, withWarnings } from "./schOutputs";

// X9 (r2-editors-exports): the dialog's own initial state, not SCH_PLOT_OPTS' constructor.
test("the plot dialog starts as KiCad's: PDF, colour, KiCad Default (getColorSettings with no selection), drawing sheet, no background colour, the PDF options on", () => {
  assert.deepEqual(buildSchPlotRequest(DEFAULT_SCH_PLOT_FORM), {
    format: "pdf",
    color: true,
    theme: "_builtin_default",
    plot_drawing_sheet: true,
    background: false,
    pdf_property_popups: true,
    pdf_hierarchical_links: true,
    pdf_metadata: true,
    png_dpi: 300,
    png_antialias: true,
    plot_all: true,
    sheet_path: "",
    output_directory: "",
  });
  assert.deepEqual(SCH_PLOT_FORMATS.map((f) => f.label), ["Postscript", "PDF", "SVG", "DXF", "PNG"]);
  assert.deepEqual(SCH_COLOR_THEMES.map((t) => t.name), ["KiCad Classic", "KiCad Default"], "sorted by name");
});

test("buildSchPlotRequest: black and white PDF without the drawing sheet", () => {
  const body = buildSchPlotRequest({ ...DEFAULT_SCH_PLOT_FORM, format: "pdf", color: false, plotDrawingSheet: false });
  assert.equal(body.format, "pdf");
  assert.equal(body.color, false);
  assert.equal(body.plot_drawing_sheet, false);
  assert.equal(buildSchPlotRequest({ ...DEFAULT_SCH_PLOT_FORM, format: "png", pngDpi: 9000 }).png_dpi, 2400, "the spin control's range");
  assert.equal(buildSchPlotRequest({ ...DEFAULT_SCH_PLOT_FORM, outputDirectory: "plots\\sch" }).output_directory, "plots/sch");
});

test("buildSchPlotRequest: Plot Current Page sends the viewed sheet's placement path, Plot All Pages never does", () => {
  const form = { ...DEFAULT_SCH_PLOT_FORM, currentSheetIds: ["sheetA", "sheetB"] };
  assert.equal(buildSchPlotRequest(form, "current").sheet_path, "sheetA/sheetB");
  assert.equal(buildSchPlotRequest(form, "current").plot_all, false);
  assert.equal(buildSchPlotRequest(form, "all").sheet_path, "");
  assert.equal(buildSchPlotRequest(form, "all").plot_all, true);
  // viewing the root: an empty path (the root sheet)
  assert.equal(buildSchPlotRequest(DEFAULT_SCH_PLOT_FORM, "current").sheet_path, "");
});

test("onPlotFormatSelection and onColorMode", () => {
  assert.deepEqual(schPlotEnables(DEFAULT_SCH_PLOT_FORM), { pdfOptions: true, pngOptions: false, theme: true, background: true });
  assert.deepEqual(schPlotEnables({ ...DEFAULT_SCH_PLOT_FORM, format: "dxf" }), { pdfOptions: false, pngOptions: false, theme: true, background: false });
  assert.deepEqual(schPlotEnables({ ...DEFAULT_SCH_PLOT_FORM, format: "png", color: false }), { pdfOptions: false, pngOptions: true, theme: false, background: false });
});

test("buildSchNetlistRequest and the format list", () => {
  assert.deepEqual(buildSchNetlistRequest("spice"), { format: "spice" });
  assert.deepEqual(
    SCH_NETLIST_FORMATS.map((f) => [f.label, f.extension]),
    [
      ["KiCad", ".net"],
      ["OrcadPCB2", ".net"],
      ["Allegro", ".txt"],
      ["CadStar", ".frp"],
      ["PADS", ".asc"],
      ["SPICE", ".cir"],
      ["SPICE Model", ".cir"],
      ["Generic XML", ".xml"],
    ]
  );
});

test("summarizeOutputs", () => {
  assert.equal(summarizeOutputs([]), "Nothing was written.");
  assert.equal(summarizeOutputs(["export/a.svg"]), "Wrote export/a.svg");
  assert.equal(summarizeOutputs(["export/a.svg", "export/a-b.svg"]), "Wrote 2 files");
});

test("withWarnings: kicad-cli's warnings come before the dialog's report", () => {
  assert.equal(withWarnings({}, "Done."), "Done.");
  assert.equal(withWarnings({ warnings: ["Color theme 'x' not found, will use theme from PCB Editor."] }, "Plotted to 'a.gbr'.\nDone."), "Color theme 'x' not found, will use theme from PCB Editor.\nPlotted to 'a.gbr'.\nDone.");
  assert.equal(withWarnings({ warnings: ["W"] }, ""), "W");
});

