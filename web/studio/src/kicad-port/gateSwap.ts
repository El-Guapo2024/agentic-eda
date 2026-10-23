// Swap Gate Nets' reading of a selection (pcbnew/tools/edit_tool.cpp `GATE_SWAP_MENU`, the conditions `gateSwapMultipleUnitsOnOneFootprint` and
// `gateSwapSingleUnitOnOneFootprint`): the selected pads belong to one footprint that has units (`FOOTPRINT::GetUnitInfo`, two at least), and the units they are in are
// the gates to swap. Two or more such units with as many pins as each other are swapped with each other (the verb rotates the nets of their pads, position by position);
// one unit is swapped with one of the other units that has as many pins, which the "Swap with <unit>" entries offer. Pure: no store, no DOM.
import type { BoardState } from "../api/types";
import { padById } from "./pcbItems";

export interface GateUnit {
  name: string;
  pins: readonly string[];
}

/** What the context menu needs to know of a selection of pads. */
export interface GateSwapChoice {
  /** The one footprint (with units) the selected pads belong to, or null (`GetSingleEligibleFootprint`). */
  footprint: string | null;
  /** The units that have a pad selected, in unit order (`GetUnitsHitIndices`). */
  hit: number[];
  /** `gateSwapMultipleUnitsOnOneFootprint`: two units or more are hit and they have as many pins as each other. */
  multiple: boolean;
  /** `gateSwapSingleUnitOnOneFootprint`'s targets: with exactly one unit hit, the names of the other units that have as many pins (`GetCompatibleTargets`), else null. */
  targets: string[] | null;
}

const none: GateSwapChoice = { footprint: null, hit: [], multiple: false, targets: null };

/** The units of the footprint `ref`, when it has two or more. */
function unitsOf(board: BoardState, ref: string): readonly GateUnit[] {
  const units = board.parts.find((p) => p.placed && p.ref === ref)?.units;
  return units && units.length >= 2 ? units : [];
}

/** `GetSingleEligibleFootprint` and what follows from it, for the pads `ids` name (anything else in the selection is ignored, as the C++ ignores a non-pad). */
export function gateSwapChoice(board: BoardState, ids: readonly string[]): GateSwapChoice {
  let footprint: string | null = null;
  const numbers = new Set<string>();
  for (const id of ids) {
    const hit = padById(board, id);
    if (!hit) continue;
    const units = unitsOf(board, hit.part.ref);
    // A footprint with fewer than two units, or a pad that is in none of them, does not count.
    if (units.length < 2 || !units.some((u) => u.pins.includes(hit.pad.num))) continue;
    if (footprint === null) footprint = hit.part.ref;
    else if (footprint !== hit.part.ref) return none;
    numbers.add(hit.pad.num);
  }
  if (footprint === null) return none;
  const units = unitsOf(board, footprint);
  const hit = units.map((u, i) => (u.pins.some((p) => numbers.has(p)) ? i : -1)).filter((i) => i >= 0);
  const multiple = hit.length >= 2 && hit.every((i) => units[i]!.pins.length === units[hit[0]!]!.pins.length);
  let targets: string[] | null = null;
  if (hit.length === 1) {
    const source = hit[0]!;
    targets = units.filter((u, i) => i !== source && u.pins.length === units[source]!.pins.length).map((u) => u.name);
  }
  return { footprint, hit, multiple, targets };
}

/** The pads of a selection in the order it was made, when it is only pads (`selection.OnlyContains( { PCB_PAD_T } )`); null otherwise. */
export function padsOnly(board: BoardState, ids: readonly string[]): string[] | null {
  if (ids.length === 0) return null;
  const pads: string[] = [];
  for (const id of ids) {
    if (!padById(board, id)) return null;
    pads.push(id);
  }
  return pads;
}

/** `PromptConnectedPadDecision`'s list of the unselected pads: `R2:1` for the pad `R2.1` (the footprint's reference, a colon, the pad's number). */
export function unselectedPadLabel(id: string): string {
  const dot = id.indexOf(".");
  if (dot <= 0) return id;
  const hash = id.lastIndexOf("#");
  return `${id.slice(0, dot)}:${hash > dot ? id.slice(dot + 1, hash) : id.slice(dot + 1)}`;
}
