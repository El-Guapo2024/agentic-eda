// The slice of editor state the board-control actions (docs/parity/UI-ACTIONS.md: pcbnew.Control, EditorControl, InspectionTool,
// ZoneFiller) need, kept in one object (`StudioState.bcx`, one `BCX` reducer action) like `pcbParityState.ts`, so the shared store
// carries a few small hunks instead of a field per action.

import type { FilledZones, RatsnestMode } from "./boardControl";

/** The export / output dialogs `BoardExportDialog.tsx` serves (each one is a kicad-cli command, except `cmp`). */
export type BoardExportKind = "3d" | "vrml" | "gencad" | "ipcd356" | "ipc2581" | "odb" | "pcb_bom" | "cmp";

export interface BoardControlState {
  /** `PCB_DISPLAY_OPTIONS::m_DisplayGraphicsFill` off (`pcbnew.Control.graphicOutlines`, "Sketch Graphic Items"): filled graphics drawn as outlines. */
  sketchGraphics: boolean;
  /** `m_DisplayTextFill` off (`pcbnew.Control.textOutlines`, "Sketch Text Items"): text drawn as outlines. */
  sketchText: boolean;
  /** `m_DisplayPadNum` (`pcbnew.Control.showPadNumbers`). */
  showPadNumbers: boolean;
  /** `m_RatsnestMode` (`pcbnew.Control.ratsnestModeCycle`): lines for every copper layer, or only the ones on show. */
  ratsnestMode: RatsnestMode;
  /** `rs->GetHiddenNets()` (`pcbnew.EditorControl.hideNet` / `showNet`). */
  hiddenRatsnestNets: string[];
  /** The rest of a multi-net highlight (`highlightNetSelection`); the first net is `StudioState.netHighlight`. */
  netHighlightMore: string[];
  /** `BOARD_INSPECTION_TOOL::m_lastHighlighted`: what `toggleLastNetHighlight` swaps back. */
  lastHighlight: string[];
  /** Pads whose local-ratsnest flag differs from the global state (`pcbnew.Control.localRatsnestTool`'s clicks), as `REF.NUMBER`. */
  localRatsnestPads: string[];
  /** `BOARD_DESIGN_SETTINGS::m_UseConnectedTrackWidth` (`pcbnew.EditorControl.autoTrackWidth`). */
  autoTrackWidth: boolean;
  /** Which zones have a fill showing: `null` = all of them (see `FilledZones`). */
  zoneFilled: FilledZones;
  /** `PCB_DISPLAY_OPTIONS::m_FlipBoardView` (`pcbnew.Control.flipBoard`). */
  boardFlipped: boolean;
  /** The Zone Manager dialog (`pcbnew.Control.zonesManager`). */
  zoneManagerOpen: boolean;
  /** Which export / output dialog is open. */
  boardExport: BoardExportKind | null;
  /** The footprint whose associations report is open (`pcbnew.InspectionTool.ShowFootprintAssociations`). */
  associationsRef: string | null;
  /** The zone being duplicated: the Zone Properties dialog is open on a copy of it (`pcbnew.EditorControl.zoneDuplicate`). */
  duplicateZoneId: string | null;
}

export const DEFAULT_BOARD_CONTROL: BoardControlState = {
  sketchGraphics: false,
  sketchText: false,
  showPadNumbers: false,
  ratsnestMode: "all",
  hiddenRatsnestNets: [],
  netHighlightMore: [],
  lastHighlight: [],
  localRatsnestPads: [],
  autoTrackWidth: false,
  zoneFilled: null,
  boardFlipped: false,
  zoneManagerOpen: false,
  boardExport: null,
  associationsRef: null,
  duplicateZoneId: null,
};

/**
 * The state after the highlighted nets change from `before` (the first net, plus `bcx.netHighlightMore`) to `nets` (the
 * first net, then the rest): `m_lastHighlighted` takes what was on show whenever the highlight really changes, as
 * `BOARD_INSPECTION_TOOL::HighlightNet` does (`if( !netcodes.empty() ) m_lastHighlighted = netcodes`).
 */
export function withHighlight(bcx: BoardControlState, before: string | null, nets: readonly string[]): BoardControlState {
  const was = before ? [before, ...bcx.netHighlightMore.filter((n) => n !== before)] : [];
  const same = was.length === nets.length && was.every((n, i) => n === nets[i]);
  return { ...bcx, netHighlightMore: nets.slice(1), lastHighlight: same || was.length === 0 ? bcx.lastHighlight : was };
}
