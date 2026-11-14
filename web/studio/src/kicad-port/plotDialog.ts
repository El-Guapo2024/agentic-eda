// The logic of the PCB Plot dialog (File > Fabrication Outputs > Gerbers... / File > Plot..., pcbnew/dialogs/dialog_plot.cpp and
// dialog_plot_base.cpp, KiCad 8303b2ad), as pure functions: what the dialog starts with (`TransferDataToWindow`, `reInitDialog`), what each
// format enables and resets (`SetPlotFormat`, `OnChangeDXFPlotMode`, `onDNPCheckbox`, `onSketchPads`, `updatePdfColorOptions`), the two layer
// lists and their right-click menus (`OnRightClickLayers`, `OnRightClickAllLayers`, `arrangeAllLayersList`), and what Plot stores back in the
// board's plot settings (`applyPlotSettings`, with its constrained PostScript values and their messages). PlotDialog.tsx is the thin view.
//
// The board's plot settings arrive as the studio state's `plot_params` (crates/cli/src/studio.rs `plot_params_json`): the layer selection,
// "Use drill/place file origin", "Plot on All Layers" and every other token `PCB_PLOT_PARAMS::Format` writes (`options`, the defaults for
// those the board lacks). Plot sends what changed through the `set_plot_settings` verb (by the same tokens), then plots with
// POST /api/fab/plot, which reads them back and maps each onto kicad-cli's flag (crates/cli/src/fab_api.rs `plot_args`).
//
// kicad-cli is the only plot engine. Where it has no flag for a control, the control is shown disabled with the reason (`KICAD_CLI_LIMITS`):
// "Use drill/place file origin" for PostScript, SVG, PDF and PNG, SVG's precision, the PDF metadata,
// DXF's "Use KiCad font to plot text", the two PDF popup boxes apart (one flag clears both). The design variant is kicad-cli's `--variant` (KiCad 25ec2335).
// "Generate Gerber job file" has no flag either, but works: unticked, the backend deletes the job file kicad-cli always writes.
//
// Dependency-free on purpose (tsconfig.test.json).

import { isCopperLayerName, userLayerNumber } from "./boardLayers";

/** `m_plotFormatOpt`'s choices, in order. */
export type PcbPlotFormat = "gerber" | "ps" | "svg" | "dxf" | "pdf" | "png";

export const PLOT_FORMATS: ReadonlyArray<{ id: PcbPlotFormat; label: string }> = [
  { id: "gerber", label: "Gerber" },
  { id: "ps", label: "Postscript" },
  { id: "svg", label: "SVG" },
  { id: "dxf", label: "DXF" },
  { id: "pdf", label: "PDF" },
  { id: "png", label: "PNG" },
];

/** `PLOT_FORMAT` as `(outputformat n)` stores it: HPGL 0, GERBER 1, POST 2, DXF 3, PDF 4, SVG 5, PNG 6. */
const OUTPUT_FORMAT: Readonly<Record<PcbPlotFormat, number>> = { gerber: 1, ps: 2, dxf: 3, pdf: 4, svg: 5, png: 6 };

/** `TransferDataToWindow`'s switch on `GetFormat()`: HPGL ("no longer supported") and anything unknown leave the choice at Gerber. */
export function formatOfOutputformat(n: number): PcbPlotFormat {
  return (Object.keys(OUTPUT_FORMAT) as PcbPlotFormat[]).find((f) => OUTPUT_FORMAT[f] === n) ?? "gerber";
}

/** "Drill marks:" (`m_drillShapeOptChoices`, `DRILL_MARKS`). */
export const DRILL_MARKS = ["None", "Small", "Actual size"] as const;
/** "Scaling:" (`m_scaleOptChoices`). */
export const SCALES = ["Auto", "1:1", "3:2", "2:1", "3:1"] as const;
/** "Coordinate format:" (`m_coordFormatCtrlChoices`): 4.5 or 4.6, unit mm. */
export const COORD_FORMATS = ["4.5, unit mm", "4.6, unit mm"] as const;

/** `PLOT_MIN_SCALE`, `PLOT_MAX_SCALE` (`pcbplot.h`). */
export const PLOT_MIN_SCALE = 0.01;
export const PLOT_MAX_SCALE = 100.0;

