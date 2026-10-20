// Pin Helpers: the submenu `PIN_TRICKS_MENU` (eeschema/tools/sch_edit_tool.cpp at 8303b2ad) puts on a selection of pins and sheet pins -- Wire, No Connect, Net Label,
// Hierarchical Label and Global Label -- and what `SCH_SELECTION_TOOL::Main` makes of the new items afterwards (`ID_POPUP_SCH_PIN_TRICKS_*`, sch_selection_tool.cpp).
//
// The work is the `sch_edit` verb `pin_helper` (crates/ops/src/sch_pin_helpers.rs), one undo step. This file is the studio's side of it: which entries the menu has for a
// selection, which of the selected items the verb is given, the command, and what the new items are -- the labels are selected ("Many users will want to drag these items to
// wire off of the pins, so pre-select them"), the no-connects are not (`ClearSelection()`), and the wire stubs are selected with only their far ends picked so that the Drag
// tool can take them at once ("Select only the ends so we can immediately start dragging them"; the studio's drag, `SchematicView`, stretches a wire by the ends picked).
//
// Pure: no React, no DOM (compiled by `npm run test:unit`).
import type { Cmd, Schematic } from "../api/types";
import { isPinItemId } from "./schPins";

export type PinHelperKind = "wire" | "no_connect" | "net_label" | "hier_label" | "global_label";

/** The five entries of the submenu, in the order `PIN_TRICKS_MENU::update` adds them. They are this studio's own actions (`studio.PinHelpers.*`): in KiCad they are menu ids, not tool actions. */
export const PIN_HELPERS: ReadonlyArray<{ kind: PinHelperKind; action: string; label: string }> = [
  { kind: "wire", action: "studio.PinHelpers.wire", label: "Wire" },
  { kind: "no_connect", action: "studio.PinHelpers.noConnect", label: "No Connect" },
  { kind: "net_label", action: "studio.PinHelpers.netLabel", label: "Net Label" },
  { kind: "hier_label", action: "studio.PinHelpers.hierLabel", label: "Hierarchical Label" },
  { kind: "global_label", action: "studio.PinHelpers.globalLabel", label: "Global Label" },
];

/** `SCH_CONDITIONS::AllPinsOrSheetPins`: `aSel.GetSize() >= 1 && aSel.OnlyContains( { SCH_PIN_T, SCH_SHEET_PIN_T } )` -- when the submenu is there at all. */
export const allPinsOrSheetPins = (s: { total: number; pins: number; sheetPins: number }): boolean => s.total >= 1 && s.pins + s.sheetPins === s.total;

/** The ids of the selection a helper acts on, in selection order: its pins (`pin:<reference>:<number>`) and the pins of its sheets (`if( item->Type() != SCH_PIN_T && item->Type() != SCH_SHEET_PIN_T ) continue`). */
export function pinHelperTargets(sch: Pick<Schematic, "sheets">, selection: readonly string[]): string[] {
  const sheetPins = new Set(sch.sheets.flatMap((s) => s.pins.map((p) => p.id).filter((id): id is string => !!id)));
  return [...new Set(selection)].filter((id) => isPinItemId(id) || sheetPins.has(id));
}

/** The command: `kind` on `pins`, a stub being one step of `gridUm`, the wire grid (`GRID_HELPER_GRIDS::GRID_WIRES`). */
export const pinHelperCmd = (kind: PinHelperKind, pins: readonly string[], gridUm: number): Cmd => ({ op: "sch_edit", verb: "pin_helper", what: kind, pins: [...pins], grid: Math.max(0, Math.round(gridUm)) }) as unknown as Cmd;

/** What the helper made, from the sheet before and after it: the ids to select, and for a wire stub the end of it that is not at the pin (to be dragged) as `vertices` of the wire, and where that end is. */
export interface MadeByPinHelper {
  select: string[];
  ends: Record<string, number[]>;
  /** The far end of the first stub: where the Drag tool holds the stubs (`vc->SetCrossHairCursorPosition( first->GetEndPoint() )`). */
  at: [number, number] | null;
}

export function madeByPinHelper(kind: PinHelperKind, before: Pick<Schematic, "wires" | "labels">, after: Pick<Schematic, "wires" | "labels">, pinPoints: ReadonlyArray<readonly [number, number]>): MadeByPinHelper {
  const none: MadeByPinHelper = { select: [], ends: {}, at: null };
  if (kind === "no_connect") return none; // `ClearSelection()`
  if (kind !== "wire") {
    const had = new Set(before.labels.map((l) => l.id));
    return { ...none, select: after.labels.filter((l) => !had.has(l.id)).map((l) => l.id) };
  }
  const had = new Set(before.wires.map((w) => w.id));
  const atPin = (p: readonly [number, number]) => pinPoints.some((q) => q[0] === p[0] && q[1] === p[1]);
  const made = { ...none, ends: {} as Record<string, number[]> };
  for (const w of after.wires) {
    if (had.has(w.id) || w.pts.length < 2) continue;
    // a stub starts at its pin (the verb draws it from the pin): the other end is the one that is dragged; one the cleanup turned round is read the other way
    const last = w.pts.length - 1;
    const far = atPin(w.pts[0]!) ? last : atPin(w.pts[last]!) ? 0 : last;
    made.select.push(w.id);
    made.ends[w.id] = [far];
    if (!made.at) made.at = [w.pts[far]![0], w.pts[far]![1]];
  }
  return made;
}
