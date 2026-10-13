// Geographical Reannotate (`pcbnew.ReannotateTool.ShowReannotateDialog`, DIALOG_BOARD_REANNOTATE, pcbnew/dialogs/dialog_board_reannotate.cpp at 8303b2ad): the dialog's controls with the
// values it opens with, and the lines of its message window, composed from the plan the server works out (`POST /api/reannotate_plan`, crates/model/src/reannotate.rs).
//
// Pure functions, micrometres; the length formatter is the caller's (the editor's display units: `CoordTowxString` is `MessageTextFromValue`).

import type { ReannotateBad, ReannotatePlan } from "../api/fpAssign";
import type { ReannotateOptions } from "../api/types";
import type { ReportLine } from "./pcbExchange";

/** The eight radio buttons of "Footprint Order", in the order of `m_sortButtons` (the tooltips of the dialog's bitmaps). */
export const DIRECTIONS: readonly string[] = [
  "Horizontally: top left to bottom right",
  "Horizontally: top right to bottom left",
  "Horizontally: bottom left to top right",
  "Horizontally: bottom right to top left",
  "Vertically: top left to bottom right",
  "Vertically: bottom left to top right",
  "Vertically: top right to bottom left",
  "Vertically: bottom right to top left",
];

/** `VALIDPREFIX`: besides letters and digits, what a prefix may hold. */
export const VALID_PREFIX = "_-+=/\\";

/** The controls as the dialog holds them: the start numbers and the prefixes are what is typed. */
export interface ReannotateForm {
  direction: number;
  location: ReannotateOptions["location"];
  gridUm: number;
  scope: ReannotateOptions["scope"];
  excludeLocked: boolean;
  exclude: string;
  frontStart: string;
  backStart: string;
  frontPrefix: string;
  backPrefix: string;
  removeFrontPrefix: boolean;
  removeBackPrefix: boolean;
}

/**
 * What the dialog opens with: the first order, the footprint's own location, the editor's grid, the whole board -- the selection when something is selected
 * (`TransferDataToWindow`) --, front reference start 1, back blank (it continues from the front), no prefixes.
 */
export function defaultForm(gridUm: number, hasSelection: boolean): ReannotateForm {
  return {
    direction: 0,
    location: "footprint",
    gridUm,
    scope: hasSelection ? "selection" : "all",
    excludeLocked: false,
    exclude: "",
    frontStart: "1",
    backStart: "",
    frontPrefix: "",
    backPrefix: "",
    removeFrontPrefix: false,
    removeBackPrefix: false,
  };
}

/** `wxAtoi`: the number a start field holds; nothing, or no digits, is zero. */
export function startNumber(text: string): number {
  const m = /^\s*([+-]?\d+)/.exec(text);
  const n = m ? parseInt(m[1]!, 10) : 0;
  return Number.isFinite(n) && n > 0 ? n : 0;
}

/** `FilterPrefix`: a prefix keeps its letters, digits and what `VALIDPREFIX` allows. */
export function filterPrefix(text: string): string {
  return [...text].filter((c) => /[A-Za-z0-9]/.test(c) || VALID_PREFIX.includes(c)).join("");
}

/** The options of the command for the dialog's controls. */
export function formOptions(f: ReannotateForm): ReannotateOptions {
  return {
    direction: f.direction,
    location: f.location,
    grid_um: f.gridUm,
    scope: f.scope,
    exclude_locked: f.excludeLocked,
    exclude: f.exclude,
    front_start: startNumber(f.frontStart),
    back_start: startNumber(f.backStart),
    front_prefix: filterPrefix(f.frontPrefix),
    back_prefix: filterPrefix(f.backPrefix),
    remove_front_prefix: f.removeFrontPrefix,
    remove_back_prefix: f.removeBackPrefix,
  };
}

const RULE = "-------------------------------------------------------------";
const UNANNOTATED = "unannotated footprint";
const ACTION_MESSAGE = { empty: "(not updated)", invalid: "(unannotated; not updated)", exclude: "(excluded)" } as const;

type Format = (um: number) => string;