/** Why a control the dialog has is disabled here: kicad-cli has no flag for it (or the studio no such thing). */
export const KICAD_CLI_LIMITS = {
  auxOrigin: "kicad-cli pcb export ps, svg, pdf and png have no flag for the drill/place file origin",
  svgPrecision: "kicad-cli plots SVG with 4 digits below 1 mm (it has no precision flag)",
  pdfMetadata: "kicad-cli always generates the PDF metadata (it has no flag to leave it out)",
  pdfPopups: "kicad-cli sets both together (--no-property-popups)",
  browse: "the studio runs in a browser: type the folder, relative to the board's folder",
  openDir: "the studio runs in a browser and cannot open a folder of the server; the files are listed below after a plot",
} as const;

/** The dialog's controls, one field per control. */
export interface PlotForm {
  format: PcbPlotFormat;
  /** `m_layerCheckListBox`'s checked layers (board-file names). */
  checked: string[];
  /** `m_plotAllLayersList`: its order, and its checked items. */
  allLayersOrder: string[];
  allLayersChecked: string[];
  outputDirectory: string;
  // General Options
  plotSheetRef: boolean;
  subtractMaskFromSilk: boolean;
  plotDNP: boolean;
  /** The "Hide" / "Cross-out" radios. */
  dnpMode: "hide" | "crossout";
  sketchPads: boolean;
  padNumbers: boolean;
  zoneFillCheck: boolean;
  drillMarks: number;
  scale: number;
  useAuxOrigin: boolean;
  mirror: boolean;
  negative: boolean;
  // Gerber Options
  protelExt: boolean;
  jobFile: boolean;
  /** 5 or 6 (`m_coordFormatCtrl`). */
  coordFormat: number;
  x2: boolean;
  netAttributes: boolean;
  disableMacros: boolean;
  // Postscript Options: the text controls' text.
  psXScale: string;
  psYScale: string;
  /** "Track width correction:", mm. */
  psWidthAdjust: string;
  forceA4: boolean;
  // DXF Options
  dxfContours: boolean;
  /** "Export units:": Inches or Millimeters. */
  dxfUnits: "in" | "mm";
  dxfKiCadFont: boolean;
  dxfSingle: boolean;
  // SVG Options
  svgPrecision: number;
  svgBlackAndWhite: boolean;
  svgFitPage: boolean;
  // PDF Options
  pdfBlackAndWhite: boolean;
  pdfFrontPopups: boolean;
  pdfBackPopups: boolean;
  pdfMetadata: boolean;
  pdfSingle: boolean;
  /** "Background Color": a `#rrggbb`, or null for `COLOR4D::UNSPECIFIED`. */
  pdfBackground: string | null;
  // PNG Options
  pngDpi: number;
  pngAntialias: boolean;
}

/** The board's plot settings as the studio state carries them. */
export interface BoardPlotParams {
  layers: readonly string[];
  use_aux_origin: boolean;
  plot_on_all_layers?: readonly string[];
  options?: Readonly<Record<string, string>>;
}

/** What KiCad keeps in its session, not in the board file (`m_svgFitPagetoBoard`, `m_PDFBackgroundColor`, `m_DXFExportAsMultiLayeredFile`
 * are `PCB_PLOT_PARAMS` members `Format` does not write; the PostScript text controls are `DIALOG_SHIM`'s saved control state). */
export interface PlotSessionValues {
  svgFitPage: boolean;
  pdfBackground: string | null;
  dxfSingle: boolean;
  psXScale: string;
  psYScale: string;
  psWidthAdjust: string;
  zoneFillCheck: boolean;
  /** `s_lastAllLayersOrder`: the "Plot on All Layers" order when the dialog last closed. */
  lastAllLayersOrder: string[];
}

/** `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS`' values of the session's members, `m_zoneFillCheck->SetValue(true)`. The PostScript text controls start as
 * `dialog_plot_base.cpp` builds them, empty (`wxEmptyString`): `TransferDataToWindow` sets them only for a job (`if( m_job && .. )`), and
 * `DIALOG_SHIM::LoadControlState` gives them what was last typed (`loadPsControls`). */
export const DEFAULT_SESSION: PlotSessionValues = { svgFitPage: false, pdfBackground: null, dxfSingle: false, psXScale: "", psYScale: "", psWidthAdjust: "", zoneFillCheck: true, lastAllLayersOrder: [] };

