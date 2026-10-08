// The logic behind Board Setup's rule pages (pcbnew/dialogs/dialog_board_setup.cpp and the panels it hosts), as pure functions: the net-name
// patterns and the assignment dialog's proposal, the checks each panel makes before OK, the default physical stackup, and the violation
// severity table. The dialog (components/BoardSetupDialog.tsx and components/boardSetup/*) holds the drafts and renders; the page's whole
// content goes to the backend as one command (`set_net_classes`, `set_constraints`, ...), which re-checks it (crates/model/src/rules.rs) --
// the checks here exist so a mistake is shown next to its field instead of as a refusal after the click.
//
// Ported from:
//   common/dialogs/dialog_assign_netclass.cpp        GetNetclassPatternForSet (the pattern proposed for the nets picked)
//   common/project/net_settings.cpp                  SetNetclassPatternAssignment / addSinglePatternAssignment
//   common/eda_pattern_match.cpp                     EDA_PATTERN_MATCH_WILDCARD_ANCHORED (what a net class pattern matches)
//   common/dialogs/panel_setup_netclasses.cpp        validateNetclassName, validateNetclassClearance (MAXIMUM_CLEARANCE)
//   pcbnew/board_design_settings.cpp                 ValidateDesignRules (the ranges of Constraints and Solder Mask/Paste)
//   pcbnew/dialogs/panel_setup_text_and_graphics.cpp TransferDataFromWindow's checks
//   pcbnew/board_stackup_manager/board_stackup.cpp   BuildDefaultStackupList, BuildBoardThicknessFromStackup, IsThicknessEditable ...
//
// Not ported: KiCad's regular-expression mode for net class patterns (a pattern here is `*` and `?` wildcards, anchored, as
// crates/model/src/lib.rs `glob_match` reads it; the alternation `a|b` and the grouped `prefix(a|b)` KiCad proposes are expanded into one pattern per
// alternative, which matches the same nets), bus-notation patterns (`ForEachBusMember`), and the composite net class KiCad builds
// for a net several classes claim (here the first class whose pattern matches owns the net).
//
// The default stackup is also computed by the backend (`eda_model::rules::default_stackup`); both are held to the same cases in
// src/kicad/default_stackups.json (a Rust test fails when the file is stale, boardSetupRules.test.ts when this code disagrees with it).

import type { Constraints, LayerClassDefaults, MaskPaste, NetClass, RuleSeverity, Stackup, StackupLayer, StackupSettings, TextGraphicsDefaults, Um } from "../api/types";
import { strNumCmp } from "./sheetPages";

// ------------------------------------------------------------------ net class patterns

/**
 * What a net class pattern matches: `*` is any run of characters (empty too), `?` any one character, everything else itself, and the
 * pattern must cover the whole net name (`EDA_PATTERN_MATCH_WILDCARD_ANCHORED`; `eda_model::glob_match` is the backend's twin).
 */
export function globMatch(pattern: string, name: string): boolean {
  const p = Array.from(pattern);
  const s = Array.from(name);
  let pi = 0;
  let si = 0;
  let star = -1; // where the last `*` is in the pattern
  let mark = 0; //  and how much of the name it has swallowed so far
  while (si < s.length) {
    if (pi < p.length && p[pi] === "*") {
      star = pi++;
      mark = si;
    } else if (pi < p.length && (p[pi] === "?" || p[pi] === s[si])) {
      pi++;
      si++;
    } else if (star >= 0) {
      pi = star + 1;
      si = ++mark;
    } else {
      return false;
    }
  }
  while (pi < p.length && p[pi] === "*") pi++;
  return pi === p.length;
}

/**
 * The plain patterns a typed or proposed pattern stands for. KiCad reads a pattern as a regular expression too, and its assignment dialog
 * proposes `prefix(tail|tail)` or `a|b|c`; matching those needs no regular expressions here: `a|b|c` is the three patterns `a`, `b` and `c`,
 * and `prefix(x|y)suffix` is `prefixxsuffix` and `prefixysuffix`. Anything else, including a parenthesis that does not hold an alternation
 * (`Net-(R1-Pad1)` is a net's name), is one pattern, as written.
 */
