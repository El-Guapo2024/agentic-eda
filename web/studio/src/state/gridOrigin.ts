// The Footprint Editor's grid origin. The board's is the design's (`board.grid_origin`, the `set_grid_origin` verb: it is saved in the `.kicad_pcb`); the
// footprint editor has no board to save it in -- KiCad keeps it in that frame's design settings, per window and not in the footprint file -- so here it
// is plain module state with subscribers (like `state/commonOptions.ts`): it lasts as long as the page, and holds across opening another footprint.
//
//   pcbnew/tools/pcb_control.cpp  PCB_CONTROL::DoSetGridOrigin  `aFrame->GetDesignSettings().SetGridOrigin( aPoint )`
import { useSyncExternalStore } from "react";
import { NO_ORIGIN, type Origin } from "../kicad-port/gridOrigin";

let footprint: Origin = NO_ORIGIN;
const listeners = new Set<() => void>();

export function getFootprintGridOrigin(): Origin {
  return footprint;
}

/** `DoSetGridOrigin` in the Footprint Editor; (0, 0) is the reset. */
export function setFootprintGridOrigin(at: Origin): void {
  if (at.x === footprint.x && at.y === footprint.y) return;
  footprint = at.x === 0 && at.y === 0 ? NO_ORIGIN : { x: at.x, y: at.y };
  for (const l of listeners) l();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useFootprintGridOrigin(): Origin {
  return useSyncExternalStore(subscribe, getFootprintGridOrigin, getFootprintGridOrigin);
}