/** Where `DIALOG_SHIM::SaveControlState` keeps the Plot dialog's PostScript text controls (KiCad: `kicad_common.json`'s `m_dialogControlValues`, kept
 * across runs; here the browser's storage). */
const PS_CONTROLS_KEY = "eda.dialogControlValues.plot.ps";

/** `DIALOG_SHIM::LoadControlState` of the three PostScript text controls: the session's values with what was last typed, if anything was. */
export function loadPsControls(session: PlotSessionValues): PlotSessionValues {
  try {
    const kept = JSON.parse(globalThis.localStorage?.getItem(PS_CONTROLS_KEY) ?? "null") as Partial<Record<"x" | "y" | "w", unknown>> | null;
    if (!kept) return session;
    const str = (v: unknown, d: string) => (typeof v === "string" ? v : d);
    return { ...session, psXScale: str(kept.x, session.psXScale), psYScale: str(kept.y, session.psYScale), psWidthAdjust: str(kept.w, session.psWidthAdjust) };
  } catch {
    return session;
  }
}

/** `DIALOG_SHIM::SaveControlState` of the three PostScript text controls (on the dialog's close). */
export function savePsControls(session: PlotSessionValues): void {
  try {
    globalThis.localStorage?.setItem(PS_CONTROLS_KEY, JSON.stringify({ x: session.psXScale, y: session.psYScale, w: session.psWidthAdjust }));
  } catch {
    /* storage refused: the values last for the page */
  }
}

/** `DIALOG_PLOT::Plot`'s "No layer selected, Nothing to plot": `applyPlotSettings` adds every copper layer the board does not have to the selection
 * (`selectedLayers | disabledCopperLayers`, "If someone enables more copper layers they will be selected by default"), so the selection is empty only when
 * nothing is checked on a board of all 32 copper layers. With nothing checked on any other board the plot goes on and `PCB_PLOTTER::Plot` skips the
 * disabled copper layers (`copperLayerShouldBeSkipped`): nothing is plotted and the report is "Done.". */
export function noLayerSelected(checked: readonly string[], copperLayerCount: number): boolean {
  return checked.length === 0 && copperLayerCount >= 32;
}

/** `PCB_PLOT_PARAMS::PCB_PLOT_PARAMS` as `PCB_PLOT_PARAMS::Format` writes it: what a token the board lacks starts as. */
export const DEFAULT_OPTIONS: Readonly<Record<string, string>> = {
  disableapertmacros: "no",
  usegerberextensions: "no",
  usegerberattributes: "yes",
  usegerberadvancedattributes: "yes",
  creategerberjobfile: "yes",
  gerberprecision: "6",
  svgprecision: "4",
  pngdpi: "300",
  pngantialias: "yes",
  plotframeref: "no",
  mode: "1",
  useauxorigin: "no",
  pdf_front_fp_property_popups: "yes",
  pdf_back_fp_property_popups: "yes",
  pdf_metadata: "yes",
  pdf_single_document: "no",
  dxfpolygonmode: "yes",
  dxfimperialunits: "yes",
  dxfusepcbnewfont: "yes",
  psnegative: "no",
  psa4output: "no",
  plot_black_and_white: "yes",
  sketchpadsonfab: "no",
  plotpadnumbers: "no",
  hidednponfab: "no",
  sketchdnponfab: "yes",
  crossoutdnponfab: "yes",
  subtractmaskfromsilk: "no",
  outputformat: "1",
  mirror: "no",
  drillshape: "1",
  scaleselection: "1",
  outputdirectory: "",
};

const TECH_UI_ORDER = ["F.Adhes", "B.Adhes", "F.Paste", "B.Paste", "F.SilkS", "B.SilkS", "F.Mask", "B.Mask", "Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User", "Edge.Cuts", "Margin", "F.CrtYd", "B.CrtYd", "F.Fab", "B.Fab"];

function copperRank(l: string): number {
  if (l === "F.Cu") return 0;
  if (l === "B.Cu") return 31;
  const m = /^In(\d+)\.Cu$/.exec(l);
  return m ? Number(m[1]) : -1;
}