export function expandPattern(pattern: string): string[] {
  const grouped = /^([^()|]*)\(([^()]*\|[^()]*)\)([^()|]*)$/.exec(pattern);
  const out = grouped ? grouped[2]!.split("|").map((alt) => grouped[1]! + alt + grouped[3]!) : pattern.includes("|") && !/[()]/.test(pattern) ? pattern.split("|") : [pattern];
  return out.filter((p) => p !== "");
}

/**
 * `GetNetclassPatternForSet`: the pattern the Assign Netclass dialog starts with for the nets picked -- the net's own name for one, else the
 * names sorted naturally (case ignored) with their common prefix factored out as `prefix(tail|tail|...)`, or just `a|b|c` when there is no
 * prefix worth factoring (none, or only a slash). KiCad turns a glob star in a name into a regular-expression star first; here a star is
 * already a wildcard, so the names go in as they are.
 */
export function proposePattern(netNames: readonly string[]): string {
  const set = [...new Set(netNames)].sort();
  if (set.length === 0) return "";
  if (set.length === 1) return set[0]!;
  const names = [...set].sort((a, b) => strNumCmp(a.toLowerCase(), b.toLowerCase()));
  let prefix = names[0]!;
  for (const n of names) {
    let k = 0;
    while (k < prefix.length && k < n.length && prefix[k] === n[k]) k++;
    prefix = prefix.slice(0, k);
    if (prefix === "") break;
  }
  if (prefix !== "" && prefix !== "/") return `${prefix}(${names.map((n) => n.slice(prefix.length)).join("|")})`;
  return names.join("|");
}

/** One row of the Netclass Assignments table: a pattern and the class that owns the nets it matches. */
export interface AssignmentRow {
  pattern: string;
  netclass: string;
}

/** The assignment table of a set of classes: one row per pattern, class by class in their order. */
export function assignmentRows(classes: readonly NetClass[]): AssignmentRow[] {
  return classes.flatMap((c) => c.nets.map((pattern) => ({ pattern, netclass: c.name })));
}

/**
 * `SetNetclassPatternAssignment` / `addSinglePatternAssignment`: assigning `pattern` to `netclass` adds one row per plain pattern it
 * stands for ([`expandPattern`]), none for a pattern already assigned to that class. A pattern already assigned to another class moves to
 * this one -- KiCad keeps both and merges the two classes for the nets it matches; here the first class that matches owns a net, so a copy
 * under a class further down would never apply.
 */
export function addAssignment(rows: readonly AssignmentRow[], pattern: string, netclass: string): AssignmentRow[] {
  let out = [...rows];
  for (const p of expandPattern(pattern.trim())) {
    out = out.filter((r) => r.pattern !== p || r.netclass === netclass);
    if (!out.some((r) => r.pattern === p && r.netclass === netclass)) out.push({ pattern: p, netclass });
  }
  return out;
}

/** The classes with their patterns taken from the assignment table. A row naming no class, or with nothing in its pattern, is dropped; a pattern typed as `a|b` becomes two. */
export function applyAssignments(classes: readonly NetClass[], rows: readonly AssignmentRow[]): NetClass[] {
  return classes.map((c) => ({ ...c, nets: rows.filter((r) => r.netclass === c.name).flatMap((r) => expandPattern(r.pattern.trim())) }));
}

/** The nets, among `nets`, that `pattern` (as typed, so `a|b` too) assigns -- the "Nets matching" list. */
export function netsMatching(pattern: string, nets: readonly string[]): string[] {
  const plain = expandPattern(pattern.trim());
  return nets.filter((n) => plain.some((p) => globMatch(p, n)));
}

/** The class that owns `net`: the first class, in order, with a pattern that matches it, else the Default class. */
export function classOfNet(classes: readonly NetClass[], net: string, defaultName = "Default"): string {
  for (const c of classes) if (c.nets.some((p) => globMatch(p, net))) return c.name;
  return defaultName;
}

/** `MAXIMUM_CLEARANCE` (board_design_settings.h): a larger clearance overflows the integer maths in KiCad's engines. */
export const MAXIMUM_CLEARANCE_UM: Um = 500_000;

/** A mistake in the Net Classes page: row 0 is the Default class, row i + 1 the i-th other class. */
export interface NetClassIssue {
  row: number;
  field: "name" | "track_width" | "clearance" | "via_diameter" | "via_drill" | "microvia_diameter" | "microvia_drill" | "diff_pair_width" | "diff_pair_gap" | "priority" | "pattern";
  message: string;
}

