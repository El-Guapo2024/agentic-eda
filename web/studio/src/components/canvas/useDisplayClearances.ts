// The clearances the board's clearance outlines are drawn at (UI-LEFT-3 P3-6): `GetOwnClearance( layer )` of every track, via and pad on the active copper
// layer, asked of the server (`POST /api/display/clearances`, api/displayClient.ts) when Display Options wants them -- "Tracks:" "Show always" or "Show pad
// clearance" -- and again whenever the board or the active layer changes. KiCad only ever shows the active copper layer's clearance layer
// (`PCB_EDIT_FRAME::SetActiveLayer`), so one layer's answer is all the painter needs. A custom pad's outline is its copper grown by its clearance
// (`PAD::TransformShapeToPolygon`: `outline.Inflate( aClearance, CORNER_STRATEGY::ROUND_ALL_CORNERS, aMaxError )`), which the page cannot work out: the
// server's polygon arithmetic does it (`POST /api/board3d/polys`, crates/cli/src/board3d_api.rs), and the answer is kept as `customPads`.
import { useEffect, useState } from "react";
import { fetchDisplayClearances } from "../../api/displayClient";
import type { BoardState, DisplayClearances } from "../../api/types";
import { boardShowsTrackClearance, customPadJobs, type TrackClearanceMode } from "../../kicad-port/pcbDisplayOptions";

/** Whether the board's clearance outlines are wanted at all, on `layer` (a KiCad layer name, the active one; null when none). */
export function clearancesWanted(layer: string | null, copper: readonly string[], trackClearance: TrackClearanceMode, padClearance: boolean): boolean {
  return !!layer && copper.includes(layer) && (boardShowsTrackClearance(trackClearance) || padClearance);
}

type Contour = [number, number][];

/** The active copper layer's clearances, refetched for each board `version`; null while not wanted or not answered yet. */
export function useDisplayClearances(layer: string | null, board: BoardState | null | undefined, version: number | string | undefined, trackClearance: TrackClearanceMode, padClearance: boolean): DisplayClearances | null {
  const [reply, setReply] = useState<DisplayClearances | null>(null);
  const wanted = clearancesWanted(layer, board?.layers ?? [], trackClearance, padClearance);
  useEffect(() => {
    if (!wanted || !layer || !board) {
      setReply(null);
      return;
    }
    let live = true;
    void (async () => {
      const r = await fetchDisplayClearances(layer);
      if (!live) return;
      if (!r.ok) return setReply(null);
      const jobs = padClearance ? customPadJobs(board, r) : [];
      if (jobs.length > 0) {
        try {
          const res = await fetch("/api/board3d/polys", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ jobs }) });
          const out = (await res.json()) as { results?: Record<string, { outer: Contour; holes: Contour[] }[]> };
          r.customPads = Object.fromEntries(Object.entries(out.results ?? {}).map(([k, polys]) => [k, polys.flatMap((p) => [p.outer, ...p.holes])]));
        } catch {
          // no grown outline: the pad's own is drawn
        }
      }
      if (live) setReply(r);
    })();
    return () => {
      live = false;
    };
    // the board's version stands for the board
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted, layer, version, padClearance]);
  return reply;
}