/** `LSET::UIOrder`: `CuStack()` (the copper from the front), then `TechAndUserUIOrder()` and `User.1` ... The checklist's order (`m_layerList`). */
export function uiOrder(layers: readonly string[]): string[] {
  const rank = (l: string): number => {
    const cu = copperRank(l);
    if (cu >= 0) return cu;
    const t = TECH_UI_ORDER.indexOf(l);
    if (t >= 0) return 100 + t;
    const u = userLayerNumber(l);
    return u !== null ? 200 + u : Number.MAX_SAFE_INTEGER;
  };
  return [...new Set(layers)].sort((a, b) => rank(a) - rank(b));
}

/** `LSET::SeqStackupForPlotting`: bottom to top -- B.Cu and the back technical layers, the other copper layers from the bottom, the front
 * technical layers, the user drawing layers, `User.N` from the last, then Margin and Edge.Cuts. The "Plot on All Layers" list's first order. */
export function stackupForPlotting(layers: readonly string[]): string[] {
  const has = new Set(layers);
  const seq = (names: readonly string[]) => names.filter((n) => has.has(n));
  const out = seq(["B.Cu", "B.Mask", "B.Paste", "B.SilkS", "B.Adhes", "B.CrtYd", "B.Fab"]);
  const copper = layers.filter((l) => isCopperLayerName(l) && l !== "B.Cu").sort((a, b) => copperRank(a) - copperRank(b));
  out.push(...copper.reverse());
  out.push(...seq(["F.Mask", "F.Paste", "F.SilkS", "F.Adhes", "F.CrtYd", "F.Fab"]));
  out.push(...seq(["Dwgs.User", "Cmts.User", "Eco1.User", "Eco2.User"]));
  const users = layers.filter((l) => userLayerNumber(l) !== null).sort((a, b) => (userLayerNumber(a) ?? 0) - (userLayerNumber(b) ?? 0));
  out.push(...users.reverse());
  out.push(...seq(["Margin", "Edge.Cuts"]));
  return out;
}

/** `arrangeAllLayersList( aSeq )`: each layer of `seq` the list has is moved up to the place it has in `seq`; the others keep their order after them. */
export function arrangeAllLayers(order: readonly string[], seq: readonly string[]): string[] {
  const list = [...order];
  let idx = 0;
  for (const layer of seq) {
    let pos = list.indexOf(layer);
    if (pos < 0) {
      idx++;
      continue;
    }
    while (pos > idx) {
      [list[pos - 1], list[pos]] = [list[pos] as string, list[pos - 1] as string];
      pos--;
    }
    idx++;
  }
  return list;
}

/** `OnRightClickLayers`' menu on the checklist. "Select Fab Layers": the copper and technical layers, courtyards aside
 * (`( LSET::AllCuMask() | LSET::AllTechMask() ) & ~LSET( { B_CrtYd, F_CrtYd } )`, so not Edge.Cuts). */
export type LayerMenuItem = "fab" | "selectCopper" | "deselectCopper" | "selectAll" | "deselectAll";
export const LAYER_MENU: ReadonlyArray<{ id: LayerMenuItem; label: string } | null> = [
  { id: "fab", label: "Select Fab Layers" },
  null,
  { id: "selectCopper", label: "Select All Copper Layers" },
  { id: "deselectCopper", label: "Deselect All Copper Layers" },
  null,
  { id: "selectAll", label: "Select All Layers" },
  { id: "deselectAll", label: "Deselect All Layers" },
];

const ALL_TECH = ["B.SilkS", "B.Mask", "B.Adhes", "B.Paste", "B.CrtYd", "B.Fab", "F.SilkS", "F.Mask", "F.Adhes", "F.Paste", "F.CrtYd", "F.Fab"];

export function applyLayerMenu(item: LayerMenuItem, listed: readonly string[], checked: readonly string[]): string[] {
  const on = new Set(checked);
  for (const l of listed) {
    switch (item) {
      case "fab":
        if (isCopperLayerName(l) || (ALL_TECH.includes(l) && l !== "F.CrtYd" && l !== "B.CrtYd")) on.add(l);
        else on.delete(l);
        break;
      case "selectCopper":
        if (isCopperLayerName(l)) on.add(l);
        break;
      case "deselectCopper":
        if (isCopperLayerName(l)) on.delete(l);
        break;
      case "selectAll":
        on.add(l);
        break;
      case "deselectAll":
        on.delete(l);
        break;
    }
  }
  return listed.filter((l) => on.has(l));
}