const SIZE_FIELDS = ["track_width", "clearance", "via_diameter", "via_drill", "microvia_diameter", "microvia_drill", "diff_pair_width", "diff_pair_gap"] as const;
const SIZE_LABELS: Record<(typeof SIZE_FIELDS)[number], string> = {
  track_width: "Track width",
  clearance: "Clearance",
  via_diameter: "Via size",
  via_drill: "Via hole",
  microvia_diameter: "uVia size",
  microvia_drill: "uVia hole",
  diff_pair_width: "DP width",
  diff_pair_gap: "DP gap",
};

/**
 * The Net Classes page's checks (`validateNetclassName`, `validateNetclassClearance`, and the backend's size checks): a name for every class and no
 * two alike (case ignored), no negative size and no zero one but a clearance or a gap, a clearance no larger than `MAXIMUM_CLEARANCE`, a via
 * hole smaller than its via, and no empty pattern. A size that is not a number (`NaN`: text the field could not read) is a mistake too.
 */
export function validateNetClasses(defaultClass: NetClass, classes: readonly NetClass[], rows: readonly AssignmentRow[] = []): NetClassIssue[] {
  const issues: NetClassIssue[] = [];
  const all = [defaultClass, ...classes];
  const seen = new Map<string, number>();
  all.forEach((c, row) => {
    const name = c.name.trim();
    if (name === "") issues.push({ row, field: "name", message: "Netclass must have a name." });
    else if (seen.has(name.toLowerCase())) issues.push({ row, field: "name", message: "Netclass name already in use." });
    seen.set(name.toLowerCase(), row);
    for (const f of SIZE_FIELDS) {
      const v = c[f];
      if (v === undefined) continue;
      if (!Number.isFinite(v)) issues.push({ row, field: f, message: `${SIZE_LABELS[f]} is not a number.` });
      else if (v < 0) issues.push({ row, field: f, message: `${SIZE_LABELS[f]} cannot be negative.` });
      else if (v === 0 && f !== "clearance" && f !== "diff_pair_gap") issues.push({ row, field: f, message: `${SIZE_LABELS[f]} must be larger than zero.` });
      else if (f === "clearance" && v > MAXIMUM_CLEARANCE_UM) issues.push({ row, field: f, message: "Clearance was too large. The most is 500 mm." });
    }
    if (!Number.isInteger(c.priority)) issues.push({ row, field: "priority", message: "Priority must be a whole number." });
    if (c.via_diameter !== undefined && c.via_drill !== undefined && Number.isFinite(c.via_diameter) && Number.isFinite(c.via_drill) && c.via_drill >= c.via_diameter) {
      issues.push({ row, field: "via_drill", message: "The via hole must be smaller than the via, or it leaves no annular ring." });
    }
  });
  if (defaultClass.name.trim() !== "Default") issues.push({ row: 0, field: "name", message: "The default net class is required." });
  rows.forEach((r, i) => {
    if (r.pattern.trim() === "" && r.netclass !== "") issues.push({ row: i, field: "pattern", message: "A pattern cannot be empty." });
  });
  return issues;
}

/** The net names a board uses: every pad's net, and every track, via and zone's. */
export function boardNetNames(board: {
  parts: ReadonlyArray<{ pads?: ReadonlyArray<{ net: string | null }> }>;
  routing: { tracks: ReadonlyArray<{ net: string }>; vias: ReadonlyArray<{ net: string }>; zones: ReadonlyArray<{ net: string }> } | null;
}): string[] {
  const nets = new Set<string>();
  for (const p of board.parts) for (const pad of p.pads ?? []) if (pad.net) nets.add(pad.net);
  for (const group of [board.routing?.tracks, board.routing?.vias, board.routing?.zones]) for (const item of group ?? []) if (item.net) nets.add(item.net);
  return [...nets].sort((a, b) => a.localeCompare(b));
}

// ------------------------------------------------------------------ constraints and mask / paste: ranges

/** A length in µm as the millimetres a message shows (`Value must be between 0 and 25 mm.`). */
export function mmText(um: Um): string {
  const s = (um / 1000).toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
  return s === "" || s === "-0" ? "0" : s;
}

