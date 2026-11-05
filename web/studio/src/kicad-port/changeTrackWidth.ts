// Change Track Width / Via Size (`pcbnew.InteractiveEdit.changeTrackWidth`): `EDIT_TOOL::ChangeTrackWidth` (pcbnew/tools/edit_tool.cpp:1383-1454 at
// 8303b2ad), planned as the `/api/cmd` verbs of its one commit ("Edit Track Width/Via Size"). Pure; no React, no DOM.
//
// KiCad asks `BOARD_DESIGN_SETTINGS` for the sizes each time the action runs (round 7 R7-5): a track or an arc gets `GetCurrentTrackWidth()`, a via
// `GetCurrentViaSize()` and `GetCurrentViaDrill()`, and a microvia its effective net class's uVia size and drill. Those read the board's Pre-defined Sizes
// lists by the chosen index now, so a Board Setup edit of the chosen row, or of the Default class while "use net class values" is chosen, is seen -- the
// studio's `currentTrackWidthNm` / `currentViaPreset` are snapshots taken when an entry was picked and are not read here (the defect C6-2 fixed for Draw Via).

import type { BoardState, Cmd, Nm, Via } from "../api/types";
import { chosenTrackWidthOf } from "./routerSizes";
import { currentViaSize, viaSizeFor, type CustomTrackVia } from "./routerVia";

/** What the studio holds of `BOARD_DESIGN_SETTINGS`' size choice: the chosen indexes (0 = the net class's entry) and the custom sizes and whether they are in use. */
export interface SizeChoice {
  trackWidthIndex: number | null;
  viaSizeIndex: number | null;
  useCustomTrackVia: boolean;
  custom: CustomTrackVia;
}

/**
 * `BOARD_DESIGN_SETTINGS::GetCurrentTrackWidth()` (board_design_settings.cpp:1689-1697): the custom width while custom sizes are in use; else the board's
 * list entry at the index, read from the list now (`m_TrackWidthList[ m_trackWidthIndex ]`); at index 0 or past the list, the Default class's width
 * (`m_NetSettings->GetDefaultNetclass()->GetTrackWidth()`).
 */
export function currentTrackWidth(board: BoardState, index: number | null, useCustomTrackVia: boolean, customTrackNm: Nm): Nm {
  const chosen = chosenTrackWidthOf(board.routing?.track_width_presets ?? [], index, useCustomTrackVia, customTrackNm);
  if (chosen != null) return chosen;
  const rules = board.board_rules;
  return rules?.default_class?.track_width ?? rules?.track_width ?? 250_000;
}

/**
 * The verbs of `EDIT_TOOL::ChangeTrackWidth` for the tracks and vias `ids` (already filtered: tracks, arcs and vias, locked items out): every track the
 * current track width, every via the current via size and drill, and a microvia (`via->GetViaType() == VIATYPE::MICROVIA`) its effective net class's
 * `GetuViaDiameter()` / `GetuViaDrill()` (edit_tool.cpp:1413-1418). One `edit_tracks_and_vias` holds the tracks and the other vias; the microvias go in
 * one more per size their classes give. Send them as one batch (one undo step, as KiCad's one commit). Empty when `ids` is.
 */
export function changeTrackWidthCmds(board: BoardState, ids: readonly string[], choice: SizeChoice): Cmd[] {
  if (ids.length === 0) return [];
  const width = currentTrackWidth(board, choice.trackWidthIndex, choice.useCustomTrackVia, choice.custom.trackNm);
  const via = currentViaSize(board, choice.viaSizeIndex, choice.useCustomTrackVia, choice.custom);
  const vias = new Map<string, Via>((board.routing?.vias ?? []).map((v) => [v.id, v]));
  const plain: string[] = [];
  const micro = new Map<string, { diameter: Nm; drill: Nm; ids: string[] }>();
  for (const id of ids) {
    const v = vias.get(id);
    if (v?.type !== "micro") {
      plain.push(id);
      continue;
    }
    const size = viaSizeFor(board, v.net, "micro", null);
    const key = `${size.diameter}/${size.drill}`;
    const group = micro.get(key) ?? { ...size, ids: [] };
    group.ids.push(id);
    micro.set(key, group);
  }
  const cmds: Cmd[] = [];
  if (plain.length > 0) cmds.push({ op: "edit_tracks_and_vias", ids: plain, track_width: { kind: "value", nm: width }, via_size: { kind: "value", diameter: via.diameter, drill: via.drill } });
  for (const g of micro.values()) cmds.push({ op: "edit_tracks_and_vias", ids: g.ids, via_size: { kind: "value", diameter: g.diameter, drill: g.drill } });
  return cmds;
}