/** `OnRightClickAllLayers`' menu on "Plot on All Layers". */
export type AllLayersMenuItem = "selectAll" | "deselectAll" | "stackup";
export const ALL_LAYERS_MENU: ReadonlyArray<{ id: AllLayersMenuItem; label: string } | null> = [
  { id: "selectAll", label: "Select All Layers" },
  { id: "deselectAll", label: "Deselect All Layers" },
  null,
  { id: "stackup", label: "Order as Board Stackup" },
];

const yes = (o: Readonly<Record<string, string>>, k: string): boolean => (o[k] ?? DEFAULT_OPTIONS[k]) === "yes";
const num = (o: Readonly<Record<string, string>>, k: string): number => Number(o[k] ?? DEFAULT_OPTIONS[k] ?? 0);

/** The board's options, the defaults filled in. */
export function optionsOf(board: BoardPlotParams | null | undefined): Record<string, string> {
  return { ...DEFAULT_OPTIONS, ...(board?.options ?? {}) };
}

/** `DIALOG_PLOT::TransferDataToWindow` (and `reInitDialog`, then `SetPlotFormat`): the dialog as it opens on the board's plot settings. `listed` are
 * the board's enabled layers (the checklist's rows); `session` what KiCad's session keeps. */
export function initialPlotForm(board: BoardPlotParams | null | undefined, listed: readonly string[], session: PlotSessionValues): PlotForm {
  const o = optionsOf(board);
  const selection = new Set(board?.layers ?? []);
  const hide = yes(o, "hidednponfab");
  const plotDNP = hide || yes(o, "sketchdnponfab") || yes(o, "crossoutdnponfab");
  const allLayers = arrangeAllLayers(stackupForPlotting(listed), session.lastAllLayersOrder);
  const form: PlotForm = {
    format: formatOfOutputformat(num(o, "outputformat")),
    checked: uiOrder(listed).filter((l) => selection.has(l)),
    allLayersOrder: allLayers,
    allLayersChecked: allLayers.filter((l) => (board?.plot_on_all_layers ?? []).includes(l)),
    outputDirectory: o.outputdirectory ?? "",
    plotSheetRef: yes(o, "plotframeref"),
    subtractMaskFromSilk: yes(o, "subtractmaskfromsilk"),
    plotDNP,
    // The radios are a group whose first ("Hide") is selected until one is set: `if( m_plotDNP ) { hide ? m_hideDNP : m_crossoutDNP }`.
    dnpMode: plotDNP && !hide ? "crossout" : "hide",
    sketchPads: yes(o, "sketchpadsonfab"),
    padNumbers: yes(o, "plotpadnumbers"),
    zoneFillCheck: session.zoneFillCheck,
    drillMarks: num(o, "drillshape"),
    scale: num(o, "scaleselection"),
    useAuxOrigin: board?.use_aux_origin ?? false,
    mirror: yes(o, "mirror"),
    negative: yes(o, "psnegative"),
    protelExt: yes(o, "usegerberextensions"),
    jobFile: yes(o, "creategerberjobfile"),
    coordFormat: num(o, "gerberprecision") === 5 ? 5 : 6,
    x2: yes(o, "usegerberattributes"),
    netAttributes: yes(o, "usegerberadvancedattributes"),
    disableMacros: yes(o, "disableapertmacros"),
    psXScale: session.psXScale,
    psYScale: session.psYScale,
    psWidthAdjust: session.psWidthAdjust,
    forceA4: yes(o, "psa4output"),
    dxfContours: yes(o, "dxfpolygonmode"),
    dxfUnits: yes(o, "dxfimperialunits") ? "in" : "mm",
    dxfKiCadFont: yes(o, "dxfusepcbnewfont"),
    dxfSingle: session.dxfSingle,
    svgPrecision: num(o, "svgprecision"),
    svgBlackAndWhite: yes(o, "plot_black_and_white"),
    svgFitPage: session.svgFitPage,
    pdfBlackAndWhite: yes(o, "plot_black_and_white"),
    pdfFrontPopups: yes(o, "pdf_front_fp_property_popups"),
    pdfBackPopups: yes(o, "pdf_back_fp_property_popups"),
    pdfMetadata: yes(o, "pdf_metadata"),
    pdfSingle: yes(o, "pdf_single_document"),
    pdfBackground: session.pdfBackground,
    pngDpi: num(o, "pngdpi"),
    pngAntialias: yes(o, "pngantialias"),
  };
  return withFormat(form, form.format);
}

