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
//   pcbnew/board_design_settings.cpp                 ValidateDesignRules (the ranges of the Constraints page)
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

import type { Cmd, Constraints, LayerClassDefaults, NetClass, NetClassSettings, RuleSeverity, Stackup, StackupLayer, StackupSettings, TextGraphicsDefaults, Nm } from "../api/types";
import { strNumCmp } from "./sheetPages";
import { globMatch, patternMatches } from "./netPattern";

export { globMatch };

// ------------------------------------------------------------------ net class patterns


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
 * `SetNetclassPatternAssignment` / `addSinglePatternAssignment`: assigning `pattern` (trimmed, as typed: KiCad reads `a|b|c` and `prefix(x|y)` as regular expressions, so they are patterns
 * like any other) to `netclass` adds a row, none for a pattern already assigned to that class. A pattern already assigned to another class stays there: KiCad keeps both, and the nets
 * the pattern matches are in both classes, whose settings it composes (`GetEffectiveNetClass`).
 */
export function addAssignment(rows: readonly AssignmentRow[], pattern: string, netclass: string): AssignmentRow[] {
  const p = pattern.trim();
  if (p === "" || rows.some((r) => r.pattern === p && r.netclass === netclass)) return [...rows];
  return [...rows, { pattern: p, netclass }];
}

/**
 * `DIALOG_ASSIGN_NETCLASS::TransferDataFromWindow`: the Net Classes page after `pattern` is assigned to `netclass`. Assigning to the Default
 * class takes the pattern off the other classes (every net no pattern claims is Default's; KiCad would keep a row that names Default, here the
 * Default class holds no patterns). An empty pattern assigns nothing.
 */
export function withAssignment(settings: NetClassSettings, pattern: string, netclass: string): NetClassSettings {
  const p = pattern.trim();
  if (p === "") return settings;
  const rows = assignmentRows(settings.classes);
  const next = netclass === settings.default.name ? rows.filter((r) => r.pattern !== p) : addAssignment(rows, pattern, netclass);
  return { default: settings.default, classes: applyAssignments(settings.classes, next) };
}

/** `NETINFO_ITEM::HasAutoGeneratedNetname`: a net KiCad named itself (`Net-(R1-Pad1)`, `unconnected-(U1-NC)`), judged by the part of the name after the last slash. */
export function hasAutoGeneratedNetname(name: string): boolean {
  const short = name.slice(name.lastIndexOf("/") + 1);
  return short.startsWith("Net-(") || short.startsWith("unconnected-(");
}

/** The Net Classes page's starting point from what the board reports: the Default class (rebuilt from the board's sizes when the backend does not send it) and the other classes. */
export function netClassSettingsOf(rules: { default_class?: NetClass; track_width: Nm; clearance: Nm; via_diameter: Nm; via_drill: Nm; net_classes: NetClass[] }): NetClassSettings {
  const def: NetClass = rules.default_class ?? { name: "Default", nets: [], track_width: rules.track_width, clearance: rules.clearance, via_diameter: rules.via_diameter, via_drill: rules.via_drill, priority: 0 };
  return { default: def, classes: rules.net_classes };
}

/** The classes with their patterns taken from the assignment table. A row naming no class, or with nothing in its pattern, is dropped. */
export function applyAssignments(classes: readonly NetClass[], rows: readonly AssignmentRow[]): NetClass[] {
  return classes.map((c) => ({ ...c, nets: rows.filter((r) => r.netclass === c.name).map((r) => r.pattern.trim()).filter((p) => p !== "") }));
}

/** The nets, among `nets`, that `pattern` assigns -- the "Nets matching" list (`EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`: a wildcard pattern or a regular expression, the whole name). */
export function netsMatching(pattern: string, nets: readonly string[]): string[] {
  const p = pattern.trim();
  return p === "" ? [] : nets.filter((n) => patternMatches(p, n));
}

/** The class that owns `net`: the first class, in order, with a pattern that matches it, else the Default class. */
export function classOfNet(classes: readonly NetClass[], net: string, defaultName = "Default"): string {
  for (const c of classes) if (c.nets.some((p) => patternMatches(p, net))) return c.name;
  return defaultName;
}

/** Every class `net` is in, the one that takes precedence first (a class's place in the list is its priority). */
export function classesOfNet(classes: readonly NetClass[], net: string): NetClass[] {
  return classes.filter((c) => c.nets.some((p) => patternMatches(p, net)));
}

/**
 * `NET_SETTINGS::GetEffectiveNetClass`' name for a net: the classes it is in, highest priority first, joined by a comma, with `Default` last when some setting of the net comes from it (a class
 * that sets all of clearance, track width, via size and hole, uVia size and hole and the pair's width and gap needs none). A net in no class is in Default alone.
 */