/**
 * The message window after the plan is built (`LogFootprints` for each side that has footprints, `LogChangePlan`, then the check for duplicates): the lists in their order with where
 * each footprint is and the place it is taken to be, the prefixes in use, the change log, and -- when two footprints would keep one reference -- the errors and
 * "Selected options resulted in errors! Change them and try again."
 */
export function planLines(plan: ReannotatePlan, location: ReannotateOptions["location"], format: Format): ReportLine[] {
  const out: ReportLine[] = [];
  const where = location === "footprint" ? "(sorted by footprint location)" : "(sorted by reference designator location)";
  const point = (x: number, y: number) => `${format(x)}, ${format(y)}`;
  const side = (title: string, rows: ReannotatePlan["front"]) => {
    out.push({ severity: "info", text: `${title} ${where}` }, { severity: "info", text: RULE });
    for (const r of rows) out.push({ severity: "info", text: `${r.index} ${r.reference || UNANNOTATED} at ${point(r.x, r.y)} (rounded to ${point(r.rounded_x, r.rounded_y)})` });
    out.push({ severity: "info", text: "" });
  };
  if (plan.front.length > 0) side("Front Footprints", plan.front);
  if (plan.back.length > 0) side("Back Footprints", plan.back);

  out.push({ severity: "info", text: "Reference Designator Prefixes in Use" }, { severity: "info", text: RULE });
  for (let i = 0; i < plan.prefixes.length; i += 16) out.push({ severity: "info", text: plan.prefixes.slice(i, i + 16).join(" ") });
  out.push({ severity: "info", text: "" });
  if (plan.excluded.length > 0) out.push({ severity: "info", text: `(Excluding ${plan.excluded.join(" ")} from reannotation.)` }, { severity: "info", text: "" });

  out.push({ severity: "action", text: "Change Log" }, { severity: "action", text: RULE });
  for (const c of plan.changes) {
    const old = c.old || UNANNOTATED;
    out.push({ severity: "action", text: c.action === "update" ? `${old} -> ${c.new || UNANNOTATED}` : `${old}  ${ACTION_MESSAGE[c.action]}` });
  }

  for (const e of plan.errors) out.push({ severity: "error", text: e });
  if (plan.errors.length > 0) out.push({ severity: "error", text: "Selected options resulted in errors! Change them and try again." });
  return out;
}

/** The footprints with an empty or invalid reference, as the warning lists them (`BadRefDes`), or none. */
export function badLines(bad: readonly ReannotateBad[], format: Format): ReportLine[] {
  if (bad.length === 0) return [];
  const out: ReportLine[] = [
    { severity: "warning", text: `PCB has ${bad.length} empty or invalid reference designations.` },
    { severity: "warning", text: "You may wish to run DRC with 'Test for parity between PCB and schematic' checked." },
  ];
  for (const b of bad) out.push({ severity: "warning", text: `    RefDes: ${b.reference || UNANNOTATED}; footprint: ${b.fpid || "unknown"} at ${format(b.x)}, ${format(b.y)} on PCB.` });
  return out;
}

/** What `IsOK` asks when the plan has footprints with an empty or invalid reference. */
export function confirmQuestion(bad: readonly ReannotateBad[]): string | null {
  if (bad.length === 0) return null;
  return `PCB has ${bad.length} empty or invalid reference designations.\nYou may wish to run DRC with 'Test for parity between PCB and schematic' checked.\n \nReannotate anyway?`;
}

/**
 * After the run: "PCB successfully reannotated". KiCad then warns that the PCB's annotation has to be synchronized with the schematic by Update Schematic from PCB; the studio's
 * symbols took the new references in the same step, so the two agree.
 */
export function doneLines(): ReportLine[] {
  return [
    { severity: "action", text: "PCB successfully reannotated" },
    { severity: "info", text: "The schematic's symbols have the new references too (what Update Schematic from PCB would bring back)." },
  ];
}

/** The references a selection has after the run: each footprint that was given a new one under it. */
export function renamedSelection(selection: readonly string[], plan: ReannotatePlan): string[] {
  const now = new Map(plan.changes.filter((c) => c.action === "update" && c.old !== c.new).map((c) => [c.old, c.new]));
  return selection.map((r) => now.get(r) ?? r);
}