/** Which controls are enabled (`SetPlotFormat`'s `Enable` calls, `OnChangeDXFPlotMode`, `onDNPCheckbox`, `onSketchPads`, `updatePdfColorOptions`),
 * and, separately, the ones kicad-cli cannot do whatever the format (`cliDisabled`). */
export interface PlotEnables {
  drillMarks: boolean;
  mirror: boolean;
  useAuxOrigin: boolean;
  scale: boolean;
  psAdjust: boolean;
  negative: boolean;
  forceA4: boolean;
  dxfContours: boolean;
  dxfKiCadFont: boolean;
  dnpRadios: boolean;
  padNumbers: boolean;
  pdfBackground: boolean;
  /** The reason "Use drill/place file origin" is off where KiCad has it on (kicad-cli has no flag), else null. */
  auxOriginCliReason: string | null;
}

export function plotEnables(form: PlotForm): PlotEnables {
  const f = form.format;
  const auxByKiCad = f !== "ps";
  const auxByCli = f === "gerber" || f === "dxf";
  return {
    drillMarks: f !== "gerber",
    mirror: f === "svg" || f === "pdf" || f === "ps" || f === "png",
    useAuxOrigin: auxByKiCad && auxByCli,
    scale: f !== "gerber",
    psAdjust: f === "ps",
    negative: f === "svg" || f === "pdf" || f === "ps" || f === "png",
    forceA4: f === "ps",
    dxfContours: f === "dxf",
    // `m_DXF_plotTextStrokeFontOpt->Enable( !m_DXF_plotModeOpt->GetValue() )` (unticked, the plot goes through kicad-cli's jobset runner, whose DXF job has
    // `use_kicad_font`: UI-LEFT-3 P3-25)
    dxfKiCadFont: !form.dxfContours,
    dnpRadios: form.plotDNP,
    padNumbers: form.sketchPads,
    pdfBackground: !form.pdfBlackAndWhite,
    auxOriginCliReason: auxByKiCad && !auxByCli ? KICAD_CLI_LIMITS.auxOrigin : null,
  };
}

/** `SetPlotFormat` for `format`: the controls it disables take the values it sets (Gerber: no drill marks, no mirror, 1:1, no negative; DXF:
 * no mirror or negative; PostScript: no drill/place file origin; every format but PostScript: no forced A4), and `OnChangeDXFPlotMode` forces the
 * KiCad font on while the DXF contours box is checked. */
export function withFormat(form: PlotForm, format: PcbPlotFormat): PlotForm {
  const f: PlotForm = { ...form, format };
  if (format !== "ps") f.forceA4 = false;
  if (format === "ps") f.useAuxOrigin = false;
  if (format === "gerber") {
    f.drillMarks = 0;
    f.mirror = false;
    f.scale = 1;
    f.negative = false;
  }
  if (format === "dxf") {
    f.mirror = false;
    f.negative = false;
    if (f.dxfContours) f.dxfKiCadFont = true;
  }
  return f;
}

/** `m_DXF_plotModeOpt` toggled (`OnChangeDXFPlotMode`). */
export function withDxfContours(form: PlotForm, on: boolean): PlotForm {
  return { ...form, dxfContours: on, dxfKiCadFont: on ? true : form.dxfKiCadFont };
}

/** `setDouble`/`setInt`: `value` clipped to `[min, max]`; `clipped` when it was outside. */
function clip(value: number, min: number, max: number): { value: number; clipped: boolean } {
  if (value < min) return { value: min, clipped: true };
  if (value > max) return { value: max, clipped: true };
  return { value, clipped: false };
}

/** wxString::ToDouble: the number, or 0 for text that is none (`msg.ToDouble( &tmpDouble )` leaves 0 for a fresh double here). */
function toDouble(text: string): number {
  const v = Number(text.trim().replace(",", "."));
  return Number.isFinite(v) ? v : 0;
}

