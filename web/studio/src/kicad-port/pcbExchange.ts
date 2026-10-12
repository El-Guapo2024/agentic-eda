// Change Footprint(s) and Update Footprint(s) from Library (`pcbnew.GlobalEdit.changeFootprint(s)` / `updateFootprint(s)`, GLOBAL_EDIT_TOOL::ExchangeFootprints ->
// DIALOG_EXCHANGE_FOOTPRINTS, pcbnew/dialogs/dialog_exchange_footprints.cpp at 8303b2ad): the dialog's options with the values it opens with, which footprints
// a choice names, and the report lines of `processFootprint`. The exchange itself is `exchange_footprints` (crates/ops/src/fp_exchange.rs).
//
// Pure functions over the studio's board state, units micrometres; the commands they build go to the server as one undo step.

import type { BoardState, Cmd, ExchangeOptions } from "../api/types";
import type { ExchangePlanEntry } from "../api/fpAssign";
import { wildMatch } from "./pcbGlobalEdit";

/** What `BOARD_NETLIST_UPDATER::replaceFootprint` asks of `ExchangeFootprint`: every option on (the symbol's Footprint field, Update PCB from Schematic). */
export const REPLACE_OPTIONS: ExchangeOptions = {
  match_pad_positions: true,
  remove_extra_texts: true,
  reset_text_layers: true,
  reset_text_effects: true,
  reset_text_positions: true,
  reset_text_content: true,
  reset_fabrication_attrs: true,
  reset_clearance_overrides: true,
  reset_3d_models: true,
};

/** The Change Footprint dialog as it opens (`updateMode` off). */
export const CHANGE_OPTIONS: ExchangeOptions = { ...REPLACE_OPTIONS, remove_extra_texts: false };

/** The Update Footprint dialog as it opens (`updateMode` on): the texts and the attributes of the board stay; the library's overrides and models are taken. */
export const UPDATE_OPTIONS: ExchangeOptions = {
  ...REPLACE_OPTIONS,
  remove_extra_texts: false,
  reset_text_layers: false,
  reset_text_effects: false,
  reset_text_positions: false,
  reset_text_content: false,
  reset_fabrication_attrs: false,
};

/** "Check All Update Options" / "Uncheck All Update Options": every option but the match mode. */
export function setAllOptions(on: boolean): ExchangeOptions {
  return {
    match_pad_positions: on,
    remove_extra_texts: on,
    reset_text_layers: on,
    reset_text_effects: on,
    reset_text_positions: on,
    reset_text_content: on,
    reset_fabrication_attrs: on,
    reset_clearance_overrides: on,
    reset_3d_models: on,
  };
}

/** `DIALOG_EXCHANGE_FOOTPRINTS`' match modes (`ID_MATCH_FP_*`): Change has no "all" row, Update has no restriction. */
export type MatchMode = "all" | "selected" | "reference" | "value" | "footprint";

export interface MatchChoice {
  mode: MatchMode;
  /** `m_specifiedRef`, `m_specifiedValue`, `m_specifiedID`: the patterns the three text rows hold. */
  reference: string;
  value: string;
  footprint: string;
}

/**
 * `isMatch`: the footprints (placed parts) a choice names. `selected` is the selection and, when there is one, the footprint the dialog was opened on
 * (`aFootprint == m_currentFootprint || aFootprint->IsSelected()`). The patterns are `WildCompareString`'s (`*`, `?`, case ignored); a footprint id is compared whole, as
 * `aFootprint->GetFPID() == specifiedID`.
 */
export function matchingFootprints(board: BoardState, choice: MatchChoice, selected: ReadonlySet<string>, current: string | null): string[] {
  const placed = board.parts.filter((p) => p.placed);
  switch (choice.mode) {
    case "all":
      return placed.map((p) => p.ref);
    case "selected":
      return placed.filter((p) => p.ref === current || selected.has(p.ref)).map((p) => p.ref);
    case "reference":
      return placed.filter((p) => wildMatch(choice.reference, p.ref)).map((p) => p.ref);
    case "value":
      return placed.filter((p) => wildMatch(choice.value, p.value ?? "")).map((p) => p.ref);
    case "footprint":
      return placed.filter((p) => (p.footprint ?? "") === choice.footprint.trim()).map((p) => p.ref);
  }
}

/** The command for one dialog run: change mode names the footprint, update mode leaves it out. */
export function exchangeCmd(references: readonly string[], footprint: string | null, options: ExchangeOptions): Cmd {
  return { op: "exchange_footprints", references: [...references], footprint, options };
}

/** A line of the dialog's message window (`WX_HTML_REPORT_PANEL`, `RPT_SEVERITY_*`). */
export interface ReportLine {
  severity: "info" | "action" | "warning" | "error";
  text: string;
}

/** `LIB_ID::IsValid`: a library nickname, a colon and an item name, both non-empty (`m_newID`; a bare name is not a library id). */
export function isLibraryId(id: string): boolean {
  const colon = id.indexOf(":");
  return colon > 0 && colon < id.length - 1;
}

/**
 * The message window after a run (`processFootprint`): one line a footprint. A change tells "Changed footprint R1 from 'A' to 'B': OK"; an update tells
 * "Updated footprint R1 (A): OK", or "(no changes)" when the library copy is the board's; a footprint whose library copy is not found is an error line in either.
 */
export function exchangeReport(mode: "change" | "update", entries: readonly ExchangePlanEntry[]): ReportLine[] {
  return entries.map((e): ReportLine => {
    const head = mode === "change" ? `Changed footprint ${e.reference} from '${e.from}' to '${e.to}'` : `Updated footprint ${e.reference} (${e.from})`;
    if (!e.found) return { severity: "error", text: `${head}: *** library footprint not found ***` };
    if (mode === "update" && e.differs === false) return { severity: "info", text: `${head}: (no changes)` };
    return { severity: "action", text: `${head}: OK` };
  });
}
