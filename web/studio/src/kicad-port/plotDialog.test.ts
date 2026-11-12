import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyLayerMenu,
  arrangeAllLayers,
  constrainPostscript,
  DEFAULT_SESSION,
  loadPsControls,
  noLayerSelected,
  savePsControls,
  drcWarning,
  formatOfOutputformat,
  initialPlotForm,
  optionsFromForm,
  plotEnables,
  plotRequest,
  plotSettingsCommand,
  showSolderMaskAlert,
  stackupForPlotting,
  uiOrder,
  withDxfContours,
  withFormat,
} from "./plotDialog";

const LISTED = ["F.Cu", "In1.Cu", "B.Cu", "F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab", "User.1", "User.2"];
const BOARD = { layers: ["F.Cu", "In1.Cu", "B.Cu", "F.Mask", "B.Mask", "Edge.Cuts"], use_aux_origin: false };

test("the dialog opens as TransferDataToWindow reads the board: Gerber, no drill marks, 1:1, KiCad's Gerber options", () => {
  const f = initialPlotForm(BOARD, LISTED, DEFAULT_SESSION);
  assert.equal(f.format, "gerber");
  assert.deepEqual(f.checked, ["F.Cu", "In1.Cu", "B.Cu", "F.Mask", "B.Mask", "Edge.Cuts"], "in UIOrder");
  // Gerber: SetPlotFormat sets drill marks None and the scale 1:1, whatever the board says (drillshape 1, Small).
  assert.equal(f.drillMarks, 0);
  assert.equal(f.scale, 1);
  assert.deepEqual([f.protelExt, f.jobFile, f.coordFormat, f.x2, f.netAttributes, f.disableMacros], [false, true, 6, true, true, false]);
  // DNP: sketch and cross-out on by default, so "Indicate DNP" is checked with Cross-out.
  assert.deepEqual([f.plotDNP, f.dnpMode], [true, "crossout"]);
  assert.equal(f.zoneFillCheck, true);
  assert.deepEqual([f.dxfContours, f.dxfUnits, f.svgPrecision, f.pngDpi, f.pngAntialias], [true, "in", 4, 300, true]);
});

test("each format enables what SetPlotFormat enables, and kicad-cli's limits are named", () => {
  const f = initialPlotForm(BOARD, LISTED, DEFAULT_SESSION);
  const g = plotEnables(f);
  assert.deepEqual([g.drillMarks, g.mirror, g.scale, g.negative, g.useAuxOrigin, g.psAdjust], [false, false, false, false, true, false]);
  const ps = withFormat({ ...f, useAuxOrigin: true }, "ps");
  assert.equal(ps.useAuxOrigin, false, "PostScript turns the drill/place file origin off");
  assert.deepEqual([plotEnables(ps).psAdjust, plotEnables(ps).forceA4, plotEnables(ps).useAuxOrigin, plotEnables(ps).auxOriginCliReason], [true, true, false, null]);
  const pdf = plotEnables(withFormat(f, "pdf"));
  assert.equal(pdf.useAuxOrigin, false);
  assert.match(pdf.auxOriginCliReason ?? "", /no flag for the drill\/place file origin/);
  const dxf = withFormat({ ...f, mirror: true, negative: true }, "dxf");
  assert.deepEqual([dxf.mirror, dxf.negative, plotEnables(dxf).useAuxOrigin, plotEnables(dxf).dxfContours], [false, false, true, true]);
  assert.equal(withDxfContours({ ...dxf, dxfKiCadFont: false }, true).dxfKiCadFont, true, "OnChangeDXFPlotMode forces the KiCad font");
  assert.equal(withFormat({ ...f, forceA4: true }, "svg").forceA4, false);
  assert.equal(plotEnables({ ...f, plotDNP: false }).dnpRadios, false, "onDNPCheckbox");
  assert.equal(plotEnables({ ...f, sketchPads: false }).padNumbers, false, "onSketchPads");
  assert.equal(plotEnables({ ...f, format: "pdf", pdfBlackAndWhite: true }).pdfBackground, false, "updatePdfColorOptions");
});