function rangeMessage(v: number, minUm: Um, maxUm: Um): string | null {
  if (!Number.isFinite(v)) return "Value is not a number.";
  return v < minUm || v > maxUm ? `Value must be between ${mmText(minUm)} and ${mmText(maxUm)} mm.` : null;
}

/** One number of the Constraints page and the range `ValidateDesignRules` holds it to (`setting` is the `.kicad_pro` key). */
export interface ConstraintRange {
  key: keyof Constraints;
  setting: string;
  min: Um;
  max: Um;
}

export const CONSTRAINT_RANGES: readonly ConstraintRange[] = [
  { key: "min_clearance_um", setting: "min_clearance", min: 0, max: 25_000 },
  { key: "min_connection_um", setting: "min_connection", min: 0, max: 100_000 },
  { key: "min_track_width_um", setting: "min_track_width", min: 0, max: 25_000 },
  { key: "min_annular_width_um", setting: "min_via_annular_width", min: 0, max: 25_000 },
  { key: "min_via_diameter_um", setting: "min_via_diameter", min: 0, max: 25_000 },
  { key: "min_through_hole_um", setting: "min_through_hole_diameter", min: 0, max: 25_000 },
  { key: "min_microvia_diameter_um", setting: "min_microvia_diameter", min: 0, max: 10_000 },
  { key: "min_microvia_drill_um", setting: "min_microvia_drill", min: 0, max: 10_000 },
  { key: "min_hole_to_hole_um", setting: "min_hole_to_hole", min: 0, max: 10_000 },
  { key: "min_hole_clearance_um", setting: "min_hole_clearance", min: 0, max: 100_000 },
  { key: "min_silk_clearance_um", setting: "min_silk_clearance", min: -10_000, max: 100_000 },
  { key: "min_groove_width_um", setting: "min_groove_width", min: 0, max: 25_000 },
  { key: "min_silk_text_height_um", setting: "min_text_height", min: 0, max: 100_000 },
  { key: "min_silk_text_thickness_um", setting: "min_text_thickness", min: 0, max: 25_000 },
  // -0.01 mm is the flag for the pre-6.0 way of measuring the edge; a whole micrometre cannot hold it.
  { key: "min_copper_edge_clearance_um", setting: "min_copper_edge_clearance", min: -10, max: 25_000 },
  { key: "max_error_um", setting: "max_error", min: 1, max: 1_000 },
];

/** `BOARD_DESIGN_SETTINGS::ValidateDesignRules`: the message for each number of the Constraints page that is out of range. */
export function validateConstraints(c: Constraints): Partial<Record<keyof Constraints, string>> {
  const out: Partial<Record<keyof Constraints, string>> = {};
  for (const r of CONSTRAINT_RANGES) {
    const m = rangeMessage(c[r.key] as number, r.min, r.max);
    if (m) out[r.key] = m;
  }
  if (!Number.isInteger(c.min_resolved_spokes) || c.min_resolved_spokes < 0 || c.min_resolved_spokes > 99) out.min_resolved_spokes = "Value must be a whole number between 0 and 99.";
  return out;
}

/** Solder Mask/Paste's ranges. The paste ratio is checked as the fraction it is stored as (-1 .. 1; the page shows percent). */
export function validateMaskPaste(m: MaskPaste): Partial<Record<keyof MaskPaste, string>> {
  const out: Partial<Record<keyof MaskPaste, string>> = {};
  const check = (key: keyof MaskPaste, minUm: Um, maxUm: Um) => {
    const msg = rangeMessage(m[key] as number, minUm, maxUm);
    if (msg) out[key] = msg;
  };
  check("expansion_um", -25_000, 25_000);
  check("min_width_um", 0, 25_000);
  check("to_copper_clearance_um", 0, 25_000);
  check("paste_margin_um", -25_000, 25_000);
  if (!Number.isFinite(m.paste_margin_ratio) || m.paste_margin_ratio < -1 || m.paste_margin_ratio > 1) out.paste_margin_ratio = "Value must be between -100 and 100 %.";
  return out;
}

// ------------------------------------------------------------------ text & graphics

export type TextRowId = "silk" | "copper" | "edge_cuts" | "courtyard" | "fab" | "others";