export function effectiveClassName(classes: readonly NetClass[], net: string, defaultName = "Default"): string {
  const own = classesOfNet(classes, net);
  const sets = (f: (c: NetClass) => number | undefined) => own.some((c) => f(c) !== undefined);
  const complete = own.length > 0 && sets((c) => c.clearance) && sets((c) => c.track_width) && sets((c) => c.via_diameter) && sets((c) => c.via_drill) && sets((c) => c.microvia_diameter) && sets((c) => c.microvia_drill) && sets((c) => c.diff_pair_width) && sets((c) => c.diff_pair_gap);
  return [...own.map((c) => c.name), ...(complete ? [] : [defaultName])].join(",");
}

/** `MAXIMUM_CLEARANCE` (board_design_settings.h): a larger clearance overflows the integer maths in KiCad's engines. */
export const MAXIMUM_CLEARANCE_NM: Nm = 500_000_000;

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
      else if (f === "clearance" && v > MAXIMUM_CLEARANCE_NM) issues.push({ row, field: f, message: "Clearance was too large. The most is 500 mm." });
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

/** A length in µm as the millimetres a message shows (`Value must be between 0 mm and 25 mm.`). */
export function mmText(um: Nm): string {
  const s = (um / 1e6).toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
  return s === "" || s === "-0" ? "0" : s;
}

/** `addRangeError`: each bound is `StringFromValue( .., true )`, which puts the unit on both. */
function rangeMessage(v: number, minNm: Nm, maxNm: Nm): string | null {
  if (!Number.isFinite(v)) return "Value is not a number.";
  return v < minNm || v > maxNm ? `Value must be between ${mmText(minNm)} mm and ${mmText(maxNm)} mm.` : null;
}

/** One number of the Constraints page and the range `ValidateDesignRules` holds it to (`setting` is the `.kicad_pro` key). */
export interface ConstraintRange {
  key: keyof Constraints;
  setting: string;
  min: Nm;
  max: Nm;
}

export const CONSTRAINT_RANGES: readonly ConstraintRange[] = [
  { key: "min_clearance_nm", setting: "min_clearance", min: 0, max: 25_000_000 },
  { key: "min_connection_nm", setting: "min_connection", min: 0, max: 100_000_000 },
  { key: "min_track_width_nm", setting: "min_track_width", min: 0, max: 25_000_000 },
  { key: "min_annular_width_nm", setting: "min_via_annular_width", min: 0, max: 25_000_000 },
  { key: "min_via_diameter_nm", setting: "min_via_diameter", min: 0, max: 25_000_000 },
  { key: "min_through_hole_nm", setting: "min_through_hole_diameter", min: 0, max: 25_000_000 },
  { key: "min_microvia_diameter_nm", setting: "min_microvia_diameter", min: 0, max: 10_000_000 },
  { key: "min_microvia_drill_nm", setting: "min_microvia_drill", min: 0, max: 10_000_000 },
  { key: "min_hole_to_hole_nm", setting: "min_hole_to_hole", min: 0, max: 10_000_000 },
  { key: "min_hole_clearance_nm", setting: "min_hole_clearance", min: 0, max: 100_000_000 },
  { key: "min_silk_clearance_nm", setting: "min_silk_clearance", min: -10_000_000, max: 100_000_000 },
  { key: "min_groove_width_nm", setting: "min_groove_width", min: 0, max: 25_000_000 },
  { key: "min_silk_text_height_nm", setting: "min_text_height", min: 0, max: 100_000_000 },
  { key: "min_silk_text_thickness_nm", setting: "min_text_thickness", min: 0, max: 25_000_000 },
  // -0.01 mm is the flag for the pre-6.0 way of measuring the edge (-10_000 nm).
  { key: "min_copper_edge_clearance_nm", setting: "min_copper_edge_clearance", min: -10_000, max: 25_000_000 },
  // `MAXIMUM_ERROR_SIZE_MM` (0.1 mm), the bound the dialog checks -- not the 1 mm of the `rules.max_error` load range.
  { key: "max_error_nm", setting: "max_error", min: 1_000, max: 100_000 },
];

/** `BOARD_DESIGN_SETTINGS::ValidateDesignRules`: the message for each number of the Constraints page that is out of range. */
export function validateConstraints(c: Constraints): Partial<Record<keyof Constraints, string>> {
  const out: Partial<Record<keyof Constraints, string>> = {};
  for (const r of CONSTRAINT_RANGES) {
    const m = rangeMessage(c[r.key] as number, r.min, r.max);
    if (m) out[r.key] = m;
  }
  if (!Number.isInteger(c.min_resolved_spokes) || c.min_resolved_spokes < 0 || c.min_resolved_spokes > 99) out.min_resolved_spokes = "Value must be between 0 and 99.";
  return out;
}