test("applyPlotSettings: a fresh PCB_PLOT_PARAMS, so the PDF options outside PDF are the defaults; black and white per format", () => {
  const f = withFormat(initialPlotForm(BOARD, LISTED, DEFAULT_SESSION), "svg");
  const o = optionsFromForm({ ...f, svgBlackAndWhite: false, pdfFrontPopups: false, pdfSingle: true, outputDirectory: "plots\\svg" });
  assert.equal(o.outputformat, "5");
  assert.equal(o.plot_black_and_white, "no", "the SVG Output mode");
  assert.equal(o.pdf_front_fp_property_popups, "yes", "not PDF: the default");
  assert.equal(o.pdf_single_document, "no");
  assert.equal(o.outputdirectory, "plots/svg");
  assert.equal(optionsFromForm(withFormat(f, "png")).plot_black_and_white, "no", "PNG never");
  assert.equal(optionsFromForm(withFormat(f, "ps")).plot_black_and_white, "yes", "PostScript always");
  const pdf = optionsFromForm({ ...withFormat(f, "pdf"), pdfFrontPopups: false, pdfSingle: true, pdfBlackAndWhite: false });
  assert.deepEqual([pdf.pdf_front_fp_property_popups, pdf.pdf_single_document, pdf.plot_black_and_white, pdf.outputformat], ["no", "yes", "no", "4"]);
  const dnp = optionsFromForm({ ...f, plotDNP: true, dnpMode: "hide" });
  assert.deepEqual([dnp.hidednponfab, dnp.sketchdnponfab, dnp.crossoutdnponfab], ["yes", "no", "no"]);
  const none = optionsFromForm({ ...f, plotDNP: false });
  assert.deepEqual([none.hidednponfab, none.sketchdnponfab, none.crossoutdnponfab], ["no", "no", "no"]);
});

test("Plot sends only what changed (IsSameAs), and nothing when nothing did", () => {
  const options = optionsFromForm(initialPlotForm(BOARD, LISTED, DEFAULT_SESSION));
  const board = { ...BOARD, options, plot_on_all_layers: [] };
  const f = initialPlotForm(board, LISTED, DEFAULT_SESSION);
  assert.equal(plotSettingsCommand(f, board, LISTED), null);
  const cmd = plotSettingsCommand({ ...f, plotSheetRef: true, checked: f.checked.filter((l) => l !== "B.Mask"), allLayersChecked: ["Edge.Cuts"] }, board, LISTED);
  assert.deepEqual(cmd, { op: "set_plot_settings", layers: ["F.Cu", "In1.Cu", "B.Cu", "F.Mask", "Edge.Cuts"], plot_on_all_layers: ["Edge.Cuts"], options: { plotframeref: "yes" } });
  // A board whose block says drillshape 1 opens in Gerber with None, so the first Plot stores 0, as KiCad's does.
  const fresh = { ...BOARD, options: { drillshape: "1" } };
  assert.equal(plotSettingsCommand(initialPlotForm(fresh, LISTED, DEFAULT_SESSION), fresh, LISTED)?.options?.drillshape, "0");
});

test("the layer lists: UIOrder, the stackup order for plotting, and the right-click menus", () => {
  assert.deepEqual(uiOrder(["B.Fab", "Edge.Cuts", "User.1", "B.Cu", "F.Cu", "In1.Cu", "F.SilkS"]), ["F.Cu", "In1.Cu", "B.Cu", "F.SilkS", "Edge.Cuts", "B.Fab", "User.1"]);
  assert.deepEqual(stackupForPlotting(["F.Cu", "In1.Cu", "In2.Cu", "B.Cu", "F.Mask", "B.Mask", "F.SilkS", "Dwgs.User", "User.1", "User.2", "Edge.Cuts", "Margin"]), [
    "B.Cu",
    "B.Mask",
    "In2.Cu",
    "In1.Cu",
    "F.Cu",
    "F.Mask",
    "F.SilkS",
    "Dwgs.User",
    "User.2",
    "User.1",
    "Margin",
    "Edge.Cuts",
  ]);
  assert.deepEqual(arrangeAllLayers(["B.Cu", "F.Cu", "F.Mask", "Edge.Cuts"], ["Edge.Cuts", "F.Cu"]), ["Edge.Cuts", "F.Cu", "B.Cu", "F.Mask"], "the last order first, the rest after");
  const listed = ["F.Cu", "B.Cu", "F.Paste", "F.SilkS", "Edge.Cuts", "F.CrtYd", "F.Fab", "Dwgs.User"];
  assert.deepEqual(applyLayerMenu("fab", listed, []), ["F.Cu", "B.Cu", "F.Paste", "F.SilkS", "F.Fab"], "copper and technical layers, no courtyard, no Edge.Cuts");
  assert.deepEqual(applyLayerMenu("deselectCopper", listed, listed), ["F.Paste", "F.SilkS", "Edge.Cuts", "F.CrtYd", "F.Fab", "Dwgs.User"]);
  assert.deepEqual(applyLayerMenu("selectAll", listed, []), listed);
});

