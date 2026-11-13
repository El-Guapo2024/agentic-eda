// The clearances the board's clearance outlines are drawn at (UI-LEFT-3 P3-6): `GetOwnClearance( layer )` of every track, via and pad on the active copper
// layer, asked of the server (`POST /api/display/clearances`, api/displayClient.ts) when Display Options wants them -- "Tracks:" "Show always" or "Show pad
// clearance" -- and again whenever the board or the active layer changes. KiCad only ever shows the active copper layer's clearance layer
// (`PCB_EDIT_FRAME::SetActiveLayer`), so one layer's answer is all the painter needs.
import { useEffect, useState } from "react";
import { fetchDisplayClearances } from "../../api/displayClient";
import type { DisplayClearances } from "../../api/types";
import { boardShowsTrackClearance, type TrackClearanceMode } from "../../kicad-port/pcbDisplayOptions";

/** Whether the board's clearance outlines are wanted at all, on `layer` (a KiCad layer name, the active one; null when none). */
export function clearancesWanted(layer: string | null, copper: readonly string[], trackClearance: TrackClearanceMode, padClearance: boolean): boolean {
  return !!layer && copper.includes(layer) && (boardShowsTrackClearance(trackClearance) || padClearance);
}

/** The active copper layer's clearances, refetched for each board `version`; null while not wanted or not answered yet. */
export function useDisplayClearances(layer: string | null, copper: readonly string[], version: number | string | undefined, trackClearance: TrackClearanceMode, padClearance: boolean): DisplayClearances | null {
  const [reply, setReply] = useState<DisplayClearances | null>(null);
  const wanted = clearancesWanted(layer, copper, trackClearance, padClearance);
  useEffect(() => {
    if (!wanted || !layer) {
      setReply(null);
      return;
    }
    let live = true;
    void fetchDisplayClearances(layer).then((r) => {
      if (live) setReply(r.ok ? r : null);
    });
    return () => {
      live = false;
    };
  }, [wanted, layer, version]);
  return reply;
}