// Solder Mask/Paste has no range checks here: `PANEL_SETUP_MASK_AND_PASTE::TransferDataFromWindow` validates nothing, it stores the
// values and returns true. (The mask and paste ranges `ValidateDesignRules` holds belong to the Constraints page, which has no field for them.)

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
export function textRowOf(t: TextGraphicsDefaults, id: TextRowId): LayerClassDefaults | { line_width_nm: Nm } {
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
      return { line_width_nm: t.edge_cuts_line_width_nm };
    case "courtyard":
      return { line_width_nm: t.courtyard_line_width_nm };
  }
}

/** The grid with one row's line thickness replaced. */
export function withLineWidth(t: TextGraphicsDefaults, id: TextRowId, um: Nm): TextGraphicsDefaults {
  switch (id) {
    case "edge_cuts":
      return { ...t, edge_cuts_line_width_nm: um };
    case "courtyard":
      return { ...t, courtyard_line_width_nm: um };
    default:
      return { ...t, [id]: { ...t[id], line_width_nm: um } };
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
  const line = (id: TextRowId, w: Nm) => {
    if (!Number.isFinite(w) || w < 5000 || w > 100_000_000) out[id] ??= "Incorrect line width. It must be between 0.005 and 100 mm.";
  };
  const text = (id: TextRowId, c: LayerClassDefaults) => {
    line(id, c.line_width_nm);
    const size = (v: Nm) => Number.isFinite(v) && v >= 1000 && v <= 250_000_000;
    if (!size(c.text_width_nm) || !size(c.text_height_nm)) {
      out[id] ??= "Text size is incorrect. Size must be between 0.001 and 250 mm.";
    } else if (!Number.isFinite(c.text_thickness_nm)) {
      out[id] ??= "Text thickness is not a number.";
    } else {
      const max = Math.min(Math.floor(Math.min(c.text_width_nm, c.text_height_nm) / 4), 100_000_000);
      if (c.text_thickness_nm > max) out[id] ??= `Text thickness is too large. It must be at most ${mmText(max)} mm.`;
      else if (c.text_thickness_nm < 5000) out[id] ??= "Text thickness is too small. It must be at least 0.005 mm.";
    }
  };
  text("silk", t.silk);
  text("copper", t.copper);
  line("edge_cuts", t.edge_cuts_line_width_nm);
  line("courtyard", t.courtyard_line_width_nm);
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
 * `BOARD_STACKUP::BuildDefaultStackupList` for a board of `copperLayers` and `boardThicknessNm`: silkscreen, paste and mask on each face, the
 * copper layers with a dielectric (alternating core and prepreg, FR4) between each pair, the dielectrics sharing what is left of the thickness
 * after the copper (0.035 mm a layer) and the two masks (0.01 mm each).
 */
export function defaultStackup(copperLayers: number, boardThicknessNm: Nm): Stackup {
  const n = Math.max(copperLayers, 2);
  const boardMm = boardThicknessNm / 1e6;
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
export function stackupThicknessNm(s: Stackup): Nm {
  return Math.round(s.layers.reduce((sum, l) => sum + (l.thickness_mm ?? 0), 0) * 1e6);
}

/** The default stackup for `copperLayers` and `boardThicknessNm`, as the settings the page edits. */
export function newStackupSettings(copperLayers: number, boardThicknessNm: Nm): StackupSettings {
  return { copper_layers: Math.min(Math.max(copperLayers, 2), 32), stackup: defaultStackup(copperLayers, boardThicknessNm) };
}

/**
 * The stackup for a different copper layer count, keeping what this one says about every layer the new one still has
 * (`BOARD_STACKUP::SynchronizeWithBoard`): a layer with the same name and type keeps its thickness and material; a new one gets the default.
 * The total thickness is kept, and the finish and edge options.
 */
export function withCopperLayers(settings: StackupSettings, copperLayers: number): StackupSettings {
  const next = newStackupSettings(copperLayers, Math.max(stackupThicknessNm(settings.stackup), 1));
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

/**
 * Pre-defined Sizes' Apply: the track widths and the via sizes as one command (`batch`), so the page is one undo step, as every other page of the
 * dialog is. (KiCad's Board Setup applies `BOARD_DESIGN_SETTINGS` outside the undo list; here every edit is an undoable verb, one per Apply.)
 */
export function predefinedSizesCmds(widths: readonly Nm[], vias: ReadonlyArray<{ diameter: Nm; drill: Nm }>): Cmd[] {
  return [{ op: "batch", cmds: [{ op: "set_track_width_presets", widths: [...widths] }, { op: "set_via_presets", presets: vias.map((v) => ({ ...v })) }] }];
}