/** The rows of the Defaults grid in KiCad's order (`ROW_SILK` .. `ROW_OTHERS`); the board outline and the courtyards have a line thickness only. */
export const TEXT_GRAPHICS_ROWS: ReadonlyArray<{ id: TextRowId; label: string; hasText: boolean }> = [
  { id: "silk", label: "Silk Layers", hasText: true },
  { id: "copper", label: "Copper Layers", hasText: true },
  { id: "edge_cuts", label: "Edge Cuts", hasText: false },
  { id: "courtyard", label: "Courtyards", hasText: false },
  { id: "fab", label: "Fab Layers", hasText: true },
  { id: "others", label: "Other Layers", hasText: true },
];

/** A row of the grid as the page edits it: the line thickness, and for the rows that carry text, the text. */
export function textRowOf(t: TextGraphicsDefaults, id: TextRowId): LayerClassDefaults | { line_width_um: Um } {
  switch (id) {
    case "silk":
      return t.silk;
    case "copper":
      return t.copper;
    case "fab":
      return t.fab;
    case "others":
      return t.others;
    case "edge_cuts":
      return { line_width_um: t.edge_cuts_line_width_um };
    case "courtyard":
      return { line_width_um: t.courtyard_line_width_um };
  }
}

/** The grid with one row's line thickness replaced. */
export function withLineWidth(t: TextGraphicsDefaults, id: TextRowId, um: Um): TextGraphicsDefaults {
  switch (id) {
    case "edge_cuts":
      return { ...t, edge_cuts_line_width_um: um };
    case "courtyard":
      return { ...t, courtyard_line_width_um: um };
    default:
      return { ...t, [id]: { ...t[id], line_width_um: um } };
  }
}

/** The grid with one text row's text fields replaced. */
export function withTextFields(t: TextGraphicsDefaults, id: "silk" | "copper" | "fab" | "others", patch: Partial<LayerClassDefaults>): TextGraphicsDefaults {
  return { ...t, [id]: { ...t[id], ...patch } };
}

/**
 * `PANEL_SETUP_TEXT_AND_GRAPHICS::TransferDataFromWindow`'s checks, one message per row: a line thickness of 0.005 to 100 mm, a text size of
 * 0.001 to 250 mm, a text thickness of at least 0.005 mm and, to stay legible, at most a quarter of the smaller text dimension (and 100 mm).
 */
export function validateTextGraphics(t: TextGraphicsDefaults): Partial<Record<TextRowId, string>> {
  const out: Partial<Record<TextRowId, string>> = {};
  const line = (id: TextRowId, w: Um) => {
    if (!Number.isFinite(w) || w < 5 || w > 100_000) out[id] ??= "Incorrect line width. It must be between 0.005 and 100 mm.";
  };
  const text = (id: TextRowId, c: LayerClassDefaults) => {
    line(id, c.line_width_um);
    const size = (v: Um) => Number.isFinite(v) && v >= 1 && v <= 250_000;
    if (!size(c.text_width_um) || !size(c.text_height_um)) {
      out[id] ??= "Text size is incorrect. Size must be between 0.001 and 250 mm.";
    } else if (!Number.isFinite(c.text_thickness_um)) {
      out[id] ??= "Text thickness is not a number.";
    } else {
      const max = Math.min(Math.floor(Math.min(c.text_width_um, c.text_height_um) / 4), 100_000);
      if (c.text_thickness_um > max) out[id] ??= `Text thickness is too large. It must be at most ${mmText(max)} mm.`;
      else if (c.text_thickness_um < 5) out[id] ??= "Text thickness is too small. It must be at least 0.005 mm.";
    }
  };
  text("silk", t.silk);
  text("copper", t.copper);
  line("edge_cuts", t.edge_cuts_line_width_um);
  line("courtyard", t.courtyard_line_width_um);
  text("fab", t.fab);
  text("others", t.others);
  return out;
}

// ------------------------------------------------------------------ physical stackup

const COPPER_MM = 0.035;
const MASK_MM = 0.01;