/** The PostScript values `applyPlotSettings` takes from the text controls, constrained, with the messages it reports ("X scale constrained.",
 * "Y scale constrained." as info, the width correction's range as a warning). `widthRange` is `[-(m_TrackMinWidth - 1), GetSmallestClearanceValue() - 1]`
 * in nm; `fmt` writes a length as `StringFromValue( .., true )` does (with its units). */
export function constrainPostscript(form: PlotForm, widthRange: [number, number], fmt: (nm: number) => string): { x: number; y: number; widthMm: number; form: PlotForm; messages: string[] } {
  const messages: string[] = [];
  // An empty control (how the dialog first opens) parses nothing: `wxString::ToDouble` leaves `tmpDouble` as it was, which the C++ never set, so what
  // KiCad does then cannot be copied. Here it is no adjustment: `Plot`'s `if( m_XScaleAdjust != 0.0 )` keeps the plot options' scale, 1, and nothing
  // is said.
  const scale = (text: string) => (text.trim() === "" ? { value: 1, clipped: false } : clip(toDouble(text), PLOT_MIN_SCALE, PLOT_MAX_SCALE));
  const x = scale(form.psXScale);
  const y = scale(form.psYScale);
  let next = form;
  if (x.clipped) {
    next = { ...next, psXScale: x.value.toFixed(6) };
    messages.push("X scale constrained.");
  }
  if (y.clipped) {
    next = { ...next, psYScale: y.value.toFixed(6) };
    messages.push("Y scale constrained.");
  }
  const widthNm = Math.round(toDouble(form.psWidthAdjust) * 1e6);
  const w = clip(widthNm, widthRange[0], widthRange[1]);
  if (w.clipped) {
    next = { ...next, psWidthAdjust: String(w.value / 1e6) };
    messages.push(`Width correction constrained.  The width correction value must be in the range of [${fmt(widthRange[0])}; ${fmt(widthRange[1])}] for the current design rules.`);
  }
  return { x: x.value, y: y.value, widthMm: w.value / 1e6, form: next, messages };
}

/** `applyPlotSettings`: the board's plot settings the dialog's controls make, by `PCB_PLOT_PARAMS::Format`'s tokens. `tempOptions` is a fresh
 * `PCB_PLOT_PARAMS`, so whatever a control does not set is the default again: the PDF options outside the PDF format (`else if( PDF )`), the
 * DXF graphic items' mode (`mode`, no control). Black and white is the SVG or PDF choice for those formats, off for PNG, on for the others. */
export function optionsFromForm(form: PlotForm): Record<string, string> {
  const yn = (b: boolean) => (b ? "yes" : "no");
  const pdf = form.format === "pdf";
  const bw = form.format === "svg" ? form.svgBlackAndWhite : pdf ? form.pdfBlackAndWhite : form.format !== "png";
  return {
    subtractmaskfromsilk: yn(form.subtractMaskFromSilk),
    plotframeref: yn(form.plotSheetRef),
    sketchpadsonfab: yn(form.sketchPads),
    plotpadnumbers: yn(form.padNumbers),
    hidednponfab: yn(form.plotDNP && form.dnpMode === "hide"),
    sketchdnponfab: yn(form.plotDNP && form.dnpMode === "crossout"),
    crossoutdnponfab: yn(form.plotDNP && form.dnpMode === "crossout"),
    scaleselection: String(form.scale),
    drillshape: String(form.drillMarks),
    mirror: yn(form.mirror),
    dxfpolygonmode: yn(form.dxfContours),
    dxfimperialunits: yn(form.dxfUnits === "in"),
    // `if( !m_DXF_plotTextStrokeFontOpt->IsEnabled() ) SetTextMode( DEFAULT ) else SetTextMode( GetValue() ? DEFAULT : NATIVE )`: the box is enabled while
    // the contours box is unticked, whatever the format
    dxfusepcbnewfont: yn(form.dxfContours || form.dxfKiCadFont),
    plot_black_and_white: yn(bw),
    pdf_front_fp_property_popups: pdf ? yn(form.pdfFrontPopups) : "yes",
    pdf_back_fp_property_popups: pdf ? yn(form.pdfBackPopups) : "yes",
    pdf_metadata: pdf ? yn(form.pdfMetadata) : "yes",
    pdf_single_document: pdf ? yn(form.pdfSingle) : "no",
    mode: "1",
    outputformat: String(OUTPUT_FORMAT[form.format]),
    disableapertmacros: yn(form.disableMacros),
    usegerberextensions: yn(form.protelExt),
    usegerberattributes: yn(form.x2),
    usegerberadvancedattributes: yn(form.netAttributes),
    creategerberjobfile: yn(form.jobFile),
    gerberprecision: String(form.coordFormat),
    svgprecision: String(form.svgPrecision),
    pngdpi: String(form.pngDpi),
    pngantialias: yn(form.pngAntialias),
    psnegative: yn(form.negative),
    psa4output: yn(form.forceA4),
    // "Set output directory and replace backslashes with forward ones".
    outputdirectory: form.outputDirectory.replace(/\\/g, "/"),
  };
}