test("PostScript: the scales and the width correction are constrained, with KiCad's messages", () => {
  const f = { ...withFormat(initialPlotForm(BOARD, LISTED, DEFAULT_SESSION), "ps"), psXScale: "500", psYScale: "1.02", psWidthAdjust: "0.5" };
  const r = constrainPostscript(f, [-149999, 199999], (nm) => `${(nm / 1e6).toFixed(6)} mm`);
  assert.equal(r.x, 100);
  assert.equal(r.y, 1.02);
  assert.equal(r.widthMm, 0.199999);
  assert.deepEqual(r.messages, ["X scale constrained.", "Width correction constrained.  The width correction value must be in the range of [-0.149999 mm; 0.199999 mm] for the current design rules."]);
  assert.equal(r.form.psXScale, "100.000000");
  const body = plotRequest(f, r);
  assert.deepEqual([body.format, body.ps_x_scale, body.ps_width_adjust_mm, body.check_zones], ["ps", 100, 0.199999, true]);
});

test("PostScript's text controls start empty, as dialog_plot_base.cpp builds them, and an empty one adjusts nothing", () => {
  const f = withFormat(initialPlotForm(BOARD, LISTED, DEFAULT_SESSION), "ps");
  assert.deepEqual([f.psXScale, f.psYScale, f.psWidthAdjust], ["", "", ""]);
  const r = constrainPostscript(f, [-149999, 199999], (nm) => `${nm}`);
  assert.deepEqual([r.x, r.y, r.widthMm, r.messages], [1, 1, 0, []]);
  // `DIALOG_SHIM::LoadControlState`: what was typed last comes back
  const store = new Map<string, string>();
  const g = globalThis as { localStorage?: unknown };
  const had = g.localStorage;
  g.localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
  try {
    assert.equal(loadPsControls(DEFAULT_SESSION), DEFAULT_SESSION);
    savePsControls({ ...DEFAULT_SESSION, psXScale: "1.02", psYScale: "0.98", psWidthAdjust: "0.01" });
    assert.deepEqual([loadPsControls(DEFAULT_SESSION).psXScale, loadPsControls(DEFAULT_SESSION).psYScale, loadPsControls(DEFAULT_SESSION).psWidthAdjust], ["1.02", "0.98", "0.01"]);
  } finally {
    g.localStorage = had;
  }
});

test("\"No layer selected\" only where the selection, with the copper layers the board lacks, is empty", () => {
  assert.equal(noLayerSelected([], 2), false, "a two-layer board selects the 30 copper layers it lacks: nothing is plotted, \"Done.\"");
  assert.equal(noLayerSelected([], 32), true);
  assert.equal(noLayerSelected(["F.Cu"], 32), false);
});

test("the DRC line, the solder mask alert and the format of an old board", () => {
  assert.equal(drcWarning(null), null, "no DRC run: no markers");
  assert.equal(drcWarning([[], []]), null);
  assert.equal(drcWarning([[{ excluded: false }, { excluded: true }], [{}]]), "(2 known DRC violations; 1 exclusions)");
  assert.equal(showSolderMaskAlert("gerber", { expansion_nm: 50000, min_width_nm: 0 }), true);
  assert.equal(showSolderMaskAlert("pdf", { expansion_nm: 50000, min_width_nm: 0 }), false);
  assert.equal(showSolderMaskAlert("gerber", { expansion_nm: 0, min_width_nm: 0 }), false);
  assert.equal(formatOfOutputformat(0), "gerber", "HPGL is no longer supported");
  assert.equal(formatOfOutputformat(5), "svg");
});