/** Copper layer names in stack order for an n-layer board: `F.Cu`, `In1.Cu` .. `In{n-2}.Cu`, `B.Cu`. */
export function copperLayerNames(n: number): string[] {
  const count = Math.max(n, 2);
  const names = ["F.Cu"];
  for (let i = 1; i < count - 1; i++) names.push(`In${i}.Cu`);
  names.push("B.Cu");
  return names;
}

/**
 * `BOARD_STACKUP::BuildDefaultStackupList` for a board of `copperLayers` and `boardThicknessUm`: silkscreen, paste and mask on each face, the
 * copper layers with a dielectric (alternating core and prepreg, FR4) between each pair, the dielectrics sharing what is left of the thickness
 * after the copper (0.035 mm a layer) and the two masks (0.01 mm each).
 */
export function defaultStackup(copperLayers: number, boardThicknessUm: Um): Stackup {
  const n = Math.max(copperLayers, 2);
  const boardMm = boardThicknessUm / 1000;
  const dielectricMm = Math.max(0, (boardMm - COPPER_MM * n - MASK_MM * 2) / Math.max(n - 1, 1));
  const layer = (name: string, kind: string, material: string | null, thickness: number | null, epsilon?: number, tangent?: number): StackupLayer => {
    const l: StackupLayer = { name, material, thickness_mm: thickness, kind };
    if (epsilon !== undefined) l.epsilon_r = epsilon;
    if (tangent !== undefined) l.loss_tangent = tangent;
    return l;
  };
  const layers: StackupLayer[] = [layer("F.SilkS", "Top Silk Screen", null, null), layer("F.Paste", "Top Solder Paste", null, null), layer("F.Mask", "Top Solder Mask", null, MASK_MM)];
  copperLayerNames(n).forEach((name, i) => {
    layers.push(layer(name, "copper", null, COPPER_MM));
    if (i + 1 < n) layers.push(layer(`dielectric ${i + 1}`, i % 2 === 0 ? "core" : "prepreg", "FR4", dielectricMm, 4.5, 0.02));
  });
  layers.push(layer("B.Mask", "Bottom Solder Mask", null, MASK_MM), layer("B.Paste", "Bottom Solder Paste", null, null), layer("B.SilkS", "Bottom Silk Screen", null, null));
  return { layers };
}

/** `BuildBoardThicknessFromStackup`: every layer's thickness, in µm. */
export function stackupThicknessUm(s: Stackup): Um {
  return Math.round(s.layers.reduce((sum, l) => sum + (l.thickness_mm ?? 0), 0) * 1000);
}

/** The default stackup for `copperLayers` and `boardThicknessUm`, as the settings the page edits. */
export function newStackupSettings(copperLayers: number, boardThicknessUm: Um): StackupSettings {
  return { copper_layers: Math.min(Math.max(copperLayers, 2), 32), stackup: defaultStackup(copperLayers, boardThicknessUm) };
}

/**
 * The stackup for a different copper layer count, keeping what this one says about every layer the new one still has
 * (`BOARD_STACKUP::SynchronizeWithBoard`): a layer with the same name and type keeps its thickness and material; a new one gets the default.
 * The total thickness is kept, and the finish and edge options.
 */
export function withCopperLayers(settings: StackupSettings, copperLayers: number): StackupSettings {
  const next = newStackupSettings(copperLayers, Math.max(stackupThicknessUm(settings.stackup), 1));
  next.stackup.layers = next.stackup.layers.map((l) => {
    const old = settings.stackup.layers.find((o) => o.name === l.name && o.kind === l.kind);
    return old ? { ...old } : l;
  });
  if (settings.stackup.copper_finish !== undefined) next.stackup.copper_finish = settings.stackup.copper_finish;
  if (settings.stackup.dielectric_constraints) next.stackup.dielectric_constraints = true;
  if (settings.stackup.edge_connector) next.stackup.edge_connector = settings.stackup.edge_connector;
  if (settings.stackup.edge_plating) next.stackup.edge_plating = true;
  return next;
}

/** A stackup written before the stackup was editable has no type on its layers: the copper layers (by name) become `copper`, every other layer is left as it is. */
export function withKnownKinds(s: Stackup): Stackup {
  const copper = new Set(copperLayerNames(32));
  return { ...s, layers: s.layers.map((l) => (l.kind === undefined && copper.has(l.name) ? { ...l, kind: "copper" } : l)) };
}