/** The `set_plot_settings` command Plot sends (`if( !m_plotOpts.IsSameAs( tempOptions ) ) SetPlotSettings(..)`): only what changed, or null
 * when nothing did. `listed` are the checklist's rows. */
export function plotSettingsCommand(
  form: PlotForm,
  board: BoardPlotParams | null | undefined,
  listed: readonly string[],
): { op: "set_plot_settings"; layers?: string[]; use_aux_origin?: boolean; plot_on_all_layers?: string[]; options?: Record<string, string> } | null {
  const was = optionsOf(board);
  const now = optionsFromForm(form);
  const changed: Record<string, string> = {};
  for (const [k, v] of Object.entries(now)) if ((was[k] ?? "") !== v) changed[k] = v;
  const before = new Set(board?.layers ?? []);
  const checked = uiOrder(form.checked);
  const listedBefore = listed.filter((l) => before.has(l));
  const layersChanged = listedBefore.length !== checked.length || checked.some((l) => !before.has(l));
  const common = form.allLayersOrder.filter((l) => form.allLayersChecked.includes(l));
  const commonChanged = JSON.stringify(common) !== JSON.stringify(board?.plot_on_all_layers ?? []);
  const auxChanged = form.useAuxOrigin !== (board?.use_aux_origin ?? false);
  if (!layersChanged && !commonChanged && !auxChanged && Object.keys(changed).length === 0) return null;
  return {
    op: "set_plot_settings",
    ...(layersChanged ? { layers: checked } : {}),
    ...(auxChanged ? { use_aux_origin: form.useAuxOrigin } : {}),
    ...(commonChanged ? { plot_on_all_layers: common } : {}),
    ...(Object.keys(changed).length > 0 ? { options: changed } : {}),
  };
}

/** The POST /api/fab/plot body: the format, the checked layers in `UIOrder` (`GetLayerSelection().UIOrder()`), the zone check and the session's values. */
export function plotRequest(form: PlotForm, ps: { x: number; y: number; widthMm: number }): Record<string, unknown> {
  return {
    format: form.format,
    layers: uiOrder(form.checked),
    check_zones: form.zoneFillCheck,
    svg_fit_page_to_board: form.svgFitPage,
    pdf_background_color: form.pdfBackground,
    dxf_single_document: form.dxfSingle,
    ps_x_scale: ps.x,
    ps_y_scale: ps.y,
    ps_width_adjust_mm: ps.widthMm,
  };
}

/** `reInitDialog`'s "(%d known DRC violations; %d exclusions)", shown when the board has markers (the last DRC's violations, unconnected
 * items and parity issues, `PCB_MARKER`s), else null. */
export function drcWarning(lists: ReadonlyArray<ReadonlyArray<{ excluded?: boolean }> | undefined> | null): string | null {
  if (!lists) return null;
  let known = 0;
  let exclusions = 0;
  for (const list of lists) for (const v of list ?? []) v.excluded ? exclusions++ : known++;
  return known || exclusions ? `(${known} known DRC violations; ${exclusions} exclusions)` : null;
}

/** `SetPlotFormat`'s solder mask alert: Gerber, and a global solder mask margin or minimum width that is not 0. */
export function showSolderMaskAlert(format: PcbPlotFormat, mask: { expansion_nm: number; min_width_nm: number } | null | undefined): boolean {
  return format === "gerber" && !!mask && (mask.expansion_nm !== 0 || mask.min_width_nm !== 0);
}
