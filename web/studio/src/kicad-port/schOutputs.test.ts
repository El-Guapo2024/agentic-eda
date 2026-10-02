import { test } from "node:test";
import assert from "node:assert/strict";
import { buildSchNetlistRequest, buildSchPlotRequest, DEFAULT_SCH_PLOT_FORM, SCH_NETLIST_FORMATS, summarizeOutputs } from "./schOutputs";

test("buildSchPlotRequest: defaults mirror SCH_PLOT_OPTS (colour, drawing sheet, background, all pages)", () => {
  assert.deepEqual(buildSchPlotRequest(DEFAULT_SCH_PLOT_FORM), {
    format: "svg",
    color: true,
    plot_drawing_sheet: true,
    background: true,
    plot_all: true,
    sheet_path: "",
    page_size: "auto",
  });
});

test("buildSchPlotRequest: black and white PDF on A4 without the drawing sheet", () => {
  const body = buildSchPlotRequest({ ...DEFAULT_SCH_PLOT_FORM, format: "pdf", color: false, plotDrawingSheet: false, pageSize: "a4" });
  assert.equal(body.format, "pdf");
  assert.equal(body.color, false);
  assert.equal(body.plot_drawing_sheet, false);
  assert.equal(body.page_size, "a4");
});

test("buildSchPlotRequest: 'current page only' sends the viewed sheet's placement path, 'all' never does", () => {
  const form = { ...DEFAULT_SCH_PLOT_FORM, currentSheetIds: ["sheetA", "sheetB"] };
  assert.equal(buildSchPlotRequest({ ...form, scope: "current" }).sheet_path, "sheetA/sheetB");
  assert.equal(buildSchPlotRequest({ ...form, scope: "current" }).plot_all, false);
  assert.equal(buildSchPlotRequest({ ...form, scope: "all" }).sheet_path, "");
  assert.equal(buildSchPlotRequest({ ...form, scope: "all" }).plot_all, true);
  // viewing the root: an empty path (the root sheet)
  assert.equal(buildSchPlotRequest({ ...DEFAULT_SCH_PLOT_FORM, scope: "current" }).sheet_path, "");
});

test("buildSchNetlistRequest and the format list", () => {
  assert.deepEqual(buildSchNetlistRequest("xml"), { format: "xml" });
  assert.deepEqual(
    SCH_NETLIST_FORMATS.map((f) => [f.id, f.extension]),
    [
      ["kicad", ".net"],
      ["xml", ".xml"],
    ]
  );
});

test("summarizeOutputs", () => {
  assert.equal(summarizeOutputs([]), "Nothing was written.");
  assert.equal(summarizeOutputs(["export/a.svg"]), "Wrote export/a.svg");
  assert.equal(summarizeOutputs(["export/a.svg", "export/a-b.svg"]), "Wrote 2 files");
});