/**
 * The copper layers a board of `copperLayers` would lose that still carry something: a track, a via end or a zone on them. KiCad asks, then deletes
 * the items; the backend refuses instead (an edit that deletes copper is not one to hide in a settings dialog), so the page says so up front.
 */
export function copperLayersLost(
  routing: { tracks: ReadonlyArray<{ layer: string }>; vias: ReadonlyArray<{ from: string; to: string }>; zones: ReadonlyArray<{ layer: string }> } | null,
  copperLayers: number
): Array<{ layer: string; items: number }> {
  const keep = new Set(copperLayerNames(copperLayers));
  const used = new Map<string, number>();
  const count = (layer: string) => {
    if (/^(F|B|In\d+)\.Cu$/.test(layer) && !keep.has(layer)) used.set(layer, (used.get(layer) ?? 0) + 1);
  };
  for (const t of routing?.tracks ?? []) count(t.layer);
  for (const z of routing?.zones ?? []) count(z.layer);
  for (const v of routing?.vias ?? []) {
    count(v.from);
    count(v.to);
  }
  return [...used].map(([layer, items]) => ({ layer, items })).sort((a, b) => strNumCmp(a.layer, b.layer));
}

/** `IsThicknessEditable`: copper, a dielectric and a solder mask have a thickness; silkscreen and paste do not. */
export function isThicknessEditable(kind: string | undefined): boolean {
  return kind === "copper" || kind === "core" || kind === "prepreg" || kind === "Top Solder Mask" || kind === "Bottom Solder Mask";
}

/** `IsMaterialEditable`: a dielectric, a solder mask and a silkscreen have a material. */
export function isMaterialEditable(kind: string | undefined): boolean {
  return kind === "core" || kind === "prepreg" || (kind !== undefined && /Solder Mask|Silk Screen/.test(kind));
}

/** Whether a layer is a dielectric (the only kind with a dielectric constant and a loss tangent). */
export function isDielectric(kind: string | undefined): boolean {
  return kind === "core" || kind === "prepreg";
}

/** A stackup page the model can hold: an even copper count from 2 to 32 whose copper layers are the ones that count names, in order, and no negative thickness. */
export function validateStackup(s: StackupSettings): string | null {
  const n = s.copper_layers;
  if (!Number.isInteger(n) || n < 2 || n > 32 || n % 2 !== 0) return `A board has 2, 4, 6 .. 32 copper layers, not ${n}.`;
  const copper = s.stackup.layers.filter((l) => l.kind === "copper").map((l) => l.name);
  const expect = copperLayerNames(n);
  if (copper.length !== expect.length || copper.some((name, i) => name !== expect[i])) return `The stackup's copper layers are not the ${n} layers ${expect.join(", ")}.`;
  const bad = s.stackup.layers.find((l) => l.thickness_mm !== null && (!Number.isFinite(l.thickness_mm) || l.thickness_mm < 0));
  return bad ? `Layer ${bad.name} has a negative or invalid thickness.` : null;
}

// ------------------------------------------------------------------ violation severity

/** A check of the Violation Severity page: the `.kicad_pro` key (`rule_severities`) and the severity KiCad starts it at. */
export interface SeverityItem {
  key: string;
  defaultSeverity: RuleSeverity;
}

/** The severity a check has: the board's table when it names the check, else KiCad's default. */
export function severityOf(item: SeverityItem, table: Readonly<Record<string, RuleSeverity>>): RuleSeverity {
  return table[item.key] ?? item.defaultSeverity;
}

/**
 * What `set_rule_severities` is sent for the page as it stands: the checks whose chosen severity differs from KiCad's default, and the ones the
 * board's table already names even at the default (this app fixes two library-link checks to ignore unless a table entry says otherwise,
 * so going back to the default must be said, not left out).
 */
export function severitiesToSend(items: readonly SeverityItem[], chosen: Readonly<Record<string, RuleSeverity>>, current: Readonly<Record<string, RuleSeverity>>): Record<string, RuleSeverity> {
  const out: Record<string, RuleSeverity> = {};
  for (const item of items) {
    const sev = chosen[item.key] ?? current[item.key] ?? item.defaultSeverity;
    if (sev !== item.defaultSeverity || item.key in current) out[item.key] = sev;
  }
  return out;
}
